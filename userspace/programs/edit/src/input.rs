//! Input handling for the editor

/// Key event representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Alt(char),
    
    // Special keys
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    
    // Arrow keys
    Up,
    Down,
    Left,
    Right,
    
    // Navigation keys
    Home,
    End,
    PageUp,
    PageDown,
    
    // Function keys
    F(u8),
    
    // Mouse events
    MousePress(MouseButton, usize, usize),
    MouseRelease(usize, usize),
    MouseDrag(usize, usize),
    MouseScroll(ScrollDirection, usize, usize),
    
    // Unknown/unhandled
    Unknown(Vec<u8>),
}

/// Mouse button
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// Scroll direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Input reader that parses escape sequences
pub struct InputReader {
    buffer: Vec<u8>,
}

impl InputReader {
    pub fn new() -> Self {
        InputReader {
            buffer: Vec::with_capacity(32),
        }
    }
    
    /// Parse input bytes and return a key event
    pub fn parse_key(&mut self, bytes: &[u8]) -> Option<Key> {
        if bytes.is_empty() {
            return None;
        }
        
        self.buffer.clear();
        self.buffer.extend_from_slice(bytes);
        
        self.parse_from_buffer()
    }
    
    fn parse_from_buffer(&mut self) -> Option<Key> {
        if self.buffer.is_empty() {
            return None;
        }
        
        let first = self.buffer[0];
        
        // Escape sequence
        if first == 0x1b {
            if self.buffer.len() == 1 {
                return Some(Key::Escape);
            }
            
            return self.parse_escape_sequence();
        }
        
        // Control characters
        if first < 32 {
            return Some(match first {
                0 => Key::Ctrl(' '),
                1..=26 => Key::Ctrl((first + b'a' - 1) as char),
                9 => Key::Tab,
                10 | 13 => Key::Enter,
                27 => Key::Escape,
                127 => Key::Backspace,
                _ => Key::Unknown(vec![first]),
            });
        }
        
        // Backspace (some terminals send 127)
        if first == 127 {
            return Some(Key::Backspace);
        }
        
        // Regular character (potentially UTF-8)
        if let Some(ch) = self.parse_utf8_char() {
            return Some(Key::Char(ch));
        }
        
        Some(Key::Unknown(self.buffer.clone()))
    }
    
    fn parse_escape_sequence(&mut self) -> Option<Key> {
        if self.buffer.len() < 2 {
            return Some(Key::Escape);
        }
        
        let second = self.buffer[1];
        
        // Alt + key
        if second != b'[' && second != b'O' {
            if second >= 32 && second < 127 {
                return Some(Key::Alt(second as char));
            }
            return Some(Key::Unknown(self.buffer.clone()));
        }
        
        // CSI sequence: ESC [
        if second == b'[' {
            return self.parse_csi_sequence();
        }
        
        // SS3 sequence: ESC O (function keys)
        if second == b'O' {
            if self.buffer.len() >= 3 {
                let third = self.buffer[2];
                return Some(match third {
                    b'P' => Key::F(1),
                    b'Q' => Key::F(2),
                    b'R' => Key::F(3),
                    b'S' => Key::F(4),
                    _ => Key::Unknown(self.buffer.clone()),
                });
            }
        }
        
        Some(Key::Unknown(self.buffer.clone()))
    }
    
    fn parse_csi_sequence(&mut self) -> Option<Key> {
        if self.buffer.len() < 3 {
            return Some(Key::Unknown(self.buffer.clone()));
        }
        
        // Check for simple arrow keys: ESC [ A/B/C/D
        let last = self.buffer[self.buffer.len() - 1];
        
        match last {
            b'A' => return Some(Key::Up),
            b'B' => return Some(Key::Down),
            b'C' => return Some(Key::Right),
            b'D' => return Some(Key::Left),
            b'H' => return Some(Key::Home),
            b'F' => return Some(Key::End),
            _ => {}
        }
        
        // Parse numeric parameters
        let params: Vec<u32> = self.buffer[2..self.buffer.len()-1]
            .split(|&b| b == b';')
            .filter_map(|s| {
                std::str::from_utf8(s).ok().and_then(|s| s.parse().ok())
            })
            .collect();
        
        match last {
            b'~' => {
                // Extended key codes
                if !params.is_empty() {
                    return Some(match params[0] {
                        1 | 7 => Key::Home,
                        2 => Key::Unknown(self.buffer.clone()), // Insert
                        3 => Key::Delete,
                        4 | 8 => Key::End,
                        5 => Key::PageUp,
                        6 => Key::PageDown,
                        11 => Key::F(1),
                        12 => Key::F(2),
                        13 => Key::F(3),
                        14 => Key::F(4),
                        15 => Key::F(5),
                        17 => Key::F(6),
                        18 => Key::F(7),
                        19 => Key::F(8),
                        20 => Key::F(9),
                        21 => Key::F(10),
                        23 => Key::F(11),
                        24 => Key::F(12),
                        _ => Key::Unknown(self.buffer.clone()),
                    });
                }
            }
            b'M' | b'm' => {
                // Mouse event (SGR format)
                return self.parse_mouse_event(&params, last == b'm');
            }
            _ => {}
        }
        
        Some(Key::Unknown(self.buffer.clone()))
    }
    
    fn parse_mouse_event(&mut self, params: &[u32], is_release: bool) -> Option<Key> {
        if params.len() >= 3 {
            let button_code = params[0];
            let col = params[1] as usize;
            let row = params[2] as usize;
            
            if is_release {
                return Some(Key::MouseRelease(row, col));
            }
            
            let button = match button_code & 0x03 {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                _ => return Some(Key::Unknown(self.buffer.clone())),
            };
            
            // Check for scroll
            if button_code & 64 != 0 {
                let dir = if button_code & 0x01 == 0 {
                    ScrollDirection::Up
                } else {
                    ScrollDirection::Down
                };
                return Some(Key::MouseScroll(dir, row, col));
            }
            
            // Check for drag
            if button_code & 32 != 0 {
                return Some(Key::MouseDrag(row, col));
            }
            
            return Some(Key::MousePress(button, row, col));
        }
        
        Some(Key::Unknown(self.buffer.clone()))
    }
    
    fn parse_utf8_char(&mut self) -> Option<char> {
        if self.buffer.is_empty() {
            return None;
        }
        
        let first = self.buffer[0];
        
        // ASCII
        if first < 128 {
            return Some(first as char);
        }
        
        // Determine UTF-8 length
        let len = if first & 0xE0 == 0xC0 {
            2
        } else if first & 0xF0 == 0xE0 {
            3
        } else if first & 0xF8 == 0xF0 {
            4
        } else {
            return None;
        };
        
        if self.buffer.len() >= len {
            if let Ok(s) = std::str::from_utf8(&self.buffer[..len]) {
                return s.chars().next();
            }
        }
        
        None
    }
}

impl Default for InputReader {
    fn default() -> Self {
        Self::new()
    }
}
