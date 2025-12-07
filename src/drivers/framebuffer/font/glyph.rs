//! Glyph bitmap representation and caching
//!
//! This module provides structures for storing rendered glyph bitmaps
//! and a cache for avoiding redundant rasterization.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

/// Maximum number of cached glyphs per font size
const MAX_CACHED_GLYPHS: usize = 512;

/// Rendered glyph bitmap
#[derive(Clone)]
pub struct GlyphBitmap {
    /// Width in pixels
    pub width: u16,
    /// Height in pixels
    pub height: u16,
    /// Left bearing (offset from cursor)
    pub bearing_x: i16,
    /// Top bearing (offset from baseline)
    pub bearing_y: i16,
    /// Advance width for cursor positioning
    pub advance: u16,
    /// Grayscale bitmap data (0-255, row-major)
    pub data: Vec<u8>,
}

impl GlyphBitmap {
    /// Create an empty glyph bitmap (for space characters etc.)
    pub fn empty(advance: u16) -> Self {
        Self {
            width: 0,
            height: 0,
            bearing_x: 0,
            bearing_y: 0,
            advance,
            data: Vec::new(),
        }
    }

    /// Create a placeholder glyph (for missing characters)
    pub fn placeholder(size: u16) -> Self {
        // Create a simple box placeholder
        let width = (size / 2).max(8) as u16;
        let height = size;
        let mut data = vec![0u8; (width as usize) * (height as usize)];

        // Draw box outline
        for x in 0..width as usize {
            data[x] = 255; // Top
            data[(height as usize - 1) * width as usize + x] = 255; // Bottom
        }
        for y in 0..height as usize {
            data[y * width as usize] = 255; // Left
            data[y * width as usize + width as usize - 1] = 255; // Right
        }

        // Draw X in the middle
        let mid_start = height as usize / 4;
        let mid_end = height as usize * 3 / 4;
        for i in 0..(mid_end - mid_start) {
            let y = mid_start + i;
            let x1 = (i * (width as usize - 4)) / (mid_end - mid_start) + 2;
            let x2 = width as usize - 3 - x1 + 2;
            if y < height as usize && x1 < width as usize {
                data[y * width as usize + x1] = 200;
            }
            if y < height as usize && x2 < width as usize {
                data[y * width as usize + x2] = 200;
            }
        }

        Self {
            width,
            height,
            bearing_x: 0,
            bearing_y: height as i16,
            advance: width,
            data,
        }
    }

    /// Get pixel value at (x, y)
    #[inline]
    pub fn get_pixel(&self, x: u16, y: u16) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }
}

/// Cache key for glyph lookup
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct GlyphKey {
    codepoint: u32,
    size: u16,
}

/// Glyph cache with LRU eviction
pub struct GlyphCache {
    cache: BTreeMap<GlyphKey, GlyphBitmap>,
    // Simple usage counter for pseudo-LRU
    access_order: Vec<GlyphKey>,
}

impl GlyphCache {
    /// Create a new glyph cache
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
            access_order: Vec::new(),
        }
    }

    /// Get a cached glyph
    pub fn get(&mut self, codepoint: u32, size: u16) -> Option<&GlyphBitmap> {
        let key = GlyphKey { codepoint, size };
        
        if self.cache.contains_key(&key) {
            // Move to end of access order (most recently used)
            self.access_order.retain(|k| k != &key);
            self.access_order.push(key);
            self.cache.get(&key)
        } else {
            None
        }
    }

    /// Insert a glyph into the cache
    pub fn insert(&mut self, codepoint: u32, size: u16, glyph: GlyphBitmap) {
        let key = GlyphKey { codepoint, size };

        // Evict if necessary
        while self.cache.len() >= MAX_CACHED_GLYPHS && !self.access_order.is_empty() {
            let oldest = self.access_order.remove(0);
            self.cache.remove(&oldest);
        }

        self.cache.insert(key, glyph);
        self.access_order.push(key);
    }

    /// Check if a glyph is cached
    pub fn contains(&self, codepoint: u32, size: u16) -> bool {
        let key = GlyphKey { codepoint, size };
        self.cache.contains_key(&key)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }

    /// Get cache size
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}
