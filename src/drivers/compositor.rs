//! Parallel Display Compositor
//!
//! This module provides NUMA-aware parallel display composition that leverages
//! SMP to accelerate framebuffer rendering operations. It distributes rendering
//! work across multiple CPU cores after SMP and NUMA initialization completes.
//!
//! # Architecture
//!
//! ```text
//! +------------------+     +------------------+
//! |  CPU 0 (BSP)     |     |  CPU 1 (AP)      |
//! | Row 0-31         | <-> | Row 32-63        |
//! +------------------+     +------------------+
//!           |                      |
//!           v                      v
//!       +-------------------------------+
//!       |       Framebuffer Memory      |
//!       +-------------------------------+
//! ```
//!
//! # Features
//!
//! - NUMA-aware memory allocation for composition buffers
//! - Lock-free work distribution using atomic operations
//! - Per-CPU rendering queues to minimize contention
//! - Stripe-based parallelization for cache efficiency

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::smp;
use crate::numa;

// =============================================================================
// Configuration Constants
// =============================================================================

/// Maximum number of composition layers supported
pub const MAX_LAYERS: usize = 16;

/// Maximum number of pending composition tasks per CPU
pub const MAX_TASKS_PER_CPU: usize = 32;

/// Minimum rows per worker for effective parallelization
pub const MIN_ROWS_PER_WORKER: usize = 32;

/// Default stripe height for parallel composition
/// Increased for 2.5K+ screens to improve cache locality (64KB L1 cache line efficiency)
pub const DEFAULT_STRIPE_HEIGHT: usize = 64;

/// Threshold for using fast memset-style fill (in pixels)
const FAST_FILL_THRESHOLD: usize = 8;

/// Threshold for using batch pixel processing (increased for better cache utilization)
const BATCH_BLEND_THRESHOLD: usize = 16;

/// SIMD batch size (process 16 pixels at once with SSE/AVX)
const SIMD_BATCH_SIZE: usize = 16;

/// Cache line size for memory alignment (64 bytes on x86-64)
/// Used for future alignment optimizations
#[allow(dead_code)]
const CACHE_LINE_SIZE: usize = 64;

/// Prefetch distance (rows ahead to prefetch)
#[allow(dead_code)]
const PREFETCH_DISTANCE: usize = 8;

/// Threshold for parallel scroll (in total bytes to move)
/// For 2.5K (2560x1440x4bpp ≈ 14MB), always use parallel
const PARALLEL_SCROLL_THRESHOLD: usize = 512 * 1024; // 512KB (reduced for better parallelism)

// =============================================================================
// Multi-Core Work Distribution
// =============================================================================

/// Work type for parallel operations
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkType {
    /// No work pending
    None = 0,
    /// Scroll memory copy operation
    Scroll = 1,
    /// Fill memory operation
    Fill = 2,
    /// Composition operation
    Compose = 3,
}

impl WorkType {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => WorkType::Scroll,
            2 => WorkType::Fill,
            3 => WorkType::Compose,
            _ => WorkType::None,
        }
    }
}

/// Global work type indicator for AP cores
static WORK_TYPE: AtomicU8 = AtomicU8::new(0);

/// Number of workers that have joined current work
static WORKERS_JOINED: AtomicUsize = AtomicUsize::new(0);

/// Signal that work is available and AP cores should wake
static WORK_AVAILABLE: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Compositor State
// =============================================================================

/// Global compositor initialization flag
static COMPOSITOR_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Number of workers (online CPUs) available for composition
static WORKER_COUNT: AtomicUsize = AtomicUsize::new(1);

/// Composition task counter for statistics
static TOTAL_COMPOSITIONS: AtomicU64 = AtomicU64::new(0);

/// Current composition generation (for synchronization)
static COMPOSITION_GEN: AtomicU64 = AtomicU64::new(0);

/// Work distribution state
static WORK_NEXT_STRIPE: AtomicUsize = AtomicUsize::new(0);
static WORK_TOTAL_STRIPES: AtomicUsize = AtomicUsize::new(0);
static WORK_COMPLETED: AtomicUsize = AtomicUsize::new(0);
static WORK_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Scroll operation state for parallel processing
static SCROLL_SRC_ADDR: AtomicU64 = AtomicU64::new(0);
static SCROLL_DST_ADDR: AtomicU64 = AtomicU64::new(0);
static SCROLL_ROW_BYTES: AtomicUsize = AtomicUsize::new(0);
static SCROLL_PITCH: AtomicUsize = AtomicUsize::new(0);
static SCROLL_NEXT_ROW: AtomicUsize = AtomicUsize::new(0);
static SCROLL_TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);
static SCROLL_ROWS_DONE: AtomicUsize = AtomicUsize::new(0);

/// Fill operation state
static FILL_BUFFER_ADDR: AtomicU64 = AtomicU64::new(0);
static FILL_PITCH: AtomicUsize = AtomicUsize::new(0);
static FILL_WIDTH: AtomicUsize = AtomicUsize::new(0);
static FILL_BPP: AtomicUsize = AtomicUsize::new(0);
static FILL_COLOR: AtomicU32 = AtomicU32::new(0);
static FILL_NEXT_ROW: AtomicUsize = AtomicUsize::new(0);
static FILL_TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);
static FILL_ROWS_DONE: AtomicUsize = AtomicUsize::new(0);

/// Composition operation state for parallel rendering
static COMPOSE_DST_BUFFER: AtomicU64 = AtomicU64::new(0);
static COMPOSE_DST_PITCH: AtomicUsize = AtomicUsize::new(0);
static COMPOSE_DST_BPP: AtomicUsize = AtomicUsize::new(0);
static COMPOSE_SCREEN_WIDTH: AtomicUsize = AtomicUsize::new(0);
static COMPOSE_TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);
static COMPOSE_STRIPE_HEIGHT: AtomicUsize = AtomicUsize::new(0);
static COMPOSE_STRIPES_DONE: AtomicUsize = AtomicUsize::new(0);

/// Layer storage for parallel composition (fixed-size array for no_std)
/// AP cores read this during composition work
static mut COMPOSE_LAYERS: [CompositionLayer; MAX_LAYERS] = {
    const INIT: CompositionLayer = CompositionLayer::empty();
    [INIT; MAX_LAYERS]
};
static COMPOSE_LAYER_COUNT: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Composition Region
// =============================================================================

/// Defines a rectangular region for composition
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CompositionRegion {
    /// X coordinate of top-left corner
    pub x: u32,
    /// Y coordinate of top-left corner
    pub y: u32,
    /// Width of region
    pub width: u32,
    /// Height of region
    pub height: u32,
}

impl CompositionRegion {
    /// Create a new composition region
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Create a region covering the entire screen
    pub const fn full_screen(width: u32, height: u32) -> Self {
        Self { x: 0, y: 0, width, height }
    }

    /// Check if region is valid (non-zero area)
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Calculate area in pixels
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Check if this region intersects another
    pub fn intersects(&self, other: &Self) -> bool {
        !(self.x + self.width <= other.x
            || other.x + other.width <= self.x
            || self.y + self.height <= other.y
            || other.y + other.height <= self.y)
    }

    /// Calculate intersection with another region
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.intersects(other) {
            return None;
        }

        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);

        Some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

// =============================================================================
// Composition Layer
// =============================================================================

