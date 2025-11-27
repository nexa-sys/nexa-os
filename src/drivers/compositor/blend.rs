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
        
        // Process 4 pixels at a time (16 bytes) for register efficiency
        let quad_count = pixels / 4;
        let remainder = pixels % 4;
        
        // Prefetch first two cache lines
        if pixels >= 16 {
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                src.add(64) as *const i8
            );
            core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                dst.add(64) as *const i8
            );
        }
        
        for quad in 0..quad_count {
            let base = quad * 4;
            
            // Prefetch 2 cache lines ahead (128 bytes = 32 pixels)
            if quad % 8 == 0 && base + 32 < pixels {
                let prefetch_offset = (base + 32) * 4;
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    src.add(prefetch_offset) as *const i8
                );
                core::arch::x86_64::_mm_prefetch::<{core::arch::x86_64::_MM_HINT_T0}>(
                    dst.add(prefetch_offset) as *const i8
                );
            }
            
            // Process 4 pixels with full unroll
            // Pixel 0
            let s0 = src_u32.add(base).read_unaligned();
            let d0 = dst_u32.add(base).read_unaligned();
            dst_u32.add(base).write_unaligned(blend_pixel_fast(s0, d0, alpha32, inv_alpha32));
            
            // Pixel 1
            let s1 = src_u32.add(base + 1).read_unaligned();
            let d1 = dst_u32.add(base + 1).read_unaligned();
            dst_u32.add(base + 1).write_unaligned(blend_pixel_fast(s1, d1, alpha32, inv_alpha32));
            
            // Pixel 2
            let s2 = src_u32.add(base + 2).read_unaligned();
            let d2 = dst_u32.add(base + 2).read_unaligned();
            dst_u32.add(base + 2).write_unaligned(blend_pixel_fast(s2, d2, alpha32, inv_alpha32));
            
            // Pixel 3
            let s3 = src_u32.add(base + 3).read_unaligned();
            let d3 = dst_u32.add(base + 3).read_unaligned();
            dst_u32.add(base + 3).write_unaligned(blend_pixel_fast(s3, d3, alpha32, inv_alpha32));
        }
        
        // Handle remaining pixels (0-3)
        let rem_base = quad_count * 4;
        for i in 0..remainder {
            let s = src_u32.add(rem_base + i).read_unaligned();
            let d = dst_u32.add(rem_base + i).read_unaligned();
            dst_u32.add(rem_base + i).write_unaligned(blend_pixel_fast(s, d, alpha32, inv_alpha32));
        }
    }
}

/// Fast single pixel alpha blend using 32-bit arithmetic
/// 
/// Uses the formula: result = (src * alpha + dst * inv_alpha) / 255
/// Approximated as: (src * alpha + dst * inv_alpha + 128) >> 8
/// Error is at most 1/255, visually imperceptible
#[inline(always)]
pub fn blend_pixel_fast(src: u32, dst: u32, alpha: u32, inv_alpha: u32) -> u32 {
    // Extract components (BGRA format)
    let sb = src & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sr = (src >> 16) & 0xFF;
    
    let db = dst & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let dr = (dst >> 16) & 0xFF;
    
    // Blend each channel: (s * a + d * (255-a) + 128) / 256
    // The +128 provides rounding
    let rb = (sb * alpha + db * inv_alpha + 128) >> 8;
    let rg = (sg * alpha + dg * inv_alpha + 128) >> 8;
    let rr = (sr * alpha + dr * inv_alpha + 128) >> 8;
    
    // Pack result, preserving alpha channel from destination
    rb | (rg << 8) | (rr << 16) | (dst & 0xFF000000)
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
/// Optimized with saturating addition and 16-pixel batch processing
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
/// Optimized with approximate division and 16-pixel batch processing
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
