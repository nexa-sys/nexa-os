//! High-performance framebuffer rendering primitives
//!
//! This module contains optimized rendering functions for:
//! - Character/glyph drawing with scaling
//! - Rectangle filling (solid colors)
//! - Screen scrolling
//!
//! Optimizations include:
//! - 32bpp fast path with 64-bit writes
//! - Row buffer pre-computation
//! - Line copying instead of re-rendering
//! - Multi-core parallel operations via compositor (optional, `gfx_compositor` feature)

use super::color::PackedColor;
use super::spec::FramebufferSpec;
#[cfg(feature = "gfx_compositor")]
use crate::drivers::compositor;

/// Font rendering constants
pub const BASE_FONT_WIDTH: usize = 8;
pub const BASE_FONT_HEIGHT: usize = 8;
pub const SCALE_X: usize = 2;
pub const SCALE_Y: usize = 2;
pub const CELL_WIDTH: usize = BASE_FONT_WIDTH * SCALE_X;
pub const CELL_HEIGHT: usize = BASE_FONT_HEIGHT * SCALE_Y;

/// Rendering context with pre-computed values
pub struct RenderContext {
    pub buffer: *mut u8,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bytes_per_pixel: usize,
}

impl RenderContext {
    pub fn new(buffer: *mut u8, spec: &FramebufferSpec) -> Self {
        Self {
            buffer,
            width: spec.width as usize,
            height: spec.height as usize,
            pitch: spec.pitch as usize,
            bytes_per_pixel: ((spec.bpp + 7) / 8) as usize,
        }
    }

    /// Draw a character glyph at the specified cell position
    ///
    /// Uses GPU-style batch writes for optimal performance.
    pub fn draw_cell(
        &self,
        col: usize,
        row: usize,
        glyph: &[u8; BASE_FONT_HEIGHT],
        fg: PackedColor,
        bg: PackedColor,
    ) {
        let pixel_x = col * CELL_WIDTH;
        let pixel_y = row * CELL_HEIGHT;

        // Bounds check - only once at start
        if pixel_x + CELL_WIDTH > self.width || pixel_y + CELL_HEIGHT > self.height {
            return;
        }

        // Use optimized path for 32bpp
        if self.bytes_per_pixel == 4 {
            self.draw_cell_fast_32bpp(pixel_x, pixel_y, glyph, fg, bg);
        } else {
            self.draw_cell_generic(pixel_x, pixel_y, glyph, fg, bg);
        }
    }

    /// 32bpp fast character rendering - uses pre-computed row buffer and batch writes
    ///
    /// GPU-inspired optimizations:
    /// - Pre-compute complete row data (like GPU tile rasterization)
    /// - Use non-volatile writes to temp buffer, final batch write out
    /// - 128-bit writes (4 pixels at once) to utilize memory bus bandwidth
    /// - Copy identical rows instead of recomputing
    #[inline(always)]
    fn draw_cell_fast_32bpp(
        &self,
        pixel_x: usize,
        pixel_y: usize,
        glyph: &[u8; BASE_FONT_HEIGHT],
        fg: PackedColor,
        bg: PackedColor,
    ) {
        let fg_u32 = u32::from_le_bytes(fg.bytes);
        let bg_u32 = u32::from_le_bytes(bg.bytes);

        // Pre-compute per-row pixel data (CELL_WIDTH = 16 pixels = 64 bytes)
        let mut row_buffer: [u32; CELL_WIDTH] = [0; CELL_WIDTH];

        // Create 64-bit patterns for fast fill
        let fg_u64 = (fg_u32 as u64) | ((fg_u32 as u64) << 32);
        let bg_u64 = (bg_u32 as u64) | ((bg_u32 as u64) << 32);

        for (glyph_row, bits) in glyph.iter().enumerate() {
            // Use 64-bit operations - 2 pixels at once
            let row_ptr64 = row_buffer.as_mut_ptr() as *mut u64;

            // Unrolled: 8 font pixels -> 16 display pixels -> 8 u64s
            unsafe {
                for bit_idx in 0..BASE_FONT_WIDTH {
                    let mask = 1u8 << bit_idx;
                    let color64 = if bits & mask != 0 { fg_u64 } else { bg_u64 };
                    row_ptr64.add(bit_idx).write(color64);
                }
            }

            // Write SCALE_Y rows (usually = 2)
            let first_row_y = pixel_y + glyph_row * SCALE_Y;
            let first_row_offset = first_row_y * self.pitch + pixel_x * 4;

            unsafe {
                let dst = self.buffer.add(first_row_offset);
                let src = row_buffer.as_ptr() as *const u8;

                // Use 64-bit writes, 2 pixels at a time
                let dst64 = dst as *mut u64;
                let src64 = src as *const u64;

                // Unroll 8 u64 writes (64 bytes = 1 cache line)
                dst64.write(*src64);
                dst64.add(1).write(*src64.add(1));
                dst64.add(2).write(*src64.add(2));
                dst64.add(3).write(*src64.add(3));
                dst64.add(4).write(*src64.add(4));
                dst64.add(5).write(*src64.add(5));
                dst64.add(6).write(*src64.add(6));
                dst64.add(7).write(*src64.add(7));

                // Remaining rows: copy from first row (faster than recomputing)
                for sy in 1..SCALE_Y {
                    let target_y = first_row_y + sy;
                    let target_offset = target_y * self.pitch + pixel_x * 4;
                    core::ptr::copy_nonoverlapping(dst, self.buffer.add(target_offset), 64);
                }
            }
        }
    }

