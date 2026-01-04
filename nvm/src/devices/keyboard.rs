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

/// Special key codes for boot-time handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKey {
    /// F2 key pressed (enter BIOS setup)
    F2,
    /// Delete key pressed (enter BIOS setup)
    Delete,
    /// F12 key pressed (boot menu)
    F12,
    /// Escape key pressed
    Escape,
    /// Ctrl+Alt+Del combination detected
    CtrlAltDel,
    /// Enter key pressed
    Enter,
    /// Up arrow key
    Up,
    /// Down arrow key
    Down,
    /// F10 key (save & exit in BIOS)
    F10,
}

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
    /// Modifier key state
    modifiers: ModifierState,
    /// Special key events detected during boot
    special_keys: VecDeque<SpecialKey>,
    /// Interrupt pending flag
    interrupt_pending: bool,
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
            special_keys: VecDeque::new(),
            interrupt_pending: false,
            enabled: true,
            scancode_set: 2,
        }
    }
    
    /// Process an incoming scancode (from host keyboard event)
    pub fn inject_scancode(&mut self, scancode: u8, is_release: bool) {
        // Handle modifier keys
        self.update_modifiers(scancode, is_release);
        
        // Check for special key combinations during boot
        if !is_release {
            self.check_special_keys(scancode);
        }
        
        // Add to output buffer
        if is_release {
            // Send break code (0xF0 prefix for scancode set 2)
            self.output_buffer.push_back(0xF0);
        }
        self.output_buffer.push_back(scancode);
        
        // Update status and raise interrupt
        self.status |= status::OUTPUT_FULL;
        self.interrupt_pending = true;
    }
    
    /// Inject a key press event using human-readable key name
    /// 
    /// Supports full PS/2 Scancode Set 2 for enterprise-grade keyboard emulation.
    /// Compatible with ESXi-style console input handling.
    pub fn inject_key(&mut self, key: &str, is_release: bool) {
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
            #[cfg(debug_assertions)]
            eprintln!("[PS2] Unknown key: '{}'", key);
            return;
        };
        
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
    
    /// Check for Ctrl+Alt+Del and other special combinations
    fn check_special_keys(&mut self, scancode: u8) {
        // F2 key (scancode set 2)
        if scancode == 0x06 {
            self.special_keys.push_back(SpecialKey::F2);
        }
        
        // F10 key
        if scancode == 0x09 {
            self.special_keys.push_back(SpecialKey::F10);
        }
        
        // F12 key
        if scancode == 0x07 {
            self.special_keys.push_back(SpecialKey::F12);
        }
        
        // Delete key (with E0 prefix) - 0x71
        if scancode == 0x71 {
            // Check if Ctrl+Alt is held
            if self.modifiers.ctrl_pressed() && self.modifiers.alt_pressed() {
                self.special_keys.push_back(SpecialKey::CtrlAltDel);
            } else {
                self.special_keys.push_back(SpecialKey::Delete);
            }
        }
        
        // Escape
        if scancode == 0x76 {
            self.special_keys.push_back(SpecialKey::Escape);
        }
        
        // Enter
        if scancode == 0x5A {
            self.special_keys.push_back(SpecialKey::Enter);
        }
        
        // Up arrow
        if scancode == 0x75 {
            self.special_keys.push_back(SpecialKey::Up);
        }
        
        // Down arrow
        if scancode == 0x72 {
            self.special_keys.push_back(SpecialKey::Down);
        }
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
    
    /// Poll for special key events (for boot-time handling)
    pub fn poll_special_key(&mut self) -> Option<SpecialKey> {
        self.special_keys.pop_front()
    }
    
    /// Check if any special key was pressed
    pub fn has_special_key(&self) -> bool {
        !self.special_keys.is_empty()
    }
    
    /// Check if setup key (F2 or DEL) was pressed
    pub fn setup_key_pressed(&self) -> bool {
        self.special_keys.iter().any(|k| matches!(k, SpecialKey::F2 | SpecialKey::Delete))
    }
    
    /// Check if reboot combination (Ctrl+Alt+Del) was pressed
    pub fn reboot_requested(&self) -> bool {
        self.special_keys.iter().any(|k| matches!(k, SpecialKey::CtrlAltDel))
    }
    
    /// Check if Ctrl key is currently pressed
    pub fn is_ctrl_pressed(&self) -> bool {
        self.modifiers.ctrl_pressed()
    }
    
    /// Check if Alt key is currently pressed  
    pub fn is_alt_pressed(&self) -> bool {
        self.modifiers.alt_pressed()
    }
    
    /// Clear special key queue
    pub fn clear_special_keys(&mut self) {
        self.special_keys.clear();
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
                // Pulse output port (CPU reset)
                // This is used for Ctrl+Alt+Del
                self.special_keys.push_back(SpecialKey::CtrlAltDel);
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
                    // CPU reset requested
                    self.special_keys.push_back(SpecialKey::CtrlAltDel);
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
        self.special_keys.clear();
        self.interrupt_pending = false;
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
    }
    
    #[test]
    fn test_special_key_detection() {
        let mut kb = Ps2Keyboard::new();
        
        // Inject F2 key
        kb.inject_key("f2", false);
        assert!(kb.setup_key_pressed());
        
        kb.clear_special_keys();
        
        // Inject Delete key
        kb.inject_key("del", false);
        assert!(kb.setup_key_pressed());
    }
    
    #[test]
    fn test_ctrl_alt_del() {
        let mut kb = Ps2Keyboard::new();
        
        // Press Ctrl
        kb.inject_key("lctrl", false);
        // Press Alt
        kb.inject_key("lalt", false);
        // Press Delete
        kb.inject_key("del", false);
        
        assert!(kb.reboot_requested());
    }
    
    #[test]
    fn test_ctrl_alt_del_from_frontend() {
        // Test with exact key names sent by frontend: ["ctrl", "alt", "delete"]
        let mut kb = Ps2Keyboard::new();
        
        // Simulate frontend sending Ctrl+Alt+Del combo
        kb.inject_key("ctrl", false);    // Press ctrl
        assert!(kb.modifiers.ctrl_pressed(), "Ctrl should be pressed after 'ctrl' key");
        
        kb.inject_key("alt", false);     // Press alt
        assert!(kb.modifiers.alt_pressed(), "Alt should be pressed after 'alt' key");
        
        kb.inject_key("delete", false);  // Press delete
        
        // Check if reboot was requested
        assert!(kb.reboot_requested(), "Ctrl+Alt+Del should trigger reboot request");
        
        // Now simulate release in reverse order
        kb.inject_key("delete", true);
        kb.inject_key("alt", true);
        kb.inject_key("ctrl", true);
        
        assert!(!kb.modifiers.ctrl_pressed(), "Ctrl should be released");
        assert!(!kb.modifiers.alt_pressed(), "Alt should be released");
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
        
        // Test all letter keys generate valid scancodes
        for c in 'a'..='z' {
            kb.output_buffer.clear();
            kb.inject_key(&c.to_string(), false);
            assert!(!kb.output_buffer.is_empty(), "Letter '{}' should produce output", c);
            
            // Test release (should have F0 prefix)
            kb.output_buffer.clear();
            kb.inject_key(&c.to_string(), true);
            assert_eq!(kb.output_buffer.len(), 2, "Release of '{}' should have F0 prefix", c);
            assert_eq!(kb.output_buffer[0], 0xF0);
        }
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
        let mut kb = Ps2Keyboard::new();
        
        // Test "/" character directly
        kb.output_buffer.clear();
        kb.inject_key("/", false);
        assert!(!kb.output_buffer.is_empty(), "Slash '/' should produce output");
        assert_eq!(kb.output_buffer[0], 0x4A, "Slash should map to scancode 0x4A");
        
        // Test "slash" string (from JS code mapping)
        kb.output_buffer.clear();
        kb.inject_key("slash", false);
        assert!(!kb.output_buffer.is_empty(), "Key name 'slash' should produce output");
        assert_eq!(kb.output_buffer[0], 0x4A, "Slash should map to scancode 0x4A");
    }
}
