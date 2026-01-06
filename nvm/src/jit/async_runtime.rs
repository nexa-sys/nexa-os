//! Async JIT Compilation Runtime
//!
//! Zero-STW (Stop-The-World) asynchronous JIT compilation system.
//! All compilation happens in background threads without pausing VM execution.
//!
//! ## Design Principles
//!
//! 1. **Never Block VM Execution**: Compilation requests are queued and processed
//!    asynchronously. VM continues interpreting while compilation proceeds.
//!
//! 2. **Priority-Based Scheduling**: Hot code paths get compiled first.
//!    S2 compilation has lower priority than S1 (faster S1 = faster startup).
//!
//! 3. **Incremental Installation**: Compiled code is installed atomically
//!    without requiring VM pause. Uses atomic pointer swaps.
//!
//! 4. **Concurrent Compilation**: Multiple blocks can be compiled in parallel
//!    up to a configurable limit (default: CPU count).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                      Async JIT Runtime Architecture                          │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                              │
//! │  VM Thread                    Compilation Workers                            │
//! │  ┌───────────┐               ┌───────────────────────────────────────────┐  │
//! │  │ Execute   │               │  Worker Pool (N threads)                  │  │
//! │  │ (interp.) │──request────▶│  ┌─────────┐ ┌─────────┐ ┌─────────┐     │  │
//! │  │           │               │  │ Worker 0│ │ Worker 1│ │ Worker N│     │  │
//! │  │ Check     │◀──callback───│  │ (S1/S2) │ │ (S1/S2) │ │ (S1/S2) │     │  │
//! │  │ hotswap   │               │  └─────────┘ └─────────┘ └─────────┘     │  │
//! │  └───────────┘               └───────────────────────────────────────────┘  │
//! │       │                                      │                               │
//! │       │                                      │                               │
//! │       ▼                                      ▼                               │
//! │  ┌───────────┐               ┌───────────────────────────────────────────┐  │
//! │  │ CodeCache │◀───install────│  Priority Queue                           │  │
//! │  │ (atomic)  │               │  ┌─────────────────────────────────────┐  │  │
//! │  └───────────┘               │  │ High: Hot S1 requests               │  │  │
//! │                              │  │ Med:  Warm S1, Hot S2 requests      │  │  │
//! │                              │  │ Low:  Background S2 optimizations   │  │  │
//! │                              │  └─────────────────────────────────────┘  │  │
//! │                              └───────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, Mutex, Condvar};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::cmp::Ordering as CmpOrdering;

use super::cache::{CodeCache, CompileTier, CompiledBlock};
use super::ir::IrBlock;
use super::decoder::X86Decoder;
use super::compiler_s1::S1Compiler;
use super::compiler_s2::S2Compiler;
use super::codegen::CodeGen;
use super::profile::ProfileDb;
use super::{JitError, JitResult, BlockMeta, ExecutionTier};

// ============================================================================
// Configuration
// ============================================================================

/// Maximum compilation requests in queue
const MAX_QUEUE_SIZE: usize = 10_000;

/// Default worker thread count (0 = auto-detect)
const DEFAULT_WORKER_COUNT: usize = 0;

/// Compilation timeout per block (seconds)
const COMPILATION_TIMEOUT_SECS: u64 = 30;

/// Batch size for priority queue processing
const PRIORITY_BATCH_SIZE: usize = 16;

/// Hot code detection threshold (executions before S1 request)
const HOT_S1_THRESHOLD: u64 = 50;

/// Very hot code threshold (for priority boost)
const VERY_HOT_THRESHOLD: u64 = 500;

// ============================================================================
// Compilation Priority
// ============================================================================

/// Compilation request priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CompilePriority {
    /// Lowest: Background optimization work
    Background = 0,
    /// Low: Cold code S1 compilation
    Low = 1,
    /// Normal: Warm code compilation
    Normal = 2,
    /// High: Hot code S1 compilation
    High = 3,
    /// Critical: Very hot code needing immediate compilation
    Critical = 4,
}

