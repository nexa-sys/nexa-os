//! Smart Code Cache Eviction
//!
//! Intelligent eviction policy that:
//! 1. Tracks hotness over time with decay
//! 2. Maintains call graph for locality-aware eviction
//! 3. Tiers eviction: S2→disk (preserve), S1→optional disk (semi-preserve)
//! 4. Restores cold-turned-hot blocks from disk
//!
//! ## NVM vs ZingJVM/ReadyNow! Eviction Strategies
//!
//! NVM's eviction differs fundamentally from ZingJVM's ReadyNow!:
//!
//! | Aspect | NVM NReady! | ZingJVM ReadyNow! |
//! |--------|-------------|-------------------|
//! | **Hotness Metric** | Time-decayed score with half-life (60s) | Static execution count |
//! | **Locality** | Call graph tracking (1.5x bonus for hot neighbors) | No locality awareness |
//! | **Eviction Priority** | Score-based with tier preference (S1 before S2) | LRU-style |
//! | **S2 Preservation** | Always saved to disk with optimization metadata | Native-only preservation |
//! | **Restoration** | Includes escape analysis + loop opt results | Recompile from scratch |
//! | **Memory Pressure** | Dynamic cache expansion before eviction | Fixed capacity |
//!
//! ### Key Differences Explained
//!
//! 1. **Hotness Decay**: NVM uses `Score = exec_count * 2^(-time/half_life) * locality_bonus`.
//!    A block executed 1000 times 2 minutes ago has lower score than one executed
//!    100 times 30 seconds ago. ZingJVM uses raw execution counts.
//!
//! 2. **Locality Bonus**: NVM tracks call edges. If a block's callers/callees are hot,
//!    it gets a 1.5x score boost. This keeps hot call chains in cache together.
//!
//! 3. **Optimization Metadata Preservation**: When NVM evicts an S2 block, it saves
//!    escape analysis and loop optimization results. On restoration, these are
//!    reused to skip expensive re-analysis. ZingJVM must reanalyze from scratch.
//!
//! 4. **Dynamic Expansion**: NVM tries to expand the CodeCache before evicting.
//!    Only when the hard limit is reached does eviction occur.
//!
//! ## Eviction Strategy
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                      Smart Eviction Pipeline                                 │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  CodeCache Full → HotnessTracker.select_victims() → Tiered Eviction        │
//! │                                                                             │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │                    Hotness-Based Selection                          │   │
//! │  │                                                                     │   │
//! │  │  Score = exec_count * recency_weight * locality_bonus              │   │
//! │  │                                                                     │   │
//! │  │  recency_weight = 2^(-time_since_last_exec / half_life)            │   │
//! │  │  locality_bonus = 1.5 if callers/callees are hot                   │   │
//! │  └─────────────────────────────────────────────────────────────────────┘   │
//! │                                                                             │
//! │  ┌───────────────┐    ┌───────────────┐    ┌───────────────┐              │
//! │  │    S2 Blocks  │───▶│  Evict to     │───▶│  NReady!      │              │
//! │  │   (valuable)  │    │  Disk First   │    │  Persistence  │              │
//! │  └───────────────┘    └───────────────┘    └───────────────┘              │
//! │                                                                             │
//! │  ┌───────────────┐    ┌───────────────┐    ┌───────────────┐              │
//! │  │    S1 Blocks  │───▶│ Try Disk,     │───▶│ Or Discard    │              │
//! │  │ (replaceable) │    │ Else Discard  │    │ (Recompile OK)│              │
//! │  └───────────────┘    └───────────────┘    └───────────────┘              │
//! │                                                                             │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │                    Restoration on Re-heat                           │   │
//! │  │                                                                     │   │
//! │  │  Cache Miss + Block in EvictedIndex → restore_from_disk()          │   │
//! │  │  Faster than recompile, preserves profile data + opt metadata      │   │
//! │  └─────────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::{HashMap, HashSet, BTreeMap};
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::sync::RwLock;
use std::time::Instant;

use super::cache::{CompileTier, BlockPersistInfo};

