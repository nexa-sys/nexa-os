//! VGA/Framebuffer Device Emulation
//!
//! Provides VGA text mode and framebuffer graphics for VM console display.
//! Supports:
//! - VGA text mode (80x25, 16 colors)
//! - Linear framebuffer (up to 1920x1080, 32bpp)
//! - VGA register emulation (3C0-3CF, 3D4-3D5)

use std::any::Any;
use super::{Device, DeviceId, IoAccess};
use crate::memory::PhysAddr;
use std::sync::{Arc, Mutex};

/// VGA text mode dimensions
pub const TEXT_COLS: usize = 80;
pub const TEXT_ROWS: usize = 25;
pub const TEXT_BUFFER_SIZE: usize = TEXT_COLS * TEXT_ROWS * 2; // char + attr

/// Default framebuffer dimensions
pub const FB_WIDTH: usize = 800;
pub const FB_HEIGHT: usize = 600;
pub const FB_BPP: usize = 24;
pub const FB_STRIDE: usize = FB_WIDTH * (FB_BPP / 8);
pub const FB_SIZE: usize = FB_STRIDE * FB_HEIGHT;

/// VGA MMIO base address (standard VGA memory at 0xA0000)
pub const VGA_MMIO_BASE: PhysAddr = 0xA0000;
pub const VGA_MMIO_SIZE: usize = 0x20000; // 128KB

/// Linear framebuffer base (placed at high memory)
pub const LFB_BASE: PhysAddr = 0xFD000000;

/// VGA display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VgaMode {
    /// Text mode 80x25
    Text,
    /// Graphics mode with linear framebuffer
    Graphics,
}

/// VGA device emulation
pub struct Vga {
    /// Device ID
    id: DeviceId,
    
    /// Current display mode
    mode: VgaMode,
    
    /// Text mode buffer (80x25 characters with attributes)
    text_buffer: Vec<u8>,
    
    /// Linear framebuffer (RGB format, 24bpp)
    framebuffer: Vec<u8>,
    
    /// Framebuffer dimensions
    fb_width: usize,
    fb_height: usize,
    fb_bpp: usize,
    
    /// VGA registers
    /// Miscellaneous Output Register
    misc_output: u8,
    /// Sequencer registers (index 0-4)
    seq_index: u8,
    seq_regs: [u8; 5],
    /// CRT Controller registers (index 0-24)
    crtc_index: u8,
    crtc_regs: [u8; 25],
    /// Graphics Controller registers (index 0-8)
    gc_index: u8,
    gc_regs: [u8; 9],
    /// Attribute Controller registers (index 0-20)
    attr_index: u8,
    attr_regs: [u8; 21],
    attr_flip_flop: bool,
    /// DAC (palette) registers
    dac_read_index: u8,
    dac_write_index: u8,
    dac_state: u8,
    dac_palette: [[u8; 3]; 256], // RGB for each of 256 colors
    
    /// Cursor position
    cursor_x: u8,
    cursor_y: u8,
    cursor_visible: bool,
    
    /// Dirty flag (framebuffer changed)
    dirty: bool,
    
    /// Shared framebuffer for console access
    shared_fb: Arc<Mutex<Vec<u8>>>,
}

impl Vga {
    /// Create a new VGA device
    pub fn new() -> Self {
        let fb_size = FB_WIDTH * FB_HEIGHT * (FB_BPP / 8);
        let shared_fb = Arc::new(Mutex::new(vec![0u8; fb_size]));
        
        let mut vga = Self {
            id: DeviceId::FRAMEBUFFER,
            mode: VgaMode::Text,
            text_buffer: vec![0u8; TEXT_BUFFER_SIZE],
            framebuffer: vec![0u8; fb_size],
            fb_width: FB_WIDTH,
            fb_height: FB_HEIGHT,
            fb_bpp: FB_BPP,
            misc_output: 0,
            seq_index: 0,
            seq_regs: [0; 5],
            crtc_index: 0,
            crtc_regs: [0; 25],
            gc_index: 0,
            gc_regs: [0; 9],
            attr_index: 0,
            attr_regs: [0; 21],
            attr_flip_flop: false,
            dac_read_index: 0,
            dac_write_index: 0,
            dac_state: 0,
            dac_palette: [[0; 3]; 256],
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            dirty: true,
            shared_fb,
        };
        
        // Initialize default VGA palette (16 standard colors)
        vga.init_default_palette();
        
        // Clear text buffer with spaces
        for i in 0..TEXT_ROWS * TEXT_COLS {
            vga.text_buffer[i * 2] = b' ';     // space character
            vga.text_buffer[i * 2 + 1] = 0x07; // light gray on black
        }
        
        vga
    }
    
