//! TrueType Font (TTF) file parser
//!
//! This module implements a minimal TTF parser capable of:
//! - Reading font tables (cmap, glyf, head, hhea, hmtx, loca, maxp)
//! - Extracting glyph outlines for rendering
//! - Mapping Unicode code points to glyph IDs
//!
//! Reference: https://docs.microsoft.com/en-us/typography/opentype/spec/

use alloc::vec::Vec;

/// TTF parsing errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtfError {
    /// Invalid magic number
    InvalidMagic,
    /// Table not found
    TableNotFound,
    /// Invalid table format
    InvalidFormat,
    /// Invalid glyph index
    InvalidGlyph,
    /// Unsupported cmap format
    UnsupportedCmap,
    /// Buffer too small
    BufferTooSmall,
    /// Invalid offset
    InvalidOffset,
}

/// TTF table directory entry
#[derive(Debug, Clone, Copy)]
struct TableEntry {
    tag: [u8; 4],
    checksum: u32,
    offset: u32,
    length: u32,
}

/// Head table - font header
#[derive(Debug, Clone, Copy)]
pub struct HeadTable {
    pub units_per_em: u16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    pub index_to_loc_format: i16,
}

/// Hhea table - horizontal metrics header
#[derive(Debug, Clone, Copy)]
pub struct HheaTable {
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub advance_width_max: u16,
    pub number_of_h_metrics: u16,
}

/// Maxp table - maximum profile
#[derive(Debug, Clone, Copy)]
pub struct MaxpTable {
    pub num_glyphs: u16,
}

/// Horizontal metric entry
#[derive(Debug, Clone, Copy)]
pub struct HMetric {
    pub advance_width: u16,
    pub left_side_bearing: i16,
}

/// Point in glyph outline
#[derive(Debug, Clone, Copy)]
pub struct GlyphPoint {
    pub x: i16,
    pub y: i16,
    pub on_curve: bool,
}

/// Glyph outline data
#[derive(Debug, Clone)]
pub struct GlyphOutline {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    pub contours: Vec<Vec<GlyphPoint>>,
}

impl GlyphOutline {
    pub fn empty() -> Self {
        Self {
            x_min: 0,
            y_min: 0,
            x_max: 0,
            y_max: 0,
            contours: Vec::new(),
        }
    }
}

/// Parsed TTF font
///
/// Uses borrowed static data from vmalloc to avoid copying large font files.
/// The data must live for 'static lifetime (typically from read_file_bytes).
pub struct TtfFont {
    data: &'static [u8],
    tables: Vec<TableEntry>,
    pub head: HeadTable,
    pub hhea: HheaTable,
    pub maxp: MaxpTable,
    cmap_offset: u32,
    glyf_offset: u32,
    loca_offset: u32,
    hmtx_offset: u32,
}

impl TtfFont {
    /// Parse a TTF font from raw bytes
    ///
    /// The data must have 'static lifetime (e.g., from vmalloc-backed read_file_bytes).
    /// This avoids copying large font files (~8MB) into a new Vec.
    pub fn parse(data: &'static [u8]) -> Result<Self, TtfError> {
        if data.len() < 12 {
            return Err(TtfError::BufferTooSmall);
        }

        // Check magic number (0x00010000 for TTF, 'OTTO' for OTF)
        let magic = read_u32_be(data, 0);
        if magic != 0x00010000 && magic != 0x4F54544F {
            return Err(TtfError::InvalidMagic);
        }

        let num_tables = read_u16_be(data, 4);
        let mut tables = Vec::with_capacity(num_tables as usize);

        // Parse table directory
        let mut offset = 12;
        for _ in 0..num_tables {
            if offset + 16 > data.len() {
                return Err(TtfError::BufferTooSmall);
            }

            let tag = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ];
            let checksum = read_u32_be(data, offset + 4);
            let table_offset = read_u32_be(data, offset + 8);
            let length = read_u32_be(data, offset + 12);

            tables.push(TableEntry {
                tag,
                checksum,
                offset: table_offset,
                length,
            });

            offset += 16;
        }

        // Helper function to find table entry
        fn find_table_entry(tables: &[TableEntry], tag: &[u8; 4]) -> Result<(u32, u32), TtfError> {
            tables
                .iter()
                .find(|t| &t.tag == tag)
                .map(|t| (t.offset, t.length))
                .ok_or(TtfError::TableNotFound)
        }

