//! ext2 Filesystem Module for NexaOS
//!
//! This module provides ext2 filesystem support as a loadable kernel module.
//! The implementation is compiled into the kernel, but registered as a module
//! for better organization and potential future dynamic loading.
//!
//! ## Module Info
//! - Name: ext2
//! - Type: Filesystem
//! - Version: 1.0.0
//! - Dependencies: None (built-in)

#![no_std]

/// Module metadata
pub const MODULE_NAME: &str = "ext2";
pub const MODULE_VERSION: &str = "1.0.0";
pub const MODULE_DESCRIPTION: &str = "ext2 filesystem driver";
pub const MODULE_TYPE: u8 = 1; // Filesystem

/// Module initialization function
/// This is called when the module is loaded
pub fn init() -> Result<(), &'static str> {
    // ext2 initialization is handled by the kernel's fs::ext2 module
    // This is a placeholder for the modular architecture
    Ok(())
}

/// Module cleanup function
/// This is called when the module is unloaded
pub fn cleanup() -> Result<(), &'static str> {
    // Cleanup would unmount all ext2 filesystems
    // For now, ext2 cannot be unloaded while in use
    Ok(())
}

/// Check if the module is in use
pub fn in_use() -> bool {
    // Check if any ext2 filesystems are currently mounted
    // For safety, we always return true to prevent unloading
    true
}
