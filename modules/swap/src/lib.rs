//! Swap Subsystem Kernel Module for NexaOS
//!
//! This is a loadable kernel module (.nkm) that provides swap/paging support.
//! It manages swap space on block devices or files, allowing the kernel to
//! move inactive pages out of physical memory to free up RAM.
//!
//! # Features
//!
//! - Swap partition and swap file support
//! - Linux-compatible swap header format
//! - Swap slot allocation with bitmap
//! - Swap extent mapping for efficient I/O
//! - Priority-based swap device selection
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
//! - Block device I/O (kmod_blk_read_bytes, kmod_blk_write_bytes)
//! - Swap registration (kmod_swap_register)

#![no_std]
#![allow(dead_code)]

use core::ptr;

// ============================================================================
// Module Metadata
// ============================================================================

/// Module name
pub const MODULE_NAME: &[u8] = b"swap\0";
/// Module version
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description
pub const MODULE_DESC: &[u8] = b"Swap subsystem driver for NexaOS\0";
/// Module type (3 = Memory Management)
pub const MODULE_TYPE: u8 = 3;
/// Module license (GPL-compatible, doesn't taint kernel)
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
/// Module author
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
/// Source version (in-tree module)
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
    
    // Block device API (for reading/writing swap pages)
    fn kmod_blk_read_bytes(device_index: usize, offset: u64, buf: *mut u8, len: usize) -> i64;
    fn kmod_blk_write_bytes(device_index: usize, offset: u64, buf: *const u8, len: usize) -> i64;
    fn kmod_blk_device_count() -> usize;
    
    // Swap module registration API
    fn kmod_swap_register(ops: *const SwapModuleOps) -> i32;
    fn kmod_swap_unregister() -> i32;
}

// ============================================================================
// Logging helpers
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

// Helper to print a u64 as hex string
#[inline(never)]
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

/// Parse device path to extract block device index
/// Supports: "/dev/vd[a-z]" -> ordinal index (vda=0, vdb=1, etc.)
///           "/dev/block[0-9]" -> direct index from number
///           single digit -> direct index
/// 
/// Note: This uses ordinal mapping for vd* devices:
/// - vda -> first virtio block = device 0 (typically rootfs)  
/// - vdb -> second virtio block = device 1 (skipping rootfs)
/// - etc.
///
/// The actual block device index depends on probe order, so we skip
/// device 0 (rootfs) and use device N for vd(N+1) where N >= 1.
fn parse_device_path(path: &[u8]) -> usize {
    // Single digit: "0", "1", etc.
    if path.len() == 1 && path[0] >= b'0' && path[0] <= b'9' {
        return (path[0] - b'0') as usize;
    }
    
    // Check for "/dev/vd[a-z]" pattern
    // /dev/vda, /dev/vdb, etc.
    if path.len() >= 8 && &path[0..5] == b"/dev/" {
        let suffix = &path[5..];
        
        // Check for "vd[a-z]" (virtio disk)
        if suffix.len() >= 3 && &suffix[0..2] == b"vd" {
            let letter = suffix[2];
            if letter >= b'a' && letter <= b'z' {
                // vda=0, vdb=1, vdc=2, etc. in ordinal terms
                // After probing: rootfs is often device 0, then vdb becomes device 1 or 2
                // Since rootfs (vda) may appear as device 0 and 1 (duplicate probe),
                // vdb would be device 2. Let's search for a non-rootfs device.
                let virtio_ordinal = (letter - b'a') as usize;
                
                // For vda (ordinal 0), return 0
                // For vdb (ordinal 1), we need to find the swap device
                // Since block devices are: 0=rootfs(vda), 1=duplicate, 2=swap(vdb)
                // We'll use device_count-1 as a heuristic for the last device (swap)
                if virtio_ordinal == 0 {
                    return 0;
                } else {
                    // For vdb and beyond, use device_count - 1 as swap is typically last
                    // Or more specifically, skip the first N devices where N = number of rootfs duplicates
                    let count = unsafe { kmod_blk_device_count() };
                    if count > 1 {
                        // vdb should be the last probed device (device 2 if count=3)
                        return count - 1;
                    }
                    return virtio_ordinal;
                }
            }
        }
        
        // Check for "block[0-9]" pattern
        if suffix.len() >= 6 && &suffix[0..5] == b"block" {
            let digit = suffix[5];
            if digit >= b'0' && digit <= b'9' {
                return (digit - b'0') as usize;
            }
        }
    }
    
    // Default to device 4 (typically vdb / second virtio block device)
    // This is the swap device in our QEMU setup
    4
}