impl CompilePriority {
    /// Calculate priority from execution statistics
    pub fn from_stats(exec_count: u64, tier: CompileTier) -> Self {
        match tier {
            CompileTier::Interpreter => {
                // S1 compilation request
                if exec_count >= VERY_HOT_THRESHOLD {
                    CompilePriority::Critical
                } else if exec_count >= HOT_S1_THRESHOLD {
                    CompilePriority::High
                } else {
                    CompilePriority::Normal
                }
            }
            CompileTier::S1 => {
                // S2 promotion request (lower priority - S1 already works)
                if exec_count >= VERY_HOT_THRESHOLD * 10 {
                    CompilePriority::High
                } else if exec_count >= VERY_HOT_THRESHOLD {
                    CompilePriority::Normal
                } else {
                    CompilePriority::Low
                }
            }
            CompileTier::S2 => {
                // Already at highest tier - reoptimization
                CompilePriority::Background
            }
        }
    }
}

// ============================================================================
// Compilation Request
// ============================================================================

/// A request to compile a code block
#[derive(Debug, Clone)]
pub struct CompileRequest {
    /// Guest RIP to compile
    pub rip: u64,
    /// Target tier
    pub target_tier: CompileTier,
    /// Current execution count (for priority)
    pub exec_count: u64,
    /// Guest code checksum (for validation)
    pub guest_checksum: u64,
    /// Guest code size
    pub guest_size: u32,
    /// Request timestamp (for timeout detection)
    pub requested_at: Instant,
    /// Request ID (for deduplication)
    pub request_id: u64,
    /// Priority
    pub priority: CompilePriority,
    /// Pre-decoded IR (optional, for S1→S2 promotion)
    pub ir_hint: Option<Arc<IrBlock>>,
    /// Profile data hint
    pub profile_hint: Option<ProfileHint>,
    /// Guest code bytes (copied from memory at request time)
    /// This allows async compilation without holding memory lock
    pub guest_code: Vec<u8>,
}

/// Profile hint for guided compilation
#[derive(Debug, Clone)]
pub struct ProfileHint {
    /// Branch taken probabilities: offset -> (taken_count, not_taken_count)
    pub branch_probs: Vec<(u32, u64, u64)>,
    /// Hot call targets: call_offset -> target_rip
    pub call_targets: Vec<(u32, u64)>,
    /// Loop back-edges: offset -> iteration_count
    pub loop_counts: Vec<(u32, u64)>,
}

impl CompileRequest {
    pub fn new_s1(rip: u64, guest_size: u32, guest_checksum: u64, exec_count: u64, guest_code: Vec<u8>) -> Self {
        Self {
            rip,
            target_tier: CompileTier::S1,
            exec_count,
            guest_checksum,
            guest_size,
            requested_at: Instant::now(),
            request_id: generate_request_id(),
            priority: CompilePriority::from_stats(exec_count, CompileTier::Interpreter),
            ir_hint: None,
            profile_hint: None,
            guest_code,
        }
    }
    
    pub fn new_s2(rip: u64, guest_size: u32, guest_checksum: u64, exec_count: u64, ir: Option<Arc<IrBlock>>, guest_code: Vec<u8>) -> Self {
        Self {
            rip,
            target_tier: CompileTier::S2,
            exec_count,
            guest_checksum,
            guest_size,
            requested_at: Instant::now(),
            request_id: generate_request_id(),
            priority: CompilePriority::from_stats(exec_count, CompileTier::S1),
            ir_hint: ir,
            profile_hint: None,
            guest_code,
        }
    }
    
    /// Check if request has timed out
    pub fn is_expired(&self) -> bool {
        self.requested_at.elapsed().as_secs() > COMPILATION_TIMEOUT_SECS
    }
}

// For priority queue ordering (higher priority = higher value)
impl PartialEq for CompileRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request_id == other.request_id
    }
}

impl Eq for CompileRequest {}

impl PartialOrd for CompileRequest {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for CompileRequest {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Higher priority first, then higher exec_count, then older request
        match self.priority.cmp(&other.priority) {
            CmpOrdering::Equal => {
                match self.exec_count.cmp(&other.exec_count) {
                    CmpOrdering::Equal => other.requested_at.cmp(&self.requested_at),
                    other => other,
                }
            }
            other => other,
        }
    }
}

// ============================================================================
// Compilation Result
// ============================================================================

/// Result of an asynchronous compilation
#[derive(Debug)]
pub struct CompileResult {
    /// Request ID
    pub request_id: u64,
    /// Guest RIP
    pub rip: u64,
    /// Target tier achieved
    pub tier: CompileTier,
    /// Native code (if successful)
    pub native_code: Option<Vec<u8>>,
    /// Guest instruction count
    pub guest_instrs: u32,
    /// Compilation duration
    pub compile_time: Duration,
    /// Error (if failed)
    pub error: Option<JitError>,
    /// IR representation (for later reuse)
    pub ir: Option<Arc<IrBlock>>,
}

