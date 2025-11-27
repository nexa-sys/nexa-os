//! Pixel blending algorithms
//!
//! High-performance blending functions for alpha, additive, and multiply modes.
//! Optimized with SIMD-style batch processing and prefetching.

use super::config::{BATCH_BLEND_THRESHOLD, SIMD_BATCH_SIZE};

// =============================================================================
// Alpha Blending
// =============================================================================

/// Alpha blend a row of pixels - High-performance version
/// 
/// GPU-inspired optimizations:
/// - 8-pixel batch processing (32 bytes = half cache line)
/// - Aggressive prefetching (2 cache lines ahead)
/// - Fixed-point arithmetic with accurate rounding
/// - 4-pixel unroll for better instruction-level parallelism
/// - Reduced memory barriers
#[inline(always)]
pub fn blend_row_alpha(
    src: *const u8,
    dst: *mut u8,
    pixels: usize,
    bpp: usize,
    alpha: u8,
) {
    if bpp != 4 || pixels < BATCH_BLEND_THRESHOLD {
        blend_row_alpha_scalar(src, dst, pixels, bpp, alpha);
        return;
    }
    
    // Pre-compute alpha factors as 16-bit for precision
    // Using (alpha * 257) >> 8 approximation for division by 255
    let alpha32 = alpha as u32;
    let inv_alpha32 = 255u32 - alpha32;
    
    unsafe {
        let src_u32 = src as *const u32;
        let dst_u32 = dst as *mut u32;
        
        // Use 64-bit operations for better throughput (2 pixels at a time)
        let src_u64 = src as *const u64;
        let dst_u64 = dst as *mut u64;
        
        // Process 8 pixels at a time (4 x 64-bit = 32 bytes) for optimal cache line usage
        let octet_count = pixels / 8;
        let remainder = pixels % 8;
        
        // Prefetch first two cache lines
        if pixels >= 16 {
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                src.add(64) as *const i8
            );
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                dst.add(64) as *const i8
            );
        }
        
        for octet in 0..octet_count {
            let base = octet * 4; // 4 x 64-bit words = 8 pixels
            
            // Prefetch 2 cache lines ahead (128 bytes = 32 pixels)
            if octet % 4 == 0 && (octet + 4) * 8 < pixels {
                let prefetch_offset = (octet + 4) * 32;  // 4 octets * 32 bytes
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    dst.add(prefetch_offset) as *const i8
                );
            }
            
            // Process 8 pixels (4 pairs) with full unroll using 64-bit operations
            // Pair 0-1 (pixels 0-1)
            let sp0 = src_u64.add(base).read_unaligned();
            let dp0 = dst_u64.add(base).read_unaligned();
            dst_u64.add(base).write_unaligned(blend_pixel_pair_fast(sp0, dp0, alpha32, inv_alpha32));
            
            // Pair 2-3 (pixels 2-3)
            let sp1 = src_u64.add(base + 1).read_unaligned();
            let dp1 = dst_u64.add(base + 1).read_unaligned();
            dst_u64.add(base + 1).write_unaligned(blend_pixel_pair_fast(sp1, dp1, alpha32, inv_alpha32));
            
            // Pair 4-5 (pixels 4-5)
            let sp2 = src_u64.add(base + 2).read_unaligned();
            let dp2 = dst_u64.add(base + 2).read_unaligned();
            dst_u64.add(base + 2).write_unaligned(blend_pixel_pair_fast(sp2, dp2, alpha32, inv_alpha32));
            
            // Pair 6-7 (pixels 6-7)
            let sp3 = src_u64.add(base + 3).read_unaligned();
            let dp3 = dst_u64.add(base + 3).read_unaligned();
            dst_u64.add(base + 3).write_unaligned(blend_pixel_pair_fast(sp3, dp3, alpha32, inv_alpha32));
        }
        
        // Handle remaining pixels (0-7)
        let rem_base = octet_count * 8;
        // First handle pairs
        let rem_pairs = remainder / 2;
        for i in 0..rem_pairs {
            let idx = rem_base / 2 + i;
            let sp = src_u64.add(idx).read_unaligned();
            let dp = dst_u64.add(idx).read_unaligned();
            dst_u64.add(idx).write_unaligned(blend_pixel_pair_fast(sp, dp, alpha32, inv_alpha32));
        }
        // Handle final odd pixel if any
        if remainder % 2 == 1 {
            let idx = rem_base + remainder - 1;
            let s = src_u32.add(idx).read_unaligned();
            let d = dst_u32.add(idx).read_unaligned();
            dst_u32.add(idx).write_unaligned(blend_pixel_fast(s, d, alpha32, inv_alpha32));
        }
    }
}

