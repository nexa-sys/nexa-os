//! PS/2 Keyboard Controller Emulation
//!
//! Provides enterprise-grade keyboard emulation with:
//! - Standard PS/2 keyboard protocol
//! - Scancode Set 2 support
//! - Special key detection (F2, DEL, Ctrl+Alt+Del)
//! - Boot-time setup key handling
//!
//! ## Port Mapping
//! - 0x60: Data port (read keyboard data, write commands to keyboard)
//! - 0x64: Status/Command port (read status, write controller commands)

use std::any::Any;
use super::{Device, DeviceId, IoAccess};
use std::collections::VecDeque;
use log::{trace, debug, warn};

/// PS/2 keyboard controller status register bits
#[allow(dead_code)]
mod status {
    pub const OUTPUT_FULL: u8 = 0x01;     // Output buffer full (data available)
    pub const INPUT_FULL: u8 = 0x02;      // Input buffer full (controller busy)
    pub const SYSTEM_FLAG: u8 = 0x04;     // System flag (POST completed)
    pub const COMMAND: u8 = 0x08;         // Command/data (last write was to 0x64/0x60)
    pub const KEYBOARD_ENABLED: u8 = 0x10; // Keyboard not inhibited
    pub const AUX_OUTPUT: u8 = 0x20;      // Mouse data in output buffer
    pub const TIMEOUT: u8 = 0x40;         // Timeout error
    pub const PARITY: u8 = 0x80;          // Parity error
}

// PS/2 keyboard only sends scancodes and triggers IRQ1
// BIOS/OS handles key combinations like Ctrl+Alt+Del via INT 09h

/// Key state tracking for modifier keys
#[derive(Debug, Clone, Default)]
struct ModifierState {
    left_ctrl: bool,
    right_ctrl: bool,
    left_alt: bool,
    right_alt: bool,
    left_shift: bool,
    right_shift: bool,
}

impl ModifierState {
    fn ctrl_pressed(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }
    
    fn alt_pressed(&self) -> bool {
        self.left_alt || self.right_alt
    }
}

/// PS/2 Keyboard Controller
pub struct Ps2Keyboard {
    id: DeviceId,
    /// Output buffer (data to be read by guest)
    output_buffer: VecDeque<u8>,
    /// Input buffer (commands from guest)
    input_buffer: VecDeque<u8>,
    /// Status register
    status: u8,
    /// Controller configuration byte
    config: u8,
    /// Last command sent to controller
    last_command: Option<u8>,
    /// Waiting for data after command
    expecting_data: bool,
    /// Modifier key state (for internal tracking only)
    modifiers: ModifierState,
    /// Interrupt pending flag
    interrupt_pending: bool,
    /// CPU reset requested (via 0xFE command or output port bit 0)
    reset_requested: bool,
    /// Keyboard enabled
    enabled: bool,
    /// Current scancode set (1, 2, or 3)
    scancode_set: u8,
}

impl Ps2Keyboard {
    pub fn new() -> Self {
        Self {
            id: DeviceId::KEYBOARD, // Use standard device ID
            output_buffer: VecDeque::new(),
            input_buffer: VecDeque::new(),
            status: status::SYSTEM_FLAG | status::KEYBOARD_ENABLED,
            config: 0x45, // Default: keyboard interrupt enabled, translation enabled
            last_command: None,
            expecting_data: false,
            modifiers: ModifierState::default(),
            interrupt_pending: false,
            reset_requested: false,
            enabled: true,
            scancode_set: 2,
        }
    }
    