impl CompileResult {
    pub fn success(
        request_id: u64,
        rip: u64,
        tier: CompileTier,
        native_code: Vec<u8>,
        guest_instrs: u32,
        compile_time: Duration,
        ir: Option<Arc<IrBlock>>,
    ) -> Self {
        Self {
            request_id,
            rip,
            tier,
            native_code: Some(native_code),
            guest_instrs,
            compile_time,
            error: None,
            ir,
        }
    }
    
    pub fn failure(request_id: u64, rip: u64, tier: CompileTier, error: JitError) -> Self {
        Self {
            request_id,
            rip,
            tier,
            native_code: None,
            guest_instrs: 0,
            compile_time: Duration::ZERO,
            error: Some(error),
            ir: None,
        }
    }
}

// ============================================================================
// Compilation Callback
// ============================================================================

/// Callback for compilation completion
pub trait CompileCallback: Send + Sync {
    /// Called when compilation completes (success or failure)
    fn on_compile_complete(&self, result: CompileResult);
}

/// Default callback that installs to CodeCache (for testing/simple use cases)
pub struct CodeCacheInstaller {
    cache: Arc<CodeCache>,
}

impl CodeCacheInstaller {
    pub fn new(cache: Arc<CodeCache>) -> Self {
        Self { cache }
    }
}

impl CompileCallback for CodeCacheInstaller {
    fn on_compile_complete(&self, result: CompileResult) {
        if let Some(native_code) = result.native_code {
            // Allocate executable memory and install
            if let Some(code_ptr) = self.cache.allocate_code(&native_code) {
                let block = CompiledBlock {
                    guest_rip: result.rip,
                    guest_size: 0, // Will be updated
                    host_code: code_ptr,
                    host_size: native_code.len() as u32,
                    tier: result.tier,
                    exec_count: AtomicU64::new(0),
                    last_access: AtomicU64::new(0),
                    guest_instrs: result.guest_instrs,
                    guest_checksum: 0,
                    depends_on: Vec::new(),
                    invalidated: false,
                };
                
                if let Err(e) = self.cache.insert(block) {
                    log::warn!("[AsyncJIT] Failed to install compiled block {:#x}: {:?}", result.rip, e);
                } else {
                    log::debug!("[AsyncJIT] Installed {:?} block {:#x} ({} bytes)", 
                        result.tier, result.rip, native_code.len());
                }
            }
        }
    }
}

/// Enhanced callback that installs to CodeCache AND updates BlockMeta tier
/// This is the production callback that ensures proper tier synchronization.
pub struct JitCompileCallback {
    cache: Arc<CodeCache>,
    blocks: Arc<RwLock<HashMap<u64, Arc<BlockMeta>>>>,
}

impl JitCompileCallback {
    pub fn new(
        cache: Arc<CodeCache>,
        blocks: Arc<RwLock<HashMap<u64, Arc<BlockMeta>>>>,
    ) -> Self {
        Self { cache, blocks }
    }
    
    /// Convert CompileTier to ExecutionTier
    fn compile_tier_to_execution_tier(tier: CompileTier) -> ExecutionTier {
        match tier {
            CompileTier::Interpreter => ExecutionTier::Interpreter,
            CompileTier::S1 => ExecutionTier::S1,
            CompileTier::S2 => ExecutionTier::S2,
        }
    }
}

impl CompileCallback for JitCompileCallback {
    fn on_compile_complete(&self, result: CompileResult) {
        if let Some(native_code) = result.native_code {
            // Allocate executable memory and install
            if let Some(code_ptr) = self.cache.allocate_code(&native_code) {
                let block = CompiledBlock {
                    guest_rip: result.rip,
                    guest_size: 0, // Will be updated
                    host_code: code_ptr,
                    host_size: native_code.len() as u32,
                    tier: result.tier,
                    exec_count: AtomicU64::new(0),
                    last_access: AtomicU64::new(0),
                    guest_instrs: result.guest_instrs,
                    guest_checksum: 0,
                    depends_on: Vec::new(),
                    invalidated: false,
                };
                
                if let Err(e) = self.cache.insert(block) {
                    log::warn!("[AsyncJIT] Failed to install compiled block {:#x}: {:?}", result.rip, e);
                } else {
                    // CRITICAL: Update BlockMeta tier to prevent duplicate submissions
                    let exec_tier = Self::compile_tier_to_execution_tier(result.tier);
                    if let Ok(blocks) = self.blocks.read() {
                        if let Some(block_meta) = blocks.get(&result.rip) {
                            block_meta.set_tier(exec_tier);
                        }
                    }
                    
                    log::debug!("[AsyncJIT] Installed {:?} block {:#x} ({} bytes)", 
                        result.tier, result.rip, native_code.len());
                }
            }
        }
    }
}