/// Layer blend mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendMode {
    /// Source replaces destination
    Opaque = 0,
    /// Alpha blending: dst = src * alpha + dst * (1 - alpha)
    Alpha = 1,
    /// Additive: dst = src + dst
    Additive = 2,
    /// Multiply: dst = src * dst
    Multiply = 3,
}

impl Default for BlendMode {
    fn default() -> Self {
        Self::Opaque
    }
}

/// A composition layer
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CompositionLayer {
    /// Layer enabled flag
    pub enabled: bool,
    /// Layer visibility
    pub visible: bool,
    /// Z-order (higher = on top)
    pub z_order: u16,
    /// Blend mode
    pub blend_mode: BlendMode,
    /// Global alpha (0-255)
    pub alpha: u8,
    /// Source region in layer buffer
    pub src_region: CompositionRegion,
    /// Destination region on screen
    pub dst_region: CompositionRegion,
    /// Buffer address (physical or virtual depending on context)
    pub buffer_addr: u64,
    /// Buffer pitch (bytes per row)
    pub buffer_pitch: u32,
    /// Bytes per pixel
    pub bpp: u8,
    /// NUMA node hint for memory allocation
    pub numa_node: u32,
}

impl CompositionLayer {
    /// Create an empty/disabled layer
    pub const fn empty() -> Self {
        Self {
            enabled: false,
            visible: false,
            z_order: 0,
            blend_mode: BlendMode::Opaque,
            alpha: 255,
            src_region: CompositionRegion::new(0, 0, 0, 0),
            dst_region: CompositionRegion::new(0, 0, 0, 0),
            buffer_addr: 0,
            buffer_pitch: 0,
            bpp: 0,
            numa_node: numa::NUMA_NO_NODE,
        }
    }

    /// Check if layer should be rendered
    pub fn should_render(&self) -> bool {
        self.enabled && self.visible && self.alpha > 0 && self.dst_region.is_valid()
    }
}

// =============================================================================
// Per-CPU Work State
// =============================================================================

/// Per-CPU compositor work state
#[repr(C, align(64))]  // Cache-line aligned to avoid false sharing
pub struct CpuWorkState {
    /// Current work generation this CPU is working on
    pub current_gen: AtomicU64,
    /// Number of stripes completed by this CPU
    pub stripes_completed: AtomicUsize,
    /// CPU is currently working
    pub working: AtomicBool,
    /// NUMA node this CPU belongs to
    pub numa_node: AtomicU32,
    /// Padding for cache line alignment
    _pad: [u8; 32],
}

impl CpuWorkState {
    pub const fn new() -> Self {
        Self {
            current_gen: AtomicU64::new(0),
            stripes_completed: AtomicUsize::new(0),
            working: AtomicBool::new(false),
            numa_node: AtomicU32::new(numa::NUMA_NO_NODE),
            _pad: [0; 32],
        }
    }

    pub fn reset(&self) {
        self.current_gen.store(0, Ordering::Release);
        self.stripes_completed.store(0, Ordering::Release);
        self.working.store(false, Ordering::Release);
    }
}

/// Per-CPU work states (MAX_CPUS from smp module)
static mut CPU_WORK_STATES: [CpuWorkState; smp::MAX_CPUS] = {
    const INIT: CpuWorkState = CpuWorkState::new();
    [INIT; smp::MAX_CPUS]
};

// =============================================================================
// Compositor Statistics
// =============================================================================

/// Compositor performance statistics
#[derive(Clone, Copy, Debug, Default)]
pub struct CompositorStats {
    /// Total compositions completed
    pub total_compositions: u64,
    /// Number of workers used in last composition
    pub last_worker_count: usize,
    /// Stripes processed in last composition
    pub last_stripes: usize,
    /// Whether parallel composition was used
    pub parallel_enabled: bool,
}

// =============================================================================
// Public API
// =============================================================================

/// Initialize the parallel compositor
///
/// This should be called after SMP and NUMA initialization completes.
/// The compositor will detect available CPUs and NUMA topology to
/// optimize work distribution.
pub fn init() {
    if COMPOSITOR_INITIALIZED.load(Ordering::SeqCst) {
        return;
    }

    // Wait for SMP to be ready
    let online_cpus = smp::online_cpus();
    WORKER_COUNT.store(online_cpus, Ordering::SeqCst);

    // Initialize per-CPU work states with NUMA affinity
    unsafe {
        for i in 0..online_cpus.min(smp::MAX_CPUS) {
            CPU_WORK_STATES[i].reset();
            
            // Get NUMA node for this CPU
            if numa::is_initialized() {
                let node_id = numa::cpu_to_node(i as u32);
                CPU_WORK_STATES[i].numa_node.store(node_id, Ordering::Release);
            }
        }
    }

    COMPOSITOR_INITIALIZED.store(true, Ordering::SeqCst);

    crate::kinfo!(
        "Compositor: Initialized with {} worker(s), NUMA-aware: {}",
        online_cpus,
        numa::is_initialized()
    );
}

/// Check if compositor is initialized
#[inline]
pub fn is_initialized() -> bool {
    COMPOSITOR_INITIALIZED.load(Ordering::Acquire)
}

/// Get number of available workers
#[inline]
pub fn worker_count() -> usize {
    WORKER_COUNT.load(Ordering::Acquire)
}

/// Get compositor statistics
pub fn stats() -> CompositorStats {
    CompositorStats {
        total_compositions: TOTAL_COMPOSITIONS.load(Ordering::Relaxed),
        last_worker_count: WORKER_COUNT.load(Ordering::Relaxed),
        last_stripes: WORK_TOTAL_STRIPES.load(Ordering::Relaxed),
        parallel_enabled: worker_count() > 1,
    }
}

/// Reset compositor statistics for benchmarking
/// 
/// Clears all per-CPU stripe counters and operation counters.
/// Useful for measuring performance of specific rendering operations.
pub fn reset_stats() {
    TOTAL_COMPOSITIONS.store(0, Ordering::Relaxed);
    COMPOSE_STRIPES_DONE.store(0, Ordering::Relaxed);
    SCROLL_ROWS_DONE.store(0, Ordering::Relaxed);
    FILL_ROWS_DONE.store(0, Ordering::Relaxed);
    
    // Reset per-CPU stats
    let online = smp::online_cpus();
    unsafe {
        for i in 0..online.min(smp::MAX_CPUS) {
            CPU_WORK_STATES[i].stripes_completed.store(0, Ordering::Relaxed);
        }
    }
}

// =============================================================================
// AP Core Work Entry Point
// =============================================================================

/// Entry point for AP cores when receiving IPI_CALL_FUNCTION
/// 
/// This function is called from the IPI handler to participate in parallel work.
/// AP cores will claim work atomically and process it until all work is done.
/// 
/// # Safety
/// 
/// This function accesses global mutable state but uses atomic operations
/// for synchronization. It must only be called from the IPI handler context.
pub fn ap_work_entry() {
    // Check if compositor is ready and work is available
    if !COMPOSITOR_INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    
    if !WORK_AVAILABLE.load(Ordering::Acquire) {
        return;
    }
    
    // Mark this worker as joined
    WORKERS_JOINED.fetch_add(1, Ordering::AcqRel);
    
    // Get work type and execute appropriate handler
    let work_type = WorkType::from_u8(WORK_TYPE.load(Ordering::Acquire));
    
    match work_type {
        WorkType::Scroll => {
            // Execute scroll work
            scroll_worker();
        }
        WorkType::Fill => {
            // Execute fill work
            fill_worker();
        }
        WorkType::Compose => {
            // Execute composition work - parallel rendering
            compose_worker_internal();
        }
        WorkType::None => {
            // No work to do
        }
    }
}