// ============================================================================
// Configuration
// ============================================================================

/// Half-life for hotness decay (in seconds)
/// After this time, a block's effective hotness is halved
const HOTNESS_HALF_LIFE_SECS: f64 = 60.0;

/// Locality bonus multiplier for blocks with hot callers/callees
const LOCALITY_BONUS: f64 = 1.5;

/// Minimum score threshold to consider preserving to disk
const MIN_PRESERVE_SCORE: f64 = 100.0;

/// Maximum evicted blocks to keep index of (for restoration)
const MAX_EVICTED_INDEX: usize = 10_000;

/// Batch size for eviction operations
const EVICTION_BATCH_SIZE: usize = 64;

// ============================================================================
// Hotness Entry
// ============================================================================

/// Hotness tracking entry for a single block
#[derive(Debug)]
pub struct HotnessEntry {
    /// Guest RIP
    pub rip: u64,
    /// Raw execution count (monotonically increasing)
    pub exec_count: AtomicU64,
    /// Last execution timestamp (epoch micros since tracker start)
    pub last_exec_time: AtomicU64,
    /// Compilation tier
    pub tier: CompileTier,
    /// Set of callers (RIPs that call this block)
    pub callers: RwLock<HashSet<u64>>,
    /// Set of callees (RIPs that this block calls)
    pub callees: RwLock<HashSet<u64>>,
    /// Whether this block was evicted to disk
    pub evicted: std::sync::atomic::AtomicBool,
}

impl HotnessEntry {
    pub fn new(rip: u64, tier: CompileTier) -> Self {
        Self {
            rip,
            exec_count: AtomicU64::new(0),
            last_exec_time: AtomicU64::new(0),
            tier,
            callers: RwLock::new(HashSet::new()),
            callees: RwLock::new(HashSet::new()),
            evicted: std::sync::atomic::AtomicBool::new(false),
        }
    }
    
    /// Record an execution
    pub fn record_exec(&self, timestamp: u64) {
        self.exec_count.fetch_add(1, Ordering::Relaxed);
        self.last_exec_time.store(timestamp, Ordering::Relaxed);
    }
    
    /// Record a call edge (this block calls target)
    pub fn add_callee(&self, target: u64) {
        let mut callees = self.callees.write().unwrap();
        callees.insert(target);
    }
    
    /// Record that caller calls this block
    pub fn add_caller(&self, caller: u64) {
        let mut callers = self.callers.write().unwrap();
        callers.insert(caller);
    }
    
    /// Calculate effective hotness score
    pub fn hotness_score(&self, current_time: u64, hot_neighbors: &HashSet<u64>) -> f64 {
        let exec = self.exec_count.load(Ordering::Relaxed) as f64;
        let last = self.last_exec_time.load(Ordering::Relaxed);
        
        // Time-based decay
        let elapsed_secs = if current_time > last {
            (current_time - last) as f64 / 1_000_000.0 // micros to secs
        } else {
            0.0
        };
        let recency_weight = 2.0_f64.powf(-elapsed_secs / HOTNESS_HALF_LIFE_SECS);
        
        // Locality bonus: if neighbors are hot, this block is more valuable
        let has_hot_neighbor = {
            let callers = self.callers.read().unwrap();
            let callees = self.callees.read().unwrap();
            callers.iter().any(|c| hot_neighbors.contains(c)) ||
            callees.iter().any(|c| hot_neighbors.contains(c))
        };
        let locality = if has_hot_neighbor { LOCALITY_BONUS } else { 1.0 };
        
        exec * recency_weight * locality
    }
}

// ============================================================================
// Evicted Block Index
// ============================================================================

/// Information about an evicted block (for restoration)
#[derive(Debug, Clone)]
pub struct EvictedBlockInfo {
    /// Guest RIP
    pub rip: u64,
    /// Tier when evicted
    pub tier: CompileTier,
    /// Execution count at eviction time
    pub exec_count: u64,
    /// Guest code checksum (to verify integrity)
    pub guest_checksum: u64,
    /// Timestamp when evicted
    pub evicted_at: u64,
    /// Path to persisted file (within NReady! cache)
    pub persist_path: String,
    /// Whether native code was saved (vs just profile)
    pub has_native: bool,
    /// Whether IR was saved
    pub has_ir: bool,
}

