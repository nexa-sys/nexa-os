//! ext4 Inode handling
//!
//! ext4 inodes extend ext2/ext3 with larger sizes, nanosecond timestamps,
//! and extent-based block mapping.

use crate::extent::{ExtentTree, EXT4_EXT_MAGIC};

pub const EXT4_INODE_FLAG_EXTENTS: u32 = 0x00080000;

/// ext4 Inode structure
#[derive(Debug, Clone)]
pub struct Ext4Inode {
    pub mode: u16,
    pub uid: u16,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks_lo: u32,
    pub flags: u32,
    pub block: [u8; 60],  // Raw block data (extents or indirect)
    pub generation: u32,
    pub file_acl_lo: u32,
    pub size_high: u32,
    
    // Extended fields (for inode_size > 128)
    pub extra_isize: u16,
    pub ctime_extra: u32,   // Extra ctime bits for nanoseconds
    pub mtime_extra: u32,
    pub atime_extra: u32,
    pub crtime: u32,        // File creation time
    pub crtime_extra: u32,
}

impl Ext4Inode {
    pub fn parse(raw: &[u8], inode_size: usize) -> Result<Self, i32> {
        if raw.len() < 128 {
            return Err(-1);
        }

        let mut block = [0u8; 60];
        block.copy_from_slice(&raw[40..100]);

        let mut inode = Self {
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
            generation: u32::from_le_bytes([raw[100], raw[101], raw[102], raw[103]]),
            file_acl_lo: u32::from_le_bytes([raw[104], raw[105], raw[106], raw[107]]),
            size_high: u32::from_le_bytes([raw[108], raw[109], raw[110], raw[111]]),
            extra_isize: 0,
            ctime_extra: 0,
            mtime_extra: 0,
            atime_extra: 0,
            crtime: 0,
            crtime_extra: 0,
        };

        // Parse extended fields if available
        if inode_size > 128 && raw.len() >= 160 {
            inode.extra_isize = u16::from_le_bytes([raw[128], raw[129]]);
            if raw.len() >= 140 {
                inode.ctime_extra = u32::from_le_bytes([raw[132], raw[133], raw[134], raw[135]]);
                inode.mtime_extra = u32::from_le_bytes([raw[136], raw[137], raw[138], raw[139]]);
            }
            if raw.len() >= 148 {
                inode.atime_extra = u32::from_le_bytes([raw[140], raw[141], raw[142], raw[143]]);
                inode.crtime = u32::from_le_bytes([raw[144], raw[145], raw[146], raw[147]]);
            }
            if raw.len() >= 152 {
                inode.crtime_extra = u32::from_le_bytes([raw[148], raw[149], raw[150], raw[151]]);
            }
        }

        Ok(inode)
    }

    pub fn size(&self) -> u64 {
        ((self.size_high as u64) << 32) | (self.size_lo as u64)
    }

    pub fn is_regular_file(&self) -> bool {
        (self.mode & 0o170000) == 0o100000
    }

    pub fn is_directory(&self) -> bool {
        (self.mode & 0o170000) == 0o040000
    }

    pub fn is_symlink(&self) -> bool {
        (self.mode & 0o170000) == 0o120000
    }

    pub fn uses_extents(&self) -> bool {
        (self.flags & EXT4_INODE_FLAG_EXTENTS) != 0
    }

    /// Get extent tree for this inode (if using extents)
    pub fn extent_tree(&self, block_size: usize) -> Option<ExtentTree<'_>> {
        if !self.uses_extents() {
            return None;
        }
        ExtentTree::new(&self.block, block_size)
    }

    /// Get block number using traditional indirect blocks (ext2/ext3 style)
    pub fn indirect_block(&self, index: usize, block_size: usize) -> Option<u32> {
        if self.uses_extents() {
            return None;
        }
        
        let pointers_per_block = block_size / 4;
        
        // Direct blocks (0-11)
        if index < 12 {
            let offset = index * 4;
            return Some(u32::from_le_bytes([
                self.block[offset],
                self.block[offset + 1],
                self.block[offset + 2],
                self.block[offset + 3],
            ]));
        }
        
        // Indirect blocks would require reading from disk
        // Return None to indicate need for I/O
        None
    }
}

/// Group descriptor for ext4 (32 or 64 bytes)
#[derive(Debug, Clone)]
pub struct GroupDescriptor {
    pub block_bitmap_lo: u32,
    pub inode_bitmap_lo: u32,
    pub inode_table_lo: u32,
    pub free_blocks_count_lo: u16,
    pub free_inodes_count_lo: u16,
    pub used_dirs_count_lo: u16,
    pub flags: u16,
    // 64-bit fields
    pub block_bitmap_hi: u32,
    pub inode_bitmap_hi: u32,
    pub inode_table_hi: u32,
}

impl GroupDescriptor {
    pub fn parse(raw: &[u8], desc_size: usize) -> Result<Self, i32> {
        if raw.len() < 32 {
            return Err(-1);
        }

        let mut desc = Self {
            block_bitmap_lo: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            inode_bitmap_lo: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            inode_table_lo: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
            free_blocks_count_lo: u16::from_le_bytes([raw[12], raw[13]]),
            free_inodes_count_lo: u16::from_le_bytes([raw[14], raw[15]]),
            used_dirs_count_lo: u16::from_le_bytes([raw[16], raw[17]]),
            flags: u16::from_le_bytes([raw[18], raw[19]]),
            block_bitmap_hi: 0,
            inode_bitmap_hi: 0,
            inode_table_hi: 0,
        };

        // Parse 64-bit fields if available
        if desc_size >= 64 && raw.len() >= 64 {
            desc.block_bitmap_hi = u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]);
            desc.inode_bitmap_hi = u32::from_le_bytes([raw[36], raw[37], raw[38], raw[39]]);
            desc.inode_table_hi = u32::from_le_bytes([raw[40], raw[41], raw[42], raw[43]]);
        }

        Ok(desc)
    }

    pub fn block_bitmap(&self) -> u64 {
        ((self.block_bitmap_hi as u64) << 32) | (self.block_bitmap_lo as u64)
    }

    pub fn inode_bitmap(&self) -> u64 {
        ((self.inode_bitmap_hi as u64) << 32) | (self.inode_bitmap_lo as u64)
    }

    pub fn inode_table(&self) -> u64 {
        ((self.inode_table_hi as u64) << 32) | (self.inode_table_lo as u64)
    }
}
