//! ext4 Filesystem Kernel Module for NexaOS
//!
//! This module provides ext4 filesystem support, building upon ext3.
//! ext4 adds extents, larger filesystem support, and improved performance.
//!
//! # Module Structure
//!
//! - `lib.rs` - Module entry points and registration
//! - `superblock.rs` - Superblock parsing and ext4 features
//! - `extent.rs` - Extent tree implementation
//! - `inode.rs` - Inode handling with ext4 extensions
//! - `ops.rs` - Filesystem operations
//! - `journal.rs` - JBD2 journaling support

#![no_std]
#![allow(dead_code)]

mod superblock;
mod extent;
mod inode;
mod ops;
mod journal;

pub use superblock::Ext4Superblock;
pub use extent::{ExtentHeader, ExtentIdx, Extent};
pub use inode::Ext4Inode;

// ============================================================================
// Module Metadata
// ============================================================================

pub const MODULE_NAME: &[u8] = b"ext4\0";
pub const MODULE_VERSION: &[u8] = b"1.0.0\0";
pub const MODULE_DESC: &[u8] = b"ext4 filesystem driver for NexaOS\0";
pub const MODULE_TYPE: u8 = 1;
pub const MODULE_LICENSE: &[u8] = b"MIT\0";
pub const MODULE_AUTHOR: &[u8] = b"NexaOS Team\0";
pub const MODULE_SRCVERSION: &[u8] = b"in-tree\0";
pub const MODULE_DEPENDS: &[u8] = b"ext3\0";

// ============================================================================
// Kernel API
// ============================================================================

extern "C" {
    pub fn kmod_log_info(msg: *const u8, len: usize);
    pub fn kmod_log_error(msg: *const u8, len: usize);
    pub fn kmod_log_warn(msg: *const u8, len: usize);
    pub fn kmod_log_debug(msg: *const u8, len: usize);
    fn kmod_ext4_register(ops: *const ops::Ext4ModuleOps) -> i32;
    fn kmod_ext4_unregister() -> i32;
    pub fn kmod_is_module_loaded(name: *const u8, name_len: usize) -> i32;
    pub fn kmod_blk_read_bytes(device_index: usize, offset: u64, buf: *mut u8, len: usize) -> i64;
    pub fn kmod_blk_write_bytes(device_index: usize, offset: u64, buf: *const u8, len: usize) -> i64;
    pub fn kmod_blk_find_rootfs() -> i32;
}

// ============================================================================
// Logging macros
// ============================================================================

#[macro_export]
macro_rules! mod_info {
    ($msg:expr) => { unsafe { crate::kmod_log_info($msg.as_ptr(), $msg.len()) } };
}

#[macro_export]
macro_rules! mod_error {
    ($msg:expr) => { unsafe { crate::kmod_log_error($msg.as_ptr(), $msg.len()) } };
}

#[macro_export]
macro_rules! mod_warn {
    ($msg:expr) => { unsafe { crate::kmod_log_warn($msg.as_ptr(), $msg.len()) } };
}

#[macro_export]
macro_rules! mod_debug {
    ($msg:expr) => { unsafe { crate::kmod_log_debug($msg.as_ptr(), $msg.len()) } };
}

// ============================================================================
// Module state
// ============================================================================

static mut EXT4_FS_INSTANCE: Option<ops::Ext4Filesystem> = None;
static mut MODULE_INITIALIZED: bool = false;
static mut EXT4_WRITABLE: bool = false;

/// Module entry points
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

// ============================================================================
// Module entry points
// ============================================================================

#[no_mangle]
#[inline(never)]
pub extern "C" fn module_init() -> i32 {
    mod_info!(b"ext4 module: initializing...");
    
    unsafe {
        if MODULE_INITIALIZED {
            mod_warn!(b"ext4 module: already initialized");
            return 0;
        }
        
        // Check ext3 dependency
        let ext3_name = b"ext3";
        let ext3_loaded = kmod_is_module_loaded(ext3_name.as_ptr(), ext3_name.len());
        if ext3_loaded == 0 {
            mod_warn!(b"ext4 module: ext3 not loaded, continuing standalone");
        }
        
        let module_ops = ops::Ext4ModuleOps::new();
        let result = kmod_ext4_register(&module_ops);
        
        if result != 0 {
            mod_error!(b"ext4 module: registration failed");
            return -1;
        }
        
        MODULE_INITIALIZED = true;
    }
    
    mod_info!(b"ext4 module: initialized");
    0
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn module_exit() -> i32 {
    mod_info!(b"ext4 module: unloading...");
    
    unsafe {
        if !MODULE_INITIALIZED { return 0; }
        
        if let Some(ref fs) = EXT4_FS_INSTANCE {
            journal::sync_journal(fs);
        }
        
        kmod_ext4_unregister();
        EXT4_FS_INSTANCE = None;
        MODULE_INITIALIZED = false;
    }
    
    mod_info!(b"ext4 module: unloaded");
    0
}

#[no_mangle]
pub extern "C" fn ext4_module_init() -> i32 { module_init() }

#[no_mangle]
pub extern "C" fn ext4_module_exit() -> i32 { module_exit() }

// ============================================================================
// Panic handler
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