/// Index of blocks that were evicted to disk
pub struct EvictedIndex {
    /// RIP -> eviction info
    entries: RwLock<HashMap<u64, EvictedBlockInfo>>,
    /// Ordered by eviction time for LRU cleanup
    eviction_order: RwLock<Vec<u64>>,
    /// Maximum entries to track
    max_entries: usize,
}

impl EvictedIndex {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            eviction_order: RwLock::new(Vec::new()),
            max_entries,
        }
    }
    
    /// Record that a block was evicted
    pub fn record_eviction(&self, info: EvictedBlockInfo) {
        let rip = info.rip;
        
        {
            let mut entries = self.entries.write().unwrap();
            let mut order = self.eviction_order.write().unwrap();
            
            // Remove oldest if at capacity
            while entries.len() >= self.max_entries && !order.is_empty() {
                let oldest = order.remove(0);
                entries.remove(&oldest);
            }
            
            entries.insert(rip, info);
            order.push(rip);
        }
    }
    
    /// Check if a block was evicted and can be restored
    pub fn get_evicted(&self, rip: u64) -> Option<EvictedBlockInfo> {
        let entries = self.entries.read().unwrap();
        entries.get(&rip).cloned()
    }
    
    /// Remove from index (after restoration)
    pub fn mark_restored(&self, rip: u64) {
        let mut entries = self.entries.write().unwrap();
        let mut order = self.eviction_order.write().unwrap();
        
        entries.remove(&rip);
        order.retain(|&r| r != rip);
    }
    
    /// Get count of evicted blocks
    pub fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ============================================================================
// Hotness Tracker
// ============================================================================

/// Tracks code block hotness for intelligent eviction
pub struct HotnessTracker {
    /// Block hotness entries
    entries: RwLock<HashMap<u64, HotnessEntry>>,
    /// Start time for relative timestamps
    start_time: Instant,
    /// Index of evicted blocks
    pub evicted_index: EvictedIndex,
    /// Statistics
    pub stats: EvictionStats,
}

/// Eviction statistics
pub struct EvictionStats {
    pub s2_evicted_to_disk: AtomicU64,
    pub s1_evicted_to_disk: AtomicU64,
    pub s1_discarded: AtomicU64,
    pub restorations: AtomicU64,
    pub restoration_hits: AtomicU64,  // Restored block executed again
}

impl Default for EvictionStats {
    fn default() -> Self {
        Self {
            s2_evicted_to_disk: AtomicU64::new(0),
            s1_evicted_to_disk: AtomicU64::new(0),
            s1_discarded: AtomicU64::new(0),
            restorations: AtomicU64::new(0),
            restoration_hits: AtomicU64::new(0),
        }
    }
}

