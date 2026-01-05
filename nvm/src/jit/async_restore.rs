//! Async Restoration Manager
//!
//! Zero-STW background restoration for evicted JIT code blocks.
//! Supports prefetching and on-demand loading without VM pause.
//!
//! ## Design Goals
//!
//! 1. **No VM Pause**: Restoration happens in background. If code isn't ready,
//!    VM falls back to interpreter/recompile while restoration proceeds.
//!
//! 2. **Predictive Prefetch**: Analyzes execution patterns to prefetch blocks
//!    that are likely to be needed soon.
//!
//! 3. **On-Demand Loading**: When a cache miss hits evicted code, restoration
//!    is triggered with high priority.
//!
//! 4. **Cooperative Installation**: Restored code is installed atomically
//!    using CAS operations, no locking required.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                    Zero-STW Restoration Architecture                         │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                              │
//! │  VM Thread (Cache Miss)                                                      │
//! │  ┌───────────────────┐                                                       │
//! │  │ lookup(rip) fails │                                                       │
//! │  │ check evicted idx │───────────────┐                                      │
//! │  │ if evicted:       │               │                                      │
//! │  │   request_restore │               ▼                                      │
//! │  │   fallback interp │    ┌─────────────────────────┐                       │
//! │  └───────────────────┘    │  Restoration Queue      │                       │
//! │                           │  ┌────┐ ┌────┐ ┌────┐  │                       │
//! │                           │  │High│ │Med │ │Low │  │                       │
//! │                           │  │Pri │ │Pri │ │Pri │  │                       │
//! │                           │  └────┘ └────┘ └────┘  │                       │
//! │  Prefetch Analyzer        └─────────────────────────┘                       │
//! │  ┌───────────────────┐               │                                      │
//! │  │ Analyze patterns  │               │                                      │
//! │  │ Predict hot paths │───prefetch───▶│                                      │
//! │  │ Queue prefetch    │               │                                      │
//! │  └───────────────────┘               │                                      │
//! │                                       ▼                                      │
//! │                           ┌─────────────────────────┐                       │
//! │                           │  Restoration Workers    │                       │
//! │                           │  ┌───────┐ ┌───────┐   │                       │
//! │                           │  │Worker0│ │Worker1│   │                       │
//! │                           │  │ Read  │ │ Read  │   │                       │
//! │                           │  │ Disk  │ │ Disk  │   │                       │
//! │                           │  └───────┘ └───────┘   │                       │
//! │                           └─────────────────────────┘                       │
//! │                                       │                                      │
//! │                                       ▼                                      │
//! │                           ┌─────────────────────────┐                       │
//! │                           │  Atomic Installation    │                       │
//! │                           │  (CAS to CodeCache)     │                       │
//! │                           └─────────────────────────┘                       │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, Mutex, Condvar};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::cmp::Ordering as CmpOrdering;

use super::cache::{CodeCache, CompileTier, CompiledBlock};
use super::eviction::{HotnessTracker, EvictedBlockInfo};
use super::nready::{NReadyCache, RestoredBlock};
use super::{JitResult, JitError};

// ============================================================================
// Configuration
// ============================================================================

/// Maximum pending restoration requests
const MAX_RESTORE_QUEUE: usize = 1000;

/// Default restoration worker count (0 = auto)
const DEFAULT_RESTORE_WORKERS: usize = 0;

/// Prefetch lookahead window (blocks)
const PREFETCH_LOOKAHEAD: usize = 8;

/// Prefetch trigger threshold (sequential misses)
const PREFETCH_TRIGGER_THRESHOLD: usize = 3;

/// Restoration timeout (seconds)
const RESTORE_TIMEOUT_SECS: u64 = 10;

/// Batch size for prefetch analysis
const PREFETCH_BATCH_SIZE: usize = 16;

/// History window for pattern detection
const PATTERN_HISTORY_SIZE: usize = 64;

// ============================================================================
// Restoration Priority
// ============================================================================

/// Restoration priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum RestorePriority {
    /// Background prefetch (lowest)
    Prefetch = 0,
    /// Warm restoration (medium)
    Warm = 1,
    /// Hot restoration (high)  
    Hot = 2,
    /// On-demand restoration (highest)
    OnDemand = 3,
}