    /// Generic character rendering (non-32bpp) - optimized version
    #[inline(always)]
    fn draw_cell_generic(
        &self,
        pixel_x: usize,
        pixel_y: usize,
        glyph: &[u8; BASE_FONT_HEIGHT],
        fg: PackedColor,
        bg: PackedColor,
    ) {
        let row_bytes = CELL_WIDTH * self.bytes_per_pixel;
        let mut row_buffer: [u8; 64] = [0; 64];

        for (glyph_row_idx, bits) in glyph.iter().enumerate() {
            // Fill row buffer
            for col_offset in 0..BASE_FONT_WIDTH {
                let mask = 1u8 << col_offset;
                let color = if bits & mask != 0 { &fg } else { &bg };
                let base_x = col_offset * SCALE_X;

                for sx in 0..SCALE_X {
                    let pixel_idx = (base_x + sx) * self.bytes_per_pixel;
                    for i in 0..self.bytes_per_pixel {
                        row_buffer[pixel_idx + i] = color.bytes[i];
                    }
                }
            }

            // Write SCALE_Y rows
            let first_row_y = pixel_y + glyph_row_idx * SCALE_Y;
            let first_row_offset = first_row_y * self.pitch + pixel_x * self.bytes_per_pixel;

            unsafe {
                let dst = self.buffer.add(first_row_offset);
                core::ptr::copy_nonoverlapping(row_buffer.as_ptr(), dst, row_bytes);

                for sy in 1..SCALE_Y {
                    let target_y = first_row_y + sy;
                    let target_offset = target_y * self.pitch + pixel_x * self.bytes_per_pixel;
                    core::ptr::copy_nonoverlapping(dst, self.buffer.add(target_offset), row_bytes);
                }
            }
        }
    }

    /// Clear a single character cell
    pub fn clear_cell(&self, col: usize, row: usize, bg: PackedColor) {
        let pixel_x = col * CELL_WIDTH;
        let pixel_y = row * CELL_HEIGHT;

        if pixel_x + CELL_WIDTH > self.width || pixel_y + CELL_HEIGHT > self.height {
            return;
        }

        self.fill_rect(pixel_x, pixel_y, CELL_WIDTH, CELL_HEIGHT, bg);
    }

