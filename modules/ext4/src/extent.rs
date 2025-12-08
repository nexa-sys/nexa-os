//! ext4 Extent Tree Implementation
//!
//! ext4 uses extents instead of indirect blocks for more efficient
//! storage of file block mappings.

/// Extent header - appears at start of extent tree node
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExtentHeader {
    pub magic: u16,      // 0xF30A
    pub entries: u16,    // Number of valid entries
    pub max: u16,        // Capacity of entries
    pub depth: u16,      // Depth of tree (0 = leaf)
    pub generation: u32, // Generation for checksumming
}

pub const EXT4_EXT_MAGIC: u16 = 0xF30A;

impl ExtentHeader {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 12 {
            return None;
        }
        
        let magic = u16::from_le_bytes([raw[0], raw[1]]);
        if magic != EXT4_EXT_MAGIC {
            return None;
        }
        
        Some(Self {
            magic,
            entries: u16::from_le_bytes([raw[2], raw[3]]),
            max: u16::from_le_bytes([raw[4], raw[5]]),
            depth: u16::from_le_bytes([raw[6], raw[7]]),
            generation: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
        })
    }

    pub fn is_leaf(&self) -> bool {
        self.depth == 0
    }
}

/// Extent index - internal node entry pointing to child block
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExtentIdx {
    pub block: u32,      // First logical block this index covers
    pub leaf_lo: u32,    // Lower 32 bits of child block
    pub leaf_hi: u16,    // Upper 16 bits of child block
    pub unused: u16,
}

impl ExtentIdx {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 12 {
            return None;
        }
        
        Some(Self {
            block: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            leaf_lo: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
            leaf_hi: u16::from_le_bytes([raw[8], raw[9]]),
            unused: u16::from_le_bytes([raw[10], raw[11]]),
        })
    }

    pub fn leaf_block(&self) -> u64 {
        ((self.leaf_hi as u64) << 32) | (self.leaf_lo as u64)
    }
}

/// Extent - leaf node entry mapping logical to physical blocks
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Extent {
    pub block: u32,      // First logical block
    pub len: u16,        // Number of blocks (up to 32768, high bit = uninitialized)
    pub start_hi: u16,   // Upper 16 bits of physical block
    pub start_lo: u32,   // Lower 32 bits of physical block
}

impl Extent {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 12 {
            return None;
        }
        
        Some(Self {
            block: u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            len: u16::from_le_bytes([raw[4], raw[5]]),
            start_hi: u16::from_le_bytes([raw[6], raw[7]]),
            start_lo: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
        })
    }

    pub fn start_block(&self) -> u64 {
        ((self.start_hi as u64) << 32) | (self.start_lo as u64)
    }

    pub fn length(&self) -> u32 {
        (self.len & 0x7FFF) as u32  // Mask out uninitialized bit
    }

    pub fn is_uninitialized(&self) -> bool {
        (self.len & 0x8000) != 0
    }

    /// Check if logical block falls within this extent
    pub fn contains(&self, logical_block: u32) -> bool {
        logical_block >= self.block && logical_block < self.block + self.length()
    }

    /// Get physical block for a logical block within this extent
    pub fn physical_block(&self, logical_block: u32) -> Option<u64> {
        if !self.contains(logical_block) {
            return None;
        }
        let offset = logical_block - self.block;
        Some(self.start_block() + offset as u64)
    }
}

/// Extent tree walker for finding block mappings
pub struct ExtentTree<'a> {
    inode_data: &'a [u8],  // i_block area from inode (60 bytes)
    block_size: usize,
}

impl<'a> ExtentTree<'a> {
    pub fn new(inode_block_data: &'a [u8], block_size: usize) -> Option<Self> {
        if inode_block_data.len() < 60 {
            return None;
        }
        
        // Verify extent header
        let header = ExtentHeader::parse(inode_block_data)?;
        if header.magic != EXT4_EXT_MAGIC {
            return None;
        }
        
        Some(Self {
            inode_data: inode_block_data,
            block_size,
        })
    }

    pub fn header(&self) -> Option<ExtentHeader> {
        ExtentHeader::parse(self.inode_data)
    }

    /// Find extent covering the given logical block (leaf level only)
    pub fn find_extent(&self, logical_block: u32) -> Option<Extent> {
        let header = self.header()?;
        
        if !header.is_leaf() {
            // Multi-level tree not fully supported yet
            // Would need to read intermediate blocks
            return None;
        }
        
        // Search leaf extents
        let entry_offset = 12; // After header
        for i in 0..header.entries as usize {
            let offset = entry_offset + i * 12;
            if offset + 12 > self.inode_data.len() {
                break;
            }
            
            let extent = Extent::parse(&self.inode_data[offset..])?;
            if extent.contains(logical_block) {
                return Some(extent);
            }
        }
        
        None
    }

    /// Get all extents at leaf level
    pub fn extents(&self) -> ExtentIter<'_> {
        ExtentIter {
            data: self.inode_data,
            index: 0,
            count: self.header().map(|h| h.entries).unwrap_or(0),
        }
    }
}

/// Iterator over extents
pub struct ExtentIter<'a> {
    data: &'a [u8],
    index: u16,
    count: u16,
}

impl<'a> Iterator for ExtentIter<'a> {
    type Item = Extent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        
        let offset = 12 + (self.index as usize) * 12;
        if offset + 12 > self.data.len() {
            return None;
        }
        
        self.index += 1;
        Extent::parse(&self.data[offset..])
    }
}