impl HotnessTracker {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            start_time: Instant::now(),
            evicted_index: EvictedIndex::new(MAX_EVICTED_INDEX),
            stats: EvictionStats::default(),
        }
    }
    
    /// Get current timestamp in microseconds since tracker start
    fn current_timestamp(&self) -> u64 {
        self.start_time.elapsed().as_micros() as u64
    }
    
    /// Register a new block
    pub fn register_block(&self, rip: u64, tier: CompileTier) {
        let mut entries = self.entries.write().unwrap();
        entries.entry(rip).or_insert_with(|| HotnessEntry::new(rip, tier));
    }
    
    /// Update tier for a block (e.g., after S1→S2 promotion)
    pub fn update_tier(&self, rip: u64, tier: CompileTier) {
        let entries = self.entries.read().unwrap();
        if let Some(entry) = entries.get(&rip) {
            // We can't mutate tier directly since it's not atomic, but we store it
            // in a way that allows the eviction logic to query current tier
            drop(entries);
            let mut entries = self.entries.write().unwrap();
            if let Some(entry) = entries.get_mut(&rip) {
                // Create new entry with updated tier, preserving stats
                let new_entry = HotnessEntry {
                    rip,
                    exec_count: AtomicU64::new(entry.exec_count.load(Ordering::Relaxed)),
                    last_exec_time: AtomicU64::new(entry.last_exec_time.load(Ordering::Relaxed)),
                    tier,
                    callers: RwLock::new(entry.callers.read().unwrap().clone()),
                    callees: RwLock::new(entry.callees.read().unwrap().clone()),
                    evicted: std::sync::atomic::AtomicBool::new(false),
                };
                *entry = new_entry;
            }
        }
    }
    
    /// Record block execution
    pub fn record_execution(&self, rip: u64) {
        let timestamp = self.current_timestamp();
        let entries = self.entries.read().unwrap();
        if let Some(entry) = entries.get(&rip) {
            entry.record_exec(timestamp);
        }
    }
    
    /// Record a call edge
    pub fn record_call(&self, caller_rip: u64, callee_rip: u64) {
        let entries = self.entries.read().unwrap();
        if let Some(caller) = entries.get(&caller_rip) {
            caller.add_callee(callee_rip);
        }
        if let Some(callee) = entries.get(&callee_rip) {
            callee.add_caller(caller_rip);
        }
    }
    
    /// Select victims for eviction
    /// 
    /// Returns blocks ordered by priority:
    /// - First: coldest S1 blocks (can be discarded)
    /// - Then: coldest S2 blocks (should be preserved to disk)
    pub fn select_victims(&self, count: usize, exclude: &HashSet<u64>) -> Vec<EvictionCandidate> {
        let current_time = self.current_timestamp();
        let entries = self.entries.read().unwrap();
        
        // First pass: identify hot blocks for locality calculation
        let hot_threshold = self.compute_hot_threshold(&entries, current_time);
        let hot_blocks: HashSet<u64> = entries.iter()
            .filter(|(_, e)| e.hotness_score(current_time, &HashSet::new()) >= hot_threshold)
            .map(|(&rip, _)| rip)
            .collect();
        
        // Compute scores for all blocks
        let mut candidates: Vec<EvictionCandidate> = entries.iter()
            .filter(|(&rip, e)| !exclude.contains(&rip) && !e.evicted.load(Ordering::Relaxed))
            .map(|(&rip, entry)| {
                let score = entry.hotness_score(current_time, &hot_blocks);
                EvictionCandidate {
                    rip,
                    tier: entry.tier,
                    score,
                    exec_count: entry.exec_count.load(Ordering::Relaxed),
                }
            })
            .collect();
        
        // Sort by score ascending (coldest first)
        // But prioritize S1 over S2 for eviction (S1 cheaper to recompile)
        candidates.sort_by(|a, b| {
            // S1 before S2 if both have similar coldness
            match (a.tier, b.tier) {
                (CompileTier::S1, CompileTier::S2) if a.score < b.score * 2.0 => {
                    std::cmp::Ordering::Less
                }
                (CompileTier::S2, CompileTier::S1) if b.score < a.score * 2.0 => {
                    std::cmp::Ordering::Greater
                }
                _ => a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)
            }
        });
        
        candidates.truncate(count);
        candidates
    }
    
    /// Compute threshold for "hot" classification
    fn compute_hot_threshold(&self, entries: &HashMap<u64, HotnessEntry>, current_time: u64) -> f64 {
        if entries.is_empty() {
            return 0.0;
        }
        
        // Use 75th percentile as "hot" threshold
        let mut scores: Vec<f64> = entries.values()
            .map(|e| e.hotness_score(current_time, &HashSet::new()))
            .collect();
        scores.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let idx = (scores.len() as f64 * 0.75) as usize;
        scores.get(idx).copied().unwrap_or(0.0)
    }
    
    /// Mark a block as evicted
    pub fn mark_evicted(&self, rip: u64) {
        let entries = self.entries.read().unwrap();
        if let Some(entry) = entries.get(&rip) {
            entry.evicted.store(true, Ordering::Relaxed);
        }
    }
    
    /// Mark a block as restored (back in cache)
    pub fn mark_restored(&self, rip: u64) {
        let entries = self.entries.read().unwrap();
        if let Some(entry) = entries.get(&rip) {
            entry.evicted.store(false, Ordering::Relaxed);
        }
        self.evicted_index.mark_restored(rip);
    }
    
    /// Check if a block was evicted and can be restored
    pub fn can_restore(&self, rip: u64) -> Option<EvictedBlockInfo> {
        self.evicted_index.get_evicted(rip)
    }
    
    /// Get snapshot of hotness stats
    pub fn get_stats(&self) -> HotnessSnapshot {
        let entries = self.entries.read().unwrap();
        let current_time = self.current_timestamp();
        
        let mut total_blocks = 0;
        let mut s1_blocks = 0;
        let mut s2_blocks = 0;
        let mut evicted_blocks = 0;
        let mut total_execs = 0u64;
        
        for entry in entries.values() {
            total_blocks += 1;
            total_execs += entry.exec_count.load(Ordering::Relaxed);
            
            match entry.tier {
                CompileTier::S1 => s1_blocks += 1,
                CompileTier::S2 => s2_blocks += 1,
                _ => {}
            }
            
            if entry.evicted.load(Ordering::Relaxed) {
                evicted_blocks += 1;
            }
        }
        
        HotnessSnapshot {
            total_blocks,
            s1_blocks,
            s2_blocks,
            evicted_blocks,
            total_execs,
            evicted_index_size: self.evicted_index.len(),
        }
    }
}

