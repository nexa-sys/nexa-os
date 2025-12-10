//! TrueType Font (TTF) support for framebuffer rendering
//!
//! This module provides TTF font parsing and rendering for displaying
//! Chinese characters, special symbols, and extended Unicode characters.
//!
//! # Architecture
//!
//! The font system is designed to work in two modes:
//! 1. **Boot mode** (pre-pivot_root): Uses embedded 8x8 bitmap font for ASCII
//! 2. **Runtime mode** (post-pivot_root): Loads TTF fonts from /etc/fonts
//!
//! # Module Organization
//!
//! - `ttf`: TrueType font file parser
//! - `rasterizer`: Converts glyph outlines to bitmaps
//! - `config`: Parses fontconfig XML files
//! - `manager`: Font loading and caching
//! - `glyph`: Glyph bitmap representation

pub mod config;
pub mod glyph;
pub mod manager;
pub mod rasterizer;
pub mod ttf;

use spin::Mutex;

pub use config::FontConfig;
pub use glyph::{GlyphBitmap, GlyphCache};
pub use manager::FontManager;
pub use rasterizer::Rasterizer;
pub use ttf::{TtfError, TtfFont};

/// Global font manager instance
static FONT_MANAGER: Mutex<Option<FontManager>> = Mutex::new(None);

/// Font system state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSystemState {
    /// Font system not initialized (using fallback bitmap font)
    Uninitialized,
    /// Font system is initializing
    Initializing,
    /// Font system ready with TTF support
    Ready,
    /// Font system failed to initialize (using fallback)
    Failed,
}

static FONT_STATE: Mutex<FontSystemState> = Mutex::new(FontSystemState::Uninitialized);

/// Check if the font system is ready for TTF rendering
pub fn is_ready() -> bool {
    *FONT_STATE.lock() == FontSystemState::Ready
}

/// Get current font system state
pub fn state() -> FontSystemState {
    *FONT_STATE.lock()
}

/// Initialize font system after pivot_root
///
/// This function should be called after the real root filesystem is mounted
/// and /etc/fonts/fonts.conf is accessible.
pub fn init_after_pivot_root() {
    let mut state = FONT_STATE.lock();
    if *state != FontSystemState::Uninitialized {
        return;
    }
    *state = FontSystemState::Initializing;
    drop(state);

    crate::kinfo!("Font system: initializing TTF support");

    // Load font configuration
    let config = match FontConfig::load_from_file("/etc/fonts/fonts.conf") {
        Some(cfg) => {
            crate::kinfo!("Font system: loaded fonts.conf");
            cfg
        }
        None => {
            crate::kwarn!("Font system: failed to load fonts.conf, using defaults");
            FontConfig::default()
        }
    };

    // Create font manager and load fonts
    let mut manager = FontManager::new(config);

    if let Err(e) = manager.load_fonts() {
        crate::kwarn!("Font system: failed to load fonts: {:?}", e);
        *FONT_STATE.lock() = FontSystemState::Failed;
        return;
    }

    // Store manager globally
    *FONT_MANAGER.lock() = Some(manager);
    *FONT_STATE.lock() = FontSystemState::Ready;

    crate::kinfo!("Font system: TTF support ready");
}

/// Get a glyph bitmap for a character
///
/// Returns None if font system is not ready or character not available
pub fn get_glyph(ch: char, size: u16) -> Option<GlyphBitmap> {
    let manager = FONT_MANAGER.lock();
    manager.as_ref()?.get_glyph(ch, size)
}

/// Check if a character can be rendered with TTF fonts
pub fn can_render(ch: char) -> bool {
    if !is_ready() {
        return false;
    }
    let manager = FONT_MANAGER.lock();
    manager.as_ref().map_or(false, |m| m.has_glyph(ch))
}

/// Get the line height for a given font size
pub fn get_line_height(size: u16) -> u16 {
    let manager = FONT_MANAGER.lock();
    manager.as_ref().map_or(size, |m| m.get_line_height(size))
}

/// Get the baseline offset from the top of a cell
pub fn get_baseline_offset(size: u16) -> u16 {
    let manager = FONT_MANAGER.lock();
    manager
        .as_ref()
        .map_or(size, |m| m.get_baseline_offset(size))
}

/// Get the advance width for a character at a given size
pub fn get_advance(ch: char, size: u16) -> u16 {
    let manager = FONT_MANAGER.lock();
    manager
        .as_ref()
        .map_or(size / 2, |m| m.get_advance(ch, size))
}

/// Check if a character is a wide (fullwidth) character
///
/// East Asian Wide characters should occupy 2 cells in a terminal.
/// This includes CJK Unified Ideographs, Hangul, Katakana, Hiragana, etc.
#[inline]
pub fn is_wide_char(ch: char) -> bool {
    let cp = ch as u32;

    // Fast path for ASCII
    if cp < 0x80 {
        return false;
    }

    // East Asian Wide characters
    matches!(cp,
        // CJK Radicals Supplement, Kangxi Radicals, Ideographic Description
        0x2E80..=0x2FFF |
        // CJK Symbols and Punctuation (most)
        0x3000..=0x303F |
        // Hiragana
        0x3040..=0x309F |
        // Katakana
        0x30A0..=0x30FF |
        // Bopomofo
        0x3100..=0x312F |
        // Hangul Compatibility Jamo
        0x3130..=0x318F |
        // Kanbun, Bopomofo Extended, CJK Strokes, Katakana Phonetic Extensions
        0x3190..=0x31FF |
        // Enclosed CJK Letters and Months
        0x3200..=0x32FF |
        // CJK Compatibility
        0x3300..=0x33FF |
        // CJK Unified Ideographs Extension A
        0x3400..=0x4DBF |
        // CJK Unified Ideographs
        0x4E00..=0x9FFF |
        // Yi Syllables and Radicals
        0xA000..=0xA4CF |
        // Hangul Jamo Extended-A
        0xA960..=0xA97F |
        // Hangul Syllables
        0xAC00..=0xD7AF |
        // Hangul Jamo Extended-B
        0xD7B0..=0xD7FF |
        // CJK Compatibility Ideographs
        0xF900..=0xFAFF |
        // Fullwidth Forms (Halfwidth and Fullwidth Forms, wide part)
        0xFF00..=0xFF60 |
        0xFFE0..=0xFFE6 |
        // CJK Unified Ideographs Extension B, C, D, E, F (SIP) and Compatibility Supplement
        0x20000..=0x2FA1F
    )
}
