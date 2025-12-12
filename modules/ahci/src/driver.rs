//! AHCI Driver Entry Points

use crate::regs::*;
use crate::port::AhciPort;
use crate::{kmod_mmio_read32, kmod_mmio_write32, kmod_zalloc, kmod_dealloc};
use crate::{kmod_spinlock_init, kmod_spinlock_lock, kmod_spinlock_unlock};
use crate::{kmod_blk_register, kmod_blk_unregister, kmod_phys_to_virt};
use crate::{mod_info, mod_error};
use core::ptr;

// ============================================================================
// FFI Types
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootBlockDevice {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub sector_size: u32,
    pub total_sectors: u64,
    pub features: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockDeviceInfo {
    pub name: [u8; 16],
    pub sector_size: u32,
    pub total_sectors: u64,
    pub read_only: bool,
    pub removable: bool,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BlockDeviceHandle(pub *mut u8);

pub type FnBlkProbe = extern "C" fn(u16, u16) -> i32;
pub type FnBlkNew = extern "C" fn(*const BootBlockDevice) -> BlockDeviceHandle;
pub type FnBlkDestroy = extern "C" fn(BlockDeviceHandle);
pub type FnBlkInit = extern "C" fn(BlockDeviceHandle) -> i32;
pub type FnBlkGetInfo = extern "C" fn(BlockDeviceHandle, *mut BlockDeviceInfo) -> i32;
pub type FnBlkRead = extern "C" fn(BlockDeviceHandle, u64, u32, *mut u8) -> i32;
pub type FnBlkWrite = extern "C" fn(BlockDeviceHandle, u64, u32, *const u8) -> i32;
pub type FnBlkFlush = extern "C" fn(BlockDeviceHandle) -> i32;

#[repr(C)]
pub struct BlockDriverOps {
    pub name: [u8; 32],
    pub probe: Option<FnBlkProbe>,
    pub new: Option<FnBlkNew>,
    pub destroy: Option<FnBlkDestroy>,
    pub init: Option<FnBlkInit>,
    pub get_info: Option<FnBlkGetInfo>,
    pub read: Option<FnBlkRead>,
    pub write: Option<FnBlkWrite>,
    pub flush: Option<FnBlkFlush>,
}

// ============================================================================
// AHCI Device
// ============================================================================

#[repr(C)]
struct AhciDevice {
    abar: u64,           // AHCI Base Address (virtual)
    port: AhciPort,
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
}

// ============================================================================
// Driver Callbacks
// ============================================================================

extern "C" fn ahci_probe(vendor_id: u16, device_id: u16) -> i32 {
    // Common AHCI controller IDs
    // Intel: 8086:2922, 8086:2829, etc.
    // QEMU: 8086:2922
    if vendor_id == 0x8086 {
        match device_id {
            0x2922 | 0x2829 | 0x1C02 | 0x1C03 | 0xA102 | 0xA103 => return 0,
            _ => {}
        }
    }
    // VirtIO SCSI is not AHCI
    if vendor_id == 0 && device_id == 0 {
        return 0; // Legacy probe
    }
    -1
}

extern "C" fn ahci_new(desc: *const BootBlockDevice) -> BlockDeviceHandle {
    if desc.is_null() {
        return BlockDeviceHandle(ptr::null_mut());
    }

    let desc = unsafe { &*desc };
    mod_info!(b"ahci: Creating device\n");

    let dev = unsafe {
        let p = kmod_zalloc(core::mem::size_of::<AhciDevice>(), 8) as *mut AhciDevice;
        if p.is_null() {
            mod_error!(b"ahci: Alloc failed\n");
            return BlockDeviceHandle(ptr::null_mut());
        }
        &mut *p
    };

    // Map MMIO
    dev.abar = unsafe { kmod_phys_to_virt(desc.mmio_base) };
    dev.pci_bus = desc.pci_bus;
    dev.pci_device = desc.pci_device;
    dev.pci_function = desc.pci_function;

    // Get port number from features
    let port_num = (desc.features & 0x1F) as u8;
    dev.port.port_num = port_num;
    dev.port.port_base = dev.abar + 0x100 + (port_num as u64 * 0x80);
    dev.port.sector_size = SECTOR_SIZE;

    unsafe { kmod_spinlock_init(&mut dev.port.lock); }

    BlockDeviceHandle(dev as *mut AhciDevice as *mut u8)
}

extern "C" fn ahci_destroy(handle: BlockDeviceHandle) {
    if handle.0.is_null() { return; }
    let dev = unsafe { &mut *(handle.0 as *mut AhciDevice) };
    dev.port.cleanup();
    unsafe { kmod_dealloc(handle.0, core::mem::size_of::<AhciDevice>(), 8); }
    mod_info!(b"ahci: Destroyed\n");
}

extern "C" fn ahci_init(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() { return -1; }
    let dev = unsafe { &mut *(handle.0 as *mut AhciDevice) };

    mod_info!(b"ahci: Initializing...\n");

    // Enable AHCI mode
    let ghc = unsafe { kmod_mmio_read32(dev.abar + HBA_GHC) };
    unsafe { kmod_mmio_write32(dev.abar + HBA_GHC, ghc | GHC_AE); }

    // Check port implemented
    let pi = unsafe { kmod_mmio_read32(dev.abar + HBA_PI) };
    if (pi & (1 << dev.port.port_num)) == 0 {
        mod_error!(b"ahci: Port not implemented\n");
        return -2;
    }

    // Check device present
    let ssts = dev.port.read(PORT_SSTS);
    if (ssts & SSTS_DET_MASK) != SSTS_DET_PRESENT {
        mod_error!(b"ahci: No device\n");
        return -3;
    }

    // Check signature
    let sig = dev.port.read(PORT_SIG);
    dev.port.atapi = sig == SATA_SIG_ATAPI;
    if sig != SATA_SIG_ATA && sig != SATA_SIG_ATAPI {
        mod_error!(b"ahci: Unknown signature\n");
        return -4;
    }

    // Init port memory
    if dev.port.init_memory() != 0 {
        mod_error!(b"ahci: Port init failed\n");
        return -5;
    }

    // Identify device
    if dev.port.identify() != 0 {
        mod_error!(b"ahci: Identify failed\n");
        return -6;
    }

    mod_info!(b"ahci: Initialized OK\n");
    0
}

extern "C" fn ahci_get_info(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32 {
    if handle.0.is_null() || info.is_null() { return -1; }
    let dev = unsafe { &*(handle.0 as *mut AhciDevice) };
    let info = unsafe { &mut *info };

    // Name: sda, sdb, etc.
    let name = b"sda\0";
    info.name[..name.len()].copy_from_slice(name);
    info.name[2] = b'a' + dev.port.port_num;

    info.sector_size = dev.port.sector_size;
    info.total_sectors = dev.port.total_sectors;
    info.read_only = false;
    info.removable = dev.port.atapi;
    info.pci_bus = dev.pci_bus;
    info.pci_device = dev.pci_device;
    info.pci_function = dev.pci_function;
    0
}

extern "C" fn ahci_read(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *mut u8) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 { return -1; }
    let dev = unsafe { &mut *(handle.0 as *mut AhciDevice) };

    if sector + count as u64 > dev.port.total_sectors {
        return -2;
    }

    unsafe { kmod_spinlock_lock(&mut dev.port.lock); }
    let r = dev.port.read_sectors(sector, count, buf);
    unsafe { kmod_spinlock_unlock(&mut dev.port.lock); }
    r
}

extern "C" fn ahci_write(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *const u8) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 { return -1; }
    let dev = unsafe { &mut *(handle.0 as *mut AhciDevice) };

    if sector + count as u64 > dev.port.total_sectors {
        return -2;
    }

    unsafe { kmod_spinlock_lock(&mut dev.port.lock); }
    let r = dev.port.write_sectors(sector, count, buf);
    unsafe { kmod_spinlock_unlock(&mut dev.port.lock); }
    r
}

extern "C" fn ahci_flush(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() { return -1; }
    let dev = unsafe { &mut *(handle.0 as *mut AhciDevice) };

    unsafe { kmod_spinlock_lock(&mut dev.port.lock); }
    let r = dev.port.flush();
    unsafe { kmod_spinlock_unlock(&mut dev.port.lock); }
    r
}

// ============================================================================
// Module Entry Points
// ============================================================================

#[used]
#[no_mangle]
pub static MODULE_ENTRY_POINTS: [unsafe extern "C" fn() -> i32; 2] = [
    module_init_wrapper,
    module_exit_wrapper,
];

#[no_mangle]
unsafe extern "C" fn module_init_wrapper() -> i32 { module_init() }

#[no_mangle]
unsafe extern "C" fn module_exit_wrapper() -> i32 { module_exit() }

#[no_mangle]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"ahci: Loading...\n");

    let mut name = [0u8; 32];
    name[..4].copy_from_slice(b"ahci");

    let ops = BlockDriverOps {
        name,
        probe: Some(ahci_probe),
        new: Some(ahci_new),
        destroy: Some(ahci_destroy),
        init: Some(ahci_init),
        get_info: Some(ahci_get_info),
        read: Some(ahci_read),
        write: Some(ahci_write),
        flush: Some(ahci_flush),
    };

    let r = unsafe { kmod_blk_register(&ops) };
    if r != 0 {
        mod_error!(b"ahci: Register failed\n");
        return -1;
    }

    mod_info!(b"ahci: Loaded\n");
    0
}

#[no_mangle]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"ahci: Unloading...\n");
    unsafe { kmod_blk_unregister(b"ahci".as_ptr(), 4); }
    mod_info!(b"ahci: Unloaded\n");
    0
}
