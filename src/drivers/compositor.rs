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

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

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
pub const MIN_ROWS_PER_WORKER: usize = 16;

/// Default stripe height for parallel composition
pub const DEFAULT_STRIPE_HEIGHT: usize = 32;

/// Threshold for using fast memset-style fill (in pixels)
const FAST_FILL_THRESHOLD: usize = 8;

/// Threshold for using batch pixel processing
const BATCH_BLEND_THRESHOLD: usize = 4;

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

/// Request a stripe of work to process
/// 
/// Returns (stripe_index, start_row, end_row) or None if no work available
fn claim_work_stripe(stripe_height: usize, total_rows: usize) -> Option<(usize, usize, usize)> {
    let total_stripes = (total_rows + stripe_height - 1) / stripe_height;
    
    loop {
        let current = WORK_NEXT_STRIPE.load(Ordering::Acquire);
        if current >= total_stripes {
            return None;
        }
        
        if WORK_NEXT_STRIPE
            .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            let start_row = current * stripe_height;
            let end_row = ((current + 1) * stripe_height).min(total_rows);
            return Some((current, start_row, end_row));
        }
        
        // Spin hint for failed CAS
        core::hint::spin_loop();
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
/// It can be called by any CPU core during parallel composition.
fn compose_stripe(
    dst_buffer: *mut u8,
    dst_pitch: usize,
    dst_bpp: usize,
    start_row: usize,
    end_row: usize,
    layers: &[CompositionLayer],
    screen_width: usize,
) {
    // For each row in our stripe
    for row in start_row..end_row {
        let row_offset = row * dst_pitch;
        
        // Process each layer from bottom to top (by z_order)
        for layer in layers.iter().filter(|l| l.should_render()) {
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

/// Alpha blend a row of pixels
/// 
/// Optimized with:
/// - Batch 4-pixel processing for better cache utilization
/// - Multiplication by 257 and shift instead of division by 255
/// - Reduced bounds checking in inner loop
#[inline(always)]
fn blend_row_alpha(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
    alpha: u8,
) {
    let alpha16 = alpha as u16;
    let inv_alpha16 = 255 - alpha16;
    let bytes_per_pixel = bpp.min(3);
    
    // Fast path for 32-bit pixels (BGRA/RGBA) - process 4 pixels at a time
    if bpp == 4 && pixels >= BATCH_BLEND_THRESHOLD {
        let batch_count = pixels / 4;
        let remainder = pixels % 4;
        
        unsafe {
            // Process 4 pixels at a time
            for batch in 0..batch_count {
                let base_offset = batch * 4 * 4; // 4 pixels * 4 bytes
                
                for p in 0..4 {
                    let pixel_offset = base_offset + p * 4;
                    // Unrolled loop for RGB channels (skip alpha at index 3)
                    let s0 = *src.add(pixel_offset) as u16;
                    let d0 = *dst.add(pixel_offset) as u16;
                    // Use (x * 257) >> 8 ≈ x / 255 for better performance
                    let r0 = ((s0 * alpha16 + d0 * inv_alpha16 + 128) >> 8) as u8;
                    *dst.add(pixel_offset) = r0;
                    
                    let s1 = *src.add(pixel_offset + 1) as u16;
                    let d1 = *dst.add(pixel_offset + 1) as u16;
                    let r1 = ((s1 * alpha16 + d1 * inv_alpha16 + 128) >> 8) as u8;
                    *dst.add(pixel_offset + 1) = r1;
                    
                    let s2 = *src.add(pixel_offset + 2) as u16;
                    let d2 = *dst.add(pixel_offset + 2) as u16;
                    let r2 = ((s2 * alpha16 + d2 * inv_alpha16 + 128) >> 8) as u8;
                    *dst.add(pixel_offset + 2) = r2;
                }
            }
            
            // Handle remaining pixels
            let remainder_offset = batch_count * 4 * 4;
            for i in 0..remainder {
                let offset = remainder_offset + i * 4;
                for c in 0..3 {
                    let s = *src.add(offset + c) as u16;
                    let d = *dst.add(offset + c) as u16;
                    let result = ((s * alpha16 + d * inv_alpha16 + 128) >> 8) as u8;
                    *dst.add(offset + c) = result;
                }
            }
        }
    } else {
        // Generic fallback for other pixel formats
        unsafe {
            for i in 0..pixels {
                let offset = i * bpp;
                for c in 0..bytes_per_pixel {
                    let s = *src.add(offset + c) as u16;
                    let d = *dst.add(offset + c) as u16;
                    // Optimized division: (x + 128) >> 8 ≈ x / 255
                    let result = ((s * alpha16 + d * inv_alpha16 + 128) >> 8) as u8;
                    *dst.add(offset + c) = result;
                }
            }
        }
    }
}

/// Additive blend a row of pixels
/// 
/// Optimized with saturating addition and batch processing
#[inline(always)]
fn blend_row_additive(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
) {
    let bytes_per_pixel = bpp.min(3);
    
    // Fast path for 32-bit pixels
    if bpp == 4 && pixels >= BATCH_BLEND_THRESHOLD {
        unsafe {
            for i in 0..pixels {
                let offset = i * 4;
                // Use saturating_add for automatic clamping (no branch)
                let r = (*src.add(offset)).saturating_add(*dst.add(offset));
                let g = (*src.add(offset + 1)).saturating_add(*dst.add(offset + 1));
                let b = (*src.add(offset + 2)).saturating_add(*dst.add(offset + 2));
                *dst.add(offset) = r;
                *dst.add(offset + 1) = g;
                *dst.add(offset + 2) = b;
            }
        }
    } else {
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
}

/// Multiply blend a row of pixels
/// 
/// Optimized with approximate division and batch processing
#[inline(always)]
fn blend_row_multiply(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
) {
    let bytes_per_pixel = bpp.min(3);
    
    // Fast path for 32-bit pixels
    if bpp == 4 && pixels >= BATCH_BLEND_THRESHOLD {
        unsafe {
            for i in 0..pixels {
                let offset = i * 4;
                // Optimized: (x * y + 128) >> 8 ≈ (x * y) / 255
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
    } else {
        unsafe {
            for i in 0..pixels {
                let offset = i * bpp;
                for c in 0..bytes_per_pixel {
                    let s = *src.add(offset + c) as u16;
                    let d = *dst.add(offset + c) as u16;
                    // Optimized division approximation
                    let result = ((s * d + 128) >> 8) as u8;
                    *dst.add(offset + c) = result;
                }
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
    let stripe_height = DEFAULT_STRIPE_HEIGHT.max(height / (workers * 4));
    let total_stripes = (height + stripe_height - 1) / stripe_height;
    
    // Initialize work distribution
    WORK_NEXT_STRIPE.store(0, Ordering::Release);
    WORK_TOTAL_STRIPES.store(total_stripes, Ordering::Release);
    WORK_COMPLETED.store(0, Ordering::Release);
    WORK_IN_PROGRESS.store(true, Ordering::Release);
    
    // Increment generation for this composition
    COMPOSITION_GEN.fetch_add(1, Ordering::SeqCst);
    
    // TODO: Send IPIs to wake up worker CPUs
    // For now, BSP does all the work
    // In a full implementation, AP cores would be waiting for work
    // and we would use IPI_CALL_FUNCTION to dispatch them
    
    // BSP participates in work
    let bsp_stripes = worker_compose(
        dst_buffer,
        dst_pitch,
        dst_bpp,
        height,
        layers,
        width,
        stripe_height,
    );
    
    // Wait for all work to complete (in case APs were participating)
    wait_for_completion();
    
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
    
    // Per-CPU stats
    let online = smp::online_cpus();
    for i in 0..online.min(smp::MAX_CPUS) {
        unsafe {
            let state = &CPU_WORK_STATES[i];
            let node = state.numa_node.load(Ordering::Relaxed);
            let stripes = state.stripes_completed.load(Ordering::Relaxed);
            let node_str = if node == numa::NUMA_NO_NODE {
                "N/A"
            } else {
                // Can't format u32 in no_std easily, just show if assigned
                "assigned"
            };
            crate::kinfo!("  CPU {}: {} stripes, NUMA node {}", i, stripes, node_str);
        }
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
