//! JIT Code Cache
//!
//! Manages compiled code blocks with:
//! - LRU eviction for memory pressure
//! - Invalidation on self-modifying code
//! - Fast lookup by guest RIP
//! - Tier promotion tracking
//! - Executable memory allocation via mmap

use std::collections::{HashMap, BTreeMap};
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::sync::RwLock;

/// Default size for executable memory pool (16 MB)
const DEFAULT_EXEC_POOL_SIZE: usize = 16 * 1024 * 1024;

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

/// Code cache for compiled blocks with dynamic expansion
pub struct CodeCache {
    /// RIP -> compiled block
    blocks: RwLock<HashMap<u64, Box<CompiledBlock>>>,
    
    /// Memory regions covered: start_addr -> end_addr
    /// Used for invalidation on self-modifying code
    regions: RwLock<BTreeMap<u64, u64>>,
    
    /// Total host code size
    total_size: AtomicU64,
    
    /// Current cache size limit (may grow)
    current_max_size: AtomicU64,
    
    /// Absolute maximum cache size (cannot exceed)
    hard_max_size: u64,
    
    /// Growth factor for expansion (e.g., 1.5 = 50% growth)
    growth_factor: f64,
    
    /// Current epoch for LRU
    epoch: AtomicU64,
    
    /// Statistics
    stats: CacheStats,
    
    /// Executable memory pools (dynamically allocated)
    exec_pools: RwLock<Vec<ExecutablePool>>,
    
    /// Initial pool size
    initial_pool_size: usize,
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
    pub expansions: AtomicU64,
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
            expansions: AtomicU64::new(0),
        }
    }
}

impl CodeCache {
    /// Create a new code cache with fixed size (legacy API)
    pub fn new(max_size: u64) -> Self {
        Self::new_dynamic(max_size, max_size, 1.0)
    }
    
    /// Create a new code cache with dynamic expansion support
    /// 
    /// # Arguments
    /// * `initial_size` - Initial cache size (first pool size)
    /// * `max_size` - Maximum cache size (hard limit)
    /// * `growth_factor` - Factor for pool size growth (e.g., 1.5)
    pub fn new_dynamic(initial_size: u64, max_size: u64, growth_factor: f64) -> Self {
        let initial_pool_size = initial_size.min(DEFAULT_EXEC_POOL_SIZE as u64) as usize;
        let mut pools = Vec::new();
        pools.push(ExecutablePool::new(initial_pool_size));
        
        log::info!("[JIT] CodeCache: initial={}MB, max={}MB, growth={}x",
            initial_size / (1024 * 1024),
            max_size / (1024 * 1024),
            growth_factor);
        
        Self {
            blocks: RwLock::new(HashMap::new()),
            regions: RwLock::new(BTreeMap::new()),
            total_size: AtomicU64::new(0),
            current_max_size: AtomicU64::new(initial_size),
            hard_max_size: max_size,
            growth_factor,
            epoch: AtomicU64::new(0),
            stats: CacheStats::default(),
            exec_pools: RwLock::new(pools),
            initial_pool_size,
        }
    }
    
    /// Allocate executable memory and copy code into it
    /// Dynamically expands the code cache if needed
    pub fn allocate_code(&self, code: &[u8]) -> Option<*const u8> {
        // Try existing pools first
        {
            let pools = self.exec_pools.read().unwrap();
            for pool in pools.iter() {
                if let Some(ptr) = pool.allocate(code) {
                    return Some(ptr);
                }
            }
        }
        
        // Need to expand - try to allocate a new pool
        self.try_expand_cache(code.len())?;
        
        // Retry allocation
        let pools = self.exec_pools.read().unwrap();
        pools.last()?.allocate(code)
    }
    
    /// Try to expand the code cache by adding a new executable memory pool
    fn try_expand_cache(&self, min_needed: usize) -> Option<()> {
        let current = self.current_max_size.load(Ordering::Relaxed);
        
        // Calculate new pool size
        let new_pool_size = ((self.initial_pool_size as f64 * self.growth_factor) as usize)
            .max(min_needed * 2)
            .max(4 * 1024 * 1024); // Minimum 4MB
        
        let new_total = current + new_pool_size as u64;
        
        if new_total > self.hard_max_size {
            log::warn!("[JIT] CodeCache: cannot expand beyond hard limit {}MB", 
                self.hard_max_size / (1024 * 1024));
            return None;
        }
        
        // Allocate new pool
        let mut pools = self.exec_pools.write().unwrap();
        let new_pool = ExecutablePool::new(new_pool_size);
        pools.push(new_pool);
        
        self.current_max_size.store(new_total, Ordering::Relaxed);
        self.stats.expansions.fetch_add(1, Ordering::Relaxed);
        
        log::info!("[JIT] CodeCache: expanded to {}MB (pools: {})",
            new_total / (1024 * 1024),
            pools.len());
        
        Some(())
    }
    
    /// Get current cache capacity
    pub fn capacity(&self) -> u64 {
        self.current_max_size.load(Ordering::Relaxed)
    }
    