impl RestorePriority {
    pub fn from_exec_count(exec_count: u64, is_on_demand: bool) -> Self {
        if is_on_demand {
            RestorePriority::OnDemand
        } else if exec_count >= 1000 {
            RestorePriority::Hot
        } else if exec_count >= 100 {
            RestorePriority::Warm
        } else {
            RestorePriority::Prefetch
        }
    }
}

// ============================================================================
// Restoration Request
// ============================================================================

/// Request to restore an evicted block
#[derive(Debug, Clone)]
pub struct RestoreRequest {
    /// Guest RIP to restore
    pub rip: u64,
    /// Priority
    pub priority: RestorePriority,
    /// Previous execution count (for priority)
    pub exec_count: u64,
    /// Request timestamp
    pub requested_at: Instant,
    /// Request ID (for deduplication)
    pub request_id: u64,
    /// Eviction info (path, tier, etc.)
    pub eviction_info: EvictedBlockInfo,
    /// Is this from prefetch prediction?
    pub is_prefetch: bool,
}

impl RestoreRequest {
    pub fn new(rip: u64, info: EvictedBlockInfo, is_on_demand: bool) -> Self {
        let priority = RestorePriority::from_exec_count(info.exec_count, is_on_demand);
        Self {
            rip,
            priority,
            exec_count: info.exec_count,
            requested_at: Instant::now(),
            request_id: generate_request_id(),
            eviction_info: info,
            is_prefetch: !is_on_demand,
        }
    }
    
    pub fn is_expired(&self) -> bool {
        self.requested_at.elapsed().as_secs() > RESTORE_TIMEOUT_SECS
    }
}

impl PartialEq for RestoreRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request_id == other.request_id
    }
}

impl Eq for RestoreRequest {}

impl PartialOrd for RestoreRequest {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for RestoreRequest {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Higher priority first, then higher exec_count
        match self.priority.cmp(&other.priority) {
            CmpOrdering::Equal => self.exec_count.cmp(&other.exec_count),
            other => other,
        }
    }
}

// ============================================================================
// Restoration Result
// ============================================================================

/// Result of a restoration attempt
#[derive(Debug)]
pub struct RestoreResult {
    pub request_id: u64,
    pub rip: u64,
    pub success: bool,
    pub native_code: Option<Vec<u8>>,
    pub tier: CompileTier,
    pub restore_time: Duration,
    pub from_native: bool,  // true = native code restored, false = needs recompile
    pub error: Option<String>,
}

impl RestoreResult {
    pub fn success(
        request_id: u64,
        rip: u64,
        native_code: Vec<u8>,
        tier: CompileTier,
        restore_time: Duration,
        from_native: bool,
    ) -> Self {
        Self {
            request_id,
            rip,
            success: true,
            native_code: Some(native_code),
            tier,
            restore_time,
            from_native,
            error: None,
        }
    }
    
    pub fn failure(request_id: u64, rip: u64, error: String) -> Self {
        Self {
            request_id,
            rip,
            success: false,
            native_code: None,
            tier: CompileTier::Interpreter,
            restore_time: Duration::ZERO,
            from_native: false,
            error: Some(error),
        }
    }
}

// ============================================================================
// Restoration Callback
// ============================================================================

/// Callback for restoration completion
pub trait RestoreCallback: Send + Sync {
    fn on_restore_complete(&self, result: RestoreResult);
}

/// Default callback that installs to CodeCache
pub struct CacheRestoreInstaller {
    cache: Arc<CodeCache>,
    hotness: Arc<HotnessTracker>,
}

impl CacheRestoreInstaller {
    pub fn new(cache: Arc<CodeCache>, hotness: Arc<HotnessTracker>) -> Self {
        Self { cache, hotness }
    }
}

