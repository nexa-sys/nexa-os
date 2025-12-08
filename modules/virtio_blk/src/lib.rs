//! VirtIO Block Device Driver Kernel Module for NexaOS
//!
//! This is a loadable kernel module (.nkm) that provides VirtIO block device support.
//! It is loaded from initramfs during boot and dynamically linked to the kernel.
//!
//! # VirtIO Block Protocol
//!
//! VirtIO uses a split virtqueue mechanism:
//! - Descriptor table: describes buffers for data transfer
//! - Available ring: driver → device (requests to process)
//! - Used ring: device → driver (completed requests)
//!
//! # Module Entry Points
//!
//! - `module_init`: Called when module is loaded
//! - `module_exit`: Called when module is unloaded
//!
//! # Kernel API Usage
//!
//! This module uses the kernel's exported symbol table for:
//! - Logging (kmod_log_*)
//! - Memory allocation (kmod_alloc, kmod_dealloc)
//! - Block device registration (kmod_blk_register)
//! - MMIO access

#![no_std]
#![allow(dead_code)]

use core::ptr;
use core::sync::atomic::{fence, Ordering};

// ============================================================================
// Module Metadata
// ============================================================================

/// Module name
pub const MODULE_NAME: &[u8] = b"virtio_blk\0";
/// Module version
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description
pub const MODULE_DESC: &[u8] = b"VirtIO Block Device driver for NexaOS\0";
/// Module type (2 = Block Device)
pub const MODULE_TYPE: u8 = 2;
/// Module license
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
/// Module author
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
/// Source version
pub const MODULE_SRCVERSION: &[u8] = b"in-tree\0";

// ============================================================================
// Kernel API declarations (resolved at load time from kernel symbol table)
// ============================================================================

extern "C" {
    fn kmod_log_info(msg: *const u8, len: usize);
    fn kmod_log_error(msg: *const u8, len: usize);
    fn kmod_log_warn(msg: *const u8, len: usize);
    fn kmod_log_debug(msg: *const u8, len: usize);
    fn kmod_alloc(size: usize, align: usize) -> *mut u8;
    fn kmod_zalloc(size: usize, align: usize) -> *mut u8;
    fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);
    fn kmod_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    fn kmod_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8;
    fn kmod_spinlock_init(lock: *mut u64);
    fn kmod_spinlock_lock(lock: *mut u64);
    fn kmod_spinlock_unlock(lock: *mut u64);
    fn kmod_fence();
    fn kmod_spin_hint();
    
    // I/O port access functions (for VirtIO-PCI legacy mode)
    fn kmod_inb(port: u16) -> u8;
    fn kmod_inw(port: u16) -> u16;
    fn kmod_inl(port: u16) -> u32;
    fn kmod_outb(port: u16, value: u8);
    fn kmod_outw(port: u16, value: u16);
    fn kmod_outl(port: u16, value: u32);
    
    // Block device modular API
    fn kmod_blk_register(ops: *const BlockDriverOps) -> i32;
    fn kmod_blk_unregister(name: *const u8, name_len: usize) -> i32;
}

// ============================================================================
// Logging Helpers
// ============================================================================

