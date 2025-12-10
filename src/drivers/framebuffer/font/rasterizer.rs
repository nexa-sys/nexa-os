//! TTF glyph rasterizer
//!
//! This module converts TrueType glyph outlines (Bézier curves) to bitmap images.
//! Uses a scanline-based rendering approach optimized for no_std environments.
//!
//! # Algorithm Overview
//!
//! 1. Scale glyph outline from font units to pixel coordinates
//! 2. Convert quadratic Bézier curves to line segments
//! 3. Build edge list from contour segments
//! 4. Scanline fill using non-zero winding rule
//! 5. Apply anti-aliasing through subpixel sampling

use alloc::vec;
use alloc::vec::Vec;

use super::glyph::GlyphBitmap;
use super::ttf::{GlyphOutline, GlyphPoint, HMetric, TtfFont};

// Soft-float math functions for no_std
#[inline]
fn floor_f32(x: f32) -> f32 {
    let i = x as i32;
    if x < i as f32 {
        (i - 1) as f32
    } else {
        i as f32
    }
}

#[inline]
fn ceil_f32(x: f32) -> f32 {
    let i = x as i32;
    if x > i as f32 {
        (i + 1) as f32
    } else {
        i as f32
    }
}

#[inline]
fn round_f32(x: f32) -> f32 {
    floor_f32(x + 0.5)
}

#[inline]
fn sqrt_f32(x: f32) -> f32 {
    // Newton-Raphson method for square root
    if x <= 0.0 {
        return 0.0;
    }
    let mut guess = x;
    for _ in 0..8 {
        guess = 0.5 * (guess + x / guess);
    }
    guess
}