impl RestoreCallback for CacheRestoreInstaller {
    fn on_restore_complete(&self, result: RestoreResult) {
        if !result.success {
            return;
        }
        
        if let Some(native_code) = result.native_code {
            if let Some(code_ptr) = self.cache.allocate_code(&native_code) {
                let block = CompiledBlock {
                    guest_rip: result.rip,
                    guest_size: 0,
                    host_code: code_ptr,
                    host_size: native_code.len() as u32,
                    tier: result.tier,
                    exec_count: AtomicU64::new(0),
                    last_access: AtomicU64::new(0),
                    guest_instrs: 0,
                    guest_checksum: 0,
                    depends_on: Vec::new(),
                    invalidated: false,
                };
                
                if self.cache.insert(block).is_ok() {
                    // Mark as restored in hotness tracker
                    self.hotness.mark_restored(result.rip);
                    log::debug!("[AsyncRestore] Installed restored block {:#x}", result.rip);
                }
            }
        }
    }
}

// ============================================================================
// Restoration Statistics
// ============================================================================

#[derive(Debug, Default)]
pub struct RestoreStats {
    /// Total restore requests
    pub requests_total: AtomicU64,
    /// Successful restorations
    pub restorations_success: AtomicU64,
    /// Failed restorations
    pub restorations_failed: AtomicU64,
    /// Native code restored (fast path)
    pub native_restored: AtomicU64,
    /// IR restored (needs recompile)
    pub ir_restored: AtomicU64,
    /// Prefetch hits (prefetched block was needed)
    pub prefetch_hits: AtomicU64,
    /// Prefetch misses (prefetched block wasn't used)
    pub prefetch_misses: AtomicU64,
    /// On-demand restorations
    pub on_demand_count: AtomicU64,
    /// Total restore time (microseconds)
    pub restore_time_us: AtomicU64,
    /// Current queue depth
    pub queue_depth: AtomicUsize,
}

impl RestoreStats {
    pub fn snapshot(&self) -> RestoreStatsSnapshot {
        RestoreStatsSnapshot {
            requests_total: self.requests_total.load(Ordering::Relaxed),
            restorations_success: self.restorations_success.load(Ordering::Relaxed),
            restorations_failed: self.restorations_failed.load(Ordering::Relaxed),
            native_restored: self.native_restored.load(Ordering::Relaxed),
            ir_restored: self.ir_restored.load(Ordering::Relaxed),
            prefetch_hits: self.prefetch_hits.load(Ordering::Relaxed),
            prefetch_misses: self.prefetch_misses.load(Ordering::Relaxed),
            on_demand_count: self.on_demand_count.load(Ordering::Relaxed),
            avg_restore_time: self.avg_restore_time(),
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
        }
    }
    
    pub fn avg_restore_time(&self) -> Duration {
        let count = self.restorations_success.load(Ordering::Relaxed);
        if count == 0 {
            return Duration::ZERO;
        }
        let total_us = self.restore_time_us.load(Ordering::Relaxed);
        Duration::from_micros(total_us / count)
    }
}

#[derive(Debug, Clone)]
pub struct RestoreStatsSnapshot {
    pub requests_total: u64,
    pub restorations_success: u64,
    pub restorations_failed: u64,
    pub native_restored: u64,
    pub ir_restored: u64,
    pub prefetch_hits: u64,
    pub prefetch_misses: u64,
    pub on_demand_count: u64,
    pub avg_restore_time: Duration,
    pub queue_depth: usize,
}

// ============================================================================
// Prefetch Analyzer
// ============================================================================

/// Analyzes execution patterns for prefetch prediction
pub struct PrefetchAnalyzer {
    /// Recent cache miss RIPs (circular buffer)
    miss_history: RwLock<VecDeque<u64>>,
    /// Call graph edges: caller -> callees
    call_graph: RwLock<HashMap<u64, HashSet<u64>>>,
    /// Sequential access patterns: rip -> next_rip count
    sequential_patterns: RwLock<HashMap<u64, HashMap<u64, u32>>>,
    /// Blocks currently being prefetched
    prefetching: RwLock<HashSet<u64>>,
}

impl PrefetchAnalyzer {
    pub fn new() -> Self {
        Self {
            miss_history: RwLock::new(VecDeque::with_capacity(PATTERN_HISTORY_SIZE)),
            call_graph: RwLock::new(HashMap::new()),
            sequential_patterns: RwLock::new(HashMap::new()),
            prefetching: RwLock::new(HashSet::new()),
        }
    }
    
