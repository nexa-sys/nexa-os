//! tmpfs - Temporary In-Memory Filesystem
//!
//! This module implements a POSIX-compatible tmpfs filesystem that stores
//! files entirely in RAM. It supports:
//! - File and directory creation
//! - Read/write operations
//! - Standard metadata (permissions, ownership, timestamps)
//! - Size limits based on available memory
//!
//! Unlike disk-based filesystems, tmpfs contents are lost on reboot.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::posix::{FileType, Metadata};

use super::traits::{BlockFileSystem, DirEntry, FsError, FsFileHandle, FsResult, FsStats};
use super::vfs::{FileContent, FileSystem, OpenFile};
/// Maximum size of tmpfs (default 50% of available RAM, capped at 256 MiB)
const MAX_TMPFS_SIZE: usize = 256 * 1024 * 1024;

/// Maximum number of inodes in a single tmpfs instance
const MAX_INODES: usize = 65536;

/// Maximum file size in tmpfs
const MAX_FILE_SIZE: usize = 64 * 1024 * 1024; // 64 MiB per file

/// Inode type for tmpfs entries
#[derive(Debug, Clone)]
pub enum TmpfsInode {
    /// Regular file with data
    File {
        data: Vec<u8>,
        metadata: TmpfsMetadata,
    },
    /// Directory with children
    Directory {
        children: BTreeMap<String, u64>,
        metadata: TmpfsMetadata,
    },
    /// Symbolic link
    Symlink {
        target: String,
        metadata: TmpfsMetadata,
    },
}

impl TmpfsInode {
    fn metadata(&self) -> &TmpfsMetadata {
        match self {
            TmpfsInode::File { metadata, .. } => metadata,
            TmpfsInode::Directory { metadata, .. } => metadata,
            TmpfsInode::Symlink { metadata, .. } => metadata,
        }
    }

    fn metadata_mut(&mut self) -> &mut TmpfsMetadata {
        match self {
            TmpfsInode::File { metadata, .. } => metadata,
            TmpfsInode::Directory { metadata, .. } => metadata,
            TmpfsInode::Symlink { metadata, .. } => metadata,
        }
    }
}

/// Metadata for tmpfs entries
#[derive(Debug, Clone)]
pub struct TmpfsMetadata {
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub nlink: u32,
}

impl TmpfsMetadata {
    fn new_file(mode: u16) -> Self {
        let now = crate::scheduler::get_tick();
        Self {
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
            nlink: 1,
        }
    }

    fn new_dir(mode: u16) -> Self {
        let now = crate::scheduler::get_tick();
        Self {
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
            nlink: 2, // . and parent
        }
    }

    fn new_symlink(mode: u16) -> Self {
        let now = crate::scheduler::get_tick();
        Self {
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
            nlink: 1,
        }
    }

    fn to_posix_metadata(&self, file_type: FileType) -> Metadata {
        let mut meta = Metadata::empty().with_type(file_type).with_mode(self.mode);
        meta.size = self.size;
        meta.uid = self.uid;
        meta.gid = self.gid;
        meta.mtime = self.mtime;
        meta.nlink = self.nlink;
        meta.blocks = (self.size + 511) / 512;
        meta
    }
}

/// A single tmpfs instance
pub struct TmpfsInstance {
    /// All inodes in this tmpfs
    inodes: BTreeMap<u64, TmpfsInode>,
    /// Next available inode number
    next_inode: u64,
    /// Total bytes used
    bytes_used: usize,
    /// Maximum size limit
    max_size: usize,
    /// Mount options
    options: TmpfsMountOptions,
}

/// Mount options for tmpfs
#[derive(Debug, Clone)]
pub struct TmpfsMountOptions {
    /// Maximum size in bytes (0 = unlimited up to MAX_TMPFS_SIZE)
    pub size: usize,
    /// File mode for new files
    pub mode: u16,
    /// User ID for all files
    pub uid: u32,
    /// Group ID for all files
    pub gid: u32,
}

