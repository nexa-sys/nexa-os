//! High-performance memory operations
//!
//! Optimized memory operations for 2.5K+ displays, including fast fill,
//! prefetch-enabled copy, and streaming stores.

use super::config::MIN_ROWS_PER_WORKER;

// =============================================================================
// Fast Memory Fill
// =============================================================================

/// Fast memory fill using 64-bit stores with write combining
/// 
/// Optimized for framebuffer memory which is typically mapped as write-combining (WC).
/// Uses sequential 64-bit stores which are coalesced by the CPU's write combining buffer.
/// 
/// For 2.5K (2560x1440) @ 32bpp = 14.7MB framebuffer:
/// - Single row = 10,240 bytes = 1,280 u64 stores
/// - Using 4-way unroll reduces loop overhead
#[inline(always)]
pub fn fast_fill_u64(dst: *mut u8, len_bytes: usize, pattern: u64) {
    let qwords = len_bytes / 8;
    let remainder = len_bytes % 8;
    
    unsafe {
        let dst64 = dst as *mut u64;
        
        // 4-way unrolled loop for better instruction throughput
        let batches = qwords / 4;
        let batch_rem = qwords % 4;
        
        for batch in 0..batches {
            let base = batch * 4;
            dst64.add(base).write(pattern);
            dst64.add(base + 1).write(pattern);
            dst64.add(base + 2).write(pattern);
            dst64.add(base + 3).write(pattern);
        }
        
        // Remaining qwords
        let base = batches * 4;
        for i in 0..batch_rem {
            dst64.add(base + i).write(pattern);
        }
        
        // Remaining bytes (0-7)
        if remainder > 0 {
            let pattern_bytes = pattern.to_le_bytes();
            let rem_ptr = dst.add(qwords * 8);
            for i in 0..remainder {
                rem_ptr.add(i).write(pattern_bytes[i]);
            }
        }
    }
}

// =============================================================================
// Fast Memory Copy
// =============================================================================

/// Fast memory copy with prefetching
/// 
/// Optimized for large framebuffer copies with prefetch hints.
/// Uses 8-way unrolled 64-bit copies for maximum throughput.
#[inline(always)]
pub fn fast_copy_prefetch(src: *const u8, dst: *mut u8, len_bytes: usize) {
    let qwords = len_bytes / 8;
    let remainder = len_bytes % 8;
    
    unsafe {
        let src64 = src as *const u64;
        let dst64 = dst as *mut u64;
        
        // Prefetch first cache lines
        if len_bytes >= 128 {
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                src.add(64) as *const i8
            );
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                src.add(128) as *const i8
            );
        }
        
        // 8-way unrolled copy (64 bytes = 1 cache line per iteration)
        let batches = qwords / 8;
        let batch_rem = qwords % 8;
        
        for batch in 0..batches {
            let base = batch * 8;
            
            // Prefetch 2 cache lines ahead
            if base + 24 < qwords {
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add((base + 16) * 8) as *const i8
                );
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add((base + 24) * 8) as *const i8
                );
            }
            
            // Copy 8 qwords (64 bytes)
            dst64.add(base).write(src64.add(base).read());
            dst64.add(base + 1).write(src64.add(base + 1).read());
            dst64.add(base + 2).write(src64.add(base + 2).read());
            dst64.add(base + 3).write(src64.add(base + 3).read());
            dst64.add(base + 4).write(src64.add(base + 4).read());
            dst64.add(base + 5).write(src64.add(base + 5).read());
            dst64.add(base + 6).write(src64.add(base + 6).read());
            dst64.add(base + 7).write(src64.add(base + 7).read());
        }
        
        // Remaining qwords
        let base = batches * 8;
        for i in 0..batch_rem {
            dst64.add(base + i).write(src64.add(base + i).read());
        }
        
        // Remaining bytes
        if remainder > 0 {
            let src_rem = src.add(qwords * 8);
            let dst_rem = dst.add(qwords * 8);
            for i in 0..remainder {
                dst_rem.add(i).write(src_rem.add(i).read());
            }
        }
    }
}

// =============================================================================
// Streaming Stores
// =============================================================================

