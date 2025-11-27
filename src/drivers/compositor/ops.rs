//! High-level compositor operations
//!
//! This module provides the main API for composition, filling, copying,
//! and scrolling operations.

use core::sync::atomic::Ordering;

use super::config::*;
use super::state::*;
use super::types::{CompositionLayer, CompositionRegion, WorkType, COMPOSE_LAYERS};
use super::workers::*;

// =============================================================================
// Composition
// =============================================================================

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
    if workers <= 1 || height < MIN_ROWS_PER_WORKER {
        // Single-threaded is more efficient for small regions
        compose_stripe(dst_buffer, dst_pitch, dst_bpp, 0, height, layers, width);
        TOTAL_COMPOSITIONS.fetch_add(1, Ordering::Relaxed);
        return 1;
    }
    
    // Calculate stripe height for good load balancing
    // Aim for 8x more stripes than workers for better dynamic load balancing
    // Use smaller stripes on high-core systems for finer granularity
    let target_stripes = workers * 8;
    let computed_height = height / target_stripes.max(1);
    // Clamp between MIN_ROWS_PER_WORKER and DEFAULT_STRIPE_HEIGHT
    let stripe_height = computed_height.clamp(MIN_ROWS_PER_WORKER, DEFAULT_STRIPE_HEIGHT);
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

// =============================================================================
// Fill Operations
// =============================================================================

