//! VGA/Framebuffer Device Emulation
//!
//! Provides VGA text mode and framebuffer graphics for VM console display.
//! Supports:
//! - VGA text mode (80x25, 16 colors)
//! - Linear framebuffer (up to 1920x1080, 32bpp)
//! - VGA register emulation (3C0-3CF, 3D4-3D5)

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
pub const FB_BPP: usize = 32;
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
    
    /// Linear framebuffer (BGRA format)
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
        
        // Simple 8x16 font rendering
        const CHAR_WIDTH: usize = 8;
        const CHAR_HEIGHT: usize = 16;
        
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
                
                // Render character (simplified - just solid blocks for now)
                let is_printable = ch >= 0x20 && ch < 0x7F;
                
                for cy in 0..CHAR_HEIGHT {
                    for cx in 0..CHAR_WIDTH {
                        let px = col * CHAR_WIDTH + cx;
                        let py = row * CHAR_HEIGHT + cy;
                        
                        if px >= self.fb_width || py >= self.fb_height {
                            continue;
                        }
                        
                        let fb_idx = (py * self.fb_width + px) * 4;
                        
                        // Simple rendering: show character as foreground, space as background
                        let color = if is_printable && ch != b' ' {
                            // Very basic "font" - just show a block for non-space chars
                            // Real implementation would use a bitmap font
                            if cx > 0 && cx < 7 && cy > 1 && cy < 14 {
                                fg
                            } else {
                                bg
                            }
                        } else {
                            bg
                        };
                        
                        // BGRA format
                        fb[fb_idx] = color[2];     // B
                        fb[fb_idx + 1] = color[1]; // G
                        fb[fb_idx + 2] = color[0]; // R
                        fb[fb_idx + 3] = 0xFF;     // A
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
                            let fb_idx = (py * self.fb_width + px) * 4;
                            fb[fb_idx] = 0xFF;     // B
                            fb[fb_idx + 1] = 0xFF; // G
                            fb[fb_idx + 2] = 0xFF; // R
                            fb[fb_idx + 3] = 0xFF; // A
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
