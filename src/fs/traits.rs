//! Filesystem abstraction layer for NexaOS
//!
//! This module defines traits and types that abstract filesystem operations,
//! allowing the kernel to use different filesystem implementations interchangeably.
//! This is a foundation for decoupling ext2 (or any specific filesystem) from the core VFS.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────────────┐     ┌──────────────────────┐
//! │   VFS/App   │────▶│  ModularFsRegistry  │────▶│  ext2.nkm / ext4.nkm │
//! │   Layer     │     │  (this module)      │     │  (loadable modules)  │
//! └─────────────┘     └─────────────────────┘     └──────────────────────┘
//! ```
//!
//! The registry allows multiple filesystem modules (ext2, ext3, ext4, etc.) to
//! register themselves dynamically, and the VFS layer queries files through
//! a unified interface without knowing which filesystem backs them.

use crate::posix::{FileType, Metadata};
use spin::Mutex;

/// Error type for filesystem operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// File or directory not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// Invalid path or argument
    InvalidArgument,
    /// File already exists
    AlreadyExists,
    /// Directory not empty
    NotEmpty,
    /// Not a directory
    NotADirectory,
    /// Is a directory (when file expected)
    IsADirectory,
    /// No space left on device
    NoSpace,
    /// Read-only filesystem
    ReadOnly,
    /// I/O error
    IoError,
    /// Invalid inode or block number
    InvalidReference,
    /// Filesystem-specific error
    FsSpecific(&'static str),
    /// Operation not supported
    NotSupported,
    /// Name too long
    NameTooLong,
    /// Too many symbolic links encountered
    TooManySymlinks,
    /// Cross-device link not permitted
    CrossDevice,
    /// Bad file descriptor
    BadFd,
    /// Resource temporarily unavailable
    WouldBlock,
    /// File too large
    FileTooLarge,
}

impl FsError {
    /// Convert to POSIX errno value
    pub fn to_errno(&self) -> i32 {
        match self {
            FsError::NotFound => -2,          // ENOENT
            FsError::PermissionDenied => -13, // EACCES
            FsError::InvalidArgument => -22,  // EINVAL
            FsError::AlreadyExists => -17,    // EEXIST
            FsError::NotEmpty => -39,         // ENOTEMPTY
            FsError::NotADirectory => -20,    // ENOTDIR
            FsError::IsADirectory => -21,     // EISDIR
            FsError::NoSpace => -28,          // ENOSPC
            FsError::ReadOnly => -30,         // EROFS
            FsError::IoError => -5,           // EIO
            FsError::InvalidReference => -5,  // EIO
            FsError::FsSpecific(_) => -5,     // EIO
            FsError::NotSupported => -95,     // ENOTSUP
            FsError::NameTooLong => -36,      // ENAMETOOLONG
            FsError::TooManySymlinks => -40,  // ELOOP
            FsError::CrossDevice => -18,      // EXDEV
            FsError::BadFd => -9,             // EBADF
            FsError::WouldBlock => -11,       // EAGAIN
            FsError::FileTooLarge => -27,     // EFBIG
        }
    }
}

/// Result type alias for filesystem operations
pub type FsResult<T> = Result<T, FsError>;

/// File handle for abstract filesystem operations
/// This is a lightweight reference to an open file
#[derive(Debug, Clone, Copy)]
pub struct FsFileHandle {
    /// Filesystem-specific identifier (e.g., inode number)
    pub id: u64,
    /// File size in bytes
    pub size: u64,
    /// File mode (permissions and type)
    pub mode: u16,
    /// User ID of owner
    pub uid: u32,
    /// Group ID of owner
    pub gid: u32,
    /// Last modification time (Unix timestamp)
    pub mtime: u64,
    /// Number of hard links
    pub nlink: u32,
    /// Number of 512-byte blocks allocated
    pub blocks: u64,
}

impl FsFileHandle {
    /// Create a new file handle
    pub const fn new(
        id: u64,
        size: u64,
        mode: u16,
        uid: u32,
        gid: u32,
        mtime: u64,
        nlink: u32,
        blocks: u64,
    ) -> Self {
        Self {
            id,
            size,
            mode,
            uid,
            gid,
            mtime,
            nlink,
            blocks,
        }
    }

    /// Check if this handle refers to a regular file
    pub fn is_file(&self) -> bool {
        (self.mode & 0o170000) == 0o100000
    }

