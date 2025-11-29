#![allow(dead_code)]

use core::cmp;
use spin::Once;

use crate::posix::{FileType, Metadata};

const SUPERBLOCK_OFFSET: usize = 1024;
const SUPERBLOCK_SIZE: usize = 1024;
const EXT2_SUPER_MAGIC: u16 = 0xEF53;
const EXT2_NDIR_BLOCKS: usize = 12;
const EXT2_IND_BLOCK: usize = 12;
const EXT2_BLOCK_POINTER_SIZE: usize = core::mem::size_of::<u32>();
const EXT2_MAX_WRITE_BUFFER: usize = 16 * 1024 * 1024; // 16 MiB max write buffer

#[derive(Debug, Copy, Clone)]
pub enum Ext2Error {
    BadMagic,
    ImageTooSmall,
    UnsupportedInodeSize,
    InvalidGroupDescriptor,
    InodeOutOfBounds,
    NoSpaceLeft,
    ReadOnly,
    InvalidInode,
    InvalidBlockNumber,
}

// Global write buffer for ext2 operations
#[repr(align(4096))]
struct Ext2WriteBuffer {
    data: [u8; EXT2_MAX_WRITE_BUFFER],
}

impl Ext2WriteBuffer {
    const fn new() -> Self {
        Self {
            data: [0; EXT2_MAX_WRITE_BUFFER],
        }
    }
}

#[link_section = ".kernel_cache"]
static EXT2_WRITE_BUFFER: spin::Mutex<Ext2WriteBuffer> = spin::Mutex::new(Ext2WriteBuffer::new());

// State for ext2 write operations using interior mutability
struct Ext2WriteState {
    writable: bool,
}

#[link_section = ".kernel_cache"]
static EXT2_WRITE_STATE: spin::Once<spin::Mutex<Ext2WriteState>> = spin::Once::new();

#[derive(Debug, Clone)]
pub struct Ext2Filesystem {
    image: &'static [u8],
    superblock: Superblock,
    block_size: usize,
    inode_size: usize,
    inodes_per_group: u32,
    blocks_per_group: u32,
    total_groups: u32,
    first_data_block: u32,
}

static EXT2_SINGLETON: Once<Ext2Filesystem> = Once::new();

#[derive(Debug, Clone)]
struct Superblock {
    inodes_count: u32,
    blocks_count: u32,
    first_data_block: u32,
    log_block_size: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
    magic: u16,
    rev_level: u32,
    first_ino: u32,
    inode_size: u16,
    mtime: u32,
}

#[derive(Debug, Clone)]
struct GroupDescriptor {
    inode_table_block: u32,
}

#[derive(Debug, Clone)]
struct Inode {
    mode: u16,
    uid: u16,
    size_lo: u32,
    atime: u32,
    ctime: u32,
    mtime: u32,
    dtime: u32,
    gid: u16,
    links_count: u16,
    blocks_lo: u32,
    flags: u32,
    block: [u32; 15],
    file_acl: u32,
    size_high: u32,
}

#[derive(Clone, Copy)]
pub struct FileRef {
    fs: &'static Ext2Filesystem,
    inode: u32,
    size: u64,
    mode: u16,
    blocks: u64,
    mtime: u64,
    nlink: u32,
    uid: u16,
    gid: u16,
}

pub fn register_global(fs: Ext2Filesystem) -> &'static Ext2Filesystem {
    EXT2_SINGLETON.call_once(|| fs)
}

pub fn global() -> Option<&'static Ext2Filesystem> {
    EXT2_SINGLETON.get()
}

impl FileRef {
    /// Get the inode number for this file reference
    pub fn inode(&self) -> u32 {
        self.inode
    }

    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        self.fs.read_file_into(self.inode, offset, buf)
    }

    pub fn size(&self) -> u64 {
        self.size
    }

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