    /// Record a cache miss
    pub fn record_miss(&self, rip: u64) {
        let mut history = self.miss_history.write().unwrap();
        
        // Update sequential pattern from last miss
        if let Some(&last_rip) = history.back() {
            let mut patterns = self.sequential_patterns.write().unwrap();
            let next_counts = patterns.entry(last_rip).or_insert_with(HashMap::new);
            *next_counts.entry(rip).or_insert(0) += 1;
        }
        
        history.push_back(rip);
        if history.len() > PATTERN_HISTORY_SIZE {
            history.pop_front();
        }
    }
    
    /// Record a call edge
    pub fn record_call(&self, caller: u64, callee: u64) {
        let mut graph = self.call_graph.write().unwrap();
        graph.entry(caller).or_insert_with(HashSet::new).insert(callee);
    }
    
    /// Predict blocks to prefetch based on a restored block
    pub fn predict_prefetch(&self, rip: u64, evicted_index: &HashMap<u64, EvictedBlockInfo>) -> Vec<u64> {
        let mut predictions = Vec::new();
        let prefetching = self.prefetching.read().unwrap();
        
        // 1. Sequential pattern predictions
        {
            let patterns = self.sequential_patterns.read().unwrap();
            if let Some(next_counts) = patterns.get(&rip) {
                // Sort by count, take top predictions
                let mut sorted: Vec<_> = next_counts.iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(a.1));
                
                for (&next_rip, &count) in sorted.iter().take(PREFETCH_LOOKAHEAD / 2) {
                    if count >= PREFETCH_TRIGGER_THRESHOLD as u32 
                        && evicted_index.contains_key(&next_rip)
                        && !prefetching.contains(&next_rip)
                    {
                        predictions.push(next_rip);
                    }
                }
            }
        }
        
        // 2. Call graph predictions
        {
            let graph = self.call_graph.read().unwrap();
            if let Some(callees) = graph.get(&rip) {
                for &callee in callees.iter().take(PREFETCH_LOOKAHEAD / 2) {
                    if evicted_index.contains_key(&callee)
                        && !prefetching.contains(&callee)
                        && !predictions.contains(&callee)
                    {
                        predictions.push(callee);
                    }
                }
            }
        }
        
        predictions.truncate(PREFETCH_LOOKAHEAD);
        
        // Mark as prefetching
        if !predictions.is_empty() {
            let mut prefetching = self.prefetching.write().unwrap();
            for &rip in &predictions {
                prefetching.insert(rip);
            }
        }
        
        predictions
    }
    
    /// Mark prefetch complete
    pub fn prefetch_complete(&self, rip: u64, was_used: bool) {
        let mut prefetching = self.prefetching.write().unwrap();
        prefetching.remove(&rip);
        
        // Could track hit/miss rates here for adaptive prefetching
    }
}

impl Default for PrefetchAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Async Restoration Manager
// ============================================================================

/// Asynchronous restoration manager
pub struct AsyncRestoreManager {
    /// NReady! cache for reading persisted blocks
    nready: Arc<NReadyCache>,
    /// Hotness tracker (for evicted index)
    hotness: Arc<HotnessTracker>,
    /// Priority queue for restore requests
    queue: Arc<Mutex<BinaryHeap<RestoreRequest>>>,
    /// In-flight requests (for deduplication)
    in_flight: Arc<RwLock<HashSet<u64>>>,
    /// Condition variable for worker wakeup
    cv: Arc<Condvar>,
    /// Worker threads
    workers: Vec<JoinHandle<()>>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Restoration callback
    callback: Arc<dyn RestoreCallback>,
    /// Prefetch analyzer
    pub prefetch: Arc<PrefetchAnalyzer>,
    /// Statistics
    pub stats: Arc<RestoreStats>,
    /// Worker count
    worker_count: usize,
}

impl AsyncRestoreManager {
    pub fn new(
        nready: Arc<NReadyCache>,
        hotness: Arc<HotnessTracker>,
        callback: Arc<dyn RestoreCallback>,
        worker_count: Option<usize>,
    ) -> Self {
        let count = worker_count.unwrap_or_else(|| {
            // Default: 2 workers (restore is I/O bound)
            let cpus = std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(4);
            (cpus / 4).max(2).min(4)
        });
        
        Self {
            nready,
            hotness,
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            in_flight: Arc::new(RwLock::new(HashSet::new())),
            cv: Arc::new(Condvar::new()),
            workers: Vec::with_capacity(count),
            shutdown: Arc::new(AtomicBool::new(false)),
            callback,
            prefetch: Arc::new(PrefetchAnalyzer::new()),
            stats: Arc::new(RestoreStats::default()),
            worker_count: count,
        }
    }
    