    /// Scancode Set 2 to Set 1 translation table
    /// Used when controller translation is enabled (config bit 6)
    /// BIOS expects Set 1 scancodes for interrupt handling
    const SET2_TO_SET1: [u8; 256] = {
        let mut table = [0u8; 256];
        // Common keys - mapping from Set 2 to Set 1
        // Letters
        table[0x1C] = 0x1E; // A
        table[0x32] = 0x30; // B
        table[0x21] = 0x2E; // C
        table[0x23] = 0x20; // D
        table[0x24] = 0x12; // E
        table[0x2B] = 0x21; // F
        table[0x34] = 0x22; // G
        table[0x33] = 0x23; // H
        table[0x43] = 0x17; // I
        table[0x3B] = 0x24; // J
        table[0x42] = 0x25; // K
        table[0x4B] = 0x26; // L
        table[0x3A] = 0x32; // M
        table[0x31] = 0x31; // N
        table[0x44] = 0x18; // O
        table[0x4D] = 0x19; // P
        table[0x15] = 0x10; // Q
        table[0x2D] = 0x13; // R
        table[0x1B] = 0x1F; // S
        table[0x2C] = 0x14; // T
        table[0x3C] = 0x16; // U
        table[0x2A] = 0x2F; // V
        table[0x1D] = 0x11; // W
        table[0x22] = 0x2D; // X
        table[0x35] = 0x15; // Y
        table[0x1A] = 0x2C; // Z
        // Numbers
        table[0x45] = 0x0B; // 0
        table[0x16] = 0x02; // 1
        table[0x1E] = 0x03; // 2
        table[0x26] = 0x04; // 3
        table[0x25] = 0x05; // 4
        table[0x2E] = 0x06; // 5
        table[0x36] = 0x07; // 6
        table[0x3D] = 0x08; // 7
        table[0x3E] = 0x09; // 8
        table[0x46] = 0x0A; // 9
        // Symbols
        table[0x4E] = 0x0C; // -
        table[0x55] = 0x0D; // =
        table[0x54] = 0x1A; // [
        table[0x5B] = 0x1B; // ]
        table[0x5D] = 0x2B; // backslash
        table[0x4C] = 0x27; // ;
        table[0x52] = 0x28; // '
        table[0x0E] = 0x29; // `
        table[0x41] = 0x33; // ,
        table[0x49] = 0x34; // .
        table[0x4A] = 0x35; // /
        // Control keys
        table[0x5A] = 0x1C; // Enter
        table[0x29] = 0x39; // Space
        table[0x0D] = 0x0F; // Tab
        table[0x66] = 0x0E; // Backspace
        table[0x58] = 0x3A; // CapsLock
        table[0x76] = 0x01; // Escape
        // Modifiers
        table[0x12] = 0x2A; // Left Shift
        table[0x59] = 0x36; // Right Shift
        table[0x14] = 0x1D; // Ctrl (both left and right use same in Set 1)
        table[0x11] = 0x38; // Alt
        // Function keys
        table[0x05] = 0x3B; // F1
        table[0x06] = 0x3C; // F2
        table[0x04] = 0x3D; // F3
        table[0x0C] = 0x3E; // F4
        table[0x03] = 0x3F; // F5
        table[0x0B] = 0x40; // F6
        table[0x83] = 0x41; // F7
        table[0x0A] = 0x42; // F8
        table[0x01] = 0x43; // F9
        table[0x09] = 0x44; // F10
        table[0x78] = 0x57; // F11
        table[0x07] = 0x58; // F12
        // Extended keys (E0 prefix in both sets)
        table[0x70] = 0x52; // Insert
        table[0x71] = 0x53; // Delete - CRITICAL for Ctrl+Alt+Del!
        table[0x6C] = 0x47; // Home
        table[0x69] = 0x4F; // End
        table[0x7D] = 0x49; // PageUp
        table[0x7A] = 0x51; // PageDown
        table[0x75] = 0x48; // Up
        table[0x72] = 0x50; // Down
        table[0x6B] = 0x4B; // Left
        table[0x74] = 0x4D; // Right
        // NumLock and ScrollLock
        table[0x77] = 0x45; // NumLock
        table[0x7E] = 0x46; // ScrollLock
        table
    };
    
    /// Translate Set 2 scancode to Set 1 if translation is enabled
    fn translate_scancode(&self, scancode: u8) -> u8 {
        // Check if translation is enabled (config bit 6)
        if (self.config & 0x40) != 0 {
            let translated = Self::SET2_TO_SET1[scancode as usize];
            if translated != 0 {
                return translated;
            }
        }
        scancode
    }
    
    /// Process an incoming scancode (from host keyboard event)
    pub fn inject_scancode(&mut self, scancode: u8, is_release: bool) {
        // Handle modifier keys (for internal state tracking, use original Set 2 codes)
        self.update_modifiers(scancode, is_release);
        
        // Translate to Set 1 if enabled (BIOS expects Set 1)
        let output_code = self.translate_scancode(scancode);
        
        trace!("[PS2] inject_scancode: Set2=0x{:02X} -> Set1=0x{:02X}, release={}", 
               scancode, output_code, is_release);
        
        // Add to output buffer using Set 1 format
        if is_release {
            // Set 1 break code: scancode | 0x80
            self.output_buffer.push_back(output_code | 0x80);
        } else {
            self.output_buffer.push_back(output_code);
        }
        
        // Update status and raise interrupt
        self.status |= status::OUTPUT_FULL;
        self.interrupt_pending = true;
        debug!("[PS2] Scancode in buffer, interrupt_pending=true, buffer_len={}", 
               self.output_buffer.len());
    }
    