    /// Check if this handle refers to a directory
    pub fn is_directory(&self) -> bool {
        (self.mode & 0o170000) == 0o040000
    }

    /// Check if this handle refers to a symbolic link
    pub fn is_symlink(&self) -> bool {
        (self.mode & 0o170000) == 0o120000
    }

    /// Get file type from mode
    pub fn file_type(&self) -> FileType {
        match self.mode & 0o170000 {
            0o040000 => FileType::Directory,
            0o100000 => FileType::Regular,
            0o120000 => FileType::Symlink,
            0o020000 => FileType::Character,
            0o060000 => FileType::Block,
            0o010000 => FileType::Fifo,
            0o140000 => FileType::Socket,
            other => FileType::Unknown(other as u16),
        }
    }

    /// Convert to POSIX Metadata
    pub fn to_metadata(&self) -> Metadata {
        Metadata {
            mode: self.mode,
            uid: self.uid,
            gid: self.gid,
            size: self.size,
            mtime: self.mtime,
            file_type: self.file_type(),
            nlink: self.nlink,
            blocks: self.blocks,
        }
        .normalize()
    }
}

/// Directory entry returned during directory listing
#[derive(Debug, Clone, Copy)]
pub struct DirEntry {
    /// Filesystem-specific identifier (e.g., inode number)
    pub id: u64,
    /// Entry name (null-terminated, max 255 chars)
    pub name: [u8; 256],
    /// Length of the name
    pub name_len: usize,
    /// File type (directory, regular file, symlink, etc.)
    pub file_type: u8,
}

impl DirEntry {
    /// Create a new directory entry
    pub fn new(id: u64, name: &str, file_type: u8) -> Self {
        let mut entry = Self {
            id,
            name: [0; 256],
            name_len: 0,
            file_type,
        };
        let len = name.len().min(255);
        entry.name[..len].copy_from_slice(&name.as_bytes()[..len]);
        entry.name_len = len;
        entry
    }

    /// Get the name as a string slice
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// Core trait for filesystem implementations
///
/// This trait defines the minimal interface that any filesystem must implement
/// to be usable by the VFS layer. It supports both read-only and read-write
/// filesystems through optional methods.
pub trait BlockFileSystem: Sync + Send {
    /// Return the filesystem type name (e.g., "ext2", "ext4", "fat32")
    fn fs_type(&self) -> &'static str;

    /// Return whether the filesystem is mounted read-only
    fn is_readonly(&self) -> bool {
        true // Default to read-only
    }

    /// Lookup a file or directory by path
    /// Returns a file handle if found
    fn lookup(&self, path: &str) -> FsResult<FsFileHandle>;

    /// Read data from a file at the given offset
    /// Returns the number of bytes read
    fn read(&self, handle: &FsFileHandle, offset: usize, buf: &mut [u8]) -> FsResult<usize>;

    /// Get metadata for a path
    fn stat(&self, path: &str) -> FsResult<Metadata> {
        self.lookup(path).map(|h| h.to_metadata())
    }

    /// List directory contents
    /// Calls the callback for each entry
    fn readdir(&self, path: &str, callback: &mut dyn FnMut(DirEntry)) -> FsResult<()>;

    // === Optional write operations (default to not supported) ===

    /// Write data to a file at the given offset
    /// Returns the number of bytes written
    fn write(&self, _handle: &FsFileHandle, _offset: usize, _data: &[u8]) -> FsResult<usize> {
        Err(FsError::ReadOnly)
    }

    /// Truncate a file to the specified length
    fn truncate(&self, _handle: &FsFileHandle, _length: u64) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Create a new file
    /// Returns a handle to the created file
    fn create(&self, _path: &str, _mode: u16) -> FsResult<FsFileHandle> {
        Err(FsError::ReadOnly)
    }

    /// Create a new directory
    fn mkdir(&self, _path: &str, _mode: u16) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Remove a file
    fn unlink(&self, _path: &str) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Remove a directory (must be empty)
    fn rmdir(&self, _path: &str) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Rename a file or directory
    fn rename(&self, _old_path: &str, _new_path: &str) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Create a hard link
    fn link(&self, _old_path: &str, _new_path: &str) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Create a symbolic link
    fn symlink(&self, _target: &str, _link_path: &str) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Read the target of a symbolic link
    fn readlink(&self, _path: &str, _buf: &mut [u8]) -> FsResult<usize> {
        Err(FsError::NotSupported)
    }

