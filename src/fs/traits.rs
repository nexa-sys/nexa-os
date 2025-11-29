//! Filesystem abstraction layer for NexaOS
//!
//! This module defines traits and types that abstract filesystem operations,
//! allowing the kernel to use different filesystem implementations interchangeably.
//! This is a foundation for decoupling ext2 (or any specific filesystem) from the core VFS.

use crate::posix::{FileType, Metadata};

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
            FsError::NotFound => -2,           // ENOENT
            FsError::PermissionDenied => -13,  // EACCES
            FsError::InvalidArgument => -22,   // EINVAL
            FsError::AlreadyExists => -17,     // EEXIST
            FsError::NotEmpty => -39,          // ENOTEMPTY
            FsError::NotADirectory => -20,     // ENOTDIR
            FsError::IsADirectory => -21,      // EISDIR
            FsError::NoSpace => -28,           // ENOSPC
            FsError::ReadOnly => -30,          // EROFS
            FsError::IoError => -5,            // EIO
            FsError::InvalidReference => -5,   // EIO
            FsError::FsSpecific(_) => -5,      // EIO
            FsError::NotSupported => -95,      // ENOTSUP
            FsError::NameTooLong => -36,       // ENAMETOOLONG
            FsError::TooManySymlinks => -40,   // ELOOP
            FsError::CrossDevice => -18,       // EXDEV
            FsError::BadFd => -9,              // EBADF
            FsError::WouldBlock => -11,        // EAGAIN
            FsError::FileTooLarge => -27,      // EFBIG
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
// Dynamic Filesystem Registry (for kernel modules)
// ============================================================================

use spin::Mutex;

/// Maximum number of registered filesystem drivers
pub const MAX_FS_DRIVERS: usize = 16;

/// Filesystem driver operations - C ABI compatible for kernel modules
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsOps {
    /// Initialize filesystem from image data, returns opaque fs handle
    pub init: Option<unsafe extern "C" fn(image: *const u8, size: usize) -> *mut core::ffi::c_void>,
    /// Lookup a file by path, fills FileRef structure, returns 0 on success
    pub lookup: Option<unsafe extern "C" fn(
        fs: *mut core::ffi::c_void,
        path: *const u8,
        path_len: usize,
        out_ref: *mut DynamicFileRef,
    ) -> i32>,
    /// Read file data at offset
    pub read: Option<unsafe extern "C" fn(
        fs: *mut core::ffi::c_void,
        inode: u32,
        offset: usize,
        buf: *mut u8,
        buf_len: usize,
    ) -> isize>,
    /// List directory entries
    pub readdir: Option<unsafe extern "C" fn(
        fs: *mut core::ffi::c_void,
        path: *const u8,
        path_len: usize,
        callback: unsafe extern "C" fn(*const u8, usize, *const DynamicFileRef, *mut core::ffi::c_void),
        user_data: *mut core::ffi::c_void,
    ) -> i32>,
    /// Get filesystem name
    pub name: Option<unsafe extern "C" fn() -> *const u8>,
}

impl Default for FsOps {
    fn default() -> Self {
        Self {
            init: None,
            lookup: None,
            read: None,
            readdir: None,
            name: None,
        }
    }
}

/// Dynamic file reference - C ABI compatible
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DynamicFileRef {
    /// Filesystem handle (opaque pointer)
    pub fs_handle: *mut core::ffi::c_void,
    /// Inode number
    pub inode: u32,
    /// File size in bytes
    pub size: u64,
    /// File mode (permissions and type)
    pub mode: u16,
    /// Number of 512-byte blocks
    pub blocks: u64,
    /// Last modification time (Unix timestamp)
    pub mtime: u64,
    /// Number of hard links
    pub nlink: u32,
    /// User ID
    pub uid: u16,
    /// Group ID
    pub gid: u16,
    /// Filesystem driver index (for looking up ops)
    pub driver_idx: u8,
    /// Padding for alignment
    pub _pad: [u8; 5],
}

impl DynamicFileRef {
    /// Create a new empty file reference
    pub const fn empty() -> Self {
        Self {
            fs_handle: core::ptr::null_mut(),
            inode: 0,
            size: 0,
            mode: 0,
            blocks: 0,
            mtime: 0,
            nlink: 0,
            uid: 0,
            gid: 0,
            driver_idx: 0,
            _pad: [0; 5],
        }
    }

    /// Read data from this file at the given offset
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        if buf.is_empty() || self.fs_handle.is_null() {
            return 0;
        }