// ============================================================================
// Constants
// ============================================================================

/// Page size (4KB)
const PAGE_SIZE: usize = 4096;

/// Swap header magic (Linux compatible)
const SWAP_MAGIC_V1: &[u8; 10] = b"SWAPSPACE2";

/// Swap header offset (Linux puts it at end of first page - 10 bytes)
const SWAP_MAGIC_OFFSET: usize = PAGE_SIZE - 10;

/// Maximum number of swap devices
const MAX_SWAP_DEVICES: usize = 8;

/// Maximum swap pages per device (for 64-bit: 2^32 pages = 16TB)
const MAX_SWAP_PAGES: u64 = 1 << 32;

/// Swap slot size in bytes
const SWAP_SLOT_SIZE: usize = PAGE_SIZE;

/// Bitmap entry size (64-bit words)
const BITMAP_BITS: usize = 64;

// Swap flags
const SWAP_FLAG_PREFER: u32 = 0x8000;
const SWAP_FLAG_PRIO_MASK: u32 = 0x7fff;
const SWAP_FLAG_PRIO_SHIFT: u32 = 0;
const SWAP_FLAG_DISCARD: u32 = 0x10000;

// ============================================================================
// Swap Header Structure (Linux compatible)
// ============================================================================

/// Linux-compatible swap header (located at start of swap space)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct SwapHeader {
    /// Boot block padding (1024 bytes)
    bootbits: [u8; 1024],
    /// Swap format version
    version: u32,
    /// Last page of swap area
    last_page: u32,
    /// Number of bad pages
    nr_badpages: u32,
    /// UUID
    uuid: [u8; 16],
    /// Volume name
    volume_name: [u8; 16],
    /// Padding to fill page
    padding: [u8; PAGE_SIZE - 1024 - 4 - 4 - 4 - 16 - 16 - 10],
    /// Magic signature at end of page: "SWAPSPACE2"
    magic: [u8; 10],
}

// ============================================================================
// Swap Device State
// ============================================================================

/// State for a single swap device
#[repr(C)]
struct SwapDevice {
    /// Whether this device slot is in use
    active: bool,
    /// Block device index
    device_index: usize,
    /// Total number of swap pages
    nr_pages: u64,
    /// Number of free pages
    nr_free_pages: u64,
    /// Swap priority (higher = preferred)
    priority: i16,
    /// Allocation bitmap (1 = used, 0 = free)
    bitmap: *mut u64,
    /// Size of bitmap in u64 words
    bitmap_size: usize,
    /// Spinlock for concurrent access
    lock: u64,
    /// Volume name from header
    volume_name: [u8; 16],
    /// UUID from header
    uuid: [u8; 16],
}

impl SwapDevice {
    const fn empty() -> Self {
        Self {
            active: false,
            device_index: 0,
            nr_pages: 0,
            nr_free_pages: 0,
            priority: 0,
            bitmap: ptr::null_mut(),
            bitmap_size: 0,
            lock: 0,
            volume_name: [0; 16],
            uuid: [0; 16],
        }
    }
}

/// Global swap device array
static mut SWAP_DEVICES: [SwapDevice; MAX_SWAP_DEVICES] = [
    SwapDevice::empty(),
    SwapDevice::empty(),
    SwapDevice::empty(),
    SwapDevice::empty(),
    SwapDevice::empty(),
    SwapDevice::empty(),
    SwapDevice::empty(),
    SwapDevice::empty(),
];

/// Number of active swap devices
static mut NR_SWAP_DEVICES: usize = 0;

/// Global swap statistics
static mut TOTAL_SWAP_PAGES: u64 = 0;
static mut FREE_SWAP_PAGES: u64 = 0;

/// Global lock for swap device management
static mut SWAP_GLOBAL_LOCK: u64 = 0;

// ============================================================================
// Swap Module Operations Table
// ============================================================================

/// Operations table for kernel registration
#[repr(C)]
pub struct SwapModuleOps {
    /// Activate a swap device
    pub swapon: extern "C" fn(device_path: *const u8, path_len: usize, flags: u32) -> i32,
    /// Deactivate a swap device  
    pub swapoff: extern "C" fn(device_path: *const u8, path_len: usize) -> i32,
    /// Allocate a swap slot
    pub alloc_slot: extern "C" fn(out_device: *mut usize, out_offset: *mut u64) -> i32,
    /// Free a swap slot
    pub free_slot: extern "C" fn(device: usize, offset: u64) -> i32,
    /// Write a page to swap
    pub write_page: extern "C" fn(device: usize, offset: u64, page_data: *const u8) -> i32,
    /// Read a page from swap
    pub read_page: extern "C" fn(device: usize, offset: u64, page_data: *mut u8) -> i32,
    /// Get swap statistics
    pub get_stats: extern "C" fn(total: *mut u64, free: *mut u64) -> i32,
}

