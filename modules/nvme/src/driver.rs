//! NVMe Block Device Driver Entry Points
//!
//! Implements the block device driver interface for NexaOS kernel.

use crate::cmd::NvmeCmd;
use crate::controller::NvmeController;
use crate::queue::NvmeQueuePair;
use crate::{kmod_zalloc, kmod_dealloc, kmod_phys_to_virt, kmod_virt_to_phys};
use crate::{kmod_spinlock_init, kmod_spinlock_lock, kmod_spinlock_unlock};
use crate::{kmod_blk_register, kmod_blk_unregister, kmod_fence};
use crate::{mod_info, mod_error};
use core::ptr;

// =============================================================================
// FFI Types (matching kernel's block device interface)
// =============================================================================

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

// =============================================================================
// NVMe Device Wrapper
// =============================================================================

/// NVMe device state wrapper
#[repr(C)]
struct NvmeDevice {
    /// The controller
    controller: NvmeController,
    /// Active namespace ID (for block device interface)
    active_nsid: u32,
    /// Active namespace block size
    block_size: u32,
    /// Active namespace total blocks
    total_blocks: u64,
    /// PRP list buffer (for large transfers)
    prp_list: *mut u64,
    /// PRP list physical address
    prp_list_phys: u64,
}

impl NvmeDevice {
    /// Get a mutable reference to the I/O queue
    fn io_queue(&mut self) -> Option<&mut NvmeQueuePair> {
        self.controller.get_io_queue()
    }
}

// =============================================================================
// PCI Device Detection
// =============================================================================

/// Known NVMe controller vendor/device IDs
const NVME_DEVICES: &[(u16, u16)] = &[
    // Generic NVMe (class code should be used primarily)
    (0x8086, 0xF1A5),   // Intel NVMe
    (0x8086, 0xF1A6),   // Intel NVMe
    (0x8086, 0x0953),   // Intel DC P3700
    (0x8086, 0x0A54),   // Intel NVMe
    (0x144D, 0xA808),   // Samsung 970 PRO
    (0x144D, 0xA809),   // Samsung 970 EVO
    (0x144D, 0xA80A),   // Samsung 980 PRO
    (0x15B7, 0x5001),   // Sandisk/WD
    (0x1987, 0x5012),   // Phison E12
    (0x1E0F, 0x0001),   // KIOXIA
    (0x1CC1, 0x8201),   // ADATA
    // QEMU NVMe
    (0x1B36, 0x0010),   // QEMU NVMe
];

// =============================================================================
// Driver Callbacks
// =============================================================================

/// Probe for NVMe devices
extern "C" fn nvme_probe(vendor_id: u16, device_id: u16) -> i32 {
    // Check known devices
    for (vid, did) in NVME_DEVICES {
        if vendor_id == *vid && device_id == *did {
            return 0; // Match
        }
    }
    
    // QEMU typically uses vendor 0x1B36 for NVMe
    if vendor_id == 0x1B36 {
        return 0;
    }

    // Also accept if class code indicates NVMe (checked by kernel)
    // Return 0 for potential match, -1 for no match
    if vendor_id == 0 && device_id == 0 {
        return 0; // Let kernel do class code check
    }

    -1
}

/// Create a new NVMe device instance
extern "C" fn nvme_new(desc: *const BootBlockDevice) -> BlockDeviceHandle {
    if desc.is_null() {
        return BlockDeviceHandle(ptr::null_mut());
    }

    let desc = unsafe { &*desc };
    mod_info!(b"nvme: Creating device\n");

    // Allocate device structure
    let p = unsafe { kmod_zalloc(core::mem::size_of::<NvmeDevice>(), 8) as *mut NvmeDevice };
    if p.is_null() {
        mod_error!(b"nvme: Device alloc failed\n");
        return BlockDeviceHandle(ptr::null_mut());
    }
    let dev = unsafe { &mut *p };

    // Initialize controller fields
    dev.controller.bar0_phys = desc.mmio_base;
    dev.controller.bar0 = unsafe { kmod_phys_to_virt(desc.mmio_base) };
    dev.controller.bar0_size = desc.mmio_length;
    dev.controller.pci_bus = desc.pci_bus;
    dev.controller.pci_device = desc.pci_device;
    dev.controller.pci_function = desc.pci_function;
    dev.controller.initialized = false;

    unsafe { kmod_spinlock_init(&mut dev.controller.lock); }

    // Allocate PRP list buffer (4KB, page-aligned)
    dev.prp_list = unsafe { kmod_zalloc(4096, 4096) } as *mut u64;
    if !dev.prp_list.is_null() {
        dev.prp_list_phys = unsafe { kmod_virt_to_phys(dev.prp_list as u64) };
    }

    BlockDeviceHandle(dev as *mut NvmeDevice as *mut u8)
}