impl Default for TmpfsMountOptions {
    fn default() -> Self {
        Self {
            size: MAX_TMPFS_SIZE,
            mode: 0o1777, // sticky bit + rwxrwxrwx (like /tmp)
            uid: 0,
            gid: 0,
        }
    }
}

impl TmpfsInstance {
    /// Create a new tmpfs instance with default options
    pub fn new() -> Self {
        Self::with_options(TmpfsMountOptions::default())
    }

    /// Create a new tmpfs instance with specified options
    pub fn with_options(options: TmpfsMountOptions) -> Self {
        let mut fs = Self {
            inodes: BTreeMap::new(),
            next_inode: 1,
            bytes_used: 0,
            max_size: options.size.min(MAX_TMPFS_SIZE),
            options,
        };

        // Create root directory (inode 1)
        let root_meta = TmpfsMetadata::new_dir(fs.options.mode);
        fs.inodes.insert(
            1,
            TmpfsInode::Directory {
                children: BTreeMap::new(),
                metadata: root_meta,
            },
        );
        fs.next_inode = 2;

        fs
    }

    /// Allocate a new inode number
    fn alloc_inode(&mut self) -> FsResult<u64> {
        if self.inodes.len() >= MAX_INODES {
            return Err(FsError::NoSpace);
        }
        let inode = self.next_inode;
        self.next_inode += 1;
        Ok(inode)
    }