    /// Change file mode (permissions)
    fn chmod(&self, _path: &str, _mode: u16) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Change file owner
    fn chown(&self, _path: &str, _uid: u32, _gid: u32) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Update file timestamps
    fn utimes(&self, _path: &str, _atime: u64, _mtime: u64) -> FsResult<()> {
        Err(FsError::ReadOnly)
    }

    /// Sync filesystem to disk
    fn sync(&self) -> FsResult<()> {
        Ok(()) // No-op for read-only
    }

    /// Get filesystem statistics
    fn statfs(&self) -> FsResult<FsStats> {
        Err(FsError::NotSupported)
    }
}

/// Filesystem statistics
#[derive(Debug, Clone, Copy)]
pub struct FsStats {
    /// Total blocks in filesystem
    pub total_blocks: u64,
    /// Free blocks in filesystem
    pub free_blocks: u64,
    /// Available blocks for unprivileged users
    pub avail_blocks: u64,
    /// Total inodes
    pub total_inodes: u64,
    /// Free inodes
    pub free_inodes: u64,
    /// Block size in bytes
    pub block_size: u32,
    /// Maximum filename length
    pub name_max: u32,
    /// Filesystem type magic number
    pub fs_type: u32,
}

impl Default for FsStats {
    fn default() -> Self {
        Self {
            total_blocks: 0,
            free_blocks: 0,
            avail_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            block_size: 4096,
            name_max: 255,
            fs_type: 0,
        }
    }
}

/// Marker trait for filesystems that support write operations
pub trait WritableFileSystem: BlockFileSystem {}

/// Extension trait for convenient file operations
pub trait FileSystemExt: BlockFileSystem {
    /// Read entire file content into a buffer
    /// Returns the number of bytes read
    fn read_file(&self, path: &str, buf: &mut [u8]) -> FsResult<usize> {
        let handle = self.lookup(path)?;
        if !handle.is_file() {
            return Err(FsError::IsADirectory);
        }
        self.read(&handle, 0, buf)
    }

    /// Check if a path exists
    fn exists(&self, path: &str) -> bool {
        self.lookup(path).is_ok()
    }

    /// Check if path is a directory
    fn is_dir(&self, path: &str) -> bool {
        self.lookup(path).map(|h| h.is_directory()).unwrap_or(false)
    }

    /// Check if path is a regular file
    fn is_file(&self, path: &str) -> bool {
        self.lookup(path).map(|h| h.is_file()).unwrap_or(false)
    }
}

// Blanket implementation for all BlockFileSystem types
impl<T: BlockFileSystem + ?Sized> FileSystemExt for T {}

// ============================================================================
// Modular Filesystem Registry
// ============================================================================

/// Maximum number of registered filesystem modules
const MAX_MODULAR_FS: usize = 8;

/// A generic file handle for modular filesystems
/// This replaces the ext2-specific FileRefHandle in VFS layer
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModularFileHandle {
    /// Which registered filesystem this handle belongs to (index in registry)
    pub fs_index: u8,
    /// Filesystem-specific opaque handle (e.g., Ext2Handle pointer)
    pub fs_handle: *mut u8,
    /// Inode number within the filesystem
    pub inode: u32,
    /// File size in bytes
    pub size: u64,
    /// File mode (permissions + type)
    pub mode: u16,
    /// Number of 512-byte blocks allocated
    pub blocks: u64,
    /// Last modification time (Unix timestamp)
    pub mtime: u64,
    /// Number of hard links
    pub nlink: u32,
    /// User ID of owner
    pub uid: u16,
    /// Group ID of owner
    pub gid: u16,
}

impl ModularFileHandle {
    /// Check if this handle is valid
    pub fn is_valid(&self) -> bool {
        !self.fs_handle.is_null() && self.inode != 0
    }

