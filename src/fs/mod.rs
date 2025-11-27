//! Filesystem subsystem for NexaOS
//!
//! This module contains filesystem-related functionality including:
//! - Virtual File System (VFS) layer
//! - ext2 filesystem support
//! - Initial RAM filesystem (initramfs/CPIO)
//! - procfs pseudo-filesystem (Linux-compatible /proc)
//! - sysfs pseudo-filesystem (Linux-compatible /sys)

pub mod ext2;
pub mod initramfs;
pub mod procfs;
pub mod sysfs;
pub mod vfs;

// Re-export commonly used items from vfs
pub use vfs::{
    add_directory, add_file, add_file_bytes, add_file_with_metadata, create_file,
    enable_ext2_write, file_exists, init, list_directory, list_files, mount_at, open,
    read_file, read_file_bytes, remount_root, stat, write_file, File, FileContent, OpenFile,
};

// Re-export from initramfs
pub use initramfs::{
    get as get_initramfs, init as init_initramfs, CpioNewcHeader, Initramfs, InitramfsEntry,
    GsData, GS_DATA,
};

// Re-export from ext2
pub use ext2::{
    global as ext2_global, register_global as ext2_register_global, Ext2Error, Ext2Filesystem,
    FileRef as Ext2FileRef,
};