    /// Lookup a path and return the inode number
    fn lookup_path(&self, path: &str) -> FsResult<u64> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(1); // Root inode
        }

        let mut current_inode = 1u64;
        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                // For simplicity, ".." not fully supported
                continue;
            }

            let inode = self.inodes.get(&current_inode).ok_or(FsError::NotFound)?;
            match inode {
                TmpfsInode::Directory { children, .. } => {
                    current_inode = *children.get(component).ok_or(FsError::NotFound)?;
                }
                _ => return Err(FsError::NotADirectory),
            }
        }

        Ok(current_inode)
    }

    /// Lookup parent directory and return (parent_inode, filename_copy)
    /// Returns owned String to avoid borrow conflicts
    fn lookup_parent_owned(&self, path: &str) -> FsResult<(u64, String)> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut parts: Vec<&str> = path.split('/').collect();
        let filename = parts.pop().ok_or(FsError::InvalidArgument)?;

        if filename.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        if parts.is_empty() {
            // Parent is root
            return Ok((1, String::from(filename)));
        }

        let parent_path = parts.join("/");
        let parent_inode = self.lookup_path(&parent_path)?;

        // Verify parent is a directory
        match self.inodes.get(&parent_inode) {
            Some(TmpfsInode::Directory { .. }) => Ok((parent_inode, String::from(filename))),
            Some(_) => Err(FsError::NotADirectory),
            None => Err(FsError::NotFound),
        }
    }

    /// Create a file at the given path
    pub fn create_file(&mut self, path: &str, mode: u16) -> FsResult<u64> {
        let (parent_inode, filename) = self.lookup_parent_owned(path)?;

        // Check if file already exists
        if let Some(TmpfsInode::Directory { children, .. }) = self.inodes.get(&parent_inode) {
            if children.contains_key(&filename) {
                return Err(FsError::AlreadyExists);
            }
        }

        // Allocate new inode
        let new_inode = self.alloc_inode()?;
        let metadata = TmpfsMetadata::new_file(mode);

        // Insert the file inode
        self.inodes.insert(
            new_inode,
            TmpfsInode::File {
                data: Vec::new(),
                metadata,
            },
        );

        // Add to parent directory
        if let Some(TmpfsInode::Directory { children, metadata }) =
            self.inodes.get_mut(&parent_inode)
        {
            children.insert(filename, new_inode);
            metadata.mtime = crate::scheduler::get_tick();
        }

        Ok(new_inode)
    }

    /// Create a directory at the given path
    pub fn create_directory(&mut self, path: &str, mode: u16) -> FsResult<u64> {
        let (parent_inode, dirname) = self.lookup_parent_owned(path)?;

        // Check if directory already exists
        if let Some(TmpfsInode::Directory { children, .. }) = self.inodes.get(&parent_inode) {
            if children.contains_key(&dirname) {
                return Err(FsError::AlreadyExists);
            }
        }

        // Allocate new inode
        let new_inode = self.alloc_inode()?;
        let metadata = TmpfsMetadata::new_dir(mode);

        // Insert the directory inode
        self.inodes.insert(
            new_inode,
            TmpfsInode::Directory {
                children: BTreeMap::new(),
                metadata,
            },
        );

        // Add to parent directory
        if let Some(TmpfsInode::Directory { children, metadata }) =
            self.inodes.get_mut(&parent_inode)
        {
            children.insert(dirname, new_inode);
            metadata.mtime = crate::scheduler::get_tick();
            metadata.nlink += 1; // Parent's nlink increases for subdirectory
        }

        Ok(new_inode)
    }

    /// Write data to a file
    pub fn write_file(&mut self, inode: u64, offset: usize, data: &[u8]) -> FsResult<usize> {
        let file = self.inodes.get_mut(&inode).ok_or(FsError::NotFound)?;

        match file {
            TmpfsInode::File {
                data: file_data,
                metadata,
            } => {
                // Check size limits
                let new_size = offset + data.len();
                if new_size > MAX_FILE_SIZE {
                    return Err(FsError::FileTooLarge);
                }

                let size_increase = new_size.saturating_sub(file_data.len());
                if self.bytes_used + size_increase > self.max_size {
                    return Err(FsError::NoSpace);
                }

                // Extend file if needed
                if file_data.len() < new_size {
                    file_data.resize(new_size, 0);
                }

                // Write data
                file_data[offset..offset + data.len()].copy_from_slice(data);

                // Update metadata
                metadata.size = file_data.len() as u64;
                metadata.mtime = crate::scheduler::get_tick();
                self.bytes_used += size_increase;

                Ok(data.len())
            }
            _ => Err(FsError::IsADirectory),
        }
    }

    /// Read data from a file
    pub fn read_file(&self, inode: u64, offset: usize, buf: &mut [u8]) -> FsResult<usize> {
        let file = self.inodes.get(&inode).ok_or(FsError::NotFound)?;

        match file {
            TmpfsInode::File { data, .. } => {
                if offset >= data.len() {
                    return Ok(0);
                }

                let available = data.len() - offset;
                let to_read = buf.len().min(available);
                buf[..to_read].copy_from_slice(&data[offset..offset + to_read]);
                Ok(to_read)
            }
            _ => Err(FsError::IsADirectory),
        }
    }

    /// Get inode metadata
    pub fn get_metadata(&self, inode: u64) -> FsResult<Metadata> {
        let node = self.inodes.get(&inode).ok_or(FsError::NotFound)?;
        let meta = node.metadata();
        let file_type = match node {
            TmpfsInode::File { .. } => FileType::Regular,
            TmpfsInode::Directory { .. } => FileType::Directory,
            TmpfsInode::Symlink { .. } => FileType::Symlink,
        };
        Ok(meta.to_posix_metadata(file_type))
    }

    /// List directory contents
    pub fn list_directory(&self, inode: u64) -> FsResult<Vec<(String, u64)>> {
        let dir = self.inodes.get(&inode).ok_or(FsError::NotFound)?;

        match dir {
            TmpfsInode::Directory { children, .. } => {
                Ok(children.iter().map(|(k, v)| (k.clone(), *v)).collect())
            }
            _ => Err(FsError::NotADirectory),
        }
    }

    /// Remove a file
    pub fn remove_file(&mut self, path: &str) -> FsResult<()> {
        let (parent_inode, filename) = self.lookup_parent_owned(path)?;
        let file_inode = self.lookup_path(path)?;

        // Verify it's a file
        match self.inodes.get(&file_inode) {
            Some(TmpfsInode::File { data, .. }) => {
                self.bytes_used = self.bytes_used.saturating_sub(data.len());
            }
            Some(TmpfsInode::Directory { children, .. }) => {
                if !children.is_empty() {
                    return Err(FsError::NotEmpty);
                }
            }
            _ => {}
        }

        // Remove from parent
        if let Some(TmpfsInode::Directory { children, metadata }) =
            self.inodes.get_mut(&parent_inode)
        {
            children.remove(&filename);
            metadata.mtime = crate::scheduler::get_tick();
        }

        // Remove inode
        self.inodes.remove(&file_inode);

        Ok(())
    }

    /// Get filesystem statistics
    pub fn stats(&self) -> FsStats {
        let block_size = 4096u32;
        let total_blocks = (self.max_size / block_size as usize) as u64;
        let used_blocks = (self.bytes_used + block_size as usize - 1) / block_size as usize;
        let free_blocks = total_blocks - used_blocks as u64;

        FsStats {
            total_blocks,
            free_blocks,
            avail_blocks: free_blocks,
            total_inodes: MAX_INODES as u64,
            free_inodes: (MAX_INODES - self.inodes.len()) as u64,
            block_size,
            name_max: 255,
            fs_type: 0x01021994, // TMPFS_MAGIC
        }
    }
}