/// Internal compose worker called by AP cores during parallel composition
/// 
/// This worker claims stripes atomically and composites them using the
/// shared layer data stored in COMPOSE_LAYERS.
fn compose_worker_internal() -> usize {
    let dst_buffer = COMPOSE_DST_BUFFER.load(Ordering::Acquire) as *mut u8;
    let dst_pitch = COMPOSE_DST_PITCH.load(Ordering::Acquire);
    let dst_bpp = COMPOSE_DST_BPP.load(Ordering::Acquire);
    let screen_width = COMPOSE_SCREEN_WIDTH.load(Ordering::Acquire);
    let total_rows = COMPOSE_TOTAL_ROWS.load(Ordering::Acquire);
    let stripe_height = COMPOSE_STRIPE_HEIGHT.load(Ordering::Acquire);
    let layer_count = COMPOSE_LAYER_COUNT.load(Ordering::Acquire);
    
    // Safety: layers are set up before work is dispatched and not modified until completion
    let layers = unsafe { &COMPOSE_LAYERS[..layer_count] };
    
    let cpu_id = smp::current_cpu_id() as usize;
    let mut stripes_done = 0;
    
    // Update CPU work state
    unsafe {
        if cpu_id < smp::MAX_CPUS {
            CPU_WORK_STATES[cpu_id].working.store(true, Ordering::Release);
        }
    }
    
    // Process stripes until none remain
    while let Some((_stripe_idx, start_row, end_row)) = claim_work_stripe(stripe_height, total_rows) {
        compose_stripe(
            dst_buffer,
            dst_pitch,
            dst_bpp,
            start_row,
            end_row,
            layers,
            screen_width,
        );
        
        complete_stripe();
        COMPOSE_STRIPES_DONE.fetch_add(1, Ordering::AcqRel);
        stripes_done += 1;
    }
    
    // Update CPU work state
    unsafe {
        if cpu_id < smp::MAX_CPUS {
            CPU_WORK_STATES[cpu_id].working.store(false, Ordering::Release);
            CPU_WORK_STATES[cpu_id].stripes_completed.fetch_add(stripes_done, Ordering::Relaxed);
        }
    }
    
    stripes_done
}

/// Internal scroll worker - claims and processes rows
fn scroll_worker() -> usize {
    let src_base = SCROLL_SRC_ADDR.load(Ordering::Acquire) as *const u8;
    let dst_base = SCROLL_DST_ADDR.load(Ordering::Acquire) as *mut u8;
    let row_bytes = SCROLL_ROW_BYTES.load(Ordering::Acquire);
    let pitch = SCROLL_PITCH.load(Ordering::Acquire);
    
    // Process rows in batches for better cache utilization
    // Batch size tuned for L2 cache (256KB typical)
    let batch_size = (256 * 1024) / row_bytes.max(1);
    let batch_size = batch_size.max(16).min(128);
    
    let mut rows_done = 0;
    
    while let Some((start, end)) = claim_scroll_rows(batch_size) {
        // Copy rows in this batch
        for row in start..end {
            let offset = row * pitch;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src_base.add(offset),
                    dst_base.add(offset),
                    row_bytes,
                );
            }
        }
        rows_done += end - start;
        SCROLL_ROWS_DONE.fetch_add(end - start, Ordering::AcqRel);
    }
    
    rows_done
}

/// Internal fill worker - claims and processes rows  
fn fill_worker() -> usize {
    let buffer = FILL_BUFFER_ADDR.load(Ordering::Acquire) as *mut u8;
    let pitch = FILL_PITCH.load(Ordering::Acquire);
    let width = FILL_WIDTH.load(Ordering::Acquire);
    let bpp = FILL_BPP.load(Ordering::Acquire);
    let color = FILL_COLOR.load(Ordering::Acquire);
    let total_rows = FILL_TOTAL_ROWS.load(Ordering::Acquire);
    
    let batch_size = 32; // Fixed batch for fill
    let mut rows_done = 0;
    
    while let Some((start, end)) = claim_fill_rows(batch_size, total_rows) {
        // Fill rows in this batch
        for row in start..end {
            fill_single_row(buffer, pitch, width, bpp, row, color);
        }
        rows_done += end - start;
        FILL_ROWS_DONE.fetch_add(end - start, Ordering::AcqRel);
    }
    
    rows_done
}

/// Claim a batch of rows for fill operation
#[inline(always)]
fn claim_fill_rows(batch_size: usize, total: usize) -> Option<(usize, usize)> {
    loop {
        let current = FILL_NEXT_ROW.load(Ordering::Acquire);
        if current >= total {
            return None;
        }
        
        let end = (current + batch_size).min(total);
        if FILL_NEXT_ROW
            .compare_exchange_weak(current, end, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return Some((current, end));
        }
        core::hint::spin_loop();
    }
}

/// Fill a single row with color
#[inline(always)]
fn fill_single_row(buffer: *mut u8, pitch: usize, width: usize, bpp: usize, row: usize, color: u32) {
    let row_offset = row * pitch;
    
    if bpp == 4 {
        // Fast path: 32-bit color, use 64-bit writes
        let color64 = (color as u64) | ((color as u64) << 32);
        let qwords = width / 2;
        let remainder = width % 2;
        
        unsafe {
            let qword_ptr = buffer.add(row_offset) as *mut u64;
            for i in 0..qwords {
                qword_ptr.add(i).write(color64);
            }
            if remainder > 0 {
                let dword_ptr = buffer.add(row_offset + qwords * 8) as *mut u32;
                dword_ptr.write(color);
            }
        }
    } else {
        // Generic path
        let color_bytes = color.to_le_bytes();
        unsafe {
            for x in 0..width {
                let pixel_ptr = buffer.add(row_offset + x * bpp);
                for c in 0..bpp.min(4) {
                    pixel_ptr.add(c).write(color_bytes[c]);
                }
            }
        }
    }
}

/// Dispatch work to AP cores and wait for completion
/// 
/// This sends IPI_CALL_FUNCTION to all online AP cores and waits
/// until all work is completed.
fn dispatch_to_ap_cores() {
    let workers = worker_count();
    if workers <= 1 {
        return; // No AP cores to dispatch to
    }
    
    // Signal that work is available
    WORK_AVAILABLE.store(true, Ordering::Release);
    WORKERS_JOINED.store(0, Ordering::Release);
    
    // Memory fence to ensure all work parameters are visible
    core::sync::atomic::fence(Ordering::SeqCst);
    
    // Send IPI to all AP cores
    smp::send_ipi_broadcast(smp::IPI_CALL_FUNCTION);
}

/// Request a stripe of work to process
/// 
/// Returns (stripe_index, start_row, end_row) or None if no work available
/// Optimized: claim multiple stripes at once to reduce contention
fn claim_work_stripe(stripe_height: usize, total_rows: usize) -> Option<(usize, usize, usize)> {
    let total_stripes = (total_rows + stripe_height - 1) / stripe_height;
    
    // Adaptive batch size: claim 2 stripes if many available
    let batch_size = if total_stripes > 32 { 2 } else { 1 };
    
    let mut attempts = 0;
    loop {
        let current = WORK_NEXT_STRIPE.load(Ordering::Acquire);
        if current >= total_stripes {
            return None;
        }
        
        // Claim batch_size stripes, but only process the first one now
        let next = (current + batch_size).min(total_stripes);
        if WORK_NEXT_STRIPE
            .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            let start_row = current * stripe_height;
            let end_row = ((current + 1) * stripe_height).min(total_rows);
            return Some((current, start_row, end_row));
        }
        
        // Exponential backoff to reduce contention
        attempts += 1;
        if attempts > 10 {
            for _ in 0..(1 << (attempts - 10).min(6)) {
                core::hint::spin_loop();
            }
        } else {
            core::hint::spin_loop();
        }
    }
}