    /// Draw a TTF glyph bitmap at pixel coordinates with alpha blending
    ///
    /// This method renders anti-aliased TTF glyphs with proper alpha blending
    /// for smooth text rendering.
    ///
    /// # Arguments
    /// * `pixel_x` - X coordinate in pixels (not cell units)
    /// * `pixel_y` - Y coordinate in pixels (baseline position)
    /// * `glyph` - The glyph bitmap to render
    /// * `fg` - Foreground color
    /// * `bg` - Background color
    pub fn draw_ttf_glyph(
        &self,
        pixel_x: usize,
        pixel_y: usize,
        glyph_data: &[u8],
        glyph_width: usize,
        glyph_height: usize,
        bearing_x: i16,
        bearing_y: i16,
        fg: PackedColor,
        _bg: PackedColor, // Background is already cleared by caller
    ) {
        if glyph_width == 0 || glyph_height == 0 || glyph_data.is_empty() {
            return;
        }

        // Calculate actual drawing position
        // bearing_x is horizontal offset from cursor to glyph left edge
        // bearing_y is vertical offset from baseline to glyph top edge (positive = above baseline)
        let draw_x = (pixel_x as i32 + bearing_x as i32).max(0) as usize;
        let draw_y_signed = pixel_y as i32 - bearing_y as i32;
        
        // Handle cases where glyph would start above the screen
        let (draw_y, skip_rows) = if draw_y_signed < 0 {
            (0usize, (-draw_y_signed) as usize)
        } else {
            (draw_y_signed as usize, 0usize)
        };

        // Bounds check
        if draw_x >= self.width || draw_y >= self.height || skip_rows >= glyph_height {
            return;
        }

        let max_width = (self.width - draw_x).min(glyph_width);
        let remaining_height = glyph_height - skip_rows;
        let max_height = (self.height - draw_y).min(remaining_height);

        if self.bytes_per_pixel == 4 {
            self.draw_ttf_glyph_32bpp(draw_x, draw_y, glyph_data, glyph_width, max_width, max_height, skip_rows, fg);
        } else {
            self.draw_ttf_glyph_generic(draw_x, draw_y, glyph_data, glyph_width, max_width, max_height, skip_rows, fg);
        }
    }

    /// 32bpp TTF glyph rendering with alpha blending
    #[inline(always)]
    fn draw_ttf_glyph_32bpp(
        &self,
        draw_x: usize,
        draw_y: usize,
        glyph_data: &[u8],
        glyph_pitch: usize,
        width: usize,
        height: usize,
        skip_rows: usize,
        fg: PackedColor,
    ) {
        let fg_r = fg.bytes[2] as u32;
        let fg_g = fg.bytes[1] as u32;
        let fg_b = fg.bytes[0] as u32;

        for y in 0..height {
            let row_offset = draw_y + y;
            if row_offset >= self.height {
                break;
            }

            let dst_row = row_offset * self.pitch + draw_x * 4;
            let src_row = (y + skip_rows) * glyph_pitch;

            unsafe {
                let dst_ptr = self.buffer.add(dst_row) as *mut u32;

                for x in 0..width {
                    if draw_x + x >= self.width {
                        break;
                    }

                    let alpha = glyph_data[src_row + x] as u32;

                    if alpha == 0 {
                        // Fully transparent - keep existing pixel (background already cleared)
                        continue;
                    } else if alpha == 255 {
                        // Fully opaque - use foreground
                        dst_ptr.add(x).write(u32::from_le_bytes(fg.bytes));
                    } else {
                        // Alpha blend with existing pixel (already contains background)
                        let existing = dst_ptr.add(x).read();
                        let bg_r = ((existing >> 16) & 0xFF) as u32;
                        let bg_g = ((existing >> 8) & 0xFF) as u32;
                        let bg_b = (existing & 0xFF) as u32;
                        
                        let inv_alpha = 255 - alpha;
                        let r = (fg_r * alpha + bg_r * inv_alpha) / 255;
                        let g = (fg_g * alpha + bg_g * inv_alpha) / 255;
                        let b = (fg_b * alpha + bg_b * inv_alpha) / 255;

                        let pixel = (r << 16) | (g << 8) | b;
                        dst_ptr.add(x).write(pixel);
                    }
                }
            }
        }
    }

    /// Generic TTF glyph rendering (non-32bpp)
    #[inline(always)]
    fn draw_ttf_glyph_generic(
        &self,
        draw_x: usize,
        draw_y: usize,
        glyph_data: &[u8],
        glyph_pitch: usize,
        width: usize,
        height: usize,
        skip_rows: usize,
        fg: PackedColor,
    ) {
        for y in 0..height {
            let row_offset = draw_y + y;
            if row_offset >= self.height {
                break;
            }

            let dst_row = row_offset * self.pitch + draw_x * self.bytes_per_pixel;
            let src_row = (y + skip_rows) * glyph_pitch;

            for x in 0..width {
                if draw_x + x >= self.width {
                    break;
                }

                let alpha = glyph_data[src_row + x];
                
                // Only draw if alpha is significant (threshold at 128)
                if alpha > 128 {
                    unsafe {
                        let dst = self.buffer.add(dst_row + x * self.bytes_per_pixel);
                        for i in 0..self.bytes_per_pixel {
                            dst.add(i).write_volatile(fg.bytes[i]);
                        }
                    }
                }
            }
        }
    }