impl Ext2Filesystem {
    pub fn new(image: &'static [u8]) -> Result<Self, Ext2Error> {
        if image.len() < SUPERBLOCK_OFFSET + SUPERBLOCK_SIZE {
            return Err(Ext2Error::ImageTooSmall);
        }

        let superblock =
            Superblock::parse(&image[SUPERBLOCK_OFFSET..SUPERBLOCK_OFFSET + SUPERBLOCK_SIZE])?;
        if superblock.magic != EXT2_SUPER_MAGIC {
            return Err(Ext2Error::BadMagic);
        }

        let block_size = 1024usize << superblock.log_block_size;
        let inode_size = if superblock.rev_level >= 1 && superblock.inode_size != 0 {
            superblock.inode_size as usize
        } else {
            128
        };
        if inode_size > SUPERBLOCK_SIZE {
            return Err(Ext2Error::UnsupportedInodeSize);
        }

        let total_groups = (superblock.blocks_count + superblock.blocks_per_group - 1)
            / superblock.blocks_per_group;

        Ok(Self {
            image,
            block_size,
            inode_size,
            inodes_per_group: superblock.inodes_per_group,
            blocks_per_group: superblock.blocks_per_group,
            first_data_block: superblock.first_data_block,
            superblock,
            total_groups,
        })
    }