    /// Inject a key press event using human-readable key name
    /// 
    /// Supports full PS/2 Scancode Set 2 for enterprise-grade keyboard emulation.
    /// Compatible with ESXi-style console input handling.
    pub fn inject_key(&mut self, key: &str, is_release: bool) {
        log::info!("[PS2] inject_key: key='{}', is_release={}", key, is_release);
        let key_lower = key.to_lowercase();
        let key_str = key_lower.as_str();
        
        // Determine if this is an extended key (requires 0xE0 prefix)
        let is_extended = matches!(key_str, 
            "delete" | "del" | "insert" | "home" | "end" | "pageup" | "pagedown" |
            "up" | "down" | "left" | "right" |
            "rctrl" | "ralt" | "rmeta" |
            "kpenter" | "kpslash" |
            "printscreen" | "pause" | "menu" |
            "lmeta" | "rmeta" | "apps"
        );
        
        // Map key name to Scancode Set 2
        let scancode: Option<u8> = match key_str {
            // ========== Letter keys (A-Z) ==========
            "a" => Some(0x1C),
            "b" => Some(0x32),
            "c" => Some(0x21),
            "d" => Some(0x23),
            "e" => Some(0x24),
            "f" => Some(0x2B),
            "g" => Some(0x34),
            "h" => Some(0x33),
            "i" => Some(0x43),
            "j" => Some(0x3B),
            "k" => Some(0x42),
            "l" => Some(0x4B),
            "m" => Some(0x3A),
            "n" => Some(0x31),
            "o" => Some(0x44),
            "p" => Some(0x4D),
            "q" => Some(0x15),
            "r" => Some(0x2D),
            "s" => Some(0x1B),
            "t" => Some(0x2C),
            "u" => Some(0x3C),
            "v" => Some(0x2A),
            "w" => Some(0x1D),
            "x" => Some(0x22),
            "y" => Some(0x35),
            "z" => Some(0x1A),
            
            // ========== Number row (0-9 and symbols) ==========
            "1" | "!" => Some(0x16),
            "2" | "@" => Some(0x1E),
            "3" | "#" => Some(0x26),
            "4" | "$" => Some(0x25),
            "5" | "%" => Some(0x2E),
            "6" | "^" => Some(0x36),
            "7" | "&" => Some(0x3D),
            "8" | "*" => Some(0x3E),
            "9" | "(" => Some(0x46),
            "0" | ")" => Some(0x45),
            
            // ========== Symbol keys ==========
            "-" | "_" | "minus" => Some(0x4E),
            "=" | "+" | "equal" => Some(0x55),
            "[" | "{" | "bracketleft" => Some(0x54),
            "]" | "}" | "bracketright" => Some(0x5B),
            "\\" | "|" | "backslash" => Some(0x5D),
            ";" | ":" | "semicolon" => Some(0x4C),
            "'" | "\"" | "quote" => Some(0x52),
            "`" | "~" | "backquote" => Some(0x0E),
            "," | "<" | "comma" => Some(0x41),
            "." | ">" | "period" => Some(0x49),
            "/" | "?" | "slash" => Some(0x4A),
            
            // ========== Function keys (F1-F12) ==========
            "f1" => Some(0x05),
            "f2" => Some(0x06),
            "f3" => Some(0x04),
            "f4" => Some(0x0C),
            "f5" => Some(0x03),
            "f6" => Some(0x0B),
            "f7" => Some(0x83),
            "f8" => Some(0x0A),
            "f9" => Some(0x01),
            "f10" => Some(0x09),
            "f11" => Some(0x78),
            "f12" => Some(0x07),
            
            // ========== Modifier keys ==========
            "lshift" | "shiftleft" => Some(0x12),
            "rshift" | "shiftright" => Some(0x59),
            "lctrl" | "controlleft" | "ctrl" | "control" => Some(0x14),
            "rctrl" | "controlright" => Some(0x14), // Extended
            "lalt" | "altleft" | "alt" => Some(0x11),
            "ralt" | "altright" | "altgraph" => Some(0x11), // Extended
            "lmeta" | "metaleft" | "meta" | "win" | "super" => Some(0x1F), // Extended
            "rmeta" | "metaright" => Some(0x27), // Extended
            
            // ========== Control keys ==========
            "enter" | "return" => Some(0x5A),
            "space" | " " => Some(0x29),
            "tab" => Some(0x0D),
            "backspace" => Some(0x66),
            "capslock" => Some(0x58),
            "numlock" => Some(0x77),
            "scrolllock" => Some(0x7E),
            "esc" | "escape" => Some(0x76),
            
            // ========== Navigation keys (Extended) ==========
            "insert" => Some(0x70),
            "delete" | "del" => Some(0x71),
            "home" => Some(0x6C),
            "end" => Some(0x69),
            "pageup" => Some(0x7D),
            "pagedown" => Some(0x7A),
            
            // ========== Arrow keys (Extended) ==========
            "up" | "arrowup" => Some(0x75),
            "down" | "arrowdown" => Some(0x72),
            "left" | "arrowleft" => Some(0x6B),
            "right" | "arrowright" => Some(0x74),
            
            // ========== Numpad keys ==========
            "kp0" | "numpad0" => Some(0x70),
            "kp1" | "numpad1" => Some(0x69),
            "kp2" | "numpad2" => Some(0x72),
            "kp3" | "numpad3" => Some(0x7A),
            "kp4" | "numpad4" => Some(0x6B),
            "kp5" | "numpad5" => Some(0x73),
            "kp6" | "numpad6" => Some(0x74),
            "kp7" | "numpad7" => Some(0x6C),
            "kp8" | "numpad8" => Some(0x75),
            "kp9" | "numpad9" => Some(0x7D),
            "kpdot" | "numpaddecimal" => Some(0x71),
            "kpenter" | "numpadenter" => Some(0x5A), // Extended
            "kpplus" | "numpadadd" => Some(0x79),
            "kpminus" | "numpadsubtract" => Some(0x7B),
            "kpasterisk" | "numpadmultiply" => Some(0x7C),
            "kpslash" | "numpaddivide" => Some(0x4A), // Extended
            
            // ========== Special keys ==========
            "printscreen" => Some(0x7C), // Extended, complex sequence
            "pause" => Some(0x77), // Special handling needed
            "menu" | "contextmenu" | "apps" => Some(0x2F), // Extended
            
            _ => None,
        };
        
        let Some(code) = scancode else {
            // Log unknown key for debugging in enterprise environments
            warn!("[PS2] Unknown key: '{}'", key);
            return;
        };
        
        trace!("[PS2] key='{}' -> scancode=0x{:02X}, extended={}", key, code, is_extended);
        
        // Send extended prefix if needed
        if is_extended {
            self.output_buffer.push_back(0xE0);
        }
        
        self.inject_scancode(code, is_release);
    }
    
