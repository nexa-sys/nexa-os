//! ext4 JBD2 Journal support
//!
//! ext4 uses JBD2 (Journaling Block Device 2) for transaction management.

use crate::{mod_info, mod_warn};
use crate::ops::Ext4Filesystem;

pub const JBD2_MAGIC_NUMBER: u32 = 0xC03B3998;

/// Journal superblock (at start of journal)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct JournalSuperblock {
    pub magic: u32,
    pub blocktype: u32,
    pub sequence: u32,
    pub blocksize: u32,
    pub maxlen: u32,
    pub first: u32,
    pub sequence_first: u32,
    pub start: u32,
    pub errno: u32,
    // Features
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
}

impl JournalSuperblock {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 48 {
            return None;
        }

        let magic = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
        if magic != JBD2_MAGIC_NUMBER {
            return None;
        }

        Some(Self {
            magic,
            blocktype: u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]),
            sequence: u32::from_be_bytes([raw[8], raw[9], raw[10], raw[11]]),
            blocksize: u32::from_be_bytes([raw[12], raw[13], raw[14], raw[15]]),
            maxlen: u32::from_be_bytes([raw[16], raw[17], raw[18], raw[19]]),
            first: u32::from_be_bytes([raw[20], raw[21], raw[22], raw[23]]),
            sequence_first: u32::from_be_bytes([raw[24], raw[25], raw[26], raw[27]]),
            start: u32::from_be_bytes([raw[28], raw[29], raw[30], raw[31]]),
            errno: u32::from_be_bytes([raw[32], raw[33], raw[34], raw[35]]),
            feature_compat: u32::from_be_bytes([raw[36], raw[37], raw[38], raw[39]]),
            feature_incompat: u32::from_be_bytes([raw[40], raw[41], raw[42], raw[43]]),
            feature_ro_compat: u32::from_be_bytes([raw[44], raw[45], raw[46], raw[47]]),
        })
    }
}

/// Journal block types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum JournalBlockType {
    DescriptorBlock = 1,
    CommitBlock = 2,
    SuperblockV1 = 3,
    SuperblockV2 = 4,
    RevokeBlock = 5,
}

/// Journal state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
    Uninitialized,
    Clean,
    NeedsRecovery,
    Active,
}

/// Sync journal (flush pending transactions)
pub fn sync_journal(fs: &Ext4Filesystem) {
    if fs.journal_initialized {
        crate::mod_info!(b"ext4: journal sync");
        // TODO: Implement actual journal sync
    }
}

/// Initialize journal for filesystem
pub fn init_journal(fs: &mut Ext4Filesystem) -> Result<(), i32> {
    if !fs.has_journal {
        return Ok(());
    }

    // TODO: Read journal inode, parse journal superblock
    fs.journal_initialized = true;
    crate::mod_info!(b"ext4: journal initialized");
    Ok(())
}

/// Check if journal needs recovery
pub fn needs_recovery(_fs: &Ext4Filesystem) -> bool {
    // TODO: Check journal state
    false
}