        let (head_offset, _) = find_table_entry(&tables, b"head")?;
        let (hhea_offset, _) = find_table_entry(&tables, b"hhea")?;
        let (maxp_offset, _) = find_table_entry(&tables, b"maxp")?;
        let (cmap_offset, _) = find_table_entry(&tables, b"cmap")?;
        let (glyf_offset, _) = find_table_entry(&tables, b"glyf")?;
        let (loca_offset, _) = find_table_entry(&tables, b"loca")?;
        let (hmtx_offset, _) = find_table_entry(&tables, b"hmtx")?;

        // Parse head table
        let head = Self::parse_head(data, head_offset as usize)?;

        // Parse hhea table
        let hhea = Self::parse_hhea(data, hhea_offset as usize)?;

        // Parse maxp table
        let maxp = Self::parse_maxp(data, maxp_offset as usize)?;

        Ok(Self {
            data, // Use borrowed reference directly, no copy needed
            tables,
            head,
            hhea,
            maxp,
            cmap_offset,
            glyf_offset,
            loca_offset,
            hmtx_offset,
        })
    }

    fn parse_head(data: &[u8], offset: usize) -> Result<HeadTable, TtfError> {
        if offset + 54 > data.len() {
            return Err(TtfError::BufferTooSmall);
        }

        Ok(HeadTable {
            units_per_em: read_u16_be(data, offset + 18),
            x_min: read_i16_be(data, offset + 36),
            y_min: read_i16_be(data, offset + 38),
            x_max: read_i16_be(data, offset + 40),
            y_max: read_i16_be(data, offset + 42),
            index_to_loc_format: read_i16_be(data, offset + 50),
        })
    }

    fn parse_hhea(data: &[u8], offset: usize) -> Result<HheaTable, TtfError> {
        if offset + 36 > data.len() {
            return Err(TtfError::BufferTooSmall);
        }

        Ok(HheaTable {
            ascender: read_i16_be(data, offset + 4),
            descender: read_i16_be(data, offset + 6),
            line_gap: read_i16_be(data, offset + 8),
            advance_width_max: read_u16_be(data, offset + 10),
            number_of_h_metrics: read_u16_be(data, offset + 34),
        })
    }

    fn parse_maxp(data: &[u8], offset: usize) -> Result<MaxpTable, TtfError> {
        if offset + 6 > data.len() {
            return Err(TtfError::BufferTooSmall);
        }

        Ok(MaxpTable {
            num_glyphs: read_u16_be(data, offset + 4),
        })
    }

    /// Get the glyph ID for a Unicode code point
    pub fn get_glyph_id(&self, codepoint: u32) -> Option<u16> {
        self.lookup_cmap(codepoint)
    }

    /// Look up a character in the cmap table
    fn lookup_cmap(&self, codepoint: u32) -> Option<u16> {
        let offset = self.cmap_offset as usize;
        if offset + 4 > self.data.len() {
            return None;
        }

        let num_subtables = read_u16_be(&self.data, offset + 2);

        // Find the best cmap subtable (prefer format 12 for full Unicode, then format 4)
        let mut format4_offset: Option<usize> = None;
        let mut format12_offset: Option<usize> = None;

        for i in 0..num_subtables {
            let entry_offset = offset + 4 + (i as usize) * 8;
            if entry_offset + 8 > self.data.len() {
                break;
            }

            let platform_id = read_u16_be(&self.data, entry_offset);
            let encoding_id = read_u16_be(&self.data, entry_offset + 2);
            let subtable_offset = offset + read_u32_be(&self.data, entry_offset + 4) as usize;

            if subtable_offset + 2 > self.data.len() {
                continue;
            }

            let format = read_u16_be(&self.data, subtable_offset);

            // Prefer Unicode platform (0, 3) or Windows platform (3, 1)
            if (platform_id == 0 || (platform_id == 3 && encoding_id == 1))
                || (platform_id == 3 && encoding_id == 10)
            {
                match format {
                    4 => format4_offset = Some(subtable_offset),
                    12 => format12_offset = Some(subtable_offset),
                    _ => {}
                }
            }
        }

        // Try format 12 first (full Unicode support), then format 4 (BMP only)
        if let Some(off) = format12_offset {
            if let Some(glyph) = self.lookup_cmap_format12(off, codepoint) {
                return Some(glyph);
            }
        }

        if let Some(off) = format4_offset {
            if codepoint <= 0xFFFF {
                return self.lookup_cmap_format4(off, codepoint as u16);
            }
        }

        None
    }

    /// Look up in cmap format 4 (segment mapping to delta values)
    fn lookup_cmap_format4(&self, offset: usize, codepoint: u16) -> Option<u16> {
        if offset + 14 > self.data.len() {
            return None;
        }

        let seg_count = read_u16_be(&self.data, offset + 6) / 2;
        let end_code_offset = offset + 14;
        let start_code_offset = end_code_offset + (seg_count as usize) * 2 + 2;
        let id_delta_offset = start_code_offset + (seg_count as usize) * 2;
        let id_range_offset_base = id_delta_offset + (seg_count as usize) * 2;

        // Binary search for the segment containing codepoint
        let mut low = 0usize;
        let mut high = seg_count as usize;

        while low < high {
            let mid = (low + high) / 2;
            let end_code = read_u16_be(&self.data, end_code_offset + mid * 2);

            if codepoint > end_code {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        if low >= seg_count as usize {
            return None;
        }

        let seg_idx = low;
        let start_code = read_u16_be(&self.data, start_code_offset + seg_idx * 2);

        if codepoint < start_code {
            return None;
        }

        let id_delta = read_i16_be(&self.data, id_delta_offset + seg_idx * 2);
        let id_range_offset = read_u16_be(&self.data, id_range_offset_base + seg_idx * 2);

        let glyph_id = if id_range_offset == 0 {
            ((codepoint as i32 + id_delta as i32) & 0xFFFF) as u16
        } else {
            let glyph_offset = id_range_offset_base
                + seg_idx * 2
                + id_range_offset as usize
                + ((codepoint - start_code) as usize) * 2;

            if glyph_offset + 2 > self.data.len() {
                return None;
            }

            let glyph = read_u16_be(&self.data, glyph_offset);
            if glyph == 0 {
                0
            } else {
                ((glyph as i32 + id_delta as i32) & 0xFFFF) as u16
            }
        };

        if glyph_id == 0 {
            None
        } else {
            Some(glyph_id)
        }
    }

    /// Look up in cmap format 12 (segmented coverage)
    fn lookup_cmap_format12(&self, offset: usize, codepoint: u32) -> Option<u16> {
        if offset + 16 > self.data.len() {
            return None;
        }

        let num_groups = read_u32_be(&self.data, offset + 12);
        let groups_offset = offset + 16;

        // Binary search for the group containing codepoint
        let mut low = 0u32;
        let mut high = num_groups;

        while low < high {
            let mid = (low + high) / 2;
            let group_offset = groups_offset + (mid as usize) * 12;

            if group_offset + 12 > self.data.len() {
                return None;
            }

            let start_char = read_u32_be(&self.data, group_offset);
            let end_char = read_u32_be(&self.data, group_offset + 4);

            if codepoint < start_char {
                high = mid;
            } else if codepoint > end_char {
                low = mid + 1;
            } else {
                // Found the group
                let start_glyph = read_u32_be(&self.data, group_offset + 8);
                return Some((start_glyph + (codepoint - start_char)) as u16);
            }
        }

        None
    }

    /// Get glyph offset from loca table
    fn get_glyph_offset(&self, glyph_id: u16) -> Option<usize> {
        if glyph_id as u32 >= self.maxp.num_glyphs as u32 {
            return None;
        }

        let loca_offset = self.loca_offset as usize;

        if self.head.index_to_loc_format == 0 {
            // Short format (16-bit offsets, multiply by 2)
            let offset = loca_offset + (glyph_id as usize) * 2;
            if offset + 4 > self.data.len() {
                return None;
            }

            let off1 = read_u16_be(&self.data, offset) as usize * 2;
            let off2 = read_u16_be(&self.data, offset + 2) as usize * 2;

            // Empty glyph (like space)
            if off1 == off2 {
                return None;
            }

            Some(self.glyf_offset as usize + off1)
        } else {
            // Long format (32-bit offsets)
            let offset = loca_offset + (glyph_id as usize) * 4;
            if offset + 8 > self.data.len() {
                return None;
            }

            let off1 = read_u32_be(&self.data, offset) as usize;
            let off2 = read_u32_be(&self.data, offset + 4) as usize;

            if off1 == off2 {
                return None;
            }

            Some(self.glyf_offset as usize + off1)
        }
    }

    /// Get glyph outline data
    pub fn get_glyph_outline(&self, glyph_id: u16) -> Result<GlyphOutline, TtfError> {
        let offset = match self.get_glyph_offset(glyph_id) {
            Some(off) => off,
            None => return Ok(GlyphOutline::empty()), // Empty glyph (like space)
        };

        if offset + 10 > self.data.len() {
            return Err(TtfError::InvalidOffset);
        }

        let num_contours = read_i16_be(&self.data, offset);
        let x_min = read_i16_be(&self.data, offset + 2);
        let y_min = read_i16_be(&self.data, offset + 4);
        let x_max = read_i16_be(&self.data, offset + 6);
        let y_max = read_i16_be(&self.data, offset + 8);

        if num_contours < 0 {
            // Compound glyph - parse components
            return self.parse_compound_glyph(offset + 10, x_min, y_min, x_max, y_max);
        }

        if num_contours == 0 {
            return Ok(GlyphOutline {
                x_min,
                y_min,
                x_max,
                y_max,
                contours: Vec::new(),
            });
        }

        self.parse_simple_glyph(offset + 10, num_contours as u16, x_min, y_min, x_max, y_max)
    }

    /// Parse a simple glyph
    fn parse_simple_glyph(
        &self,
        offset: usize,
        num_contours: u16,
        x_min: i16,
        y_min: i16,
        x_max: i16,
        y_max: i16,
    ) -> Result<GlyphOutline, TtfError> {
        if num_contours == 0 {
            return Ok(GlyphOutline {
                x_min,
                y_min,
                x_max,
                y_max,
                contours: Vec::new(),
            });
        }

        // Read end points of contours
        let mut contour_ends = Vec::with_capacity(num_contours as usize);
        let mut off = offset;

        for _ in 0..num_contours {
            if off + 2 > self.data.len() {
                return Err(TtfError::BufferTooSmall);
            }
            contour_ends.push(read_u16_be(&self.data, off));
            off += 2;
        }

        let num_points = (*contour_ends.last().unwrap_or(&0) + 1) as usize;

        // Skip instruction length and instructions
        if off + 2 > self.data.len() {
            return Err(TtfError::BufferTooSmall);
        }
        let instruction_len = read_u16_be(&self.data, off) as usize;
        off += 2 + instruction_len;

        // Read flags
        let mut flags = Vec::with_capacity(num_points);
        while flags.len() < num_points {
            if off >= self.data.len() {
                return Err(TtfError::BufferTooSmall);
            }

            let flag = self.data[off];
            off += 1;

            flags.push(flag);

            // Check repeat flag
            if (flag & 0x08) != 0 {
                if off >= self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                let repeat = self.data[off] as usize;
                off += 1;

                for _ in 0..repeat {
                    if flags.len() >= num_points {
                        break;
                    }
                    flags.push(flag);
                }
            }
        }

        // Read x coordinates
        let mut x_coords = Vec::with_capacity(num_points);
        let mut x: i16 = 0;

        for flag in &flags {
            let x_short = (flag & 0x02) != 0;
            let x_same_or_positive = (flag & 0x10) != 0;

            if x_short {
                if off >= self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                let dx = self.data[off] as i16;
                off += 1;
                x += if x_same_or_positive { dx } else { -dx };
            } else if !x_same_or_positive {
                if off + 2 > self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                x += read_i16_be(&self.data, off);
                off += 2;
            }
            // else: same as previous x

            x_coords.push(x);
        }

        // Read y coordinates
        let mut y_coords = Vec::with_capacity(num_points);
        let mut y: i16 = 0;

        for flag in &flags {
            let y_short = (flag & 0x04) != 0;
            let y_same_or_positive = (flag & 0x20) != 0;

            if y_short {
                if off >= self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                let dy = self.data[off] as i16;
                off += 1;
                y += if y_same_or_positive { dy } else { -dy };
            } else if !y_same_or_positive {
                if off + 2 > self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                y += read_i16_be(&self.data, off);
                off += 2;
            }

            y_coords.push(y);
        }

        // Build contours
        let mut contours = Vec::with_capacity(num_contours as usize);
        let mut start_idx = 0usize;

        for &end_idx in &contour_ends {
            let end = end_idx as usize;
            let mut contour = Vec::with_capacity(end - start_idx + 1);

            for i in start_idx..=end {
                if i < flags.len() {
                    contour.push(GlyphPoint {
                        x: x_coords[i],
                        y: y_coords[i],
                        on_curve: (flags[i] & 0x01) != 0,
                    });
                }
            }

            contours.push(contour);
            start_idx = end + 1;
        }

        Ok(GlyphOutline {
            x_min,
            y_min,
            x_max,
            y_max,
            contours,
        })
    }

    /// Parse a compound glyph (composed of multiple simple glyphs)
    fn parse_compound_glyph(
        &self,
        offset: usize,
        x_min: i16,
        y_min: i16,
        x_max: i16,
        y_max: i16,
    ) -> Result<GlyphOutline, TtfError> {
        let mut combined = GlyphOutline {
            x_min,
            y_min,
            x_max,
            y_max,
            contours: Vec::new(),
        };

        let mut off = offset;
        let mut has_more = true;

        while has_more {
            if off + 4 > self.data.len() {
                return Err(TtfError::BufferTooSmall);
            }

            let flags = read_u16_be(&self.data, off);
            let glyph_index = read_u16_be(&self.data, off + 2);
            off += 4;

            has_more = (flags & 0x0020) != 0; // MORE_COMPONENTS flag

            // Read transformation parameters
            let (dx, dy): (i16, i16);

            if (flags & 0x0001) != 0 {
                // ARG_1_AND_2_ARE_WORDS
                if off + 4 > self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                dx = read_i16_be(&self.data, off);
                dy = read_i16_be(&self.data, off + 2);
                off += 4;
            } else {
                if off + 2 > self.data.len() {
                    return Err(TtfError::BufferTooSmall);
                }
                dx = self.data[off] as i8 as i16;
                dy = self.data[off + 1] as i8 as i16;
                off += 2;
            }

            // Skip scale/transformation (for simplicity, we only handle translation)
            if (flags & 0x0008) != 0 {
                // WE_HAVE_A_SCALE
                off += 2;
            } else if (flags & 0x0040) != 0 {
                // WE_HAVE_AN_X_AND_Y_SCALE
                off += 4;
            } else if (flags & 0x0080) != 0 {
                // WE_HAVE_A_TWO_BY_TWO
                off += 8;
            }

            // Get component glyph and apply transformation
            if let Ok(component) = self.get_glyph_outline(glyph_index) {
                for contour in component.contours {
                    let transformed: Vec<GlyphPoint> = contour
                        .into_iter()
                        .map(|p| GlyphPoint {
                            x: p.x.saturating_add(dx),
                            y: p.y.saturating_add(dy),
                            on_curve: p.on_curve,
                        })
                        .collect();
                    combined.contours.push(transformed);
                }
            }
        }

        Ok(combined)
    }

    /// Get horizontal metrics for a glyph
    pub fn get_h_metrics(&self, glyph_id: u16) -> HMetric {
        let num_h_metrics = self.hhea.number_of_h_metrics;
        let offset = self.hmtx_offset as usize;

        if glyph_id < num_h_metrics {
            let entry_offset = offset + (glyph_id as usize) * 4;
            if entry_offset + 4 <= self.data.len() {
                return HMetric {
                    advance_width: read_u16_be(&self.data, entry_offset),
                    left_side_bearing: read_i16_be(&self.data, entry_offset + 2),
                };
            }
        } else {
            // Use last advance width for glyphs beyond num_h_metrics
            let last_aw_offset = offset + ((num_h_metrics - 1) as usize) * 4;
            let lsb_offset =
                offset + (num_h_metrics as usize) * 4 + ((glyph_id - num_h_metrics) as usize) * 2;

            if last_aw_offset + 2 <= self.data.len() && lsb_offset + 2 <= self.data.len() {
                return HMetric {
                    advance_width: read_u16_be(&self.data, last_aw_offset),
                    left_side_bearing: read_i16_be(&self.data, lsb_offset),
                };
            }
        }

        // Default metrics
        HMetric {
            advance_width: self.head.units_per_em,
            left_side_bearing: 0,
        }
    }

    /// Get the number of glyphs in the font
    pub fn num_glyphs(&self) -> u16 {
        self.maxp.num_glyphs
    }

    /// Get units per em
    pub fn units_per_em(&self) -> u16 {
        self.head.units_per_em
    }
}

// Helper functions for reading big-endian values
#[inline]
fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    ((data[offset] as u16) << 8) | (data[offset + 1] as u16)
}

#[inline]
fn read_i16_be(data: &[u8], offset: usize) -> i16 {
    read_u16_be(data, offset) as i16
}

#[inline]
fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    ((data[offset] as u32) << 24)
        | ((data[offset + 1] as u32) << 16)
        | ((data[offset + 2] as u32) << 8)
        | (data[offset + 3] as u32)
}
