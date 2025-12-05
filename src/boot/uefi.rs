use nexa_boot_info::{
    bar_flags, BlockDeviceInfo, FramebufferInfo, HidInputInfo, NetworkDeviceInfo, PciBarInfo,
    PciDeviceInfo, UsbHostInfo,
};
use spin::Mutex;

use crate::bootinfo;
use crate::fs;
use crate::paging;
use crate::posix::{FileType, Metadata};

const MAX_NETWORK_DEVICES: usize = 8;
const MAX_BLOCK_DEVICES: usize = 8;
const MAX_USB_HOSTS: usize = 8;
const MAX_HID_DEVICES: usize = 8;

const DEV_NET_PATHS: [&str; MAX_NETWORK_DEVICES] = [
    "/dev/net0",
    "/dev/net1",
    "/dev/net2",
    "/dev/net3",
    "/dev/net4",
    "/dev/net5",
    "/dev/net6",
    "/dev/net7",
];

const DEV_BLOCK_PATHS: [&str; MAX_BLOCK_DEVICES] = [
    "/dev/block0",
    "/dev/block1",
    "/dev/block2",
    "/dev/block3",
    "/dev/block4",
    "/dev/block5",
    "/dev/block6",
    "/dev/block7",
];

const DEV_USB_PATHS: [&str; MAX_USB_HOSTS] = [
    "/dev/usb0",
    "/dev/usb1",
    "/dev/usb2",
    "/dev/usb3",
    "/dev/usb4",
    "/dev/usb5",
    "/dev/usb6",
    "/dev/usb7",
];

const DEV_HID_PATHS: [&str; MAX_HID_DEVICES] = [
    "/dev/hid0",
    "/dev/hid1",
    "/dev/hid2",
    "/dev/hid3",
    "/dev/hid4",
    "/dev/hid5",
    "/dev/hid6",
    "/dev/hid7",
];