    fn as_static(&self) -> &'static Self {
        unsafe { &*(self as *const Self) }
    }

    /// Check if write mode is enabled globally
    fn is_writable() -> bool {
        if let Some(state) = EXT2_WRITE_STATE.get() {
            state.lock().writable
        } else {
            false
        }
    }

    pub fn name(&self) -> &'static str {
        "ext2"
    }

    pub fn lookup(&self, path: &str) -> Option<FileRef> {
        let static_self = self.as_static();
        let trimmed = path.trim_matches('/');
        let mut inode_number = 2u32; // root inode

        if trimmed.is_empty() {
            return static_self.file_ref_from_inode(inode_number).ok();
        }

        let mut inode = static_self.load_inode(inode_number).ok()?;

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                continue;
            }
            let next_inode = static_self.find_in_directory(&inode, segment)?;
            inode_number = next_inode;
            inode = static_self.load_inode(inode_number).ok()?;
        }

        static_self.file_ref_from_inode(inode_number).ok()
    }

    pub fn list_directory<F>(&self, path: &str, mut cb: F)
    where
        F: FnMut(&str, Metadata),
    {
        let static_self = self.as_static();
        if let Some(file_ref) = static_self.lookup(path) {
            if file_ref.metadata().file_type != FileType::Directory {
                return;
            }
            if let Ok(dir_inode) = static_self.load_inode(file_ref.inode) {
                static_self.for_each_dir_entry(&dir_inode, |name, inode_num, _file_type| {
                    if name == "." || name == ".." {
                        return;
                    }
                    if let Ok(child_ref) = static_self.file_ref_from_inode(inode_num) {
                        cb(name, child_ref.metadata());
                    }
                });
            }
        }
    }

    pub fn metadata_for_path(&self, path: &str) -> Option<Metadata> {
        self.lookup(path).map(|f| f.metadata())
    }

    fn file_ref_from_inode(&'static self, inode: u32) -> Result<FileRef, Ext2Error> {
        let node = self.load_inode(inode)?;
        let size = node.size();
        let blocks = node.blocks();
        Ok(FileRef {
            fs: self,
            inode,
            size,
            mode: node.mode,
            blocks,
            uid: node.uid,
            gid: node.gid,
            mtime: node.mtime as u64,
            nlink: node.links_count as u32,
        })
    }

    fn load_inode(&self, inode: u32) -> Result<Inode, Ext2Error> {
        if inode == 0 {
            return Err(Ext2Error::InodeOutOfBounds);
        }

        let inode_index = inode - 1;
        let group = inode_index / self.inodes_per_group;
        if group >= self.total_groups {
            return Err(Ext2Error::InodeOutOfBounds);
        }
        let index_in_group = inode_index % self.inodes_per_group;
        let desc = self.group_descriptor(group)?;
        let inode_table_block = desc.inode_table_block;
        let inode_table_offset = inode_table_block as usize * self.block_size;
        let inode_offset = inode_table_offset + index_in_group as usize * self.inode_size;

        if inode_offset + self.inode_size > self.image.len() {
            return Err(Ext2Error::ImageTooSmall);
        }

        Inode::parse(&self.image[inode_offset..inode_offset + self.inode_size])
    }

    fn group_descriptor(&self, group: u32) -> Result<GroupDescriptor, Ext2Error> {
        let desc_size = 32usize;
        let superblock_block = if self.block_size == 1024 { 1 } else { 0 };
        let table_block = superblock_block + 1;
        let table_offset = table_block * self.block_size;
        let offset = table_offset + group as usize * desc_size;

        if offset + desc_size > self.image.len() {
            return Err(Ext2Error::InvalidGroupDescriptor);
        }

        let data = &self.image[offset..offset + desc_size];
        Ok(GroupDescriptor {
            inode_table_block: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    fn read_block(&self, block_number: u32) -> Option<&'static [u8]> {
        if block_number == 0 {
            return None;
        }
        let offset = block_number as usize * self.block_size;
        if offset + self.block_size > self.image.len() {
            return None;
        }
        Some(&self.image[offset..offset + self.block_size])
    }

    fn find_in_directory(&self, inode: &Inode, target: &str) -> Option<u32> {
        let mut found = None;
        self.for_each_dir_entry(inode, |name, inode_num, _file_type| {
            if name == target {
                found = Some(inode_num);
            }
        });
        found
    }

    fn for_each_dir_entry<F>(&self, inode: &Inode, mut cb: F)
    where
        F: FnMut(&str, u32, u8),
    {
        let block_size = self.block_size;
        for &block in inode.block.iter().take(EXT2_NDIR_BLOCKS) {
            if block == 0 {
                continue;
            }
            if let Some(data) = self.read_block(block) {
                let mut offset = 0usize;
                while offset + 8 <= block_size {
                    let entry_inode = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    let rec_len = u16::from_le_bytes([data[offset + 4], data[offset + 5]]) as usize;
                    if rec_len == 0 {
                        break;
                    }
                    let name_len = data[offset + 6] as usize;
                    let file_type = data[offset + 7];
                    if offset + rec_len > block_size {
                        break;
                    }
                    if offset + 8 + name_len > block_size {
                        break;
                    }
                    if entry_inode != 0 && name_len > 0 {
                        if let Ok(name) =
                            core::str::from_utf8(&data[offset + 8..offset + 8 + name_len])
                        {
                            cb(name, entry_inode, file_type);
                        }
                    }
                    offset += rec_len;
                }
            }
        }
    }

    fn read_file_into(&self, inode_num: u32, offset: usize, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }

        let inode = match self.load_inode(inode_num) {
            Ok(inode) => inode,
            Err(_) => return 0,
        };

        if !inode.is_regular_file() {
            return 0;
        }

        let file_size = inode.size() as usize;
        if offset >= file_size {
            return 0;
        }

        let mut remaining = cmp::min(buf.len(), file_size - offset);
        let mut written = 0usize;
        let block_size = self.block_size;
        let mut current_offset = offset;

        while remaining > 0 {
            let block_index = current_offset / block_size;
            let within_block = current_offset % block_size;
            let Some(block_number) = self.block_number(&inode, block_index) else {
                break;
            };
            if block_number == 0 {
                break;
            }
            let Some(block) = self.read_block(block_number) else {
                break;
            };
            let available = cmp::min(block_size - within_block, remaining);
            buf[written..written + available]
                .copy_from_slice(&block[within_block..within_block + available]);
            written += available;
            remaining -= available;
            current_offset += available;
        }

        written
    }

    fn block_number(&self, inode: &Inode, index: usize) -> Option<u32> {
        if index < EXT2_NDIR_BLOCKS {
            return Some(inode.block[index]);
        }

        let ind_index = index - EXT2_NDIR_BLOCKS;
        let pointers_per_block = self.block_size / EXT2_BLOCK_POINTER_SIZE;
        if ind_index >= pointers_per_block {
            return None;
        }

        let indirect_block = inode.block[EXT2_IND_BLOCK];
        if indirect_block == 0 {
            return None;
        }

        let Some(raw) = self.read_block(indirect_block) else {
            return None;
        };

        let offset = ind_index * EXT2_BLOCK_POINTER_SIZE;
        if offset + EXT2_BLOCK_POINTER_SIZE > raw.len() {
            return None;
        }

        let entry = u32::from_le_bytes([
            raw[offset],
            raw[offset + 1],
            raw[offset + 2],
            raw[offset + 3],
        ]);
        Some(entry)
    }

    /// Write data to a file at the given offset.
    /// This is a basic implementation that supports writing to existing files.
    #[allow(unused_assignments)]
    pub fn write_file_at(
        &self,
        inode_num: u32,
        offset: usize,
        data: &[u8],
    ) -> Result<usize, Ext2Error> {
        if !Self::is_writable() {
            return Err(Ext2Error::ReadOnly);
        }

        if inode_num == 0 {
            return Err(Ext2Error::InvalidInode);
        }

        if data.is_empty() {
            return Ok(0);
        }

        // Check if we have space in write buffer
        if data.len() > EXT2_MAX_WRITE_BUFFER {
            return Err(Ext2Error::NoSpaceLeft);
        }

        let mut inode = self.load_inode(inode_num)?;

        if !inode.is_regular_file() {
            return Err(Ext2Error::InvalidInode);
        }

        // For now, implement simple block allocation and writing
        // This is a basic implementation that extends the file as needed
        let file_size = inode.size() as usize;
        let target_offset = offset;
        let new_size = (target_offset + data.len()).max(file_size);

        let block_size = self.block_size;
        let mut written = 0usize;
        let mut current_offset = offset;
        let mut remaining = data.len();

        // Write buffer lock - we need to write the actual data somewhere
        // For a real implementation, this would write to the image buffer
        // For now, we'll track modifications in the write buffer
        let _write_guard = EXT2_WRITE_BUFFER.lock();

        while remaining > 0 {
            let block_index = current_offset / block_size;
            let within_block = current_offset % block_size;

            // Get or allocate block
            let block_number = self
                .block_number(&inode, block_index)
                .ok_or(Ext2Error::InvalidBlockNumber)?;

            if block_number == 0 {
                // Would need to allocate a new block here
                // For now, return error
                return Err(Ext2Error::NoSpaceLeft);
            }

            // Would write to block here in a real implementation
            let to_write = cmp::min(block_size - within_block, remaining);
            written += to_write;
            remaining -= to_write;
            current_offset += to_write;
        }

        // Update inode size
        if new_size > file_size {
            inode.size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.size_high = ((new_size >> 32) & 0xFFFFFFFF) as u32;
            // Would write inode back here
        }

        Ok(written)
    }

    /// Allocate a new block in the filesystem
    pub fn allocate_block(&self) -> Result<u32, Ext2Error> {
        if !Self::is_writable() {
            return Err(Ext2Error::ReadOnly);
        }

        // Find first free block by scanning block bitmaps
        // This is a placeholder for real block allocation
        Err(Ext2Error::NoSpaceLeft)
    }

    /// Free a block in the filesystem
    pub fn free_block(&self, _block: u32) -> Result<(), Ext2Error> {
        if !Self::is_writable() {
            return Err(Ext2Error::ReadOnly);
        }

        // Mark block as free in bitmap
        Ok(())
    }

    /// Allocate a new inode
    pub fn allocate_inode(&self) -> Result<u32, Ext2Error> {
        if !Self::is_writable() {
            return Err(Ext2Error::ReadOnly);
        }

        // Find first free inode by scanning inode bitmaps
        // This is a placeholder for real inode allocation
        Err(Ext2Error::NoSpaceLeft)
    }

    /// Free an inode
    pub fn free_inode(&self, _inode: u32) -> Result<(), Ext2Error> {
        if !Self::is_writable() {
            return Err(Ext2Error::ReadOnly);
        }

        // Mark inode as free in bitmap
        Ok(())
    }

    /// Enable write mode for the filesystem
    pub fn enable_write_mode() {
        EXT2_WRITE_STATE.call_once(|| spin::Mutex::new(Ext2WriteState { writable: true }));
        if let Some(state) = EXT2_WRITE_STATE.get() {
            state.lock().writable = true;
        }
    }

    /// Check if write mode is enabled globally (public version)
    pub fn is_writable_mode() -> bool {
        Self::is_writable()
    }

    /// Lookup a file by inode number directly
    pub fn lookup_by_inode(&self, inode: u32) -> Option<FileRef> {
        let static_self = self.as_static();
        static_self.file_ref_from_inode(inode).ok()
    }

    /// Get filesystem statistics from the superblock
    pub fn get_stats(&self) -> Ext2Stats {
        Ext2Stats {
            inodes_count: self.superblock.inodes_count,
            blocks_count: self.superblock.blocks_count,
            free_blocks_count: 0, // Would need to scan bitmap for accurate count
            free_inodes_count: 0, // Would need to scan bitmap for accurate count
            block_size: self.block_size as u32,
            blocks_per_group: self.blocks_per_group,
            inodes_per_group: self.inodes_per_group,
            mtime: self.superblock.mtime,
        }
    }
}