    /// Start the restoration workers
    pub fn start(&mut self) {
        log::info!("[AsyncRestore] Starting with {} workers", self.worker_count);
        
        for i in 0..self.worker_count {
            let nready = Arc::clone(&self.nready);
            let hotness = Arc::clone(&self.hotness);
            let queue = Arc::clone(&self.queue);
            let in_flight = Arc::clone(&self.in_flight);
            let cv = Arc::clone(&self.cv);
            let shutdown = Arc::clone(&self.shutdown);
            let callback = Arc::clone(&self.callback);
            let prefetch = Arc::clone(&self.prefetch);
            let stats = Arc::clone(&self.stats);
            
            let handle = thread::Builder::new()
                .name(format!("jit-restore-{}", i))
                .spawn(move || {
                    restore_worker_loop(
                        i, nready, hotness, queue, in_flight, cv,
                        shutdown, callback, prefetch, stats,
                    );
                })
                .expect("Failed to spawn restore worker");
            
            self.workers.push(handle);
        }
    }
    
    /// Request restoration of an evicted block (non-blocking)
    /// 
    /// Returns true if request was queued, false if already in-flight or not evicted.
    pub fn request_restore(&self, rip: u64, is_on_demand: bool) -> bool {
        // Check if already in-flight
        {
            let in_flight = self.in_flight.read().unwrap();
            if in_flight.contains(&rip) {
                return false;
            }
        }
        
        // Check if block was evicted
        let info = match self.hotness.evicted_index.get_evicted(rip) {
            Some(info) => info,
            None => return false,
        };
        
        // Check queue capacity
        let mut queue = self.queue.lock().unwrap();
        if queue.len() >= MAX_RESTORE_QUEUE {
            log::warn!("[AsyncRestore] Queue full, dropping request for {:#x}", rip);
            return false;
        }
        
        // Mark as in-flight
        {
            let mut in_flight = self.in_flight.write().unwrap();
            in_flight.insert(rip);
        }
        
        // Record stats
        self.stats.requests_total.fetch_add(1, Ordering::Relaxed);
        if is_on_demand {
            self.stats.on_demand_count.fetch_add(1, Ordering::Relaxed);
        }
        self.stats.queue_depth.fetch_add(1, Ordering::Relaxed);
        
        // Record miss for prefetch analysis
        self.prefetch.record_miss(rip);
        
        // Queue request
        let request = RestoreRequest::new(rip, info, is_on_demand);
        queue.push(request);
        drop(queue);
        
        // Wake up a worker
        self.cv.notify_one();
        
        log::trace!("[AsyncRestore] Queued restore for {:#x} (on_demand={})", rip, is_on_demand);
        true
    }
    
    /// Request prefetch of multiple blocks (lower priority)
    pub fn prefetch_blocks(&self, rips: &[u64]) {
        for &rip in rips {
            self.request_restore(rip, false);
        }
    }
    
    /// Check if block is being restored
    pub fn is_restoring(&self, rip: u64) -> bool {
        let in_flight = self.in_flight.read().unwrap();
        in_flight.contains(&rip)
    }
    