// ============================================================================
// Async Compilation Stats
// ============================================================================

/// Statistics for async compilation
#[derive(Debug, Default)]
pub struct AsyncCompileStats {
    /// Total requests submitted
    pub requests_submitted: AtomicU64,
    /// Requests completed successfully
    pub requests_completed: AtomicU64,
    /// Requests that failed
    pub requests_failed: AtomicU64,
    /// Requests dropped (expired or deduplicated)
    pub requests_dropped: AtomicU64,
    /// Total S1 compilations
    pub s1_compilations: AtomicU64,
    /// Total S2 compilations
    pub s2_compilations: AtomicU64,
    /// Total compilation time (microseconds)
    pub total_compile_time_us: AtomicU64,
    /// Current queue depth
    pub queue_depth: AtomicUsize,
    /// Peak queue depth
    pub peak_queue_depth: AtomicUsize,
    /// Active workers
    pub active_workers: AtomicUsize,
}

impl AsyncCompileStats {
    pub fn record_submit(&self) {
        self.requests_submitted.fetch_add(1, Ordering::Relaxed);
        let current = self.queue_depth.fetch_add(1, Ordering::Relaxed) + 1;
        let peak = self.peak_queue_depth.load(Ordering::Relaxed);
        if current > peak {
            self.peak_queue_depth.store(current, Ordering::Relaxed);
        }
    }
    
    pub fn record_complete(&self, tier: CompileTier, compile_time: Duration) {
        self.requests_completed.fetch_add(1, Ordering::Relaxed);
        self.queue_depth.fetch_sub(1, Ordering::Relaxed);
        self.total_compile_time_us.fetch_add(compile_time.as_micros() as u64, Ordering::Relaxed);
        
        match tier {
            CompileTier::S1 => self.s1_compilations.fetch_add(1, Ordering::Relaxed),
            CompileTier::S2 => self.s2_compilations.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }
    
    pub fn record_failure(&self) {
        self.requests_failed.fetch_add(1, Ordering::Relaxed);
        self.queue_depth.fetch_sub(1, Ordering::Relaxed);
    }
    
    pub fn record_drop(&self) {
        self.requests_dropped.fetch_add(1, Ordering::Relaxed);
        self.queue_depth.fetch_sub(1, Ordering::Relaxed);
    }
    
    pub fn avg_compile_time(&self) -> Duration {
        let completed = self.requests_completed.load(Ordering::Relaxed);
        if completed == 0 {
            return Duration::ZERO;
        }
        let total_us = self.total_compile_time_us.load(Ordering::Relaxed);
        Duration::from_micros(total_us / completed)
    }
}

// ============================================================================
// Compiler Context - Shared compilation infrastructure
// ============================================================================

/// Compiler context shared between worker threads
/// 
/// Contains all components needed to compile guest code.
/// Each component is thread-safe (either Send+Sync or internally synchronized).
pub struct CompilerContext {
    /// x86 decoder (stateless, thread-safe)
    pub decoder: X86Decoder,
    /// S1 quick compiler (stateless, thread-safe)
    pub s1_compiler: S1Compiler,
    /// S2 optimizing compiler (stateless, thread-safe)
    pub s2_compiler: S2Compiler,
    /// Native code generator (stateless, thread-safe)
    pub codegen: CodeGen,
    /// Profile database (internally synchronized)
    pub profile_db: Arc<ProfileDb>,
}

impl CompilerContext {
    /// Create new compiler context with default settings
    pub fn new(profile_db: Arc<ProfileDb>) -> Self {
        Self {
            decoder: X86Decoder::new(),
            s1_compiler: S1Compiler::new(),
            s2_compiler: S2Compiler::new(),
            codegen: CodeGen::new(),
            profile_db,
        }
    }
    
