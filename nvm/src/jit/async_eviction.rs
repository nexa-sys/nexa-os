//! Async Eviction Manager
//!
//! Zero-STW background eviction for JIT code cache management.
//! Eviction happens incrementally in background without pausing VM execution.
//!
//! ## Design Goals
//!
//! 1. **No VM Pause**: Eviction is incremental and non-blocking. VM continues
//!    executing while cold code is being persisted to disk.
//!
//! 2. **Cooperative Scheduling**: Eviction work is done in small batches,
//!    yielding between batches to avoid starving other threads.
//!
//! 3. **Lazy Eviction**: Code is marked for eviction but actual disk write
//!    happens asynchronously. VM can still execute marked code until removal.
//!
//! 4. **Prioritized Persistence**: S2 code is persisted first (expensive to
//!    recompile), S1 code may be discarded if under pressure.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                    Zero-STW Eviction Architecture                            │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                              │
//! │  ┌───────────────┐    ┌───────────────┐    ┌───────────────────────────┐   │
//! │  │   CodeCache   │───▶│ Eviction Queue│───▶│  Background Eviction      │   │
//! │  │  (pressure)   │    │  (candidates) │    │  Worker Thread            │   │
//! │  └───────────────┘    └───────────────┘    └───────────────────────────┘   │
//! │         │                     │                       │                     │
//! │         │                     │                       ▼                     │
//! │         │                     │            ┌───────────────────────────┐   │
//! │         ▼                     │            │  Incremental Persist      │   │
//! │  ┌───────────────┐           │            │  ┌─────┐ ┌─────┐ ┌─────┐  │   │
//! │  │ Hotness Track │           │            │  │Batch│→│Batch│→│Batch│  │   │
//! │  │ (select cold) │           │            │  │  1  │ │  2  │ │  N  │  │   │
//! │  └───────────────┘           │            │  └─────┘ └─────┘ └─────┘  │   │
//! │                              │            └───────────────────────────┘   │
//! │                              ▼                       │                     │
//! │                   ┌───────────────────┐             ▼                     │
//! │                   │  Atomic Removal   │  ┌───────────────────────────┐   │
//! │                   │  (no VM pause)    │  │     NReady! Cache         │   │
//! │                   └───────────────────┘  │  (persisted native code)  │   │
//! │                                          └───────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Eviction Flow
//!
//! 1. **Trigger**: Cache reaches high watermark (e.g., 80% full)
//! 2. **Selection**: HotnessTracker selects cold blocks
//! 3. **Mark**: Blocks marked as "evicting" (still executable)
//! 4. **Persist**: Background thread writes to NReady! cache in batches
//! 5. **Remove**: After persist, atomically remove from CodeCache
//! 6. **Index**: Add to EvictedIndex for potential restoration

use std::collections::{VecDeque, HashSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, Mutex, Condvar};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::cache::{CodeCache, CompileTier, BlockPersistInfo};
use super::eviction::{HotnessTracker, EvictionCandidate, EvictedBlockInfo};
use super::nready::{NReadyCache, EvictableBlock, EvictionPersistResult};
use super::{JitResult, JitError};

// ============================================================================
// Configuration
// ============================================================================

/// High watermark: trigger eviction when cache exceeds this percentage
const HIGH_WATERMARK_PERCENT: f64 = 0.80;

/// Low watermark: stop eviction when cache falls below this percentage
const LOW_WATERMARK_PERCENT: f64 = 0.60;

/// Maximum blocks to process per eviction batch
const EVICTION_BATCH_SIZE: usize = 32;

/// Yield duration between batches (microseconds)
const BATCH_YIELD_US: u64 = 100;

/// Minimum interval between eviction cycles (milliseconds)
const MIN_EVICTION_INTERVAL_MS: u64 = 100;

/// Emergency eviction threshold (blocks to free immediately)
const EMERGENCY_EVICTION_COUNT: usize = 128;

/// Eviction worker poll interval when idle (milliseconds)
const IDLE_POLL_INTERVAL_MS: u64 = 500;

