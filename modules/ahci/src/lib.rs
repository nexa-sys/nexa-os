//! AHCI/SATA Block Device Driver for NexaOS
//!
//! Supports SATA drives via AHCI (Advanced Host Controller Interface).

#![no_std]
#![allow(dead_code)]

mod regs;
mod fis;
mod port;
mod driver;

pub use driver::*;

// Module metadata
pub const MODULE_NAME: &[u8] = b"ahci\0";
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
pub const MODULE_DESC: &[u8] = b"AHCI/SATA Block Device driver\0";
pub const MODULE_TYPE: u8 = 2; // Block device

// Kernel API
extern "C" {
    pub fn kmod_log_info(msg: *const u8, len: usize);
    pub fn kmod_log_error(msg: *const u8, len: usize);
    pub fn kmod_log_warn(msg: *const u8, len: usize);
    pub fn kmod_log_debug(msg: *const u8, len: usize);
    pub fn kmod_alloc(size: usize, align: usize) -> *mut u8;
    pub fn kmod_zalloc(size: usize, align: usize) -> *mut u8;
    pub fn kmod_dealloc(ptr: *mut u8, size: usize, align: usize);
    pub fn kmod_spinlock_init(lock: *mut u64);
    pub fn kmod_spinlock_lock(lock: *mut u64);
    pub fn kmod_spinlock_unlock(lock: *mut u64);
    pub fn kmod_fence();
    pub fn kmod_mmio_read32(addr: u64) -> u32;
    pub fn kmod_mmio_write32(addr: u64, val: u32);
    pub fn kmod_phys_to_virt(phys: u64) -> u64;
    pub fn kmod_virt_to_phys(virt: u64) -> u64;
    pub fn kmod_blk_register(ops: *const driver::BlockDriverOps) -> i32;
    pub fn kmod_blk_unregister(name: *const u8, name_len: usize) -> i32;
}

#[macro_export]
macro_rules! mod_info {
    ($msg:expr) => { unsafe { $crate::kmod_log_info($msg.as_ptr(), $msg.len()) } };
}

#[macro_export]
macro_rules! mod_error {
    ($msg:expr) => { unsafe { $crate::kmod_log_error($msg.as_ptr(), $msg.len()) } };
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    mod_error!(b"ahci: PANIC!\n");
    loop { core::hint::spin_loop(); }
}