/// Mark a stripe as completed
fn complete_stripe() {
    WORK_COMPLETED.fetch_add(1, Ordering::AcqRel);
}

/// Wait for all stripes to complete
fn wait_for_completion() {
    let total = WORK_TOTAL_STRIPES.load(Ordering::Acquire);
    
    // Spin wait with backoff
    let mut backoff = 1u32;
    loop {
        let completed = WORK_COMPLETED.load(Ordering::Acquire);
        if completed >= total {
            break;
        }
        
        // Exponential backoff with cap
        for _ in 0..backoff {
            core::hint::spin_loop();
        }
        backoff = (backoff * 2).min(1024);
    }
}

/// Compose a single stripe of the framebuffer
/// 
/// This is the core composition function that processes one horizontal stripe.
/// Optimized with prefetching and reduced branching in inner loops.
fn compose_stripe(
    dst_buffer: *mut u8,
    dst_pitch: usize,
    dst_bpp: usize,
    start_row: usize,
    end_row: usize,
    layers: &[CompositionLayer],
    screen_width: usize,
) {
    // Pre-filter layers to avoid repeated checks (use fixed-size array for no_std)
    let mut active_layers: [Option<&CompositionLayer>; MAX_LAYERS] = [None; MAX_LAYERS];
    let mut active_count = 0;
    
    for layer in layers.iter() {
        if layer.should_render() && active_count < MAX_LAYERS {
            active_layers[active_count] = Some(layer);
            active_count += 1;
        }
    }
    
    if active_count == 0 {
        return;
    }
    
    // For each row in our stripe
    for row in start_row..end_row {
        let row_offset = row * dst_pitch;
        
        // Prefetch next row's destination if available
        if row + 1 < end_row {
            unsafe {
                let prefetch_offset = (row + 1) * dst_pitch;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    dst_buffer.add(prefetch_offset) as *const i8
                );
            }
        }
        
        // Process each active layer from bottom to top (by z_order)
        for i in 0..active_count {
            if let Some(layer) = active_layers[i] {
                // Check if this row intersects the layer's destination
                let layer_start_y = layer.dst_region.y as usize;
                let layer_end_y = layer_start_y + layer.dst_region.height as usize;
                
                if row < layer_start_y || row >= layer_end_y {
                    continue;
                }
                
                // Calculate source row in layer buffer
                let src_row = row - layer_start_y;
                let layer_x = layer.dst_region.x as usize;
                let layer_width = layer.dst_region.width as usize;
                
                // Bounds check
                if layer_x >= screen_width {
                    continue;
                }
                let actual_width = layer_width.min(screen_width - layer_x);
                
                // Calculate buffer addresses
                let dst_row_start = unsafe { 
                    dst_buffer.add(row_offset + layer_x * dst_bpp) 
                };
                
                let src_row_offset = src_row * layer.buffer_pitch as usize;
                let src_row_start = layer.buffer_addr as usize + src_row_offset;
                
                // Prefetch source data
                unsafe {
                    core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                        src_row_start as *const i8
                    );
                }
                
                // Composite based on blend mode
                match layer.blend_mode {
                    BlendMode::Opaque => {
                        // Direct copy (fast path)
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                src_row_start as *const u8,
                                dst_row_start,
                                actual_width * dst_bpp,
                            );
                        }
                    }
                    BlendMode::Alpha => {
                        // Alpha blending
                        let alpha = layer.alpha;
                        if alpha == 255 {
                            // Full opacity - same as opaque
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    src_row_start as *const u8,
                                    dst_row_start,
                                    actual_width * dst_bpp,
                                );
                            }
                        } else if alpha > 0 {
                            // Actual alpha blending
                            blend_row_alpha(
                                src_row_start as *const u8,
                                dst_row_start,
                                actual_width,
                                dst_bpp,
                                alpha,
                            );
                        }
                        // alpha == 0 -> skip entirely
                    }
                    BlendMode::Additive => {
                        blend_row_additive(
                            src_row_start as *const u8,
                            dst_row_start,
                            actual_width,
                            dst_bpp,
                        );
                    }
                    BlendMode::Multiply => {
                        blend_row_multiply(
                            src_row_start as *const u8,
                            dst_row_start,
                            actual_width,
                            dst_bpp,
                        );
                    }
                }
            }
        }
    }
}

/// Alpha blend a row of pixels - SIMD optimized version
/// 
/// Optimized with:
/// - 16-pixel batch processing with manual SIMD-style operations
/// - Prefetching for better cache performance
/// - Reduced memory access latency
/// - Multiplication by shift approximation
#[inline(always)]
fn blend_row_alpha(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
    alpha: u8,
) {
    if bpp != 4 || pixels < BATCH_BLEND_THRESHOLD {
        // Fallback for non-32bit or small regions
        blend_row_alpha_scalar(src, dst, pixels, bpp, alpha);
        return;
    }
    
    let alpha16 = alpha as u16;
    let inv_alpha16 = 255u16.wrapping_sub(alpha16);
    
    unsafe {
        let batch_count = pixels / SIMD_BATCH_SIZE;
        let remainder = pixels % SIMD_BATCH_SIZE;
        
        // Process 16 pixels at a time for better throughput
        for batch in 0..batch_count {
            let base_offset = batch * SIMD_BATCH_SIZE * 4;
            
            // Prefetch next batch
            if batch + 1 < batch_count {
                let prefetch_offset = (batch + 1) * SIMD_BATCH_SIZE * 4;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    dst.add(prefetch_offset) as *const i8
                );
            }
            
            // Unrolled loop for 16 pixels (64 bytes = 1 cache line)
            for p in 0..SIMD_BATCH_SIZE {
                let pixel_offset = base_offset + p * 4;
                
                // Load source and destination as u32 for better memory access
                let src_pixel = (src.add(pixel_offset) as *const u32).read_unaligned();
                let dst_pixel = (dst.add(pixel_offset) as *const u32).read_unaligned();
                
                // Extract RGB components (BGRA format)
                let sb = (src_pixel & 0xFF) as u16;
                let sg = ((src_pixel >> 8) & 0xFF) as u16;
                let sr = ((src_pixel >> 16) & 0xFF) as u16;
                
                let db = (dst_pixel & 0xFF) as u16;
                let dg = ((dst_pixel >> 8) & 0xFF) as u16;
                let dr = ((dst_pixel >> 16) & 0xFF) as u16;
                
                // Alpha blend with optimized fixed-point arithmetic
                let rb = ((sb * alpha16 + db * inv_alpha16 + 128) >> 8) as u32;
                let rg = ((sg * alpha16 + dg * inv_alpha16 + 128) >> 8) as u32;
                let rr = ((sr * alpha16 + dr * inv_alpha16 + 128) >> 8) as u32;
                
                // Pack result back to u32 and write
                let result = rb | (rg << 8) | (rr << 16) | (dst_pixel & 0xFF000000);
                (dst.add(pixel_offset) as *mut u32).write_unaligned(result);
            }
        }
        
        // Handle remaining pixels
        let remainder_offset = batch_count * SIMD_BATCH_SIZE * 4;
        for i in 0..remainder {
            let offset = remainder_offset + i * 4;
            let src_pixel = (src.add(offset) as *const u32).read_unaligned();
            let dst_pixel = (dst.add(offset) as *const u32).read_unaligned();
            
            let sb = (src_pixel & 0xFF) as u16;
            let sg = ((src_pixel >> 8) & 0xFF) as u16;
            let sr = ((src_pixel >> 16) & 0xFF) as u16;
            
            let db = (dst_pixel & 0xFF) as u16;
            let dg = ((dst_pixel >> 8) & 0xFF) as u16;
            let dr = ((dst_pixel >> 16) & 0xFF) as u16;
            
            let rb = ((sb * alpha16 + db * inv_alpha16 + 128) >> 8) as u32;
            let rg = ((sg * alpha16 + dg * inv_alpha16 + 128) >> 8) as u32;
            let rr = ((sr * alpha16 + dr * inv_alpha16 + 128) >> 8) as u32;
            
            let result = rb | (rg << 8) | (rr << 16) | (dst_pixel & 0xFF000000);
            (dst.add(offset) as *mut u32).write_unaligned(result);
        }
    }
}

