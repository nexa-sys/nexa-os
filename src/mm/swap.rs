//! Swap Subsystem Support for NexaOS
//!
//! This module provides the kernel-side infrastructure for swap support.
//! The actual swap logic is implemented in a loadable kernel module,
//! while this module provides:
//!
//! - Registration/unregistration API for the swap module
//! - Swap operations dispatch to the loaded module
//! - Integration with the memory management subsystem
//! - swapon/swapoff system call handlers
//!
//! # Swap Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  User Space                         │
//! │  swapon /dev/sda2    swapoff /dev/sda2              │
//! └───────────────────────┬─────────────────────────────┘
//!                         │ syscall
//! ┌───────────────────────▼─────────────────────────────┐
//! │                Kernel (this module)                 │
//! │  - SYS_SWAPON / SYS_SWAPOFF handlers                │
//! │  - Swap module registration                         │
//! │  - swap_in() / swap_out() for page reclaim          │
//! └───────────────────────┬─────────────────────────────┘
//!                         │ kmod API
//! ┌───────────────────────▼─────────────────────────────┐
//! │            Swap Kernel Module (swap.nkm)            │
//! │  - Swap header parsing                              │
//! │  - Slot allocation bitmap                           │
//! │  - Block device I/O                                 │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! The swap module must be loaded before swap can be used:
//!
//! ```ignore
//! // In kernel init or userspace
//! modprobe swap
//!
//! // Then from userspace
//! swapon /dev/sda2
//! ```

use crate::{kerror, kinfo, kwarn};
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use spin::Mutex;

/// Page size constant
pub const PAGE_SIZE: usize = 4096;

/// Swap entry encoding
/// Format: [device:8][offset:56]
/// - device: 8-bit device index (0-255)
/// - offset: 56-bit page offset within device
pub type SwapEntry = u64;

/// Encode a swap entry from device and offset
#[inline]
pub fn encode_swap_entry(device: u8, offset: u64) -> SwapEntry {
    ((device as u64) << 56) | (offset & 0x00FFFFFFFFFFFFFF)
}

/// Decode device from swap entry
#[inline]
pub fn decode_swap_device(entry: SwapEntry) -> u8 {
    (entry >> 56) as u8
}

/// Decode offset from swap entry
#[inline]
pub fn decode_swap_offset(entry: SwapEntry) -> u64 {
    entry & 0x00FFFFFFFFFFFFFF
}

/// Check if a swap entry is valid (non-zero)
#[inline]
pub fn is_swap_entry_valid(entry: SwapEntry) -> bool {
    entry != 0
}

// ============================================================================
// Swap Module Operations (from loaded .nkm module)
// ============================================================================

/// Operations table provided by the swap kernel module
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

/// Whether the swap module is registered
static SWAP_MODULE_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Pointer to the swap module operations table
static SWAP_MODULE_OPS: AtomicPtr<SwapModuleOps> = AtomicPtr::new(core::ptr::null_mut());

/// Lock for swap module registration
static SWAP_REGISTER_LOCK: Mutex<()> = Mutex::new(());

// ============================================================================
// Module Registration API (called by swap.nkm)
// ============================================================================

/// Register the swap module with the kernel
/// Called by kmod_swap_register in the loadable module
#[no_mangle]
pub extern "C" fn kmod_swap_register(ops: *const SwapModuleOps) -> i32 {
    let _guard = SWAP_REGISTER_LOCK.lock();

    if ops.is_null() {
        kerror!("swap: cannot register null operations table");
        return -1;
    }

    if SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        kerror!("swap: module already registered");
        return -1;
    }

    // Store the operations pointer
    SWAP_MODULE_OPS.store(ops as *mut SwapModuleOps, Ordering::SeqCst);
    SWAP_MODULE_REGISTERED.store(true, Ordering::SeqCst);

    kinfo!("swap: module registered successfully");
    0
}

/// Unregister the swap module from the kernel
/// Called by kmod_swap_unregister in the loadable module
#[no_mangle]
pub extern "C" fn kmod_swap_unregister() -> i32 {
    let _guard = SWAP_REGISTER_LOCK.lock();

    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        kwarn!("swap: module not registered");
        return -1;
    }

    SWAP_MODULE_OPS.store(core::ptr::null_mut(), Ordering::SeqCst);
    SWAP_MODULE_REGISTERED.store(false, Ordering::SeqCst);

    kinfo!("swap: module unregistered");
    0
}

/// Check if the swap module is available
pub fn is_swap_available() -> bool {
    SWAP_MODULE_REGISTERED.load(Ordering::SeqCst)
}