    /// Convert to POSIX Metadata
    pub fn metadata(&self) -> Metadata {
        let file_type = match self.mode & 0o170000 {
            0o040000 => FileType::Directory,
            0o100000 => FileType::Regular,
            0o120000 => FileType::Symlink,
            0o020000 => FileType::Character,
            0o060000 => FileType::Block,
            0o010000 => FileType::Fifo,
            0o140000 => FileType::Socket,
            other => FileType::Unknown(other as u16),
        };

        Metadata {
            mode: self.mode,
            uid: self.uid as u32,
            gid: self.gid as u32,
            size: self.size,
            mtime: self.mtime,
            file_type,
            nlink: self.nlink,
            blocks: self.blocks,
        }
        .normalize()
    }
}

// SAFETY: ModularFileHandle contains a raw pointer that is managed by the filesystem module
// The module ensures thread safety through its own locking mechanisms
unsafe impl Send for ModularFileHandle {}
unsafe impl Sync for ModularFileHandle {}

/// Directory entry callback type for modular filesystems
pub type ModularDirCallback = extern "C" fn(
    name: *const u8,
    name_len: usize,
    inode: u32,
    file_type: u8,
    ctx: *mut u8,
);

/// Operations table for modular filesystem modules
/// Each filesystem module (ext2, ext3, ext4, etc.) implements this interface
#[repr(C)]
pub struct ModularFsOps {
    /// Filesystem type name (e.g., "ext2", "ext4")
    pub fs_type: &'static str,
    
    /// Create a new filesystem instance from raw image data
    /// Returns a filesystem handle or null on failure
    pub new_from_image: Option<extern "C" fn(image: *const u8, size: usize) -> *mut u8>,
    
    /// Destroy a filesystem instance
    pub destroy: Option<extern "C" fn(handle: *mut u8)>,
    
    /// Lookup a file by path
    /// Returns 0 on success, negative error code on failure
    pub lookup: Option<extern "C" fn(
        handle: *mut u8,
        path: *const u8,
        path_len: usize,
        out: *mut ModularFileHandle,
    ) -> i32>,
    
    /// Read data from a file at offset
    /// Returns bytes read or negative error code
    pub read_at: Option<extern "C" fn(
        file: *const ModularFileHandle,
        offset: usize,
        buf: *mut u8,
        len: usize,
    ) -> i32>,
    
    /// Write data to a file at offset
    /// Returns bytes written or negative error code
    pub write_at: Option<extern "C" fn(
        file: *const ModularFileHandle,
        offset: usize,
        data: *const u8,
        len: usize,
    ) -> i32>,
    
    /// List directory contents
    pub list_dir: Option<extern "C" fn(
        handle: *mut u8,
        path: *const u8,
        path_len: usize,
        cb: ModularDirCallback,
        ctx: *mut u8,
    ) -> i32>,
    
    /// Get filesystem statistics
    pub get_stats: Option<extern "C" fn(handle: *mut u8, stats: *mut FsStats) -> i32>,
    
    /// Set writable mode
    pub set_writable: Option<extern "C" fn(writable: bool)>,
    
    /// Check if writable
    pub is_writable: Option<extern "C" fn() -> bool>,
    
    /// Create a new file
    pub create_file: Option<extern "C" fn(
        handle: *mut u8,
        path: *const u8,
        path_len: usize,
        mode: u16,
    ) -> i32>,
    
    /// Create a new directory
    pub mkdir: Option<extern "C" fn(
        handle: *mut u8,
        path: *const u8,
        path_len: usize,
        mode: u16,
    ) -> i32>,
    
    /// Remove a file
    pub unlink: Option<extern "C" fn(
        handle: *mut u8,
        path: *const u8,
        path_len: usize,
    ) -> i32>,
    
    /// Remove a directory
    pub rmdir: Option<extern "C" fn(
        handle: *mut u8,
        path: *const u8,
        path_len: usize,
    ) -> i32>,
    
    /// Rename a file or directory
    pub rename: Option<extern "C" fn(
        handle: *mut u8,
        old_path: *const u8,
        old_len: usize,
        new_path: *const u8,
        new_len: usize,
    ) -> i32>,
    