/// Scalar fallback for alpha blending (non-32bit or small regions)
#[inline(always)]
fn blend_row_alpha_scalar(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
    alpha: u8,
) {
    let alpha16 = alpha as u16;
    let inv_alpha16 = 255u16.wrapping_sub(alpha16);
    let bytes_per_pixel = bpp.min(3);
    
    unsafe {
        for i in 0..pixels {
            let offset = i * bpp;
            for c in 0..bytes_per_pixel {
                let s = *src.add(offset + c) as u16;
                let d = *dst.add(offset + c) as u16;
                let result = ((s * alpha16 + d * inv_alpha16 + 128) >> 8) as u8;
                *dst.add(offset + c) = result;
            }
        }
    }
}

/// Additive blend a row of pixels
/// 
/// Optimized with saturating addition and 16-pixel batch processing
#[inline(always)]
fn blend_row_additive(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
) {
    if bpp != 4 || pixels < BATCH_BLEND_THRESHOLD {
        blend_row_additive_scalar(src, dst, pixels, bpp);
        return;
    }
    
    unsafe {
        let batch_count = pixels / SIMD_BATCH_SIZE;
        let remainder = pixels % SIMD_BATCH_SIZE;
        
        for batch in 0..batch_count {
            let base_offset = batch * SIMD_BATCH_SIZE * 4;
            
            // Prefetch next batch
            if batch + 1 < batch_count {
                let prefetch_offset = (batch + 1) * SIMD_BATCH_SIZE * 4;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
            }
            
            for p in 0..SIMD_BATCH_SIZE {
                let offset = base_offset + p * 4;
                let r = (*src.add(offset)).saturating_add(*dst.add(offset));
                let g = (*src.add(offset + 1)).saturating_add(*dst.add(offset + 1));
                let b = (*src.add(offset + 2)).saturating_add(*dst.add(offset + 2));
                *dst.add(offset) = r;
                *dst.add(offset + 1) = g;
                *dst.add(offset + 2) = b;
            }
        }
        
        // Handle remainder
        let remainder_offset = batch_count * SIMD_BATCH_SIZE * 4;
        for i in 0..remainder {
            let offset = remainder_offset + i * 4;
            let r = (*src.add(offset)).saturating_add(*dst.add(offset));
            let g = (*src.add(offset + 1)).saturating_add(*dst.add(offset + 1));
            let b = (*src.add(offset + 2)).saturating_add(*dst.add(offset + 2));
            *dst.add(offset) = r;
            *dst.add(offset + 1) = g;
            *dst.add(offset + 2) = b;
        }
    }
}

#[inline(always)]
fn blend_row_additive_scalar(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
) {
    let bytes_per_pixel = bpp.min(3);
    unsafe {
        for i in 0..pixels {
            let offset = i * bpp;
            for c in 0..bytes_per_pixel {
                let result = (*src.add(offset + c)).saturating_add(*dst.add(offset + c));
                *dst.add(offset + c) = result;
            }
        }
    }
}

/// Multiply blend a row of pixels
/// 
/// Optimized with approximate division and 16-pixel batch processing
#[inline(always)]
fn blend_row_multiply(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
) {
    if bpp != 4 || pixels < BATCH_BLEND_THRESHOLD {
        blend_row_multiply_scalar(src, dst, pixels, bpp);
        return;
    }
    
    unsafe {
        let batch_count = pixels / SIMD_BATCH_SIZE;
        let remainder = pixels % SIMD_BATCH_SIZE;
        
        for batch in 0..batch_count {
            let base_offset = batch * SIMD_BATCH_SIZE * 4;
            
            // Prefetch next batch
            if batch + 1 < batch_count {
                let prefetch_offset = (batch + 1) * SIMD_BATCH_SIZE * 4;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
            }
            
            for p in 0..SIMD_BATCH_SIZE {
                let offset = base_offset + p * 4;
                let s0 = *src.add(offset) as u16;
                let d0 = *dst.add(offset) as u16;
                *dst.add(offset) = ((s0 * d0 + 128) >> 8) as u8;
                
                let s1 = *src.add(offset + 1) as u16;
                let d1 = *dst.add(offset + 1) as u16;
                *dst.add(offset + 1) = ((s1 * d1 + 128) >> 8) as u8;
                
                let s2 = *src.add(offset + 2) as u16;
                let d2 = *dst.add(offset + 2) as u16;
                *dst.add(offset + 2) = ((s2 * d2 + 128) >> 8) as u8;
            }
        }
        
        // Handle remainder
        let remainder_offset = batch_count * SIMD_BATCH_SIZE * 4;
        for i in 0..remainder {
            let offset = remainder_offset + i * 4;
            let s0 = *src.add(offset) as u16;
            let d0 = *dst.add(offset) as u16;
            *dst.add(offset) = ((s0 * d0 + 128) >> 8) as u8;
            
            let s1 = *src.add(offset + 1) as u16;
            let d1 = *dst.add(offset + 1) as u16;
            *dst.add(offset + 1) = ((s1 * d1 + 128) >> 8) as u8;
            
            let s2 = *src.add(offset + 2) as u16;
            let d2 = *dst.add(offset + 2) as u16;
            *dst.add(offset + 2) = ((s2 * d2 + 128) >> 8) as u8;
        }
    }
}

#[inline(always)]
fn blend_row_multiply_scalar(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
) {
    let bytes_per_pixel = bpp.min(3);
    unsafe {
        for i in 0..pixels {
            let offset = i * bpp;
            for c in 0..bytes_per_pixel {
                let s = *src.add(offset + c) as u16;
                let d = *dst.add(offset + c) as u16;
                let result = ((s * d + 128) >> 8) as u8;
                *dst.add(offset + c) = result;
            }
        }
    }
}