// ============================================================================
// Swap Operations API (for kernel use)
// ============================================================================

/// Activate a swap area
///
/// # Arguments
/// * `device_path` - Path to the swap device or file
/// * `flags` - Swap flags (SWAP_FLAG_*)
///
/// # Returns
/// * 0 on success
/// * Negative error code on failure
pub fn swapon(device_path: &[u8], flags: u32) -> i32 {
    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        kerror!("swap: module not loaded");
        return -crate::posix::errno::ENOSYS as i32;
    }

    let ops = SWAP_MODULE_OPS.load(Ordering::SeqCst);
    if ops.is_null() {
        return -crate::posix::errno::ENOSYS as i32;
    }

    unsafe {
        ((*ops).swapon)(device_path.as_ptr(), device_path.len(), flags)
    }
}

/// Deactivate a swap area
///
/// # Arguments
/// * `device_path` - Path to the swap device or file
///
/// # Returns
/// * 0 on success
/// * Negative error code on failure
pub fn swapoff(device_path: &[u8]) -> i32 {
    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        kerror!("swap: module not loaded");
        return -crate::posix::errno::ENOSYS as i32;
    }

    let ops = SWAP_MODULE_OPS.load(Ordering::SeqCst);
    if ops.is_null() {
        return -crate::posix::errno::ENOSYS as i32;
    }

    unsafe {
        ((*ops).swapoff)(device_path.as_ptr(), device_path.len())
    }
}

/// Swap out a page to swap space
///
/// # Arguments
/// * `page_data` - Pointer to the page data (PAGE_SIZE bytes)
///
/// # Returns
/// * Ok(SwapEntry) - Swap entry that can be used to retrieve the page
/// * Err(error_code) - Negative error code on failure
pub fn swap_out(page_data: *const u8) -> Result<SwapEntry, i32> {
    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        return Err(-crate::posix::errno::ENOSYS as i32);
    }

    let ops = SWAP_MODULE_OPS.load(Ordering::SeqCst);
    if ops.is_null() {
        return Err(-crate::posix::errno::ENOSYS as i32);
    }

    unsafe {
        let mut device: usize = 0;
        let mut offset: u64 = 0;

        // Allocate a swap slot
        let result = ((*ops).alloc_slot)(&mut device, &mut offset);
        if result != 0 {
            return Err(result);
        }

        // Write the page to swap
        let result = ((*ops).write_page)(device, offset, page_data);
        if result != 0 {
            // Free the slot on write failure
            let _ = ((*ops).free_slot)(device, offset);
            return Err(result);
        }

        // Return the swap entry
        Ok(encode_swap_entry(device as u8, offset))
    }
}

/// Swap in a page from swap space
///
/// # Arguments
/// * `entry` - Swap entry returned from swap_out
/// * `page_data` - Pointer to buffer to receive page data (PAGE_SIZE bytes)
///
/// # Returns
/// * Ok(()) on success
/// * Err(error_code) on failure
pub fn swap_in(entry: SwapEntry, page_data: *mut u8) -> Result<(), i32> {
    if !is_swap_entry_valid(entry) {
        return Err(-crate::posix::errno::EINVAL as i32);
    }

    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        return Err(-crate::posix::errno::ENOSYS as i32);
    }

    let ops = SWAP_MODULE_OPS.load(Ordering::SeqCst);
    if ops.is_null() {
        return Err(-crate::posix::errno::ENOSYS as i32);
    }

    let device = decode_swap_device(entry) as usize;
    let offset = decode_swap_offset(entry);

    unsafe {
        // Read the page from swap
        let result = ((*ops).read_page)(device, offset, page_data);
        if result != 0 {
            return Err(result);
        }

        Ok(())
    }
}

/// Free a swap entry (release the swap slot)
///
/// # Arguments
/// * `entry` - Swap entry to free
///
/// # Returns
/// * Ok(()) on success
/// * Err(error_code) on failure
pub fn swap_free(entry: SwapEntry) -> Result<(), i32> {
    if !is_swap_entry_valid(entry) {
        return Err(-crate::posix::errno::EINVAL as i32);
    }

    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        return Err(-crate::posix::errno::ENOSYS as i32);
    }

    let ops = SWAP_MODULE_OPS.load(Ordering::SeqCst);
    if ops.is_null() {
        return Err(-crate::posix::errno::ENOSYS as i32);
    }

    let device = decode_swap_device(entry) as usize;
    let offset = decode_swap_offset(entry);

    unsafe {
        let result = ((*ops).free_slot)(device, offset);
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }
}

