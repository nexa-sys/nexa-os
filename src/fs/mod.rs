//! Filesystem subsystem for NexaOS
//!
//! This module contains filesystem-related functionality including:
//! - Virtual File System (VFS) layer
//! - Filesystem abstraction traits (for pluggable filesystem support)
//! - Bridge adapters for trait interoperability
//! - Dynamic filesystem driver registration (for kernel modules like ext2)
//! - Initial RAM filesystem (initramfs/CPIO)
//! - procfs pseudo-filesystem (Linux-compatible /proc)
//! - sysfs pseudo-filesystem (Linux-compatible /sys)
//!
//! Note: ext2 filesystem support is now provided as a loadable kernel module.
//! See modules/ext2/ for the ext2 module implementation.

pub mod bridge;
pub mod initramfs;
pub mod procfs;
pub mod sysfs;
pub mod traits;
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

// Re-export filesystem abstraction traits
pub use traits::{
    BlockFileSystem, DirEntry, DynamicFileRef, FileSystemExt, FsDriverRegistry, FsError,
    FsFileHandle, FsOps, FsResult, FsStats, WritableFileSystem, FS_DRIVER_REGISTRY,
};

// Re-export bridge adapters
pub use bridge::{BlockFsVfsAdapter, VfsBlockFsAdapter};