macro_rules! mod_info {
    ($msg:expr) => {
        unsafe { kmod_log_info($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_error {
    ($msg:expr) => {
        unsafe { kmod_log_error($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_warn {
    ($msg:expr) => {
        unsafe { kmod_log_warn($msg.as_ptr(), $msg.len()) }
    };
}

macro_rules! mod_debug {
    ($msg:expr) => {
        unsafe { kmod_log_debug($msg.as_ptr(), $msg.len()) }
    };
}

// Helper to log hex values
fn log_hex(prefix: &[u8], value: u64) {
    let mut buf = [0u8; 64];
    let prefix_len = prefix.len().min(40);
    unsafe {
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), buf.as_mut_ptr(), prefix_len);
    }
    buf[prefix_len] = b'0';
    buf[prefix_len + 1] = b'x';

    let hex_chars = b"0123456789abcdef";
    let mut pos = prefix_len + 2;
    let mut started = false;
    for i in (0..16).rev() {
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        if nibble != 0 || started || i == 0 {
            buf[pos] = hex_chars[nibble];
            pos += 1;
            started = true;
        }
    }
    buf[pos] = b'\n';
    pos += 1;

    unsafe { kmod_log_info(buf.as_ptr(), pos); }
}

// ============================================================================
// VirtIO Constants
// ============================================================================

// VirtIO PCI vendor/device IDs
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_BLK_DEVICE_LEGACY: u16 = 0x1001;
const VIRTIO_BLK_DEVICE_MODERN: u16 = 0x1042;

// VirtIO device status bits
const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
const VIRTIO_STATUS_FAILED: u8 = 128;

// VirtIO block request types
const VIRTIO_BLK_T_IN: u32 = 0;      // Read
const VIRTIO_BLK_T_OUT: u32 = 1;     // Write
const VIRTIO_BLK_T_FLUSH: u32 = 4;   // Flush
const VIRTIO_BLK_T_GET_ID: u32 = 8;  // Get device ID

// VirtIO block status codes
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// VirtIO feature bits
const VIRTIO_BLK_F_SIZE_MAX: u64 = 1 << 1;
const VIRTIO_BLK_F_SEG_MAX: u64 = 1 << 2;
const VIRTIO_BLK_F_GEOMETRY: u64 = 1 << 4;
const VIRTIO_BLK_F_RO: u64 = 1 << 5;
const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;
const VIRTIO_BLK_F_FLUSH: u64 = 1 << 9;
const VIRTIO_BLK_F_TOPOLOGY: u64 = 1 << 10;

// VirtIO MMIO registers (legacy interface)
const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
const VIRTIO_MMIO_VERSION: usize = 0x004;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;
const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;
const VIRTIO_MMIO_DEVICE_FEATURES_SEL: usize = 0x014;
const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;
const VIRTIO_MMIO_DRIVER_FEATURES_SEL: usize = 0x024;
const VIRTIO_MMIO_GUEST_PAGE_SIZE: usize = 0x028;
const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
const VIRTIO_MMIO_QUEUE_ALIGN: usize = 0x03c;
const VIRTIO_MMIO_QUEUE_PFN: usize = 0x040;
const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;
const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
const VIRTIO_MMIO_INTERRUPT_STATUS: usize = 0x060;
const VIRTIO_MMIO_INTERRUPT_ACK: usize = 0x064;
const VIRTIO_MMIO_STATUS: usize = 0x070;
const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const VIRTIO_MMIO_QUEUE_AVAIL_LOW: usize = 0x090;
const VIRTIO_MMIO_QUEUE_AVAIL_HIGH: usize = 0x094;
const VIRTIO_MMIO_QUEUE_USED_LOW: usize = 0x0a0;
const VIRTIO_MMIO_QUEUE_USED_HIGH: usize = 0x0a4;
const VIRTIO_MMIO_CONFIG: usize = 0x100;

// VirtIO magic value
const VIRTIO_MAGIC: u32 = 0x74726976; // "virt"

// VirtIO-PCI Legacy I/O port registers (BAR0)
// These offsets are relative to the I/O port base address
const VIRTIO_PCI_HOST_FEATURES: u16 = 0;       // 32-bit: Device features (R)
const VIRTIO_PCI_GUEST_FEATURES: u16 = 4;      // 32-bit: Driver features (R/W)
const VIRTIO_PCI_QUEUE_PFN: u16 = 8;           // 32-bit: Queue PFN (R/W)
const VIRTIO_PCI_QUEUE_SIZE: u16 = 12;         // 16-bit: Queue size (R)
const VIRTIO_PCI_QUEUE_SEL: u16 = 14;          // 16-bit: Queue select (R/W)
const VIRTIO_PCI_QUEUE_NOTIFY: u16 = 16;       // 16-bit: Queue notify (R/W)
const VIRTIO_PCI_STATUS: u16 = 18;             // 8-bit: Device status (R/W)
const VIRTIO_PCI_ISR: u16 = 19;                // 8-bit: ISR status (R)
// Device-specific config starts at offset 20 for legacy devices
const VIRTIO_PCI_CONFIG: u16 = 20;

// Queue size
const QUEUE_SIZE: u16 = 128;

// Descriptor flags
const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;
const VRING_DESC_F_INDIRECT: u16 = 4;

// ============================================================================
// VirtIO Structures
// ============================================================================

/// VirtIO queue descriptor
#[repr(C)]
#[derive(Clone, Copy)]
struct VringDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// VirtIO available ring
#[repr(C)]
struct VringAvail {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE as usize],
    used_event: u16,
}

/// VirtIO used ring element
#[repr(C)]
#[derive(Clone, Copy)]
struct VringUsedElem {
    id: u32,
    len: u32,
}

/// VirtIO used ring
#[repr(C)]
struct VringUsed {
    flags: u16,
    idx: u16,
    ring: [VringUsedElem; QUEUE_SIZE as usize],
    avail_event: u16,
}

/// VirtIO block request header
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioBlkReqHeader {
    req_type: u32,
    reserved: u32,
    sector: u64,
}

/// VirtIO block device configuration
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioBlkConfig {
    capacity: u64,
    size_max: u32,
    seg_max: u32,
    geometry_cylinders: u16,
    geometry_heads: u8,
    geometry_sectors: u8,
    blk_size: u32,
}

// ============================================================================
// FFI Types (must match kernel's block/mod.rs)
// ============================================================================

/// Boot block device descriptor
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootBlockDevice {
    pub pci_segment: u16,
    pub pci_bus: u8,
    pub pci_device: u8,
    pub pci_function: u8,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub sector_size: u32,
    pub total_sectors: u64,
    pub features: u64,
}

/// Block device info
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

/// Block device handle
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BlockDeviceHandle(pub *mut u8);

/// Function pointer types
pub type FnBlkProbe = extern "C" fn(vendor_id: u16, device_id: u16) -> i32;
pub type FnBlkNew = extern "C" fn(desc: *const BootBlockDevice) -> BlockDeviceHandle;
pub type FnBlkDestroy = extern "C" fn(handle: BlockDeviceHandle);
pub type FnBlkInit = extern "C" fn(handle: BlockDeviceHandle) -> i32;
pub type FnBlkGetInfo = extern "C" fn(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32;
pub type FnBlkRead = extern "C" fn(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *mut u8) -> i32;
pub type FnBlkWrite = extern "C" fn(handle: BlockDeviceHandle, sector: u64, count: u32, buf: *const u8) -> i32;
pub type FnBlkFlush = extern "C" fn(handle: BlockDeviceHandle) -> i32;

/// Block driver operations table
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
// VirtIO Block Device Driver
// ============================================================================

/// Transport mode for VirtIO device
#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
enum VirtioTransport {
    /// Memory-mapped I/O (virtio-mmio)
    Mmio = 0,
    /// I/O port (virtio-pci legacy)
    IoPort = 1,
}

/// VirtIO block device instance
#[repr(C)]
struct VirtioBlkDevice {
    /// Base address (MMIO address or I/O port base)
    base_addr: u64,
    /// Transport mode
    transport: VirtioTransport,
    /// Device capacity in sectors
    capacity: u64,
    /// Sector size (typically 512)
    sector_size: u32,
    /// Device is read-only
    read_only: bool,
    /// PCI address
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    /// Virtqueue
    queue: *mut VirtQueue,
    /// Device features
    features: u64,
    /// Lock for synchronization
    lock: u64,
}

/// Virtqueue structure
#[repr(C)]
struct VirtQueue {
    /// Queue size
    num: u16,
    /// Free descriptor head
    free_head: u16,
    /// Number of free descriptors
    num_free: u16,
    /// Last seen used index
    last_used_idx: u16,
    /// Descriptor table
    desc: *mut VringDesc,
    /// Available ring
    avail: *mut VringAvail,
    /// Used ring
    used: *mut VringUsed,
    /// Request buffers
    requests: *mut VirtioBlkReqHeader,
    /// Status buffers
    status: *mut u8,
    /// Data buffers
    data_buffers: *mut u8,
}

// ============================================================================
// MMIO Helpers
// ============================================================================

#[inline]
unsafe fn mmio_read32(base: u64, offset: usize) -> u32 {
    let ptr = (base + offset as u64) as *const u32;
    core::ptr::read_volatile(ptr)
}

#[inline]
unsafe fn mmio_write32(base: u64, offset: usize, value: u32) {
    let ptr = (base + offset as u64) as *mut u32;
    core::ptr::write_volatile(ptr, value);
}

#[inline]
unsafe fn mmio_read64(base: u64, offset: usize) -> u64 {
    let low = mmio_read32(base, offset) as u64;
    let high = mmio_read32(base, offset + 4) as u64;
    low | (high << 32)
}

// ============================================================================
// I/O Port Helpers (VirtIO-PCI Legacy)
// ============================================================================

#[inline]
unsafe fn pio_read8(base: u16, offset: u16) -> u8 {
    kmod_inb(base + offset)
}

#[inline]
unsafe fn pio_read16(base: u16, offset: u16) -> u16 {
    kmod_inw(base + offset)
}

#[inline]
unsafe fn pio_read32(base: u16, offset: u16) -> u32 {
    kmod_inl(base + offset)
}

#[inline]
unsafe fn pio_write8(base: u16, offset: u16, value: u8) {
    kmod_outb(base + offset, value);
}

#[inline]
unsafe fn pio_write16(base: u16, offset: u16, value: u16) {
    kmod_outw(base + offset, value);
}

#[inline]
unsafe fn pio_write32(base: u16, offset: u16, value: u32) {
    kmod_outl(base + offset, value);
}

// ============================================================================
// Driver Implementation
// ============================================================================

/// Check if we support this PCI device
extern "C" fn virtio_blk_probe(vendor_id: u16, device_id: u16) -> i32 {
    if vendor_id == VIRTIO_VENDOR_ID &&
       (device_id == VIRTIO_BLK_DEVICE_LEGACY || device_id == VIRTIO_BLK_DEVICE_MODERN) {
        0 // Supported
    } else {
        -1 // Not supported
    }
}

/// Create a new VirtIO block device instance
extern "C" fn virtio_blk_new(desc: *const BootBlockDevice) -> BlockDeviceHandle {
    if desc.is_null() {
        return BlockDeviceHandle(ptr::null_mut());
    }

    let desc = unsafe { &*desc };
    mod_info!(b"virtio_blk: Creating new device instance\n");
    log_hex(b"  Base address: ", desc.mmio_base);
    
    // Determine transport mode based on features flag
    // Flag 0x1 means I/O port mode (set by boot stage when BAR is I/O space)
    let transport = if (desc.features & 0x1) != 0 {
        mod_info!(b"  Transport: VirtIO-PCI Legacy (I/O port)\n");
        VirtioTransport::IoPort
    } else {
        mod_info!(b"  Transport: VirtIO-MMIO\n");
        VirtioTransport::Mmio
    };

    // Allocate device structure
    let device = unsafe {
        let ptr = kmod_zalloc(
            core::mem::size_of::<VirtioBlkDevice>(),
            core::mem::align_of::<VirtioBlkDevice>(),
        ) as *mut VirtioBlkDevice;
        if ptr.is_null() {
            mod_error!(b"virtio_blk: Failed to allocate device structure\n");
            return BlockDeviceHandle(ptr::null_mut());
        }
        &mut *ptr
    };

    device.base_addr = desc.mmio_base;
    device.transport = transport;
    device.pci_bus = desc.pci_bus;
    device.pci_device = desc.pci_device;
    device.pci_function = desc.pci_function;
    device.sector_size = if desc.sector_size > 0 { desc.sector_size } else { 512 };
    device.capacity = desc.total_sectors;

    unsafe { kmod_spinlock_init(&mut device.lock) };

    BlockDeviceHandle(device as *mut VirtioBlkDevice as *mut u8)
}

/// Destroy a VirtIO block device instance
extern "C" fn virtio_blk_destroy(handle: BlockDeviceHandle) {
    if handle.0.is_null() {
        return;
    }

    let device = handle.0 as *mut VirtioBlkDevice;
    unsafe {
        let dev = &mut *device;

        // Free virtqueue if allocated
        if !dev.queue.is_null() {
            let queue = &mut *dev.queue;

            // Free queue memory
            if !queue.desc.is_null() {
                let desc_size = QUEUE_SIZE as usize * core::mem::size_of::<VringDesc>();
                kmod_dealloc(queue.desc as *mut u8, desc_size, 16);
            }
            if !queue.requests.is_null() {
                let req_size = QUEUE_SIZE as usize * core::mem::size_of::<VirtioBlkReqHeader>();
                kmod_dealloc(queue.requests as *mut u8, req_size, 8);
            }
            if !queue.status.is_null() {
                kmod_dealloc(queue.status, QUEUE_SIZE as usize, 1);
            }
            if !queue.data_buffers.is_null() {
                let buf_size = QUEUE_SIZE as usize * 4096; // 4K per request
                kmod_dealloc(queue.data_buffers, buf_size, 4096);
            }

            kmod_dealloc(
                dev.queue as *mut u8,
                core::mem::size_of::<VirtQueue>(),
                core::mem::align_of::<VirtQueue>(),
            );
        }

        // Free device structure
        kmod_dealloc(
            device as *mut u8,
            core::mem::size_of::<VirtioBlkDevice>(),
            core::mem::align_of::<VirtioBlkDevice>(),
        );
    }

    mod_info!(b"virtio_blk: Device destroyed\n");
}

/// Initialize the VirtIO block device
extern "C" fn virtio_blk_init(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut VirtioBlkDevice) };

    mod_info!(b"virtio_blk: Initializing device...\n");
    
    // Dispatch to appropriate initialization based on transport
    match device.transport {
        VirtioTransport::Mmio => unsafe { init_mmio_device(device) },
        VirtioTransport::IoPort => unsafe { init_pio_device(device) },
    }
}

/// Initialize device via MMIO transport
unsafe fn init_mmio_device(device: &mut VirtioBlkDevice) -> i32 {
    let base = device.base_addr;
    
    mod_info!(b"virtio_blk: Using MMIO transport\n");

    // Step 1: Reset device
    mmio_write32(base, VIRTIO_MMIO_STATUS, 0);

    // Step 2: Acknowledge device
    mmio_write32(base, VIRTIO_MMIO_STATUS, VIRTIO_STATUS_ACKNOWLEDGE as u32);

    // Step 3: Set DRIVER status bit
    let status = mmio_read32(base, VIRTIO_MMIO_STATUS);
    mmio_write32(base, VIRTIO_MMIO_STATUS, status | VIRTIO_STATUS_DRIVER as u32);

    // Step 4: Read device features
    mmio_write32(base, VIRTIO_MMIO_DEVICE_FEATURES_SEL, 0);
    let features_low = mmio_read32(base, VIRTIO_MMIO_DEVICE_FEATURES);
    mmio_write32(base, VIRTIO_MMIO_DEVICE_FEATURES_SEL, 1);
    let features_high = mmio_read32(base, VIRTIO_MMIO_DEVICE_FEATURES);
    device.features = (features_low as u64) | ((features_high as u64) << 32);

    log_hex(b"  Device features: ", device.features);

    // Check for read-only
    device.read_only = (device.features & VIRTIO_BLK_F_RO) != 0;
    if device.read_only {
        mod_info!(b"virtio_blk: Device is read-only\n");
    }

    // Step 5: Write driver features (acknowledge what we support)
    let driver_features = VIRTIO_BLK_F_FLUSH | VIRTIO_BLK_F_BLK_SIZE;
    mmio_write32(base, VIRTIO_MMIO_DRIVER_FEATURES_SEL, 0);
    mmio_write32(base, VIRTIO_MMIO_DRIVER_FEATURES, driver_features as u32);

    // Step 6: Set FEATURES_OK
    let status = mmio_read32(base, VIRTIO_MMIO_STATUS);
    mmio_write32(base, VIRTIO_MMIO_STATUS, status | VIRTIO_STATUS_FEATURES_OK as u32);

    // Step 7: Re-read status to ensure features OK was accepted
    let status = mmio_read32(base, VIRTIO_MMIO_STATUS);
    if (status & VIRTIO_STATUS_FEATURES_OK as u32) == 0 {
        mod_error!(b"virtio_blk: Device did not accept features\n");
        mmio_write32(base, VIRTIO_MMIO_STATUS, VIRTIO_STATUS_FAILED as u32);
        return -2;
    }

    // Step 8: Read device configuration
    let capacity = mmio_read64(base, VIRTIO_MMIO_CONFIG);
    device.capacity = capacity;
    log_hex(b"  Capacity (sectors): ", capacity);

    // Read block size if feature is supported
    if (device.features & VIRTIO_BLK_F_BLK_SIZE) != 0 {
        let blk_size = mmio_read32(base, VIRTIO_MMIO_CONFIG + 20);
        if blk_size > 0 && blk_size <= 4096 {
            device.sector_size = blk_size;
        }
    }
    log_hex(b"  Sector size: ", device.sector_size as u64);

    // Step 9: Setup virtqueue
    if setup_virtqueue_mmio(device) != 0 {
        mod_error!(b"virtio_blk: Failed to setup virtqueue\n");
        mmio_write32(base, VIRTIO_MMIO_STATUS, VIRTIO_STATUS_FAILED as u32);
        return -3;
    }

    // Step 10: Set DRIVER_OK
    let status = mmio_read32(base, VIRTIO_MMIO_STATUS);
    mmio_write32(base, VIRTIO_MMIO_STATUS, status | VIRTIO_STATUS_DRIVER_OK as u32);

    mod_info!(b"virtio_blk: MMIO device initialized successfully\n");
    0
}

/// Initialize device via I/O port transport (VirtIO-PCI legacy)
unsafe fn init_pio_device(device: &mut VirtioBlkDevice) -> i32 {
    let base = device.base_addr as u16;
    
    mod_info!(b"virtio_blk: Using I/O port transport (legacy PCI)\n");
    log_hex(b"  I/O port base: ", base as u64);

    // Step 1: Reset device
    pio_write8(base, VIRTIO_PCI_STATUS, 0);
    kmod_fence();

    // Step 2: Acknowledge device
    pio_write8(base, VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACKNOWLEDGE);
    kmod_fence();

    // Step 3: Set DRIVER status bit
    let status = pio_read8(base, VIRTIO_PCI_STATUS);
    pio_write8(base, VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_DRIVER);
    kmod_fence();

    // Step 4: Read device features (32-bit in legacy mode)
    let features = pio_read32(base, VIRTIO_PCI_HOST_FEATURES);
    device.features = features as u64;
    log_hex(b"  Device features: ", device.features);

    // Check for read-only
    device.read_only = (device.features & VIRTIO_BLK_F_RO) != 0;
    if device.read_only {
        mod_info!(b"virtio_blk: Device is read-only\n");
    }

    // Step 5: Write driver features
    let driver_features = (VIRTIO_BLK_F_FLUSH | VIRTIO_BLK_F_BLK_SIZE) as u32;
    pio_write32(base, VIRTIO_PCI_GUEST_FEATURES, driver_features & features);
    kmod_fence();

    // Step 6: Read device configuration
    // VirtIO block config starts at offset VIRTIO_PCI_CONFIG (20)
    // Read capacity (8 bytes)
    let cap_low = pio_read32(base, VIRTIO_PCI_CONFIG) as u64;
    let cap_high = pio_read32(base, VIRTIO_PCI_CONFIG + 4) as u64;
    device.capacity = cap_low | (cap_high << 32);
    log_hex(b"  Capacity (sectors): ", device.capacity);

    // Read block size if feature is supported (offset 20 from config start = 40 from BAR)
    if (device.features & VIRTIO_BLK_F_BLK_SIZE) != 0 {
        let blk_size = pio_read32(base, VIRTIO_PCI_CONFIG + 20);
        if blk_size > 0 && blk_size <= 4096 {
            device.sector_size = blk_size;
        }
    }
    log_hex(b"  Sector size: ", device.sector_size as u64);

    // Step 7: Setup virtqueue
    if setup_virtqueue_pio(device) != 0 {
        mod_error!(b"virtio_blk: Failed to setup virtqueue\n");
        pio_write8(base, VIRTIO_PCI_STATUS, VIRTIO_STATUS_FAILED);
        return -3;
    }

    // Step 8: Set DRIVER_OK
    let status = pio_read8(base, VIRTIO_PCI_STATUS);
    pio_write8(base, VIRTIO_PCI_STATUS, status | VIRTIO_STATUS_DRIVER_OK);
    kmod_fence();

    mod_info!(b"virtio_blk: PCI legacy device initialized successfully\n");
    0
}

/// Setup the virtqueue for MMIO device
unsafe fn setup_virtqueue_mmio(device: &mut VirtioBlkDevice) -> i32 {
    let base = device.base_addr;

    // Select queue 0
    mmio_write32(base, VIRTIO_MMIO_QUEUE_SEL, 0);

    // Get max queue size
    let max_size = mmio_read32(base, VIRTIO_MMIO_QUEUE_NUM_MAX) as u16;
    if max_size == 0 {
        mod_error!(b"virtio_blk: Queue not available\n");
        return -1;
    }

    let queue_size = max_size.min(QUEUE_SIZE);
    log_hex(b"  Queue size: ", queue_size as u64);

    // Allocate virtqueue structure
    let queue = kmod_zalloc(
        core::mem::size_of::<VirtQueue>(),
        core::mem::align_of::<VirtQueue>(),
    ) as *mut VirtQueue;
    if queue.is_null() {
        return -2;
    }

    let q = &mut *queue;
    q.num = queue_size;
    q.num_free = queue_size;
    q.free_head = 0;
    q.last_used_idx = 0;

    // Allocate descriptor table (16-byte aligned)
    let desc_size = queue_size as usize * core::mem::size_of::<VringDesc>();
    q.desc = kmod_zalloc(desc_size, 16) as *mut VringDesc;
    if q.desc.is_null() {
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -3;
    }

    // Initialize descriptor free list
    for i in 0..(queue_size - 1) {
        (*q.desc.add(i as usize)).next = i + 1;
    }
    (*q.desc.add(queue_size as usize - 1)).next = 0xFFFF;

    // Allocate available ring (2-byte aligned)
    let avail_size = 6 + 2 * queue_size as usize;
    q.avail = kmod_zalloc(avail_size, 2) as *mut VringAvail;
    if q.avail.is_null() {
        kmod_dealloc(q.desc as *mut u8, desc_size, 16);
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -4;
    }

    // Allocate used ring (4-byte aligned)
    let used_size = 6 + 8 * queue_size as usize;
    q.used = kmod_zalloc(used_size, 4) as *mut VringUsed;
    if q.used.is_null() {
        kmod_dealloc(q.avail as *mut u8, avail_size, 2);
        kmod_dealloc(q.desc as *mut u8, desc_size, 16);
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -5;
    }

    // Allocate request headers
    let req_size = queue_size as usize * core::mem::size_of::<VirtioBlkReqHeader>();
    q.requests = kmod_zalloc(req_size, 8) as *mut VirtioBlkReqHeader;
    if q.requests.is_null() {
        return -6;
    }

    // Allocate status buffers
    q.status = kmod_zalloc(queue_size as usize, 1);
    if q.status.is_null() {
        return -7;
    }

    // Allocate data buffers (4K per request)
    let buf_size = queue_size as usize * 4096;
    q.data_buffers = kmod_zalloc(buf_size, 4096);
    if q.data_buffers.is_null() {
        return -8;
    }

    device.queue = queue;

    // Configure queue in device
    mmio_write32(base, VIRTIO_MMIO_QUEUE_NUM, queue_size as u32);

    // For legacy MMIO, set page size and PFN
    let version = mmio_read32(base, VIRTIO_MMIO_VERSION);
    if version == 1 {
        // Legacy interface
        mmio_write32(base, VIRTIO_MMIO_GUEST_PAGE_SIZE, 4096);
        let pfn = (q.desc as u64) / 4096;
        mmio_write32(base, VIRTIO_MMIO_QUEUE_PFN, pfn as u32);
    } else {
        // Modern interface
        let desc_addr = q.desc as u64;
        let avail_addr = q.avail as u64;
        let used_addr = q.used as u64;

        mmio_write32(base, VIRTIO_MMIO_QUEUE_DESC_LOW, desc_addr as u32);
        mmio_write32(base, VIRTIO_MMIO_QUEUE_DESC_HIGH, (desc_addr >> 32) as u32);
        mmio_write32(base, VIRTIO_MMIO_QUEUE_AVAIL_LOW, avail_addr as u32);
        mmio_write32(base, VIRTIO_MMIO_QUEUE_AVAIL_HIGH, (avail_addr >> 32) as u32);
        mmio_write32(base, VIRTIO_MMIO_QUEUE_USED_LOW, used_addr as u32);
        mmio_write32(base, VIRTIO_MMIO_QUEUE_USED_HIGH, (used_addr >> 32) as u32);
        mmio_write32(base, VIRTIO_MMIO_QUEUE_READY, 1);
    }

    mod_info!(b"virtio_blk: MMIO virtqueue setup complete\n");
    0
}

/// Setup the virtqueue for PIO device (VirtIO-PCI legacy)
unsafe fn setup_virtqueue_pio(device: &mut VirtioBlkDevice) -> i32 {
    let base = device.base_addr as u16;

    // Select queue 0
    pio_write16(base, VIRTIO_PCI_QUEUE_SEL, 0);
    kmod_fence();

    // Get max queue size - for legacy PCI, this is the REQUIRED size
    let queue_size = pio_read16(base, VIRTIO_PCI_QUEUE_SIZE);
    if queue_size == 0 {
        mod_error!(b"virtio_blk: PIO queue not available\n");
        return -1;
    }
    
    log_hex(b"  Device queue size: ", queue_size as u64);
    
    // For legacy VirtIO-PCI, we MUST use the device's queue size
    // (it's not configurable like in modern VirtIO)

    // Allocate virtqueue structure
    let queue = kmod_zalloc(
        core::mem::size_of::<VirtQueue>(),
        core::mem::align_of::<VirtQueue>(),
    ) as *mut VirtQueue;
    if queue.is_null() {
        return -2;
    }

    let q = &mut *queue;
    q.num = queue_size;
    q.num_free = queue_size;
    q.free_head = 0;
    q.last_used_idx = 0;

    // For legacy VirtIO-PCI, descriptor table, available ring, and used ring
    // must be contiguous in a single page-aligned allocation
    // Layout: desc | avail | padding | used
    let desc_size = queue_size as usize * core::mem::size_of::<VringDesc>();
    let avail_size = 6 + 2 * queue_size as usize;
    let used_size = 6 + 8 * queue_size as usize;
    
    // Calculate aligned size for used ring (must be page-aligned from start)
    let avail_end = desc_size + avail_size;
    let used_start = (avail_end + 4095) & !4095; // Page-align used ring
    let total_size = used_start + used_size;
    
    // Allocate contiguous page-aligned memory
    let vring_mem = kmod_zalloc(total_size, 4096);
    if vring_mem.is_null() {
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -3;
    }

    q.desc = vring_mem as *mut VringDesc;
    q.avail = (vring_mem as usize + desc_size) as *mut VringAvail;
    q.used = (vring_mem as usize + used_start) as *mut VringUsed;

    // Initialize descriptor free list
    for i in 0..(queue_size - 1) {
        (*q.desc.add(i as usize)).next = i + 1;
    }
    (*q.desc.add(queue_size as usize - 1)).next = 0xFFFF;

    // Allocate request headers
    let req_size = queue_size as usize * core::mem::size_of::<VirtioBlkReqHeader>();
    q.requests = kmod_zalloc(req_size, 8) as *mut VirtioBlkReqHeader;
    if q.requests.is_null() {
        kmod_dealloc(vring_mem, total_size, 4096);
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -6;
    }

    // Allocate status buffers
    q.status = kmod_zalloc(queue_size as usize, 1);
    if q.status.is_null() {
        kmod_dealloc(q.requests as *mut u8, req_size, 8);
        kmod_dealloc(vring_mem, total_size, 4096);
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -7;
    }

    // Allocate data buffers (4K per request)
    let buf_size = queue_size as usize * 4096;
    q.data_buffers = kmod_zalloc(buf_size, 4096);
    if q.data_buffers.is_null() {
        kmod_dealloc(q.status, queue_size as usize, 1);
        kmod_dealloc(q.requests as *mut u8, req_size, 8);
        kmod_dealloc(vring_mem, total_size, 4096);
        kmod_dealloc(queue as *mut u8, core::mem::size_of::<VirtQueue>(), 8);
        return -8;
    }

    device.queue = queue;

    // Configure queue in device via PIO
    // Legacy VirtIO-PCI uses PFN (page frame number) for queue address
    let pfn = (vring_mem as u64) / 4096;
    pio_write32(base, VIRTIO_PCI_QUEUE_PFN, pfn as u32);
    kmod_fence();

    mod_info!(b"virtio_blk: PIO virtqueue setup complete\n");
    0
}

/// Get device information
extern "C" fn virtio_blk_get_info(handle: BlockDeviceHandle, info: *mut BlockDeviceInfo) -> i32 {
    if handle.0.is_null() || info.is_null() {
        return -1;
    }

    let device = unsafe { &*(handle.0 as *mut VirtioBlkDevice) };
    let info = unsafe { &mut *info };

    // Set device name
    let name = b"vda\0";
    info.name[..name.len()].copy_from_slice(name);

    info.sector_size = device.sector_size;
    info.total_sectors = device.capacity;
    info.read_only = device.read_only;
    info.removable = false;
    info.pci_bus = device.pci_bus;
    info.pci_device = device.pci_device;
    info.pci_function = device.pci_function;

    0
}

/// Read sectors from the device
extern "C" fn virtio_blk_read(
    handle: BlockDeviceHandle,
    sector: u64,
    count: u32,
    buf: *mut u8,
) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut VirtioBlkDevice) };

    // Check bounds
    if sector + count as u64 > device.capacity {
        mod_error!(b"virtio_blk: Read beyond device capacity\n");
        return -2;
    }

    unsafe {
        kmod_spinlock_lock(&mut device.lock);
    }

    let result = unsafe { do_block_io(device, VIRTIO_BLK_T_IN, sector, count, buf) };

    unsafe {
        kmod_spinlock_unlock(&mut device.lock);
    }

    result
}

/// Write sectors to the device
extern "C" fn virtio_blk_write(
    handle: BlockDeviceHandle,
    sector: u64,
    count: u32,
    buf: *const u8,
) -> i32 {
    if handle.0.is_null() || buf.is_null() || count == 0 {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut VirtioBlkDevice) };

    if device.read_only {
        mod_error!(b"virtio_blk: Device is read-only\n");
        return -3;
    }

    // Check bounds
    if sector + count as u64 > device.capacity {
        mod_error!(b"virtio_blk: Write beyond device capacity\n");
        return -2;
    }

    unsafe {
        kmod_spinlock_lock(&mut device.lock);
    }

    let result = unsafe { do_block_io(device, VIRTIO_BLK_T_OUT, sector, count, buf as *mut u8) };

    unsafe {
        kmod_spinlock_unlock(&mut device.lock);
    }

    result
}

