//! Color types and utilities for framebuffer rendering
//!
//! This module contains color-related types, ANSI color palettes,
//! and color packing utilities for different pixel formats.

use multiboot2::FramebufferField;

/// RGB color representation
#[derive(Clone, Copy)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Default foreground color (light gray-blue)
pub const DEFAULT_FG: RgbColor = RgbColor::new(0xE6, 0xEC, 0xF1);

/// Default background color (dark blue-gray)
pub const DEFAULT_BG: RgbColor = RgbColor::new(0x08, 0x0C, 0x12);

/// Standard ANSI base colors (0-7)
pub const ANSI_BASE_COLORS: [RgbColor; 8] = [
    RgbColor::new(0x00, 0x00, 0x00), // Black
    RgbColor::new(0xAA, 0x00, 0x00), // Red
    RgbColor::new(0x00, 0xAA, 0x00), // Green
    RgbColor::new(0xAA, 0x55, 0x00), // Yellow/Brown
    RgbColor::new(0x00, 0x00, 0xAA), // Blue
    RgbColor::new(0xAA, 0x00, 0xAA), // Magenta
    RgbColor::new(0x00, 0xAA, 0xAA), // Cyan
    RgbColor::new(0xAA, 0xAA, 0xAA), // Light gray
];

/// Bright ANSI colors (8-15)
pub const ANSI_BRIGHT_COLORS: [RgbColor; 8] = [
    RgbColor::new(0x55, 0x55, 0x55), // Dark gray
    RgbColor::new(0xFF, 0x55, 0x55), // Bright red
    RgbColor::new(0x55, 0xFF, 0x55), // Bright green
    RgbColor::new(0xFF, 0xFF, 0x55), // Bright yellow
    RgbColor::new(0x55, 0x55, 0xFF), // Bright blue
    RgbColor::new(0xFF, 0x55, 0xFF), // Bright magenta
    RgbColor::new(0x55, 0xFF, 0xFF), // Bright cyan
    RgbColor::new(0xFF, 0xFF, 0xFF), // White
];

/// Packed color for efficient framebuffer writes
///
/// Pre-packs RGB color into the target pixel format to avoid
/// repeated color conversion during rendering.
#[derive(Clone, Copy)]
pub struct PackedColor {
    pub bytes: [u8; 4],
    #[allow(dead_code)]
    pub len: usize,
}

impl PackedColor {
    pub fn new(value: u32, len: usize) -> Self {
        let mut bytes = value.to_le_bytes();
        if len < 4 {
            for byte in bytes[len..].iter_mut() {
                *byte = 0;
            }
        }
        Self { bytes, len }
    }
}

/// Select an ANSI color by index
///
/// # Arguments
/// * `index` - Color index (0-7)
/// * `bright` - Use bright variant if true
pub fn select_color(index: usize, bright: bool) -> RgbColor {
    if bright {
        ANSI_BRIGHT_COLORS.get(index).copied().unwrap_or(DEFAULT_FG)
    } else {
        ANSI_BASE_COLORS.get(index).copied().unwrap_or(DEFAULT_FG)
    }
}

/// Pack RGB color into framebuffer pixel format
///
/// Converts RGB values to the packed format expected by the framebuffer,
/// taking into account the bit position and size of each color channel.
pub fn pack_color(
    red: &FramebufferField,
    green: &FramebufferField,
    blue: &FramebufferField,
    r: u8,
    g: u8,
    b: u8,
) -> u32 {
    fn pack_component(field: &FramebufferField, value: u8) -> u32 {
        if field.size == 0 {
            return 0;
        }
        let max_value = if field.size >= 31 {
            u32::MAX
        } else {
            (1u32 << field.size) - 1
        };
        let scaled = (value as u32 * max_value + 127) / 255;
        scaled << field.position
    }

    pack_component(red, r) | pack_component(green, g) | pack_component(blue, b)
}
