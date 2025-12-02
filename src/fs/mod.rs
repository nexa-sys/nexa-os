//! Filesystem subsystem for NexaOS
//!
//! This module contains filesystem-related functionality including:
//! - Virtual File System (VFS) layer
//! - Filesystem abstraction traits (for pluggable filesystem support)
//! - Bridge adapters for trait interoperability
//! - Modular ext2 filesystem support (loaded via kmod)
//! - Initial RAM filesystem (initramfs/CPIO)
//! - procfs pseudo-filesystem (Linux-compatible /proc)
//! - sysfs pseudo-filesystem (Linux-compatible /sys)

pub mod bridge;
pub mod ext2_modular;
pub mod initramfs;
pub mod procfs;
pub mod sysfs;
pub mod traits;
pub mod vfs;

// Re-export commonly used items from vfs
pub use vfs::{
    add_directory, add_file, add_file_bytes, add_file_with_metadata, create_file,
    enable_ext2_write, file_exists, init, list_directory, list_files, mount_at, open, read_file,
    read_file_bytes, remount_root, stat, write_file, File, FileContent, OpenFile,
};

// Re-export from initramfs
pub use initramfs::{
    get as get_initramfs, init as init_initramfs, CpioNewcHeader, GsData, Initramfs,
    InitramfsEntry, GS_DATA,
};

// Re-export from ext2_modular (modular ext2 via kmod)
pub use ext2_modular::{
    enable_write_mode as ext2_enable_write_mode, get_stats as ext2_get_stats,
    global as ext2_global, init as ext2_modular_init, is_module_loaded as ext2_is_module_loaded,
    is_writable as ext2_is_writable, list_directory as ext2_list_directory, lookup as ext2_lookup,
    metadata_for_path as ext2_metadata_for_path, new as ext2_new, read_at as ext2_read_at,
    register_global as ext2_register_global, write_at as ext2_write_at, Ext2Error, Ext2Handle,
    Ext2ModularFs, Ext2Stats, FileRefHandle,
};

// Re-export filesystem abstraction traits
pub use traits::{
    BlockFileSystem, DirEntry, FileSystemExt, FsError, FsFileHandle, FsResult, FsStats,
    WritableFileSystem,
};

// Re-export bridge adapters
pub use bridge::{BlockFsVfsAdapter, VfsBlockFsAdapter};