    /// Initialize default 16-color VGA palette
    fn init_default_palette(&mut self) {
        // Standard VGA 16-color palette
        let colors: [[u8; 3]; 16] = [
            [0x00, 0x00, 0x00], // 0: Black
            [0x00, 0x00, 0xAA], // 1: Blue
            [0x00, 0xAA, 0x00], // 2: Green
            [0x00, 0xAA, 0xAA], // 3: Cyan
            [0xAA, 0x00, 0x00], // 4: Red
            [0xAA, 0x00, 0xAA], // 5: Magenta
            [0xAA, 0x55, 0x00], // 6: Brown
            [0xAA, 0xAA, 0xAA], // 7: Light Gray
            [0x55, 0x55, 0x55], // 8: Dark Gray
            [0x55, 0x55, 0xFF], // 9: Light Blue
            [0x55, 0xFF, 0x55], // 10: Light Green
            [0x55, 0xFF, 0xFF], // 11: Light Cyan
            [0xFF, 0x55, 0x55], // 12: Light Red
            [0xFF, 0x55, 0xFF], // 13: Light Magenta
            [0xFF, 0xFF, 0x55], // 14: Yellow
            [0xFF, 0xFF, 0xFF], // 15: White
        ];
        
        for (i, color) in colors.iter().enumerate() {
            self.dac_palette[i] = *color;
        }
    }
    
    /// Get shared framebuffer for console display
    pub fn get_framebuffer(&self) -> Arc<Mutex<Vec<u8>>> {
        Arc::clone(&self.shared_fb)
    }
    
    /// Get framebuffer width
    pub fn width(&self) -> usize {
        self.fb_width
    }
    
    /// Get framebuffer height
    pub fn height(&self) -> usize {
        self.fb_height
    }
    
    /// Get framebuffer dimensions
    pub fn get_dimensions(&self) -> (usize, usize, usize) {
        (self.fb_width, self.fb_height, self.fb_bpp)
    }
    