    /// Get expansion count
    pub fn expansion_count(&self) -> u64 {
        self.stats.expansions.load(Ordering::Relaxed)
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
        
        // Check if we need to evict (use current dynamic limit)
        let current_size = self.total_size.load(Ordering::Relaxed);
        let current_max = self.current_max_size.load(Ordering::Relaxed);
        if current_size + host_size > current_max {
            // Try to expand first before evicting
            if self.try_expand_cache(host_size as usize).is_none() {
                // Cannot expand, must evict
                self.evict_lru(host_size)?;
            }
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
        
        // Allocate executable memory and copy code
        let code_len = code.len();
        let host_ptr = self.allocate_code(&code)
            .ok_or(CacheError::OutOfMemory)?;
        
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
    
    /// Get all compiled blocks for persistence
    /// 
    /// Returns a vector of (guest_rip, BlockPersistInfo) for each block in the cache.
    /// The native code is copied out of executable memory for safe serialization.
    pub fn get_all_blocks_for_persist(&self) -> Vec<(u64, BlockPersistInfo)> {
        let blocks = self.blocks.read().unwrap();
        blocks.iter()
            .filter(|(_, b)| !b.invalidated && !b.host_code.is_null())
            .map(|(&rip, b)| {
                // Copy native code out of executable memory
                let native_code = if !b.host_code.is_null() && b.host_size > 0 {
                    unsafe {
                        std::slice::from_raw_parts(b.host_code, b.host_size as usize).to_vec()
                    }
                } else {
                    Vec::new()
                };
                
                (rip, BlockPersistInfo {
                    guest_rip: b.guest_rip,
                    guest_size: b.guest_size,
                    host_size: b.host_size,
                    tier: b.tier,
                    exec_count: b.exec_count.load(Ordering::Relaxed),
                    guest_instrs: b.guest_instrs,
                    guest_checksum: b.guest_checksum,
                    native_code,
                })
            })
            .collect()
    }
    
    /// Get the number of compiled blocks
    pub fn block_count(&self) -> usize {
        self.blocks.read().unwrap().len()
    }
}

/// Block info for persistence (with native code copy)
#[derive(Clone, Debug)]
pub struct BlockPersistInfo {
    pub guest_rip: u64,
    pub guest_size: u32,
    pub host_size: u32,
    pub tier: CompileTier,
    pub exec_count: u64,
    pub guest_instrs: u32,
    pub guest_checksum: u64,
    pub native_code: Vec<u8>,
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

/// Executable memory pool using mmap
/// 
/// Allocates memory with read-write-execute permissions for JIT code.
pub struct ExecutablePool {
    /// Base address of the pool
    base: *mut u8,
    /// Total pool size
    size: usize,
    /// Current allocation offset
    offset: AtomicU32,
}

impl ExecutablePool {
    /// Create a new executable memory pool
    pub fn new(size: usize) -> Self {
        let base = unsafe {
            #[cfg(unix)]
            {
                use std::ptr;
                let addr = libc::mmap(
                    ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                );
                if addr == libc::MAP_FAILED {
                    panic!("Failed to allocate executable memory");
                }
                addr as *mut u8
            }
            #[cfg(windows)]
            {
                use std::ptr;
                use winapi::um::memoryapi::VirtualAlloc;
                use winapi::um::winnt::{MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};
                let addr = VirtualAlloc(
                    ptr::null_mut(),
                    size,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                );
                if addr.is_null() {
                    panic!("Failed to allocate executable memory");
                }
                addr as *mut u8
            }
        };
        
        Self {
            base,
            size,
            offset: AtomicU32::new(0),
        }
    }
    
    /// Allocate space for code and copy it
    /// Returns pointer to executable code
    pub fn allocate(&self, code: &[u8]) -> Option<*const u8> {
        let aligned_size = (code.len() + 15) & !15; // 16-byte align
        
        loop {
            let current = self.offset.load(Ordering::Relaxed);
            let new_offset = current + aligned_size as u32;
            
            if new_offset as usize > self.size {
                return None; // Out of memory
            }
            
            if self.offset.compare_exchange(
                current,
                new_offset,
                Ordering::SeqCst,
                Ordering::Relaxed
            ).is_ok() {
                let ptr = unsafe { self.base.add(current as usize) };
                // Copy code to executable memory
                unsafe {
                    std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, code.len());
                }
                return Some(ptr as *const u8);
            }
        }
    }
    
    /// Reset the pool (invalidates all code!)
    pub fn reset(&self) {
        self.offset.store(0, Ordering::Relaxed);
    }
    
    /// Get used space
    pub fn used(&self) -> usize {
        self.offset.load(Ordering::Relaxed) as usize
    }
    
    /// Get available space
    pub fn available(&self) -> usize {
        self.size - self.used()
    }
}

impl Drop for ExecutablePool {
    fn drop(&mut self) {
        unsafe {
            #[cfg(unix)]
            {
                libc::munmap(self.base as *mut libc::c_void, self.size);
            }
            #[cfg(windows)]
            {
                use winapi::um::memoryapi::VirtualFree;
                use winapi::um::winnt::MEM_RELEASE;
                VirtualFree(self.base as *mut _, 0, MEM_RELEASE);
            }
        }
    }
}

// Safety: ExecutablePool uses atomic operations for thread-safe allocation
unsafe impl Send for ExecutablePool {}
unsafe impl Sync for ExecutablePool {}

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
