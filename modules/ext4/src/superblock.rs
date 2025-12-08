//! ext4 Superblock handling
//!
//! ext4 extends the ext2/ext3 superblock with additional fields for
//! 64-bit support, flexible block groups, and metadata checksumming.

use crate::{mod_error, mod_info};

pub const SUPERBLOCK_OFFSET: usize = 1024;
pub const SUPERBLOCK_SIZE: usize = 1024;
pub const EXT4_SUPER_MAGIC: u16 = 0xEF53;

// Feature flags
pub const EXT4_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
pub const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32 = 0x0200;
pub const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: u32 = 0x0008;
pub const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32 = 0x0020;
pub const EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE: u32 = 0x0040;
pub const EXT4_FEATURE_RO_COMPAT_METADATA_CSUM: u32 = 0x0400;

/// ext4 Superblock structure
#[derive(Debug, Clone)]
pub struct Ext4Superblock {
    // Base ext2 fields
    pub inodes_count: u32,
    pub blocks_count_lo: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub magic: u16,
    pub state: u16,
    pub rev_level: u32,
    pub first_ino: u32,
    pub inode_size: u16,
    pub mtime: u32,
    
    // ext3 fields
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub journal_inum: u32,
    
    // ext4 specific fields
    pub blocks_count_hi: u32,
    pub log_groups_per_flex: u8,
    pub checksum_type: u8,
    pub desc_size: u16,
    pub flags: u32,
}

impl Ext4Superblock {
    pub fn parse(raw: &[u8]) -> Result<Self, i32> {
        if raw.len() < 256 {
            return Err(-1);
        }

        let magic = u16::from_le_bytes([raw[56], raw[57]]);
        if magic != EXT4_SUPER_MAGIC {
            return Err(-2);
        }

        Ok(Self {
            inodes_count: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            blocks_count_lo: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            first_data_block: u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]),
            log_block_size: u32::from_le_bytes([raw[24], raw[25], raw[26], raw[27]]),
            blocks_per_group: u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]),
            inodes_per_group: u32::from_le_bytes([raw[40], raw[41], raw[42], raw[43]]),
            magic,
            state: u16::from_le_bytes([raw[58], raw[59]]),
            rev_level: u32::from_le_bytes([raw[76], raw[77], raw[78], raw[79]]),
            first_ino: u32::from_le_bytes([raw[84], raw[85], raw[86], raw[87]]),
            inode_size: u16::from_le_bytes([raw[88], raw[89]]),
            mtime: u32::from_le_bytes([raw[44], raw[45], raw[46], raw[47]]),
            feature_compat: u32::from_le_bytes([raw[92], raw[93], raw[94], raw[95]]),
            feature_incompat: u32::from_le_bytes([raw[96], raw[97], raw[98], raw[99]]),
            feature_ro_compat: u32::from_le_bytes([raw[100], raw[101], raw[102], raw[103]]),
            journal_inum: u32::from_le_bytes([raw[224], raw[225], raw[226], raw[227]]),
            // ext4 64-bit block count at offset 336
            blocks_count_hi: if raw.len() > 340 {
                u32::from_le_bytes([raw[336], raw[337], raw[338], raw[339]])
            } else { 0 },
            log_groups_per_flex: if raw.len() > 372 { raw[372] } else { 0 },
            checksum_type: if raw.len() > 375 { raw[375] } else { 0 },
            desc_size: if raw.len() > 254 {
                u16::from_le_bytes([raw[254], raw[255]])
            } else { 32 },
            flags: if raw.len() > 352 {
                u32::from_le_bytes([raw[352], raw[353], raw[354], raw[355]])
            } else { 0 },
        })
    }

    pub fn block_size(&self) -> usize {
        1024 << self.log_block_size
    }

    pub fn blocks_count(&self) -> u64 {
        if self.has_64bit() {
            ((self.blocks_count_hi as u64) << 32) | (self.blocks_count_lo as u64)
        } else {
            self.blocks_count_lo as u64
        }
    }

    pub fn total_groups(&self) -> u32 {
        ((self.blocks_count() + self.blocks_per_group as u64 - 1) / self.blocks_per_group as u64) as u32
    }

    pub fn group_desc_size(&self) -> usize {
        if self.has_64bit() && self.desc_size >= 64 {
            self.desc_size as usize
        } else {
            32
        }
    }

    // Feature checks
    pub fn has_journal(&self) -> bool {
        (self.feature_compat & EXT4_FEATURE_COMPAT_HAS_JOURNAL) != 0
    }

    pub fn has_extents(&self) -> bool {
        (self.feature_incompat & EXT4_FEATURE_INCOMPAT_EXTENTS) != 0
    }

    pub fn has_64bit(&self) -> bool {
        (self.feature_incompat & EXT4_FEATURE_INCOMPAT_64BIT) != 0
    }

    pub fn has_flex_bg(&self) -> bool {
        (self.feature_incompat & EXT4_FEATURE_INCOMPAT_FLEX_BG) != 0
    }

    pub fn has_huge_file(&self) -> bool {
        (self.feature_ro_compat & EXT4_FEATURE_RO_COMPAT_HUGE_FILE) != 0
    }

    pub fn has_metadata_csum(&self) -> bool {
        (self.feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM) != 0
    }
}
