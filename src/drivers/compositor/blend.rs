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
/// - Uses Bayer-style rounding for better precision
/// Error is at most 1/255, visually imperceptible
#[inline(always)]
pub fn blend_pixel_fast(src: u32, dst: u32, alpha: u32, inv_alpha: u32) -> u32 {
    // SWAR technique: process channel pairs in parallel
    // This reduces operations from 12 to ~8 with better ILP
    
    // Extract channel pairs (R+B in low bits, G+A in positions for later shift)
    let src_rb = src & 0x00FF00FF;
    let dst_rb = dst & 0x00FF00FF;
    let src_g = (src >> 8) & 0xFF;
    let dst_g = (dst >> 8) & 0xFF;
    
    // Blend R+B channels in parallel with improved rounding:
    // Using (x + 128) >> 8 approximates x / 255 with minimal error
    // Pre-add bias for both channels simultaneously
    let rb_sum = src_rb * alpha + dst_rb * inv_alpha;
    let rb = ((rb_sum + 0x00800080) >> 8) & 0x00FF00FF;
    
    // Blend G channel separately (better precision than merged approach)
    let g = ((src_g * alpha + dst_g * inv_alpha + 128) >> 8) & 0xFF;
    
    // Pack result: RB + G + original A (preserve destination alpha)
    rb | (g << 8) | (dst & 0xFF000000)
}

/// Blend two pixels at once using 64-bit operations (for aligned data)
/// 
/// Processes 2 pixels simultaneously for better throughput on 64-bit CPUs.
/// Uses extended SWAR to process 4 channel pairs (8 channels) in parallel.
#[inline(always)]
pub fn blend_pixel_pair_fast(src_pair: u64, dst_pair: u64, alpha: u32, inv_alpha: u32) -> u64 {
    // Extended SWAR: process R0+B0+R1+B1 together using 64-bit arithmetic
    let alpha64 = alpha as u64;
    let inv_alpha64 = inv_alpha as u64;
    
    // Extract R+B channels from both pixels (interleaved in 64-bit word)
    let src_rb = src_pair & 0x00FF00FF_00FF00FF;
    let dst_rb = dst_pair & 0x00FF00FF_00FF00FF;
    
    // Extract G channels from both pixels
    let src_g0 = ((src_pair >> 8) & 0xFF) as u32;
    let dst_g0 = ((dst_pair >> 8) & 0xFF) as u32;
    let src_g1 = ((src_pair >> 40) & 0xFF) as u32;
    let dst_g1 = ((dst_pair >> 40) & 0xFF) as u32;
    
    // Blend R+B channels for both pixels in one 64-bit operation
    // Note: this may overflow for each 16-bit result, but we mask it out
    let rb_sum = src_rb.wrapping_mul(alpha64).wrapping_add(dst_rb.wrapping_mul(inv_alpha64));
    let rb = ((rb_sum.wrapping_add(0x00800080_00800080)) >> 8) & 0x00FF00FF_00FF00FF;
    
    // Blend G channels separately (32-bit is sufficient)
    let g0 = ((src_g0 * alpha + dst_g0 * inv_alpha + 128) >> 8) & 0xFF;
    let g1 = ((src_g1 * alpha + dst_g1 * inv_alpha + 128) >> 8) & 0xFF;
    
    // Preserve destination alpha, combine all channels
    let alpha_mask = dst_pair & 0xFF000000_FF000000;
    rb | ((g0 as u64) << 8) | ((g1 as u64) << 40) | alpha_mask
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

/// Fast additive blend for a single pixel using optimized SWAR technique
/// 
/// Uses carry-propagation trick for faster saturation detection
#[inline(always)]
fn additive_blend_pixel(src: u32, dst: u32) -> u32 {
    // Optimized saturating add using carry propagation
    // Key insight: overflow occurs when high bit goes from 0->1 due to carry
    
    // Process alternating bytes to avoid inter-byte carry
    let lo_mask = 0x00FF00FF_u32;
    
    // Extract alternating bytes
    let src_lo = src & lo_mask;
    let dst_lo = dst & lo_mask;
    let src_hi = (src >> 8) & lo_mask;
    let dst_hi = (dst >> 8) & lo_mask;
    
    // Add with saturation check
    let sum_lo = src_lo + dst_lo;
    let sum_hi = src_hi + dst_hi;
    
    // Detect overflow: if sum > 255, bit 8 or higher is set
    // Use this to generate saturation mask
    let sat_lo = (sum_lo & 0x01000100) >> 8;  // Overflow bits
    let sat_hi = (sum_hi & 0x01000100) >> 8;
    
    // Create saturation mask: 0xFF for overflowed bytes, 0x00 otherwise
    let sat_mask_lo = sat_lo.wrapping_mul(0xFF);
    let sat_mask_hi = sat_hi.wrapping_mul(0xFF);
    
    // Apply saturation: if overflowed, use 0xFF, else use sum
    let result_lo = (sum_lo | sat_mask_lo) & lo_mask;
    let result_hi = ((sum_hi | sat_mask_hi) & lo_mask) << 8;
    
    result_lo | result_hi
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
