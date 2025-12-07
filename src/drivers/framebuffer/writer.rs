//! Framebuffer text writer
//!
//! The main FramebufferWriter struct that combines color management,
//! ANSI parsing, and rendering to provide a text console interface.
//! Supports both legacy 8x8 bitmap fonts and TTF fonts for Unicode.

use core::fmt::{self, Write};
use font8x8::legacy::BASIC_LEGACY;

use super::ansi::{color_from_sgr, AnsiParser, AnsiState, SgrAction, SgrProcessor};
use super::color::{pack_color, PackedColor, RgbColor, DEFAULT_BG, DEFAULT_FG};
use super::font;
use super::render::{RenderContext, CELL_HEIGHT, CELL_WIDTH};
use super::spec::FramebufferSpec;
use crate::drivers::compositor;
use crate::ktrace;

/// Tab width in characters
const TAB_WIDTH: usize = 4;

/// Maximum characters per line for width history
const MAX_LINE_CHARS: usize = 256;

/// Framebuffer text console writer
///
/// Provides a text console interface on top of the framebuffer,
/// including ANSI escape sequence support for colors and cursor control.
pub struct FramebufferWriter {
    render: RenderContext,
    spec: FramebufferSpec,
    cursor_x: usize,      // Cell-based cursor X (for bitmap font compatibility)
    cursor_y: usize,      // Cell-based cursor Y (row number)
    pixel_x: usize,       // Pixel-based cursor X (for TTF fonts)
    columns: usize,
    rows: usize,
    fg: PackedColor,
    bg: PackedColor,
    fg_rgb: RgbColor,
    bg_rgb: RgbColor,
    default_fg_rgb: RgbColor,
    default_bg_rgb: RgbColor,
    bold: bool,
    ansi: AnsiParser,
    /// Character width history for current line (for accurate backspace)
    char_widths: [u8; MAX_LINE_CHARS],
    /// Number of characters in current line
    char_count: usize,
}

unsafe impl Send for FramebufferWriter {}

impl FramebufferWriter {
    /// Create a new framebuffer writer
    ///
    /// Returns None if the framebuffer spec is invalid (bpp < 16 or zero dimensions)
    pub fn new(buffer: *mut u8, spec: FramebufferSpec) -> Option<Self> {
        if spec.bpp < 16 {
            return None;
        }

        let bytes_per_pixel = ((spec.bpp + 7) / 8) as usize;
        let columns = (spec.width as usize) / CELL_WIDTH;
        let rows = (spec.height as usize) / CELL_HEIGHT;

        if columns == 0 || rows == 0 {
            return None;
        }

        let render = RenderContext::new(buffer, &spec);

        let default_fg = PackedColor::new(
            pack_color(&spec.red, &spec.green, &spec.blue, DEFAULT_FG.r, DEFAULT_FG.g, DEFAULT_FG.b),
            bytes_per_pixel,
        );
        let default_bg = PackedColor::new(
            pack_color(&spec.red, &spec.green, &spec.blue, DEFAULT_BG.r, DEFAULT_BG.g, DEFAULT_BG.b),
            bytes_per_pixel,
        );

        Some(Self {
            render,
            spec,
            cursor_x: 0,
            cursor_y: 0,
            pixel_x: 0,
            columns,
            rows,
            fg: default_fg,
            bg: default_bg,
            fg_rgb: DEFAULT_FG,
            bg_rgb: DEFAULT_BG,
            default_fg_rgb: DEFAULT_FG,
            default_bg_rgb: DEFAULT_BG,
            bold: false,
            ansi: AnsiParser::new(),
            char_widths: [0; MAX_LINE_CHARS],
            char_count: 0,
        })
    }

    /// Pack an RGB color for this framebuffer's pixel format
    fn pack_rgb(&self, color: RgbColor) -> PackedColor {
        PackedColor::new(
            pack_color(
                &self.spec.red,
                &self.spec.green,
                &self.spec.blue,
                color.r,
                color.g,
                color.b,
            ),
            self.render.bytes_per_pixel,
        )
    }

