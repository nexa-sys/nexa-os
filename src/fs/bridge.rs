//! VFS-BlockFileSystem bridge adapter
//!
//! This module provides a bridge between the existing VFS FileSystem trait
//! and the new BlockFileSystem abstraction, allowing gradual migration.

use crate::posix::Metadata;

use super::traits::{BlockFileSystem, DirEntry, FsError, FsFileHandle, FsResult};
use super::vfs::{FileContent, FileSystem, OpenFile};

/// Adapter that wraps a BlockFileSystem to implement the VFS FileSystem trait
/// This allows new-style filesystems to be used with the existing VFS layer
pub struct BlockFsVfsAdapter<F: BlockFileSystem> {
    inner: F,
}

impl<F: BlockFileSystem> BlockFsVfsAdapter<F> {
    /// Create a new adapter wrapping a BlockFileSystem
    pub const fn new(fs: F) -> Self {
        Self { inner: fs }
    }

    /// Get a reference to the inner BlockFileSystem
    pub fn inner(&self) -> &F {
        &self.inner
    }
}

impl<F: BlockFileSystem + 'static> FileSystem for BlockFsVfsAdapter<F> {
    fn name(&self) -> &'static str {
        self.inner.fs_type()
    }

    fn read(&self, path: &str) -> Option<OpenFile> {
        let handle = self.inner.lookup(path).ok()?;
        let metadata = handle.to_metadata();

        // Note: We can't return an Ext2 FileContent here as we don't have
        // the specific type. This adapter is primarily for new filesystem types
        // that aren't ext2. For ext2, use the Ext2Filesystem directly.
        //
        // For a complete solution, we would need to either:
        // 1. Add a new FileContent variant for BlockFileSystem
        // 2. Or cache the file data and return it as Inline

        // For now, return None for actual file content - this adapter is
        // primarily useful for metadata operations
        Some(OpenFile {
            content: FileContent::Inline(&[]), // Placeholder - would need buffer management
            metadata,
        })
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        self.inner.stat(path).ok()
    }

    fn list(&self, path: &str, cb: &mut dyn FnMut(&str, Metadata)) {
        let _ = self.inner.readdir(path, &mut |entry: DirEntry| {
            // Get full metadata for the entry
            let entry_path = if path == "/" || path.is_empty() {
                alloc::format!("/{}", entry.name_str())
            } else {
                alloc::format!("{}/{}", path.trim_end_matches('/'), entry.name_str())
            };

            if let Ok(meta) = self.inner.stat(&entry_path) {
                cb(entry.name_str(), meta);
            } else {
                // Fallback: create minimal metadata from entry type
                let file_type = match entry.file_type {
                    1 => crate::posix::FileType::Regular,
                    2 => crate::posix::FileType::Directory,
                    3 => crate::posix::FileType::Character,
                    4 => crate::posix::FileType::Block,
                    5 => crate::posix::FileType::Fifo,
                    6 => crate::posix::FileType::Socket,
                    7 => crate::posix::FileType::Symlink,
                    _ => crate::posix::FileType::Unknown(0),
                };
                let meta = Metadata::empty().with_type(file_type);
                cb(entry.name_str(), meta);
            }
        });
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<usize, &'static str> {
        let handle = self.inner.lookup(path).map_err(|_| "file not found")?;
        self.inner.write(&handle, 0, data).map_err(|e| match e {
            FsError::ReadOnly => "filesystem is read-only",
            FsError::NoSpace => "no space left on device",
            FsError::PermissionDenied => "permission denied",
            _ => "write error",
        })
    }

    fn create(&self, path: &str) -> Result<(), &'static str> {
        self.inner
            .create(path, 0o644)
            .map(|_| ())
            .map_err(|e| match e {
                FsError::ReadOnly => "filesystem is read-only",
                FsError::AlreadyExists => "file already exists",
                FsError::NoSpace => "no space left on device",
                FsError::PermissionDenied => "permission denied",
                _ => "create error",
            })
    }
}

/// Adapter that wraps a VFS FileSystem to implement BlockFileSystem
/// This allows existing VFS filesystems to be used through the abstract interface
pub struct VfsBlockFsAdapter<'a> {
    inner: &'a dyn FileSystem,
}

impl<'a> VfsBlockFsAdapter<'a> {
    /// Create a new adapter wrapping a VFS FileSystem
    pub const fn new(fs: &'a dyn FileSystem) -> Self {
        Self { inner: fs }
    }
}

impl<'a> BlockFileSystem for VfsBlockFsAdapter<'a> {
    fn fs_type(&self) -> &'static str {
        self.inner.name()
    }

    fn is_readonly(&self) -> bool {
        // Try to detect if writable by checking if write returns an error
        // This is a heuristic - actual readonly status would need to be
        // tracked separately
        true // Default to readonly for safety
    }

    fn lookup(&self, path: &str) -> FsResult<FsFileHandle> {
        let meta = self.inner.metadata(path).ok_or(FsError::NotFound)?;

        // VFS doesn't have inode numbers, use a hash of the path
        let id = simple_path_hash(path);

        Ok(FsFileHandle::new(
            id,
            meta.size,
            meta.mode,
            meta.uid,
            meta.gid,
            meta.mtime,
            meta.nlink,
            meta.blocks,
        ))
    }

    fn read(&self, handle: &FsFileHandle, offset: usize, buf: &mut [u8]) -> FsResult<usize> {
        // VFS FileSystem doesn't support offset reads directly
        // We'd need to read the whole file and then slice
        // This is a limitation of the adapter
        let _ = (handle, offset, buf);
        Err(FsError::NotSupported)
    }

    fn stat(&self, path: &str) -> FsResult<Metadata> {
        self.inner.metadata(path).ok_or(FsError::NotFound)
    }

    fn readdir(&self, path: &str, callback: &mut dyn FnMut(DirEntry)) -> FsResult<()> {
        let meta = self.inner.metadata(path).ok_or(FsError::NotFound)?;

        if meta.file_type != crate::posix::FileType::Directory {
            return Err(FsError::NotADirectory);
        }

        self.inner
            .list(path, &mut |name: &str, entry_meta: Metadata| {
                let file_type = match entry_meta.file_type {
                    crate::posix::FileType::Regular => 1,
                    crate::posix::FileType::Directory => 2,
                    crate::posix::FileType::Character => 3,
                    crate::posix::FileType::Block => 4,
                    crate::posix::FileType::Fifo => 5,
                    crate::posix::FileType::Socket => 6,
                    crate::posix::FileType::Symlink => 7,
                    _ => 0,
                };
                callback(DirEntry::new(0, name, file_type));
            });

        Ok(())
    }

    fn write(&self, _handle: &FsFileHandle, _offset: usize, _data: &[u8]) -> FsResult<usize> {
        // VFS FileSystem write doesn't support handle-based operations
        Err(FsError::NotSupported)
    }
}

// SAFETY: VfsBlockFsAdapter is safe to send/sync because it only holds
// a reference to a dyn FileSystem which is already required to be Sync
unsafe impl<'a> Send for VfsBlockFsAdapter<'a> {}
unsafe impl<'a> Sync for VfsBlockFsAdapter<'a> {}

/// Simple hash function for paths (used to generate pseudo-inode numbers)
fn simple_path_hash(path: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in path.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}