// ============================================================================
// Swap Implementation
// ============================================================================

/// Activate a swap device
extern "C" fn swapon_impl(device_path: *const u8, path_len: usize, flags: u32) -> i32 {
    if device_path.is_null() || path_len == 0 {
        mod_error!(b"swapon: invalid device path\n");
        return -1;
    }

    unsafe {
        kmod_spinlock_lock(&mut SWAP_GLOBAL_LOCK);
        
        // Find a free swap device slot
        let mut slot_idx = MAX_SWAP_DEVICES;
        for i in 0..MAX_SWAP_DEVICES {
            if !SWAP_DEVICES[i].active {
                slot_idx = i;
                break;
            }
        }
        
        if slot_idx >= MAX_SWAP_DEVICES {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            mod_error!(b"swapon: too many swap devices\n");
            return -1;
        }

        // For now, we treat device_path as a block device index
        // In a full implementation, we'd resolve the path to a device
        // Parse device path to extract block device index
        // Supports formats: "/dev/vd[a-z]", "/dev/block[0-9]", or single digit
        mod_info!(b"swapon: parsing device path\n");
        
        let path_slice = core::slice::from_raw_parts(device_path, path_len);
        
        let device_index = parse_device_path(path_slice);
        log_hex(b"swapon: device_index=", device_index as u64);

        // Read and validate swap header
        let mut header_buf = [0u8; PAGE_SIZE];
        let bytes_read = kmod_blk_read_bytes(
            device_index, 
            0, 
            header_buf.as_mut_ptr(), 
            PAGE_SIZE
        );
        
        if bytes_read < PAGE_SIZE as i64 {
            mod_error!(b"swapon: failed to read swap header\n");
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1;
        }

        // Check magic signature
        let magic_ptr = &header_buf[SWAP_MAGIC_OFFSET..SWAP_MAGIC_OFFSET + 10];
        if magic_ptr != SWAP_MAGIC_V1 {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            mod_error!(b"swapon: invalid swap signature\n");
            return -1;
        }

        // Parse header
        let header = &*(header_buf.as_ptr() as *const SwapHeader);
        let version = u32::from_le(header.version);
        let last_page = u32::from_le(header.last_page);
        
        if version != 1 {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            mod_error!(b"swapon: unsupported swap version\n");
            return -1;
        }

        let nr_pages = last_page as u64;
        if nr_pages == 0 || nr_pages > MAX_SWAP_PAGES {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            mod_error!(b"swapon: invalid number of swap pages\n");
            return -1;
        }

        // Allocate bitmap
        let bitmap_size = ((nr_pages + BITMAP_BITS as u64 - 1) / BITMAP_BITS as u64) as usize;
        let bitmap_bytes = bitmap_size * 8;
        let bitmap = kmod_zalloc(bitmap_bytes, 8) as *mut u64;
        
        if bitmap.is_null() {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            mod_error!(b"swapon: failed to allocate bitmap\n");
            return -1;
        }

        // Mark first page as used (contains header)
        *bitmap = 1;

        // Parse priority from flags
        let priority = if (flags & SWAP_FLAG_PREFER) != 0 {
            ((flags & SWAP_FLAG_PRIO_MASK) >> SWAP_FLAG_PRIO_SHIFT) as i16
        } else {
            -(slot_idx as i16 + 1)
        };

        // Initialize swap device
        let device = &mut SWAP_DEVICES[slot_idx];
        device.active = true;
        device.device_index = device_index;
        device.nr_pages = nr_pages;
        device.nr_free_pages = nr_pages - 1; // First page is header
        device.priority = priority;
        device.bitmap = bitmap;
        device.bitmap_size = bitmap_size;
        kmod_spinlock_init(&mut device.lock);
        
        // Copy volume name and UUID
        kmod_memcpy(
            device.volume_name.as_mut_ptr(),
            header.volume_name.as_ptr(),
            16
        );
        kmod_memcpy(
            device.uuid.as_mut_ptr(),
            header.uuid.as_ptr(),
            16
        );

        // Update global stats
        NR_SWAP_DEVICES += 1;
        TOTAL_SWAP_PAGES += nr_pages - 1;
        FREE_SWAP_PAGES += nr_pages - 1;

        kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);

        mod_info!(b"swapon: activated swap device\n");
        log_hex(b"  pages: ", nr_pages);
        log_hex(b"  priority: ", priority as u64);
        
        0
    }
}