    /// Set both foreground and background colors
    fn set_colors(&mut self, fg: RgbColor, bg: RgbColor) {
        self.fg = self.pack_rgb(fg);
        self.bg = self.pack_rgb(bg);
        self.fg_rgb = fg;
        self.bg_rgb = bg;
    }

    /// Reset colors and attributes to defaults
    fn reset_colors(&mut self) {
        self.bold = false;
        self.set_colors(self.default_fg_rgb, self.default_bg_rgb);
    }

    /// Set foreground color
    fn set_fg_color(&mut self, color: RgbColor) {
        self.fg = self.pack_rgb(color);
        self.fg_rgb = color;
    }

    /// Set background color
    fn set_bg_color(&mut self, color: RgbColor) {
        self.bg = self.pack_rgb(color);
        self.bg_rgb = color;
    }

    /// Reset foreground to default
    fn reset_fg(&mut self) {
        self.set_fg_color(self.default_fg_rgb);
    }

    /// Reset background to default
    fn reset_bg(&mut self) {
        self.set_bg_color(self.default_bg_rgb);
    }

    /// Move to the next line, scrolling if necessary
    fn newline(&mut self) {
        self.cursor_x = 0;
        self.pixel_x = 0;
        self.char_count = 0; // Reset character width history for new line
        if self.cursor_y + 1 >= self.rows {
            // Before multi-core compositor init: clear instead of scroll for performance
            // After compositor init: use proper scrolling
            if compositor::is_initialized() {
                self.scroll_up();
            } else {
                self.clear();
            }
        } else {
            self.cursor_y += 1;
        }
    }

    /// Record a character's advance width for backspace support
    #[inline]
    fn push_char_width(&mut self, width: usize) {
        if self.char_count < MAX_LINE_CHARS {
            self.char_widths[self.char_count] = width.min(255) as u8;
            self.char_count += 1;
        }
    }

    /// Pop the last character's width for backspace
    #[inline]
    fn pop_char_width(&mut self) -> usize {
        if self.char_count > 0 {
            self.char_count -= 1;
            self.char_widths[self.char_count] as usize
        } else {
            CELL_WIDTH // Default fallback
        }
    }

    /// Scroll the screen up by one text row
    fn scroll_up(&mut self) {
        let bg_color = u32::from_le_bytes(self.bg.bytes);
        self.render.scroll_up(bg_color);
    }

