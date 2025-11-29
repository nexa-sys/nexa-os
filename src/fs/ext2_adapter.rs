//! ext2 filesystem abstraction adapter
//!
//! This module provides an adapter that implements the `BlockFileSystem` trait
//! for the ext2 filesystem, allowing ext2 to be used through the abstract
//! filesystem interface.

use crate::posix::Metadata;

use super::ext2::{self, Ext2Error, Ext2Filesystem, FileRef};
use super::traits::{BlockFileSystem, DirEntry, FsError, FsFileHandle, FsResult, FsStats};

/// Convert ext2-specific errors to generic filesystem errors
impl From<Ext2Error> for FsError {
    fn from(err: Ext2Error) -> Self {
        match err {
            Ext2Error::BadMagic => FsError::FsSpecific("bad ext2 magic number"),
            Ext2Error::ImageTooSmall => FsError::FsSpecific("image too small"),
            Ext2Error::UnsupportedInodeSize => FsError::FsSpecific("unsupported inode size"),
            Ext2Error::InvalidGroupDescriptor => FsError::InvalidReference,
            Ext2Error::InodeOutOfBounds => FsError::InvalidReference,
            Ext2Error::NoSpaceLeft => FsError::NoSpace,
            Ext2Error::ReadOnly => FsError::ReadOnly,
            Ext2Error::InvalidInode => FsError::InvalidReference,
            Ext2Error::InvalidBlockNumber => FsError::InvalidReference,
        }
    }
}

/// Convert FileRef to FsFileHandle
impl From<&FileRef> for FsFileHandle {
    fn from(file_ref: &FileRef) -> Self {
        let meta = file_ref.metadata();
        FsFileHandle::new(
            file_ref.inode() as u64,
            meta.size,
            meta.mode,
            meta.uid,
            meta.gid,
            meta.mtime,
            meta.nlink,
            meta.blocks,
        )
    }
}

/// Adapter struct that wraps Ext2Filesystem and implements BlockFileSystem
pub struct Ext2Adapter {
    fs: &'static Ext2Filesystem,
}

impl Ext2Adapter {
    /// Create a new adapter from a static ext2 filesystem reference
    pub fn new(fs: &'static Ext2Filesystem) -> Self {
        Self { fs }
    }

    /// Get the underlying ext2 filesystem reference
    pub fn inner(&self) -> &'static Ext2Filesystem {
        self.fs
    }

    /// Try to create an adapter from the global ext2 filesystem
    pub fn from_global() -> Option<Self> {
        ext2::global().map(Self::new)
    }
}

impl BlockFileSystem for Ext2Adapter {
    fn fs_type(&self) -> &'static str {
        "ext2"
    }

    fn is_readonly(&self) -> bool {
        !Ext2Filesystem::is_writable_mode()
    }

    fn lookup(&self, path: &str) -> FsResult<FsFileHandle> {
        self.fs
            .lookup(path)
            .map(|ref file_ref| FsFileHandle::from(file_ref))
            .ok_or(FsError::NotFound)
    }

    fn read(&self, handle: &FsFileHandle, offset: usize, buf: &mut [u8]) -> FsResult<usize> {
        // We need to get the FileRef from the inode number stored in the handle
        let file_ref = self
            .fs
            .lookup_by_inode(handle.id as u32)
            .ok_or(FsError::InvalidReference)?;

        Ok(file_ref.read_at(offset, buf))
    }

    fn stat(&self, path: &str) -> FsResult<Metadata> {
        self.fs
            .metadata_for_path(path)
            .ok_or(FsError::NotFound)
    }

    fn readdir(&self, path: &str, callback: &mut dyn FnMut(DirEntry)) -> FsResult<()> {
        let file_ref = self.fs.lookup(path).ok_or(FsError::NotFound)?;
        
        if file_ref.metadata().file_type != crate::posix::FileType::Directory {
            return Err(FsError::NotADirectory);
        }

        self.fs.list_directory(path, |name, meta| {
            // Map FileType to ext2 directory entry type
            let file_type = match meta.file_type {
                crate::posix::FileType::Regular => 1,    // EXT2_FT_REG_FILE
                crate::posix::FileType::Directory => 2,  // EXT2_FT_DIR
                crate::posix::FileType::Character => 3,  // EXT2_FT_CHRDEV
                crate::posix::FileType::Block => 4,      // EXT2_FT_BLKDEV
                crate::posix::FileType::Fifo => 5,       // EXT2_FT_FIFO
                crate::posix::FileType::Socket => 6,     // EXT2_FT_SOCK
                crate::posix::FileType::Symlink => 7,    // EXT2_FT_SYMLINK
                _ => 0,                                   // EXT2_FT_UNKNOWN
            };

            let entry = DirEntry::new(0, name, file_type); // inode not easily available here
            callback(entry);
        });

        Ok(())
    }

    fn write(&self, handle: &FsFileHandle, offset: usize, data: &[u8]) -> FsResult<usize> {
        if self.is_readonly() {
            return Err(FsError::ReadOnly);
        }

        self.fs
            .write_file_at(handle.id as u32, offset, data)
            .map_err(FsError::from)
    }

    fn statfs(&self) -> FsResult<FsStats> {
        // Get filesystem statistics from ext2 superblock
        let stats = self.fs.get_stats();
        Ok(FsStats {
            total_blocks: stats.blocks_count as u64,
            free_blocks: stats.free_blocks_count as u64,
            avail_blocks: stats.free_blocks_count as u64, // Same for now
            total_inodes: stats.inodes_count as u64,
            free_inodes: stats.free_inodes_count as u64,
            block_size: stats.block_size,
            name_max: 255,
            fs_type: 0xEF53, // EXT2_SUPER_MAGIC
        })
    }

    fn sync(&self) -> FsResult<()> {
        // ext2 currently operates on memory-mapped image, sync is a no-op
        Ok(())
    }
}

// Make Ext2Adapter Send + Sync (required by BlockFileSystem)
// SAFETY: Ext2Filesystem is already Sync (uses spin locks internally)
unsafe impl Send for Ext2Adapter {}
unsafe impl Sync for Ext2Adapter {}

/// Helper function to create an ext2 adapter from a raw image
pub fn create_ext2_adapter(image: &'static [u8]) -> Result<Ext2Adapter, Ext2Error> {
    let fs = Ext2Filesystem::new(image)?;
    let fs_ref = ext2::register_global(fs);
    Ok(Ext2Adapter::new(fs_ref))
}

/// Get the global ext2 adapter (if ext2 is mounted)
pub fn global_ext2_adapter() -> Option<Ext2Adapter> {
    Ext2Adapter::from_global()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that FsError conversions work correctly
    #[test]
    fn test_error_conversion() {
        let err: FsError = Ext2Error::BadMagic.into();
        assert!(matches!(err, FsError::FsSpecific(_)));

        let err: FsError = Ext2Error::NoSpaceLeft.into();
        assert!(matches!(err, FsError::NoSpace));

        let err: FsError = Ext2Error::ReadOnly.into();
        assert!(matches!(err, FsError::ReadOnly));
    }
}
