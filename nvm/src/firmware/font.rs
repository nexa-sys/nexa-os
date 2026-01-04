//! VGA ROM Font Data
//!
//! Provides 8x16 bitmap font for VGA text mode rendering.
//! This is the standard IBM VGA font used in BIOS.

/// Font dimensions
pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;
pub const FONT_CHARS: usize = 256;

/// VGA Font structure
pub struct VgaFont {
    /// Glyph data: 256 characters × 16 rows × 1 byte (8 pixels)
    pub glyphs: &'static [u8],
    pub width: usize,
    pub height: usize,
}

impl VgaFont {
    /// Get glyph bitmap for a character
    pub fn get_glyph(&self, ch: u8) -> &[u8] {
        let start = (ch as usize) * self.height;
        let end = start + self.height;
        if end <= self.glyphs.len() {
            &self.glyphs[start..end]
        } else {
            &self.glyphs[0..self.height] // Return first char as fallback
        }
    }
    
    /// Check if pixel is set in glyph
    pub fn pixel_set(&self, ch: u8, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let glyph = self.get_glyph(ch);
        (glyph[y] >> (7 - x)) & 1 != 0
    }
}

/// Get font reference
pub fn get_vga_font() -> VgaFont {
    VgaFont {
        glyphs: &*FONT_8X16_DATA,
        width: FONT_WIDTH,
        height: FONT_HEIGHT,
    }
}

include!("font_data.inc");

/// Get initialized font data
pub fn init_font() -> [u8; 4096] {
    get_font_data()
}

/// Lazy-initialized font data
pub static FONT_8X16_DATA: std::sync::LazyLock<[u8; 4096]> = 
    std::sync::LazyLock::new(|| get_font_data());
