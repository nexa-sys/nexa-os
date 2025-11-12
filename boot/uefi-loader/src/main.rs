#![no_std]
#![no_main]

extern crate alloc;

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
    NetworkDeviceInfo,
    PciBarInfo,
    PciDeviceInfo,
    MAX_DEVICE_DESCRIPTORS,
};
use r_efi::base as raw_base;
use r_efi::protocols::pci_io;
use uefi::prelude::*;
use uefi::Guid;
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat};
use uefi::proto::device_path::{DevicePath, DevicePathNodeEnum};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::block::BlockIO;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileMode, RegularFile};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::network::snp::{NetworkState, SimpleNetwork};
use uefi::proto::unsafe_protocol;
use uefi::table::boot::{AllocateType, MemoryType, OpenProtocolAttributes, OpenProtocolParams, SearchType};
use uefi::Identify;
use uefi::Error;
use uefi::{cstr16, Handle, Status};

const KERNEL_PATH: &uefi::CStr16 = cstr16!("\\EFI\\NEXAOS\\KERNEL.ELF");
const INITRAMFS_PATH: &uefi::CStr16 = cstr16!("\\EFI\\NEXAOS\\INITRAMFS.CPIO");
const ROOTFS_PATH: &uefi::CStr16 = cstr16!("\\EFI\\NEXAOS\\ROOTFS.EXT2");
const MAX_PHYS_ADDR: u64 = 0x0000FFFF_FFFF;

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

// Convert the r_efi GUID to the `uefi::Guid` type expected by the
// `unsafe_protocol` macro. `pci_io::PROTOCOL_GUID` is a
// `r_efi::base::Guid` so we copy its fields into a `uefi::Guid`
// constant and reference that in the attribute.
const PCI_IO_GUID: Guid = Guid::from_bytes(*pci_io::PROTOCOL_GUID.as_bytes());