    /// Cancel pending restore (if not started)
    pub fn cancel(&self, rip: u64) -> bool {
        let mut in_flight = self.in_flight.write().unwrap();
        if in_flight.remove(&rip) {
            self.stats.queue_depth.fetch_sub(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
    
    /// Get statistics
    pub fn get_stats(&self) -> RestoreStatsSnapshot {
        self.stats.snapshot()
    }
    
    /// Shutdown
    pub fn shutdown(&mut self) {
        log::info!("[AsyncRestore] Shutting down...");
        
        self.shutdown.store(true, Ordering::SeqCst);
        self.cv.notify_all();
        
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        
        let stats = self.stats.snapshot();
        log::info!("[AsyncRestore] Shutdown complete. Restored: {}, Failed: {}, Prefetch hits: {}",
            stats.restorations_success,
            stats.restorations_failed,
            stats.prefetch_hits);
    }
}

impl Drop for AsyncRestoreManager {
    fn drop(&mut self) {
        if !self.shutdown.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

// ============================================================================
// Restoration Worker
// ============================================================================

fn restore_worker_loop(
    worker_id: usize,
    nready: Arc<NReadyCache>,
    hotness: Arc<HotnessTracker>,
    queue: Arc<Mutex<BinaryHeap<RestoreRequest>>>,
    in_flight: Arc<RwLock<HashSet<u64>>>,
    cv: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
    callback: Arc<dyn RestoreCallback>,
    prefetch: Arc<PrefetchAnalyzer>,
    stats: Arc<RestoreStats>,
) {
    log::debug!("[AsyncRestore] Worker {} started", worker_id);
    
    loop {
        // Get next request
        let request = {
            let mut queue_guard = queue.lock().unwrap();
            
            while queue_guard.is_empty() && !shutdown.load(Ordering::Relaxed) {
                queue_guard = cv.wait(queue_guard).unwrap();
            }
            
            if shutdown.load(Ordering::Relaxed) && queue_guard.is_empty() {
                break;
            }
            
            queue_guard.pop()
        };
        
        let Some(request) = request else { continue };
        
        // Check if cancelled
        {
            let in_flight_guard = in_flight.read().unwrap();
            if !in_flight_guard.contains(&request.rip) {
                stats.queue_depth.fetch_sub(1, Ordering::Relaxed);
                continue;
            }
        }
        
        // Check if expired
        if request.is_expired() {
            log::debug!("[AsyncRestore] Worker {}: Request {:#x} expired", worker_id, request.rip);
            
            let mut in_flight_guard = in_flight.write().unwrap();
            in_flight_guard.remove(&request.rip);
            stats.queue_depth.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        
        // Perform restoration
        let start = Instant::now();
        let result = perform_restore(worker_id, &request, &nready, &stats);
        let restore_time = start.elapsed();
        
        // Remove from in-flight
        {
            let mut in_flight_guard = in_flight.write().unwrap();
            in_flight_guard.remove(&request.rip);
        }
        
        stats.queue_depth.fetch_sub(1, Ordering::Relaxed);
        
        // Update stats
        if result.success {
            stats.restorations_success.fetch_add(1, Ordering::Relaxed);
            stats.restore_time_us.fetch_add(restore_time.as_micros() as u64, Ordering::Relaxed);
            
            if result.from_native {
                stats.native_restored.fetch_add(1, Ordering::Relaxed);
            } else {
                stats.ir_restored.fetch_add(1, Ordering::Relaxed);
            }
            
            // Trigger prefetch for predicted blocks
            let evicted_map: HashMap<u64, EvictedBlockInfo> = {
                // This is a simplified view - in production, we'd have a more efficient API
                HashMap::new() // TODO: Get actual evicted index
            };
            let prefetch_rips = prefetch.predict_prefetch(request.rip, &evicted_map);
            // Queue prefetch requests (handled by manager)
            
            log::debug!("[AsyncRestore] Worker {}: Restored {:#x} in {:?} (native={})",
                worker_id, request.rip, restore_time, result.from_native);
        } else {
            stats.restorations_failed.fetch_add(1, Ordering::Relaxed);
            log::warn!("[AsyncRestore] Worker {}: Failed to restore {:#x}: {:?}",
                worker_id, request.rip, result.error);
        }
        
        // Mark prefetch status
        if request.is_prefetch {
            prefetch.prefetch_complete(request.rip, result.success);
        }
        
        // Callback (installs to cache)
        callback.on_restore_complete(result);
    }
    
    log::debug!("[AsyncRestore] Worker {} stopped", worker_id);
}

/// Perform the actual restoration
fn perform_restore(
    _worker_id: usize,
    request: &RestoreRequest,
    nready: &NReadyCache,
    _stats: &RestoreStats,
) -> RestoreResult {
    let rip = request.rip;
    
    // Try to restore from NReady! cache
    match nready.restore_block(rip) {
        Ok(Some(restored)) => {
            // Check if we have native code
            if let Some(native_code) = restored.native_code {
                RestoreResult::success(
                    request.request_id,
                    rip,
                    native_code,
                    restored.tier,
                    Duration::ZERO, // Will be set by caller
                    true,
                )
            } else if restored.ir_data.is_some() {
                // Have IR, needs recompilation
                // Return empty native code to signal recompilation needed
                RestoreResult::success(
                    request.request_id,
                    rip,
                    Vec::new(),
                    restored.tier,
                    Duration::ZERO,
                    false,
                )
            } else {
                RestoreResult::failure(
                    request.request_id,
                    rip,
                    "No native code or IR in restored block".to_string(),
                )
            }
        }
        Ok(None) => {
            RestoreResult::failure(
                request.request_id,
                rip,
                "Block not found in NReady! cache".to_string(),
            )
        }
        Err(e) => {
            RestoreResult::failure(
                request.request_id,
                rip,
                format!("Restore error: {:?}", e),
            )
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn generate_request_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_restore_priority() {
        // On-demand always highest
        assert_eq!(
            RestorePriority::from_exec_count(0, true),
            RestorePriority::OnDemand
        );
        
        // Cold -> Prefetch
        assert_eq!(
            RestorePriority::from_exec_count(10, false),
            RestorePriority::Prefetch
        );
        
        // Warm -> Warm
        assert_eq!(
            RestorePriority::from_exec_count(500, false),
            RestorePriority::Warm
        );
        
        // Hot -> Hot
        assert_eq!(
            RestorePriority::from_exec_count(2000, false),
            RestorePriority::Hot
        );
    }
    
    #[test]
    fn test_request_ordering() {
        let r1 = RestoreRequest {
            rip: 0x1000,
            priority: RestorePriority::Prefetch,
            exec_count: 10,
            requested_at: Instant::now(),
            request_id: 1,
            eviction_info: EvictedBlockInfo {
                rip: 0x1000,
                tier: CompileTier::S1,
                exec_count: 10,
                guest_checksum: 0,
                evicted_at: 0,
                persist_path: String::new(),
                has_native: true,
                has_ir: false,
            },
            is_prefetch: true,
        };
        
        let r2 = RestoreRequest {
            rip: 0x2000,
            priority: RestorePriority::OnDemand,
            exec_count: 5,
            requested_at: Instant::now(),
            request_id: 2,
            eviction_info: EvictedBlockInfo {
                rip: 0x2000,
                tier: CompileTier::S1,
                exec_count: 5,
                guest_checksum: 0,
                evicted_at: 0,
                persist_path: String::new(),
                has_native: true,
                has_ir: false,
            },
            is_prefetch: false,
        };
        
        // On-demand should be higher priority
        assert!(r2 > r1);
    }
    
    #[test]
    fn test_prefetch_analyzer() {
        let analyzer = PrefetchAnalyzer::new();
        
        // Record sequential pattern
        analyzer.record_miss(0x1000);
        analyzer.record_miss(0x2000);
        analyzer.record_miss(0x1000);
        analyzer.record_miss(0x2000);
        analyzer.record_miss(0x1000);
        analyzer.record_miss(0x2000);
        
        // Should detect 0x1000 -> 0x2000 pattern
        let patterns = analyzer.sequential_patterns.read().unwrap();
        assert!(patterns.contains_key(&0x1000));
        assert!(patterns[&0x1000].get(&0x2000).unwrap_or(&0) >= &3);
    }
    
    #[test]
    fn test_stats_snapshot() {
        let stats = RestoreStats::default();
        
        stats.requests_total.fetch_add(100, Ordering::Relaxed);
        stats.restorations_success.fetch_add(90, Ordering::Relaxed);
        stats.native_restored.fetch_add(80, Ordering::Relaxed);
        stats.restore_time_us.fetch_add(9000, Ordering::Relaxed); // 100us avg
        
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.requests_total, 100);
        assert_eq!(snapshot.restorations_success, 90);
        assert_eq!(snapshot.native_restored, 80);
        assert_eq!(snapshot.avg_restore_time, Duration::from_micros(100));
    }
}