    /// Clear a rectangular area for TTF rendering
    pub fn clear_rect(&self, x: usize, y: usize, width: usize, height: usize, bg: PackedColor) {
        if x >= self.width || y >= self.height {
            return;
        }
        let actual_width = width.min(self.width - x);
        let actual_height = height.min(self.height - y);
        self.fill_rect(x, y, actual_width, actual_height, bg);
    }

    /// High-performance rectangle fill
    ///
    /// Strategies:
    /// 1. Use 64-bit batch writes
    /// 2. Fill first row, then copy to other rows
    pub fn fill_rect(
        &self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: PackedColor,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let end_x = start_x.saturating_add(width).min(self.width);
        let end_y = start_y.saturating_add(height).min(self.height);
        let actual_width = end_x.saturating_sub(start_x);
        let actual_height = end_y.saturating_sub(start_y);

        if actual_width == 0 || actual_height == 0 {
            return;
        }

        if self.bytes_per_pixel == 4 {
            self.fill_rect_fast_32bpp(start_x, start_y, actual_width, actual_height, color);
        } else {
            self.fill_rect_generic(start_x, start_y, actual_width, actual_height, color);
        }
    }

    /// 32bpp fast rectangle fill - GPU-style optimization
    #[inline(always)]
    fn fill_rect_fast_32bpp(
        &self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: PackedColor,
    ) {
        let color_u32 = u32::from_le_bytes(color.bytes);

        #[cfg(feature = "gfx_compositor")]
        {
            let total_pixels = width * height;

            // Lower parallel threshold for 2.5K resolution
            let parallel_threshold = if start_x == 0 && width == self.width {
                1024 // Full-width: more aggressive parallelization
            } else {
                2048 // Non-full-width: slightly higher threshold
            };

            // Use compositor multi-core fill for large areas
            if total_pixels >= parallel_threshold && height >= 4 {
                if start_x == 0 && width == self.width {
                    let aligned_buffer = unsafe { self.buffer.add(start_y * self.pitch) };
                    compositor::parallel_fill(
                        aligned_buffer,
                        self.pitch,
                        width,
                        height,
                        self.bytes_per_pixel,
                        color_u32,
                    );
                } else {
                    self.fill_rect_optimized(start_x, start_y, width, height, color_u32);
                }
                return;
            }
        }

        self.fill_rect_optimized(start_x, start_y, width, height, color_u32);
    }

    /// Optimized single-core rectangle fill - for small or non-aligned areas
    #[inline(always)]
    fn fill_rect_optimized(
        &self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color_u32: u32,
    ) {
        let color_u64 = (color_u32 as u64) | ((color_u32 as u64) << 32);
        let row_bytes = width * 4;

        let first_row_offset = start_y * self.pitch + start_x * 4;
        unsafe {
            let first_row_ptr = self.buffer.add(first_row_offset);

            // Check if pointer is 8-byte aligned for 64-bit writes
            let ptr_addr = first_row_ptr as usize;
            let is_aligned = (ptr_addr & 7) == 0;

            if is_aligned && width >= 2 {
                // Fast path: 64-bit writes when aligned
                let qwords = width / 2;
                let remainder = width % 2;
                let qword_ptr = first_row_ptr as *mut u64;

                let batches = qwords / 4;
                let batch_remainder = qwords % 4;

                for batch in 0..batches {
                    let base = batch * 4;
                    qword_ptr.add(base).write(color_u64);
                    qword_ptr.add(base + 1).write(color_u64);
                    qword_ptr.add(base + 2).write(color_u64);
                    qword_ptr.add(base + 3).write(color_u64);
                }

                let batch_base = batches * 4;
                for i in 0..batch_remainder {
                    qword_ptr.add(batch_base + i).write(color_u64);
                }

                if remainder > 0 {
                    let dword_ptr = first_row_ptr.add(qwords * 8) as *mut u32;
                    dword_ptr.write(color_u32);
                }
            } else {
                // Slow path: 32-bit writes when unaligned or small width
                let dword_ptr = first_row_ptr as *mut u32;
                for x in 0..width {
                    dword_ptr.add(x).write(color_u32);
                }
            }

            // Copy first row to remaining rows
            for row in 1..height {
                let dst_offset = (start_y + row) * self.pitch + start_x * 4;
                core::ptr::copy_nonoverlapping(
                    first_row_ptr,
                    self.buffer.add(dst_offset),
                    row_bytes,
                );
            }
        }
    }