/// Worker function for parallel composition
/// 
/// This is called by each CPU participating in composition.
/// Each worker claims stripes atomically until all work is done.
pub fn worker_compose(
    dst_buffer: *mut u8,
    dst_pitch: usize,
    dst_bpp: usize,
    total_rows: usize,
    layers: &[CompositionLayer],
    screen_width: usize,
    stripe_height: usize,
) -> usize {
    let cpu_id = smp::current_cpu_id() as usize;
    let mut stripes_done = 0;
    
    // Update CPU work state
    unsafe {
        if cpu_id < smp::MAX_CPUS {
            CPU_WORK_STATES[cpu_id].working.store(true, Ordering::Release);
        }
    }
    
    // Process stripes until none remain
    while let Some((_stripe_idx, start_row, end_row)) = claim_work_stripe(stripe_height, total_rows) {
        compose_stripe(
            dst_buffer,
            dst_pitch,
            dst_bpp,
            start_row,
            end_row,
            layers,
            screen_width,
        );
        
        complete_stripe();
        stripes_done += 1;
    }
    
    // Update CPU work state
    unsafe {
        if cpu_id < smp::MAX_CPUS {
            CPU_WORK_STATES[cpu_id].working.store(false, Ordering::Release);
            CPU_WORK_STATES[cpu_id].stripes_completed.fetch_add(stripes_done, Ordering::Relaxed);
        }
    }
    
    stripes_done
}

/// Perform parallel composition
/// 
/// This is the main entry point for compositing layers onto the framebuffer.
/// It distributes work across available CPUs using stripe-based parallelization.
/// 
/// # Arguments
/// 
/// * `dst_buffer` - Destination framebuffer address
/// * `dst_pitch` - Bytes per row in destination
/// * `dst_bpp` - Bytes per pixel in destination
/// * `width` - Screen width in pixels
/// * `height` - Screen height in pixels
/// * `layers` - Slice of layers to composite (must be sorted by z_order)
/// 
/// # Returns
/// 
/// Number of stripes processed
pub fn compose(
    dst_buffer: *mut u8,
    dst_pitch: usize,
    dst_bpp: usize,
    width: usize,
    height: usize,
    layers: &[CompositionLayer],
) -> usize {
    if !is_initialized() {
        // Fallback: single-threaded composition
        compose_stripe(dst_buffer, dst_pitch, dst_bpp, 0, height, layers, width);
        return 1;
    }
    
    // Check if parallel composition is beneficial
    let workers = worker_count();
    if workers <= 1 || height < MIN_ROWS_PER_WORKER * 2 {
        // Single-threaded is more efficient for small regions
        compose_stripe(dst_buffer, dst_pitch, dst_bpp, 0, height, layers, width);
        TOTAL_COMPOSITIONS.fetch_add(1, Ordering::Relaxed);
        return 1;
    }
    
    // Calculate stripe height for good load balancing
    // Aim for 4x more stripes than workers for dynamic load balancing
    let stripe_height = DEFAULT_STRIPE_HEIGHT.max(height / (workers * 4));
    let total_stripes = (height + stripe_height - 1) / stripe_height;
    
    // Copy layers to static storage for AP cores to access
    // Safety: we hold exclusive access during setup phase before dispatching
    let layer_count = layers.len().min(MAX_LAYERS);
    unsafe {
        for i in 0..layer_count {
            COMPOSE_LAYERS[i] = layers[i];
        }
        // Clear remaining slots
        for i in layer_count..MAX_LAYERS {
            COMPOSE_LAYERS[i] = CompositionLayer::empty();
        }
    }
    COMPOSE_LAYER_COUNT.store(layer_count, Ordering::Release);
    
    // Setup composition parameters for AP cores
    COMPOSE_DST_BUFFER.store(dst_buffer as u64, Ordering::Release);
    COMPOSE_DST_PITCH.store(dst_pitch, Ordering::Release);
    COMPOSE_DST_BPP.store(dst_bpp, Ordering::Release);
    COMPOSE_SCREEN_WIDTH.store(width, Ordering::Release);
    COMPOSE_TOTAL_ROWS.store(height, Ordering::Release);
    COMPOSE_STRIPE_HEIGHT.store(stripe_height, Ordering::Release);
    COMPOSE_STRIPES_DONE.store(0, Ordering::Release);
    
    // Initialize work distribution
    WORK_NEXT_STRIPE.store(0, Ordering::Release);
    WORK_TOTAL_STRIPES.store(total_stripes, Ordering::Release);
    WORK_COMPLETED.store(0, Ordering::Release);
    WORK_IN_PROGRESS.store(true, Ordering::Release);
    
    // Set work type for AP cores
    WORK_TYPE.store(WorkType::Compose as u8, Ordering::Release);
    
    // Increment generation for this composition
    COMPOSITION_GEN.fetch_add(1, Ordering::SeqCst);
    
    // Memory fence to ensure all parameters are visible to AP cores
    core::sync::atomic::fence(Ordering::SeqCst);
    
    // Dispatch work to AP cores via IPI
    dispatch_to_ap_cores();
    
    // BSP participates in work (using internal worker that reads shared params)
    let bsp_stripes = compose_worker_internal();
    
    // Wait for all work to complete
    wait_for_completion();
    
    // Clear work flags
    WORK_AVAILABLE.store(false, Ordering::Release);
    WORK_TYPE.store(WorkType::None as u8, Ordering::Release);
    WORK_IN_PROGRESS.store(false, Ordering::Release);
    TOTAL_COMPOSITIONS.fetch_add(1, Ordering::Relaxed);
    
    bsp_stripes
}

/// Fill a region with a solid color (parallel version)
/// 
/// Optimized with:
/// - Row-level batch filling using write_bytes/copy_nonoverlapping
/// - First row as template for subsequent rows
/// - Reduced volatile writes (only first row is volatile)
pub fn fill_rect(
    dst_buffer: *mut u8,
    dst_pitch: usize,
    dst_bpp: usize,
    region: CompositionRegion,
    color: u32,
) {
    let x = region.x as usize;
    let y = region.y as usize;
    let width = region.width as usize;
    let height = region.height as usize;
    
    if width == 0 || height == 0 {
        return;
    }
    
    let row_bytes = width * dst_bpp;
    let color_bytes = color.to_le_bytes();
    
    // For small fills or non-32bit formats, use simple loop
    if width < FAST_FILL_THRESHOLD || dst_bpp != 4 {
        for row in y..(y + height) {
            let row_offset = row * dst_pitch + x * dst_bpp;
            for col in 0..width {
                let pixel_offset = row_offset + col * dst_bpp;
                unsafe {
                    for c in 0..dst_bpp.min(4) {
                        dst_buffer.add(pixel_offset + c).write_volatile(color_bytes[c]);
                    }
                }
            }
        }
        return;
    }
    
    // Fast path: fill first row, then copy to remaining rows
    unsafe {
        let first_row_ptr = dst_buffer.add(y * dst_pitch + x * dst_bpp);
        
        // Fill first row with 32-bit writes (assuming BGRA format)
        let pixel_ptr = first_row_ptr as *mut u32;
        for col in 0..width {
            pixel_ptr.add(col).write_volatile(color);
        }
        
        // Copy first row to remaining rows (much faster than pixel-by-pixel)
        for row in 1..height {
            let dst_row_ptr = dst_buffer.add((y + row) * dst_pitch + x * dst_bpp);
            core::ptr::copy_nonoverlapping(first_row_ptr, dst_row_ptr, row_bytes);
        }
    }
}