impl Default for TmpfsInstance {
    fn default() -> Self {
        Self::new()
    }
}

/// Global tmpfs registry for mounted instances
static TMPFS_MOUNTS: Mutex<[Option<TmpfsMount>; 8]> = Mutex::new([const { None }; 8]);

/// A mounted tmpfs instance
struct TmpfsMount {
    mount_point: &'static str,
    fs: TmpfsInstance,
}

/// Register a new tmpfs mount
pub fn mount_tmpfs(mount_point: &'static str, options: TmpfsMountOptions) -> FsResult<()> {
    let mut mounts = TMPFS_MOUNTS.lock();

    // Check if already mounted
    for slot in mounts.iter() {
        if let Some(mount) = slot {
            if mount.mount_point == mount_point {
                return Err(FsError::AlreadyExists);
            }
        }
    }

    // Find empty slot
    for slot in mounts.iter_mut() {
        if slot.is_none() {
            *slot = Some(TmpfsMount {
                mount_point,
                fs: TmpfsInstance::with_options(options),
            });
            crate::kinfo!("tmpfs mounted at {}", mount_point);
            return Ok(());
        }
    }

    Err(FsError::NoSpace)
}

/// Unmount a tmpfs
pub fn unmount_tmpfs(mount_point: &str) -> FsResult<()> {
    let mut mounts = TMPFS_MOUNTS.lock();

    for slot in mounts.iter_mut() {
        if let Some(mount) = slot {
            if mount.mount_point == mount_point {
                *slot = None;
                crate::kinfo!("tmpfs unmounted from {}", mount_point);
                return Ok(());
            }
        }
    }

    Err(FsError::NotFound)
}

/// Get a reference to a mounted tmpfs
fn with_tmpfs<F, R>(mount_point: &str, f: F) -> FsResult<R>
where
    F: FnOnce(&TmpfsInstance) -> FsResult<R>,
{
    let mounts = TMPFS_MOUNTS.lock();
    for slot in mounts.iter() {
        if let Some(mount) = slot {
            if mount.mount_point == mount_point {
                return f(&mount.fs);
            }
        }
    }
    Err(FsError::NotFound)
}

/// Get a mutable reference to a mounted tmpfs
fn with_tmpfs_mut<F, R>(mount_point: &str, f: F) -> FsResult<R>
where
    F: FnOnce(&mut TmpfsInstance) -> FsResult<R>,
{
    let mut mounts = TMPFS_MOUNTS.lock();
    for slot in mounts.iter_mut() {
        if let Some(mount) = slot {
            if mount.mount_point == mount_point {
                return f(&mut mount.fs);
            }
        }
    }
    Err(FsError::NotFound)
}

