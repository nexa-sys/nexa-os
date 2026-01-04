//! JIT Code Cache
//!
//! Manages compiled code blocks with:
//! - LRU eviction for memory pressure
//! - Invalidation on self-modifying code
//! - Fast lookup by guest RIP
//! - Tier promotion tracking

use std::collections::{HashMap, BTreeMap};
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::sync::RwLock;

/// Compilation tier
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompileTier {
    /// Interpreted - no compiled code
    Interpreter,
    /// S1 - quick baseline compilation
    S1,
    /// S2 - optimizing compilation
    S2,
}

/// Compiled code block metadata
pub struct CompiledBlock {
    /// Guest start RIP
    pub guest_rip: u64,
    /// Guest code size (bytes)
    pub guest_size: u32,
    /// Host code pointer
    pub host_code: *const u8,
    /// Host code size
    pub host_size: u32,
    /// Compilation tier
    pub tier: CompileTier,
    /// Execution count
    pub exec_count: AtomicU64,
    /// Last access time (epoch)
    pub last_access: AtomicU64,
    /// Number of guest instructions
    pub guest_instrs: u32,
    /// Checksum of guest code for invalidation detection
    pub guest_checksum: u64,
    /// Dependencies: blocks that must be invalidated if this is
    pub depends_on: Vec<u64>,
    /// Whether this block has been invalidated
    pub invalidated: bool,
}

// SAFETY: CompiledBlock's host_code pointer points to executable memory that is
// only written during compilation and read-only during execution. The memory
// is managed by CodeCache and lives as long as the CompiledBlock.
unsafe impl Send for CompiledBlock {}
unsafe impl Sync for CompiledBlock {}

impl CompiledBlock {
    pub fn record_execution(&self) {
        self.exec_count.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn touch(&self, epoch: u64) {
        self.last_access.store(epoch, Ordering::Relaxed);
    }
    
    pub fn is_hot(&self, threshold: u64) -> bool {
        self.exec_count.load(Ordering::Relaxed) >= threshold
    }
}

/// Code cache for compiled blocks
pub struct CodeCache {
    /// RIP -> compiled block
    blocks: RwLock<HashMap<u64, Box<CompiledBlock>>>,
    
    /// Memory regions covered: start_addr -> end_addr
    /// Used for invalidation on self-modifying code
    regions: RwLock<BTreeMap<u64, u64>>,
    
    /// Total host code size
    total_size: AtomicU64,
    
    /// Maximum cache size (bytes)
    max_size: u64,
    
    /// Current epoch for LRU
    epoch: AtomicU64,
    
    /// Statistics
    stats: CacheStats,
}

/// Cache statistics
pub struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
    pub invalidations: AtomicU64,
    pub s1_compiles: AtomicU64,
    pub s2_compiles: AtomicU64,
    pub tier_promotions: AtomicU64,
}

impl Default for CacheStats {
    fn default() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            invalidations: AtomicU64::new(0),
            s1_compiles: AtomicU64::new(0),
            s2_compiles: AtomicU64::new(0),
            tier_promotions: AtomicU64::new(0),
        }
    }
}

impl CodeCache {
    pub fn new(max_size: u64) -> Self {
        Self {
            blocks: RwLock::new(HashMap::new()),
            regions: RwLock::new(BTreeMap::new()),
            total_size: AtomicU64::new(0),
            max_size,
            epoch: AtomicU64::new(0),
            stats: CacheStats::default(),
        }
    }
    