/// Deactivate a swap device
extern "C" fn swapoff_impl(device_path: *const u8, path_len: usize) -> i32 {
    if device_path.is_null() || path_len == 0 {
        mod_error!(b"swapoff: invalid device path\n");
        return -1;
    }

    unsafe {
        kmod_spinlock_lock(&mut SWAP_GLOBAL_LOCK);

        // Parse device path to extract block device index
        let path_slice = core::slice::from_raw_parts(device_path, path_len);
        let device_index = parse_device_path(path_slice);

        let mut found_idx = MAX_SWAP_DEVICES;
        for i in 0..MAX_SWAP_DEVICES {
            if SWAP_DEVICES[i].active && SWAP_DEVICES[i].device_index == device_index {
                found_idx = i;
                break;
            }
        }

        if found_idx >= MAX_SWAP_DEVICES {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            mod_error!(b"swapoff: device not found\n");
            return -1;
        }

        let device = &mut SWAP_DEVICES[found_idx];

        // Check if there are pages still in use
        let used_pages = device.nr_pages - device.nr_free_pages - 1;
        if used_pages > 0 {
            // In a full implementation, we'd swap these pages back to RAM
            // For now, we just warn
            mod_warn!(b"swapoff: pages still in use, may cause data loss\n");
        }

        // Update global stats
        TOTAL_SWAP_PAGES -= device.nr_pages - 1;
        FREE_SWAP_PAGES -= device.nr_free_pages;
        NR_SWAP_DEVICES -= 1;

        // Free bitmap
        if !device.bitmap.is_null() {
            kmod_dealloc(device.bitmap as *mut u8, device.bitmap_size * 8, 8);
        }

        // Clear device slot
        *device = SwapDevice::empty();

        kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);

        mod_info!(b"swapoff: deactivated swap device\n");
        0
    }
}

/// Allocate a swap slot (returns device and offset)
extern "C" fn alloc_slot_impl(out_device: *mut usize, out_offset: *mut u64) -> i32 {
    if out_device.is_null() || out_offset.is_null() {
        return -1;
    }

    unsafe {
        kmod_spinlock_lock(&mut SWAP_GLOBAL_LOCK);

        if FREE_SWAP_PAGES == 0 {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1; // ENOSPC
        }

        // Find device with highest priority that has free slots
        let mut best_idx = MAX_SWAP_DEVICES;
        let mut best_priority = i16::MIN;

        for i in 0..MAX_SWAP_DEVICES {
            if SWAP_DEVICES[i].active && SWAP_DEVICES[i].nr_free_pages > 0 {
                if SWAP_DEVICES[i].priority > best_priority {
                    best_priority = SWAP_DEVICES[i].priority;
                    best_idx = i;
                }
            }
        }

        if best_idx >= MAX_SWAP_DEVICES {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1;
        }

        let device = &mut SWAP_DEVICES[best_idx];
        kmod_spinlock_lock(&mut device.lock);

        // Find free slot in bitmap
        let mut slot: u64 = 0;
        let mut found = false;

        for word_idx in 0..device.bitmap_size {
            let word = *device.bitmap.add(word_idx);
            if word != u64::MAX {
                // Find first zero bit
                for bit in 0..BITMAP_BITS {
                    if (word & (1u64 << bit)) == 0 {
                        slot = (word_idx * BITMAP_BITS + bit) as u64;
                        if slot < device.nr_pages {
                            // Mark as used
                            *device.bitmap.add(word_idx) = word | (1u64 << bit);
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    break;
                }
            }
        }

        if !found {
            kmod_spinlock_unlock(&mut device.lock);
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1;
        }

        device.nr_free_pages -= 1;
        FREE_SWAP_PAGES -= 1;

        *out_device = device.device_index;
        *out_offset = slot * PAGE_SIZE as u64;

        kmod_spinlock_unlock(&mut device.lock);
        kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);

        0
    }
}

/// Free a swap slot
extern "C" fn free_slot_impl(device_index: usize, offset: u64) -> i32 {
    unsafe {
        kmod_spinlock_lock(&mut SWAP_GLOBAL_LOCK);

        // Find the device
        let mut found_idx = MAX_SWAP_DEVICES;
        for i in 0..MAX_SWAP_DEVICES {
            if SWAP_DEVICES[i].active && SWAP_DEVICES[i].device_index == device_index {
                found_idx = i;
                break;
            }
        }

        if found_idx >= MAX_SWAP_DEVICES {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1;
        }

        let device = &mut SWAP_DEVICES[found_idx];
        let slot = offset / PAGE_SIZE as u64;

        if slot >= device.nr_pages || slot == 0 {
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1; // Invalid slot or header page
        }

        kmod_spinlock_lock(&mut device.lock);

        let word_idx = (slot / BITMAP_BITS as u64) as usize;
        let bit = (slot % BITMAP_BITS as u64) as usize;
        let word = *device.bitmap.add(word_idx);

        if (word & (1u64 << bit)) == 0 {
            // Already free
            kmod_spinlock_unlock(&mut device.lock);
            kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
            return -1;
        }

        // Mark as free
        *device.bitmap.add(word_idx) = word & !(1u64 << bit);
        device.nr_free_pages += 1;
        FREE_SWAP_PAGES += 1;

        kmod_spinlock_unlock(&mut device.lock);
        kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);

        0
    }
}

