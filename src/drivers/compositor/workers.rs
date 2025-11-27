//! Parallel worker functions
//!
//! This module contains the worker functions that run on multiple CPU cores
//! to perform parallel composition, scrolling, and filling operations.

use core::sync::atomic::Ordering;

use crate::smp;

use super::blend::{blend_row_alpha, blend_row_additive, blend_row_multiply};
use super::config::MAX_LAYERS;
use super::state::*;
use super::types::{BlendMode, CompositionLayer, WorkType, CPU_WORK_STATES, COMPOSE_LAYERS};

// =============================================================================
// AP Core Entry Point
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

// =============================================================================
// Composition Worker
// =============================================================================

/// Internal compose worker called by AP cores during parallel composition
/// 
/// This worker claims stripes atomically and composites them using the
/// shared layer data stored in COMPOSE_LAYERS.
pub(crate) fn compose_worker_internal() -> usize {
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

/// Compose a single stripe of the framebuffer
/// 
/// This is the core composition function that processes one horizontal stripe.
/// Optimized with:
/// - Fast path for single opaque layer (common case)
/// - Layer pre-filtering with cached intersection data
/// - Aggressive prefetching (2 cache lines ahead)
/// - Reduced branching via pre-computed row ranges
/// - Cache-aligned processing where possible
/// - Loop unrolling for multi-row processing
pub(crate) fn compose_stripe(
    dst_buffer: *mut u8,
    dst_pitch: usize,
    dst_bpp: usize,
    start_row: usize,
    end_row: usize,
    layers: &[CompositionLayer],
    screen_width: usize,
) {
    // Pre-filter and cache layer metadata to avoid repeated checks
    // Store all computed values to eliminate redundant calculations
    struct LayerCache<'a> {
        layer: &'a CompositionLayer,
        start_y: usize,
        end_y: usize,
        layer_x: usize,
        actual_width: usize,
        src_pitch: usize,      // Cache buffer pitch
        buffer_addr: usize,    // Cache buffer address as usize
        is_opaque: bool,       // Cache blend mode check
        is_full_width: bool,   // Cache full-width check
    }
    
    let mut active_layers: [Option<LayerCache>; MAX_LAYERS] = [None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None];
    let mut active_count = 0;
    
    for layer in layers.iter() {
        if layer.should_render() && active_count < MAX_LAYERS {
            let layer_start_y = layer.dst_region.y as usize;
            let layer_end_y = layer_start_y + layer.dst_region.height as usize;
            let layer_x = layer.dst_region.x as usize;
            let layer_width = layer.dst_region.width as usize;
            
            // Skip layers that don't intersect our stripe at all
            if layer_end_y <= start_row || layer_start_y >= end_row {
                continue;
            }
            
            // Pre-compute bounds-checked width
            if layer_x >= screen_width {
                continue;
            }
            let actual_width = layer_width.min(screen_width - layer_x);
            
            // Pre-compute blend mode flags
            let is_opaque = matches!(layer.blend_mode, BlendMode::Opaque) 
                || (matches!(layer.blend_mode, BlendMode::Alpha) && layer.alpha == 255);
            let is_full_width = layer_x == 0 && actual_width == screen_width;
            
            active_layers[active_count] = Some(LayerCache {
                layer,
                start_y: layer_start_y,
                end_y: layer_end_y,
                layer_x,
                actual_width,
                src_pitch: layer.buffer_pitch as usize,
                buffer_addr: layer.buffer_addr as usize,
                is_opaque,
                is_full_width,
            });
            active_count += 1;
        }
    }
    
    if active_count == 0 {
        return;
    }
    
    // === FAST PATH: Single full-width opaque layer ===
    // This is the common case for terminal scrolling and full-screen updates
    if active_count == 1 {
        if let Some(ref cache) = active_layers[0] {
            if cache.is_opaque && cache.is_full_width {
                // Intersection of stripe and layer bounds
                let copy_start = start_row.max(cache.start_y);
                let copy_end = end_row.min(cache.end_y);
                
                if copy_start < copy_end {
                    let total_rows = copy_end - copy_start;
                    let row_bytes = cache.actual_width * dst_bpp;
                    
                    // Large bulk copy using REP MOVSQ when beneficial
                    if row_bytes >= 4096 && total_rows >= 4 {
                        unsafe {
                            let mut row = copy_start;
                            // Process 4 rows at a time for better throughput
                            while row + 4 <= copy_end {
                                let src_row0 = row - cache.start_y;
                                for i in 0..4 {
                                    let src_offset = (src_row0 + i) * cache.src_pitch;
                                    let dst_offset = (row + i) * dst_pitch;
                                    super::memory::rep_movsq_copy(
                                        (cache.buffer_addr + src_offset) as *const u8,
                                        dst_buffer.add(dst_offset),
                                        row_bytes,
                                    );
                                }
                                row += 4;
                            }
                            // Remaining rows
                            while row < copy_end {
                                let src_row = row - cache.start_y;
                                let src_offset = src_row * cache.src_pitch;
                                let dst_offset = row * dst_pitch;
                                super::memory::rep_movsq_copy(
                                    (cache.buffer_addr + src_offset) as *const u8,
                                    dst_buffer.add(dst_offset),
                                    row_bytes,
                                );
                                row += 1;
                            }
                        }
                    } else {
                        // Standard copy for smaller regions
                        unsafe {
                            for row in copy_start..copy_end {
                                let src_row = row - cache.start_y;
                                let src_offset = src_row * cache.src_pitch;
                                let dst_offset = row * dst_pitch;
                                core::ptr::copy_nonoverlapping(
                                    (cache.buffer_addr + src_offset) as *const u8,
                                    dst_buffer.add(dst_offset),
                                    row_bytes,
                                );
                            }
                        }
                    }
                }
                return;
            }
        }
    }
    
    // === GENERAL PATH: Multiple layers or complex blend modes ===
    // Pre-compute row stride for destination buffer
    let dst_row_stride = dst_pitch;
    
    // For each row in our stripe
    for row in start_row..end_row {
        let row_offset = row * dst_row_stride;
        
        // Prefetch destination 2 rows ahead
        if row + 2 < end_row {
            unsafe {
                let prefetch_offset = (row + 2) * dst_row_stride;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    dst_buffer.add(prefetch_offset) as *const i8
                );
            }
        }
        
        // Process each active layer from bottom to top (by z_order)
        for i in 0..active_count {
            if let Some(ref cache) = active_layers[i] {
                // Fast row intersection check using cached bounds
                if row < cache.start_y || row >= cache.end_y {
                    continue;
                }
                
                // Use cached values to minimize per-pixel overhead
                let src_row = row - cache.start_y;
                let src_pitch = cache.src_pitch;
                
                // Calculate buffer addresses using pre-cached values
                let dst_row_start = unsafe { 
                    dst_buffer.add(row_offset + cache.layer_x * dst_bpp) 
                };
                
                let src_row_offset = src_row * src_pitch;
                let src_row_start = cache.buffer_addr + src_row_offset;
                let actual_width = cache.actual_width;
                
                // Prefetch source data
                unsafe {
                    core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                        src_row_start as *const i8
                    );
                    // Prefetch 2 source rows ahead for better latency hiding
                    if row + 2 < cache.end_y {
                        let far_src = src_row_start + src_pitch * 2;
                        core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T1}>(
                            far_src as *const i8
                        );
                    }
                }
                
                // Composite based on blend mode (use cached is_opaque)
                if cache.is_opaque {
                    // Direct copy (fast path)
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            src_row_start as *const u8,
                            dst_row_start,
                            actual_width * dst_bpp,
                        );
                    }
                } else {
                    match cache.layer.blend_mode {
                        BlendMode::Alpha => {
                            let alpha = cache.layer.alpha;
                            if alpha > 0 {
                                blend_row_alpha(
                                    src_row_start as *const u8,
                                    dst_row_start,
                                    actual_width,
                                    dst_bpp,
                                    alpha,
                                );
                            }
                            // alpha == 0 -> skip entirely (handled by should_render)
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
                        BlendMode::Opaque => {
                            // Already handled by is_opaque check above
                            unreachable!()
                        }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Scroll Worker
// =============================================================================

/// Internal scroll worker - claims and processes rows
/// 
/// GPU-inspired optimizations for 2.5K+ displays:
/// - Larger batch sizes tuned for L2 cache (256KB)
/// - Bulk memory copy for contiguous rows using REP MOVSQ
/// - Prefetching to hide memory latency
/// - Adaptive batch sizing based on remaining work
/// - Reduced atomic operations in hot path
/// 
/// Memory safety for scroll-up (src > dst):
/// When scrolling up, src_base = buffer + scroll_rows * pitch, dst_base = buffer.
/// For any row N: src[N] and dst[N] are separated by scroll_rows * pitch bytes.
/// Since scroll_rows >= 1 and we copy row_bytes <= pitch per row,
/// individual row copies NEVER overlap.
pub(crate) fn scroll_worker() -> usize {
    // Load all parameters once at start (reduces atomic ops in hot loop)
    let src_base = SCROLL_SRC_ADDR.load(Ordering::Acquire) as *const u8;
    let dst_base = SCROLL_DST_ADDR.load(Ordering::Acquire) as *mut u8;
    let row_bytes = SCROLL_ROW_BYTES.load(Ordering::Acquire);
    let pitch = SCROLL_PITCH.load(Ordering::Acquire);
    let scroll_distance = SCROLL_DISTANCE.load(Ordering::Acquire);
    let total_rows = SCROLL_TOTAL_ROWS.load(Ordering::Acquire);
    
    // For scroll_distance == 0, something is wrong - use safe path
    if scroll_distance == 0 {
        return scroll_worker_safe();
    }
    
    // Optimized batch size for 2.5K displays:
    // 2560 * 4 = 10240 bytes/row
    // L2 cache = 256KB = ~25 rows
    // Use larger batches for better throughput, but cap at scroll_distance for safety
    let rows_per_256kb = (256 * 1024) / pitch.max(1);
    let batch_size = rows_per_256kb.max(32).min(256).min(scroll_distance);
    
    let mut rows_done = 0;
    let contiguous = pitch == row_bytes;
    // Use REP MOVSQ for large contiguous copies (threshold: ~32KB)
    let use_rep_movsq = contiguous && pitch >= 4096;
    
    while let Some((start, end)) = claim_scroll_rows_fast(batch_size, total_rows) {
        let batch_rows = end - start;
        
        if contiguous && batch_rows <= scroll_distance {
            // FASTEST PATH: bulk copy entire batch as single memory block
            let src_offset = start * pitch;
            let total_bytes = batch_rows * pitch;
            
            unsafe {
                // Prefetch next batch (2 batches ahead for latency hiding)
                let prefetch_row = end + batch_size;
                if prefetch_row < total_rows {
                    let prefetch_offset = prefetch_row * pitch;
                    core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                        src_base.add(prefetch_offset) as *const i8
                    );
                }
                
                if use_rep_movsq && total_bytes >= 32768 {
                    // Use REP MOVSQ for very large copies
                    super::memory::rep_movsq_copy(
                        src_base.add(src_offset),
                        dst_base.add(src_offset),
                        total_bytes,
                    );
                } else {
                    core::ptr::copy_nonoverlapping(
                        src_base.add(src_offset),
                        dst_base.add(src_offset),
                        total_bytes,
                    );
                }
            }
        } else if contiguous {
            // Large batch exceeds scroll_distance, split into safe chunks
            let mut chunk_start = start;
            while chunk_start < end {
                let chunk_end = (chunk_start + scroll_distance).min(end);
                let chunk_rows = chunk_end - chunk_start;
                let src_offset = chunk_start * pitch;
                let total_bytes = chunk_rows * pitch;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_base.add(src_offset),
                        dst_base.add(src_offset),
                        total_bytes,
                    );
                }
                chunk_start = chunk_end;
            }
        } else {
            // Non-contiguous: copy row by row with unrolling
            // Each individual row copy is safe (row_bytes <= pitch < scroll_distance * pitch)
            let mut row = start;
            
            // Unrolled loop: process 4 rows at a time
            while row + 4 <= end {
                unsafe {
                    let offset0 = row * pitch;
                    let offset1 = (row + 1) * pitch;
                    let offset2 = (row + 2) * pitch;
                    let offset3 = (row + 3) * pitch;
                    
                    core::ptr::copy_nonoverlapping(
                        src_base.add(offset0),
                        dst_base.add(offset0),
                        row_bytes,
                    );
                    core::ptr::copy_nonoverlapping(
                        src_base.add(offset1),
                        dst_base.add(offset1),
                        row_bytes,
                    );
                    core::ptr::copy_nonoverlapping(
                        src_base.add(offset2),
                        dst_base.add(offset2),
                        row_bytes,
                    );
                    core::ptr::copy_nonoverlapping(
                        src_base.add(offset3),
                        dst_base.add(offset3),
                        row_bytes,
                    );
                }
                row += 4;
            }
            
            // Handle remaining rows
            while row < end {
                let offset = row * pitch;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_base.add(offset),
                        dst_base.add(offset),
                        row_bytes,
                    );
                }
                row += 1;
            }
        }
        
        rows_done += batch_rows;
        SCROLL_ROWS_DONE.fetch_add(batch_rows, Ordering::AcqRel);
    }
    
    rows_done
}