/// Candidate for eviction
#[derive(Debug, Clone)]
pub struct EvictionCandidate {
    pub rip: u64,
    pub tier: CompileTier,
    pub score: f64,
    pub exec_count: u64,
}

impl EvictionCandidate {
    /// Should this block be preserved to disk?
    pub fn should_preserve(&self) -> bool {
        // Always preserve S2 (expensive to recompile)
        // Preserve S1 only if it has some value
        match self.tier {
            CompileTier::S2 => true,
            CompileTier::S1 => self.score >= MIN_PRESERVE_SCORE || self.exec_count >= 1000,
            CompileTier::Interpreter => false,
        }
    }
}

/// Hotness tracker statistics snapshot
#[derive(Debug, Clone, Default)]
pub struct HotnessSnapshot {
    pub total_blocks: usize,
    pub s1_blocks: usize,
    pub s2_blocks: usize,
    pub evicted_blocks: usize,
    pub total_execs: u64,
    pub evicted_index_size: usize,
}

// ============================================================================
// Eviction Manager
// ============================================================================

/// Result of an eviction operation
#[derive(Debug)]
pub struct EvictionResult {
    /// Blocks evicted to disk (preserved)
    pub evicted_to_disk: Vec<u64>,
    /// Blocks discarded (can be recompiled)
    pub discarded: Vec<u64>,
    /// Bytes freed in code cache
    pub bytes_freed: u64,
    /// Errors encountered
    pub errors: Vec<String>,
}

impl Default for EvictionResult {
    fn default() -> Self {
        Self {
            evicted_to_disk: Vec::new(),
            discarded: Vec::new(),
            bytes_freed: 0,
            errors: Vec::new(),
        }
    }
}

/// Eviction decision for a single block
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionDecision {
    /// Evict to disk (preserve for potential restoration)
    EvictToDisk,
    /// Discard (can be recompiled if needed)
    Discard,
    /// Keep in cache (too hot to evict)
    Keep,
}

