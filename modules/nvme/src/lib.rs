//! NVMe Block Device Driver for NexaOS
//!
//! This module provides NVMe (Non-Volatile Memory Express) storage device support
//! for the NexaOS kernel. NVMe is a high-performance storage protocol designed
//! specifically for SSDs connected via PCIe.
//!
//! ## Features
//!
//! - Full NVMe 1.4 command set support
//! - Multiple I/O queue support for parallelism
//! - Namespace management
//! - PRP (Physical Region Page) based DMA transfers
//! - Supports both small and large I/O operations
//!
//! ## Architecture
//!
//! ```text
//! +-------------------+
//! |   Block Layer     |  (kernel interface)
//! +-------------------+
//!          |
//! +-------------------+
//! |   driver.rs       |  (FFI callbacks)
//! +-------------------+
//!          |
//! +-------------------+
//! |  controller.rs    |  (controller management)
//! +-------------------+
//!          |
//! +-------------------+
//! |    queue.rs       |  (submission/completion queues)
//! +-------------------+
//!          |
//! +-------------------+
//! |    cmd.rs         |  (command structures)
//! +-------------------+
//!          |
//! +-------------------+
//! |    regs.rs        |  (register definitions)
//! +-------------------+
//! ```

#![no_std]
#![allow(dead_code)]

mod regs;
mod cmd;
mod queue;
mod controller;
mod driver;

pub use driver::*;

// =============================================================================
// Module Metadata
// =============================================================================

/// Module name (null-terminated)
pub const MODULE_NAME: &[u8] = b"nvme\0";
/// Module version (null-terminated)
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
/// Module description (null-terminated)
pub const MODULE_DESC: &[u8] = b"NVMe Block Device driver\0";
/// Module type (2 = Block device)
pub const MODULE_TYPE: u8 = 2;

// =============================================================================
// Kernel Module API (FFI)
// =============================================================================

extern "C" {
    // Logging
    pub fn kmod_log_info(msg: *const u8, len: usize);
    pub fn kmod_log_error(msg: *const u8, len: usize);
    pub fn kmod_log_warn(msg: *const u8, len: usize);
    pub fn kmod_log_debug(msg: *const u8, len: usize);

    // Memory allocation
    pub fn kmod_alloc(size: usize, align: usize) -> *mut u8;
    pub fn kmod_zalloc(size: usize, align: usize) -> *mut u8;
    pub fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);

    // Spinlocks
    pub fn kmod_spinlock_init(lock: *mut u64);
    pub fn kmod_spinlock_lock(lock: *mut u64);
    pub fn kmod_spinlock_unlock(lock: *mut u64);

    // Memory barriers
    pub fn kmod_fence();

    // MMIO operations
    pub fn kmod_mmio_read32(addr: u64) -> u32;
    pub fn kmod_mmio_write32(addr: u64, val: u32);

    // Address translation
    pub fn kmod_phys_to_virt(phys: u64) -> u64;
    pub fn kmod_virt_to_phys(virt: u64) -> u64;

    // Block device registration
    pub fn kmod_blk_register(ops: *const driver::BlockDriverOps) -> i32;
    pub fn kmod_blk_unregister(name: *const u8, name_len: usize) -> i32;
}

// =============================================================================
// Logging Macros
// =============================================================================

#[macro_export]
macro_rules! mod_info {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_info($msg.as_ptr(), $msg.len()) }
    };
}

#[macro_export]
macro_rules! mod_error {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_error($msg.as_ptr(), $msg.len()) }
    };
}

#[macro_export]
macro_rules! mod_warn {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_warn($msg.as_ptr(), $msg.len()) }
    };
}

#[macro_export]
macro_rules! mod_debug {
    ($msg:expr) => {
        unsafe { $crate::kmod_log_debug($msg.as_ptr(), $msg.len()) }
    };
}

// =============================================================================
// Panic Handler
// =============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    mod_error!(b"nvme: PANIC!\n");
    loop {
        core::hint::spin_loop();
    }
}
