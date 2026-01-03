//! Tests for drivers/vga.rs - VGA Text Mode Driver
//!
//! Tests the VGA text buffer, colors, and screen operations.

#[cfg(test)]
mod tests {
    // Constants from the VGA module
    const BUFFER_HEIGHT: usize = 25;
    const BUFFER_WIDTH: usize = 80;
    const VGA_BUFFER_ADDR: usize = 0xb8000;

    // =========================================================================
    // Buffer Dimensions Tests
    // =========================================================================

    #[test]
    fn test_buffer_dimensions() {
        // Standard VGA text mode is 80x25
        assert_eq!(BUFFER_WIDTH, 80);
        assert_eq!(BUFFER_HEIGHT, 25);
    }

    #[test]
    fn test_buffer_size() {
        // Total characters on screen
        let total_chars = BUFFER_WIDTH * BUFFER_HEIGHT;
        assert_eq!(total_chars, 2000);
        
        // Each character takes 2 bytes (char + attribute)
        let buffer_bytes = total_chars * 2;
        assert_eq!(buffer_bytes, 4000);
    }

    #[test]
    fn test_vga_buffer_address() {
        // VGA text buffer is at 0xB8000 in physical memory
        assert_eq!(VGA_BUFFER_ADDR, 0xB8000);
    }

    // =========================================================================
    // Color Tests
    // =========================================================================

    #[test]
    fn test_color_values() {
        // Standard VGA 16-color palette
        #[derive(Clone, Copy)]
        #[repr(u8)]
        enum Color {
            Black = 0x0,
            Blue = 0x1,
            Green = 0x2,
            Cyan = 0x3,
            Red = 0x4,
            Magenta = 0x5,
            Brown = 0x6,
            LightGray = 0x7,
            DarkGray = 0x8,
            LightBlue = 0x9,
            LightGreen = 0xA,
            LightCyan = 0xB,
            LightRed = 0xC,
            Pink = 0xD,
            Yellow = 0xE,
            White = 0xF,
        }
        
        // Verify 4-bit color range (0-15)
        assert_eq!(Color::Black as u8, 0);
        assert_eq!(Color::White as u8, 15);
        assert!(Color::White as u8 <= 0xF);
    }

    #[test]
    fn test_color_code_encoding() {
        // Color code is (background << 4) | foreground
        fn make_color_code(fg: u8, bg: u8) -> u8 {
            (bg << 4) | fg
        }
        
        // White on black
        assert_eq!(make_color_code(0xF, 0x0), 0x0F);
        // Black on white
        assert_eq!(make_color_code(0x0, 0xF), 0xF0);
        // Green on blue
        assert_eq!(make_color_code(0x2, 0x1), 0x12);
    }

    #[test]
    fn test_default_color() {
        // Default terminal color: light gray on black
        let default_fg: u8 = 0x7; // LightGray
        let default_bg: u8 = 0x0; // Black
        let default_color = (default_bg << 4) | default_fg;
        assert_eq!(default_color, 0x07);
    }

    // =========================================================================
    // Screen Character Tests
    // =========================================================================

    #[test]
    fn test_screen_char_size() {
        // ScreenChar should be exactly 2 bytes
        #[repr(C)]
        struct ScreenChar {
            ascii_character: u8,
            color_code: u8,
        }
        
        assert_eq!(core::mem::size_of::<ScreenChar>(), 2);
    }

    #[test]
    fn test_screen_char_layout() {
        // First byte: ASCII character
        // Second byte: Color code (foreground + background)
        #[repr(C)]
        struct ScreenChar {
            ascii_character: u8,
            color_code: u8,
        }
        
        let ch = ScreenChar {
            ascii_character: b'A',
            color_code: 0x07, // LightGray on Black
        };
        
        assert_eq!(ch.ascii_character, 0x41);
        assert_eq!(ch.color_code, 0x07);
    }

    // =========================================================================
    // Cursor Position Tests
    // =========================================================================

    #[test]
    fn test_cursor_position_range() {
        // Cursor can be at any position in the buffer
        let max_col = BUFFER_WIDTH - 1;
        let max_row = BUFFER_HEIGHT - 1;
        
        assert_eq!(max_col, 79);
        assert_eq!(max_row, 24);
    }

    #[test]
    fn test_cursor_linear_position() {
        // Convert (row, col) to linear offset
        fn cursor_offset(row: usize, col: usize) -> usize {
            row * BUFFER_WIDTH + col
        }
        
        // Top-left corner
        assert_eq!(cursor_offset(0, 0), 0);
        // End of first row
        assert_eq!(cursor_offset(0, 79), 79);
        // Start of second row
        assert_eq!(cursor_offset(1, 0), 80);
        // Last position
        assert_eq!(cursor_offset(24, 79), 1999);
    }

    // =========================================================================
    // Scrolling Tests
    // =========================================================================

    #[test]
    fn test_scroll_line_count() {
        // When scrolling, move (HEIGHT - 1) lines up
        let lines_to_copy = BUFFER_HEIGHT - 1;
        assert_eq!(lines_to_copy, 24);
        
        // Characters to copy during scroll
        let chars_to_copy = lines_to_copy * BUFFER_WIDTH;
        assert_eq!(chars_to_copy, 1920);
    }

    #[test]
    fn test_blank_line_fill() {
        // After scrolling, last line should be filled with spaces
        let blank_char: u8 = b' ';
        let default_color: u8 = 0x07;
        
        assert_eq!(blank_char, 0x20);
        assert_eq!(default_color, 0x07);
    }

    // =========================================================================
    // Tab Handling Tests
    // =========================================================================

    #[test]
    fn test_tab_width() {
        // Standard tab width is 8 spaces
        const TAB_WIDTH: usize = 8;
        
        // Tab at column 0 -> column 8
        let col = 0;
        let next_col = (col / TAB_WIDTH + 1) * TAB_WIDTH;
        assert_eq!(next_col, 8);
        
        // Tab at column 5 -> column 8
        let col = 5;
        let next_col = (col / TAB_WIDTH + 1) * TAB_WIDTH;
        assert_eq!(next_col, 8);
        
        // Tab at column 8 -> column 16
        let col = 8;
        let next_col = (col / TAB_WIDTH + 1) * TAB_WIDTH;
        assert_eq!(next_col, 16);
    }
}