    /// Inject raw scancode directly (for advanced use cases)
    pub fn inject_raw_scancode(&mut self, scancode: u8, extended: bool, is_release: bool) {
        if extended {
            self.output_buffer.push_back(0xE0);
        }
        self.inject_scancode(scancode, is_release);
    }

    /// Check if CPU reset was requested (via 0xFE command or output port)
    /// 
    /// This is the CORRECT x86 behavior - BIOS detects Ctrl+Alt+Del and
    /// writes 0xFE to port 0x64, then keyboard controller pulses CPU reset line.
    pub fn is_reset_requested(&self) -> bool {
        self.reset_requested
    }
    
    /// Clear reset request flag after handling
    pub fn clear_reset_request(&mut self) {
        self.reset_requested = false;
    }

    /// Update modifier key state
    fn update_modifiers(&mut self, scancode: u8, is_release: bool) {
        match scancode {
            0x14 => {
                // Could be left or right ctrl (right has E0 prefix)
                // For simplicity, track as left
                self.modifiers.left_ctrl = !is_release;
            }
            0x11 => {
                // Left alt (right has E0 prefix)
                self.modifiers.left_alt = !is_release;
            }
            0x12 => {
                self.modifiers.left_shift = !is_release;
            }
            0x59 => {
                self.modifiers.right_shift = !is_release;
            }
            _ => {}
        }
    }
    

    
    /// Read from data port (0x60)
    fn read_data(&mut self) -> u8 {
        if let Some(data) = self.output_buffer.pop_front() {
            // Check if buffer is now empty
            if self.output_buffer.is_empty() {
                self.status &= !status::OUTPUT_FULL;
                self.interrupt_pending = false;
            }
            data
        } else {
            0x00
        }
    }
    