    /// Write a single character at the cursor position
    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => {
                self.cursor_x = 0;
                self.pixel_x = 0;
            }
            '\t' => {
                let next_tab = ((self.cursor_x / TAB_WIDTH) + 1) * TAB_WIDTH;
                while self.cursor_x < next_tab {
                    self.write_char(' ');
                }
            }
            _ => {
                if self.cursor_x >= self.columns {
                    self.newline();
                }
                let glyph = if (c as usize) < BASIC_LEGACY.len() {
                    &BASIC_LEGACY[c as usize]
                } else {
                    &BASIC_LEGACY[b'?' as usize]
                };
                self.render
                    .draw_cell(self.cursor_x, self.cursor_y, glyph, self.fg, self.bg);
                self.cursor_x += 1;
            }
        }
    }

    /// Write a Unicode character using TTF fonts if available
    fn write_unicode_char(&mut self, ch: char) {
        // Handle control characters first
        match ch {
            '\n' => {
                self.newline();
                return;
            }
            '\r' => {
                self.cursor_x = 0;
                self.pixel_x = 0;
                return;
            }
            '\t' => {
                // Tab: advance to next tab stop (every TAB_WIDTH cells)
                let current_cell = self.pixel_x / CELL_WIDTH;
                let next_tab = ((current_cell / TAB_WIDTH) + 1) * TAB_WIDTH;
                let target_pixel = next_tab * CELL_WIDTH;
                
                // Clear the tab area
                let row_y = self.cursor_y * CELL_HEIGHT;
                if target_pixel > self.pixel_x {
                    self.render.fill_rect(
                        self.pixel_x, row_y,
                        target_pixel - self.pixel_x, CELL_HEIGHT,
                        self.bg
                    );
                }
                
                self.pixel_x = target_pixel;
                self.cursor_x = next_tab;
                
                if self.pixel_x >= self.render.width {
                    self.newline();
                }
                return;
            }
            '\x08' => {
                // Backspace - use recorded character width for accurate deletion
                // Only process if we have characters to delete on this line
                if self.char_count > 0 {
                    let char_width = self.pop_char_width();
                    if self.pixel_x >= char_width {
                        self.pixel_x -= char_width;
                    } else {
                        self.pixel_x = 0;
                    }
                    self.cursor_x = self.pixel_x / CELL_WIDTH;
                    // Clear the area
                    let row_y = self.cursor_y * CELL_HEIGHT;
                    self.render.fill_rect(self.pixel_x, row_y, char_width, CELL_HEIGHT, self.bg);
                }
                // If char_count == 0, do nothing (already at start of editable area)
                return;
            }
            _ => {}
        }

        // Check if TTF font system is ready
        if !font::is_ready() {
            // Fall back to bitmap font (cell-based)
            if (ch as u32) <= 0x7F {
                self.write_char(ch);
                self.pixel_x = self.cursor_x * CELL_WIDTH;
                self.push_char_width(CELL_WIDTH);
            } else {
                self.write_char('?');
                self.pixel_x = self.cursor_x * CELL_WIDTH;
                self.push_char_width(CELL_WIDTH);
            }
            return;
        }

        // Try to get TTF glyph
        let font_size = CELL_HEIGHT as u16;
        if let Some(glyph) = font::get_glyph(ch, font_size) {
            // Calculate the advance for this glyph
            let advance = glyph.advance as usize;
            
            // Check if we need to wrap to next line
            if self.pixel_x + advance > self.render.width {
                self.newline();
            }
            
            // Handle empty glyphs (like space)
            if glyph.width == 0 || glyph.height == 0 {
                // Clear the area and advance
                let row_y = self.cursor_y * CELL_HEIGHT;
                self.render.fill_rect(self.pixel_x, row_y, advance.max(1), CELL_HEIGHT, self.bg);
                self.pixel_x += advance.max(1);
                self.cursor_x = self.pixel_x / CELL_WIDTH;
                self.push_char_width(advance.max(1));
                return;
            }

            // Calculate drawing position
            // Use baseline from font metrics
            let baseline_offset = font::get_baseline_offset(font_size);
            let pixel_y = self.cursor_y * CELL_HEIGHT + baseline_offset as usize;

            // Clear background area for this glyph
            let row_y = self.cursor_y * CELL_HEIGHT;
            let clear_width = advance.max(glyph.width as usize + glyph.bearing_x.max(0) as usize);
            self.render.fill_rect(self.pixel_x, row_y, clear_width, CELL_HEIGHT, self.bg);

            // Draw the TTF glyph at pixel position
            self.render.draw_ttf_glyph(
                self.pixel_x,
                pixel_y,
                &glyph.data,
                glyph.width as usize,
                glyph.height as usize,
                glyph.bearing_x,
                glyph.bearing_y,
                self.fg,
                self.bg,
            );

            // Advance cursor by glyph's advance width
            self.pixel_x += advance;
            self.cursor_x = self.pixel_x / CELL_WIDTH;
            self.push_char_width(advance);
            
            // Check for line wrap
            if self.pixel_x >= self.render.width {
                self.newline();
            }
        } else {
            // Glyph not found, use placeholder
            self.write_char('?');
            self.pixel_x = self.cursor_x * CELL_WIDTH;
            self.push_char_width(CELL_WIDTH);
        }
    }

    /// Process a single byte through the ANSI state machine
    pub fn process_byte(&mut self, byte: u8) {
        match self.ansi.state {
            AnsiState::Ground => match byte {
                0x1B => {
                    self.ansi.state = AnsiState::Escape;
                }
                0x08 => self.backspace(),
                b'\n' | b'\r' | b'\t' => self.write_char(byte as char),
                0x20..=0x7E => self.write_char(byte as char),
                // Handle control characters (0x00-0x1F except handled above, and 0x7F)
                0x00..=0x07 | 0x09..=0x0C | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F => {
                    // Skip control characters silently
                }
                // Handle extended ASCII and UTF-8 continuation bytes (0x80-0xFF)
                // These should be rendered as a placeholder character
                0x80..=0xFF => self.write_char('?'),
            },
            AnsiState::Escape => {
                if byte == b'[' {
                    self.ansi.state = AnsiState::Csi;
                    self.ansi.param_len = 0;
                } else {
                    self.ansi.state = AnsiState::Ground;
                    self.process_byte(byte);
                }
            }
            AnsiState::Csi => match byte {
                b'0'..=b'9' | b';' => {
                    self.ansi.push_param(byte);
                }
                b'm' | b'J' | b'K' => {
                    let (params, count) = self.ansi.parse_params();
                    self.handle_csi(byte, &params[..count]);
                    self.ansi.reset();
                }
                0x1B => {
                    // Restart escape sequence if new ESC arrives mid-CSI
                    self.ansi.state = AnsiState::Escape;
                    self.ansi.param_len = 0;
                }
                _ => {
                    self.ansi.reset();
                }
            },
        }
    }

    /// Handle a complete CSI sequence
    fn handle_csi(&mut self, command: u8, params: &[u16]) {
        match command {
            b'm' => self.apply_sgr(params),
            b'J' => self.handle_erase_display(params),
            b'K' => self.handle_erase_line(params),
            _ => {}
        }
    }

    /// Handle ED (Erase in Display) sequence
    fn handle_erase_display(&mut self, params: &[u16]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            2 => self.clear(),
            0 => {
                // Clear from cursor to end of screen
                let start_pixel_y = self.cursor_y * CELL_HEIGHT;
                let start_pixel_x = self.pixel_x;
                let width = self.render.width.saturating_sub(start_pixel_x);
                self.render
                    .fill_rect(start_pixel_x, start_pixel_y, width, CELL_HEIGHT, self.bg);
                self.render.clear_rows(self.cursor_y + 1, self.rows, self.bg);
            }
            1 => {
                // Clear from start to cursor
                self.render.clear_rows(0, self.cursor_y, self.bg);
                let start_pixel_y = self.cursor_y * CELL_HEIGHT;
                self.render.fill_rect(
                    0,
                    start_pixel_y,
                    self.pixel_x,
                    CELL_HEIGHT,
                    self.bg,
                );
            }
            _ => {}
        }
    }

    /// Handle EL (Erase in Line) sequence
    fn handle_erase_line(&mut self, params: &[u16]) {
        let mode = params.first().copied().unwrap_or(0);
        let pixel_y = self.cursor_y * CELL_HEIGHT;

        match mode {
            0 => {
                // Clear from cursor to end of line
                let start_pixel_x = self.pixel_x;
                let width = self.render.width.saturating_sub(start_pixel_x);
                self.render
                    .fill_rect(start_pixel_x, pixel_y, width, CELL_HEIGHT, self.bg);
            }
            1 => {
                // Clear from start to cursor
                self.render.fill_rect(
                    0,
                    pixel_y,
                    self.pixel_x,
                    CELL_HEIGHT,
                    self.bg,
                );
            }
            2 => {
                // Clear entire line
                self.render
                    .fill_rect(0, pixel_y, self.render.width, CELL_HEIGHT, self.bg);
            }
            _ => {}
        }
    }

    /// Apply SGR (Select Graphic Rendition) parameters
    fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset_colors();
            return;
        }

        let processor = SgrProcessor::new(params, self.bold);

        for action in processor {
            match action {
                SgrAction::Reset => self.reset_colors(),
                SgrAction::Bold => self.bold = true,
                SgrAction::NoBold => self.bold = false,
                SgrAction::SetFg(idx, bright) => {
                    self.set_fg_color(color_from_sgr(idx, bright));
                }
                SgrAction::SetBg(idx, bright) => {
                    self.set_bg_color(color_from_sgr(idx, bright));
                }
                SgrAction::SetFgRgb(color) => self.set_fg_color(color),
                SgrAction::SetBgRgb(color) => self.set_bg_color(color),
                SgrAction::ResetFg => self.reset_fg(),
                SgrAction::ResetBg => self.reset_bg(),
                SgrAction::None => {}
            }
        }

        // Update bold state from processor
        // Note: This is a simplified approach; in practice the processor
        // tracks bold state internally
    }

    /// Handle backspace
    pub fn backspace(&mut self) {
        // Use recorded character width for accurate deletion
        // Only process if we have characters to delete on this line
        if self.char_count > 0 {
            let char_width = self.pop_char_width();
            if self.pixel_x >= char_width {
                self.pixel_x -= char_width;
            } else {
                self.pixel_x = 0;
            }
            self.cursor_x = self.pixel_x / CELL_WIDTH;
            // Clear the area
            let row_y = self.cursor_y * CELL_HEIGHT;
            self.render.fill_rect(self.pixel_x, row_y, char_width, CELL_HEIGHT, self.bg);
        }
        // If char_count == 0, do nothing (already at start of editable area)
    }

    /// Clear the entire screen and reset cursor
    pub fn clear(&mut self) {
        ktrace!(
            "FBWRITER::clear buf={:#x} pitch={} cols={} rows={} bytes_pp={}",
            self.render.buffer as usize,
            self.render.pitch,
            self.columns,
            self.rows,
            self.render.bytes_per_pixel
        );

        let bg_color = u32::from_le_bytes(self.bg.bytes);
        self.render.clear_screen(bg_color);

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.pixel_x = 0;
        self.char_count = 0;
        self.reset_colors();
        self.ansi.reset();
    }
}

