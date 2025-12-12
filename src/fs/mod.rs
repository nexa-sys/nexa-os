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
//! - tmpfs in-memory filesystem
//! - devfs device filesystem
//! - fstab mount configuration parser

pub mod bridge;
pub mod devfs;
pub mod ext2_modular;
pub mod fstab;
pub mod initramfs;
pub mod procfs;
pub mod sysfs;
pub mod tmpfs;
pub mod traits;
pub mod vfs;

// Re-export commonly used items from vfs
pub use vfs::{
    add_directory, add_file, add_file_bytes, add_file_with_metadata, create_file,
    enable_ext2_write, file_exists, init, list_directory, list_files, mount_at, open, read_file,
    read_file_bytes, remount_at, remount_root, stat, write_file, File, FileContent, OpenFile,
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
    // ext4 exports
    Ext4ModularFs, Ext4FileRefHandle, Ext4Stats,
    is_ext4_loaded, ext4_global, ext4_new, ext4_lookup, ext4_read_at,
    ext4_list_directory, ext4_enable_write_mode, ext4_is_writable,
};

// Re-export filesystem abstraction traits
pub use traits::{
    BlockFileSystem, DirEntry, FileSystemExt, FsError, FsFileHandle, FsResult, FsStats,
    WritableFileSystem,
};

// Re-export modular filesystem registry types and functions
pub use traits::{
    find_modular_fs, get_mounted_modular_fs, modular_fs_create_file, modular_fs_enable_write,
    modular_fs_get_stats, modular_fs_is_mounted, modular_fs_is_writable, modular_fs_list_dir,
    modular_fs_lookup, modular_fs_read_at, modular_fs_type_name, modular_fs_write_at,
    mount_modular_fs, register_modular_fs, unregister_modular_fs, ModularDirCallback,
    ModularFileHandle, ModularFsOps,
};

// Re-export bridge adapters
pub use bridge::{BlockFsVfsAdapter, VfsBlockFsAdapter};

// Re-export tmpfs
pub use tmpfs::{
    is_tmpfs_mounted, list_tmpfs_mounts, mount_tmpfs, tmpfs_create_directory, tmpfs_create_file,
    tmpfs_read_file, tmpfs_remove, tmpfs_stat, tmpfs_stats, tmpfs_write_file, unmount_tmpfs,
    TmpfsInstance, TmpfsMountOptions, TmpfsVfsAdapter,
};

// Re-export devfs
pub use devfs::{
    get_device_type, init as devfs_init, is_device, register_block_device, register_device,
    register_framebuffer_device, register_network_device, register_loop_device,
    register_loop_control, register_input_event_device, register_input_mice,
    DevFs, DeviceType, DEVFS,
};

// Re-export fstab
pub use fstab::{
    add_entry as fstab_add_entry, default_fstab, find_by_device as fstab_find_by_device,
    find_by_mount_point as fstab_find_by_mount_point, get_auto_mount_entries,
    get_entries as fstab_get_entries, get_entries_by_pass, get_mounts as fstab_get_mounts,
    load_fstab, mount_all as fstab_mount_all, mount_entry as fstab_mount_entry, parse_fstab,
    FstabEntry, MountInfo,
};