/// Flush write cache
extern "C" fn virtio_blk_flush(handle: BlockDeviceHandle) -> i32 {
    if handle.0.is_null() {
        return -1;
    }

    let device = unsafe { &mut *(handle.0 as *mut VirtioBlkDevice) };

    // Check if flush is supported
    if (device.features & VIRTIO_BLK_F_FLUSH) == 0 {
        return 0; // No-op if not supported
    }

    unsafe {
        kmod_spinlock_lock(&mut device.lock);
    }

    let result = unsafe { do_block_io(device, VIRTIO_BLK_T_FLUSH, 0, 0, ptr::null_mut()) };

    unsafe {
        kmod_spinlock_unlock(&mut device.lock);
    }

    result
}

/// Perform block I/O operation
unsafe fn do_block_io(
    device: &mut VirtioBlkDevice,
    req_type: u32,
    sector: u64,
    count: u32,
    buf: *mut u8,
) -> i32 {
    let queue = &mut *device.queue;

    // Need 3 descriptors: header, data (if any), status
    let data_desc_count = if count > 0 { 1 } else { 0 };
    let total_descs = 2 + data_desc_count;

    if queue.num_free < total_descs {
        mod_error!(b"virtio_blk: No free descriptors\n");
        return -4;
    }

    // Allocate descriptors
    let head = queue.free_head;
    let mut desc_idx = head;

    // Setup request header
    let req_idx = head as usize % queue.num as usize;
    let req = &mut *queue.requests.add(req_idx);
    req.req_type = req_type;
    req.reserved = 0;
    req.sector = sector;

    // Descriptor 0: Request header (device reads)
    let d0 = &mut *queue.desc.add(desc_idx as usize);
    d0.addr = req as *const VirtioBlkReqHeader as u64;
    d0.len = core::mem::size_of::<VirtioBlkReqHeader>() as u32;
    d0.flags = VRING_DESC_F_NEXT;
    desc_idx = d0.next;
    queue.num_free -= 1;

    // Descriptor 1: Data buffer (if any)
    if count > 0 {
        let data_len = count * device.sector_size;
        let d1 = &mut *queue.desc.add(desc_idx as usize);
        d1.addr = buf as u64;
        d1.len = data_len;
        d1.flags = VRING_DESC_F_NEXT;
        if req_type == VIRTIO_BLK_T_IN {
            d1.flags |= VRING_DESC_F_WRITE; // Device writes to this buffer
        }
        desc_idx = d1.next;
        queue.num_free -= 1;
    }

    // Descriptor 2: Status byte (device writes)
    let status_ptr = queue.status.add(req_idx);
    *status_ptr = 0xFF; // Initialize to invalid status

    let d_status = &mut *queue.desc.add(desc_idx as usize);
    d_status.addr = status_ptr as u64;
    d_status.len = 1;
    d_status.flags = VRING_DESC_F_WRITE;
    let next_free = d_status.next;
    d_status.next = 0xFFFF; // End of chain
    queue.num_free -= 1;

    queue.free_head = next_free;

    // Add to available ring
    // Note: avail ring layout is: flags (u16), idx (u16), ring[num] (u16 each), used_event (u16)
    // We can't use the fixed-size struct, use pointer arithmetic instead
    let avail_ptr = queue.avail as *mut u8;
    let avail_idx_ptr = avail_ptr.add(2) as *mut u16;  // idx at offset 2
    let avail_ring_ptr = avail_ptr.add(4) as *mut u16; // ring starts at offset 4
    
    let avail_idx = core::ptr::read_volatile(avail_idx_ptr);
    let ring_slot = avail_ring_ptr.add((avail_idx % queue.num) as usize);
    core::ptr::write_volatile(ring_slot, head);
    fence(Ordering::SeqCst);
    core::ptr::write_volatile(avail_idx_ptr, avail_idx.wrapping_add(1));
    fence(Ordering::SeqCst);

    // Notify device (transport-specific)
    match device.transport {
        VirtioTransport::Mmio => {
            mmio_write32(device.base_addr, VIRTIO_MMIO_QUEUE_NOTIFY, 0);
        }
        VirtioTransport::IoPort => {
            pio_write16(device.base_addr as u16, VIRTIO_PCI_QUEUE_NOTIFY, 0);
            kmod_fence();
        }
    }

    // Wait for completion (polling)
    // Note: used ring layout is: flags (u16), idx (u16), ring[num] (struct each), avail_event (u16)
    let used_ptr = queue.used as *mut u8;
    let used_idx_ptr = used_ptr.add(2) as *mut u16;  // idx at offset 2
    
    let timeout = 10000000u32;
    let mut i = 0u32;
    
    while i < timeout {
        fence(Ordering::SeqCst);
        let current_used_idx = core::ptr::read_volatile(used_idx_ptr);
        if current_used_idx != queue.last_used_idx {
            break;
        }
        i += 1;
        core::hint::spin_loop();
    }

    if i >= timeout {
        mod_error!(b"virtio_blk: Request timeout\n");
        return -5;
    }

    // Process completion
    queue.last_used_idx = queue.last_used_idx.wrapping_add(1);

    // Return descriptors to free list
    queue.free_head = head;
    queue.num_free += total_descs;

    // Check status
    let status = *status_ptr;
    if status != VIRTIO_BLK_S_OK {
        mod_error!(b"virtio_blk: Request failed with status\n");
        return -6;
    }

    0
}