// =============================================================================
// VFS FileSystem trait implementation for tmpfs
// =============================================================================

/// Static tmpfs filesystem adapter for VFS integration
pub struct TmpfsVfsAdapter {
    mount_point: &'static str,
}

impl TmpfsVfsAdapter {
    pub const fn new(mount_point: &'static str) -> Self {
        Self { mount_point }
    }
}

impl FileSystem for TmpfsVfsAdapter {
    fn name(&self) -> &'static str {
        "tmpfs"
    }

    fn read(&self, path: &str) -> Option<OpenFile> {
        let mounts = TMPFS_MOUNTS.lock();
        for slot in mounts.iter() {
            if let Some(mount) = slot {
                if mount.mount_point == self.mount_point {
                    let inode = mount.fs.lookup_path(path).ok()?;
                    let metadata = mount.fs.get_metadata(inode).ok()?;

                    // For files, we need to return the content
                    if let Some(TmpfsInode::File { data, .. }) = mount.fs.inodes.get(&inode) {
                        // SAFETY: We're returning a reference to data in the static mount
                        // This is safe as long as the mount isn't unmounted while reading
                        let static_data: &'static [u8] =
                            unsafe { core::slice::from_raw_parts(data.as_ptr(), data.len()) };
                        return Some(OpenFile {
                            content: FileContent::Inline(static_data),
                            metadata,
                        });
                    }

                    return None;
                }
            }
        }
        None
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        with_tmpfs(self.mount_point, |fs| {
            let inode = fs.lookup_path(path)?;
            fs.get_metadata(inode)
        })
        .ok()
    }

    fn list(&self, path: &str, cb: &mut dyn FnMut(&str, Metadata)) {
        let _ = with_tmpfs(self.mount_point, |fs| {
            let inode = fs.lookup_path(path)?;
            let entries = fs.list_directory(inode)?;
            for (name, child_inode) in entries {
                if let Ok(meta) = fs.get_metadata(child_inode) {
                    cb(&name, meta);
                }
            }
            Ok(())
        });
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<usize, &'static str> {
        with_tmpfs_mut(self.mount_point, |fs| {
            // Try to lookup existing file
            let inode = match fs.lookup_path(path) {
                Ok(i) => i,
                Err(FsError::NotFound) => {
                    // Create new file
                    fs.create_file(path, 0o644)?
                }
                Err(e) => return Err(e),
            };
            fs.write_file(inode, 0, data)
        })
        .map_err(|_| "tmpfs write error")
    }

    fn create(&self, path: &str) -> Result<(), &'static str> {
        with_tmpfs_mut(self.mount_point, |fs| fs.create_file(path, 0o644).map(|_| ()))
            .map_err(|_| "tmpfs create error")
    }
}

// =============================================================================
// BlockFileSystem trait implementation for tmpfs
// =============================================================================

