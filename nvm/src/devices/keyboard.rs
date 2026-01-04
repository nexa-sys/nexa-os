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
            id: DeviceId(0x50), // New device ID for keyboard
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
    pub fn inject_key(&mut self, key: &str, is_release: bool) {
        let scancode = match key.to_lowercase().as_str() {
            "f1" => 0x05,
            "f2" => 0x06,
            "f3" => 0x04,
            "f4" => 0x0C,
            "f5" => 0x03,
            "f6" => 0x0B,
            "f7" => 0x83,
            "f8" => 0x0A,
            "f9" => 0x01,
            "f10" => 0x09,
            "f11" => 0x78,
            "f12" => 0x07,
            "esc" | "escape" => 0x76,
            "delete" | "del" => 0x71, // Extended key (E0 71)
            "enter" | "return" => 0x5A,
            "space" => 0x29,
            "up" => 0x75,    // Extended (E0 75)
            "down" => 0x72,  // Extended (E0 72)
            "left" => 0x6B,  // Extended (E0 6B)
            "right" => 0x74, // Extended (E0 74)
            "lctrl" => 0x14,
            "rctrl" => 0x14, // Extended (E0 14)
            "lalt" => 0x11,
            "ralt" => 0x11,  // Extended (E0 11)
            "lshift" => 0x12,
            "rshift" => 0x59,
            _ => return, // Unknown key
        };
        
        // Check if it's an extended key
        let is_extended = matches!(key.to_lowercase().as_str(), 
            "delete" | "del" | "up" | "down" | "left" | "right" | "rctrl" | "ralt");
        
        if is_extended {
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
    fn test_controller_self_test() {
        let mut kb = Ps2Keyboard::new();
        
        // Send self-test command
        kb.port_write(0x64, 0xAA, IoAccess::Byte);
        
        // Read result
        let result = kb.port_read(0x60, IoAccess::Byte);
        assert_eq!(result, 0x55); // Self-test passed
    }
}