    /// Generic rectangle fill (non-32bpp)
    #[inline(always)]
    fn fill_rect_generic(
        &self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: PackedColor,
    ) {
        let row_bytes = width * self.bytes_per_pixel;
        let first_row_offset = start_y * self.pitch + start_x * self.bytes_per_pixel;

        unsafe {
            let first_row_ptr = self.buffer.add(first_row_offset);

            // Fill first row pixel by pixel
            for x in 0..width {
                let pixel_ptr = first_row_ptr.add(x * self.bytes_per_pixel);
                for i in 0..self.bytes_per_pixel {
                    pixel_ptr.add(i).write_volatile(color.bytes[i]);
                }
            }

            // Copy first row to remaining rows
            for row in 1..height {
                let dst_offset = (start_y + row) * self.pitch + start_x * self.bytes_per_pixel;
                core::ptr::copy_nonoverlapping(
                    first_row_ptr,
                    self.buffer.add(dst_offset),
                    row_bytes,
                );
            }
        }
    }

    /// Scroll the entire screen up by one text row
    pub fn scroll_up(&self, bg_color: u32) {
        #[cfg(feature = "gfx_compositor")]
        {
            compositor::scroll_up_fast(
                self.buffer,
                self.pitch,
                self.width,
                self.height,
                self.bytes_per_pixel,
                CELL_HEIGHT,
                bg_color,
            );
        }
        #[cfg(not(feature = "gfx_compositor"))]
        {
            // Fallback: simple memmove-style scroll
            let row_bytes = self.width * self.bytes_per_pixel;
            let scroll_bytes = CELL_HEIGHT * self.pitch;
            let copy_rows = self.height.saturating_sub(CELL_HEIGHT);
            
            unsafe {
                let src = self.buffer.add(scroll_bytes);
                core::ptr::copy(src, self.buffer, copy_rows * self.pitch);
                
                // Clear bottom rows
                let clear_start = self.buffer.add(copy_rows * self.pitch);
                let clear_bytes = CELL_HEIGHT * self.pitch;
                core::ptr::write_bytes(clear_start, 0, clear_bytes);
            }
            let _ = bg_color; // Suppress unused warning in fallback
        }
    }

    /// Fast full-screen clear using parallel fill
    pub fn clear_screen(&self, bg_color: u32) {
        #[cfg(feature = "gfx_compositor")]
        {
            compositor::parallel_fill(
                self.buffer,
                self.pitch,
                self.width,
                self.height,
                self.bytes_per_pixel,
                bg_color,
            );
        }
        #[cfg(not(feature = "gfx_compositor"))]
        {
            // Fallback: simple memset clear
            let total_bytes = self.height * self.pitch;
            unsafe {
                core::ptr::write_bytes(self.buffer, 0, total_bytes);
            }
            let _ = bg_color; // Suppress unused warning in fallback
        }
    }

    /// Clear a range of text rows
    pub fn clear_rows(&self, start_row: usize, end_row: usize, bg: PackedColor) {
        if start_row >= end_row {
            return;
        }

        let pixel_y_start = start_row * CELL_HEIGHT;
        let pixel_y_end = end_row * CELL_HEIGHT;
        let height = pixel_y_end
            .saturating_sub(pixel_y_start)
            .min(self.height - pixel_y_start);

        if height == 0 {
            return;
        }

        self.fill_rect(0, pixel_y_start, self.width, height, bg);
    }
}