/// Destroy NVMe device
extern "C" fn nvme_destroy(handle: BlockDeviceHandle) {
    if handle.0.is_null() {
        return;
    }

    let dev = unsafe { &mut *(handle.0 as *mut NvmeDevice) };
    
    // Shutdown controller
    dev.controller.shutdown();

    // Free PRP list
    if !dev.prp_list.is_null() {
        unsafe { kmod_dealloc(dev.prp_list as *mut u8, 4096, 4096); }
    }

    // Free device structure
    unsafe { kmod_dealloc(handle.0, core::mem::size_of::<NvmeDevice>(), 8); }
    mod_info!(b"nvme: Destroyed\n");
}

/// Initialize NVMe device
extern "C" fn nvme_init(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() {
        return -1;
    }

    let dev = unsafe { &mut *(handle.0 as *mut NvmeDevice) };
    mod_info!(b"nvme: Initializing...\n");

    // Initialize controller
    if let Err(e) = dev.controller.init() {
        mod_error!(b"nvme: Init failed\n");
        return e;
    }

    // Use first namespace
    if dev.controller.num_namespaces > 0 {
        let (nsid, blocks, block_size) = dev.controller.namespaces[0];
        dev.active_nsid = nsid;
        dev.total_blocks = blocks;
        dev.block_size = block_size;
    } else {
        mod_error!(b"nvme: No namespaces found\n");
        return -6;
    }

    mod_info!(b"nvme: Initialized OK\n");
    0
}

/// Get device info
extern "C" fn nvme_get_info(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32 {
    if handle.0.is_null() || info.is_null() {
        return -1;
    }

    let dev = unsafe { &*(handle.0 as *mut NvmeDevice) };
    let info = unsafe { &mut *info };

    // Name: nvme0n1
    let name = b"nvme0n1\0";
    info.name[..name.len()].copy_from_slice(name);

    // Convert from block size to 512-byte sectors for kernel interface
    let blocks_per_sector = if dev.block_size > 512 {
        dev.block_size / 512
    } else {
        1
    };

    info.sector_size = 512; // Kernel expects 512-byte sectors
    info.total_sectors = dev.total_blocks * blocks_per_sector as u64;
    info.read_only = false;
    info.removable = false;
    info.pci_bus = dev.controller.pci_bus;
    info.pci_device = dev.controller.pci_device;
    info.pci_function = dev.controller.pci_function;

    0
}

/// Read sectors
extern "C" fn nvme_read(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *mut u8) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 {
        return -1;
    }

    let dev = unsafe { &mut *(handle.0 as *mut NvmeDevice) };

    // Convert 512-byte sectors to device blocks
    let block_size = dev.block_size;
    let sectors_per_block = if block_size > 512 {
        block_size / 512
    } else {
        1
    };

    // Calculate LBA and count in device blocks
    let slba = sector / sectors_per_block as u64;
    let nlb = ((count + sectors_per_block - 1) / sectors_per_block) as u16;

    // Check bounds
    if slba + nlb as u64 > dev.total_blocks {
        return -2;
    }

    // Lock
    unsafe { 
        kmod_spinlock_lock(&mut dev.controller.lock); 
    }

    let result = nvme_do_io(dev, slba, nlb, buf, false);

    unsafe { kmod_spinlock_unlock(&mut dev.controller.lock); }

    result
}

/// Write sectors
extern "C" fn nvme_write(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *const u8) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 {
        return -1;
    }

    let dev = unsafe { &mut *(handle.0 as *mut NvmeDevice) };

    let block_size = dev.block_size;
    let sectors_per_block = if block_size > 512 {
        block_size / 512
    } else {
        1
    };

    let slba = sector / sectors_per_block as u64;
    let nlb = ((count + sectors_per_block - 1) / sectors_per_block) as u16;

    if slba + nlb as u64 > dev.total_blocks {
        return -2;
    }

    unsafe { kmod_spinlock_lock(&mut dev.controller.lock); }

    let result = nvme_do_io(dev, slba, nlb, buf as *mut u8, true);

    unsafe { kmod_spinlock_unlock(&mut dev.controller.lock); }

    result
}