/// Fill a region with a solid color (parallel version)
/// 
/// Optimized with:
/// - Multi-core parallel filling for large regions
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
    let total_bytes = row_bytes * height;
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
    
    // Check if parallel fill is beneficial
    let workers = worker_count();
    let use_parallel = is_initialized() 
        && workers > 1 
        && total_bytes >= PARALLEL_FILL_THRESHOLD
        && height >= workers * 8;  // At least 8 rows per worker
    
    if use_parallel {
        // === Parallel fill path ===
        // First, fill a template row that workers will copy from
        unsafe {
            let first_row_ptr = dst_buffer.add(y * dst_pitch + x * dst_bpp);
            let pixel_ptr = first_row_ptr as *mut u32;
            for col in 0..width {
                pixel_ptr.add(col).write_volatile(color);
            }
        }
        
        // Setup fill parameters for workers (fill remaining rows)
        FILL_BUFFER_ADDR.store(dst_buffer as u64, Ordering::Release);
        FILL_PITCH.store(dst_pitch, Ordering::Release);
        FILL_WIDTH.store(width, Ordering::Release);
        FILL_BPP.store(dst_bpp, Ordering::Release);
        FILL_COLOR.store(color, Ordering::Release);
        FILL_NEXT_ROW.store(y + 1, Ordering::Release);  // Start from second row
        FILL_TOTAL_ROWS.store(y + height, Ordering::Release);
        FILL_ROWS_DONE.store(0, Ordering::Release);
        
        // Store template row info for workers
        FILL_TEMPLATE_ROW.store(y, Ordering::Release);
        FILL_X_OFFSET.store(x, Ordering::Release);
        
        // Set work type
        WORK_TYPE.store(WorkType::Fill as u8, Ordering::Release);
        
        // Memory fence
        core::sync::atomic::fence(Ordering::SeqCst);
        
        // Dispatch to AP cores
        dispatch_to_ap_cores();
        
        // BSP participates
        fill_worker_copy();
        
        // Wait for completion
        let rows_to_fill = height - 1;  // Excluding template row
        let mut backoff = 1u32;
        while FILL_ROWS_DONE.load(Ordering::Acquire) < rows_to_fill {
            for _ in 0..backoff {
                core::hint::spin_loop();
            }
            backoff = (backoff * 2).min(1024);
        }
        
        // Clear work flags
        WORK_AVAILABLE.store(false, Ordering::Release);
        WORK_TYPE.store(WorkType::None as u8, Ordering::Release);
    } else {
        // === Single-core fast path ===
        // Fill first row, then copy to remaining rows
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

/// Fast row clearing using 64-bit writes with optimized row replication
/// 
/// Optimized for clearing the bottom portion after scroll.
/// Uses streaming stores for large clears to avoid cache pollution.
/// Critical for smooth scrolling - the cleared area should not pollute cache.
#[inline(always)]
pub fn clear_rows_fast(
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
    let total_clear_bytes = row_bytes * num_rows;
    
    // For 32-bit color, use 64-bit writes (2 pixels at a time)
    if bpp == 4 {
        // Create 64-bit pattern (2 pixels)
        let color64 = (color as u64) | ((color as u64) << 32);
        let qwords_per_row = row_bytes / 8;
        let remainder_bytes = row_bytes % 8;
        
        unsafe {
            // Fill first row with unrolled 64-bit writes
            let first_row_ptr = buffer.add(start_row * pitch);
            let qword_ptr = first_row_ptr as *mut u64;
            
            // Unrolled loop: 8 qwords at a time for better throughput
            let batches = qwords_per_row / 8;
            let batch_rem = qwords_per_row % 8;
            
            for batch in 0..batches {
                let base = batch * 8;
                qword_ptr.add(base).write(color64);
                qword_ptr.add(base + 1).write(color64);
                qword_ptr.add(base + 2).write(color64);
                qword_ptr.add(base + 3).write(color64);
                qword_ptr.add(base + 4).write(color64);
                qword_ptr.add(base + 5).write(color64);
                qword_ptr.add(base + 6).write(color64);
                qword_ptr.add(base + 7).write(color64);
            }
            
            // Remaining qwords
            let base = batches * 8;
            for i in 0..batch_rem {
                qword_ptr.add(base + i).write(color64);
            }
            
            // Handle remainder (0-7 bytes)
            if remainder_bytes >= 4 {
                let dword_ptr = first_row_ptr.add(qwords_per_row * 8) as *mut u32;
                dword_ptr.write(color);
            }
            
            // Copy first row to remaining rows using optimized method
            if num_rows > 1 {
                // Use streaming copy for large clears to avoid cache pollution
                // This is critical after scroll - cleared area won't be read immediately
                if total_clear_bytes >= 64 * 1024 && num_rows >= 16 {
                    // Very large clear: use streaming stores
                    for row in 1..num_rows {
                        let dst_row_ptr = buffer.add((start_row + row) * pitch);
                        super::memory::streaming_copy(first_row_ptr, dst_row_ptr, row_bytes);
                    }
                    // Ensure all non-temporal stores are visible
                    core::arch::asm!("sfence", options(nostack, preserves_flags));
                } else if row_bytes >= 4096 && num_rows >= 8 {
                    // Large rows: use REP MOVSQ for efficiency
                    for row in 1..num_rows {
                        let dst_row_ptr = buffer.add((start_row + row) * pitch);
                        super::memory::rep_movsq_copy(first_row_ptr, dst_row_ptr, row_bytes);
                    }
                } else {
                    // Smaller rows: unrolled copy
                    let mut row = 1;
                    while row + 4 <= num_rows {
                        let dst0 = buffer.add((start_row + row) * pitch);
                        let dst1 = buffer.add((start_row + row + 1) * pitch);
                        let dst2 = buffer.add((start_row + row + 2) * pitch);
                        let dst3 = buffer.add((start_row + row + 3) * pitch);
                        core::ptr::copy_nonoverlapping(first_row_ptr, dst0, row_bytes);
                        core::ptr::copy_nonoverlapping(first_row_ptr, dst1, row_bytes);
                        core::ptr::copy_nonoverlapping(first_row_ptr, dst2, row_bytes);
                        core::ptr::copy_nonoverlapping(first_row_ptr, dst3, row_bytes);
                        row += 4;
                    }
                    while row < num_rows {
                        let dst_row_ptr = buffer.add((start_row + row) * pitch);
                        core::ptr::copy_nonoverlapping(first_row_ptr, dst_row_ptr, row_bytes);
                        row += 1;
                    }
                }
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

// =============================================================================
// Copy Operations
// =============================================================================

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
// Scroll Operations
// =============================================================================

/// High-performance scroll up operation
/// 
/// This is optimized for large screens (2.5K+) with:
/// - Multi-core parallel row copying
/// - REP MOVSQ for large contiguous copies
/// - Non-temporal stores for very large framebuffers
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
    // Lower parallel threshold for better utilization on large displays
    let use_parallel = is_initialized() 
        && workers > 1 
        && total_bytes >= PARALLEL_SCROLL_THRESHOLD
        && rows_to_copy >= workers * 16;  // Reduced from 32 for better parallelization
    
    // Check if rows are contiguous (no padding)
    let contiguous = pitch == row_bytes;
    
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
        // Store scroll distance for workers to determine safe batch size
        SCROLL_DISTANCE.store(scroll_rows, Ordering::Release);
        
        // Memory fence to ensure all stores are visible to AP cores
        core::sync::atomic::fence(Ordering::SeqCst);
        
        // Dispatch work to AP cores via IPI
        dispatch_to_ap_cores();
        
        // BSP also participates in the work
        scroll_worker();
        
        // Wait for all rows to complete with ultra-fast polling
        // Scroll is time-critical, use aggressive polling
        let mut backoff = 1u32;
        let mut checks = 0u32;
        while SCROLL_ROWS_DONE.load(Ordering::Acquire) < rows_to_copy {
            checks += 1;
            // Only add backoff after many rapid checks
            if checks > 64 {
                for _ in 0..backoff {
                    core::hint::spin_loop();
                }
                backoff = (backoff * 2).min(128);
            }
        }
        
        // Clear work available flag
        WORK_AVAILABLE.store(false, Ordering::Release);
        WORK_TYPE.store(WorkType::None as u8, Ordering::Release);
    } else {
        // === Single-core optimized path ===
        // Use streaming stores for large copies to avoid cache pollution
        // This is critical for smooth scrolling performance
        unsafe {
            let src = buffer.add(scroll_rows * pitch);
            
            if contiguous {
                // Fast path for contiguous rows
                // Use streaming copy for very large framebuffers (>256KB)
                if total_bytes >= 256 * 1024 {
                    // Very large copy: use streaming stores to avoid cache pollution
                    let chunk_bytes = scroll_rows * pitch;
                    let mut copied = 0;
                    
                    while copied + chunk_bytes <= total_bytes {
                        super::memory::streaming_copy(
                            src.add(copied),
                            buffer.add(copied),
                            chunk_bytes,
                        );
                        copied += chunk_bytes;
                    }
                    
                    // Copy remaining bytes
                    if copied < total_bytes {
                        super::memory::streaming_copy(
                            src.add(copied),
                            buffer.add(copied),
                            total_bytes - copied,
                        );
                    }
                } else if total_bytes >= 64 * 1024 {
                    // Medium-large copy: use REP MOVSQ with chunking
                    let chunk_bytes = scroll_rows * pitch;
                    let mut copied = 0;
                    
                    while copied + chunk_bytes <= total_bytes {
                        super::memory::rep_movsq_copy(
                            src.add(copied),
                            buffer.add(copied),
                            chunk_bytes,
                        );
                        copied += chunk_bytes;
                    }
                    
                    // Copy remaining bytes
                    if copied < total_bytes {
                        super::memory::rep_movsq_copy(
                            src.add(copied),
                            buffer.add(copied),
                            total_bytes - copied,
                        );
                    }
                } else {
                    // Smaller copy: chunked copy_nonoverlapping
                    let chunk_bytes = scroll_rows * pitch;
                    let mut copied = 0;
                    
                    while copied + chunk_bytes <= total_bytes {
                        core::ptr::copy_nonoverlapping(
                            src.add(copied),
                            buffer.add(copied),
                            chunk_bytes,
                        );
                        copied += chunk_bytes;
                    }
                    
                    // Copy remaining bytes
                    if copied < total_bytes {
                        core::ptr::copy_nonoverlapping(
                            src.add(copied),
                            buffer.add(copied),
                            total_bytes - copied,
                        );
                    }
                }
            } else {
                // Non-contiguous: use row-by-row copy with 8x unrolling
                let mut row = 0;
                while row + 8 <= rows_to_copy {
                    let offset0 = row * pitch;
                    let offset1 = (row + 1) * pitch;
                    let offset2 = (row + 2) * pitch;
                    let offset3 = (row + 3) * pitch;
                    let offset4 = (row + 4) * pitch;
                    let offset5 = (row + 5) * pitch;
                    let offset6 = (row + 6) * pitch;
                    let offset7 = (row + 7) * pitch;
                    
                    core::ptr::copy_nonoverlapping(src.add(offset0), buffer.add(offset0), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset1), buffer.add(offset1), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset2), buffer.add(offset2), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset3), buffer.add(offset3), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset4), buffer.add(offset4), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset5), buffer.add(offset5), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset6), buffer.add(offset6), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset7), buffer.add(offset7), row_bytes);
                    row += 8;
                }
                // Handle remaining rows with 4x unroll
                while row + 4 <= rows_to_copy {
                    let offset0 = row * pitch;
                    let offset1 = (row + 1) * pitch;
                    let offset2 = (row + 2) * pitch;
                    let offset3 = (row + 3) * pitch;
                    
                    core::ptr::copy_nonoverlapping(src.add(offset0), buffer.add(offset0), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset1), buffer.add(offset1), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset2), buffer.add(offset2), row_bytes);
                    core::ptr::copy_nonoverlapping(src.add(offset3), buffer.add(offset3), row_bytes);
                    row += 4;
                }
                while row < rows_to_copy {
                    let offset = row * pitch;
                    core::ptr::copy_nonoverlapping(src.add(offset), buffer.add(offset), row_bytes);
                    row += 1;
                }
            }
        }
    }
    
    // Clear the newly exposed area at bottom
    // This is always relatively small, so single-threaded is fine
    let clear_start_row = height - scroll_rows;
    clear_rows_fast(buffer, pitch, width, bpp, clear_start_row, scroll_rows, clear_color);
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Calculate optimal stripe height for the given screen dimensions
/// 
/// Returns a stripe height that:
/// 1. Fits well in L2 cache (256KB typical)
/// 2. Provides good parallelization (8+ stripes per worker)
/// 3. Aligns to reasonable boundaries
pub fn optimal_stripe_height(width: usize, height: usize, bpp: usize, num_workers: usize) -> usize {
    let bytes_per_row = width * bpp;
    
    // Target: fit stripe in L2 cache (256KB) with some margin
    let l2_cache_target = 200 * 1024; // 200KB to leave room for other data
    let rows_in_cache = l2_cache_target / bytes_per_row.max(1);
    
    // Ensure at least 8 stripes per worker for good load balancing
    let min_stripes = num_workers * 8;
    let max_stripe_height = height / min_stripes.max(1);
    
    // Clamp to reasonable range
    let computed = rows_in_cache.min(max_stripe_height).max(MIN_ROWS_PER_WORKER);
    
    // Round down to multiple of 8 for SIMD alignment benefits
    (computed / 8) * 8
}