#[repr(transparent)]
#[unsafe_protocol(PCI_IO_GUID)]
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
        self as *const _ as *mut pci_io::Protocol
    }

    fn read_config_u32(&self, offset: u32) -> Result<u32, Status> {
        let mut value = 0u32;
        let status = unsafe {
            (self.0.pci.read)(
                self.protocol_mut(),
                pci_io::WIDTH_UINT32,
                offset,
                1,
                (&mut value as *mut u32).cast::<c_void>(),
            )
        };
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn read_config_u16(&self, offset: u32) -> Result<u16, Status> {
        let mut value = 0u16;
        let status = unsafe {
            (self.0.pci.read)(
                self.protocol_mut(),
                pci_io::WIDTH_UINT16,
                offset,
                1,
                (&mut value as *mut u16).cast::<c_void>(),
            )
        };
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
    }

    fn read_config_u8(&self, offset: u32) -> Result<u8, Status> {
        let mut value = 0u8;
        let status = unsafe {
            (self.0.pci.read)(
                self.protocol_mut(),
                pci_io::WIDTH_UINT8,
                offset,
                1,
                (&mut value as *mut u8).cast::<c_void>(),
            )
        };
        if status == raw_base::Status::SUCCESS {
            Ok(value)
        } else {
            Err(Status(status.as_usize()))
        }
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
            continue;
        };
        let Some(snp) = snp_proto.get() else {
            continue;
        };
        let mode = snp.mode();

        let pci_snapshot = pci_snapshot_for_handle(bs, image, *handle);
        table.ensure_pci_entry(address, device_flags::NETWORK, |pci| {
            if let Some(snapshot) = pci_snapshot.as_ref() {
                apply_pci_snapshot(pci, snapshot);
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
    let status = unsafe {
        (pci_io.0.get_bar_attributes)(
            pci_io.protocol_mut(),
            bar_index,
            &mut _attributes,
            &mut resource_ptr,
        )
    };
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

    let descriptor = unsafe { ptr::read_unaligned(ptr.cast::<AcpiAddressSpaceDescriptor>()) };
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
                    segment = (acpi_node.uid() >> 16) as u16;
                }
            }
            DevicePathNodeEnum::AcpiExpanded(expanded) => {
                if is_pci_root_hid(expanded.hid()) {
                    bus = (expanded.uid() & 0xFF) as u8;
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

#[entry]
fn efi_main(image: Handle, mut st: SystemTable<Boot>) -> Status {
    if let Err(e) = uefi::helpers::init(&mut st) {
        return e.status();
    }

    log::info!("NexaOS UEFI loader starting");

    let bs = st.boot_services();

    let mut root = match open_boot_volume(bs, image) {
        Ok(dir) => dir,
        Err(status) => return status,
    };

    let kernel_bytes = match read_file(&mut root, KERNEL_PATH) {
        Ok(data) => data,
        Err(status) => {
            log::error!("Failed to load kernel image: {:?}", status);
            return status;
        }
    };

    let initramfs_bytes = match read_file(&mut root, INITRAMFS_PATH) {
        Ok(data) => data,
        Err(status) if status == Status::NOT_FOUND => Vec::new(),
        Err(status) => {
            log::error!("Failed to load initramfs: {:?}", status);
            return status;
        }
    };

    let rootfs_bytes = match read_file(&mut root, ROOTFS_PATH) {
        Ok(data) => data,
        Err(status) if status == Status::NOT_FOUND => Vec::new(),
        Err(status) => {
            log::error!("Failed to load rootfs image: {:?}", status);
            return status;
        }
    };

    drop(root);

    let loaded = match load_kernel_image(bs, &kernel_bytes) {
        Ok(info) => info,
        Err(status) => {
            log::error!("Kernel load failed: {:?}", status);
            return status;
        }
    };

    let initramfs_region = match stage_payload(bs, &initramfs_bytes, MemoryType::LOADER_DATA) {
        Ok(region) => region,
        Err(status) => {
            log::error!("Failed to allocate initramfs region: {:?}", status);
            return status;
        }
    };

    let rootfs_region = match stage_payload(bs, &rootfs_bytes, MemoryType::LOADER_DATA) {
        Ok(region) => region,
        Err(status) => {
            log::error!("Failed to allocate rootfs region: {:?}", status);
            return status;
        }
    };

    let framebuffer = detect_framebuffer(bs, image);
    let device_table = collect_device_table(bs, image);

    let boot_info_region = match stage_boot_info(bs, initramfs_region, rootfs_region, framebuffer, &device_table) {
        Ok(region) => region,
        Err(status) => {
            log::error!("Failed to allocate boot info block: {:?}", status);
            return status;
        }
    };

    let (_runtime_st, _) = st.exit_boot_services(MemoryType::LOADER_DATA);

    log::info!(
        "Transferring control to kernel UEFI entry at {:#x}",
        loaded.uefi_entry_point
    );

    unsafe {
        let entry: extern "C" fn(*const BootInfo) -> ! = mem::transmute(loaded.uefi_entry_point);
        entry(boot_info_region.phys_addr as *const BootInfo)
    }
}

struct LoadedKernel {
    uefi_entry_point: u64,
}

fn open_boot_volume(bs: &BootServices, image: Handle) -> Result<Directory, Status> {
    let loaded_image = unsafe {
        bs.open_protocol::<LoadedImage>(
            OpenProtocolParams {
                handle: image,
                agent: image,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    .map_err(|e: Error| e.status())?
    };
    let loaded_image_ref = loaded_image
        .get()
        .ok_or(Status::UNSUPPORTED)?;
    let device_handle = loaded_image_ref
        .device()
        .ok_or(Status::UNSUPPORTED)?;

    let fs = unsafe {
        bs.open_protocol::<SimpleFileSystem>(
            OpenProtocolParams {
                handle: device_handle,
                agent: image,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    .map_err(|e: Error| e.status())?
    };

    let file_system = fs.get_mut().ok_or(Status::UNSUPPORTED)?;
    file_system
        .open_volume()
        .map_err(|e: Error| e.status())
}

fn read_file(root: &mut Directory, path: &uefi::CStr16) -> Result<Vec<u8>, Status> {
    let handle = root
    .open(path, FileMode::Read, FileAttribute::empty())
    .map_err(|e: Error| e.status())?;
    let Some(file) = handle.into_regular_file() else {
        return Err(Status::UNSUPPORTED);
    };
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

    for i in 0..phnum {
        let offset = phoff + i * phentsize;
        if offset + mem::size_of::<Elf64Phdr>() > image.len() {
            return Err(Status::LOAD_ERROR);
        }
        let ph = unsafe { &*(image.as_ptr().add(offset) as *const Elf64Phdr) };
        if ph.p_type != 1 {
            continue;
        }

        let dest = ph.p_paddr as usize;
        let memsz = ph.p_memsz as usize;
        let filesz = ph.p_filesz as usize;
        if memsz == 0 {
            continue;
        }

        let pages = (memsz + 0xFFF) / 0x1000;
        bs
            .allocate_pages(
                AllocateType::Address(dest as u64),
                MemoryType::LOADER_DATA,
                pages,
            )
            .map_err(|e: Error| e.status())?;

        if filesz > 0 {
            let src_offset = ph.p_offset as usize;
            if src_offset + filesz > image.len() {
                return Err(Status::LOAD_ERROR);
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    image.as_ptr().add(src_offset),
                    dest as *mut u8,
                    filesz,
                );
            }
        }

        if memsz > filesz {
            unsafe {
                ptr::write_bytes((dest + filesz) as *mut u8, 0, memsz - filesz);
            }
        }
    }

    let uefi_entry_point = match find_uefi_entry(image) {
        Some(ptr) => ptr,
        None => {
            log::error!("Kernel image missing .nexa.uefi_entry section");
            return Err(Status::LOAD_ERROR);
        }
    };

    Ok(LoadedKernel { uefi_entry_point })
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

fn stage_boot_info(
    bs: &BootServices,
    initramfs: MemoryRegion,
    rootfs: MemoryRegion,
    framebuffer: Option<FramebufferInfo>,
    devices: &DeviceTable,
) -> Result<MemoryRegion, Status> {
    let size_bytes = mem::size_of::<BootInfo>();
    debug_assert!(size_bytes <= u16::MAX as usize);
    let pages = (size_bytes + 0xFFF) / 0x1000;
    let addr = bs
        .allocate_pages(
            AllocateType::MaxAddress(MAX_PHYS_ADDR),
            MemoryType::LOADER_DATA,
            pages,
        )
        .map_err(|e: Error| e.status())? as usize;

    let mut device_entries = [DeviceDescriptor::empty(); MAX_DEVICE_DESCRIPTORS];
    device_entries.copy_from_slice(&devices.entries);

    let flags_value = determine_flags(&initramfs, &rootfs, framebuffer.is_some(), devices.count != 0);

    let boot_info = BootInfo {
        signature: nexa_boot_info::BOOT_INFO_SIGNATURE,
        version: nexa_boot_info::BOOT_INFO_VERSION,
        size: size_bytes as u16,
        flags: flags_value,
        initramfs,
        rootfs,
        cmdline: MemoryRegion::empty(),
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
        reserved: [0; 64],
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

fn determine_flags(
    initramfs: &MemoryRegion,
    rootfs: &MemoryRegion,
    has_fb: bool,
    has_devices: bool,
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
    if image.len() < mem::size_of::<Elf64Ehdr>() {
        return None;
    }

    let header = unsafe { &*(image.as_ptr() as *const Elf64Ehdr) };
    let shoff = header.e_shoff as usize;
    let shentsize = header.e_shentsize as usize;
    let shnum = header.e_shnum as usize;
    let shstrndx = header.e_shstrndx as usize;

    if shoff == 0 || shentsize == 0 || shnum == 0 {
        return None;
    }
    if shoff + shentsize.saturating_mul(shnum) > image.len() {
        return None;
    }
    if shstrndx >= shnum {
        return None;
    }

    let section = |idx: usize| -> &Elf64Shdr {
        let offset = shoff + idx * shentsize;
        unsafe { &*(image.as_ptr().add(offset) as *const Elf64Shdr) }
    };

    let shstr = section(shstrndx);
    let str_offset = shstr.sh_offset as usize;
    let str_size = shstr.sh_size as usize;
    if str_offset.saturating_add(str_size) > image.len() {
        return None;
    }
    let strtab = &image[str_offset..str_offset + str_size];

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
            return Some(u64::from_le_bytes(bytes));
        }
    }

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