/// Copy a rectangular region (parallel version)
/// 
/// Optimized with prefetch hints and aligned copies when possible
#[inline(always)]
pub fn copy_rect(
    src_buffer: *const u8,
    src_pitch: usize,
    dst_buffer: *mut u8,
    dst_pitch: usize,
    bpp: usize,
    src_region: CompositionRegion,
    dst_x: usize,
    dst_y: usize,
) {
    let width = src_region.width as usize;
    let height = src_region.height as usize;
    let src_x = src_region.x as usize;
    let src_y = src_region.y as usize;
    let row_bytes = width * bpp;
    
    if width == 0 || height == 0 {
        return;
    }
    
    // Check if same pitch and can use single copy (contiguous rows)
    if src_pitch == dst_pitch && src_pitch == row_bytes {
        // Single contiguous copy - most efficient
        let src_offset = src_y * src_pitch + src_x * bpp;
        let dst_offset = dst_y * dst_pitch + dst_x * bpp;
        unsafe {
            core::ptr::copy_nonoverlapping(
                src_buffer.add(src_offset),
                dst_buffer.add(dst_offset),
                row_bytes * height,
            );
        }
        return;
    }
    
    // Row-by-row copy
    for row in 0..height {
        let src_offset = (src_y + row) * src_pitch + src_x * bpp;
        let dst_offset = (dst_y + row) * dst_pitch + dst_x * bpp;
        
        unsafe {
            core::ptr::copy_nonoverlapping(
                src_buffer.add(src_offset),
                dst_buffer.add(dst_offset),
                row_bytes,
            );
        }
    }
}

// =============================================================================
// High-Performance Scroll Operations
// =============================================================================

/// Claim a batch of rows for parallel scroll processing
/// Returns (start_row, end_row) or None if no more work
#[inline(always)]
fn claim_scroll_rows(batch_size: usize) -> Option<(usize, usize)> {
    let total = SCROLL_TOTAL_ROWS.load(Ordering::Acquire);
    
    loop {
        let current = SCROLL_NEXT_ROW.load(Ordering::Acquire);
        if current >= total {
            return None;
        }
        
        let end = (current + batch_size).min(total);
        if SCROLL_NEXT_ROW
            .compare_exchange_weak(current, end, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return Some((current, end));
        }
        core::hint::spin_loop();
    }
}

/// High-performance scroll up operation
/// 
/// This is optimized for large screens (2.5K+) with:
/// - Multi-core parallel row copying
/// - Cache-aware batch sizing
/// - Reduced memory barrier overhead
/// 
/// # Arguments
/// * `buffer` - Framebuffer address
/// * `pitch` - Bytes per row (including padding)
/// * `width` - Width in pixels
/// * `height` - Total height in pixels  
/// * `bpp` - Bytes per pixel
/// * `scroll_rows` - Number of rows to scroll up
/// * `clear_color` - Color to fill newly exposed area (packed u32)
pub fn scroll_up_fast(
    buffer: *mut u8,
    pitch: usize,
    width: usize,
    height: usize,
    bpp: usize,
    scroll_rows: usize,
    clear_color: u32,
) {
    if scroll_rows == 0 || scroll_rows >= height {
        return;
    }
    
    let row_bytes = width * bpp;
    let total_bytes = (height - scroll_rows) * pitch;
    let rows_to_copy = height - scroll_rows;
    
    // Decide between single-core and parallel based on data size
    let workers = worker_count();
    let use_parallel = is_initialized() 
        && workers > 1 
        && total_bytes >= PARALLEL_SCROLL_THRESHOLD
        && rows_to_copy >= workers * 32;  // At least 32 rows per worker
    
    if use_parallel {
        // === Parallel scroll path - dispatch to all cores ===
        
        // Setup scroll parameters for workers
        let src_addr = unsafe { buffer.add(scroll_rows * pitch) as u64 };
        let dst_addr = buffer as u64;
        
        // Set work type first (before parameters)
        WORK_TYPE.store(WorkType::Scroll as u8, Ordering::Release);
        
        SCROLL_SRC_ADDR.store(src_addr, Ordering::Release);
        SCROLL_DST_ADDR.store(dst_addr, Ordering::Release);
        SCROLL_ROW_BYTES.store(row_bytes, Ordering::Release);
        SCROLL_PITCH.store(pitch, Ordering::Release);
        SCROLL_NEXT_ROW.store(0, Ordering::Release);
        SCROLL_TOTAL_ROWS.store(rows_to_copy, Ordering::Release);
        SCROLL_ROWS_DONE.store(0, Ordering::Release);
        
        // Memory fence to ensure all stores are visible to AP cores
        core::sync::atomic::fence(Ordering::SeqCst);
        
        // Dispatch work to AP cores via IPI
        dispatch_to_ap_cores();
        
        // BSP also participates in the work
        scroll_worker();
        
        // Wait for all rows to complete (AP cores may still be working)
        let mut backoff = 1u32;
        while SCROLL_ROWS_DONE.load(Ordering::Acquire) < rows_to_copy {
            for _ in 0..backoff {
                core::hint::spin_loop();
            }
            backoff = (backoff * 2).min(1024);
        }
        
        // Clear work available flag
        WORK_AVAILABLE.store(false, Ordering::Release);
        WORK_TYPE.store(WorkType::None as u8, Ordering::Release);
    } else {
        // === Single-core optimized path ===
        // Use large block copy for better memory bandwidth
        unsafe {
            let src = buffer.add(scroll_rows * pitch);
            core::ptr::copy(src, buffer, total_bytes);
        }
    }
    
    // Clear the newly exposed area at bottom
    // This is always relatively small, so single-threaded is fine
    let clear_start_row = height - scroll_rows;
    clear_rows_fast(buffer, pitch, width, bpp, clear_start_row, scroll_rows, clear_color);
}

/// Fast row clearing using 64-bit writes
/// 
/// Optimized for clearing the bottom portion after scroll
#[inline(always)]
fn clear_rows_fast(
    buffer: *mut u8,
    pitch: usize,
    width: usize,
    bpp: usize,
    start_row: usize,
    num_rows: usize,
    color: u32,
) {
    if num_rows == 0 {
        return;
    }
    
    let row_bytes = width * bpp;
    
    // For 32-bit color, use 64-bit writes (2 pixels at a time)
    if bpp == 4 {
        // Create 64-bit pattern (2 pixels)
        let color64 = (color as u64) | ((color as u64) << 32);
        let qwords_per_row = row_bytes / 8;
        let remainder_bytes = row_bytes % 8;
        
        unsafe {
            // Fill first row
            let first_row_ptr = buffer.add(start_row * pitch);
            let qword_ptr = first_row_ptr as *mut u64;
            
            // Write 64-bit patterns
            for i in 0..qwords_per_row {
                qword_ptr.add(i).write_volatile(color64);
            }
            
            // Handle remainder (0-7 bytes)
            if remainder_bytes >= 4 {
                let dword_ptr = first_row_ptr.add(qwords_per_row * 8) as *mut u32;
                dword_ptr.write_volatile(color);
            }
            
            // Copy first row to remaining rows
            for row in 1..num_rows {
                let dst_row_ptr = buffer.add((start_row + row) * pitch);
                core::ptr::copy_nonoverlapping(first_row_ptr, dst_row_ptr, row_bytes);
            }
        }
    } else {
        // Generic path for other formats
        let color_bytes = color.to_le_bytes();
        unsafe {
            let first_row_ptr = buffer.add(start_row * pitch);
            
            // Fill first row pixel by pixel
            for x in 0..width {
                let pixel_ptr = first_row_ptr.add(x * bpp);
                for c in 0..bpp.min(4) {
                    pixel_ptr.add(c).write_volatile(color_bytes[c]);
                }
            }
            
            // Copy to remaining rows
            for row in 1..num_rows {
                let dst_row_ptr = buffer.add((start_row + row) * pitch);
                core::ptr::copy_nonoverlapping(first_row_ptr, dst_row_ptr, row_bytes);
            }
        }
    }
}