// ============================================================================
// Module Entry Points
// ============================================================================

/// Module entry point table
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

/// Module initialization
#[no_mangle]
#[inline(never)]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"virtio_blk module: initializing...\n");

    // Create driver name
    let mut name = [0u8; 32];
    let name_bytes = b"virtio_blk";
    name[..name_bytes.len()].copy_from_slice(name_bytes);

    // Create operations table
    let ops = BlockDriverOps {
        name,
        probe: Some(virtio_blk_probe),
        new: Some(virtio_blk_new),
        destroy: Some(virtio_blk_destroy),
        init: Some(virtio_blk_init),
        get_info: Some(virtio_blk_get_info),
        read: Some(virtio_blk_read),
        write: Some(virtio_blk_write),
        flush: Some(virtio_blk_flush),
    };

    // Register with the kernel
    let result = unsafe { kmod_blk_register(&ops) };

    if result != 0 {
        mod_error!(b"virtio_blk module: failed to register with kernel\n");
        return -1;
    }

    mod_info!(b"virtio_blk module: initialized successfully\n");
    0
}

/// Module cleanup
#[no_mangle]
#[inline(never)]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"virtio_blk module: unloading...\n");

    let name = b"virtio_blk";
    unsafe {
        kmod_blk_unregister(name.as_ptr(), name.len());
    }

    mod_info!(b"virtio_blk module: unloaded\n");
    0
}

// ============================================================================
// Panic Handler (required for no_std)
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    mod_error!(b"virtio_blk: PANIC!\n");
    loop {
        core::hint::spin_loop();
    }
}