    /// Write to data port (0x60)
    fn write_data(&mut self, value: u8) {
        if self.expecting_data {
            self.handle_command_data(value);
            self.expecting_data = false;
        } else {
            // Send command to keyboard
            self.handle_keyboard_command(value);
        }
    }
    
    /// Read status register (0x64)
    fn read_status(&self) -> u8 {
        self.status
    }
    
    /// Write command to controller (0x64)
    fn write_command(&mut self, cmd: u8) {
        self.last_command = Some(cmd);
        
        match cmd {
            0x20 => {
                // Read controller config byte
                self.output_buffer.push_back(self.config);
                self.status |= status::OUTPUT_FULL;
            }
            0x60 => {
                // Write controller config byte
                self.expecting_data = true;
            }
            0xA7 => {
                // Disable mouse
            }
            0xA8 => {
                // Enable mouse
            }
            0xA9 => {
                // Test mouse port - return 0x00 (OK)
                self.output_buffer.push_back(0x00);
                self.status |= status::OUTPUT_FULL;
            }
            0xAA => {
                // Self-test - return 0x55 (OK)
                self.output_buffer.push_back(0x55);
                self.status |= status::OUTPUT_FULL;
            }
            0xAB => {
                // Test keyboard port - return 0x00 (OK)
                self.output_buffer.push_back(0x00);
                self.status |= status::OUTPUT_FULL;
            }
            0xAD => {
                // Disable keyboard
                self.enabled = false;
                self.status &= !status::KEYBOARD_ENABLED;
            }
            0xAE => {
                // Enable keyboard
                self.enabled = true;
                self.status |= status::KEYBOARD_ENABLED;
            }
            0xD0 => {
                // Read output port
                self.output_buffer.push_back(0xCF); // A20 enabled, CPU reset high
                self.status |= status::OUTPUT_FULL;
            }
            0xD1 => {
                // Write output port
                self.expecting_data = true;
            }
            0xFE => {
                // Pulse output port - CPU reset line
                // Real hardware: pulses the CPU RESET pin
                // In emulation: VM should check for this and trigger reset
                // The reset_requested flag is checked by VM.tick()
                self.reset_requested = true;
            }
            0xFF => {
                // Reset controller
                self.reset();
                self.output_buffer.push_back(0xAA); // Self-test OK
                self.status |= status::OUTPUT_FULL;
            }
            _ => {}
        }
    }
    
    /// Handle data byte following a command
    fn handle_command_data(&mut self, value: u8) {
        match self.last_command {
            Some(0x60) => {
                // Set controller config
                self.config = value;
            }
            Some(0xD1) => {
                // Write output port
                // Bit 0 = CPU reset (0 = reset)
                // Bit 1 = A20 gate
                if value & 0x01 == 0 {
                    // CPU reset requested via output port
                    self.reset_requested = true;
                }
            }
            _ => {}
        }
        self.last_command = None;
    }
    
