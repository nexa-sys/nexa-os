#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr;

use nexa_boot_info::{
    bar_flags,
    block_flags,
    device_flags,
    flags,
    network_flags,
    BlockDeviceInfo,
    BootInfo,
    DeviceDescriptor,
    DeviceKind,
    FramebufferInfo,
    MemoryRegion,
    KernelSegment,
    NetworkDeviceInfo,
    PciBarInfo,
    PciDeviceInfo,
    UsbHostInfo,
    HidInputInfo,
    MAX_DEVICE_DESCRIPTORS,
};
use r_efi::base as raw_base;
use r_efi::protocols::pci_io;
use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat};
use uefi::proto::device_path::{DevicePath, DevicePathNodeEnum};
use uefi::proto::media::block::BlockIO;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileMode, RegularFile};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::network::snp::{NetworkState, SimpleNetwork};
use uefi::proto::unsafe_protocol;
use uefi::table::boot::{AllocateType, MemoryType, OpenProtocolAttributes, OpenProtocolParams, SearchType};
use uefi::Identify;
use uefi::Error;
use uefi::{cstr16, Handle, Status};

const KERNEL_PATH: &uefi::CStr16 = cstr16!("\\EFI\\BOOT\\KERNEL.ELF");
const INITRAMFS_PATH: &uefi::CStr16 = cstr16!("\\EFI\\BOOT\\INITRAMFS.CPIO");
const ROOTFS_PATH: &uefi::CStr16 = cstr16!("\\EFI\\BOOT\\ROOTFS.EXT2");
const CMDLINE_PATH: &uefi::CStr16 = cstr16!("\\EFI\\BOOT\\cmdline.txt");
// Rootfs 优先从 ESP 缓存，若缺失则回退到块设备读取
const MAX_PHYS_ADDR: u64 = 0x0000FFFF_FFFF;
const BOOT_INFO_PREF_MAX_ADDR: u64 = 0x03FF_FFFF; // Prefer BootInfo below 64 MiB
const DEFAULT_CMDLINE: &[u8] = b"root=/dev/vda rootfstype=ext2 init=/sbin/ni";

const PNP0A03_EISA_ID: u32 = encode_eisa_id(*b"PNP0A03");
const PNP0A08_EISA_ID: u32 = encode_eisa_id(*b"PNP0A08");

#[derive(Clone, Copy)]
struct DeviceTable {
    entries: [DeviceDescriptor; MAX_DEVICE_DESCRIPTORS],
    count: u16,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PciAddress {
    segment: u16,
    bus: u8,
    device: u8,
    function: u8,
}

#[repr(transparent)]
#[unsafe_protocol("937FEBF9-9284-4EC4-8E92-9A5A11B043D8")]
struct PciIo(pci_io::Protocol);

#[derive(Clone, Copy)]
struct PciSnapshot {
    vendor_id: u16,
    device_id: u16,
    class_code: u8,
    subclass: u8,
    prog_if: u8,
    revision: u8,
    header_type: u8,
    interrupt_line: u8,
    interrupt_pin: u8,
    bars: [PciBarInfo; 6],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct AcpiAddressSpaceDescriptor {
    desc: u8,
    len: u8,
    res_type: u8,
    gen_flags: u8,
    specific_flags: u8,
    addr_space_granularity: u64,
    addr_range_min: u64,
    addr_range_max: u64,
    addr_translation_offset: u64,
    addr_len: u64,
}

const ACPI_ADDRESS_SPACE_DESCRIPTOR: u8 = 0x8A;

impl PciIo {
    fn protocol_mut(&self) -> *mut pci_io::Protocol {
        &self.0 as *const _ as *mut pci_io::Protocol
    }