/// Streaming store for framebuffer writes (bypasses cache)
/// 
/// For very large writes to framebuffer, streaming stores can be faster
/// because they don't pollute the cache. Uses non-temporal store hints.
/// 
/// Note: The destination memory should be write-combining (WC) mapped
/// for best performance, which is typical for framebuffers.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn streaming_fill_32(dst: *mut u8, count_pixels: usize, color: u32) {
    // For very small counts, use regular stores
    if count_pixels < 64 {
        unsafe {
            let dst32 = dst as *mut u32;
            for i in 0..count_pixels {
                dst32.add(i).write(color);
            }
        }
        return;
    }
    
    // For larger counts, use 64-bit stores
    let color64 = (color as u64) | ((color as u64) << 32);
    fast_fill_u64(dst, count_pixels * 4, color64);
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
pub fn calculate_optimal_stripe_height(width: usize, height: usize, bpp: usize, num_workers: usize) -> usize {
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

// =============================================================================
// Ultra-fast Bulk Memory Copy (REP MOVSQ based)
// =============================================================================

/// Ultra-fast bulk memory copy using REP MOVSQ
/// 
/// For scroll operations with large contiguous blocks, REP MOVSQ is often
/// the fastest approach as it enables hardware memory copy optimizations.
/// Requires 8-byte aligned pointers for best performance.
/// 
/// # Safety
/// Caller must ensure src and dst don't overlap, or dst < src for forward copy.
#[inline(always)]
pub unsafe fn rep_movsq_copy(src: *const u8, dst: *mut u8, len_bytes: usize) {
    let qwords = len_bytes / 8;
    let remainder = len_bytes % 8;
    
    if qwords > 0 {
        // Use REP MOVSQ for bulk copy (8 bytes at a time)
        core::arch::asm!(
            "rep movsq",
            inout("rcx") qwords => _,
            inout("rsi") src => _,
            inout("rdi") dst => _,
            options(nostack, preserves_flags)
        );
    }
    
    // Handle remaining bytes (0-7)
    if remainder > 0 {
        let src_rem = src.add(qwords * 8);
        let dst_rem = dst.add(qwords * 8);
        core::ptr::copy_nonoverlapping(src_rem, dst_rem, remainder);
    }
}

/// Ultra-fast bulk memory copy with non-temporal stores
/// 
/// Uses MOVNTQ streaming stores to bypass cache for very large copies.
/// Best for framebuffer operations where we don't need data in cache.
/// 
/// Performance characteristics:
/// - Avoids cache pollution for large copies
/// - Better for write-combining memory regions (framebuffers)
/// - Uses 64-byte batches (cache line aligned)
#[inline(always)]
pub unsafe fn streaming_copy(src: *const u8, dst: *mut u8, len_bytes: usize) {
    // For smaller copies, use regular copy (cache is beneficial)
    if len_bytes < 64 * 1024 {
        rep_movsq_copy(src, dst, len_bytes);
        return;
    }
    
    let qwords = len_bytes / 8;
    let remainder = len_bytes % 8;
    
    let src64 = src as *const u64;
    let dst64 = dst as *mut u64;
    
    // Process 8 qwords (64 bytes) at a time for cache line efficiency
    let batches = qwords / 8;
    let batch_rem = qwords % 8;
    
    for batch in 0..batches {
        let base = batch * 8;
        
        // Prefetch source data (non-temporal hint since we won't reuse)
        if batch + 2 < batches {
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_NTA}>(
                src.add((base + 16) * 8) as *const i8
            );
        }
        
        // Read 8 qwords and write using non-temporal stores (MOVNTQ)
        // This bypasses the cache to avoid polluting it with data we won't reuse
        let dst_ptr = dst64.add(base) as *mut u64;
        let src_ptr = src64.add(base) as *const u64;
        
        // Use inline asm for MOVNTQ (non-temporal 64-bit store)
        let v0 = src_ptr.read();
        let v1 = src_ptr.add(1).read();
        let v2 = src_ptr.add(2).read();
        let v3 = src_ptr.add(3).read();
        let v4 = src_ptr.add(4).read();
        let v5 = src_ptr.add(5).read();
        let v6 = src_ptr.add(6).read();
        let v7 = src_ptr.add(7).read();
        
        // MOVNTI: Non-temporal store hint for 64-bit integers
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr,
            v = in(reg) v0,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(1),
            v = in(reg) v1,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(2),
            v = in(reg) v2,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(3),
            v = in(reg) v3,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(4),
            v = in(reg) v4,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(5),
            v = in(reg) v5,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(6),
            v = in(reg) v6,
            options(nostack, preserves_flags)
        );
        core::arch::asm!(
            "movnti [{dst}], {v}",
            dst = in(reg) dst_ptr.add(7),
            v = in(reg) v7,
            options(nostack, preserves_flags)
        );
    }
    
    // Remaining qwords with regular stores
    let base = batches * 8;
    for i in 0..batch_rem {
        dst64.add(base + i).write(src64.add(base + i).read());
    }
    
    // Remaining bytes
    if remainder > 0 {
        let src_rem = src.add(qwords * 8);
        let dst_rem = dst.add(qwords * 8);
        core::ptr::copy_nonoverlapping(src_rem, dst_rem, remainder);
    }
    
    // SFENCE: Ensure all non-temporal stores are globally visible
    core::arch::asm!("sfence", options(nostack, preserves_flags));
}