#[derive(Clone, Copy, Default)]
struct FramebufferState {
    info: Option<FramebufferInfo>,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CompatCounts {
    pub framebuffer: u8,
    pub network: u8,
    pub block: u8,
    pub usb_host: u8,
    pub hid_input: u8,
    pub _reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UsbHostDescriptor {
    pub info: UsbHostInfo,
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub interrupt_line: u8,
    pub _reserved: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HidInputDescriptor {
    pub info: HidInputInfo,
    pub _reserved: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetworkDescriptor {
    pub info: NetworkDeviceInfo,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub bar_flags: u32,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub _reserved: [u8; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockDescriptor {
    pub info: BlockDeviceInfo,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub bar_flags: u32,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub _reserved: [u8; 2],
}

impl Default for NetworkDescriptor {
    fn default() -> Self {
        Self {
            info: NetworkDeviceInfo::empty(),
            mmio_base: 0,
            mmio_length: 0,
            bar_flags: 0,
            interrupt_line: 0,
            interrupt_pin: 0,
            _reserved: [0; 2],
        }
    }
}

impl Default for BlockDescriptor {
    fn default() -> Self {
        Self {
            info: BlockDeviceInfo::empty(),
            mmio_base: 0,
            mmio_length: 0,
            bar_flags: 0,
            interrupt_line: 0,
            interrupt_pin: 0,
            _reserved: [0; 2],
        }
    }
}

impl Default for UsbHostDescriptor {
    fn default() -> Self {
        Self {
            info: UsbHostInfo::empty(),
            mmio_base: 0,
            mmio_size: 0,
            interrupt_line: 0,
            _reserved: [0; 7],
        }
    }
}

impl Default for HidInputDescriptor {
    fn default() -> Self {
        Self {
            info: HidInputInfo::empty(),
            _reserved: [0; 16],
        }
    }
}

static FRAMEBUFFER: Mutex<FramebufferState> = Mutex::new(FramebufferState { info: None });
static NETWORK_DEVICES: Mutex<[Option<NetworkDescriptor>; MAX_NETWORK_DEVICES]> =
    Mutex::new([None; MAX_NETWORK_DEVICES]);
static BLOCK_DEVICES: Mutex<[Option<BlockDescriptor>; MAX_BLOCK_DEVICES]> =
    Mutex::new([None; MAX_BLOCK_DEVICES]);
static USB_HOST_DEVICES: Mutex<[Option<UsbHostDescriptor>; MAX_USB_HOSTS]> =
    Mutex::new([None; MAX_USB_HOSTS]);
static HID_INPUT_DEVICES: Mutex<[Option<HidInputDescriptor>; MAX_HID_DEVICES]> =
    Mutex::new([None; MAX_HID_DEVICES]);

fn map_mmio_region(base: u64, length: u64, label: &str) {
    if base == 0 {
        return;
    }

    let span = if length == 0 { 0x1000 } else { length };
    let clamped = span.min(usize::MAX as u64) as usize;

    unsafe {
        match paging::map_device_region(base, clamped) {
            Ok(_) => {
                crate::kdebug!(
                    "uefi_compat: mapped {} region {:#x}+{:#x}",
                    label,
                    base,
                    span
                );
            }
            Err(paging::MapDeviceError::OutOfTableSpace) => {
                crate::kwarn!(
                    "uefi_compat: failed to map {} region {:#x}+{:#x}: out of paging tables",
                    label,
                    base,
                    span
                );
            }
        }
    }
}

pub fn reset() {
    *FRAMEBUFFER.lock() = FramebufferState { info: None };

    let mut nets = NETWORK_DEVICES.lock();
    for entry in nets.iter_mut() {
        *entry = None;
    }
    drop(nets);

    let mut blocks = BLOCK_DEVICES.lock();
    for entry in blocks.iter_mut() {
        *entry = None;
    }
    drop(blocks);

    let mut usbs = USB_HOST_DEVICES.lock();
    for entry in usbs.iter_mut() {
        *entry = None;
    }
    drop(usbs);

    let mut hids = HID_INPUT_DEVICES.lock();
    for entry in hids.iter_mut() {
        *entry = None;
    }
}

pub fn init() {
    {
        let mut fb_state = FRAMEBUFFER.lock();
        fb_state.info = bootinfo::framebuffer_info();

        if let Some(info) = fb_state.info {
            let size = (info.pitch as u64) * (info.height as u64);
            map_mmio_region(info.address, size, "framebuffer");
        }
    }

    populate_network_devices();
    populate_block_devices();
    populate_usb_host_devices();
    populate_hid_input_devices();
}

pub fn install_device_nodes() {
    if let Some(counts) = nonzero_counts() {
        if counts.network > 0 {
            for idx in 0..(counts.network as usize) {
                if let Some(descriptor) = network_descriptor(idx) {
                    let path = DEV_NET_PATHS[idx];
                    register_device_node(path, FileType::Character, 0o660);
                    crate::kinfo!(
                        "Registered {} (MMIO base={:#x}, len={:#x})",
                        path,
                        descriptor.mmio_base,
                        descriptor.mmio_length
                    );
                    // Pass descriptor to network subsystem
                    crate::net::ingest_boot_descriptor(idx, descriptor);
                }
            }
        }

        if counts.block > 0 {
            for idx in 0..(counts.block as usize) {
                if let Some(descriptor) = block_descriptor(idx) {
                    let path = DEV_BLOCK_PATHS[idx];
                    register_device_node(path, FileType::Block, 0o660);
                    crate::kinfo!(
                        "Registered {} (block size={}, last_lba={})",
                        path,
                        descriptor.info.block_size,
                        descriptor.info.last_block
                    );
                }
            }
        }

        if counts.usb_host > 0 {
            for idx in 0..(counts.usb_host as usize) {
                if let Some(descriptor) = usb_host_descriptor(idx) {
                    let path = DEV_USB_PATHS[idx];
                    register_device_node(path, FileType::Character, 0o660);
                    crate::kinfo!(
                        "Registered {} (USB{}, MMIO={:#x}, size={:#x})",
                        path,
                        match descriptor.info.controller_type {
                            1 => "OHCI",
                            2 => "EHCI",
                            3 => "xHCI",
                            _ => "Unknown",
                        },
                        descriptor.mmio_base,
                        descriptor.mmio_size
                    );
                }
            }
        }

        if counts.hid_input > 0 {
            for idx in 0..(counts.hid_input as usize) {
                if let Some(descriptor) = hid_input_descriptor(idx) {
                    let path = DEV_HID_PATHS[idx];
                    register_device_node(path, FileType::Character, 0o660);
                    crate::kinfo!(
                        "Registered {} ({}, protocol={})",
                        path,
                        match descriptor.info.device_type {
                            1 => "keyboard",
                            2 => "mouse",
                            3 => "combined",
                            _ => "unknown",
                        },
                        descriptor.info.protocol
                    );
                }
            }
        }

        if counts.framebuffer != 0 {
            register_device_node("/dev/fb0", FileType::Character, 0o660);
            crate::kinfo!("Registered /dev/fb0 for framebuffer access");
        }
    }
}

/// Register dynamic devices to devfs after pivot_root
/// This is called when devfs is mounted at /dev
pub fn register_devfs_devices() {
    if let Some(counts) = nonzero_counts() {
        // Register network devices
        for idx in 0..(counts.network as u8) {
            crate::fs::register_network_device(idx);
        }

        // Register block devices
        for idx in 0..(counts.block as u8) {
            crate::fs::register_block_device(idx);
        }

        // Register framebuffer
        if counts.framebuffer != 0 {
            crate::fs::register_framebuffer_device(0);
        }

        crate::kinfo!(
            "Registered {} network, {} block, {} framebuffer devices to devfs",
            counts.network,
            counts.block,
            counts.framebuffer
        );
    }
}

pub fn counts() -> CompatCounts {
    let fb = FRAMEBUFFER.lock();
    let fb_count = fb.info.map(|_| 1).unwrap_or(0);
    drop(fb);

    let nets = NETWORK_DEVICES.lock();
    let net_count = nets.iter().filter(|entry| entry.is_some()).count() as u8;
    drop(nets);

    let blocks = BLOCK_DEVICES.lock();
    let block_count = blocks.iter().filter(|entry| entry.is_some()).count() as u8;
    drop(blocks);

    let usbs = USB_HOST_DEVICES.lock();
    let usb_count = usbs.iter().filter(|entry| entry.is_some()).count() as u8;
    drop(usbs);

    let hids = HID_INPUT_DEVICES.lock();
    let hid_count = hids.iter().filter(|entry| entry.is_some()).count() as u8;

    CompatCounts {
        framebuffer: fb_count,
        network: net_count,
        block: block_count,
        usb_host: usb_count,
        hid_input: hid_count,
        _reserved: [0; 3],
    }
}

pub fn framebuffer() -> Option<FramebufferInfo> {
    FRAMEBUFFER.lock().info
}

pub fn network_descriptor(index: usize) -> Option<NetworkDescriptor> {
    let nets = NETWORK_DEVICES.lock();
    nets.get(index).and_then(|entry| *entry)
}

pub fn block_descriptor(index: usize) -> Option<BlockDescriptor> {
    let blocks = BLOCK_DEVICES.lock();
    blocks.get(index).and_then(|entry| *entry)
}

pub fn usb_host_descriptor(index: usize) -> Option<UsbHostDescriptor> {
    let usbs = USB_HOST_DEVICES.lock();
    usbs.get(index).and_then(|entry| *entry)
}

pub fn hid_input_descriptor(index: usize) -> Option<HidInputDescriptor> {
    let hids = HID_INPUT_DEVICES.lock();
    hids.get(index).and_then(|entry| *entry)
}

fn nonzero_counts() -> Option<CompatCounts> {
    let counts = counts();
    if counts.framebuffer == 0 && counts.network == 0 && counts.block == 0 {
        None
    } else {
        Some(counts)
    }
}

fn populate_network_devices() {
    let mut nets = NETWORK_DEVICES.lock();
    for entry in nets.iter_mut() {
        *entry = None;
    }

    let Some(iter) = bootinfo::network_devices() else {
        return;
    };

    for (idx, net) in iter.enumerate() {
        if idx >= MAX_NETWORK_DEVICES {
            crate::kwarn!(
                "uefi_compat: network device table full (max {})",
                MAX_NETWORK_DEVICES
            );
            break;
        }

        let Some(pci) = bootinfo::pci_device_by_location(
            net.pci_segment,
            net.pci_bus,
            net.pci_device,
            net.pci_function,
        ) else {
            crate::kwarn!(
                "uefi_compat: PCI info missing for network device {:04x}:{:02x}:{:02x}.{}",
                net.pci_segment,
                net.pci_bus,
                net.pci_device,
                net.pci_function
            );
            continue;
        };

        let mmio = select_mmio_bar(pci);
        let descriptor = NetworkDescriptor {
            info: *net,
            mmio_base: mmio.map(|bar| bar.base).unwrap_or(0),
            mmio_length: mmio.and_then(normalized_length).unwrap_or(0),
            bar_flags: mmio.map(|bar| bar.bar_flags).unwrap_or(0),
            interrupt_line: pci.interrupt_line,
            interrupt_pin: pci.interrupt_pin,
            _reserved: [0; 2],
        };

        if descriptor.mmio_base != 0 {
            map_mmio_region(
                descriptor.mmio_base,
                descriptor.mmio_length,
                "network device",
            );
        }

        nets[idx] = Some(descriptor);
    }
}

fn populate_block_devices() {
    let mut blocks = BLOCK_DEVICES.lock();
    for entry in blocks.iter_mut() {
        *entry = None;
    }

    let Some(iter) = bootinfo::block_devices() else {
        return;
    };

    for (idx, block) in iter.enumerate() {
        if idx >= MAX_BLOCK_DEVICES {
            crate::kwarn!(
                "uefi_compat: block device table full (max {})",
                MAX_BLOCK_DEVICES
            );
            break;
        }

        let pci = bootinfo::pci_device_by_location(
            block.pci_segment,
            block.pci_bus,
            block.pci_device,
            block.pci_function,
        );

        let mmio = pci.and_then(select_mmio_bar);
        let (bar_flags, interrupt_line, interrupt_pin) = pci
            .map(|p| {
                (
                    mmio.map(|bar| bar.bar_flags).unwrap_or(0),
                    p.interrupt_line,
                    p.interrupt_pin,
                )
            })
            .unwrap_or((0, 0, 0));

        let descriptor = BlockDescriptor {
            info: *block,
            mmio_base: mmio.map(|bar| bar.base).unwrap_or(0),
            mmio_length: mmio.and_then(normalized_length).unwrap_or(0),
            bar_flags,
            interrupt_line,
            interrupt_pin,
            _reserved: [0; 2],
        };

        if descriptor.mmio_base != 0 {
            map_mmio_region(descriptor.mmio_base, descriptor.mmio_length, "block device");
        }

        blocks[idx] = Some(descriptor);
    }
}

fn populate_usb_host_devices() {
    let mut usbs = USB_HOST_DEVICES.lock();
    for entry in usbs.iter_mut() {
        *entry = None;
    }

    let Some(iter) = bootinfo::usb_host_devices() else {
        return;
    };

    for (idx, usb) in iter.enumerate() {
        if idx >= MAX_USB_HOSTS {
            crate::kwarn!(
                "uefi_compat: USB host device table full (max {})",
                MAX_USB_HOSTS
            );
            break;
        }

        let descriptor = UsbHostDescriptor {
            info: *usb,
            mmio_base: usb.mmio_base,
            mmio_size: usb.mmio_size,
            interrupt_line: usb.interrupt_line,
            _reserved: [0; 7],
        };

        if descriptor.mmio_base != 0 && descriptor.mmio_size != 0 {
            map_mmio_region(
                descriptor.mmio_base,
                descriptor.mmio_size,
                "USB host controller",
            );
        }

        usbs[idx] = Some(descriptor);
    }
}

fn populate_hid_input_devices() {
    let mut hids = HID_INPUT_DEVICES.lock();
    for entry in hids.iter_mut() {
        *entry = None;
    }

    let Some(iter) = bootinfo::hid_input_devices() else {
        return;
    };

    for (idx, hid) in iter.enumerate() {
        if idx >= MAX_HID_DEVICES {
            crate::kwarn!(
                "uefi_compat: HID input device table full (max {})",
                MAX_HID_DEVICES
            );
            break;
        }

        let descriptor = HidInputDescriptor {
            info: *hid,
            _reserved: [0; 16],
        };

        hids[idx] = Some(descriptor);
    }
}

fn register_device_node(path: &'static str, file_type: FileType, mode: u16) {
    let mut meta = Metadata::empty().with_type(file_type).with_mode(mode);
    meta.blocks = 0;
    meta.size = 0;
    fs::add_file_with_metadata(path, b"", false, meta);
}

fn normalized_length(bar: PciBarInfo) -> Option<u64> {
    if bar.length != 0 {
        Some(bar.length)
    } else if bar.base != 0 {
        Some(0x1000)
    } else {
        None
    }
}

fn select_mmio_bar(pci: &PciDeviceInfo) -> Option<PciBarInfo> {
    pci.bars
        .iter()
        .copied()
        .filter(|bar| bar.base != 0)
        .filter(|bar| (bar.bar_flags & bar_flags::IO_SPACE) == 0)
        .find(|bar| bar.length != 0)
        .or_else(|| {
            pci.bars
                .iter()
                .copied()
                .filter(|bar| bar.base != 0)
                .filter(|bar| (bar.bar_flags & bar_flags::IO_SPACE) == 0)
                .next()
        })
}