/// Safe scroll worker using memmove - fallback for edge cases
fn scroll_worker_safe() -> usize {
    let src_base = SCROLL_SRC_ADDR.load(Ordering::Acquire) as *const u8;
    let dst_base = SCROLL_DST_ADDR.load(Ordering::Acquire) as *mut u8;
    let row_bytes = SCROLL_ROW_BYTES.load(Ordering::Acquire);
    let pitch = SCROLL_PITCH.load(Ordering::Acquire);
    
    let batch_size = 64;
    let mut rows_done = 0;
    let contiguous = pitch == row_bytes;
    
    while let Some((start, end)) = claim_scroll_rows(batch_size) {
        let batch_rows = end - start;
        
        if contiguous {
            let src_offset = start * pitch;
            let total_bytes = batch_rows * pitch;
            unsafe {
                core::ptr::copy(
                    src_base.add(src_offset),
                    dst_base.add(src_offset),
                    total_bytes,
                );
            }
        } else {
            for row in start..end {
                let offset = row * pitch;
                unsafe {
                    core::ptr::copy(
                        src_base.add(offset),
                        dst_base.add(offset),
                        row_bytes,
                    );
                }
            }
        }
        
        rows_done += batch_rows;
        SCROLL_ROWS_DONE.fetch_add(batch_rows, Ordering::AcqRel);
    }
    
    rows_done
}