    /// Compile a block with S1 (quick) compiler
    pub fn compile_s1(&self, guest_code: &[u8], rip: u64) -> JitResult<(Vec<u8>, u32, Arc<IrBlock>)> {
        let s1_block = self.s1_compiler.compile(guest_code, rip, &self.decoder, &self.profile_db)?;
        let native = s1_block.native.clone();
        let guest_instrs: u32 = s1_block.ir.blocks.iter()
            .map(|bb| bb.instrs.len() as u32)
            .sum();
        let ir = Arc::new(s1_block.ir);
        Ok((native, guest_instrs, ir))
    }
    
    /// Compile a block with S2 (optimizing) compiler
    pub fn compile_s2(&self, guest_code: &[u8], rip: u64, ir_hint: Option<&IrBlock>) -> JitResult<(Vec<u8>, u32, Arc<IrBlock>)> {
        // First get S1 output (or use hint)
        let s1_block = self.s1_compiler.compile(guest_code, rip, &self.decoder, &self.profile_db)?;
        
        // Then optimize with S2
        let s2_block = self.s2_compiler.compile_from_s1(&s1_block, &self.profile_db)?;
        let native = s2_block.native.clone();
        let guest_instrs: u32 = s2_block.ir.blocks.iter()
            .map(|bb| bb.instrs.len() as u32)
            .sum();
        let ir = Arc::new(s2_block.ir);
        Ok((native, guest_instrs, ir))
    }
}

// ============================================================================
// Async JIT Runtime
// ============================================================================

/// Asynchronous JIT compilation runtime
/// 
/// Manages background compilation workers and priority queue.
/// VM execution never pauses - compilation happens in parallel.
pub struct AsyncJitRuntime {
    /// Priority queue for compilation requests
    queue: Arc<Mutex<BinaryHeap<CompileRequest>>>,
    /// Condition variable for worker wakeup
    queue_cv: Arc<Condvar>,
    /// In-flight requests (for deduplication): rip -> (request_id, target_tier)
    in_flight: Arc<RwLock<HashMap<u64, (u64, CompileTier)>>>,
    /// Worker threads
    workers: Vec<JoinHandle<()>>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Compilation callback
    callback: Arc<dyn CompileCallback>,
    /// Compiler context (shared between workers)
    compiler: Arc<CompilerContext>,
    /// Code cache reference (for tier checking)
    code_cache: Option<Arc<CodeCache>>,
    /// Statistics
    pub stats: Arc<AsyncCompileStats>,
    /// Worker count
    worker_count: usize,
}

impl AsyncJitRuntime {
    /// Create a new async JIT runtime
    pub fn new(
        callback: Arc<dyn CompileCallback>,
        compiler: Arc<CompilerContext>,
        worker_count: Option<usize>,
    ) -> Self {
        let count = worker_count.unwrap_or_else(|| {
            // Default: number of CPU cores, capped at 8
            std::thread::available_parallelism()
                .map(|p| p.get().min(8))
                .unwrap_or(4)
        });
        
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            queue_cv: Arc::new(Condvar::new()),
            in_flight: Arc::new(RwLock::new(HashMap::new())),
            workers: Vec::with_capacity(count),
            shutdown: Arc::new(AtomicBool::new(false)),
            callback,
            compiler,
            code_cache: None,
            stats: Arc::new(AsyncCompileStats::default()),
            worker_count: count,
        }
    }
    
    /// Set code cache reference for tier-aware deduplication
    pub fn set_code_cache(&mut self, cache: Arc<CodeCache>) {
        self.code_cache = Some(cache);
    }
    
    /// Start the runtime (spawns worker threads)
    pub fn start(&mut self) {
        log::info!("[AsyncJIT] Starting with {} worker threads", self.worker_count);
        
        for i in 0..self.worker_count {
            let queue = Arc::clone(&self.queue);
            let queue_cv = Arc::clone(&self.queue_cv);
            let in_flight = Arc::clone(&self.in_flight);
            let shutdown = Arc::clone(&self.shutdown);
            let callback = Arc::clone(&self.callback);
            let compiler = Arc::clone(&self.compiler);
            let stats = Arc::clone(&self.stats);
            
            let handle = thread::Builder::new()
                .name(format!("jit-worker-{}", i))
                .spawn(move || {
                    worker_loop(i, queue, queue_cv, in_flight, shutdown, callback, compiler, stats);
                })
                .expect("Failed to spawn JIT worker thread");
            
            self.workers.push(handle);
        }
    }
    