/// Parallel memory fill for large regions
/// 
/// Uses multiple cores to fill large framebuffer regions via IPI dispatch
pub fn parallel_fill(
    buffer: *mut u8,
    pitch: usize,
    width: usize,
    height: usize,
    bpp: usize,
    color: u32,
) {
    let total_bytes = height * pitch;
    let workers = worker_count();
    
    // For small regions or single core, use simple fill
    if !is_initialized() || workers <= 1 || total_bytes < PARALLEL_SCROLL_THRESHOLD {
        clear_rows_fast(buffer, pitch, width, bpp, 0, height, color);
        return;
    }
    
    // Minimum rows for parallel to be worthwhile
    if height < workers * 16 {
        clear_rows_fast(buffer, pitch, width, bpp, 0, height, color);
        return;
    }
    
    // === Parallel fill path - dispatch to all cores ===
    
    // Set work type first (before parameters)
    WORK_TYPE.store(WorkType::Fill as u8, Ordering::Release);
    
    // Setup fill parameters for workers
    FILL_BUFFER_ADDR.store(buffer as u64, Ordering::Release);
    FILL_PITCH.store(pitch, Ordering::Release);
    FILL_WIDTH.store(width, Ordering::Release);
    FILL_BPP.store(bpp, Ordering::Release);
    FILL_COLOR.store(color, Ordering::Release);
    FILL_NEXT_ROW.store(0, Ordering::Release);
    FILL_TOTAL_ROWS.store(height, Ordering::Release);
    FILL_ROWS_DONE.store(0, Ordering::Release);
    
    // Memory fence to ensure all stores are visible to AP cores
    core::sync::atomic::fence(Ordering::SeqCst);
    
    // Dispatch work to AP cores via IPI
    dispatch_to_ap_cores();
    
    // BSP also participates in the work
    fill_worker();
    
    // Wait for all rows to complete (AP cores may still be working)
    let mut backoff = 1u32;
    while FILL_ROWS_DONE.load(Ordering::Acquire) < height {
        for _ in 0..backoff {
            core::hint::spin_loop();
        }
        backoff = (backoff * 2).min(1024);
    }
    
    // Clear work available flag
    WORK_AVAILABLE.store(false, Ordering::Release);
    WORK_TYPE.store(WorkType::None as u8, Ordering::Release);
}

/// Print compositor debug information
pub fn debug_info() {
    if !is_initialized() {
        crate::kinfo!("Compositor: Not initialized");
        return;
    }
    
    let stats = stats();
    crate::kinfo!("Compositor Status:");
    crate::kinfo!("  Workers: {}", stats.last_worker_count);
    crate::kinfo!("  Total compositions: {}", stats.total_compositions);
    crate::kinfo!("  Parallel enabled: {}", stats.parallel_enabled);
    crate::kinfo!("  NUMA nodes: {}", numa::node_count());
    crate::kinfo!("  Default stripe height: {} rows", DEFAULT_STRIPE_HEIGHT);
    
    // Last operation stats
    let compose_stripes = COMPOSE_STRIPES_DONE.load(Ordering::Relaxed);
    let scroll_rows = SCROLL_ROWS_DONE.load(Ordering::Relaxed);
    let fill_rows = FILL_ROWS_DONE.load(Ordering::Relaxed);
    crate::kinfo!("  Last compose stripes: {}", compose_stripes);
    crate::kinfo!("  Last scroll rows: {}", scroll_rows);
    crate::kinfo!("  Last fill rows: {}", fill_rows);
    
    // Per-CPU stats
    let online = smp::online_cpus();
    let mut total_stripes = 0usize;
    for i in 0..online.min(smp::MAX_CPUS) {
        unsafe {
            let state = &CPU_WORK_STATES[i];
            let node = state.numa_node.load(Ordering::Relaxed);
            let stripes = state.stripes_completed.load(Ordering::Relaxed);
            total_stripes += stripes;
            let node_str = if node == numa::NUMA_NO_NODE {
                "N/A"
            } else {
                // Can't format u32 in no_std easily, just show if assigned
                "assigned"
            };
            crate::kinfo!("  CPU {}: {} stripes, NUMA node {}", i, stripes, node_str);
        }
    }
    
    // Work distribution summary
    if online > 1 && total_stripes > 0 {
        let avg_stripes = total_stripes / online;
        crate::kinfo!("  Avg stripes/CPU: {}", avg_stripes);
    }
}

// =============================================================================
// Dirty Region Tracking
// =============================================================================

/// Maximum number of dirty regions to track before full repaint
const MAX_DIRTY_REGIONS: usize = 16;

/// Dirty region tracker for incremental updates
pub struct DirtyRegionTracker {
    /// Array of dirty regions
    regions: [CompositionRegion; MAX_DIRTY_REGIONS],
    /// Number of active dirty regions
    count: usize,
    /// Flag indicating full repaint needed
    full_repaint: bool,
}

impl DirtyRegionTracker {
    /// Create a new dirty region tracker
    pub const fn new() -> Self {
        Self {
            regions: [CompositionRegion::new(0, 0, 0, 0); MAX_DIRTY_REGIONS],
            count: 0,
            full_repaint: false,
        }
    }
    
    /// Mark a region as dirty
    pub fn mark_dirty(&mut self, region: CompositionRegion) {
        if self.full_repaint || !region.is_valid() {
            return;
        }
        
        // Try to merge with existing region
        for i in 0..self.count {
            if self.regions[i].intersects(&region) {
                // Merge: expand existing region to include new one
                let existing = &mut self.regions[i];
                let new_x = existing.x.min(region.x);
                let new_y = existing.y.min(region.y);
                let new_right = (existing.x + existing.width).max(region.x + region.width);
                let new_bottom = (existing.y + existing.height).max(region.y + region.height);
                existing.x = new_x;
                existing.y = new_y;
                existing.width = new_right - new_x;
                existing.height = new_bottom - new_y;
                return;
            }
        }
        
        // Add new region if space available
        if self.count < MAX_DIRTY_REGIONS {
            self.regions[self.count] = region;
            self.count += 1;
        } else {
            // Too many regions - fall back to full repaint
            self.full_repaint = true;
        }
    }
    
    /// Mark entire screen as dirty
    pub fn mark_full_repaint(&mut self) {
        self.full_repaint = true;
    }
    
    /// Check if full repaint is needed
    pub fn needs_full_repaint(&self) -> bool {
        self.full_repaint
    }
    
    /// Get dirty regions for rendering
    pub fn get_dirty_regions(&self) -> &[CompositionRegion] {
        if self.full_repaint {
            &[] // Caller should handle full repaint separately
        } else {
            &self.regions[..self.count]
        }
    }
    
    /// Clear all dirty regions after rendering
    pub fn clear(&mut self) {
        self.count = 0;
        self.full_repaint = false;
    }
    
    /// Check if any regions are dirty
    pub fn is_dirty(&self) -> bool {
        self.full_repaint || self.count > 0
    }
}