/// Get swap statistics
///
/// # Returns
/// * (total_bytes, free_bytes) tuple
pub fn get_swap_stats() -> (u64, u64) {
    if !SWAP_MODULE_REGISTERED.load(Ordering::SeqCst) {
        return (0, 0);
    }

    let ops = SWAP_MODULE_OPS.load(Ordering::SeqCst);
    if ops.is_null() {
        return (0, 0);
    }

    unsafe {
        let mut total: u64 = 0;
        let mut free: u64 = 0;
        let result = ((*ops).get_stats)(&mut total, &mut free);
        if result != 0 {
            return (0, 0);
        }
        (total, free)
    }
}

// ============================================================================
// Swap Flags (Linux compatible)
// ============================================================================

/// Prefer this swap device
pub const SWAP_FLAG_PREFER: u32 = 0x8000;
/// Priority mask
pub const SWAP_FLAG_PRIO_MASK: u32 = 0x7fff;
/// Priority shift
pub const SWAP_FLAG_PRIO_SHIFT: u32 = 0;
/// Enable discard/TRIM for SSD
pub const SWAP_FLAG_DISCARD: u32 = 0x10000;
/// Discard once at swapon time
pub const SWAP_FLAG_DISCARD_ONCE: u32 = 0x20000;
/// Discard freed pages before reuse
pub const SWAP_FLAG_DISCARD_PAGES: u32 = 0x40000;

/// Construct swap flags with priority
pub fn make_swap_flags(priority: i16, discard: bool) -> u32 {
    let mut flags = SWAP_FLAG_PREFER | ((priority as u32) & SWAP_FLAG_PRIO_MASK);
    if discard {
        flags |= SWAP_FLAG_DISCARD;
    }
    flags
}

// ============================================================================
// Swap Info for /proc/swaps
// ============================================================================

/// Information about a swap area (for /proc/swaps)
#[derive(Debug, Clone)]
pub struct SwapInfo {
    /// Device path
    pub path: [u8; 64],
    /// Path length
    pub path_len: usize,
    /// Type: "partition" or "file"
    pub swap_type: [u8; 16],
    /// Total size in KB
    pub size_kb: u64,
    /// Used size in KB
    pub used_kb: u64,
    /// Priority
    pub priority: i16,
}

/// Get swap information for /proc/swaps
pub fn get_swap_info() -> Option<SwapInfo> {
    let (total, free) = get_swap_stats();
    if total == 0 {
        return None;
    }

    let size_kb = total / 1024;
    let used_kb = (total - free) / 1024;

    let mut info = SwapInfo {
        path: [0; 64],
        path_len: 0,
        swap_type: [0; 16],
        size_kb,
        used_kb,
        priority: 0,
    };

    // Default path placeholder
    let path = b"/dev/swap0";
    info.path[..path.len()].copy_from_slice(path);
    info.path_len = path.len();

    // Type
    let stype = b"partition";
    info.swap_type[..stype.len()].copy_from_slice(stype);

    Some(info)
}

// ============================================================================
// Page Tracking for Swap (PTE bit manipulation)
// ============================================================================

/// Check if a PTE indicates a swapped-out page
/// In NexaOS, we use the Present bit = 0 and store the swap entry in bits [63:1]
#[inline]
pub fn pte_is_swap(pte: u64) -> bool {
    // Not present, but has a non-zero swap entry
    (pte & 0x1) == 0 && (pte >> 1) != 0
}

/// Create a swap PTE from a swap entry
#[inline]
pub fn make_swap_pte(entry: SwapEntry) -> u64 {
    // Present = 0, swap entry in bits [63:1]
    entry << 1
}

/// Extract swap entry from a swap PTE
#[inline]
pub fn pte_to_swap_entry(pte: u64) -> SwapEntry {
    pte >> 1
}

// ============================================================================
// Debug and Statistics
// ============================================================================

/// Print swap statistics
pub fn print_swap_stats() {
    let (total, free) = get_swap_stats();
    
    if total == 0 {
        kinfo!("Swap: not configured");
        return;
    }

    let used = total - free;
    let total_mb = total / (1024 * 1024);
    let used_mb = used / (1024 * 1024);
    let free_mb = free / (1024 * 1024);

    kinfo!("=== Swap Statistics ===");
    kinfo!("  Total: {} MB", total_mb);
    kinfo!("  Used:  {} MB", used_mb);
    kinfo!("  Free:  {} MB", free_mb);
    if total > 0 {
        kinfo!("  Usage: {}%", (used * 100) / total);
    }
}