impl Write for FramebufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Debug: track when TTF becomes ready
        static WAS_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
        let is_ready = font::is_ready();
        if is_ready && !WAS_READY.swap(true, core::sync::atomic::Ordering::Relaxed) {
            crate::kinfo!("write_str: TTF is now READY, switching to TTF rendering");
        }

        for ch in s.chars() {
            // Always go through ANSI state machine first
            match self.ansi.state {
                AnsiState::Ground => {
                    if ch == '\u{1B}' {
                        // Start of escape sequence
                        self.ansi.state = AnsiState::Escape;
                    } else if is_ready {
                        // TTF font system is ready - use TTF for ALL characters
                        self.write_unicode_char(ch);
                    } else if (ch as u32) <= 0x7F {
                        // Fallback to bitmap font for ASCII when TTF not ready
                        self.process_byte(ch as u8);
                    } else {
                        // Non-ASCII without TTF - show placeholder
                        self.write_char('?');
                    }
                }
                AnsiState::Escape => {
                    if ch == '[' {
                        self.ansi.state = AnsiState::Csi;
                        self.ansi.param_len = 0;
                    } else {
                        // Not a valid CSI sequence, reset and process character
                        self.ansi.state = AnsiState::Ground;
                        if is_ready {
                            self.write_unicode_char(ch);
                        } else if (ch as u32) <= 0x7F {
                            self.process_byte(ch as u8);
                        }
                    }
                }
                AnsiState::Csi => {
                    match ch {
                        '0'..='9' | ';' => {
                            self.ansi.push_param(ch as u8);
                        }
                        'm' | 'J' | 'K' | 'H' | 'A' | 'B' | 'C' | 'D' => {
                            let (params, count) = self.ansi.parse_params();
                            self.handle_csi(ch as u8, &params[..count]);
                            self.ansi.reset();
                        }
                        '\u{1B}' => {
                            // New escape sequence starts
                            self.ansi.state = AnsiState::Escape;
                            self.ansi.param_len = 0;
                        }
                        _ => {
                            // Unknown CSI terminator, reset
                            self.ansi.reset();
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