/// Fast single pixel alpha blend using 32-bit arithmetic with SWAR optimization
/// 
/// Uses the formula: result = (src * alpha + dst * inv_alpha) / 255
/// Optimized with SWAR (SIMD Within A Register) technique:
/// - Process R+B channels together in one 32-bit word
/// - Process G+A channels together in another 32-bit word
/// Error is at most 1/255, visually imperceptible
#[inline(always)]
pub fn blend_pixel_fast(src: u32, dst: u32, alpha: u32, inv_alpha: u32) -> u32 {
    // SWAR technique: process pairs of channels in parallel
    // This reduces the number of operations from 12 to 6
    
    // Extract R and B channels (bits 0-7 and 16-23)
    let src_rb = src & 0x00FF00FF;
    let dst_rb = dst & 0x00FF00FF;
    
    // Extract G and A channels (bits 8-15 and 24-31)
    let src_ga = (src >> 8) & 0x00FF00FF;
    let dst_ga = (dst >> 8) & 0x00FF00FF;
    
    // Blend R+B channels in parallel: (src * alpha + dst * inv_alpha + 128) >> 8
    let rb = ((src_rb * alpha + dst_rb * inv_alpha + 0x00800080) >> 8) & 0x00FF00FF;
    
    // Blend G channel, preserve A from destination
    let g_blended = ((src_ga & 0xFF) * alpha + (dst_ga & 0xFF) * inv_alpha + 128) >> 8;
    
    // Pack result: RB + G + original A
    rb | (g_blended << 8) | (dst & 0xFF000000)
}

/// Blend two pixels at once using 64-bit operations (for aligned data)
/// 
/// Processes 2 pixels simultaneously for better throughput on 64-bit CPUs
#[inline(always)]
pub fn blend_pixel_pair_fast(src_pair: u64, dst_pair: u64, alpha: u32, inv_alpha: u32) -> u64 {
    let p0 = blend_pixel_fast(src_pair as u32, dst_pair as u32, alpha, inv_alpha);
    let p1 = blend_pixel_fast((src_pair >> 32) as u32, (dst_pair >> 32) as u32, alpha, inv_alpha);
    (p0 as u64) | ((p1 as u64) << 32)
}

/// Scalar fallback for alpha blending (non-32bit or small regions)
#[inline(always)]
pub fn blend_row_alpha_scalar(
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

// =============================================================================
// Additive Blending
// =============================================================================

/// Additive blend a row of pixels
/// 
/// Optimized with saturating addition and 32-bit word processing
#[inline(always)]
pub fn blend_row_additive(
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
        let src_u32 = src as *const u32;
        let dst_u32 = dst as *mut u32;
        let batch_count = pixels / SIMD_BATCH_SIZE;
        let remainder = pixels % SIMD_BATCH_SIZE;
        
        for batch in 0..batch_count {
            let base = batch * SIMD_BATCH_SIZE;
            
            // Prefetch next batch
            if batch + 1 < batch_count {
                let prefetch_offset = (batch + 1) * SIMD_BATCH_SIZE * 4;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
            }
            
            // Process pixels using 32-bit saturating add emulation
            for p in 0..SIMD_BATCH_SIZE {
                let s = src_u32.add(base + p).read_unaligned();
                let d = dst_u32.add(base + p).read_unaligned();
                // Saturating add per channel using SWAR
                let result = additive_blend_pixel(s, d);
                dst_u32.add(base + p).write_unaligned(result);
            }
        }
        
        // Handle remainder
        let rem_base = batch_count * SIMD_BATCH_SIZE;
        for i in 0..remainder {
            let s = src_u32.add(rem_base + i).read_unaligned();
            let d = dst_u32.add(rem_base + i).read_unaligned();
            dst_u32.add(rem_base + i).write_unaligned(additive_blend_pixel(s, d));
        }
    }
}