    /// Sync filesystem to disk
    pub sync: Option<extern "C" fn(handle: *mut u8) -> i32>,
}

impl Default for ModularFsOps {
    fn default() -> Self {
        Self {
            fs_type: "unknown",
            new_from_image: None,
            destroy: None,
            lookup: None,
            read_at: None,
            write_at: None,
            list_dir: None,
            get_stats: None,
            set_writable: None,
            is_writable: None,
            create_file: None,
            mkdir: None,
            unlink: None,
            rmdir: None,
            rename: None,
            sync: None,
        }
    }
}

/// A registered modular filesystem entry
struct ModularFsEntry {
    /// Operations table provided by the module
    ops: ModularFsOps,
    /// Active filesystem handle (if mounted)
    handle: Option<*mut u8>,
    /// Whether this slot is in use
    active: bool,
}

impl Default for ModularFsEntry {
    fn default() -> Self {
        Self {
            ops: ModularFsOps::default(),
            handle: None,
            active: false,
        }
    }
}

// SAFETY: The raw pointers in ModularFsEntry are managed by the filesystem modules
// which ensure thread safety through their own mechanisms
unsafe impl Send for ModularFsEntry {}
unsafe impl Sync for ModularFsEntry {}

/// Global registry of modular filesystems
static MODULAR_FS_REGISTRY: Mutex<[ModularFsEntry; MAX_MODULAR_FS]> = 
    Mutex::new([const { ModularFsEntry { ops: ModularFsOps { fs_type: "unknown", new_from_image: None, destroy: None, lookup: None, read_at: None, write_at: None, list_dir: None, get_stats: None, set_writable: None, is_writable: None, create_file: None, mkdir: None, unlink: None, rmdir: None, rename: None, sync: None }, handle: None, active: false } }; MAX_MODULAR_FS]);

/// Register a modular filesystem
/// Returns the index in the registry, or None if registry is full
pub fn register_modular_fs(ops: ModularFsOps) -> Option<u8> {
    let mut registry = MODULAR_FS_REGISTRY.lock();
    for (i, entry) in registry.iter_mut().enumerate() {
        if !entry.active {
            entry.ops = ops;
            entry.active = true;
            crate::kinfo!("Registered modular filesystem: {} at index {}", entry.ops.fs_type, i);
            return Some(i as u8);
        }
    }
    crate::kwarn!("Modular filesystem registry is full");
    None
}

/// Unregister a modular filesystem by index
pub fn unregister_modular_fs(index: u8) {
    let mut registry = MODULAR_FS_REGISTRY.lock();
    if let Some(entry) = registry.get_mut(index as usize) {
        if entry.active {
            // Destroy the filesystem handle if present
            if let (Some(destroy), Some(handle)) = (entry.ops.destroy, entry.handle) {
                destroy(handle);
            }
            entry.handle = None;
            entry.active = false;
            crate::kinfo!("Unregistered modular filesystem at index {}", index);
        }
    }
}

/// Find a registered filesystem by type name
pub fn find_modular_fs(fs_type: &str) -> Option<u8> {
    let registry = MODULAR_FS_REGISTRY.lock();
    for (i, entry) in registry.iter().enumerate() {
        if entry.active && entry.ops.fs_type == fs_type {
            return Some(i as u8);
        }
    }
    None
}

/// Mount a modular filesystem from an image
pub fn mount_modular_fs(index: u8, image: &[u8]) -> FsResult<()> {
    let mut registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get_mut(index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    let new_fn = entry.ops.new_from_image.ok_or(FsError::NotSupported)?;
    let handle = new_fn(image.as_ptr(), image.len());
    
    if handle.is_null() {
        return Err(FsError::IoError);
    }
    
    entry.handle = Some(handle);
    crate::kinfo!("Mounted {} filesystem", entry.ops.fs_type);
    Ok(())
}

/// Lookup a file in a modular filesystem
pub fn modular_fs_lookup(index: u8, path: &str) -> FsResult<ModularFileHandle> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    let lookup_fn = entry.ops.lookup.ok_or(FsError::NotSupported)?;
    let handle = entry.handle.ok_or(FsError::InvalidArgument)?;
    
    let mut file_handle = ModularFileHandle {
        fs_index: index,
        fs_handle: handle,
        inode: 0,
        size: 0,
        mode: 0,
        blocks: 0,
        mtime: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
    };
    
    let path_bytes = path.as_bytes();
    let ret = lookup_fn(handle, path_bytes.as_ptr(), path_bytes.len(), &mut file_handle);
    
    if ret == 0 {
        file_handle.fs_index = index;
        file_handle.fs_handle = handle;
        Ok(file_handle)
    } else {
        Err(FsError::NotFound)
    }
}

/// Read from a file in a modular filesystem
pub fn modular_fs_read_at(file: &ModularFileHandle, offset: usize, buf: &mut [u8]) -> FsResult<usize> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(file.fs_index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    let read_fn = entry.ops.read_at.ok_or(FsError::NotSupported)?;
    let ret = read_fn(file, offset, buf.as_mut_ptr(), buf.len());
    
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(FsError::IoError)
    }
}