    /// Handle command sent directly to keyboard
    fn handle_keyboard_command(&mut self, cmd: u8) {
        match cmd {
            0xED => {
                // Set LEDs - ACK and wait for data
                self.output_buffer.push_back(0xFA); // ACK
                self.status |= status::OUTPUT_FULL;
                self.expecting_data = true;
                self.last_command = Some(cmd);
            }
            0xEE => {
                // Echo - return 0xEE
                self.output_buffer.push_back(0xEE);
                self.status |= status::OUTPUT_FULL;
            }
            0xF0 => {
                // Set scancode set
                self.output_buffer.push_back(0xFA); // ACK
                self.status |= status::OUTPUT_FULL;
                self.expecting_data = true;
                self.last_command = Some(cmd);
            }
            0xF2 => {
                // Identify keyboard
                self.output_buffer.push_back(0xFA); // ACK
                self.output_buffer.push_back(0xAB); // MF2 keyboard
                self.output_buffer.push_back(0x83);
                self.status |= status::OUTPUT_FULL;
            }
            0xF3 => {
                // Set typematic rate
                self.output_buffer.push_back(0xFA); // ACK
                self.status |= status::OUTPUT_FULL;
                self.expecting_data = true;
                self.last_command = Some(cmd);
            }
            0xF4 => {
                // Enable scanning
                self.enabled = true;
                self.output_buffer.push_back(0xFA); // ACK
                self.status |= status::OUTPUT_FULL;
            }
            0xF5 => {
                // Disable scanning
                self.enabled = false;
                self.output_buffer.push_back(0xFA); // ACK
                self.status |= status::OUTPUT_FULL;
            }
            0xF6 => {
                // Set default parameters
                self.scancode_set = 2;
                self.output_buffer.push_back(0xFA); // ACK
                self.status |= status::OUTPUT_FULL;
            }
            0xFF => {
                // Reset keyboard
                self.output_buffer.push_back(0xFA); // ACK
                self.output_buffer.push_back(0xAA); // Self-test passed
                self.status |= status::OUTPUT_FULL;
            }
            _ => {
                // Unknown command - ACK anyway
                self.output_buffer.push_back(0xFA);
                self.status |= status::OUTPUT_FULL;
            }
        }
    }
}

impl Default for Ps2Keyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for Ps2Keyboard {
    fn id(&self) -> DeviceId {
        self.id
    }
    