    /// Check and clear dirty flag
    pub fn is_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }
    
    /// Render text mode to framebuffer (for console display)
    pub fn render_text_to_framebuffer(&mut self) {
        if self.mode != VgaMode::Text {
            return;
        }
        
        // 8x16 font rendering using real VGA font
        const CHAR_WIDTH: usize = 8;
        const CHAR_HEIGHT: usize = 16;
        
        // Get font data
        let font_data = crate::firmware::font::get_font_data();
        
        let mut fb = self.shared_fb.lock().unwrap();
        
        // Clear framebuffer with black
        fb.fill(0);
        
        for row in 0..TEXT_ROWS {
            for col in 0..TEXT_COLS {
                let idx = (row * TEXT_COLS + col) * 2;
                let ch = self.text_buffer[idx];
                let attr = self.text_buffer[idx + 1];
                
                let fg_idx = (attr & 0x0F) as usize;
                let bg_idx = ((attr >> 4) & 0x0F) as usize;
                
                let fg = self.dac_palette[fg_idx];
                let bg = self.dac_palette[bg_idx];
                
                // Get glyph data for this character
                let glyph_start = (ch as usize) * CHAR_HEIGHT;
                
                for cy in 0..CHAR_HEIGHT {
                    let glyph_row = if glyph_start + cy < font_data.len() {
                        font_data[glyph_start + cy]
                    } else {
                        0
                    };
                    
                    for cx in 0..CHAR_WIDTH {
                        let px = col * CHAR_WIDTH + cx;
                        let py = row * CHAR_HEIGHT + cy;
                        
                        if px >= self.fb_width || py >= self.fb_height {
                            continue;
                        }
                        
                        let fb_idx = (py * self.fb_width + px) * 3;
                        
                        // Check if pixel is set in font glyph (MSB first)
                        let pixel_set = (glyph_row >> (7 - cx)) & 1 != 0;
                        let color = if pixel_set { fg } else { bg };
                        
                        // RGB format (standard VGA)
                        fb[fb_idx] = color[0];     // R
                        fb[fb_idx + 1] = color[1]; // G
                        fb[fb_idx + 2] = color[2]; // B
                    }
                }
            }
        }
        
        // Render cursor if visible
        if self.cursor_visible {
            let cx = self.cursor_x as usize;
            let cy = self.cursor_y as usize;
            
            if cx < TEXT_COLS && cy < TEXT_ROWS {
                // Draw cursor as a block at bottom of character cell
                for px in (cx * CHAR_WIDTH)..((cx + 1) * CHAR_WIDTH) {
                    for py in (cy * CHAR_HEIGHT + 14)..((cy + 1) * CHAR_HEIGHT) {
                        if px < self.fb_width && py < self.fb_height {
                            let fb_idx = (py * self.fb_width + px) * 3;
                            fb[fb_idx] = 0xFF;     // R
                            fb[fb_idx + 1] = 0xFF; // G
                            fb[fb_idx + 2] = 0xFF; // B
                        }
                    }
                }
            }
        }
    }
    
    /// Write a character to text buffer (for serial console redirection)
    pub fn write_char(&mut self, ch: u8) {
        match ch {
            b'\n' => {
                self.cursor_x = 0;
                self.cursor_y += 1;
            }
            b'\r' => {
                self.cursor_x = 0;
            }
            b'\x08' => { // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            _ => {
                if self.cursor_x < TEXT_COLS as u8 && self.cursor_y < TEXT_ROWS as u8 {
                    let idx = (self.cursor_y as usize * TEXT_COLS + self.cursor_x as usize) * 2;
                    self.text_buffer[idx] = ch;
                    // Keep existing attribute
                    self.cursor_x += 1;
                }
            }
        }
        
        // Handle line wrap
        if self.cursor_x >= TEXT_COLS as u8 {
            self.cursor_x = 0;
            self.cursor_y += 1;
        }
        
        // Handle scroll
        if self.cursor_y >= TEXT_ROWS as u8 {
            self.scroll_up();
            self.cursor_y = (TEXT_ROWS - 1) as u8;
        }
        
        self.dirty = true;
    }
    
    /// Write a string to text buffer
    pub fn write_string(&mut self, s: &str) {
        for ch in s.bytes() {
            self.write_char(ch);
        }
        // Render to framebuffer after writing
        self.render_text_to_framebuffer();
    }
    
    /// Write a string with specified color attribute
    /// attr format: 0xBF where B=background (0-F), F=foreground (0-F)
    /// Common values:
    ///   0x07 - Light gray on black (default)
    ///   0x0F - Bright white on black
    ///   0x1F - Bright white on blue (BIOS header style)
    ///   0x4F - Bright white on red (error)
    ///   0x0E - Yellow on black (warning/highlight)
    ///   0x0A - Light green on black (success)
    ///   0x0C - Light red on black (error text)
    pub fn write_string_colored(&mut self, s: &str, attr: u8) {
        for ch in s.bytes() {
            match ch {
                b'\n' => {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                }
                b'\r' => {
                    self.cursor_x = 0;
                }
                b'\x08' => {
                    if self.cursor_x > 0 {
                        self.cursor_x -= 1;
                    }
                }
                _ => {
                    if self.cursor_x < TEXT_COLS as u8 && self.cursor_y < TEXT_ROWS as u8 {
                        let idx = (self.cursor_y as usize * TEXT_COLS + self.cursor_x as usize) * 2;
                        self.text_buffer[idx] = ch;
                        self.text_buffer[idx + 1] = attr;
                        self.cursor_x += 1;
                    }
                }
            }
            
            // Handle line wrap
            if self.cursor_x >= TEXT_COLS as u8 {
                self.cursor_x = 0;
                self.cursor_y += 1;
            }
            
            // Handle scroll
            if self.cursor_y >= TEXT_ROWS as u8 {
                self.scroll_up();
                self.cursor_y = (TEXT_ROWS - 1) as u8;
            }
        }
        
        self.dirty = true;
        // Render to framebuffer after writing
        self.render_text_to_framebuffer();
    }
    
    /// Set text attribute for subsequent write_char calls at current position
    pub fn set_attribute(&mut self, attr: u8) {
        if self.cursor_x < TEXT_COLS as u8 && self.cursor_y < TEXT_ROWS as u8 {
            let idx = (self.cursor_y as usize * TEXT_COLS + self.cursor_x as usize) * 2;
            self.text_buffer[idx + 1] = attr;
        }
    }
    
    /// Fill a line with a character and attribute (for drawing borders)
    pub fn fill_line(&mut self, row: u8, ch: u8, attr: u8) {
        if row < TEXT_ROWS as u8 {
            for col in 0..TEXT_COLS {
                let idx = (row as usize * TEXT_COLS + col) * 2;
                self.text_buffer[idx] = ch;
                self.text_buffer[idx + 1] = attr;
            }
            self.dirty = true;
        }
    }
    
    /// Draw a box with double-line borders (enterprise UI style)
    pub fn draw_box(&mut self, top: u8, left: u8, bottom: u8, right: u8, attr: u8) {
        // Box drawing characters
        const TOP_LEFT: u8 = 0xC9;      // ╔
        const TOP_RIGHT: u8 = 0xBB;     // ╗
        const BOTTOM_LEFT: u8 = 0xC8;   // ╚
        const BOTTOM_RIGHT: u8 = 0xBC;  // ╝
        const HORIZONTAL: u8 = 0xCD;    // ═
        const VERTICAL: u8 = 0xBA;      // ║
        
        // Top border
        self.set_char_at(top, left, TOP_LEFT, attr);
        for col in (left + 1)..right {
            self.set_char_at(top, col, HORIZONTAL, attr);
        }
        self.set_char_at(top, right, TOP_RIGHT, attr);
        
        // Side borders
        for row in (top + 1)..bottom {
            self.set_char_at(row, left, VERTICAL, attr);
            self.set_char_at(row, right, VERTICAL, attr);
        }
        
        // Bottom border
        self.set_char_at(bottom, left, BOTTOM_LEFT, attr);
        for col in (left + 1)..right {
            self.set_char_at(bottom, col, HORIZONTAL, attr);
        }
        self.set_char_at(bottom, right, BOTTOM_RIGHT, attr);
        
        self.dirty = true;
    }
    
    /// Set a character at a specific position
    pub fn set_char_at(&mut self, row: u8, col: u8, ch: u8, attr: u8) {
        if row < TEXT_ROWS as u8 && col < TEXT_COLS as u8 {
            let idx = (row as usize * TEXT_COLS + col as usize) * 2;
            self.text_buffer[idx] = ch;
            self.text_buffer[idx + 1] = attr;
        }
    }
    
    /// Write string at specific position
    pub fn write_string_at(&mut self, row: u8, col: u8, s: &str, attr: u8) {
        let old_x = self.cursor_x;
        let old_y = self.cursor_y;
        
        self.cursor_x = col;
        self.cursor_y = row;
        
        for ch in s.bytes() {
            if ch == b'\n' || ch == b'\r' {
                continue; // Skip newlines in positioned write
            }
            if self.cursor_x < TEXT_COLS as u8 && self.cursor_y < TEXT_ROWS as u8 {
                let idx = (self.cursor_y as usize * TEXT_COLS + self.cursor_x as usize) * 2;
                self.text_buffer[idx] = ch;
                self.text_buffer[idx + 1] = attr;
                self.cursor_x += 1;
            }
        }
        
        self.cursor_x = old_x;
        self.cursor_y = old_y;
        self.dirty = true;
    }
    
    /// Clear a region with specified attribute
    pub fn clear_region(&mut self, top: u8, left: u8, bottom: u8, right: u8, attr: u8) {
        for row in top..=bottom {
            for col in left..=right {
                if row < TEXT_ROWS as u8 && col < TEXT_COLS as u8 {
                    let idx = (row as usize * TEXT_COLS + col as usize) * 2;
                    self.text_buffer[idx] = b' ';
                    self.text_buffer[idx + 1] = attr;
                }
            }
        }
        self.dirty = true;
    }

    /// Scroll text buffer up by one line
    fn scroll_up(&mut self) {
        // Move lines up
        for row in 1..TEXT_ROWS {
            let src_start = row * TEXT_COLS * 2;
            let dst_start = (row - 1) * TEXT_COLS * 2;
            for i in 0..TEXT_COLS * 2 {
                self.text_buffer[dst_start + i] = self.text_buffer[src_start + i];
            }
        }
        
        // Clear last line
        let last_row_start = (TEXT_ROWS - 1) * TEXT_COLS * 2;
        for i in 0..TEXT_COLS {
            self.text_buffer[last_row_start + i * 2] = b' ';
            self.text_buffer[last_row_start + i * 2 + 1] = 0x07;
        }
    }
    
    /// Clear screen
    pub fn clear(&mut self) {
        for i in 0..TEXT_ROWS * TEXT_COLS {
            self.text_buffer[i * 2] = b' ';
            self.text_buffer[i * 2 + 1] = 0x07;
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.dirty = true;
    }
    
    /// Get raw framebuffer data (BGRA format)
    pub fn get_raw_framebuffer(&self) -> Vec<u8> {
        self.shared_fb.lock().unwrap().clone()
    }
    
    /// Get text buffer content as string
    pub fn get_text_content(&self) -> String {
        let mut result = String::new();
        for row in 0..TEXT_ROWS {
            for col in 0..TEXT_COLS {
                let idx = (row * TEXT_COLS + col) * 2;
                let ch = self.text_buffer[idx];
                if ch >= 0x20 && ch < 0x7F {
                    result.push(ch as char);
                } else {
                    result.push(' ');
                }
            }
            result.push('\n');
        }
        result
    }
    
    /// Read from VGA I/O port (public wrapper)
    pub fn read_port(&self, port: u16) -> u8 {
        match port {
            0x3C0 => self.attr_index,
            0x3C1 => {
                if (self.attr_index as usize) < self.attr_regs.len() {
                    self.attr_regs[self.attr_index as usize]
                } else {
                    0
                }
            }
            0x3CC => self.misc_output,
            0x3C4 => self.seq_index,
            0x3C5 => {
                if (self.seq_index as usize) < self.seq_regs.len() {
                    self.seq_regs[self.seq_index as usize]
                } else {
                    0
                }
            }
            0x3CE => self.gc_index,
            0x3CF => {
                if (self.gc_index as usize) < self.gc_regs.len() {
                    self.gc_regs[self.gc_index as usize]
                } else {
                    0
                }
            }
            0x3D4 => self.crtc_index,
            0x3D5 => {
                if (self.crtc_index as usize) < self.crtc_regs.len() {
                    self.crtc_regs[self.crtc_index as usize]
                } else {
                    0
                }
            }
            0x3DA => 0x08, // Input Status 1 (vertical retrace)
            _ => 0xFF,
        }
    }
    
    /// Write to VGA I/O port (public wrapper)
    pub fn write_port(&mut self, port: u16, value: u8) {
        match port {
            0x3C0 => {
                if !self.attr_flip_flop {
                    self.attr_index = value & 0x1F;
                } else if (self.attr_index as usize) < self.attr_regs.len() {
                    self.attr_regs[self.attr_index as usize] = value;
                }
                self.attr_flip_flop = !self.attr_flip_flop;
            }
            0x3C2 => self.misc_output = value,
            0x3C4 => self.seq_index = value,
            0x3C5 => {
                if (self.seq_index as usize) < self.seq_regs.len() {
                    self.seq_regs[self.seq_index as usize] = value;
                }
            }
            0x3CE => self.gc_index = value,
            0x3CF => {
                if (self.gc_index as usize) < self.gc_regs.len() {
                    self.gc_regs[self.gc_index as usize] = value;
                }
            }
            0x3D4 => self.crtc_index = value,
            0x3D5 => {
                if (self.crtc_index as usize) < self.crtc_regs.len() {
                    self.crtc_regs[self.crtc_index as usize] = value;
                    // Handle cursor position updates
                    if self.crtc_index == 0x0E {
                        self.cursor_y = (self.cursor_y & 0x00) | (value >> 0);
                    } else if self.crtc_index == 0x0F {
                        self.cursor_x = value;
                    }
                }
            }
            _ => {}
        }
        self.dirty = true;
    }
    
    /// Read byte from VGA text buffer (VRAM at 0xB8000)
    pub fn read_vram_byte(&self, offset: usize) -> u8 {
        if offset < self.text_buffer.len() {
            self.text_buffer[offset]
        } else {
            0
        }
    }
    
    /// Read word from VGA text buffer
    pub fn read_vram_word(&self, offset: usize) -> u16 {
        if offset + 1 < self.text_buffer.len() {
            (self.text_buffer[offset] as u16) | ((self.text_buffer[offset + 1] as u16) << 8)
        } else {
            0
        }
    }
    
    /// Write byte to VGA text buffer
    pub fn write_vram_byte(&mut self, offset: usize, value: u8) {
        if offset < self.text_buffer.len() {
            self.text_buffer[offset] = value;
            self.dirty = true;
        }
    }
    
    /// Write word to VGA text buffer
    pub fn write_vram_word(&mut self, offset: usize, value: u16) {
        if offset + 1 < self.text_buffer.len() {
            self.text_buffer[offset] = (value & 0xFF) as u8;
            self.text_buffer[offset + 1] = ((value >> 8) & 0xFF) as u8;
            self.dirty = true;
        }
    }
}