    /// Submit a compilation request (non-blocking)
    pub fn submit(&self, request: CompileRequest) -> bool {
        let target_tier = request.target_tier;
        
        // Check if CodeCache already has this tier (prevents recompilation spam)
        if let Some(ref cache) = self.code_cache {
            if let Some(block_info) = cache.get_block(request.rip) {
                if block_info.tier >= target_tier {
                    log::trace!("[AsyncJIT] Already compiled {:#x} at {:?} (requested {:?})",
                        request.rip, block_info.tier, target_tier);
                    return false;
                }
            }
        }
        
        // Check for duplicate in-flight request at same or higher tier
        {
            let in_flight = self.in_flight.read().unwrap();
            if let Some(&(_, existing_tier)) = in_flight.get(&request.rip) {
                if existing_tier >= target_tier {
                    log::trace!("[AsyncJIT] Deduplicated request for {:#x} (in-flight at {:?})",
                        request.rip, existing_tier);
                    return false;
                }
            }
        }
        
        // Check queue capacity
        let mut queue = self.queue.lock().unwrap();
        if queue.len() >= MAX_QUEUE_SIZE {
            log::warn!("[AsyncJIT] Queue full, dropping request for {:#x}", request.rip);
            return false;
        }
        
        // Mark as in-flight with target tier
        {
            let mut in_flight = self.in_flight.write().unwrap();
            in_flight.insert(request.rip, (request.request_id, target_tier));
        }
        
        self.stats.record_submit();
        let rip = request.rip;
        let priority = request.priority;
        
        queue.push(request);
        drop(queue);
        
        // Wake up a worker
        self.queue_cv.notify_one();
        
        log::trace!("[AsyncJIT] Submitted {:?} request for {:#x}", priority, rip);
        true
    }
    
    /// Submit S1 compilation request with guest code
    pub fn request_s1(&self, rip: u64, guest_size: u32, guest_checksum: u64, exec_count: u64, guest_code: Vec<u8>) -> bool {
        self.submit(CompileRequest::new_s1(rip, guest_size, guest_checksum, exec_count, guest_code))
    }
    
    /// Submit S2 compilation request with guest code
    pub fn request_s2(&self, rip: u64, guest_size: u32, guest_checksum: u64, exec_count: u64, ir: Option<Arc<IrBlock>>, guest_code: Vec<u8>) -> bool {
        self.submit(CompileRequest::new_s2(rip, guest_size, guest_checksum, exec_count, ir, guest_code))
    }
    
    /// Cancel a pending request (if not yet started)
    pub fn cancel(&self, rip: u64) -> bool {
        let mut in_flight = self.in_flight.write().unwrap();
        in_flight.remove(&rip).is_some()
    }
    
    /// Get current queue depth
    pub fn queue_depth(&self) -> usize {
        self.stats.queue_depth.load(Ordering::Relaxed)
    }
    
    /// Check if runtime is busy (queue depth > threshold)
    pub fn is_busy(&self, threshold: usize) -> bool {
        self.queue_depth() > threshold
    }
    
    /// Shutdown the runtime gracefully
    pub fn shutdown(&mut self) {
        log::info!("[AsyncJIT] Shutting down...");
        
        self.shutdown.store(true, Ordering::SeqCst);
        
        // Wake up all workers
        self.queue_cv.notify_all();
        
        // Wait for workers to finish
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        
        log::info!("[AsyncJIT] Shutdown complete. Stats: submitted={}, completed={}, failed={}",
            self.stats.requests_submitted.load(Ordering::Relaxed),
            self.stats.requests_completed.load(Ordering::Relaxed),
            self.stats.requests_failed.load(Ordering::Relaxed));
    }
    
    /// Get statistics snapshot
    pub fn get_stats(&self) -> AsyncStatsSnapshot {
        AsyncStatsSnapshot {
            requests_submitted: self.stats.requests_submitted.load(Ordering::Relaxed),
            requests_completed: self.stats.requests_completed.load(Ordering::Relaxed),
            requests_failed: self.stats.requests_failed.load(Ordering::Relaxed),
            requests_dropped: self.stats.requests_dropped.load(Ordering::Relaxed),
            s1_compilations: self.stats.s1_compilations.load(Ordering::Relaxed),
            s2_compilations: self.stats.s2_compilations.load(Ordering::Relaxed),
            queue_depth: self.stats.queue_depth.load(Ordering::Relaxed),
            peak_queue_depth: self.stats.peak_queue_depth.load(Ordering::Relaxed),
            active_workers: self.stats.active_workers.load(Ordering::Relaxed),
            avg_compile_time: self.stats.avg_compile_time(),
        }
    }
}

