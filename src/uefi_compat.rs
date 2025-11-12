use nexa_boot_info::{
    bar_flags, BlockDeviceInfo, FramebufferInfo, NetworkDeviceInfo, PciBarInfo, PciDeviceInfo,
};
use spin::Mutex;

use crate::bootinfo;
use crate::fs;
use crate::posix::{FileType, Metadata};

const MAX_NETWORK_DEVICES: usize = 8;
const MAX_BLOCK_DEVICES: usize = 8;

const DEV_NET_PATHS: [&str; MAX_NETWORK_DEVICES] = [
    "/dev/net0", "/dev/net1", "/dev/net2", "/dev/net3", "/dev/net4", "/dev/net5", "/dev/net6",
    "/dev/net7",
];

const DEV_BLOCK_PATHS: [&str; MAX_BLOCK_DEVICES] = [
    "/dev/block0", "/dev/block1", "/dev/block2", "/dev/block3", "/dev/block4", "/dev/block5",
    "/dev/block6", "/dev/block7",
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
    pub _reserved: u8,
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

static FRAMEBUFFER: Mutex<FramebufferState> = Mutex::new(FramebufferState { info: None });
static NETWORK_DEVICES: Mutex<[Option<NetworkDescriptor>; MAX_NETWORK_DEVICES]> =
    Mutex::new([None; MAX_NETWORK_DEVICES]);
static BLOCK_DEVICES: Mutex<[Option<BlockDescriptor>; MAX_BLOCK_DEVICES]> =
    Mutex::new([None; MAX_BLOCK_DEVICES]);

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
}

pub fn init() {
    {
        let mut fb_state = FRAMEBUFFER.lock();
        fb_state.info = bootinfo::framebuffer_info();
    }

    populate_network_devices();
    populate_block_devices();
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

        if counts.framebuffer != 0 {
            register_device_node("/dev/fb0", FileType::Character, 0o660);
            crate::kinfo!("Registered /dev/fb0 for framebuffer access");
        }
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

    CompatCounts {
        framebuffer: fb_count,
        network: net_count,
        block: block_count,
        _reserved: 0,
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
            .map(|p| (mmio.map(|bar| bar.bar_flags).unwrap_or(0), p.interrupt_line, p.interrupt_pin))
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

        blocks[idx] = Some(descriptor);
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