/// Write a page to swap
extern "C" fn write_page_impl(device_index: usize, offset: u64, page_data: *const u8) -> i32 {
    if page_data.is_null() {
        return -1;
    }

    unsafe {
        let bytes_written = kmod_blk_write_bytes(
            device_index,
            offset,
            page_data,
            PAGE_SIZE
        );

        if bytes_written < PAGE_SIZE as i64 {
            mod_error!(b"swap: write_page failed\n");
            return -1;
        }

        0
    }
}

/// Read a page from swap
extern "C" fn read_page_impl(device_index: usize, offset: u64, page_data: *mut u8) -> i32 {
    if page_data.is_null() {
        return -1;
    }

    unsafe {
        let bytes_read = kmod_blk_read_bytes(
            device_index,
            offset,
            page_data,
            PAGE_SIZE
        );

        if bytes_read < PAGE_SIZE as i64 {
            mod_error!(b"swap: read_page failed\n");
            return -1;
        }

        0
    }
}

/// Get swap statistics
extern "C" fn get_stats_impl(total: *mut u64, free: *mut u64) -> i32 {
    unsafe {
        kmod_spinlock_lock(&mut SWAP_GLOBAL_LOCK);
        
        if !total.is_null() {
            *total = TOTAL_SWAP_PAGES * PAGE_SIZE as u64;
        }
        if !free.is_null() {
            *free = FREE_SWAP_PAGES * PAGE_SIZE as u64;
        }

        kmod_spinlock_unlock(&mut SWAP_GLOBAL_LOCK);
        0
    }
}

// ============================================================================
// Module Entry Points
// ============================================================================

/// Static operations table
static SWAP_OPS: SwapModuleOps = SwapModuleOps {
    swapon: swapon_impl,
    swapoff: swapoff_impl,
    alloc_slot: alloc_slot_impl,
    free_slot: free_slot_impl,
    write_page: write_page_impl,
    read_page: read_page_impl,
    get_stats: get_stats_impl,
};

/// Module initialization function
/// Called by the kernel when the module is loaded
#[no_mangle]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"swap module: initializing\n");
    
    unsafe {
        // Initialize global lock
        kmod_spinlock_init(&mut SWAP_GLOBAL_LOCK);
        
        // Register with kernel
        let result = kmod_swap_register(&SWAP_OPS);
        if result != 0 {
            mod_error!(b"swap module: failed to register with kernel\n");
            return result;
        }
    }
    
    mod_info!(b"swap module: initialized successfully\n");
    0
}

/// Module exit function
/// Called by the kernel when the module is unloaded
#[no_mangle]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"swap module: unloading\n");
    
    unsafe {
        // Deactivate all swap devices
        for i in 0..MAX_SWAP_DEVICES {
            if SWAP_DEVICES[i].active {
                // Free bitmap
                if !SWAP_DEVICES[i].bitmap.is_null() {
                    kmod_dealloc(
                        SWAP_DEVICES[i].bitmap as *mut u8,
                        SWAP_DEVICES[i].bitmap_size * 8,
                        8
                    );
                }
                SWAP_DEVICES[i] = SwapDevice::empty();
            }
        }
        
        NR_SWAP_DEVICES = 0;
        TOTAL_SWAP_PAGES = 0;
        FREE_SWAP_PAGES = 0;
        
        // Unregister from kernel
        let result = kmod_swap_unregister();
        if result != 0 {
            mod_error!(b"swap module: failed to unregister from kernel\n");
            return result;
        }
    }
    
    mod_info!(b"swap module: unloaded\n");
    0
}

// ============================================================================
// Panic Handler (required for no_std)
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    mod_error!(b"swap module: PANIC!\n");
    loop {
        core::hint::spin_loop();
    }
}