        let registry = FS_DRIVER_REGISTRY.lock();
        if let Some(driver) = registry.get_driver(self.driver_idx as usize) {
            if let Some(read_fn) = driver.ops.read {
                let result = unsafe {
                    read_fn(
                        self.fs_handle,
                        self.inode,
                        offset,
                        buf.as_mut_ptr(),
                        buf.len(),
                    )
                };
                if result >= 0 {
                    return result as usize;
                }
            }
        }
        0
    }

    /// Get file metadata
    pub fn metadata(&self) -> crate::posix::Metadata {
        use crate::posix::FileType;
        
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

        crate::posix::Metadata {
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

/// Registered filesystem driver
#[derive(Clone, Copy)]
pub struct FsDriver {
    /// Driver name (null-terminated)
    pub name: [u8; 32],
    /// Driver operations
    pub ops: FsOps,
    /// Whether this driver is active
    pub active: bool,
}

impl FsDriver {
    const fn empty() -> Self {
        Self {
            name: [0; 32],
            ops: FsOps {
                init: None,
                lookup: None,
                read: None,
                readdir: None,
                name: None,
            },
            active: false,
        }
    }

    /// Get driver name as string
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// Filesystem driver registry
pub struct FsDriverRegistry {
    drivers: [FsDriver; MAX_FS_DRIVERS],
    count: usize,
}

impl FsDriverRegistry {
    const fn new() -> Self {
        const EMPTY: FsDriver = FsDriver::empty();
        Self {
            drivers: [EMPTY; MAX_FS_DRIVERS],
            count: 0,
        }
    }

    /// Register a new filesystem driver
    pub fn register(&mut self, name: &str, ops: FsOps) -> Result<usize, FsError> {
        if self.count >= MAX_FS_DRIVERS {
            return Err(FsError::NoSpace);
        }

        // Check for duplicate
        for driver in self.drivers.iter() {
            if driver.active && driver.name_str() == name {
                return Err(FsError::AlreadyExists);
            }
        }

        // Find empty slot
        for (idx, driver) in self.drivers.iter_mut().enumerate() {
            if !driver.active {
                let name_bytes = name.as_bytes();
                let len = name_bytes.len().min(31);
                driver.name[..len].copy_from_slice(&name_bytes[..len]);
                driver.name[len] = 0;
                driver.ops = ops;
                driver.active = true;
                self.count += 1;
                return Ok(idx);
            }
        }

        Err(FsError::NoSpace)
    }

    /// Unregister a filesystem driver
    pub fn unregister(&mut self, name: &str) -> Result<(), FsError> {
        for driver in self.drivers.iter_mut() {
            if driver.active && driver.name_str() == name {
                driver.active = false;
                driver.name = [0; 32];
                driver.ops = FsOps::default();
                self.count -= 1;
                return Ok(());
            }
        }
        Err(FsError::NotFound)
    }

    /// Get a driver by name
    pub fn get_by_name(&self, name: &str) -> Option<&FsDriver> {
        for driver in self.drivers.iter() {
            if driver.active && driver.name_str() == name {
                return Some(driver);
            }
        }
        None
    }

    /// Get a driver by index
    pub fn get_driver(&self, idx: usize) -> Option<&FsDriver> {
        if idx < MAX_FS_DRIVERS && self.drivers[idx].active {
            Some(&self.drivers[idx])
        } else {
            None
        }
    }

    /// Get driver index by name
    pub fn get_driver_index(&self, name: &str) -> Option<usize> {
        for (idx, driver) in self.drivers.iter().enumerate() {
            if driver.active && driver.name_str() == name {
                return Some(idx);
            }
        }
        None
    }
}

/// Global filesystem driver registry
pub static FS_DRIVER_REGISTRY: Mutex<FsDriverRegistry> = Mutex::new(FsDriverRegistry::new());

/// Register a filesystem driver from a kernel module
/// Returns the driver index on success, -1 on failure
#[no_mangle]
pub extern "C" fn register_fs_driver(
    name: *const u8,
    name_len: usize,
    ops: *const FsOps,
) -> i32 {
    if name.is_null() || ops.is_null() || name_len == 0 {
        return -1;
    }

    let name_bytes = unsafe { core::slice::from_raw_parts(name, name_len) };
    let name_str = match core::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let ops = unsafe { *ops };
    let mut registry = FS_DRIVER_REGISTRY.lock();
    
    match registry.register(name_str, ops) {
        Ok(idx) => idx as i32,
        Err(_) => -1,
    }
}

/// Unregister a filesystem driver
#[no_mangle]
pub extern "C" fn unregister_fs_driver(name: *const u8, name_len: usize) -> i32 {
    if name.is_null() || name_len == 0 {
        return -1;
    }

    let name_bytes = unsafe { core::slice::from_raw_parts(name, name_len) };
    let name_str = match core::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let mut registry = FS_DRIVER_REGISTRY.lock();
    match registry.unregister(name_str) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