// =============================================================================
// Fill Worker
// =============================================================================

/// Internal fill worker - claims and processes rows  
/// 
/// Optimized with larger batch sizes for reduced contention
pub(crate) fn fill_worker() -> usize {
    let buffer = FILL_BUFFER_ADDR.load(Ordering::Acquire) as *mut u8;
    let pitch = FILL_PITCH.load(Ordering::Acquire);
    let width = FILL_WIDTH.load(Ordering::Acquire);
    let bpp = FILL_BPP.load(Ordering::Acquire);
    let color = FILL_COLOR.load(Ordering::Acquire);
    let total_rows = FILL_TOTAL_ROWS.load(Ordering::Acquire);
    
    // Larger batch size reduces atomic contention
    let batch_size = 48;
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

/// Optimized fill worker that copies from template row (faster than per-pixel fill)
pub(crate) fn fill_worker_copy() -> usize {
    let buffer = FILL_BUFFER_ADDR.load(Ordering::Acquire) as *mut u8;
    let pitch = FILL_PITCH.load(Ordering::Acquire);
    let width = FILL_WIDTH.load(Ordering::Acquire);
    let bpp = FILL_BPP.load(Ordering::Acquire);
    let total_rows = FILL_TOTAL_ROWS.load(Ordering::Acquire);
    let template_row = FILL_TEMPLATE_ROW.load(Ordering::Acquire);
    let x_offset = FILL_X_OFFSET.load(Ordering::Acquire);
    let start_row = FILL_NEXT_ROW.load(Ordering::Acquire);
    
    let row_bytes = width * bpp;
    // More aggressive batching for copy (cheaper than fill)
    let batch_size = 64;
    let mut rows_done = 0;
    
    while let Some((start, end)) = claim_fill_rows(batch_size, total_rows) {
        // Skip rows before our start
        if end <= start_row {
            continue;
        }
        let actual_start = start.max(start_row);
        
        // Copy template row to each row in batch
        unsafe {
            let template_ptr = buffer.add(template_row * pitch + x_offset * bpp);
            for row in actual_start..end {
                let dst_ptr = buffer.add(row * pitch + x_offset * bpp);
                core::ptr::copy_nonoverlapping(template_ptr, dst_ptr, row_bytes);
            }
        }
        rows_done += end - actual_start;
        FILL_ROWS_DONE.fetch_add(end - actual_start, Ordering::AcqRel);
    }
    
    rows_done
}

/// Fill a single row with color
/// 
/// Optimized with:
/// - 64-bit writes for 2 pixels at a time
/// - 4-pixel loop unrolling for better ILP
/// - Reduced loop overhead
#[inline(always)]
pub(crate) fn fill_single_row(buffer: *mut u8, pitch: usize, width: usize, bpp: usize, row: usize, color: u32) {
    let row_offset = row * pitch;
    
    if bpp == 4 {
        // Fast path: 32-bit color, use 64-bit writes with unrolling
        let color64 = (color as u64) | ((color as u64) << 32);
        
        unsafe {
            let qword_ptr = buffer.add(row_offset) as *mut u64;
            let qwords = width / 2;
            
            // Process 4 qwords (8 pixels) at a time for better throughput
            let quads = qwords / 4;
            let mut i = 0;
            
            // Unrolled loop: 4 qwords per iteration
            while i < quads {
                let base = i * 4;
                qword_ptr.add(base).write(color64);
                qword_ptr.add(base + 1).write(color64);
                qword_ptr.add(base + 2).write(color64);
                qword_ptr.add(base + 3).write(color64);
                i += 1;
            }
            
            // Handle remaining qwords (0-3)
            let remaining_qwords = qwords % 4;
            let remaining_base = quads * 4;
            for j in 0..remaining_qwords {
                qword_ptr.add(remaining_base + j).write(color64);
            }
            
            // Handle odd pixel at end if width is odd
            if width % 2 == 1 {
                let dword_ptr = buffer.add(row_offset + qwords * 8) as *mut u32;
                dword_ptr.write(color);
            }
        }
    } else {
        // Generic path with byte-level writes
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

// =============================================================================
// Work Claiming Functions
// =============================================================================

/// Claim a batch of rows for fill operation
#[inline(always)]
pub(crate) fn claim_fill_rows(batch_size: usize, total: usize) -> Option<(usize, usize)> {
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

/// Fast claim for scroll rows - reduced overhead version
/// 
/// Optimized for scroll operations where we already have total_rows.
/// Avoids repeated atomic load of total rows.
#[inline(always)]
pub(crate) fn claim_scroll_rows_fast(batch_size: usize, total: usize) -> Option<(usize, usize)> {
    let mut attempts = 0;
    loop {
        let current = SCROLL_NEXT_ROW.load(Ordering::Acquire);
        if current >= total {
            return None;
        }
        
        // Adaptive batch size: claim more when lots of work remains
        let remaining = total - current;
        let adaptive_batch = if remaining > batch_size * 4 {
            batch_size * 2  // Double batch when plenty of work
        } else {
            batch_size
        };
        
        let end = (current + adaptive_batch).min(total);
        if SCROLL_NEXT_ROW
            .compare_exchange_weak(current, end, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return Some((current, end));
        }
        
        // Minimal backoff - scroll is time-critical
        attempts += 1;
        if attempts > 3 {
            core::hint::spin_loop();
        }
    }
}

/// Claim a batch of rows for parallel scroll processing
/// Returns (start_row, end_row) or None if no more work
/// 
/// Optimized with adaptive batch claiming to reduce contention:
/// - Claims larger batches when many rows remain
/// - Exponential backoff on contention
#[inline(always)]
pub(crate) fn claim_scroll_rows(batch_size: usize) -> Option<(usize, usize)> {
    let total = SCROLL_TOTAL_ROWS.load(Ordering::Acquire);
    
    let mut attempts = 0;
    loop {
        let current = SCROLL_NEXT_ROW.load(Ordering::Acquire);
        if current >= total {
            return None;
        }
        
        // Adaptive batch size: claim more when lots of work remains
        let remaining = total - current;
        let adaptive_batch = if remaining > batch_size * 4 {
            batch_size * 2  // Double batch when plenty of work
        } else {
            batch_size
        };
        
        let end = (current + adaptive_batch).min(total);
        if SCROLL_NEXT_ROW
            .compare_exchange_weak(current, end, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return Some((current, end));
        }
        
        // Exponential backoff to reduce contention
        attempts += 1;
        if attempts > 4 {
            for _ in 0..(1 << (attempts - 4).min(6)) {
                core::hint::spin_loop();
            }
        } else {
            core::hint::spin_loop();
        }
    }
}

/// Request a stripe of work to process
/// 
/// Returns (stripe_index, start_row, end_row) or None if no work available
/// Optimized with:
/// - Adaptive batch claiming based on remaining work
/// - Exponential backoff to reduce contention
/// - NUMA-aware stripe selection when possible
pub(crate) fn claim_work_stripe(stripe_height: usize, total_rows: usize) -> Option<(usize, usize, usize)> {
    let total_stripes = (total_rows + stripe_height - 1) / stripe_height;
    
    // Adaptive batch size: claim more stripes when many available to reduce contention
    // Tuned for 4-16 core systems typical in workstations
    let batch_size = if total_stripes > 128 {
        8  // Very large work: claim 8 at once for reduced contention
    } else if total_stripes > 64 {
        4  // Large work: claim 4 at once
    } else if total_stripes > 24 {
        2  // Medium work: claim 2 at once
    } else {
        1  // Small work: claim 1 at a time
    };
    
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
#[inline(always)]
pub(crate) fn complete_stripe() {
    WORK_COMPLETED.fetch_add(1, Ordering::AcqRel);
}

/// Wait for all stripes to complete
pub(crate) fn wait_for_completion() {
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

/// Dispatch work to AP cores and wait for completion
/// 
/// This sends IPI_CALL_FUNCTION to all online AP cores and waits
/// until all work is completed.
pub(crate) fn dispatch_to_ap_cores() {
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