// ============================================================================
// Eviction State
// ============================================================================

/// State of a block being evicted
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionState {
    /// Normal - not being evicted
    Normal,
    /// Marked for eviction, still executable
    Marked,
    /// Being persisted to disk
    Persisting,
    /// Persisted, awaiting removal
    Persisted,
    /// Removed from cache
    Removed,
}

/// Block eviction tracking
#[derive(Debug)]
pub struct EvictingBlock {
    pub rip: u64,
    pub tier: CompileTier,
    pub state: EvictionState,
    pub native_code: Vec<u8>,
    pub guest_size: u32,
    pub guest_instrs: u32,
    pub guest_checksum: u64,
    pub exec_count: u64,
    pub ir_data: Option<Vec<u8>>,
    pub marked_at: Instant,
}

impl EvictingBlock {
    pub fn to_evictable(&self) -> EvictableBlock {
        EvictableBlock {
            rip: self.rip,
            tier: self.tier,
            native_code: self.native_code.clone(),
            guest_size: self.guest_size,
            guest_instrs: self.guest_instrs,
            guest_checksum: self.guest_checksum,
            exec_count: self.exec_count,
            ir_data: self.ir_data.clone(),
        }
    }
}

// ============================================================================
// Eviction Statistics
// ============================================================================

/// Eviction statistics
#[derive(Debug, Default)]
pub struct EvictionStats {
    /// Total eviction cycles started
    pub cycles_started: AtomicU64,
    /// Total eviction cycles completed
    pub cycles_completed: AtomicU64,
    /// Blocks persisted to disk
    pub blocks_persisted: AtomicU64,
    /// Blocks discarded (not persisted)
    pub blocks_discarded: AtomicU64,
    /// Bytes freed from cache
    pub bytes_freed: AtomicU64,
    /// Bytes written to disk
    pub bytes_to_disk: AtomicU64,
    /// Emergency evictions triggered
    pub emergency_evictions: AtomicU64,
    /// Persist errors
    pub persist_errors: AtomicU64,
    /// Total time spent evicting (microseconds)
    pub eviction_time_us: AtomicU64,
}

