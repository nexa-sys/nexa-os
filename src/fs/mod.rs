//! Filesystem subsystem for NexaOS
//!
//! This module contains filesystem-related functionality including:
//! - Virtual File System (VFS) layer
//! - Filesystem abstraction traits (for pluggable filesystem support)
//! - Bridge adapters for trait interoperability
//! - ext2 filesystem support
//! - ext2 adapter for abstract filesystem interface
//! - Initial RAM filesystem (initramfs/CPIO)
//! - procfs pseudo-filesystem (Linux-compatible /proc)
//! - sysfs pseudo-filesystem (Linux-compatible /sys)

pub mod bridge;
pub mod ext2;
pub mod ext2_adapter;
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

// Re-export from ext2
pub use ext2::{
    global as ext2_global, register_global as ext2_register_global, Ext2Error, Ext2Filesystem,
    Ext2Stats, FileRef as Ext2FileRef,
};

// Re-export filesystem abstraction traits
pub use traits::{
    BlockFileSystem, DirEntry, FileSystemExt, FsError, FsFileHandle, FsResult, FsStats,
    WritableFileSystem,
};

// Re-export ext2 adapter
pub use ext2_adapter::{create_ext2_adapter, global_ext2_adapter, Ext2Adapter};

// Re-export bridge adapters
pub use bridge::{BlockFsVfsAdapter, VfsBlockFsAdapter};
