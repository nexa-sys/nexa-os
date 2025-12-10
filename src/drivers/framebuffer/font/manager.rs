//! Font manager
//!
//! This module manages font loading, caching, and selection.
//! It provides the high-level interface for getting rendered glyphs.

use alloc::string::String;
use alloc::vec::Vec;

use super::config::FontConfig;
use super::glyph::{GlyphBitmap, GlyphCache};
use super::rasterizer::Rasterizer;
use super::ttf::{TtfError, TtfFont};

/// Maximum number of loaded fonts
const MAX_LOADED_FONTS: usize = 4;

/// Default font size in pixels
const DEFAULT_FONT_SIZE: u16 = 16;

/// Font manager errors
#[derive(Debug, Clone, Copy)]
pub enum FontManagerError {
    /// No fonts found
    NoFontsFound,
    /// Failed to load font file
    LoadFailed,
    /// Failed to parse font
    ParseFailed(TtfError),
}

/// A loaded font with its metadata
struct LoadedFont {
    /// Font name (from filename)
    name: String,
    /// Parsed font data
    font: TtfFont,
    /// Priority (lower = higher priority)
    priority: u8,
}

/// Font manager for loading and rendering fonts
pub struct FontManager {
    /// Font configuration
    config: FontConfig,
    /// Loaded fonts
    fonts: Vec<LoadedFont>,
    /// Glyph cache
    cache: GlyphCache,
    /// Default font size
    font_size: u16,
}

impl FontManager {
    /// Create a new font manager with the given configuration
    pub fn new(config: FontConfig) -> Self {
        Self {
            config,
            fonts: Vec::new(),
            cache: GlyphCache::new(),
            font_size: DEFAULT_FONT_SIZE,
        }
    }

    /// Load fonts from configured directories
    pub fn load_fonts(&mut self) -> Result<(), FontManagerError> {
        crate::kinfo!("Font manager: scanning font directories");

        for dir in self.config.directories.clone() {
            self.scan_directory(&dir);
        }

        if self.fonts.is_empty() {
            crate::kwarn!("Font manager: no fonts found, trying fallback paths");
            
            // Try common fallback paths
            let fallbacks = [
                "/usr/share/fonts/truetype/HarmonyOS_Sans_SC_Bold.ttf",
                "/usr/share/fonts/HarmonyOS_Sans_SC_Bold.ttf",
            ];

            for path in fallbacks {
                if self.try_load_font(path).is_ok() {
                    break;
                }
            }
        }

        if self.fonts.is_empty() {
            return Err(FontManagerError::NoFontsFound);
        }

        // Sort fonts by priority
        self.fonts.sort_by_key(|f| f.priority);

        crate::kinfo!(
            "Font manager: loaded {} font(s)",
            self.fonts.len()
        );

        Ok(())
    }

    /// Scan a directory for font files
    fn scan_directory(&mut self, dir: &str) {
        // Collect directory entries using callback API
        let mut entries: Vec<String> = Vec::new();
        crate::fs::list_directory(dir, |name, metadata| {
            entries.push(String::from(name));
        });

        if entries.is_empty() {
            return;
        }

        for entry in entries {
            let path = if dir.ends_with('/') {
                alloc::format!("{}{}", dir, entry)
            } else {
                alloc::format!("{}/{}", dir, entry)
            };

            // Check if it's a TTF file
            if entry.ends_with(".ttf") || entry.ends_with(".TTF") {
                if self.fonts.len() < MAX_LOADED_FONTS {
                    let _ = self.try_load_font(&path);
                }
            } else if !entry.contains('.') {
                // Likely a subdirectory (no extension), try recursively
                self.scan_directory(&path);
            }
        }
    }

    /// Try to load a font file
    fn try_load_font(&mut self, path: &str) -> Result<(), FontManagerError> {
        // Extract font name from path first to check for duplicates
        let name = path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .trim_end_matches(".ttf")
            .trim_end_matches(".TTF");

        // Check if we already loaded a font with this name (avoid duplicates from overlapping directories)
        if self.fonts.iter().any(|f| f.name == name) {
            crate::kinfo!("Font manager: skipping '{}' (already loaded)", name);
            return Ok(());
        }

        crate::kinfo!("Font manager: loading {}", path);

        // Read font file
        let data = crate::fs::read_file_bytes(path)
            .ok_or(FontManagerError::LoadFailed)?;

        // Parse font
        let font = TtfFont::parse(data)
            .map_err(FontManagerError::ParseFailed)?;

        // Determine priority based on name
        let priority = self.calculate_priority(name);

        self.fonts.push(LoadedFont {
            name: String::from(name),
            font,
            priority,
        });

        crate::kinfo!(
            "Font manager: loaded '{}' ({} glyphs, priority {})",
            name,
            self.fonts.last().map(|f| f.font.num_glyphs()).unwrap_or(0),
            priority
        );

        Ok(())
    }