    fn read_config_u8(&self, offset: u32) -> Result<u8, Status> {
        let mut value = 0u8;
        let status = (self.0.pci.read)(
            self.protocol_mut(),
            pci_io::WIDTH_UINT8,
            offset,
            1,
            (&mut value as *mut u8).cast::<c_void>(),
        );
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn read_config_u16(&self, offset: u32) -> Result<u16, Status> {
        let mut value = 0u16;
        let status = (self.0.pci.read)(
            self.protocol_mut(),
            pci_io::WIDTH_UINT16,
            offset,
            1,
            (&mut value as *mut u16).cast::<c_void>(),
        );
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn read_config_u32(&self, offset: u32) -> Result<u32, Status> {
        let mut value = 0u32;
        let status = (self.0.pci.read)(
            self.protocol_mut(),
            pci_io::WIDTH_UINT32,
            offset,
            1,
            (&mut value as *mut u32).cast::<c_void>(),
        );
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn read_config_u64(&self, offset: u32) -> Result<u64, Status> {
        let mut value = 0u64;
        let status = (self.0.pci.read)(
            self.protocol_mut(),
            pci_io::WIDTH_UINT64,
            offset,
            1,
            (&mut value as *mut u64).cast::<c_void>(),
        );
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn get_bar_address(&self, bar_index: u8) -> Result<u64, Status> {
        let mut value = 0u64;
        let status = (self.0.pci.read)(
            self.protocol_mut(),
            pci_io::WIDTH_UINT64,
            (bar_index as u32) * 4,
            1,
            (&mut value as *mut u64).cast::<c_void>(),
        );
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn write_config_u8(&self, offset: u32, value: u8) -> Result<(), Status> {
        (self.0.pci.write)(
            self.protocol_mut(),
            pci_io::WIDTH_UINT8,
            offset,
            1,
            (&value as *const u8).cast::<c_void>() as *mut c_void,
        );
        Ok(())
    }
}

impl DeviceTable {
    const fn new() -> Self {
        Self {
            entries: [DeviceDescriptor::empty(); MAX_DEVICE_DESCRIPTORS],
            count: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.count as usize >= MAX_DEVICE_DESCRIPTORS
    }

    fn push_descriptor(&mut self, descriptor: DeviceDescriptor) -> bool {
        if self.is_full() {
            return false;
        }
        let idx = self.count as usize;
        self.entries[idx] = descriptor;
        self.count += 1;
        true
    }

    fn ensure_pci_entry<F>(&mut self, address: PciAddress, capability: u16, mut update: F)
    where
        F: FnMut(&mut PciDeviceInfo),
    {
        if let Some(descriptor) = self.find_pci_mut(address) {
            unsafe {
                let pci = &mut descriptor.data.pci;
                if capability != 0 {
                    pci.device_flags |= capability;
                }
                update(pci);
                descriptor.flags = pci.device_flags;
            }
            return;
        }

        if self.is_full() {
            return;
        }

        let mut pci_info = PciDeviceInfo::empty();
        pci_info.segment = address.segment;
        pci_info.bus = address.bus;
        pci_info.device = address.device;
        pci_info.function = address.function;
        pci_info.device_flags |= capability;
        update(&mut pci_info);

        let mut descriptor = DeviceDescriptor::empty();
        descriptor.kind = DeviceKind::Pci;
        descriptor.flags = pci_info.device_flags;
        descriptor.data = nexa_boot_info::DeviceData { pci: pci_info };

        let idx = self.count as usize;
        self.entries[idx] = descriptor;
        self.count += 1;
    }

    fn find_pci_mut(&mut self, address: PciAddress) -> Option<&mut DeviceDescriptor> {
        let count = self.count as usize;
        for descriptor in &mut self.entries[..count] {
            if descriptor.kind != DeviceKind::Pci {
                continue;
            }
            let pci = unsafe { &descriptor.data.pci };
            if pci.segment == address.segment
                && pci.bus == address.bus
                && pci.device == address.device
                && pci.function == address.function
            {
                return Some(descriptor);
            }
        }
        None
    }
}

const fn letter_value(b: u8) -> u32 {
    (b as u32 - 0x40) & 0x1F
}

const fn hex_value(b: u8) -> u32 {
    if b >= b'0' && b <= b'9' {
        (b - b'0') as u32
    } else if b >= b'A' && b <= b'F' {
        (b - b'A' + 10) as u32
    } else if b >= b'a' && b <= b'f' {
        (b - b'a' + 10) as u32
    } else {
        0
    }
}

const fn encode_eisa_id(bytes: [u8; 7]) -> u32 {
    let manufacturer = (letter_value(bytes[0]) << 10)
        | (letter_value(bytes[1]) << 5)
        | letter_value(bytes[2]);
    let product = (hex_value(bytes[3]) << 12)
        | (hex_value(bytes[4]) << 8)
        | (hex_value(bytes[5]) << 4)
        | hex_value(bytes[6]);
    (manufacturer << 16) | product
}

fn is_pci_root_hid(hid: u32) -> bool {
    hid == PNP0A03_EISA_ID || hid == PNP0A08_EISA_ID
}

fn collect_device_table(bs: &BootServices, image: Handle) -> DeviceTable {
    let mut table = DeviceTable::new();
    collect_block_devices(bs, image, &mut table);
    collect_network_devices(bs, image, &mut table);
    collect_usb_controllers(bs, image, &mut table);
    table
}

fn collect_block_devices(bs: &BootServices, image: Handle, table: &mut DeviceTable) {
    let Ok(handles) = bs.locate_handle_buffer(SearchType::ByProtocol(&BlockIO::GUID)) else {
        return;
    };

    for handle in handles.iter() {
        if table.is_full() {
            break;
        }

        let Some(address) = pci_address_for_handle(bs, image, *handle) else {
            continue;
        };

        let block_proto = unsafe {
            bs.open_protocol::<BlockIO>(
                OpenProtocolParams {
                    handle: *handle,
                    agent: image,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        let Ok(block_proto) = block_proto else {
            continue;
        };
        let Some(block_io) = block_proto.get() else {
            continue;
        };
        let media = block_io.media();

        let pci_snapshot = pci_snapshot_for_handle(bs, image, *handle);
        table.ensure_pci_entry(address, device_flags::BLOCK, |pci| {
            if let Some(snapshot) = pci_snapshot.as_ref() {
                apply_pci_snapshot(pci, snapshot);
            }
        });

        let block_info = build_block_info(address, media);
        let mut descriptor = DeviceDescriptor::empty();
        descriptor.kind = DeviceKind::Block;
        descriptor.flags = device_flags::BLOCK;
        descriptor.data = nexa_boot_info::DeviceData { block: block_info };
        if !table.push_descriptor(descriptor) {
            break;
        }
    }
}

fn collect_network_devices(bs: &BootServices, image: Handle, table: &mut DeviceTable) {
    let Ok(handles) = bs.locate_handle_buffer(SearchType::ByProtocol(&SimpleNetwork::GUID)) else {
        return;
    };

    for handle in handles.iter() {
        if table.is_full() {
            break;
        }

        let Some(address) = pci_address_for_handle(bs, image, *handle) else {
            log::warn!("Failed to get PCI address for network device handle");
            continue;
        };

        let snp_proto = unsafe {
            bs.open_protocol::<SimpleNetwork>(
                OpenProtocolParams {
                    handle: *handle,
                    agent: image,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        let Ok(snp_proto) = snp_proto else {
            log::warn!("Failed to open SimpleNetwork protocol");
            continue;
        };
        let Some(snp) = snp_proto.get() else {
            log::warn!("SimpleNetwork protocol returned None");
            continue;
        };
        let mode = snp.mode();

        // Try to get PCI snapshot from the same handle first
        let mut pci_snapshot = pci_snapshot_for_handle(bs, image, *handle);
        
        // If that fails, try to find PCI I/O protocol for this address
        if pci_snapshot.is_none() {
            log::warn!("Failed to get PCI snapshot from SimpleNetwork handle, searching by address");
            pci_snapshot = find_pci_snapshot_by_address(bs, image, address);
        }

        table.ensure_pci_entry(address, device_flags::NETWORK, |pci| {
            if let Some(snapshot) = pci_snapshot.as_ref() {
                apply_pci_snapshot(pci, snapshot);
                log::info!("Applied PCI snapshot: vendor={:04x}, device={:04x}", snapshot.vendor_id, snapshot.device_id);
            } else {
                log::warn!("No PCI snapshot available for network device at {:04x}:{:02x}:{:02x}.{}", 
                          address.segment, address.bus, address.device, address.function);
            }
        });

        let network_info = build_network_info(address, mode);
        let mut descriptor = DeviceDescriptor::empty();
        descriptor.kind = DeviceKind::Network;
        descriptor.flags = device_flags::NETWORK;
        descriptor.data = nexa_boot_info::DeviceData { network: network_info };
        if !table.push_descriptor(descriptor) {
            break;
        }
    }
}

fn collect_usb_controllers(bs: &BootServices, image: Handle, table: &mut DeviceTable) {
    // Enumerate all PCI devices to find USB host controllers
    let Ok(handles) = bs.locate_handle_buffer(SearchType::ByProtocol(&PciIo::GUID)) else {
        log::warn!("Failed to locate PCI I/O handles");
        return;
    };

    for handle in handles.iter() {
        if table.is_full() {
            break;
        }

        let pci_proto = unsafe {
            bs.open_protocol::<PciIo>(
                OpenProtocolParams {
                    handle: *handle,
                    agent: image,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        
        let Ok(pci_proto) = pci_proto else {
            continue;
        };
        
        let Some(pci_io) = pci_proto.get() else {
            continue;
        };

        // Read class code to identify USB controllers
        // Class 0x0C = Serial Bus Controller, Subclass 0x03 = USB
        let Ok(class_reg) = pci_io.read_config_u32(0x08) else {
            continue;
        };

        let class_code = ((class_reg >> 24) & 0xFF) as u8;
        let subclass = ((class_reg >> 16) & 0xFF) as u8;
        let prog_if = ((class_reg >> 8) & 0xFF) as u8;

        // Check if this is a USB controller
        if class_code != 0x0C || subclass != 0x03 {
            continue;
        }

        let Some(address) = pci_address_for_handle(bs, image, *handle) else {
            continue;
        };

        // Determine controller type from prog_if
        // 0x00 = UHCI, 0x10 = OHCI, 0x20 = EHCI, 0x30 = xHCI, 0x80 = Unspecified
        let controller_type = match prog_if {
            0x00 => 0, // UHCI (treated as unknown/legacy)
            0x10 => 1, // OHCI
            0x20 => 2, // EHCI
            0x30 => 3, // xHCI
            _ => 0,    // Unknown
        };

        let pci_snapshot = read_pci_snapshot(bs, pci_io);
        if let Some(snapshot) = pci_snapshot.as_ref() {
            table.ensure_pci_entry(address, device_flags::USB_HOST, |pci| {
                apply_pci_snapshot(pci, snapshot);
            });

            let usb_info = build_usb_host_info(address, &snapshot, controller_type);
            let mut descriptor = DeviceDescriptor::empty();
            descriptor.kind = DeviceKind::UsbHost;
            descriptor.flags = device_flags::USB_HOST;
            descriptor.data = nexa_boot_info::DeviceData { usb_host: usb_info };
            
            if table.push_descriptor(descriptor) {
                log::info!(
                    "Found USB {} controller at {:04x}:{:02x}:{:02x}.{} (MMIO: {:#x}, size: {:#x})",
                    match controller_type {
                        1 => "OHCI",
                        2 => "EHCI",
                        3 => "xHCI",
                        _ => "Unknown",
                    },
                    address.segment,
                    address.bus,
                    address.device,
                    address.function,
                    usb_info.mmio_base,
                    usb_info.mmio_size
                );
            }
        }
    }
}

fn build_usb_host_info(
    address: PciAddress,
    snapshot: &PciSnapshot,
    controller_type: u8,
) -> UsbHostInfo {
    let mut info = UsbHostInfo::empty();
    info.pci_segment = address.segment;
    info.pci_bus = address.bus;
    info.pci_device = address.device;
    info.pci_function = address.function;
    info.controller_type = controller_type;
    info.interrupt_line = snapshot.interrupt_line;

    // Find first valid MMIO BAR
    for bar in &snapshot.bars {
        if bar.base != 0 && bar.length != 0 && (bar.bar_flags & bar_flags::IO_SPACE) == 0 {
            info.mmio_base = bar.base;
            info.mmio_size = bar.length;
            break;
        }
    }

    // Estimate USB version based on controller type
    info.usb_version = match controller_type {
        1 => 0x0110, // OHCI: USB 1.1
        2 => 0x0200, // EHCI: USB 2.0
        3 => 0x0300, // xHCI: USB 3.0
        _ => 0x0100, // Unknown: assume USB 1.0
    };

    // Port count would require parsing controller-specific registers
    // For now, set a reasonable default
    info.port_count = match controller_type {
        3 => 4,  // xHCI typically has 4-8 ports
        2 => 4,  // EHCI typically has 2-4 ports
        _ => 2,  // OHCI typically has 2 ports
    };

    info
}

fn pci_snapshot_for_handle(bs: &BootServices, image: Handle, handle: Handle) -> Option<PciSnapshot> {
    let pci_proto = unsafe {
        bs.open_protocol::<PciIo>(
            OpenProtocolParams {
                handle,
                agent: image,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()?;

    let snapshot = pci_proto.get().and_then(|pci| read_pci_snapshot(bs, pci));
    snapshot
}

fn find_pci_snapshot_by_address(bs: &BootServices, image: Handle, target_addr: PciAddress) -> Option<PciSnapshot> {
    // Try to enumerate all PCI I/O handles and find one matching the address
    let Ok(handles) = bs.locate_handle_buffer(SearchType::ByProtocol(&PciIo::GUID)) else {
        log::warn!("Failed to locate PCI I/O handles");
        // Fallback: try using PCI Root Bridge I/O protocol
        return read_pci_via_root_bridge(bs, target_addr);
    };

    for handle in handles.iter() {
        let Some(addr) = pci_address_for_handle(bs, image, *handle) else {
            continue;
        };
        
        if addr.segment == target_addr.segment 
            && addr.bus == target_addr.bus 
            && addr.device == target_addr.device 
            && addr.function == target_addr.function 
        {
            log::info!("Found matching PCI I/O handle for {:04x}:{:02x}:{:02x}.{}", 
                      addr.segment, addr.bus, addr.device, addr.function);
            return pci_snapshot_for_handle(bs, image, *handle);
        }
    }
    
    log::warn!("Could not find PCI I/O handle for {:04x}:{:02x}:{:02x}.{}, trying root bridge", 
              target_addr.segment, target_addr.bus, target_addr.device, target_addr.function);
    
    // Fallback to PCI Root Bridge I/O protocol
    read_pci_via_root_bridge(bs, target_addr)
}

fn read_pci_via_root_bridge(_bs: &BootServices, addr: PciAddress) -> Option<PciSnapshot> {
    // Fallback: Direct PCI configuration space access via I/O ports
    // This works on x86/x86_64 platforms
    log::info!("Trying direct PCI config space access for {:04x}:{:02x}:{:02x}.{}", 
              addr.segment, addr.bus, addr.device, addr.function);
    
    // Read PCI config directly without using UEFI protocols
    read_pci_config_direct(addr)
}

fn read_pci_config_direct(addr: PciAddress) -> Option<PciSnapshot> {
    unsafe {
        // PCI Configuration Address Port (0xCF8) and Data Port (0xCFC)
        let pci_addr: u32 = 0x80000000
            | ((addr.bus as u32) << 16)
            | ((addr.device as u32) << 11)
            | ((addr.function as u32) << 8);
        
        // Read vendor_id and device_id
        let vendor_device = read_pci_config_dword(pci_addr, 0x00)?;
        let vendor_id = (vendor_device & 0xFFFF) as u16;
        let device_id = (vendor_device >> 16) as u16;
        
        if vendor_id == 0xFFFF || vendor_id == 0x0000 {
            return None;
        }
        
        // Read class code and revision
        let class_rev = read_pci_config_dword(pci_addr, 0x08)?;
        let revision = (class_rev & 0xFF) as u8;
        let prog_if = ((class_rev >> 8) & 0xFF) as u8;
        let subclass = ((class_rev >> 16) & 0xFF) as u8;
        let class_code = ((class_rev >> 24) & 0xFF) as u8;
        
        // Read header type
        let header_misc = read_pci_config_dword(pci_addr, 0x0C)?;
        let header_type = ((header_misc >> 16) & 0xFF) as u8;
        
        // Read interrupt line and pin
        let interrupt_reg = read_pci_config_dword(pci_addr, 0x3C).unwrap_or(0);
        let interrupt_line = (interrupt_reg & 0xFF) as u8;
        let interrupt_pin = ((interrupt_reg >> 8) & 0xFF) as u8;
        
        // Read BARs
        let mut bars = [PciBarInfo::empty(); 6];
        let max_bars = if (header_type & 0x7F) == 0x00 { 6 } else { 2 };
        
        for i in 0..max_bars {
            if let Some(bar_info) = read_pci_bar(pci_addr, i) {
                bars[i] = bar_info;
            }
        }
        
        Some(PciSnapshot {
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            revision,
            header_type,
            interrupt_line,
            interrupt_pin,
            bars,
        })
    }
}

unsafe fn read_pci_config_dword(base_addr: u32, offset: u32) -> Option<u32> {
    use core::arch::asm;
    
    let addr = base_addr | (offset & 0xFC);
    let mut value: u32;
    
    // Write address to 0xCF8
    asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") addr, options(nomem, nostack));
    
    // Read data from 0xCFC
    asm!("in eax, dx", in("dx") 0xCFCu16, out("eax") value, options(nomem, nostack));
    
    Some(value)
}

unsafe fn read_pci_bar(base_addr: u32, bar_index: usize) -> Option<PciBarInfo> {
    let offset = 0x10 + (bar_index as u32 * 4);
    let bar_value = read_pci_config_dword(base_addr, offset)?;
    
    if bar_value == 0 {
        return Some(PciBarInfo::empty());
    }
    
    let is_io = (bar_value & 0x1) != 0;
    let is_64bit = !is_io && ((bar_value >> 1) & 0x3) == 0x2;
    let is_prefetchable = !is_io && ((bar_value >> 3) & 0x1) != 0;
    
    let mut flags = 0u32;
    if is_io {
        flags |= bar_flags::IO_SPACE;
    } else {
        // Memory-mapped BAR (no specific flag needed for base MMIO)
        if is_64bit {
            flags |= bar_flags::MEMORY_64BIT;
        }
        if is_prefetchable {
            flags |= bar_flags::PREFETCHABLE;
        }
    }
    
    let base = if is_io {
        (bar_value & 0xFFFF_FFFC) as u64
    } else {
        if is_64bit && bar_index < 5 {
            let high = read_pci_config_dword(base_addr, offset + 4).unwrap_or(0) as u64;
            ((bar_value & 0xFFFF_FFF0) as u64) | (high << 32)
        } else {
            (bar_value & 0xFFFF_FFF0) as u64
        }
    };
    
    Some(PciBarInfo {
        base,
        length: 0, // Size detection would require writing to BAR, skip for now
        bar_flags: flags,
        reserved: 0,
    })
}

fn apply_pci_snapshot(target: &mut PciDeviceInfo, snapshot: &PciSnapshot) {
    target.vendor_id = snapshot.vendor_id;
    target.device_id = snapshot.device_id;
    target.class_code = snapshot.class_code;
    target.subclass = snapshot.subclass;
    target.prog_if = snapshot.prog_if;
    target.revision = snapshot.revision;
    target.header_type = snapshot.header_type;
    target.interrupt_line = snapshot.interrupt_line;
    target.interrupt_pin = snapshot.interrupt_pin;
    target.bars.copy_from_slice(&snapshot.bars);
}

fn build_block_info(
    address: PciAddress,
    media: &uefi::proto::media::block::BlockIOMedia,
) -> BlockDeviceInfo {
    let mut info = BlockDeviceInfo::empty();
    info.pci_segment = address.segment;
    info.pci_bus = address.bus;
    info.pci_device = address.device;
    info.pci_function = address.function;
    info.block_size = media.block_size();
    info.last_block = media.last_block();
    info.media_id = media.media_id();
    info.io_align = media.io_align();
    info.logical_blocks_per_physical = media.logical_blocks_per_physical_block();
    info.optimal_transfer_granularity = media.optimal_transfer_length_granularity();
    info.lowest_aligned_lba = media.lowest_aligned_lba();

    let mut flags = 0u16;
    if media.is_media_present() {
        flags |= block_flags::MEDIA_PRESENT;
    }
    if media.is_read_only() {
        flags |= block_flags::READ_ONLY;
    }
    if media.is_removable_media() {
        flags |= block_flags::REMOVABLE;
    }
    if media.is_logical_partition() {
        flags |= block_flags::LOGICAL_PARTITION;
    }
    if media.is_write_caching() {
        flags |= block_flags::WRITE_CACHING;
    }
    info.flags = flags;

    info
}

fn build_network_info(
    address: PciAddress,
    mode: &uefi::proto::network::snp::NetworkMode,
) -> NetworkDeviceInfo {
    let mut info = NetworkDeviceInfo::empty();
    info.pci_segment = address.segment;
    info.pci_bus = address.bus;
    info.pci_device = address.device;
    info.pci_function = address.function;
    info.if_type = mode.if_type;

    let mac_len = mode
        .hw_address_size
        .min(info.mac_address.len() as u32) as usize;
    info.mac_len = mac_len as u8;
    if mac_len > 0 {
        info.mac_address[..mac_len].copy_from_slice(&mode.current_address.0[..mac_len]);
    }

    info.max_packet_size = mode.max_packet_size;
    info.receive_filter_mask = mode.receive_filter_mask;
    info.receive_filter_setting = mode.receive_filter_setting;
    info.link_speed_mbps = 0;

    let mut flags = 0u16;
    if mode.media_present {
        flags |= network_flags::MEDIA_PRESENT;
    }
    if mode.media_present && mode.state == NetworkState::INITIALIZED {
        flags |= network_flags::LINK_UP;
    }
    if mode.mac_address_changeable {
        flags |= network_flags::MAC_MUTABLE;
    }
    if mode.multiple_tx_supported {
        flags |= network_flags::MULTIPLE_TX;
    }
    info.flags = flags;

    info
}

fn read_pci_snapshot(bs: &BootServices, pci_io: &PciIo) -> Option<PciSnapshot> {
    let vendor_id = pci_io.read_config_u16(0x00).ok()?;
    if vendor_id == 0xFFFF {
        return None;
    }
    let device_id = pci_io.read_config_u16(0x02).ok()?;
    let class_reg = pci_io.read_config_u32(0x08).ok()?;
    let header_reg = pci_io.read_config_u32(0x0C).ok()?;
    let interrupt_reg = pci_io.read_config_u32(0x3C).unwrap_or(0);

    let header_type = ((header_reg >> 16) & 0xFF) as u8;
    let bars = read_pci_bars(bs, pci_io, header_type);

    Some(PciSnapshot {
        vendor_id,
        device_id,
        class_code: ((class_reg >> 24) & 0xFF) as u8,
        subclass: ((class_reg >> 16) & 0xFF) as u8,
        prog_if: ((class_reg >> 8) & 0xFF) as u8,
        revision: (class_reg & 0xFF) as u8,
        header_type,
        interrupt_line: (interrupt_reg & 0xFF) as u8,
        interrupt_pin: ((interrupt_reg >> 8) & 0xFF) as u8,
        bars,
    })
}

fn read_pci_bars(bs: &BootServices, pci_io: &PciIo, header_type: u8) -> [PciBarInfo; 6] {
    let mut bars = [PciBarInfo::empty(); 6];
    let layout = header_type & 0x7F;
    let max_bars = match layout {
        0x00 => 6,
        0x01 => 2,
        _ => 0,
    };

    let mut bar_index = 0usize;
    let mut slot = 0usize;
    while bar_index < max_bars && slot < bars.len() {
        let offset = 0x10 + (bar_index * 4) as u32;
        let raw = match pci_io.read_config_u32(offset) {
            Ok(value) => value,
            Err(_) => {
                bar_index += 1;
                continue;
            }
        };
        if raw == 0 {
            bar_index += 1;
            continue;
        }

        let mut flags = 0u32;
        let mut consumed = 1usize;
        let mut base = 0u64;
        let mut length = 0u64;

        if (raw & 0x1) != 0 {
            base = (raw & 0xFFFF_FFFC) as u64;
            flags |= bar_flags::IO_SPACE;
        } else {
            if (raw & 0x8) != 0 {
                flags |= bar_flags::PREFETCHABLE;
            }
            base = (raw & 0xFFFF_FFF0) as u64;
            let mem_type = (raw >> 1) & 0x3;
            if mem_type == 0b10 {
                let high = pci_io.read_config_u32(offset + 4).unwrap_or(0);
                base |= (high as u64) << 32;
                flags |= bar_flags::MEMORY_64BIT;
                consumed = 2;
            }
        }

        if let Some((desc_base, desc_len)) = query_bar_descriptor(bs, pci_io, bar_index as u8) {
            if desc_base != 0 {
                base = desc_base;
            }
            if desc_len != 0 {
                length = desc_len;
            }
        }

        bars[slot] = PciBarInfo {
            base,
            length,
            bar_flags: flags,
            reserved: 0,
        };

        slot += 1;
        bar_index += consumed;
    }

    bars
}

fn query_bar_descriptor(
    bs: &BootServices,
    pci_io: &PciIo,
    bar_index: u8,
) -> Option<(u64, u64)> {
    let mut _attributes: pci_io::Attribute = 0;
    let mut resource_ptr: *mut c_void = ptr::null_mut();
    let status = (pci_io.0.get_bar_attributes)(
        pci_io.protocol_mut(),
        bar_index,
        &mut _attributes,
        &mut resource_ptr,
    );
    if status != raw_base::Status::SUCCESS {
        return None;
    }

    let mut result = None;
    if !resource_ptr.is_null() {
        unsafe {
            if let Some(descriptor) = parse_address_space_descriptor(resource_ptr.cast::<c_void>())
            {
                result = Some((descriptor.addr_range_min, descriptor.addr_len));
            }
        }
    }

    if !resource_ptr.is_null() {
        unsafe {
            let _ = bs.free_pool(resource_ptr.cast::<u8>());
        }
    }

    result
}

fn parse_address_space_descriptor(ptr: *const c_void) -> Option<AcpiAddressSpaceDescriptor> {
    if ptr.is_null() {
        return None;
    }

    let descriptor = unsafe { ptr::read_unaligned(ptr as *const AcpiAddressSpaceDescriptor) };
    if descriptor.desc != ACPI_ADDRESS_SPACE_DESCRIPTOR {
        return None;
    }
    Some(descriptor)
}

fn pci_address_for_handle(bs: &BootServices, image: Handle, handle: Handle) -> Option<PciAddress> {
    let device_path = unsafe {
        bs.open_protocol::<DevicePath>(
            OpenProtocolParams {
                handle,
                agent: image,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
        .ok()?
    };
    let device_path = device_path.get()?;
    parse_pci_address(device_path)
}

fn parse_pci_address(path: &DevicePath) -> Option<PciAddress> {
    let mut segment = 0u16;
    let mut bus = 0u8;

    for node in path.node_iter() {
        let Ok(specific) = node.as_enum() else { continue };
        match specific {
            DevicePathNodeEnum::AcpiAcpi(acpi_node) => {
                if is_pci_root_hid(acpi_node.hid()) {
                    bus = (acpi_node.uid() & 0xFF) as u8;
                    let _base = (acpi_node.uid() >> 16) as u16;
                    // segment = base; // 原代码中base未定义，直接使用计算结果
                    segment = (acpi_node.uid() >> 16) as u16;
                }
            }
            DevicePathNodeEnum::AcpiExpanded(expanded) => {
                if is_pci_root_hid(expanded.hid()) {
                    bus = (expanded.uid() & 0xFF) as u8;
                    let _base = (expanded.uid() >> 16) as u16;
                    // segment = base; // 原代码中base未定义，直接使用计算结果
                    segment = (expanded.uid() >> 16) as u16;
                }
            }
            DevicePathNodeEnum::HardwarePci(pci_node) => {
                return Some(PciAddress {
                    segment,
                    bus,
                    device: pci_node.device(),
                    function: pci_node.function(),
                });
            }
            _ => {}
        }
    }

    None
}

/// Get ACPI RSDP address from UEFI configuration table
fn get_acpi_rsdp_addr(st: &SystemTable<Boot>) -> Option<u64> {
    // ACPI 2.0+ GUID: 8868e871-e4f1-11d3-bc22-0080c73c8881
    const ACPI_20_TABLE_GUID: uefi::Guid = uefi::Guid::from_bytes([
        0x71, 0xe8, 0x68, 0x88,  // time_low
        0xf1, 0xe4,              // time_mid
        0xd3, 0x11,              // time_high_and_version
        0xbc, 0x22,              // clock_seq
        0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81,  // node
    ]);
    
    // ACPI 1.0 GUID (fallback): eb9d2d30-2d88-11d3-9a16-0090273fc14d
    const ACPI_TABLE_GUID: uefi::Guid = uefi::Guid::from_bytes([
        0x30, 0x2d, 0x9d, 0xeb,  // time_low
        0x88, 0x2d,              // time_mid
        0xd3, 0x11,              // time_high_and_version
        0x9a, 0x16,              // clock_seq
        0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d,  // node
    ]);
    
    // Try ACPI 2.0+ first
    for entry in st.config_table() {
        if entry.guid == ACPI_20_TABLE_GUID {
            let rsdp_addr = entry.address as u64;
            log::info!("Found ACPI 2.0+ RSDP at {:#x}", rsdp_addr);
            return Some(rsdp_addr);
        }
    }
    
    // Fall back to ACPI 1.0
    for entry in st.config_table() {
        if entry.guid == ACPI_TABLE_GUID {
            let rsdp_addr = entry.address as u64;
            log::info!("Found ACPI 1.0 RSDP at {:#x}", rsdp_addr);
            return Some(rsdp_addr);
        }
    }
    
    log::warn!("ACPI RSDP not found in UEFI configuration table");
    None
}

#[entry]
fn efi_main(image: Handle, mut st: SystemTable<Boot>) -> Status {
    if let Err(e) = uefi::helpers::init(&mut st) {
        return e.status();
    }

    log::info!("NexaOS UEFI loader starting");
    log::info!("Image handle: {:?}", image);

    let bs = st.boot_services();
    log::info!("Boot services initialized");

    let mut root = match open_boot_volume(bs, image) {
        Ok(dir) => {
            log::info!("Boot volume opened successfully");
            dir
        },
        Err(status) => {
            log::error!("Failed to open boot volume: {:?}", status);
            return status;
        }
    };

    // List root directory
    log::info!("=== Listing root directory ===");
    let _ = list_directory(&mut root, cstr16!("\\"));
    
    // List EFI directory
    log::info!("=== Listing \\EFI directory ===");
    let _ = list_directory(&mut root, cstr16!("\\EFI"));
    
    // List BOOT directory
    log::info!("=== Listing \\EFI\\BOOT directory ===");
    let _ = list_directory(&mut root, cstr16!("\\EFI\\BOOT"));

    let kernel_bytes = match read_file(&mut root, KERNEL_PATH) {
        Ok(data) => {
            log::info!("Kernel image loaded, size: {} bytes", data.len());
            data
        },
        Err(status) => {
            log::error!("Failed to load kernel image: {:?}", status);
            return status;
        }
    };

    let initramfs_bytes = match read_file(&mut root, INITRAMFS_PATH) {
        Ok(data) => {
            log::info!("Initramfs loaded, size: {} bytes", data.len());
            data
        },
        Err(status) if status == Status::NOT_FOUND => {
            log::warn!("Initramfs not found, using empty");
            Vec::new()
        },
        Err(status) => {
            log::error!("Failed to load initramfs: {:?}", status);
            return status;
        }
    };

    let mut rootfs_bytes = match read_file(&mut root, ROOTFS_PATH) {
        Ok(data) => {
            log::info!("Rootfs image loaded from ESP, size: {} bytes", data.len());
            Some(data)
        }
        Err(status) if status == Status::NOT_FOUND => {
            log::info!("Rootfs image not present on ESP; will probe block devices");
            None
        }
        Err(status) => {
            log::warn!("Failed to load rootfs from ESP: {:?}", status);
            None
        }
    };

    let cmdline_bytes = load_kernel_cmdline(&mut root);
    match core::str::from_utf8(&cmdline_bytes) {
        Ok(text) => log::info!("Kernel command line: {}", text),
        Err(_) => log::warn!("Kernel command line contains non-UTF8 data"),
    }

    drop(root);
    log::info!("File system root dropped");

    if rootfs_bytes.is_none() {
        rootfs_bytes = load_rootfs_from_block_device(bs, image);
    }

    let loaded = match load_kernel_image(bs, &kernel_bytes) {
        Ok(info) => {
            log::info!(
                "Kernel loaded successfully: expected entry {:#x}, actual entry {:#x}",
                info.expected_entry_point,
                info.actual_entry_point
            );
            info
        },
        Err(status) => {
            log::error!("Kernel load failed: {:?}", status);
            return status;
        }
    };
    
    // 添加调试信息验证入口点地址
    log::info!("Expected entry point: {:#x}", 0x101020);
    if loaded.expected_entry_point != 0x101020 {
        log::warn!(
            "Entry point mismatch! Expected: {:#x}, Got: {:#x}",
            0x101020,
            loaded.expected_entry_point
        );
    }
    
    // 收集设备信息
    let device_table = collect_device_table(bs, image);
    log::info!("Device table collected, count: {}", device_table.count);
    
    // 获取ACPI RSDP地址
    let acpi_rsdp_addr = get_acpi_rsdp_addr(&st);
    
    // 准备帧缓冲信息
    let framebuffer_info = detect_framebuffer(bs, image);
    log::info!("Framebuffer detection completed, found: {}", framebuffer_info.is_some());
    
    // 准备initramfs
    let initramfs_region = match stage_payload(bs, &initramfs_bytes, MemoryType::LOADER_DATA) {
        Ok(region) => {
            log::info!("Initramfs staged, addr: {:#x}, size: {}", region.phys_addr, region.length);
            region
        },
        Err(status) => {
            log::error!("Failed to allocate initramfs region: {:?}", status);
            return status;
        }
    };

    let rootfs_region = if let Some(ref bytes) = rootfs_bytes {
        match stage_payload(bs, bytes, MemoryType::LOADER_DATA) {
            Ok(region) => {
                log::info!(
                    "Rootfs staged, addr: {:#x}, size: {}",
                    region.phys_addr,
                    region.length
                );
                region
            }
            Err(status) => {
                log::error!("Failed to allocate rootfs region: {:?}", status);
                return status;
            }
        }
    } else {
        log::warn!("Rootfs image unavailable; kernel will rely on /dev mounts");
        MemoryRegion::empty()
    };

    if !rootfs_region.is_empty() {
        rootfs_bytes = None;
    }
    
    // 创建启动信息
    let kernel_segments_region = match stage_kernel_segments(bs, &loaded.segments) {
        Ok(region) => {
            let segment_count = if region.length == 0 {
                0
            } else {
                region.length as usize / core::mem::size_of::<KernelSegment>()
            };
            log::info!(
                "Kernel segments staged, addr: {:#x}, size: {} ({} entries)",
                region.phys_addr,
                region.length,
                segment_count
            );
            region
        }
        Err(status) => {
            log::error!("Failed to stage kernel segment table: {:?}", status);
            return status;
        }
    };

    let boot_info_region = match stage_boot_info(
        bs,
        initramfs_region,
        rootfs_region,
        framebuffer_info,
        &device_table,
        loaded.kernel_offset,
        loaded.expected_entry_point,
        loaded.actual_entry_point,
        &cmdline_bytes,
        kernel_segments_region,
        acpi_rsdp_addr,
    ) {
        Ok(region) => {
            log::info!("Boot info staged, addr: {:#x}, size: {}", region.phys_addr, region.length);
            region
        },
        Err(status) => {
            log::error!("Failed to allocate boot info region: {:?}", status);
            return status;
        }
    };

    log::info!("About to exit boot services");
    let _ = st.exit_boot_services(MemoryType::LOADER_DATA);
    log::info!("Exit boot services completed");

    let mut entry_point = loaded.actual_entry_point;
    let mut boot_info_ptr = boot_info_region.phys_addr;

    if loaded.kernel_offset != 0 {
        log::info!(
            "Kernel relocated by offset {:#x}; mirroring segments to original addresses",
            loaded.kernel_offset
        );
        if mirror_segments_to_expected(&loaded.segments) {
            entry_point = loaded.expected_entry_point;
            log::info!(
                "Segments mirrored successfully, switching to expected entry {:#x}",
                entry_point
            );

            if let Some(expected_ptr) = mirror_boot_info_to_expected(&boot_info_region, loaded.kernel_offset) {
                boot_info_ptr = expected_ptr;
                log::info!(
                    "Boot info mirrored to expected address {:#x}",
                    boot_info_ptr
                );
            } else {
                log::warn!(
                    "Failed to mirror boot info block; continuing with relocated pointer {:#x}",
                    boot_info_ptr
                );
            }
        } else {
            log::warn!(
                "Encountered issues while mirroring segments; continuing from relocated entry {:#x}",
                entry_point
            );
        }
    }

    log::info!(
        "Transferring control to kernel UEFI entry at {:#x}",
        entry_point
    );

    // 添加更多调试信息
    log::info!(
        "Boot info address: staged {:#x}, handoff {:#x}",
        boot_info_region.phys_addr,
        boot_info_ptr
    );
    log::info!(
        "Kernel base: expected {:#x}, actual {:#x}, offset {:#x}",
        loaded.expected_base,
        loaded.kernel_base,
        loaded.kernel_offset
    );
    log::info!("About to jump to kernel entry point...");

    unsafe {
        let boot_info_ptr_ref = boot_info_ptr as *const BootInfo;
        let signature = (*boot_info_ptr_ref).signature;
        let version = (*boot_info_ptr_ref).version;
        let flags = (*boot_info_ptr_ref).flags;
        log::info!(
            "Boot info signature before handoff: {:?}, version: {}, flags: {:#x}",
            signature,
            version,
            flags
        );
    }

    // 写入原始 COM1 端口确认我们到达这里
    unsafe {
        let com1 = 0x3f8 as *mut u8;
        let msg = b"[UEFI] Jumping to kernel...\n";
        for &byte in msg {
            com1.write_volatile(byte);
        }
    }

    unsafe {
        let entry: extern "sysv64" fn(*const BootInfo) -> ! = mem::transmute(entry_point as usize);
        entry(boot_info_ptr as *const BootInfo)
    }
}

#[derive(Clone, Copy, Debug)]
struct LoadedSegment {
    expected_addr: u64,
    actual_addr: u64,
    memsz: u64,
}

struct LoadedKernel {
    expected_base: u64,
    expected_entry_point: u64,
    actual_entry_point: u64,
    kernel_base: u64,      // 实际加载的基地址
    kernel_offset: i64,    // 相对于链接地址的偏移
    segments: Vec<LoadedSegment>,
}

fn open_boot_volume(bs: &BootServices, image: Handle) -> Result<Directory, Status> {
    log::info!("Searching for boot volume with kernel files");
    
    // 获取所有支持 SimpleFileSystem 的设备
    let handles = bs
        .locate_handle_buffer(SearchType::ByProtocol(&SimpleFileSystem::GUID))
        .map_err(|e| {
            log::error!("Failed to locate handles with SimpleFileSystem protocol: {:?}", e);
            Status::UNSUPPORTED
        })?;
    
    if handles.is_empty() {
        log::error!("No devices with SimpleFileSystem protocol found");
        return Err(Status::NOT_FOUND);
    }
    
    log::info!("Found {} device(s) with SimpleFileSystem protocol", handles.len());
    
    // 尝试每个文件系统设备，找到包含内核文件的那个
    for (index, &device_handle) in handles.iter().enumerate() {
        log::info!("Trying device {} ({:?})", index, device_handle);
        
        let fs = unsafe {
            bs.open_protocol::<SimpleFileSystem>(
                OpenProtocolParams {
                    handle: device_handle,
                    agent: image,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        
        let Ok(fs) = fs else {
            log::warn!("  Failed to open SimpleFileSystem protocol on device {}", index);
            continue;
        };
        
        let Some(file_system) = fs.get_mut() else {
            log::warn!("  Failed to get SimpleFileSystem reference for device {}", index);
            continue;
        };
        
        let mut volume = match file_system.open_volume() {
            Ok(vol) => vol,
            Err(e) => {
                log::warn!("  Failed to open volume on device {}: {:?}", index, e.status());
                continue;
            }
        };
        
        // 尝试打开内核文件来验证这是正确的卷
        log::info!("  Checking for kernel file on device {}", index);
        let test_result = volume.open(KERNEL_PATH, FileMode::Read, FileAttribute::empty());
        
        match test_result {
            Ok(_) => {
                log::info!("  Found kernel file on device {}! Using this volume.", index);
                return Ok(volume);
            }
            Err(e) => {
                log::info!("  Kernel file not found on device {}: {:?}", index, e.status());
                // 继续尝试下一个设备
            }
        }
    }
    
    log::error!("Could not find boot volume with kernel file at {:?}", KERNEL_PATH);
    Err(Status::NOT_FOUND)
}

fn list_directory(root: &mut Directory, path: &uefi::CStr16) -> Result<(), Status> {
    log::info!("Listing directory: {:?}", path);
    
    let handle = root
        .open(path, FileMode::Read, FileAttribute::empty())
        .map_err(|e: Error| {
            log::error!("Failed to open directory {:?}: {:?}", path, e.status());
            e.status()
        })?;
    
    let Some(mut dir) = handle.into_directory() else {
        log::error!("Path {:?} is not a directory", path);
        return Err(Status::UNSUPPORTED);
    };
    
    let mut entry_buffer = [0u8; 512];
    loop {
        match dir.read_entry(&mut entry_buffer) {
            Ok(Some(info)) => {
                log::info!("  Found: {:?} (attr: {:?})", info.file_name(), info.attribute());
            }
            Ok(None) => {
                log::info!("End of directory listing");
                break;
            }
            Err(e) => {
                log::error!("Error reading directory entry: {:?}", e.status());
                return Err(e.status());
            }
        }
    }
    
    Ok(())
}

fn read_file(root: &mut Directory, path: &uefi::CStr16) -> Result<Vec<u8>, Status> {
    log::info!("Attempting to open file: {:?}", path);
    let handle = root
    .open(path, FileMode::Read, FileAttribute::empty())
    .map_err(|e: Error| {
        log::error!("Failed to open file {:?}: {:?}", path, e.status());
        e.status()
    })?;
    let Some(file) = handle.into_regular_file() else {
        log::error!("File {:?} is not a regular file", path);
        return Err(Status::UNSUPPORTED);
    };
    log::info!("Successfully opened file: {:?}", path);
    read_entire_file(file)
}

fn read_entire_file(mut file: RegularFile) -> Result<Vec<u8>, Status> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|e: Error| e.status())?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(buffer)
}

fn load_kernel_cmdline(root: &mut Directory) -> Vec<u8> {
    match read_file(root, CMDLINE_PATH) {
        Ok(mut data) => {
            while matches!(data.last(), Some(b' ') | Some(b'\n') | Some(b'\r') | Some(&0)) {
                data.pop();
            }
            if data.is_empty() {
                log::warn!(
                    "cmdline.txt is empty; falling back to default kernel command line"
                );
                DEFAULT_CMDLINE.to_vec()
            } else {
                log::info!("Loaded kernel command line from cmdline.txt");
                data
            }
        }
        Err(status) if status == Status::NOT_FOUND => {
            log::info!(
                "cmdline.txt not found; using default kernel command line: {}",
                core::str::from_utf8(DEFAULT_CMDLINE).unwrap_or("<non-utf8>")
            );
            DEFAULT_CMDLINE.to_vec()
        }
        Err(status) => {
            log::warn!(
                "Failed to read cmdline.txt (status {:?}); using default kernel command line",
                status
            );
            DEFAULT_CMDLINE.to_vec()
        }
    }
}

fn load_rootfs_from_block_device(bs: &BootServices, image: Handle) -> Option<Vec<u8>> {
    let handles = bs
        .locate_handle_buffer(SearchType::ByProtocol(&BlockIO::GUID))
        .ok()?;

    for handle in handles.iter() {
        let block_proto = unsafe {
            bs.open_protocol::<BlockIO>(
                OpenProtocolParams {
                    handle: *handle,
                    agent: image,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        let Ok(block_proto) = block_proto else {
            continue;
        };
        let Some(block_io) = block_proto.get() else {
            continue;
        };
        let media = block_io.media();
        if !media.is_media_present() {
            continue;
        }

        let block_size = media.block_size() as usize;
        if block_size == 0 {
            continue;
        }

        // Prefer 512-byte logical block devices (virtio-blk).
        if block_size != 512 {
            log::debug!(
                "Skipping block device with block size {} bytes (not 512)",
                block_size
            );
            continue;
        }

        let total_blocks = media.last_block().saturating_add(1);
        let total_bytes = match total_blocks.checked_mul(block_size as u64) {
            Some(value) => value,
            None => {
                log::warn!("Block device size overflow, skipping");
                continue;
            }
        };

        // Avoid pulling unreasonably large disks into memory.
        if total_bytes > 256 * 1024 * 1024 {
            log::warn!(
                "Block device reports {} bytes (>256 MiB), skipping",
                total_bytes
            );
            continue;
        }

        let mut buffer = vec![0u8; total_bytes as usize];
        let media_id = media.media_id();
        let mut lba = 0u64;
        let mut offset = 0usize;
        const CHUNK_BLOCKS: usize = 128;

        log::info!(
            "Reading root filesystem from block device: {} blocks × {} bytes",
            total_blocks,
            block_size
        );

        while lba < total_blocks {
            let remaining_blocks = (total_blocks - lba) as usize;
            let chunk_blocks = remaining_blocks.min(CHUNK_BLOCKS);
            let chunk_bytes = chunk_blocks * block_size;
            let slice = &mut buffer[offset..offset + chunk_bytes];

            if let Err(status) = block_io.read_blocks(media_id, lba, slice) {
                log::warn!(
                    "Failed to read LBA {} ({} blocks): {:?}",
                    lba,
                    chunk_blocks,
                    status
                );
                buffer.clear();
                break;
            }

            lba += chunk_blocks as u64;
            offset += chunk_bytes;
        }

        if offset == buffer.len() {
            if let Some(address) = pci_address_for_handle(bs, image, *handle) {
                log::info!(
                    "Loaded rootfs image ({} bytes) from {:04x}:{:02x}:{:02x}.{}",
                    buffer.len(),
                    address.segment,
                    address.bus,
                    address.device,
                    address.function
                );
            } else {
                log::info!(
                    "Loaded rootfs image ({} bytes) from block device (no PCI address)",
                    buffer.len()
                );
            }

            return Some(buffer);
        }
    }

    None
}

fn load_kernel_image(bs: &BootServices, image: &[u8]) -> Result<LoadedKernel, Status> {
    if image.len() < mem::size_of::<Elf64Ehdr>() {
        return Err(Status::LOAD_ERROR);
    }

    let header = unsafe { &*(image.as_ptr() as *const Elf64Ehdr) };

    if &header.e_ident[0..4] != b"\x7FELF" {
        return Err(Status::LOAD_ERROR);
    }
    if header.e_ident[4] != 2 || header.e_ident[5] != 1 {
        return Err(Status::LOAD_ERROR);
    }

    let phoff = header.e_phoff as usize;
    let phentsize = header.e_phentsize as usize;
    let phnum = header.e_phnum as usize;
    let mut segments: Vec<LoadedSegment> = Vec::new();

    // 第一遍：找出所有 LOAD 段的地址范围
    let mut min_addr = u64::MAX;
    let mut max_addr = 0u64;
    
    for i in 0..phnum {
        let offset = phoff + i * phentsize;
        if offset + mem::size_of::<Elf64Phdr>() > image.len() {
            return Err(Status::LOAD_ERROR);
        }
        let ph = unsafe { &*(image.as_ptr().add(offset) as *const Elf64Phdr) };
        if ph.p_type != 1 || ph.p_memsz == 0 {
            continue;
        }
        
        let start = ph.p_paddr;
        let end = ph.p_paddr + ph.p_memsz;
        
        if start < min_addr {
            min_addr = start;
        }
        if end > max_addr {
            max_addr = end;
        }
    }
    
    if min_addr >= max_addr {
        log::error!("No valid LOAD segments found");
        return Err(Status::LOAD_ERROR);
    }
    
    // 计算需要的总内存大小
    let total_size = (max_addr - min_addr) as usize;
    let pages = (total_size + 0xFFF) / 0x1000;
    
    log::info!("Kernel expects to be loaded at {:#x}-{:#x} (size: {:#x}, {} pages)", 
               min_addr, max_addr, total_size, pages);
    
    // 尝试在期望地址分配，如果失败则在任意地址分配
    let actual_base = match bs.allocate_pages(
        AllocateType::Address(min_addr),
        MemoryType::LOADER_DATA,
        pages,
    ) {
        Ok(_) => {
            log::info!("Successfully allocated at expected address {:#x}", min_addr);
            min_addr
        }
        Err(e) => {
            log::warn!("Cannot allocate at expected address {:#x}: {:?}", min_addr, e.status());
            
            // 尝试在 64MB 以下分配（低于常见的 UEFI 固件内存）
            log::info!("Trying to allocate below 64MB...");
            match bs.allocate_pages(
                AllocateType::MaxAddress(0x4000000), // 64MB
                MemoryType::LOADER_DATA,
                pages,
            ) {
                Ok(addr) => {
                    log::info!("Allocated at low address {:#x}", addr);
                    addr
                }
                Err(e2) => {
                    log::warn!("Cannot allocate below 64MB: {:?}, trying below 4GB", e2.status());
                    // 最后尝试在 4GB 以下分配
                    match bs.allocate_pages(
                        AllocateType::MaxAddress(0xFFFFFFFF),
                        MemoryType::LOADER_DATA,
                        pages,
                    ) {
                        Ok(addr) => {
                            log::info!("Allocated at alternative address {:#x}", addr);
                            addr
                        }
                        Err(e3) => {
                            log::error!("Failed to allocate {} pages anywhere: {:?}", pages, e3.status());
                            return Err(e3.status());
                        }
                    }
                }
            }
        }
    };
    
    // 计算加载偏移
    let load_offset = (actual_base as i64) - (min_addr as i64);
    log::info!("Load offset: {:#x} (actual base: {:#x}, expected base: {:#x})", 
               load_offset, actual_base, min_addr);
    
    // 第二遍：加载所有段
    for i in 0..phnum {
        let offset = phoff + i * phentsize;
        let ph = unsafe { &*(image.as_ptr().add(offset) as *const Elf64Phdr) };
        if ph.p_type != 1 {
            continue;
        }

        let expected_addr = ph.p_paddr;
        let actual_addr = ((expected_addr as i64) + load_offset) as u64;
        let memsz = ph.p_memsz as usize;
        let filesz = ph.p_filesz as usize;
        
        log::info!("Loading segment {}: {:#x} -> {:#x}, memsz={:#x}, filesz={:#x}", 
                   i, expected_addr, actual_addr, memsz, filesz);
        
        if memsz == 0 {
            continue;
        }

        if filesz > 0 {
            let src_offset = ph.p_offset as usize;
            if src_offset + filesz > image.len() {
                return Err(Status::LOAD_ERROR);
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    image.as_ptr().add(src_offset),
                    actual_addr as *mut u8,
                    filesz,
                );
            }
        }

        if memsz > filesz {
            unsafe {
                ptr::write_bytes((actual_addr + filesz as u64) as *mut u8, 0, memsz - filesz);
            }
        }

        segments.push(LoadedSegment {
            expected_addr,
            actual_addr,
            memsz: memsz as u64,
        });
    }

    log::info!("Loading kernel ELF program headers complete");
    
    // 查找 UEFI 入口点
    let expected_entry = match find_uefi_entry(image) {
        Some(ptr) => {
            log::info!("UEFI entry point in ELF: {:#x}", ptr);
            ptr
        },
        None => {
            log::error!("Kernel image missing .nexa.uefi_entry section");
            return Err(Status::LOAD_ERROR);
        }
    };
    
    // 应用加载偏移到入口点
    let actual_entry = ((expected_entry as i64) + load_offset) as u64;
    log::info!("Actual UEFI entry point: {:#x}", actual_entry);

    Ok(LoadedKernel { 
        expected_base: min_addr,
        expected_entry_point: expected_entry,
        actual_entry_point: actual_entry,
        kernel_base: actual_base,
        kernel_offset: load_offset,
        segments,
    })
}

fn stage_payload(bs: &BootServices, data: &[u8], mem_type: MemoryType) -> Result<MemoryRegion, Status> {
    if data.is_empty() {
        return Ok(MemoryRegion::empty());
    }

    let pages = (data.len() + 0xFFF) / 0x1000;
    let addr = bs
        .allocate_pages(
            AllocateType::MaxAddress(MAX_PHYS_ADDR),
            mem_type,
            pages,
        )
        .map_err(|e: Error| e.status())? as usize;
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), addr as *mut u8, data.len());
        let total = pages * 0x1000;
        if total > data.len() {
            ptr::write_bytes((addr + data.len()) as *mut u8, 0, total - data.len());
        }
    }
    Ok(MemoryRegion {
        phys_addr: addr as u64,
        length: data.len() as u64,
    })
}

fn stage_kernel_segments(bs: &BootServices, segments: &[LoadedSegment]) -> Result<MemoryRegion, Status> {
    if segments.is_empty() {
        return Ok(MemoryRegion::empty());
    }

    let mut serialized: Vec<KernelSegment> = Vec::with_capacity(segments.len());
    for seg in segments {
        serialized.push(KernelSegment {
            expected_addr: seg.expected_addr,
            actual_addr: seg.actual_addr,
            mem_size: seg.memsz,
        });
    }

    let bytes = unsafe {
        core::slice::from_raw_parts(
            serialized.as_ptr() as *const u8,
            serialized.len() * core::mem::size_of::<KernelSegment>(),
        )
    };

    let region = stage_payload(bs, bytes, MemoryType::LOADER_DATA)?;

    // stage_payload copies into the allocated pages, so dropping the vector is safe.
    Ok(region)
}

fn mirror_segments_to_expected(segments: &[LoadedSegment]) -> bool {
    let mut all_ok = true;

    for seg in segments {
        if seg.expected_addr == seg.actual_addr || seg.memsz == 0 {
            continue;
        }

        if seg.memsz > usize::MAX as u64 {
            log::error!(
                "Segment at expected {:#x} exceeds addressable size ({} bytes)",
                seg.expected_addr,
                seg.memsz
            );
            all_ok = false;
            continue;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                seg.actual_addr as *const u8,
                seg.expected_addr as *mut u8,
                seg.memsz as usize,
            );
        }
    }

    all_ok
}

fn mirror_boot_info_to_expected(region: &MemoryRegion, offset: i64) -> Option<u64> {
    if offset <= 0 || region.is_empty() || region.length == 0 {
        return Some(region.phys_addr);
    }

    if region.length > usize::MAX as u64 {
        log::error!(
            "Boot info region length ({}) exceeds addressable size",
            region.length
        );
        return None;
    }

    let offset_u64 = offset as u64;
    if region.phys_addr < offset_u64 {
        log::error!(
            "Boot info address {:#x} is smaller than relocation offset {:#x}",
            region.phys_addr,
            offset_u64
        );
        return None;
    }

    let expected_addr = region.phys_addr - offset_u64;

    // Avoid copying into legacy regions that are typically ROM or device windows.
    if (0x000A_0000..0x0010_0000).contains(&expected_addr) {
        log::warn!(
            "Boot info expected address {:#x} lies in reserved low memory; skipping mirror",
            expected_addr
        );
        return None;
    }

    unsafe {
        ptr::copy_nonoverlapping(
            region.phys_addr as *const u8,
            expected_addr as *mut u8,
            region.length as usize,
        );
    }

    unsafe {
        let mirrored = &*(expected_addr as *const BootInfo);
        if mirrored.signature != nexa_boot_info::BOOT_INFO_SIGNATURE {
            log::warn!(
                "Boot info mirror verification failed at {:#x}; signature bytes: {:?}",
                expected_addr,
                mirrored.signature
            );
            return None;
        }
    }

    Some(expected_addr)
}

fn stage_boot_info(
    bs: &BootServices,
    initramfs: MemoryRegion,
    rootfs: MemoryRegion,
    framebuffer: Option<FramebufferInfo>,
    devices: &DeviceTable,
    kernel_offset: i64,
    expected_entry: u64,
    actual_entry: u64,
    cmdline: &[u8],
    kernel_segments: MemoryRegion,
    acpi_rsdp_addr: Option<u64>,
) -> Result<MemoryRegion, Status> {
    let size_bytes = mem::size_of::<BootInfo>();
    debug_assert!(size_bytes <= u16::MAX as usize);
    let pages = (size_bytes + 0xFFF) / 0x1000;
    let allocation = bs
        .allocate_pages(
            AllocateType::MaxAddress(BOOT_INFO_PREF_MAX_ADDR),
            MemoryType::LOADER_DATA,
            pages,
        )
        .or_else(|_| {
            bs.allocate_pages(
                AllocateType::MaxAddress(MAX_PHYS_ADDR),
                MemoryType::LOADER_DATA,
                pages,
            )
        })
        .map_err(|e: Error| e.status())?;

    let addr = allocation as usize;
    if (addr as u64) > BOOT_INFO_PREF_MAX_ADDR {
        log::warn!(
            "Boot info allocated above preferred range: {:#x}",
            addr
        );
    }

    let cmdline_region = match stage_cmdline(bs, cmdline) {
        Ok(region) => region,
        Err(status) => {
            log::warn!(
                "Failed to stage kernel cmdline (status {:?}); continuing without cmdline",
                status
            );
            MemoryRegion::empty()
        }
    };

    let mut device_entries = [DeviceDescriptor::empty(); MAX_DEVICE_DESCRIPTORS];
    device_entries.copy_from_slice(&devices.entries);

    let mut flags_value = determine_flags(
        &initramfs,
        &rootfs,
        framebuffer.is_some(),
        devices.count != 0,
        !cmdline_region.is_empty(),
    );
    
    // 如果有内核加载偏移，设置标志位
    if kernel_offset != 0 {
        flags_value |= nexa_boot_info::flags::HAS_KERNEL_OFFSET;
        log::info!("Setting HAS_KERNEL_OFFSET flag, offset={:#x}", kernel_offset);
    }

    if !kernel_segments.is_empty() {
        let segment_count = (kernel_segments.length as usize)
            / core::mem::size_of::<KernelSegment>();
        flags_value |= nexa_boot_info::flags::HAS_KERNEL_SEGMENTS;
        log::info!(
            "Setting HAS_KERNEL_SEGMENTS flag ({} entries)",
            segment_count
        );
    }
    
    // 如果有ACPI RSDP地址，设置标志位
    if acpi_rsdp_addr.is_some() {
        flags_value |= nexa_boot_info::flags::HAS_ACPI_RSDP;
        log::info!("Setting HAS_ACPI_RSDP flag, addr={:#x}", acpi_rsdp_addr.unwrap());
    }

    let boot_info = BootInfo {
        signature: nexa_boot_info::BOOT_INFO_SIGNATURE,
        version: nexa_boot_info::BOOT_INFO_VERSION,
        size: size_bytes as u16,
        flags: flags_value,
        initramfs,
        rootfs,
        cmdline: cmdline_region,
        framebuffer: framebuffer.unwrap_or(FramebufferInfo {
            address: 0,
            pitch: 0,
            width: 0,
            height: 0,
            bpp: 0,
            red_position: 0,
            red_size: 0,
            green_position: 0,
            green_size: 0,
            blue_position: 0,
            blue_size: 0,
            reserved: [0; 5],
        }),
        device_count: devices.count,
        _padding: 0,
        devices: device_entries,
        kernel_expected_entry: expected_entry,
        kernel_actual_entry: actual_entry,
        kernel_segments,
        kernel_load_offset: kernel_offset,
        acpi_rsdp_addr: acpi_rsdp_addr.unwrap_or(0),
        reserved: [0; 16],
    };

    unsafe {
        ptr::write(addr as *mut BootInfo, boot_info);
        let total = pages * 0x1000;
        if total > size_bytes {
            ptr::write_bytes((addr + size_bytes) as *mut u8, 0, total - size_bytes);
        }
    }

    Ok(MemoryRegion {
        phys_addr: addr as u64,
        length: size_bytes as u64,
    })
}

fn stage_cmdline(bs: &BootServices, cmdline: &[u8]) -> Result<MemoryRegion, Status> {
    if cmdline.is_empty() {
        return Ok(MemoryRegion::empty());
    }

    let needs_null = cmdline.last().copied() != Some(0);
    let size = cmdline.len() + if needs_null { 1 } else { 0 };
    let pages = (size + 0xFFF) / 0x1000;

    let allocation = bs
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .map_err(|e: Error| e.status())?;
    let addr = allocation as usize;

    unsafe {
        ptr::copy_nonoverlapping(cmdline.as_ptr(), addr as *mut u8, cmdline.len());
        if needs_null {
            ptr::write((addr + cmdline.len()) as *mut u8, 0);
        }
        let total = pages * 0x1000;
        if total > size {
            ptr::write_bytes((addr + size) as *mut u8, 0, total - size);
        }
    }

    Ok(MemoryRegion {
        phys_addr: addr as u64,
        length: size as u64,
    })
}

fn determine_flags(
    initramfs: &MemoryRegion,
    rootfs: &MemoryRegion,
    has_fb: bool,
    has_devices: bool,
    has_cmdline: bool,
) -> u32 {
    let mut flags_val = 0u32;
    if !initramfs.is_empty() {
        flags_val |= flags::HAS_INITRAMFS;
    }
    if !rootfs.is_empty() {
        flags_val |= flags::HAS_ROOTFS;
    }
    if has_fb {
        flags_val |= flags::HAS_FRAMEBUFFER;
    }
    if has_devices {
        flags_val |= flags::HAS_DEVICE_TABLE;
    }
    if has_cmdline {
        flags_val |= flags::HAS_CMDLINE;
    }
    flags_val
}

fn detect_framebuffer(bs: &BootServices, image: Handle) -> Option<FramebufferInfo> {
    let handles = bs
        .locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))
        .ok()?;
    let handle = handles.iter().copied().next()?;
    let gop = unsafe {
        bs.open_protocol::<GraphicsOutput>(
            OpenProtocolParams {
                handle,
                agent: image,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
        .ok()?
    };
    let gop = match gop.get_mut() {
        Some(gop) => gop,
        None => return None,
    };
    let mode = gop.current_mode_info();
    let mut fb = gop.frame_buffer();

    let mask_info = |mask: u32| -> (u8, u8) {
        if mask == 0 {
            (0, 0)
        } else {
            (mask.trailing_zeros() as u8, mask.count_ones() as u8)
        }
    };

    let (bytes_per_pixel, bpp, red_position, red_size, green_position, green_size, blue_position, blue_size) =
        match mode.pixel_format() {
            PixelFormat::Rgb => (4, 32, 0, 8, 8, 8, 16, 8),
            PixelFormat::Bgr => (4, 32, 16, 8, 8, 8, 0, 8),
            PixelFormat::Bitmask => {
                let bitmask = mode.pixel_bitmask()?;
                let (red_position, red_size) = mask_info(bitmask.red);
                let (green_position, green_size) = mask_info(bitmask.green);
                let (blue_position, blue_size) = mask_info(bitmask.blue);
                let total = (red_size as u16 + green_size as u16 + blue_size as u16).min(32) as u8;
                (4, total, red_position, red_size, green_position, green_size, blue_position, blue_size)
            }
            PixelFormat::BltOnly => return None,
        };

    Some(FramebufferInfo {
        address: fb.as_mut_ptr() as u64,
        pitch: mode.stride() as u32 * bytes_per_pixel as u32,
        width: mode.resolution().0 as u32,
        height: mode.resolution().1 as u32,
        bpp,
        red_position,
        red_size,
        green_position,
        green_size,
        blue_position,
        blue_size,
        reserved: [0; 5],
    })
}

fn find_uefi_entry(image: &[u8]) -> Option<u64> {
    log::info!("Searching for .nexa.uefi_entry section in kernel ELF");
    if image.len() < mem::size_of::<Elf64Ehdr>() {
        log::error!("Image too small for ELF header");
        return None;
    }

    let header = unsafe { &*(image.as_ptr() as *const Elf64Ehdr) };
    let shoff = header.e_shoff as usize;
    let shentsize = header.e_shentsize as usize;
    let shnum = header.e_shnum as usize;
    let shstrndx = header.e_shstrndx as usize;

    log::info!("ELF section header: offset={:#x}, count={}, size={}, str_index={}", shoff, shnum, shentsize, shstrndx);

    if shoff == 0 || shentsize == 0 || shnum == 0 {
        log::error!("Invalid section header table");
        return None;
    }
    if shoff + shentsize.saturating_mul(shnum) > image.len() {
        log::error!("Section headers extend beyond image");
        return None;
    }
    if shstrndx >= shnum {
        log::error!("Invalid string table index");
        return None;
    }

    let section = |idx: usize| -> &Elf64Shdr {
        let offset = shoff + idx * shentsize;
        unsafe { &*(image.as_ptr().add(offset) as *const Elf64Shdr) }
    };

    let shstr = section(shstrndx);
    let str_offset = shstr.sh_offset as usize;
    let str_size = shstr.sh_size as usize;
    log::info!("String table: offset={:#x}, size={}", str_offset, str_size);
    if str_offset.saturating_add(str_size) > image.len() {
        log::error!("String table extends beyond image");
        return None;
    }
    let strtab = &image[str_offset..str_offset + str_size];

    log::info!("Scanning {} sections for .nexa.uefi_entry...", shnum);
    for idx in 0..shnum {
        let sh = section(idx);
        let name_offset = sh.sh_name as usize;
        if name_offset >= strtab.len() {
            continue;
        }
        let name = read_cstr(&strtab[name_offset..]);
        if name == ".nexa.uefi_entry" {
            let off = sh.sh_offset as usize;
            let size = sh.sh_size as usize;
            if off.saturating_add(size) > image.len() || size < 8 {
                return None;
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&image[off..off + 8]);
            log::info!("Raw bytes from .nexa.uefi_entry: {:02x?}", bytes);
            let entry_addr = u64::from_le_bytes(bytes);
            log::info!("Found UEFI entry point in ELF: {:#x}", entry_addr);
            // 验证地址是否合理
            if entry_addr < 0x100000 || entry_addr > 0x2000000 {
                log::warn!("UEFI entry point address seems invalid: {:#x}", entry_addr);
            }
            return Some(entry_addr);
        }
    }

    log::error!(".nexa.uefi_entry section not found in kernel ELF!");
    None
}

fn read_cstr(data: &[u8]) -> &str {
    let nul = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    unsafe { core::str::from_utf8_unchecked(&data[..nul]) }
}

#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[repr(C)]
struct Elf64Shdr {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
}