/// Fast additive blend for a single pixel using SWAR technique
#[inline(always)]
fn additive_blend_pixel(src: u32, dst: u32) -> u32 {
    // Saturating add using SWAR: detect overflow per byte
    // If any channel overflows, clamp to 255
    let sum = src.wrapping_add(dst);
    
    // Detect overflow: if sum < src for any byte, that byte overflowed
    // Use the fact that (a + b) < a implies overflow in unsigned arithmetic
    // We check this per-byte using masking
    
    // Extract overflow indicators per channel
    let overflow_mask = ((!sum & src) | (!sum & dst)) & 0x80808080;
    
    // For each byte with overflow bit set, set all bits to 1
    // overflow_mask has bit 7 set for each overflowed byte
    // We want to set all 8 bits of those bytes to 1
    let saturate = overflow_mask | (overflow_mask >> 1) | (overflow_mask >> 2) | 
                   (overflow_mask >> 3) | (overflow_mask >> 4) | (overflow_mask >> 5) | 
                   (overflow_mask >> 6) | (overflow_mask >> 7);
    
    sum | saturate
}

/// Scalar fallback for additive blending
#[inline(always)]
pub fn blend_row_additive_scalar(
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

// =============================================================================
// Multiply Blending
// =============================================================================

/// Multiply blend a row of pixels
/// 
/// Optimized with 32-bit SWAR-style processing
#[inline(always)]
pub fn blend_row_multiply(
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
        let src_u32 = src as *const u32;
        let dst_u32 = dst as *mut u32;
        let batch_count = pixels / SIMD_BATCH_SIZE;
        let remainder = pixels % SIMD_BATCH_SIZE;
        
        for batch in 0..batch_count {
            let base = batch * SIMD_BATCH_SIZE;
            
            // Prefetch next batch
            if batch + 1 < batch_count {
                let prefetch_offset = (batch + 1) * SIMD_BATCH_SIZE * 4;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
            }
            
            // Process pixels using optimized multiply
            for p in 0..SIMD_BATCH_SIZE {
                let s = src_u32.add(base + p).read_unaligned();
                let d = dst_u32.add(base + p).read_unaligned();
                dst_u32.add(base + p).write_unaligned(multiply_blend_pixel(s, d));
            }
        }
        
        // Handle remainder
        let rem_base = batch_count * SIMD_BATCH_SIZE;
        for i in 0..remainder {
            let s = src_u32.add(rem_base + i).read_unaligned();
            let d = dst_u32.add(rem_base + i).read_unaligned();
            dst_u32.add(rem_base + i).write_unaligned(multiply_blend_pixel(s, d));
        }
    }
}

/// Fast multiply blend for a single pixel
/// Uses SWAR to process R+B and G+A channel pairs
#[inline(always)]
fn multiply_blend_pixel(src: u32, dst: u32) -> u32 {
    // Extract R and B channels (bits 0-7 and 16-23)
    let src_rb = src & 0x00FF00FF;
    let dst_rb = dst & 0x00FF00FF;
    
    // Extract G channel (bits 8-15), preserve A from dst
    let src_g = (src >> 8) & 0xFF;
    let dst_g = (dst >> 8) & 0xFF;
    
    // Multiply R and B channels: (src * dst + 128) >> 8
    // Process them in parallel using 32-bit arithmetic
    // src_rb * dst_rb would overflow, so we need to be careful
    let rb_lo = (src_rb & 0xFF) * (dst_rb & 0xFF);
    let rb_hi = ((src_rb >> 16) & 0xFF) * ((dst_rb >> 16) & 0xFF);
    let rb = (((rb_lo + 128) >> 8) & 0xFF) | ((((rb_hi + 128) >> 8) & 0xFF) << 16);
    
    // Multiply G channel
    let g = ((src_g * dst_g + 128) >> 8) & 0xFF;
    
    // Pack result: RB + G + original A
    rb | (g << 8) | (dst & 0xFF000000)
}

/// Scalar fallback for multiply blending
#[inline(always)]
pub fn blend_row_multiply_scalar(
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