    /// Look up a compiled block by guest RIP
    pub fn lookup(&self, rip: u64) -> Option<*const u8> {
        let blocks = self.blocks.read().unwrap();
        if let Some(block) = blocks.get(&rip) {
            if block.invalidated {
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            let epoch = self.epoch.fetch_add(1, Ordering::Relaxed);
            block.touch(epoch);
            block.record_execution();
            return Some(block.host_code);
        }
        
        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        None
    }
    
    /// Get block metadata
    pub fn get_block(&self, rip: u64) -> Option<BlockInfo> {
        let blocks = self.blocks.read().unwrap();
        blocks.get(&rip).map(|b| BlockInfo {
            guest_rip: b.guest_rip,
            guest_size: b.guest_size,
            host_size: b.host_size,
            tier: b.tier,
            exec_count: b.exec_count.load(Ordering::Relaxed),
            invalidated: b.invalidated,
        })
    }
    
    /// Insert a compiled block
    pub fn insert(&self, block: CompiledBlock) -> Result<(), CacheError> {
        let host_size = block.host_size as u64;
        let rip = block.guest_rip;
        let guest_start = block.guest_rip;
        let guest_end = guest_start + block.guest_size as u64;
        
        // Check if we need to evict
        let current_size = self.total_size.load(Ordering::Relaxed);
        if current_size + host_size > self.max_size {
            self.evict_lru(host_size)?;
        }
        
        // Track region
        {
            let mut regions = self.regions.write().unwrap();
            regions.insert(guest_start, guest_end);
        }
        
        // Insert block
        let tier = block.tier;
        {
            let mut blocks = self.blocks.write().unwrap();
            
            // If replacing, subtract old size
            if let Some(old) = blocks.get(&rip) {
                self.total_size.fetch_sub(old.host_size as u64, Ordering::Relaxed);
                if old.tier < tier {
                    self.stats.tier_promotions.fetch_add(1, Ordering::Relaxed);
                }
            }
            
            blocks.insert(rip, Box::new(block));
        }
        
        self.total_size.fetch_add(host_size, Ordering::Relaxed);
        
        match tier {
            CompileTier::S1 => self.stats.s1_compiles.fetch_add(1, Ordering::Relaxed),
            CompileTier::S2 => self.stats.s2_compiles.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
        
        Ok(())
    }
    
    /// Replace an existing block with new code
    pub fn replace(&self, rip: u64, code: Vec<u8>) -> Result<(), CacheError> {
        // First invalidate existing block
        self.invalidate(rip);
        
        // Allocate and copy code
        let code_len = code.len();
        let code_box: Box<[u8]> = code.into_boxed_slice();
        let host_ptr = Box::into_raw(code_box) as *const u8;
        
        // Create new compiled block
        let block = CompiledBlock {
            guest_rip: rip,
            guest_size: 0, // Unknown for replaced blocks
            host_code: host_ptr,
            host_size: code_len as u32,
            tier: CompileTier::S2,
            exec_count: AtomicU64::new(0),
            last_access: AtomicU64::new(0),
            guest_instrs: 0,
            guest_checksum: 0,
            depends_on: Vec::new(),
            invalidated: false,
        };
        self.insert(block)
    }
    
    /// Invalidate blocks in a range (alias for invalidate_region)
    pub fn invalidate_range(&self, start: u64, end: u64) -> usize {
        self.invalidate_region(start, end)
    }
    
    /// Invalidate blocks overlapping with a memory write
    pub fn invalidate_region(&self, start: u64, end: u64) -> usize {
        let mut invalidated = Vec::new();
        
        // Find overlapping regions
        {
            let regions = self.regions.read().unwrap();
            for (&region_start, &region_end) in regions.iter() {
                if region_start < end && region_end > start {
                    invalidated.push(region_start);
                }
            }
        }
        
        // Invalidate blocks
        let count = invalidated.len();
        if count > 0 {
            let mut blocks = self.blocks.write().unwrap();
            for rip in invalidated {
                if let Some(block) = blocks.get_mut(&rip) {
                    block.invalidated = true;
                }
            }
            self.stats.invalidations.fetch_add(count as u64, Ordering::Relaxed);
        }
        
        count
    }
    
    /// Invalidate a specific block
    pub fn invalidate(&self, rip: u64) -> bool {
        let mut blocks = self.blocks.write().unwrap();
        if let Some(block) = blocks.get_mut(&rip) {
            if !block.invalidated {
                block.invalidated = true;
                self.stats.invalidations.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    }
    
    /// Evict LRU blocks to free space
    fn evict_lru(&self, needed: u64) -> Result<(), CacheError> {
        let mut freed = 0u64;
        let mut to_remove = Vec::new();
        
        // Collect candidates sorted by last access
        let blocks = self.blocks.read().unwrap();
        let mut candidates: Vec<_> = blocks.iter()
            .map(|(&rip, b)| (rip, b.last_access.load(Ordering::Relaxed), b.host_size))
            .collect();
        drop(blocks);
        
        // Sort by last access (oldest first)
        candidates.sort_by_key(|&(_, access, _)| access);
        
        // Select blocks to evict
        for (rip, _, size) in candidates {
            to_remove.push(rip);
            freed += size as u64;
            if freed >= needed {
                break;
            }
        }
        
        if freed < needed {
            return Err(CacheError::OutOfMemory);
        }
        
        // Remove selected blocks
        let mut blocks = self.blocks.write().unwrap();
        let mut regions = self.regions.write().unwrap();
        
        for rip in &to_remove {
            if let Some(block) = blocks.remove(rip) {
                regions.remove(&block.guest_rip);
                self.total_size.fetch_sub(block.host_size as u64, Ordering::Relaxed);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
        
        Ok(())
    }
    
    /// Check if a block should be promoted to S2
    pub fn should_promote(&self, rip: u64, s2_threshold: u64) -> bool {
        let blocks = self.blocks.read().unwrap();
        if let Some(block) = blocks.get(&rip) {
            return block.tier == CompileTier::S1 
                && block.exec_count.load(Ordering::Relaxed) >= s2_threshold
                && !block.invalidated;
        }
        false
    }
    
    /// Get blocks ready for S2 promotion
    pub fn get_promotion_candidates(&self, s2_threshold: u64, max: usize) -> Vec<u64> {
        let blocks = self.blocks.read().unwrap();
        let mut candidates: Vec<_> = blocks.iter()
            .filter(|(_, b)| {
                b.tier == CompileTier::S1 
                && !b.invalidated
                && b.exec_count.load(Ordering::Relaxed) >= s2_threshold
            })
            .map(|(&rip, b)| (rip, b.exec_count.load(Ordering::Relaxed)))
            .collect();
        
        // Sort by execution count (hottest first)
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.truncate(max);
        candidates.into_iter().map(|(rip, _)| rip).collect()
    }
    
    /// Clear all compiled code
    pub fn clear(&self) {
        let mut blocks = self.blocks.write().unwrap();
        let mut regions = self.regions.write().unwrap();
        
        blocks.clear();
        regions.clear();
        self.total_size.store(0, Ordering::Relaxed);
    }
    
    /// Get cache statistics
    pub fn get_stats(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
            invalidations: self.stats.invalidations.load(Ordering::Relaxed),
            s1_compiles: self.stats.s1_compiles.load(Ordering::Relaxed),
            s2_compiles: self.stats.s2_compiles.load(Ordering::Relaxed),
            tier_promotions: self.stats.tier_promotions.load(Ordering::Relaxed),
            total_size: self.total_size.load(Ordering::Relaxed),
            block_count: self.blocks.read().unwrap().len(),
        }
    }
    
    pub fn hit_rate(&self) -> f64 {
        let hits = self.stats.hits.load(Ordering::Relaxed) as f64;
        let misses = self.stats.misses.load(Ordering::Relaxed) as f64;
        let total = hits + misses;
        if total > 0.0 { hits / total } else { 0.0 }
    }
}

/// Block info (without raw pointer)
#[derive(Clone, Debug)]
pub struct BlockInfo {
    pub guest_rip: u64,
    pub guest_size: u32,
    pub host_size: u32,
    pub tier: CompileTier,
    pub exec_count: u64,
    pub invalidated: bool,
}

/// Cache statistics snapshot
#[derive(Clone, Debug)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub invalidations: u64,
    pub s1_compiles: u64,
    pub s2_compiles: u64,
    pub tier_promotions: u64,
    pub total_size: u64,
    pub block_count: usize,
}

impl CacheStatsSnapshot {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total > 0 { self.hits as f64 / total as f64 } else { 0.0 }
    }
}

/// Cache errors
#[derive(Debug, Clone)]
pub enum CacheError {
    OutOfMemory,
    InvalidBlock,
    CompilationFailed,
}

/// Code region for executable memory
pub struct CodeRegion {
    /// Base address
    base: *mut u8,
    /// Total size
    size: usize,
    /// Current allocation offset
    offset: AtomicU32,
}

impl CodeRegion {
    /// Create a new executable code region
    /// 
    /// # Safety
    /// Caller must ensure base points to valid executable memory
    pub unsafe fn new(base: *mut u8, size: usize) -> Self {
        Self {
            base,
            size,
            offset: AtomicU32::new(0),
        }
    }
    
    /// Allocate space for code
    pub fn allocate(&self, size: usize) -> Option<*mut u8> {
        let aligned_size = (size + 15) & !15; // 16-byte align
        
        loop {
            let current = self.offset.load(Ordering::Relaxed);
            let new_offset = current + aligned_size as u32;
            
            if new_offset as usize > self.size {
                return None;
            }
            
            if self.offset.compare_exchange(
                current,
                new_offset,
                Ordering::SeqCst,
                Ordering::Relaxed
            ).is_ok() {
                return Some(unsafe { self.base.add(current as usize) });
            }
        }
    }
    
    /// Reset allocation (invalidates all code!)
    pub fn reset(&self) {
        self.offset.store(0, Ordering::Relaxed);
    }
    
    pub fn used(&self) -> usize {
        self.offset.load(Ordering::Relaxed) as usize
    }
    
    pub fn available(&self) -> usize {
        self.size - self.used()
    }
}

/// Checksum for guest code integrity
pub fn compute_checksum(code: &[u8]) -> u64 {
    // Simple FNV-1a hash
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in code {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;
    
    #[test]
    fn test_cache_insert_lookup() {
        let cache = CodeCache::new(1024 * 1024);
        
        let block = CompiledBlock {
            guest_rip: 0x1000,
            guest_size: 16,
            host_code: ptr::null(),
            host_size: 32,
            tier: CompileTier::S1,
            exec_count: AtomicU64::new(0),
            last_access: AtomicU64::new(0),
            guest_instrs: 4,
            guest_checksum: 0x12345678,
            depends_on: Vec::new(),
            invalidated: false,
        };
        
        cache.insert(block).unwrap();
        
        let info = cache.get_block(0x1000).unwrap();
        assert_eq!(info.tier, CompileTier::S1);
        assert_eq!(info.guest_size, 16);
    }
    
    #[test]
    fn test_cache_invalidation() {
        let cache = CodeCache::new(1024 * 1024);
        
        let block = CompiledBlock {
            guest_rip: 0x1000,
            guest_size: 16,
            host_code: ptr::null(),
            host_size: 32,
            tier: CompileTier::S1,
            exec_count: AtomicU64::new(0),
            last_access: AtomicU64::new(0),
            guest_instrs: 4,
            guest_checksum: 0,
            depends_on: Vec::new(),
            invalidated: false,
        };
        
        cache.insert(block).unwrap();
        
        // Invalidate overlapping region
        let count = cache.invalidate_region(0x1008, 0x1010);
        assert_eq!(count, 1);
        
        let info = cache.get_block(0x1000).unwrap();
        assert!(info.invalidated);
    }
    
    #[test]
    fn test_checksum() {
        let code1 = [0x48, 0x89, 0xc0];
        let code2 = [0x48, 0x89, 0xc1];
        
        let h1 = compute_checksum(&code1);
        let h2 = compute_checksum(&code2);
        
        assert_ne!(h1, h2);
        assert_eq!(h1, compute_checksum(&code1)); // Deterministic
    }
}