#[inline]
fn abs_f32(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// Edge in the rasterization process
#[derive(Clone, Copy)]
struct Edge {
    /// Y coordinate where edge starts (top, in pixel coords)
    y_start: i32,
    /// Y coordinate where edge ends (bottom)
    y_end: i32,
    /// X coordinate at y_start
    x_start: f32,
    /// X increment per scanline (1/slope)
    dx_per_y: f32,
    /// Direction: +1 for up-going, -1 for down-going
    direction: i32,
}

/// Rasterizer for converting glyph outlines to bitmaps
pub struct Rasterizer {
    /// Target pixel size
    pixel_size: u16,
    /// Subpixel scale factor
    scale: f32,
}

impl Rasterizer {
    /// Create a new rasterizer for a given pixel size
    pub fn new(pixel_size: u16) -> Self {
        Self {
            pixel_size,
            scale: 1.0,
        }
    }

    /// Set the scale factor (derived from font units per em)
    pub fn with_scale(mut self, units_per_em: u16) -> Self {
        self.scale = self.pixel_size as f32 / units_per_em as f32;
        self
    }

    /// Rasterize a glyph outline to a bitmap
    pub fn rasterize(
        &self,
        _font: &TtfFont,
        outline: &GlyphOutline,
        metrics: &HMetric,
    ) -> GlyphBitmap {
        // Handle empty glyphs (like space)
        if outline.contours.is_empty() {
            let advance = ((metrics.advance_width as f32) * self.scale) as u16;
            return GlyphBitmap::empty(advance.max(1));
        }

        // Calculate scaled bounding box
        let x_min = floor_f32((outline.x_min as f32) * self.scale) as i32;
        let y_min = floor_f32((outline.y_min as f32) * self.scale) as i32;
        let x_max = ceil_f32((outline.x_max as f32) * self.scale) as i32;
        let y_max = ceil_f32((outline.y_max as f32) * self.scale) as i32;

        let width = ((x_max - x_min).max(1) + 2) as usize;
        let height = ((y_max - y_min).max(1) + 2) as usize;

        // Build edge list from scaled contours
        let edges = self.build_edges(outline, x_min as f32, y_max as f32);

        // Rasterize using scanline algorithm
        let data = self.scanline_fill(&edges, width, height);

        // Calculate metrics
        let bearing_x = x_min as i16 - 1;
        let bearing_y = y_max as i16 + 1;
        let advance = round_f32((metrics.advance_width as f32) * self.scale) as u16;

        GlyphBitmap {
            width: width as u16,
            height: height as u16,
            bearing_x,
            bearing_y,
            advance: advance.max(1),
            data,
        }
    }

    /// Build edge list from glyph contours
    fn build_edges(&self, outline: &GlyphOutline, x_offset: f32, y_offset: f32) -> Vec<Edge> {
        let mut edges = Vec::new();

        for contour in &outline.contours {
            if contour.is_empty() {
                continue;
            }

            // First, expand the contour to include implicit on-curve points
            let expanded = self.expand_contour(contour);
            if expanded.len() < 2 {
                continue;
            }

            // Convert to scaled coordinates
            let scaled: Vec<(f32, f32)> = expanded
                .iter()
                .map(|p| {
                    let x = (p.x as f32) * self.scale - x_offset;
                    let y = y_offset - (p.y as f32) * self.scale; // Flip Y
                    (x, y)
                })
                .collect();

            // Generate edges from the contour
            // After expand_contour, the pattern is: on, [off, on], on, [off, on], ...
            // Walk through and connect each segment
            let n = expanded.len();
            let mut i = 0;

            // Find first on-curve point
            while i < n && !expanded[i].on_curve {
                i += 1;
            }
            if i >= n {
                continue;
            }

            let first_on = i;
            let mut current = i;

            loop {
                let next = (current + 1) % n;

                if !expanded[next].on_curve {
                    // Quadratic Bézier: current (on) -> next (off) -> next+1 (on)
                    let end = (next + 1) % n;
                    self.add_bezier_edges(&mut edges, scaled[current], scaled[next], scaled[end]);
                    current = end;
                } else {
                    // Straight line: current (on) -> next (on)
                    self.add_line_edge(&mut edges, scaled[current], scaled[next]);
                    current = next;
                }

                if current == first_on {
                    break; // Completed the contour loop
                }
            }
        }

        edges
    }

    /// Expand a contour to include implicit on-curve points between off-curve points
    fn expand_contour(&self, contour: &[GlyphPoint]) -> Vec<GlyphPoint> {
        if contour.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(contour.len() * 2);

        for i in 0..contour.len() {
            let curr = contour[i];
            let next = contour[(i + 1) % contour.len()];

            result.push(curr);

            // If both current and next are off-curve, add implicit on-curve point
            if !curr.on_curve && !next.on_curve {
                result.push(GlyphPoint {
                    x: (curr.x + next.x) / 2,
                    y: (curr.y + next.y) / 2,
                    on_curve: true,
                });
            }
        }

        result
    }

    /// Add edges for a quadratic Bézier curve
    fn add_bezier_edges(
        &self,
        edges: &mut Vec<Edge>,
        p0: (f32, f32),
        ctrl: (f32, f32),
        p2: (f32, f32),
    ) {
        // Flatten curve into line segments
        // Use adaptive subdivision based on curve flatness
        const MAX_DEPTH: usize = 5;

        self.subdivide_bezier(edges, p0, ctrl, p2, 0, MAX_DEPTH);
    }

    /// Recursively subdivide a Bézier curve
    fn subdivide_bezier(
        &self,
        edges: &mut Vec<Edge>,
        p0: (f32, f32),
        ctrl: (f32, f32),
        p2: (f32, f32),
        depth: usize,
        max_depth: usize,
    ) {
        // Check if curve is flat enough
        let flatness = self.curve_flatness(p0, ctrl, p2);

        if flatness < 0.25 || depth >= max_depth {
            // Flat enough, add as line
            self.add_line_edge(edges, p0, p2);
        } else {
            // Subdivide using de Casteljau
            let mid01 = ((p0.0 + ctrl.0) / 2.0, (p0.1 + ctrl.1) / 2.0);
            let mid12 = ((ctrl.0 + p2.0) / 2.0, (ctrl.1 + p2.1) / 2.0);
            let mid = ((mid01.0 + mid12.0) / 2.0, (mid01.1 + mid12.1) / 2.0);

            self.subdivide_bezier(edges, p0, mid01, mid, depth + 1, max_depth);
            self.subdivide_bezier(edges, mid, mid12, p2, depth + 1, max_depth);
        }
    }

    /// Calculate flatness of a quadratic Bézier curve
    fn curve_flatness(&self, p0: (f32, f32), ctrl: (f32, f32), p2: (f32, f32)) -> f32 {
        // Distance from control point to line p0-p2
        let dx = p2.0 - p0.0;
        let dy = p2.1 - p0.1;
        let len_sq = dx * dx + dy * dy;

        if len_sq < 0.0001 {
            // Degenerate case
            let cdx = ctrl.0 - p0.0;
            let cdy = ctrl.1 - p0.1;
            return sqrt_f32(cdx * cdx + cdy * cdy);
        }

        let t = ((ctrl.0 - p0.0) * dx + (ctrl.1 - p0.1) * dy) / len_sq;
        let proj_x = p0.0 + t * dx;
        let proj_y = p0.1 + t * dy;

        let dist_x = ctrl.0 - proj_x;
        let dist_y = ctrl.1 - proj_y;

        sqrt_f32(dist_x * dist_x + dist_y * dist_y)
    }

    /// Add a single line edge
    fn add_line_edge(&self, edges: &mut Vec<Edge>, p0: (f32, f32), p1: (f32, f32)) {
        // Skip horizontal edges
        if abs_f32(p0.1 - p1.1) < 0.001 {
            return;
        }

        // Winding rule in SCREEN coordinates (Y increases downward):
        // - Edge going UP (y decreasing): direction = +1 (entering shape from left)
        // - Edge going DOWN (y increasing): direction = -1 (exiting shape from left)
        // This works because TTF outer contours are clockwise in font coords (Y-up),
        // which become counter-clockwise in screen coords (Y-down).
        let (y_start, y_end, x_start, direction) = if p0.1 < p1.1 {
            // Going DOWN in screen coords (Y increasing)
            (p0.1, p1.1, p0.0, -1)
        } else {
            // Going UP in screen coords (Y decreasing)
            (p1.1, p0.1, p1.0, 1)
        };

        let dx_per_y = (p1.0 - p0.0) / (p1.1 - p0.1);

        // Use floor for y_start, but keep y_end as the actual value
        // to prevent overlapping edges from adjacent curve segments
        edges.push(Edge {
            y_start: floor_f32(y_start) as i32,
            y_end: floor_f32(y_end) as i32, // Use floor, not ceil - prevents overlap
            x_start,
            dx_per_y,
            direction,
        });
    }

    /// Fill using scanline algorithm with anti-aliasing
    fn scanline_fill(&self, edges: &[Edge], width: usize, height: usize) -> Vec<u8> {
        let mut data = vec![0u8; width * height];

        if edges.is_empty() {
            return data;
        }

        // Sort edges by y_start
        let mut sorted_edges: Vec<&Edge> = edges.iter().collect();
        sorted_edges.sort_by(|a, b| a.y_start.cmp(&b.y_start));

        // Active edge list
        let mut active: Vec<ActiveEdge> = Vec::new();

        // Process each scanline with subpixel sampling
        for y in 0..height as i32 {
            // Add edges that start at this scanline
            for edge in &sorted_edges {
                if edge.y_start == y {
                    active.push(ActiveEdge {
                        x: edge.x_start + edge.dx_per_y * (y as f32 - edge.y_start as f32),
                        dx: edge.dx_per_y,
                        y_end: edge.y_end,
                        direction: edge.direction,
                    });
                } else if edge.y_start > y {
                    break;
                }
            }

            // Remove edges that end at this scanline
            active.retain(|e| e.y_end > y);

            // Sort active edges by x
            active.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(core::cmp::Ordering::Equal));

            // Fill pixels using winding rule
            let mut winding = 0i32;
            let mut x_start: Option<f32> = None;

            for ae in &active {
                if winding == 0 {
                    x_start = Some(ae.x);
                }

                winding += ae.direction;

                if winding == 0 {
                    if let Some(xs) = x_start {
                        // Fill from xs to ae.x
                        let x0 = (max_f32(xs, 0.0) as usize).min(width - 1);
                        let x1 = (ceil_f32(max_f32(ae.x, 0.0)) as usize).min(width);

                        for x in x0..x1 {
                            // Calculate coverage
                            let left = x as f32;
                            let right = (x + 1) as f32;

                            let cover_start = max_f32(xs, left);
                            let cover_end = min_f32(ae.x, right);

                            if cover_end > cover_start {
                                let coverage = ((cover_end - cover_start) * 255.0) as u8;
                                let idx = y as usize * width + x;
                                data[idx] = data[idx].saturating_add(coverage);
                            }
                        }
                    }
                    x_start = None;
                }
            }

            // Update x coordinates for next scanline
            for ae in &mut active {
                ae.x += ae.dx;
            }
        }

        data
    }
}

/// Active edge during scanline processing
struct ActiveEdge {
    x: f32,
    dx: f32,
    y_end: i32,
    direction: i32,
}

// Helper for f32 max
#[inline]
fn max_f32(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

// Helper for f32 min
#[inline]
fn min_f32(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}