/// Flush cache
extern "C" fn nvme_flush(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() {
        return -1;
    }

    let dev = unsafe { &mut *(handle.0 as *mut NvmeDevice) };
    let nsid = dev.active_nsid;

    unsafe { kmod_spinlock_lock(&mut dev.controller.lock); }

    let result = if let Some(queue) = dev.io_queue() {
        let cid = queue.alloc_cid();
        let cmd = NvmeCmd::flush(cid, nsid);
        match queue.submit_and_wait(&cmd) {
            Ok(_) => 0,
            Err(e) => e,
        }
    } else {
        -3
    };

    unsafe { kmod_spinlock_unlock(&mut dev.controller.lock); }

    result
}

// =============================================================================
// I/O Helper
// =============================================================================

/// Perform NVMe I/O (read or write)
fn nvme_do_io(dev: &mut NvmeDevice, slba: u64, nlb: u16, buf: *mut u8, is_write: bool) -> i32 {
    // Extract needed values before borrowing queue
    let block_size = dev.block_size;
    let nsid = dev.active_nsid;
    let prp_list = dev.prp_list;
    let prp_list_phys = dev.prp_list_phys;

    let buf_phys = unsafe { kmod_virt_to_phys(buf as u64) };
    let bytes = (nlb as u32) * block_size;
    
    // For small transfers (fits in 2 PRPs), use direct PRPs
    // For larger transfers, we need a PRP list
    let (prp1, prp2) = if bytes <= 4096 {
        // Single page
        (buf_phys, 0u64)
    } else if bytes <= 8192 {
        // Two pages
        (buf_phys, buf_phys + 4096)
    } else {
        // Need PRP list
        if prp_list.is_null() {
            return -4;
        }

        // First PRP is the data buffer start
        let prp1 = buf_phys;
        
        // Build PRP list for remaining pages
        let num_prps = ((bytes as usize + 4095) / 4096) - 1;
        if num_prps > 512 {
            // Too large for our PRP list buffer
            return -5;
        }

        unsafe {
            for i in 0..num_prps {
                let addr = buf_phys + ((i + 1) as u64 * 4096);
                ptr::write_volatile(prp_list.add(i), addr);
            }
            kmod_fence();
        }

        (prp1, prp_list_phys)
    };

    // Now get queue and submit
    let queue = match dev.io_queue() {
        Some(q) => q,
        None => return -3,
    };

    // Build command
    let cid = queue.alloc_cid();
    let cmd = if is_write {
        NvmeCmd::write(cid, nsid, slba, nlb, prp1, prp2)
    } else {
        NvmeCmd::read(cid, nsid, slba, nlb, prp1, prp2)
    };

    // Submit and wait
    match queue.submit_and_wait(&cmd) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

// =============================================================================
// Module Entry Points
// =============================================================================

#[used]
#[no_mangle]
pub static MODULE_ENTRY_POINTS: [unsafe extern "C" fn() -> i32; 2] = [
    module_init_wrapper,
    module_exit_wrapper,
];

#[no_mangle]
unsafe extern "C" fn module_init_wrapper() -> i32 {
    module_init()
}

#[no_mangle]
unsafe extern "C" fn module_exit_wrapper() -> i32 {
    module_exit()
}

#[no_mangle]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"nvme: Loading...\n");

    let mut name = [0u8; 32];
    name[..4].copy_from_slice(b"nvme");

    let ops = BlockDriverOps {
        name,
        probe: Some(nvme_probe),
        new: Some(nvme_new),
        destroy: Some(nvme_destroy),
        init: Some(nvme_init),
        get_info: Some(nvme_get_info),
        read: Some(nvme_read),
        write: Some(nvme_write),
        flush: Some(nvme_flush),
    };

    let r = unsafe { kmod_blk_register(&ops) };
    if r != 0 {
        mod_error!(b"nvme: Register failed\n");
        return -1;
    }

    mod_info!(b"nvme: Loaded\n");
    0
}

#[no_mangle]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"nvme: Unloading...\n");
    unsafe { kmod_blk_unregister(b"nvme".as_ptr(), 4); }
    mod_info!(b"nvme: Unloaded\n");
    0
}