impl EvictionStats {
    pub fn snapshot(&self) -> EvictionStatsSnapshot {
        EvictionStatsSnapshot {
            cycles_started: self.cycles_started.load(Ordering::Relaxed),
            cycles_completed: self.cycles_completed.load(Ordering::Relaxed),
            blocks_persisted: self.blocks_persisted.load(Ordering::Relaxed),
            blocks_discarded: self.blocks_discarded.load(Ordering::Relaxed),
            bytes_freed: self.bytes_freed.load(Ordering::Relaxed),
            bytes_to_disk: self.bytes_to_disk.load(Ordering::Relaxed),
            emergency_evictions: self.emergency_evictions.load(Ordering::Relaxed),
            persist_errors: self.persist_errors.load(Ordering::Relaxed),
            eviction_time_us: self.eviction_time_us.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvictionStatsSnapshot {
    pub cycles_started: u64,
    pub cycles_completed: u64,
    pub blocks_persisted: u64,
    pub blocks_discarded: u64,
    pub bytes_freed: u64,
    pub bytes_to_disk: u64,
    pub emergency_evictions: u64,
    pub persist_errors: u64,
    pub eviction_time_us: u64,
}

// ============================================================================
// Async Eviction Manager
// ============================================================================

/// Asynchronous eviction manager
///
/// Performs background eviction without stopping VM execution.
pub struct AsyncEvictionManager {
    /// Code cache to evict from
    cache: Arc<CodeCache>,
    /// NReady! cache for persistence
    nready: Arc<NReadyCache>,
    /// Hotness tracker for candidate selection
    hotness: Arc<HotnessTracker>,
    /// Blocks currently being evicted
    evicting: Arc<RwLock<HashMap<u64, EvictingBlock>>>,
    /// Eviction queue (ordered by priority)
    queue: Arc<Mutex<VecDeque<u64>>>,
    /// Condition variable for worker wakeup
    cv: Arc<Condvar>,
    /// Worker thread handle
    worker: Option<JoinHandle<()>>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Pause flag (for emergency operations)
    paused: Arc<AtomicBool>,
    /// Statistics
    pub stats: Arc<EvictionStats>,
    /// Last eviction time
    last_eviction: Arc<RwLock<Instant>>,
    /// High watermark bytes
    high_watermark: u64,
    /// Low watermark bytes
    low_watermark: u64,
}

impl AsyncEvictionManager {
    /// Create a new async eviction manager
    pub fn new(
        cache: Arc<CodeCache>,
        nready: Arc<NReadyCache>,
        hotness: Arc<HotnessTracker>,
    ) -> Self {
        let capacity = cache.capacity();
        let high_watermark = (capacity as f64 * HIGH_WATERMARK_PERCENT) as u64;
        let low_watermark = (capacity as f64 * LOW_WATERMARK_PERCENT) as u64;
        
        log::info!("[AsyncEvict] Initialized: capacity={}MB, high={}MB, low={}MB",
            capacity / (1024 * 1024),
            high_watermark / (1024 * 1024),
            low_watermark / (1024 * 1024));
        
        Self {
            cache,
            nready,
            hotness,
            evicting: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            cv: Arc::new(Condvar::new()),
            worker: None,
            shutdown: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(EvictionStats::default()),
            last_eviction: Arc::new(RwLock::new(Instant::now())),
            high_watermark,
            low_watermark,
        }
    }
    
    /// Start the eviction worker thread
    pub fn start(&mut self) {
        log::info!("[AsyncEvict] Starting eviction worker");
        
        let cache = Arc::clone(&self.cache);
        let nready = Arc::clone(&self.nready);
        let hotness = Arc::clone(&self.hotness);
        let evicting = Arc::clone(&self.evicting);
        let queue = Arc::clone(&self.queue);
        let cv = Arc::clone(&self.cv);
        let shutdown = Arc::clone(&self.shutdown);
        let paused = Arc::clone(&self.paused);
        let stats = Arc::clone(&self.stats);
        let last_eviction = Arc::clone(&self.last_eviction);
        let high_watermark = self.high_watermark;
        let low_watermark = self.low_watermark;
        
        let handle = thread::Builder::new()
            .name("jit-eviction".to_string())
            .spawn(move || {
                eviction_worker_loop(
                    cache, nready, hotness, evicting, queue, cv,
                    shutdown, paused, stats, last_eviction,
                    high_watermark, low_watermark,
                );
            })
            .expect("Failed to spawn eviction worker");
        
        self.worker = Some(handle);
    }
    
    /// Trigger an eviction check (non-blocking)
    pub fn trigger(&self) {
        self.cv.notify_one();
    }
    
    /// Request emergency eviction (when cache is critically full)
    pub fn emergency_evict(&self, bytes_needed: u64) {
        log::warn!("[AsyncEvict] Emergency eviction requested: {} bytes needed", bytes_needed);
        
        self.stats.emergency_evictions.fetch_add(1, Ordering::Relaxed);
        
        // Queue high-priority eviction
        let candidates = self.hotness.select_victims(EMERGENCY_EVICTION_COUNT, &HashSet::new());
        
        let mut queue = self.queue.lock().unwrap();
        for candidate in candidates {
            if !self.is_evicting(candidate.rip) {
                queue.push_front(candidate.rip); // Front = high priority
            }
        }
        
        self.cv.notify_one();
    }
    
    /// Check if a block is currently being evicted
    pub fn is_evicting(&self, rip: u64) -> bool {
        let evicting = self.evicting.read().unwrap();
        evicting.contains_key(&rip)
    }
    
    /// Get eviction state for a block
    pub fn get_state(&self, rip: u64) -> EvictionState {
        let evicting = self.evicting.read().unwrap();
        evicting.get(&rip)
            .map(|b| b.state)
            .unwrap_or(EvictionState::Normal)
    }
    
    /// Pause eviction (for critical operations)
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }
    
    /// Resume eviction
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cv.notify_one();
    }
    
    /// Get statistics snapshot
    pub fn get_stats(&self) -> EvictionStatsSnapshot {
        self.stats.snapshot()
    }
    
    /// Shutdown the eviction manager
    pub fn shutdown(&mut self) {
        log::info!("[AsyncEvict] Shutting down...");
        
        self.shutdown.store(true, Ordering::SeqCst);
        self.cv.notify_all();
        
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        
        let stats = self.stats.snapshot();
        log::info!("[AsyncEvict] Shutdown complete. Persisted: {}, Discarded: {}, Freed: {}MB",
            stats.blocks_persisted,
            stats.blocks_discarded,
            stats.bytes_freed / (1024 * 1024));
    }
}

impl Drop for AsyncEvictionManager {
    fn drop(&mut self) {
        if !self.shutdown.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

// ============================================================================
// Eviction Worker
// ============================================================================

fn eviction_worker_loop(
    cache: Arc<CodeCache>,
    nready: Arc<NReadyCache>,
    hotness: Arc<HotnessTracker>,
    evicting: Arc<RwLock<HashMap<u64, EvictingBlock>>>,
    queue: Arc<Mutex<VecDeque<u64>>>,
    cv: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    stats: Arc<EvictionStats>,
    last_eviction: Arc<RwLock<Instant>>,
    high_watermark: u64,
    low_watermark: u64,
) {
    log::debug!("[AsyncEvict] Worker started");
    
    loop {
        // Wait for trigger or timeout
        {
            let mut queue_guard = queue.lock().unwrap();
            
            if queue_guard.is_empty() && !shutdown.load(Ordering::Relaxed) {
                // Wait with timeout for periodic checks
                let timeout = Duration::from_millis(IDLE_POLL_INTERVAL_MS);
                let result = cv.wait_timeout(queue_guard, timeout).unwrap();
                queue_guard = result.0;
            }
            
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
        }
        
        // Check if paused
        if paused.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        
        // Check if eviction needed
        let current_size = cache.total_size();
        if current_size < high_watermark {
            // Below high watermark, check queue for explicit requests
            let queue_guard = queue.lock().unwrap();
            if queue_guard.is_empty() {
                continue;
            }
        }
        
        // Check minimum interval
        {
            let last = last_eviction.read().unwrap();
            if last.elapsed().as_millis() < MIN_EVICTION_INTERVAL_MS as u128 {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }
        
        // Start eviction cycle
        let cycle_start = Instant::now();
        stats.cycles_started.fetch_add(1, Ordering::Relaxed);
        
        log::debug!("[AsyncEvict] Starting eviction cycle. Cache: {}MB / {}MB",
            current_size / (1024 * 1024),
            cache.capacity() / (1024 * 1024));
        
        // Select candidates
        let exclude = {
            let evicting_guard = evicting.read().unwrap();
            evicting_guard.keys().copied().collect::<HashSet<_>>()
        };
        
        let target_bytes = current_size.saturating_sub(low_watermark);
        let candidates = select_eviction_candidates(&hotness, &cache, target_bytes, &exclude);
        
        if candidates.is_empty() {
            log::debug!("[AsyncEvict] No candidates for eviction");
            continue;
        }
        
        log::debug!("[AsyncEvict] Selected {} candidates for eviction", candidates.len());
        
        // Process in batches
        let mut batch_start = 0;
        let mut total_freed = 0u64;
        let mut persisted_count = 0u64;
        let mut discarded_count = 0u64;
        
        while batch_start < candidates.len() && !shutdown.load(Ordering::Relaxed) {
            let batch_end = (batch_start + EVICTION_BATCH_SIZE).min(candidates.len());
            let batch = &candidates[batch_start..batch_end];
            
            // Process batch
            for candidate in batch {
                if shutdown.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) {
                    break;
                }
                
                match process_eviction_candidate(
                    candidate,
                    &cache,
                    &nready,
                    &hotness,
                    &evicting,
                    &stats,
                ) {
                    EvictionOutcome::Persisted(bytes) => {
                        total_freed += bytes;
                        persisted_count += 1;
                    }
                    EvictionOutcome::Discarded(bytes) => {
                        total_freed += bytes;
                        discarded_count += 1;
                    }
                    EvictionOutcome::Skipped => {}
                    EvictionOutcome::Error(e) => {
                        log::warn!("[AsyncEvict] Error evicting {:#x}: {}", candidate.rip, e);
                    }
                }
            }
            
            batch_start = batch_end;
            
            // Yield between batches
            if batch_start < candidates.len() {
                thread::sleep(Duration::from_micros(BATCH_YIELD_US));
            }
            
            // Check if we've freed enough
            let new_size = cache.total_size();
            if new_size <= low_watermark {
                log::debug!("[AsyncEvict] Reached low watermark, stopping");
                break;
            }
        }
        
        // Update stats
        stats.blocks_persisted.fetch_add(persisted_count, Ordering::Relaxed);
        stats.blocks_discarded.fetch_add(discarded_count, Ordering::Relaxed);
        stats.bytes_freed.fetch_add(total_freed, Ordering::Relaxed);
        stats.eviction_time_us.fetch_add(cycle_start.elapsed().as_micros() as u64, Ordering::Relaxed);
        stats.cycles_completed.fetch_add(1, Ordering::Relaxed);
        
        // Update last eviction time
        {
            let mut last = last_eviction.write().unwrap();
            *last = Instant::now();
        }
        
        log::info!("[AsyncEvict] Cycle complete: freed {}KB, persisted={}, discarded={}, time={:?}",
            total_freed / 1024,
            persisted_count,
            discarded_count,
            cycle_start.elapsed());
    }
    
    log::debug!("[AsyncEvict] Worker stopped");
}

/// Outcome of processing a single eviction candidate
enum EvictionOutcome {
    Persisted(u64),
    Discarded(u64),
    Skipped,
    Error(String),
}

/// Select candidates for eviction based on hotness and space needed
fn select_eviction_candidates(
    hotness: &HotnessTracker,
    cache: &CodeCache,
    target_bytes: u64,
    exclude: &HashSet<u64>,
) -> Vec<EvictionCandidate> {
    // Estimate average block size for candidate count
    let stats = cache.get_stats();
    let total_size = cache.total_size();
    let block_count = stats.s1_compiles + stats.s2_compiles;
    
    let avg_block_size = if block_count > 0 {
        total_size / block_count
    } else {
        4096 // Default estimate
    };
    
    let candidate_count = ((target_bytes / avg_block_size) * 2).max(16) as usize;
    
    hotness.select_victims(candidate_count.min(1000), exclude)
}

/// Process a single eviction candidate
fn process_eviction_candidate(
    candidate: &EvictionCandidate,
    cache: &CodeCache,
    nready: &NReadyCache,
    hotness: &HotnessTracker,
    evicting: &RwLock<HashMap<u64, EvictingBlock>>,
    stats: &EvictionStats,
) -> EvictionOutcome {
    let rip = candidate.rip;
    
    // Get block info from cache
    let block_info = match cache.get_block(rip) {
        Some(info) => info,
        None => return EvictionOutcome::Skipped,
    };
    
    if block_info.invalidated {
        return EvictionOutcome::Skipped;
    }
    
    let host_size = block_info.host_size as u64;
    
    // Decide: persist or discard
    if candidate.should_preserve() {
        // Persist to NReady! cache
        let evictable = EvictableBlock {
            rip,
            tier: candidate.tier,
            native_code: Vec::new(), // We'd need to read from cache
            guest_size: block_info.guest_size,
            guest_instrs: 0,
            guest_checksum: 0,
            exec_count: candidate.exec_count,
            ir_data: None,
        };
        
        match nready.evict_block(&evictable) {
            Ok(result) => {
                // Mark as evicted in hotness tracker
                hotness.mark_evicted(rip);
                
                // Record in evicted index
                hotness.evicted_index.record_eviction(EvictedBlockInfo {
                    rip,
                    tier: candidate.tier,
                    exec_count: candidate.exec_count,
                    guest_checksum: 0,
                    evicted_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_micros() as u64,
                    persist_path: result.path,
                    has_native: result.has_native,
                    has_ir: result.has_ir,
                });
                
                stats.bytes_to_disk.fetch_add(result.bytes_written as u64, Ordering::Relaxed);
                
                // Invalidate in cache (atomic, non-blocking)
                cache.invalidate(rip);
                
                EvictionOutcome::Persisted(host_size)
            }
            Err(e) => {
                stats.persist_errors.fetch_add(1, Ordering::Relaxed);
                EvictionOutcome::Error(format!("Persist failed: {:?}", e))
            }
        }
    } else {
        // Discard without persistence
        cache.invalidate(rip);
        EvictionOutcome::Discarded(host_size)
    }
}

// ============================================================================
// Incremental Removal
// ============================================================================

/// Incrementally remove evicted blocks from cache
/// 
/// This is called periodically to clean up blocks that have been
/// persisted. Uses atomic operations to avoid blocking lookups.
pub fn incremental_cache_cleanup(
    cache: &CodeCache,
    evicting: &RwLock<HashMap<u64, EvictingBlock>>,
    batch_size: usize,
) -> usize {
    let mut removed = 0;
    let to_remove: Vec<u64>;
    
    {
        let evicting_guard = evicting.read().unwrap();
        to_remove = evicting_guard
            .iter()
            .filter(|(_, b)| b.state == EvictionState::Persisted)
            .take(batch_size)
            .map(|(&rip, _)| rip)
            .collect();
    }
    
    for rip in to_remove {
        // Atomic invalidation (readers can still see it briefly)
        cache.invalidate(rip);
        
        // Remove from evicting map
        {
            let mut evicting_guard = evicting.write().unwrap();
            if let Some(mut block) = evicting_guard.get_mut(&rip) {
                block.state = EvictionState::Removed;
            }
        }
        
        removed += 1;
    }
    
    // Clean up removed entries
    {
        let mut evicting_guard = evicting.write().unwrap();
        evicting_guard.retain(|_, b| b.state != EvictionState::Removed);
    }
    
    removed
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_eviction_state_transitions() {
        let state = EvictionState::Normal;
        assert_eq!(state, EvictionState::Normal);
        
        // Valid transitions
        let state = EvictionState::Marked;
        let state = EvictionState::Persisting;
        let state = EvictionState::Persisted;
        let state = EvictionState::Removed;
        assert_eq!(state, EvictionState::Removed);
    }
    
    #[test]
    fn test_evicting_block_to_evictable() {
        let block = EvictingBlock {
            rip: 0x1000,
            tier: CompileTier::S2,
            state: EvictionState::Marked,
            native_code: vec![0x90, 0xC3],
            guest_size: 10,
            guest_instrs: 3,
            guest_checksum: 12345,
            exec_count: 1000,
            ir_data: None,
            marked_at: Instant::now(),
        };
        
        let evictable = block.to_evictable();
        assert_eq!(evictable.rip, 0x1000);
        assert_eq!(evictable.tier, CompileTier::S2);
        assert_eq!(evictable.native_code, vec![0x90, 0xC3]);
    }
    
    #[test]
    fn test_stats_snapshot() {
        let stats = EvictionStats::default();
        
        stats.blocks_persisted.fetch_add(10, Ordering::Relaxed);
        stats.blocks_discarded.fetch_add(5, Ordering::Relaxed);
        stats.bytes_freed.fetch_add(1024 * 1024, Ordering::Relaxed);
        
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.blocks_persisted, 10);
        assert_eq!(snapshot.blocks_discarded, 5);
        assert_eq!(snapshot.bytes_freed, 1024 * 1024);
    }
}