impl Drop for AsyncJitRuntime {
    fn drop(&mut self) {
        if !self.shutdown.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

/// Statistics snapshot
#[derive(Debug, Clone)]
pub struct AsyncStatsSnapshot {
    pub requests_submitted: u64,
    pub requests_completed: u64,
    pub requests_failed: u64,
    pub requests_dropped: u64,
    pub s1_compilations: u64,
    pub s2_compilations: u64,
    pub queue_depth: usize,
    pub peak_queue_depth: usize,
    pub active_workers: usize,
    pub avg_compile_time: Duration,
}

// ============================================================================
// Worker Thread
// ============================================================================

fn worker_loop(
    worker_id: usize,
    queue: Arc<Mutex<BinaryHeap<CompileRequest>>>,
    queue_cv: Arc<Condvar>,
    in_flight: Arc<RwLock<HashMap<u64, (u64, CompileTier)>>>,
    shutdown: Arc<AtomicBool>,
    callback: Arc<dyn CompileCallback>,
    compiler: Arc<CompilerContext>,
    stats: Arc<AsyncCompileStats>,
) {
    log::debug!("[AsyncJIT] Worker {} started", worker_id);
    
    loop {
        // Get next request
        let request = {
            let mut queue_guard = queue.lock().unwrap();
            
            // Wait for work or shutdown
            while queue_guard.is_empty() && !shutdown.load(Ordering::Relaxed) {
                queue_guard = queue_cv.wait(queue_guard).unwrap();
            }
            
            if shutdown.load(Ordering::Relaxed) && queue_guard.is_empty() {
                break;
            }
            
            queue_guard.pop()
        };
        
        let Some(request) = request else { continue };
        
        // Check if cancelled or superseded by higher-tier request
        {
            let in_flight_guard = in_flight.read().unwrap();
            match in_flight_guard.get(&request.rip) {
                Some(&(id, _tier)) if id == request.request_id => {
                    // Still valid - this is our request
                }
                _ => {
                    // Cancelled or superseded by another request
                    stats.record_drop();
                    continue;
                }
            }
        }
        
        // Check if expired
        if request.is_expired() {
            log::debug!("[AsyncJIT] Worker {}: Request {:#x} expired", worker_id, request.rip);
            stats.record_drop();
            
            // Remove from in-flight
            let mut in_flight_guard = in_flight.write().unwrap();
            in_flight_guard.remove(&request.rip);
            continue;
        }
        
        stats.active_workers.fetch_add(1, Ordering::Relaxed);
        
        // Compile using real compiler
        let start = Instant::now();
        let result = compile_block_with_context(worker_id, &request, &compiler);
        let compile_time = start.elapsed();
        
        // Remove from in-flight
        {
            let mut in_flight_guard = in_flight.write().unwrap();
            in_flight_guard.remove(&request.rip);
        }
        
        // Report result
        match &result.error {
            None => {
                stats.record_complete(result.tier, compile_time);
                log::debug!("[AsyncJIT] Worker {}: Compiled {:#x} ({:?}) in {:?}",
                    worker_id, request.rip, result.tier, compile_time);
            }
            Some(e) => {
                stats.record_failure();
                log::warn!("[AsyncJIT] Worker {}: Failed to compile {:#x}: {}",
                    worker_id, request.rip, e);
            }
        }
        
        // Callback (installs to cache)
        callback.on_compile_complete(result);
        
        stats.active_workers.fetch_sub(1, Ordering::Relaxed);
    }
    
    log::debug!("[AsyncJIT] Worker {} stopped", worker_id);
}

/// Compile a single block using real compilers
fn compile_block_with_context(
    _worker_id: usize, 
    request: &CompileRequest,
    compiler: &CompilerContext,
) -> CompileResult {
    let start = Instant::now();
    
    match request.target_tier {
        CompileTier::S1 => {
            // S1 compilation - quick baseline
            match compiler.compile_s1(&request.guest_code, request.rip) {
                Ok((native_code, guest_instrs, ir)) => {
                    CompileResult::success(
                        request.request_id,
                        request.rip,
                        CompileTier::S1,
                        native_code,
                        guest_instrs,
                        start.elapsed(),
                        Some(ir),
                    )
                }
                Err(e) => {
                    CompileResult::failure(
                        request.request_id,
                        request.rip,
                        CompileTier::S1,
                        e,
                    )
                }
            }
        }
        CompileTier::S2 => {
            // S2 compilation - optimizing
            let ir_hint = request.ir_hint.as_ref().map(|arc| arc.as_ref());
            match compiler.compile_s2(&request.guest_code, request.rip, ir_hint) {
                Ok((native_code, guest_instrs, ir)) => {
                    CompileResult::success(
                        request.request_id,
                        request.rip,
                        CompileTier::S2,
                        native_code,
                        guest_instrs,
                        start.elapsed(),
                        Some(ir),
                    )
                }
                Err(e) => {
                    CompileResult::failure(
                        request.request_id,
                        request.rip,
                        CompileTier::S2,
                        e,
                    )
                }
            }
        }
        CompileTier::Interpreter => {
            CompileResult::failure(
                request.request_id,
                request.rip,
                CompileTier::Interpreter,
                JitError::CompilationError("Cannot compile Interpreter tier".to_string()),
            )
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Generate unique request ID
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
    use std::sync::atomic::AtomicUsize;
    
    struct CountingCallback {
        count: AtomicUsize,
    }
    
    impl CompileCallback for CountingCallback {
        fn on_compile_complete(&self, _result: CompileResult) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    fn dummy_guest_code() -> Vec<u8> {
        vec![0x90, 0x90, 0x90, 0xC3] // NOP NOP NOP RET
    }
    
    #[test]
    fn test_priority_ordering() {
        let r1 = CompileRequest::new_s1(0x1000, 100, 0, 10, dummy_guest_code());
        let r2 = CompileRequest::new_s1(0x2000, 100, 0, 100, dummy_guest_code());
        let r3 = CompileRequest::new_s1(0x3000, 100, 0, 1000, dummy_guest_code());
        
        assert!(r3 > r2);
        assert!(r2 > r1);
    }
    
    #[test]
    fn test_async_runtime_basic() {
        let callback = Arc::new(CountingCallback { count: AtomicUsize::new(0) });
        let profile_db = Arc::new(ProfileDb::new(1000));
        let compiler = Arc::new(CompilerContext::new(profile_db));
        let mut runtime = AsyncJitRuntime::new(callback.clone(), compiler, Some(2));
        
        runtime.start();
        
        // Submit some requests
        runtime.request_s1(0x1000, 100, 12345, 50, dummy_guest_code());
        runtime.request_s1(0x2000, 100, 12346, 100, dummy_guest_code());
        runtime.request_s1(0x3000, 100, 12347, 200, dummy_guest_code());
        
        // Wait for completion
        std::thread::sleep(Duration::from_millis(200));
        
        // Check callbacks were invoked
        assert!(callback.count.load(Ordering::Relaxed) >= 1);
        
        runtime.shutdown();
    }
    
    #[test]
    fn test_deduplication() {
        let callback = Arc::new(CountingCallback { count: AtomicUsize::new(0) });
        let profile_db = Arc::new(ProfileDb::new(1000));
        let compiler = Arc::new(CompilerContext::new(profile_db));
        let runtime = AsyncJitRuntime::new(callback.clone(), compiler, Some(1));
        
        // Don't start workers - just test queue behavior
        
        // First request should succeed
        assert!(runtime.submit(CompileRequest::new_s1(0x1000, 100, 0, 50, dummy_guest_code())));
        
        // Duplicate should be rejected
        assert!(!runtime.submit(CompileRequest::new_s1(0x1000, 100, 0, 50, dummy_guest_code())));
    }
    
    #[test]
    fn test_priority_from_stats() {
        // Cold interpreter -> Normal priority
        assert_eq!(CompilePriority::from_stats(10, CompileTier::Interpreter), CompilePriority::Normal);
        
        // Hot interpreter -> High priority
        assert_eq!(CompilePriority::from_stats(100, CompileTier::Interpreter), CompilePriority::High);
        
        // Very hot interpreter -> Critical priority
        assert_eq!(CompilePriority::from_stats(1000, CompileTier::Interpreter), CompilePriority::Critical);
        
        // S1 promotion -> Lower priority
        assert_eq!(CompilePriority::from_stats(100, CompileTier::S1), CompilePriority::Low);
    }
}