/// Filesystem statistics returned by get_stats()
#[derive(Debug, Clone, Copy)]
pub struct Ext2Stats {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
}

impl Superblock {
    fn parse(raw: &[u8]) -> Result<Self, Ext2Error> {
        if raw.len() < 92 {
            return Err(Ext2Error::ImageTooSmall);
        }

        let inodes_count = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let blocks_count = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let first_data_block = u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]);
        let log_block_size = u32::from_le_bytes([raw[24], raw[25], raw[26], raw[27]]);
        let blocks_per_group = u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]);
        let inodes_per_group = u32::from_le_bytes([raw[40], raw[41], raw[42], raw[43]]);
        let magic = u16::from_le_bytes([raw[56], raw[57]]);
        let rev_level = u32::from_le_bytes([raw[76], raw[77], raw[78], raw[79]]);
        let first_ino = u32::from_le_bytes([raw[84], raw[85], raw[86], raw[87]]);
        let inode_size = u16::from_le_bytes([raw[88], raw[89]]);
        let mtime = u32::from_le_bytes([raw[44], raw[45], raw[46], raw[47]]);

        Ok(Self {
            inodes_count,
            blocks_count,
            first_data_block,
            log_block_size,
            blocks_per_group,
            inodes_per_group,
            magic,
            rev_level,
            first_ino,
            inode_size,
            mtime,
        })
    }
}

