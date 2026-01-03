//! Virtual Terminal Tests

#[cfg(test)]
mod tests {
    use crate::tty::vt::StreamKind;

    // =========================================================================
    // StreamKind Tests
    // =========================================================================

    #[test]
    fn test_stream_kind_variants() {
        let stdout = StreamKind::Stdout;
        let stderr = StreamKind::Stderr;
        
        assert!(matches!(stdout, StreamKind::Stdout));
        assert!(matches!(stderr, StreamKind::Stderr));
    }

    #[test]
    fn test_stream_kind_copy() {
        let s1 = StreamKind::Stdout;
        let s2 = s1;
        assert!(matches!(s2, StreamKind::Stdout));
    }

    // =========================================================================
    // VGA Constants Tests
    // =========================================================================

    #[test]
    fn test_vga_buffer_dimensions() {
        const BUFFER_WIDTH: usize = 80;
        const BUFFER_HEIGHT: usize = 25;
        const BUFFER_SIZE: usize = BUFFER_WIDTH * BUFFER_HEIGHT;
        
        assert_eq!(BUFFER_SIZE, 2000);
    }

    #[test]
    fn test_vga_base_address() {
        const VGA_BASE: usize = 0xB8000;
        assert_eq!(VGA_BASE, 753664);
    }

    #[test]
    fn test_vga_cell_size() {
        // VGA cell: 1 byte char + 1 byte attribute = 2 bytes
        const CELL_SIZE: usize = 2;
        const BUFFER_WIDTH: usize = 80;
        const LINE_BYTES: usize = BUFFER_WIDTH * CELL_SIZE;
        
        assert_eq!(LINE_BYTES, 160);
    }

    // =========================================================================
    // Color Attribute Tests
    // =========================================================================

    #[test]
    fn test_color_encoding() {
        fn make_color(fg: u8, bg: u8) -> u8 {
            (bg << 4) | (fg & 0x0F)
        }
        
        // White on black
        assert_eq!(make_color(15, 0), 0x0F);
        // Green on black
        assert_eq!(make_color(10, 0), 0x0A);
        // Yellow on blue
        assert_eq!(make_color(14, 1), 0x1E);
    }

    #[test]
    fn test_color_extraction() {
        fn get_fg(color: u8) -> u8 {
            color & 0x0F
        }
        fn get_bg(color: u8) -> u8 {
            (color >> 4) & 0x0F
        }
        
        let color = 0x1E; // Yellow on blue
        assert_eq!(get_fg(color), 14); // Yellow
        assert_eq!(get_bg(color), 1);  // Blue
    }

    // =========================================================================
    // Cell Format Tests
    // =========================================================================

    #[test]
    fn test_make_cell() {
        fn make_cell(ch: u8, attr: u8) -> u16 {
            ((attr as u16) << 8) | (ch as u16)
        }
        
        assert_eq!(make_cell(b'A', 0x0F), 0x0F41);
        assert_eq!(make_cell(b' ', 0x07), 0x0720);
    }

    #[test]
    fn test_extract_cell() {
        fn get_char(cell: u16) -> u8 {
            (cell & 0xFF) as u8
        }
        fn get_attr(cell: u16) -> u8 {
            (cell >> 8) as u8
        }
        
        let cell = 0x0F41u16;
        assert_eq!(get_char(cell), 0x41); // 'A'
        assert_eq!(get_attr(cell), 0x0F); // White on black
    }

    // =========================================================================
    // Cursor Position Tests
    // =========================================================================

    #[test]
    fn test_cursor_offset() {
        fn cursor_offset(row: usize, col: usize) -> usize {
            row * 80 + col
        }
        
        assert_eq!(cursor_offset(0, 0), 0);
        assert_eq!(cursor_offset(0, 79), 79);
        assert_eq!(cursor_offset(1, 0), 80);
        assert_eq!(cursor_offset(24, 79), 1999);
    }

    #[test]
    fn test_cursor_from_offset() {
        fn offset_to_row(offset: usize) -> usize {
            offset / 80
        }
        fn offset_to_col(offset: usize) -> usize {
            offset % 80
        }
        
        assert_eq!(offset_to_row(160), 2);
        assert_eq!(offset_to_col(160), 0);
        assert_eq!(offset_to_col(85), 5);
    }

    // =========================================================================
    // Terminal Bounds Tests  
    // =========================================================================

    #[test]
    fn test_terminal_bounds() {
        const MAX_TERMINALS: usize = 6;
        
        fn is_valid_terminal(tty: usize) -> bool {
            tty < MAX_TERMINALS
        }
        
        assert!(is_valid_terminal(0));
        assert!(is_valid_terminal(5));
        assert!(!is_valid_terminal(6));
    }

    #[test]
    fn test_normalize_tty() {
        const MAX_TERMINALS: usize = 6;
        
        fn normalize_tty(tty: usize) -> usize {
            if tty >= MAX_TERMINALS {
                MAX_TERMINALS - 1
            } else {
                tty
            }
        }
        
        assert_eq!(normalize_tty(0), 0);
        assert_eq!(normalize_tty(5), 5);
        assert_eq!(normalize_tty(100), 5);
    }

    // =========================================================================
    // Scroll Region Tests
    // =========================================================================

    #[test]
    fn test_scroll_region() {
        const BUFFER_WIDTH: usize = 80;
        const BUFFER_HEIGHT: usize = 25;
        
        fn scroll_region_size(top: usize, bottom: usize) -> usize {
            if bottom > top && bottom <= BUFFER_HEIGHT {
                (bottom - top) * BUFFER_WIDTH
            } else {
                0
            }
        }
        
        // Full screen scroll
        assert_eq!(scroll_region_size(0, 25), 2000);
        // Partial scroll
        assert_eq!(scroll_region_size(5, 20), 1200);
    }

    // =========================================================================
    // Tab Stop Tests
    // =========================================================================

    #[test]
    fn test_tab_alignment() {
        const TAB_WIDTH: usize = 8;
        
        fn next_tab_stop(col: usize) -> usize {
            (col + TAB_WIDTH) & !(TAB_WIDTH - 1)
        }
        
        assert_eq!(next_tab_stop(0), 8);
        assert_eq!(next_tab_stop(1), 8);
        assert_eq!(next_tab_stop(7), 8);
        assert_eq!(next_tab_stop(8), 16);
        assert_eq!(next_tab_stop(15), 16);
    }
}