impl Default for Vga {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Vga {
    fn id(&self) -> DeviceId {
        self.id
    }
    
    fn name(&self) -> &str {
        "VGA"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    
    fn reset(&mut self) {
        self.mode = VgaMode::Text;
        self.clear();
        self.init_default_palette();
        self.dirty = true;
    }
    
    fn handles_port(&self, port: u16) -> bool {
        matches!(port, 
            0x3C0..=0x3CF | // Attribute Controller, Misc, Sequencer, DAC
            0x3D4..=0x3D5 | // CRT Controller
            0x3DA            // Input Status 1
        )
    }
    
    fn port_read(&mut self, port: u16, _access: IoAccess) -> u32 {
        match port {
            // Attribute Controller
            0x3C0 => self.attr_index as u32,
            0x3C1 => {
                if (self.attr_index as usize) < self.attr_regs.len() {
                    self.attr_regs[self.attr_index as usize] as u32
                } else {
                    0
                }
            }
            
            // Miscellaneous Output (read at 3CC)
            0x3CC => self.misc_output as u32,
            
            // Sequencer
            0x3C4 => self.seq_index as u32,
            0x3C5 => {
                if (self.seq_index as usize) < self.seq_regs.len() {
                    self.seq_regs[self.seq_index as usize] as u32
                } else {
                    0
                }
            }
            
            // DAC
            0x3C7 => self.dac_state as u32,
            0x3C8 => self.dac_write_index as u32,
            0x3C9 => {
                let idx = self.dac_read_index as usize;
                let sub = (self.dac_state % 3) as usize;
                let val = if idx < 256 { self.dac_palette[idx][sub] } else { 0 };
                self.dac_state = (self.dac_state + 1) % 3;
                if self.dac_state == 0 {
                    self.dac_read_index = self.dac_read_index.wrapping_add(1);
                }
                val as u32
            }
            
            // Graphics Controller
            0x3CE => self.gc_index as u32,
            0x3CF => {
                if (self.gc_index as usize) < self.gc_regs.len() {
                    self.gc_regs[self.gc_index as usize] as u32
                } else {
                    0
                }
            }
            
            // CRT Controller
            0x3D4 => self.crtc_index as u32,
            0x3D5 => {
                if (self.crtc_index as usize) < self.crtc_regs.len() {
                    self.crtc_regs[self.crtc_index as usize] as u32
                } else {
                    0
                }
            }
            
            // Input Status 1 (clears attribute flip-flop)
            0x3DA => {
                self.attr_flip_flop = false;
                // Return vertical retrace bit (toggle for timing)
                0x08
            }
            
            _ => 0xFFFFFFFF,
        }
    }
    
    fn port_write(&mut self, port: u16, value: u32, _access: IoAccess) {
        let val = value as u8;
        
        match port {
            // Attribute Controller
            0x3C0 => {
                if !self.attr_flip_flop {
                    self.attr_index = val & 0x1F;
                } else if (self.attr_index as usize) < self.attr_regs.len() {
                    self.attr_regs[self.attr_index as usize] = val;
                }
                self.attr_flip_flop = !self.attr_flip_flop;
            }
            
            // Miscellaneous Output
            0x3C2 => self.misc_output = val,
            
            // Sequencer
            0x3C4 => self.seq_index = val,
            0x3C5 => {
                if (self.seq_index as usize) < self.seq_regs.len() {
                    self.seq_regs[self.seq_index as usize] = val;
                }
            }
            
            // DAC
            0x3C7 => {
                self.dac_read_index = val;
                self.dac_state = 0;
            }
            0x3C8 => {
                self.dac_write_index = val;
                self.dac_state = 0;
            }
            0x3C9 => {
                let idx = self.dac_write_index as usize;
                let sub = (self.dac_state % 3) as usize;
                if idx < 256 {
                    self.dac_palette[idx][sub] = val;
                }
                self.dac_state = (self.dac_state + 1) % 3;
                if self.dac_state == 0 {
                    self.dac_write_index = self.dac_write_index.wrapping_add(1);
                }
                self.dirty = true;
            }
            
            // Graphics Controller
            0x3CE => self.gc_index = val,
            0x3CF => {
                if (self.gc_index as usize) < self.gc_regs.len() {
                    self.gc_regs[self.gc_index as usize] = val;
                }
            }
            
            // CRT Controller
            0x3D4 => self.crtc_index = val,
            0x3D5 => {
                if (self.crtc_index as usize) < self.crtc_regs.len() {
                    self.crtc_regs[self.crtc_index as usize] = val;
                    
                    // Update cursor position from CRTC registers
                    if self.crtc_index == 0x0E || self.crtc_index == 0x0F {
                        let cursor_pos = ((self.crtc_regs[0x0E] as u16) << 8) 
                                       | (self.crtc_regs[0x0F] as u16);
                        self.cursor_x = (cursor_pos % TEXT_COLS as u16) as u8;
                        self.cursor_y = (cursor_pos / TEXT_COLS as u16) as u8;
                    }
                }
                self.dirty = true;
            }
            
            _ => {}
        }
    }
    
    fn handles_mmio(&self, addr: PhysAddr) -> bool {
        // VGA memory (0xA0000 - 0xBFFFF)
        (VGA_MMIO_BASE..VGA_MMIO_BASE + VGA_MMIO_SIZE as u64).contains(&addr)
        // Linear framebuffer
        || (LFB_BASE..LFB_BASE + FB_SIZE as u64).contains(&addr)
    }
    
    fn mmio_region(&self) -> Option<(PhysAddr, usize)> {
        Some((VGA_MMIO_BASE, VGA_MMIO_SIZE))
    }
    
    fn mmio_read(&mut self, addr: PhysAddr, _access: IoAccess) -> u32 {
        if addr >= VGA_MMIO_BASE && addr < VGA_MMIO_BASE + VGA_MMIO_SIZE as u64 {
            // VGA text memory (0xB8000 is standard text mode address)
            let offset = (addr - VGA_MMIO_BASE) as usize;
            if offset >= 0x18000 && offset < 0x18000 + TEXT_BUFFER_SIZE {
                let text_offset = offset - 0x18000;
                if text_offset < self.text_buffer.len() {
                    return self.text_buffer[text_offset] as u32;
                }
            }
        } else if addr >= LFB_BASE && addr < LFB_BASE + FB_SIZE as u64 {
            let offset = (addr - LFB_BASE) as usize;
            if offset + 3 < self.framebuffer.len() {
                return u32::from_le_bytes([
                    self.framebuffer[offset],
                    self.framebuffer[offset + 1],
                    self.framebuffer[offset + 2],
                    self.framebuffer[offset + 3],
                ]);
            }
        }
        0
    }
    
    fn mmio_write(&mut self, addr: PhysAddr, value: u32, _access: IoAccess) {
        if addr >= VGA_MMIO_BASE && addr < VGA_MMIO_BASE + VGA_MMIO_SIZE as u64 {
            let offset = (addr - VGA_MMIO_BASE) as usize;
            // Text mode memory at 0xB8000 (offset 0x18000 from 0xA0000)
            if offset >= 0x18000 && offset < 0x18000 + TEXT_BUFFER_SIZE {
                let text_offset = offset - 0x18000;
                if text_offset < self.text_buffer.len() {
                    self.text_buffer[text_offset] = value as u8;
                    self.dirty = true;
                }
            }
        } else if addr >= LFB_BASE && addr < LFB_BASE + FB_SIZE as u64 {
            let offset = (addr - LFB_BASE) as usize;
            let bytes = value.to_le_bytes();
            for (i, &byte) in bytes.iter().enumerate() {
                if offset + i < self.framebuffer.len() {
                    self.framebuffer[offset + i] = byte;
                }
            }
            self.dirty = true;
        }
    }
}

/// VGA MMIO handler wrapper for AddressSpace integration
/// 
/// This wraps a VGA in RwLock to implement MmioHandler trait
pub struct VgaMmioHandler {
    vga: Arc<std::sync::RwLock<Vga>>,
}

impl VgaMmioHandler {
    pub fn new(vga: Arc<std::sync::RwLock<Vga>>) -> Self {
        Self { vga }
    }
}

impl crate::memory::MmioHandler for VgaMmioHandler {
    fn read(&self, offset: usize, size: u8) -> u64 {
        let mut vga = self.vga.write().unwrap();
        // offset is relative to VGA_MMIO_BASE (0xA0000)
        // Text buffer is at offset 0x18000 (0xB8000 - 0xA0000)
        if offset >= 0x18000 && offset < 0x18000 + TEXT_BUFFER_SIZE {
            let text_offset = offset - 0x18000;
            match size {
                1 => vga.read_vram_byte(text_offset) as u64,
                2 => vga.read_vram_word(text_offset) as u64,
                _ => vga.read_vram_byte(text_offset) as u64,
            }
        } else {
            0
        }
    }
    
    fn write(&self, offset: usize, size: u8, value: u64) {
        // Log first few writes for debugging
        static WRITE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = WRITE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count < 20 {
            log::debug!("[VGA MMIO] Write: offset=0x{:x}, size={}, value=0x{:x}", offset, size, value);
        }
        
        let mut vga = self.vga.write().unwrap();
        // Text buffer is at offset 0x18000 (0xB8000 - 0xA0000)
        if offset >= 0x18000 && offset < 0x18000 + TEXT_BUFFER_SIZE {
            let text_offset = offset - 0x18000;
            match size {
                1 => vga.write_vram_byte(text_offset, value as u8),
                2 => vga.write_vram_word(text_offset, value as u16),
                _ => vga.write_vram_byte(text_offset, value as u8),
            }
            // Render text to framebuffer after write
            vga.render_text_to_framebuffer();
        }
    }
}