impl BlockFileSystem for TmpfsInstance {
    fn fs_type(&self) -> &'static str {
        "tmpfs"
    }

    fn is_readonly(&self) -> bool {
        false
    }

    fn lookup(&self, path: &str) -> FsResult<FsFileHandle> {
        let inode = self.lookup_path(path)?;
        let metadata = self.get_metadata(inode)?;

        Ok(FsFileHandle::new(
            inode,
            metadata.size,
            metadata.mode,
            metadata.uid,
            metadata.gid,
            metadata.mtime,
            metadata.nlink,
            metadata.blocks,
        ))
    }

    fn read(&self, handle: &FsFileHandle, offset: usize, buf: &mut [u8]) -> FsResult<usize> {
        self.read_file(handle.id, offset, buf)
    }

    fn stat(&self, path: &str) -> FsResult<Metadata> {
        let inode = self.lookup_path(path)?;
        self.get_metadata(inode)
    }

    fn readdir(&self, path: &str, callback: &mut dyn FnMut(DirEntry)) -> FsResult<()> {
        let inode = self.lookup_path(path)?;
        let entries = self.list_directory(inode)?;

        for (name, child_inode) in entries {
            if let Ok(meta) = self.get_metadata(child_inode) {
                let file_type = match meta.file_type {
                    FileType::Regular => 8,
                    FileType::Directory => 4,
                    FileType::Symlink => 10,
                    _ => 0,
                };

                callback(DirEntry::new(child_inode, &name, file_type));
            }
        }

        Ok(())
    }

    fn write(&self, _handle: &FsFileHandle, _offset: usize, _data: &[u8]) -> FsResult<usize> {
        // BlockFileSystem trait uses immutable self, so we can't write directly
        // Use the mutable methods through with_tmpfs_mut instead
        Err(FsError::NotSupported)
    }

    fn create(&self, _path: &str, _mode: u16) -> FsResult<FsFileHandle> {
        Err(FsError::NotSupported)
    }

    fn mkdir(&self, _path: &str, _mode: u16) -> FsResult<()> {
        Err(FsError::NotSupported)
    }

    fn unlink(&self, _path: &str) -> FsResult<()> {
        Err(FsError::NotSupported)
    }

    fn statfs(&self) -> FsResult<FsStats> {
        Ok(self.stats())
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Create a file in tmpfs
pub fn tmpfs_create_file(mount_point: &str, path: &str, mode: u16) -> FsResult<u64> {
    with_tmpfs_mut(mount_point, |fs| fs.create_file(path, mode))
}

/// Create a directory in tmpfs
pub fn tmpfs_create_directory(mount_point: &str, path: &str, mode: u16) -> FsResult<u64> {
    with_tmpfs_mut(mount_point, |fs| fs.create_directory(path, mode))
}

/// Write to a file in tmpfs
pub fn tmpfs_write_file(mount_point: &str, path: &str, data: &[u8]) -> FsResult<usize> {
    with_tmpfs_mut(mount_point, |fs| {
        let inode = match fs.lookup_path(path) {
            Ok(i) => i,
            Err(FsError::NotFound) => fs.create_file(path, 0o644)?,
            Err(e) => return Err(e),
        };
        fs.write_file(inode, 0, data)
    })
}

/// Read from a file in tmpfs
pub fn tmpfs_read_file(mount_point: &str, path: &str, buf: &mut [u8]) -> FsResult<usize> {
    with_tmpfs(mount_point, |fs| {
        let inode = fs.lookup_path(path)?;
        fs.read_file(inode, 0, buf)
    })
}

/// Get metadata for a path in tmpfs
pub fn tmpfs_stat(mount_point: &str, path: &str) -> FsResult<Metadata> {
    with_tmpfs(mount_point, |fs| {
        let inode = fs.lookup_path(path)?;
        fs.get_metadata(inode)
    })
}

/// Remove a file or directory from tmpfs
pub fn tmpfs_remove(mount_point: &str, path: &str) -> FsResult<()> {
    with_tmpfs_mut(mount_point, |fs| fs.remove_file(path))
}

/// Get tmpfs statistics
pub fn tmpfs_stats(mount_point: &str) -> FsResult<FsStats> {
    with_tmpfs(mount_point, |fs| Ok(fs.stats()))
}

/// Check if a tmpfs is mounted at the given path
pub fn is_tmpfs_mounted(mount_point: &str) -> bool {
    let mounts = TMPFS_MOUNTS.lock();
    mounts
        .iter()
        .flatten()
        .any(|m| m.mount_point == mount_point)
}

/// Get list of all mounted tmpfs instances
pub fn list_tmpfs_mounts() -> [Option<&'static str>; 8] {
    let mounts = TMPFS_MOUNTS.lock();
    let mut result = [None; 8];
    for (i, slot) in mounts.iter().enumerate() {
        if let Some(mount) = slot {
            result[i] = Some(mount.mount_point);
        }
    }
    result
}