    fn name(&self) -> &str {
        "PS/2 Keyboard Controller"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    
    fn reset(&mut self) {
        self.output_buffer.clear();
        self.input_buffer.clear();
        self.status = status::SYSTEM_FLAG | status::KEYBOARD_ENABLED;
        self.config = 0x45;
        self.last_command = None;
        self.expecting_data = false;
        self.modifiers = ModifierState::default();
        self.interrupt_pending = false;
        self.reset_requested = false;
        self.enabled = true;
        self.scancode_set = 2;
    }
    
    fn port_read(&mut self, port: u16, _access: IoAccess) -> u32 {
        match port {
            0x60 => self.read_data() as u32,
            0x64 => self.read_status() as u32,
            _ => 0xFF,
        }
    }
    
    fn port_write(&mut self, port: u16, value: u32, _access: IoAccess) {
        match port {
            0x60 => self.write_data(value as u8),
            0x64 => self.write_command(value as u8),
            _ => {}
        }
    }
    
    fn handles_port(&self, port: u16) -> bool {
        port == 0x60 || port == 0x64
    }
    
    fn has_interrupt(&self) -> bool {
        self.interrupt_pending && (self.config & 0x01) != 0
    }
    
    fn interrupt_vector(&self) -> Option<u8> {
        if self.has_interrupt() {
            Some(1) // IRQ 1 for keyboard
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_keyboard_creation() {
        let kb = Ps2Keyboard::new();
        assert_eq!(kb.scancode_set, 2);
        assert!(kb.enabled);
        assert!(!kb.reset_requested);
    }
    
    #[test]
    fn test_reset_via_0xfe_command() {
        // Test CORRECT x86 behavior: BIOS sends 0xFE to trigger reset
        let mut kb = Ps2Keyboard::new();
        
        // Initially no reset requested
        assert!(!kb.is_reset_requested());
        
        // BIOS sends 0xFE command to port 0x64 (pulse reset line)
        kb.port_write(0x64, 0xFE, IoAccess::Byte);
        
        // Now reset should be requested
        assert!(kb.is_reset_requested());
        
        // After handling, clear it
        kb.clear_reset_request();
        assert!(!kb.is_reset_requested());
    }
    
    #[test]
    fn test_modifier_tracking() {
        // Test that modifier keys are tracked correctly
        // (BIOS uses this via INT 09h to detect combinations)
        let mut kb = Ps2Keyboard::new();
        
        // Press Ctrl
        kb.inject_key("lctrl", false);
        assert!(kb.modifiers.ctrl_pressed());
        
        // Press Alt
        kb.inject_key("lalt", false);
        assert!(kb.modifiers.alt_pressed());
        
        // Release
        kb.inject_key("lctrl", true);
        kb.inject_key("lalt", true);
        assert!(!kb.modifiers.ctrl_pressed());
        assert!(!kb.modifiers.alt_pressed());
    }
    
    #[test]
    fn test_scancode_output() {
        // Test that scancodes are correctly placed in output buffer
        // Note: With translation enabled, Set 2 scancodes are converted to Set 1
        let mut kb = Ps2Keyboard::new();
        
        // Type 'a' key
        kb.inject_key("a", false);
        
        // Should have scancode in buffer (Set 1: 'a' = 0x1E)
        assert!(!kb.output_buffer.is_empty());
        assert_eq!(kb.output_buffer[0], 0x1E, "'a' should be Set 1 scancode 0x1E");
        
        // Status should show data available
        assert!(kb.status & 0x01 != 0);
        
        // Read should clear buffer
        let data = kb.port_read(0x60, IoAccess::Byte);
        assert_eq!(data, 0x1E);
    }
    
    #[test]
    fn test_controller_self_test() {
        let mut kb = Ps2Keyboard::new();
        
        // Send self-test command
        kb.port_write(0x64, 0xAA, IoAccess::Byte);
        
        // Read result
        let result = kb.port_read(0x60, IoAccess::Byte);
        assert_eq!(result, 0x55); // Self-test passed
    }
    
    #[test]
    fn test_letter_keys() {
        let mut kb = Ps2Keyboard::new();
        
        // With translation enabled (config bit 6 = 0x40), output is Set 1 format
        // Set 1 release code = make code | 0x80
        // Test 'a' key: Set 2 = 0x1C -> Set 1 = 0x1E
        kb.output_buffer.clear();
        kb.inject_key("a", false);
        assert!(!kb.output_buffer.is_empty(), "Letter 'a' should produce output");
        assert_eq!(kb.output_buffer[0], 0x1E, "Letter 'a' Set 1 scancode");
        
        // Test release - Set 1 uses bit 7 for release (0x1E | 0x80 = 0x9E)
        kb.output_buffer.clear();
        kb.inject_key("a", true);
        assert_eq!(kb.output_buffer.len(), 1, "Set 1 release is single byte");
        assert_eq!(kb.output_buffer[0], 0x9E, "Release of 'a' should be 0x9E");
    }
    
    #[test]
    fn test_number_keys() {
        let mut kb = Ps2Keyboard::new();
        
        // Test all number keys
        for n in '0'..='9' {
            kb.output_buffer.clear();
            kb.inject_key(&n.to_string(), false);
            assert!(!kb.output_buffer.is_empty(), "Number '{}' should produce output", n);
        }
    }
    
    #[test]
    fn test_symbol_keys() {
        let mut kb = Ps2Keyboard::new();
        
        // Test common symbol keys that users type frequently
        let symbols = ["/", "\\", "[", "]", ";", "'", ",", ".", "-", "=", "`"];
        
        for sym in symbols {
            kb.output_buffer.clear();
            kb.inject_key(sym, false);
            assert!(!kb.output_buffer.is_empty(), "Symbol '{}' should produce output", sym);
        }
    }
    
    #[test]
    fn test_extended_keys() {
        let mut kb = Ps2Keyboard::new();
        
        // Test extended keys that require E0 prefix
        let extended_keys = ["up", "down", "left", "right", "insert", "delete", "home", "end", "pageup", "pagedown"];
        
        for key in extended_keys {
            kb.output_buffer.clear();
            kb.inject_key(key, false);
            assert!(kb.output_buffer.len() >= 2, "Extended key '{}' should have E0 prefix", key);
            assert_eq!(kb.output_buffer[0], 0xE0, "Extended key '{}' should start with E0", key);
        }
    }
    
    #[test]
    fn test_slash_key_specifically() {
        // This test specifically verifies the slash key issue is fixed
        // With translation enabled: Set 2 0x4A -> Set 1 0x35
        let mut kb = Ps2Keyboard::new();
        
        // Test "/" character directly
        kb.output_buffer.clear();
        kb.inject_key("/", false);
        assert!(!kb.output_buffer.is_empty(), "Slash '/' should produce output");
        assert_eq!(kb.output_buffer[0], 0x35, "Slash should map to Set 1 scancode 0x35");
        
        // Test "slash" string (from JS code mapping)
        kb.output_buffer.clear();
        kb.inject_key("slash", false);
        assert!(!kb.output_buffer.is_empty(), "Key name 'slash' should produce output");
        assert_eq!(kb.output_buffer[0], 0x35, "Slash should map to Set 1 scancode 0x35");
    }
}