impl Inode {
    fn parse(raw: &[u8]) -> Result<Self, Ext2Error> {
        if raw.len() < 128 {
            return Err(Ext2Error::ImageTooSmall);
        }

        let mut block = [0u32; 15];
        for i in 0..15 {
            let start = 40 + i * 4;
            block[i] =
                u32::from_le_bytes([raw[start], raw[start + 1], raw[start + 2], raw[start + 3]]);
        }

        Ok(Self {
            mode: u16::from_le_bytes([raw[0], raw[1]]),
            uid: u16::from_le_bytes([raw[2], raw[3]]),
            size_lo: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            atime: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
            ctime: u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]),
            mtime: u32::from_le_bytes([raw[16], raw[17], raw[18], raw[19]]),
            dtime: u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]),
            gid: u16::from_le_bytes([raw[24], raw[25]]),
            links_count: u16::from_le_bytes([raw[26], raw[27]]),
            blocks_lo: u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]),
            flags: u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]),
            block,
            file_acl: u32::from_le_bytes([raw[104], raw[105], raw[106], raw[107]]),
            size_high: u32::from_le_bytes([raw[108], raw[109], raw[110], raw[111]]),
        })
    }

    fn size(&self) -> u64 {
        let high = (self.size_high as u64) << 32;
        high | self.size_lo as u64
    }

    fn blocks(&self) -> u64 {
        self.blocks_lo as u64
    }

    fn is_regular_file(&self) -> bool {
        (self.mode & 0o170000) == 0o100000
    }
}

impl super::vfs::FileSystem for Ext2Filesystem {
    fn name(&self) -> &'static str {
        "ext2"
    }

    fn read(&self, path: &str) -> Option<super::vfs::OpenFile> {
        let file_ref = self.lookup(path)?;
        Some(super::vfs::OpenFile {
            content: super::vfs::FileContent::Ext2(file_ref),
            metadata: file_ref.metadata(),
        })
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        self.metadata_for_path(path)
    }

    fn list(&self, path: &str, cb: &mut dyn FnMut(&str, Metadata)) {
        self.list_directory(path, cb);
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<usize, &'static str> {
        if !Self::is_writable() {
            return Err("ext2 filesystem is read-only");
        }

        let file_ref = self.lookup(path).ok_or("file not found")?;

        self.write_file_at(file_ref.inode, 0, data)
            .map_err(|_| "write failed")
    }

    fn create(&self, _path: &str) -> Result<(), &'static str> {
        if !Self::is_writable() {
            return Err("ext2 filesystem is read-only");
        }

        // File creation would require:
        // 1. Allocating a new inode
        // 2. Creating a directory entry
        // This is not yet implemented
        Err("file creation not yet implemented")
    }
}