/// Write to a file in a modular filesystem
pub fn modular_fs_write_at(file: &ModularFileHandle, offset: usize, data: &[u8]) -> FsResult<usize> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(file.fs_index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    // Check if filesystem is writable
    if let Some(is_writable) = entry.ops.is_writable {
        if !is_writable() {
            return Err(FsError::ReadOnly);
        }
    }
    
    let write_fn = entry.ops.write_at.ok_or(FsError::NotSupported)?;
    let ret = write_fn(file, offset, data.as_ptr(), data.len());
    
    if ret >= 0 {
        Ok(ret as usize)
    } else if ret == -7 {
        Err(FsError::ReadOnly)
    } else {
        Err(FsError::IoError)
    }
}

/// List directory in a modular filesystem
pub fn modular_fs_list_dir(
    index: u8,
    path: &str,
    callback: ModularDirCallback,
    ctx: *mut u8,
) -> FsResult<()> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    let list_fn = entry.ops.list_dir.ok_or(FsError::NotSupported)?;
    let handle = entry.handle.ok_or(FsError::InvalidArgument)?;
    
    let path_bytes = path.as_bytes();
    let ret = list_fn(handle, path_bytes.as_ptr(), path_bytes.len(), callback, ctx);
    
    if ret == 0 {
        Ok(())
    } else {
        Err(FsError::IoError)
    }
}

/// Enable write mode for a modular filesystem
pub fn modular_fs_enable_write(index: u8) -> FsResult<()> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    if let Some(set_writable) = entry.ops.set_writable {
        set_writable(true);
        Ok(())
    } else {
        Err(FsError::NotSupported)
    }
}

/// Check if a modular filesystem is writable
pub fn modular_fs_is_writable(index: u8) -> bool {
    let registry = MODULAR_FS_REGISTRY.lock();
    if let Some(entry) = registry.get(index as usize) {
        if entry.active {
            if let Some(is_writable) = entry.ops.is_writable {
                return is_writable();
            }
        }
    }
    false
}

/// Get statistics for a modular filesystem
pub fn modular_fs_get_stats(index: u8) -> FsResult<FsStats> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    let get_stats_fn = entry.ops.get_stats.ok_or(FsError::NotSupported)?;
    let handle = entry.handle.ok_or(FsError::InvalidArgument)?;
    
    let mut stats = FsStats::default();
    let ret = get_stats_fn(handle, &mut stats);
    
    if ret == 0 {
        Ok(stats)
    } else {
        Err(FsError::IoError)
    }
}

/// Get the filesystem type name for an index
pub fn modular_fs_type_name(index: u8) -> Option<&'static str> {
    let registry = MODULAR_FS_REGISTRY.lock();
    registry.get(index as usize)
        .filter(|e| e.active)
        .map(|e| e.ops.fs_type)
}

/// Check if a modular filesystem is mounted at the given index
pub fn modular_fs_is_mounted(index: u8) -> bool {
    let registry = MODULAR_FS_REGISTRY.lock();
    registry.get(index as usize)
        .map(|e| e.active && e.handle.is_some())
        .unwrap_or(false)
}

/// Get the first mounted modular filesystem index
pub fn get_mounted_modular_fs() -> Option<u8> {
    let registry = MODULAR_FS_REGISTRY.lock();
    for (i, entry) in registry.iter().enumerate() {
        if entry.active && entry.handle.is_some() {
            return Some(i as u8);
        }
    }
    None
}

/// Create a file in a modular filesystem
pub fn modular_fs_create_file(index: u8, path: &str, mode: u16) -> FsResult<()> {
    let registry = MODULAR_FS_REGISTRY.lock();
    let entry = registry.get(index as usize).ok_or(FsError::InvalidArgument)?;
    
    if !entry.active {
        return Err(FsError::InvalidArgument);
    }
    
    let create_fn = entry.ops.create_file.ok_or(FsError::NotSupported)?;
    let handle = entry.handle.ok_or(FsError::InvalidArgument)?;
    
    let path_bytes = path.as_bytes();
    let ret = create_fn(handle, path_bytes.as_ptr(), path_bytes.len(), mode);
    
    if ret == 0 {
        Ok(())
    } else if ret == -7 {
        Err(FsError::ReadOnly)
    } else {
        Err(FsError::IoError)
    }
}