/// Determines eviction decisions for blocks
pub fn decide_eviction(candidate: &EvictionCandidate, disk_space_available: bool) -> EvictionDecision {
    match candidate.tier {
        CompileTier::S2 => {
            // S2 is expensive - always try to preserve
            if disk_space_available {
                EvictionDecision::EvictToDisk
            } else {
                // Even without disk space, S2 is valuable
                // Only discard if truly cold
                if candidate.score < 10.0 {
                    EvictionDecision::Discard
                } else {
                    EvictionDecision::Keep
                }
            }
        }
        CompileTier::S1 => {
            // S1 is cheaper to recompile
            if candidate.should_preserve() && disk_space_available {
                EvictionDecision::EvictToDisk
            } else {
                EvictionDecision::Discard
            }
        }
        CompileTier::Interpreter => {
            // Interpreter blocks shouldn't be in code cache
            EvictionDecision::Discard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hotness_entry_score() {
        let entry = HotnessEntry::new(0x1000, CompileTier::S1);
        
        // Initial score should be 0
        let score = entry.hotness_score(0, &HashSet::new());
        assert_eq!(score, 0.0);
        
        // After some executions
        entry.record_exec(1_000_000); // 1 second
        entry.record_exec(1_000_000);
        entry.record_exec(1_000_000);
        
        let score = entry.hotness_score(1_000_000, &HashSet::new());
        assert!(score > 0.0);
    }
    
    #[test]
    fn test_hotness_decay() {
        let entry = HotnessEntry::new(0x1000, CompileTier::S1);
        
        // Execute at time 0
        for _ in 0..100 {
            entry.record_exec(0);
        }
        
        // Score at time 0
        let score_0 = entry.hotness_score(0, &HashSet::new());
        
        // Score after 60 seconds (one half-life)
        let score_60 = entry.hotness_score(60_000_000, &HashSet::new());
        
        // Score should be roughly halved
        assert!(score_60 < score_0);
        assert!(score_60 > score_0 * 0.4);
        assert!(score_60 < score_0 * 0.6);
    }
    
    #[test]
    fn test_eviction_priority() {
        let tracker = HotnessTracker::new();
        
        // Add some blocks
        tracker.register_block(0x1000, CompileTier::S1);
        tracker.register_block(0x2000, CompileTier::S2);
        tracker.register_block(0x3000, CompileTier::S1);
        
        // Make 0x1000 hot
        for _ in 0..100 {
            tracker.record_execution(0x1000);
        }
        
        // Select victims (should prefer cold S1)
        let victims = tracker.select_victims(2, &HashSet::new());
        
        // 0x3000 (cold S1) should be first
        assert!(!victims.is_empty());
        // 0x1000 (hot S1) should not be among first victims
        assert!(!victims.iter().any(|v| v.rip == 0x1000 && victims.iter().position(|x| x.rip == v.rip) == Some(0)));
    }
    
    #[test]
    fn test_evicted_index() {
        let index = EvictedIndex::new(3);
        
        for i in 0..5 {
            index.record_eviction(EvictedBlockInfo {
                rip: i * 0x1000,
                tier: CompileTier::S1,
                exec_count: 0,
                guest_checksum: 0,
                evicted_at: i,
                persist_path: format!("/tmp/block_{}.nvnc", i),
                has_native: true,
                has_ir: false,
            });
        }
        
        // Should only keep last 3
        assert_eq!(index.len(), 3);
        
        // Oldest should be evicted from index
        assert!(index.get_evicted(0).is_none());
        assert!(index.get_evicted(0x1000).is_none());
        
        // Newest should be present
        assert!(index.get_evicted(0x4000).is_some());
    }
    
    #[test]
    fn test_eviction_decision() {
        // S2 with disk space
        let s2_candidate = EvictionCandidate {
            rip: 0x1000,
            tier: CompileTier::S2,
            score: 50.0,
            exec_count: 500,
        };
        assert_eq!(decide_eviction(&s2_candidate, true), EvictionDecision::EvictToDisk);
        
        // Cold S1 without disk
        let s1_cold = EvictionCandidate {
            rip: 0x2000,
            tier: CompileTier::S1,
            score: 10.0,
            exec_count: 100,
        };
        assert_eq!(decide_eviction(&s1_cold, false), EvictionDecision::Discard);
        
        // Hot S1 with disk
        let s1_hot = EvictionCandidate {
            rip: 0x3000,
            tier: CompileTier::S1,
            score: 200.0,
            exec_count: 2000,
        };
        assert_eq!(decide_eviction(&s1_hot, true), EvictionDecision::EvictToDisk);
    }
}