    /// Calculate font priority based on configuration and name
    fn calculate_priority(&self, name: &str) -> u8 {
        // Check if this font is preferred for monospace
        if let Some(prefs) = self.config.get_preferred("monospace") {
            for (i, pref) in prefs.iter().enumerate() {
                if name.contains(pref.as_str()) {
                    return i as u8;
                }
            }
        }

        // Check sans-serif preferences
        if let Some(prefs) = self.config.get_preferred("sans-serif") {
            for (i, pref) in prefs.iter().enumerate() {
                if name.contains(pref.as_str()) {
                    return (i + 10) as u8;
                }
            }
        }

        // CJK fonts get higher priority for Chinese support
        if name.contains("CJK") || name.contains("SC") || name.contains("CN") 
            || name.contains("HarmonyOS") || name.contains("Noto")
        {
            return 20;
        }

        // Default priority
        100
    }

    /// Set the font size
    pub fn set_font_size(&mut self, size: u16) {
        if size != self.font_size {
            self.font_size = size;
            self.cache.clear(); // Clear cache when size changes
        }
    }

    /// Get the current font size
    pub fn font_size(&self) -> u16 {
        self.font_size
    }

    /// Check if a character can be rendered
    pub fn has_glyph(&self, ch: char) -> bool {
        let codepoint = ch as u32;
        self.fonts.iter().any(|f| f.font.get_glyph_id(codepoint).is_some())
    }

    /// Get a rendered glyph bitmap for a character
    pub fn get_glyph(&self, ch: char, size: u16) -> Option<GlyphBitmap> {
        let codepoint = ch as u32;

        if self.fonts.is_empty() {
            crate::kwarn!("get_glyph: NO FONTS LOADED!");
            return Some(GlyphBitmap::placeholder(size));
        }

        // Find a font that has this glyph
        for loaded in &self.fonts {
            if let Some(glyph_id) = loaded.font.get_glyph_id(codepoint) {
                // Get glyph outline
                let outline = match loaded.font.get_glyph_outline(glyph_id) {
                    Ok(o) => o,
                    Err(_e) => {
                        continue;
                    }
                };

                // Get metrics
                let metrics = loaded.font.get_h_metrics(glyph_id);

                // Rasterize
                let rasterizer = Rasterizer::new(size)
                    .with_scale(loaded.font.units_per_em());

                return Some(rasterizer.rasterize(&loaded.font, &outline, &metrics));
            }
        }

        // Character not found in any font
        Some(GlyphBitmap::placeholder(size))
    }

    /// Get glyph with caching (mutable version)
    pub fn get_glyph_cached(&mut self, ch: char, size: u16) -> Option<GlyphBitmap> {
        let codepoint = ch as u32;

        // Check cache
        if let Some(cached) = self.cache.get(codepoint, size) {
            return Some(cached.clone());
        }

        // Rasterize and cache
        let glyph = self.get_glyph(ch, size)?;
        self.cache.insert(codepoint, size, glyph.clone());
        Some(glyph)
    }

    /// Get advance width for a character
    pub fn get_advance(&self, ch: char, size: u16) -> u16 {
        let codepoint = ch as u32;

        for loaded in &self.fonts {
            if let Some(glyph_id) = loaded.font.get_glyph_id(codepoint) {
                let metrics = loaded.font.get_h_metrics(glyph_id);
                let scale = size as f32 / loaded.font.units_per_em() as f32;
                return (metrics.advance_width as f32 * scale) as u16;
            }
        }

        // Default to size/2 for missing characters
        size / 2
    }

    /// Get the number of loaded fonts
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache.len(), MAX_LOADED_FONTS)
    }

    /// Clear the glyph cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get the line height (ascender - descender) for the current font size
    /// Returns the total height in pixels that a line of text should occupy
    pub fn get_line_height(&self, size: u16) -> u16 {
        if let Some(loaded) = self.fonts.first() {
            let scale = size as f32 / loaded.font.units_per_em() as f32;
            let ascender = loaded.font.hhea.ascender as f32 * scale;
            let descender = loaded.font.hhea.descender as f32 * scale; // descender is negative
            let line_gap = loaded.font.hhea.line_gap as f32 * scale;
            // Total line height = ascender - descender + line_gap
            ((ascender - descender + line_gap) as u16).max(size)
        } else {
            size
        }
    }

    /// Get the baseline offset from the top of a cell
    /// This is the ascender height scaled to the given size
    pub fn get_baseline_offset(&self, size: u16) -> u16 {
        if let Some(loaded) = self.fonts.first() {
            let scale = size as f32 / loaded.font.units_per_em() as f32;
            (loaded.font.hhea.ascender as f32 * scale) as u16
        } else {
            size
        }
    }
}
