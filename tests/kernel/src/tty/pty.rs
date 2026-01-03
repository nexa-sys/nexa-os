//! PTY (Pseudo-Terminal) Tests

#[cfg(test)]
mod tests {
    use crate::tty::pty::{Termios, WinSize, PtyDirection, PtyIoResult};

    // =========================================================================
    // Termios Structure Tests
    // =========================================================================

    #[test]
    fn test_termios_size() {
        let size = core::mem::size_of::<Termios>();
        // c_iflag(4) + c_oflag(4) + c_cflag(4) + c_lflag(4) + c_line(1) + c_cc(32) + padding(3) + c_ispeed(4) + c_ospeed(4) = 60
        assert_eq!(size, 60);
    }

    #[test]
    fn test_termios_alignment() {
        let align = core::mem::align_of::<Termios>();
        assert_eq!(align, 4);
    }

    #[test]
    fn test_termios_copy() {
        let t1 = Termios {
            c_iflag: 0x100,
            c_oflag: 0x001,
            c_cflag: 0o60,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0; 32],
            c_ispeed: 115200,
            c_ospeed: 115200,
        };
        let t2 = t1;
        assert_eq!(t1.c_iflag, t2.c_iflag);
        assert_eq!(t1.c_ispeed, t2.c_ispeed);
    }

    // =========================================================================
    // Termios Flags Tests
    // =========================================================================

    #[test]
    fn test_termios_iflag_icrnl() {
        const ICRNL: u32 = 0o000400;
        let mut t = Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0; 32],
            c_ispeed: 0,
            c_ospeed: 0,
        };
        
        assert_eq!(t.c_iflag & ICRNL, 0);
        t.c_iflag |= ICRNL;
        assert_ne!(t.c_iflag & ICRNL, 0);
    }

    #[test]
    fn test_termios_lflag_echo() {
        const ECHO: u32 = 0o000010;
        const ICANON: u32 = 0o000002;
        
        let t = Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: ECHO | ICANON,
            c_line: 0,
            c_cc: [0; 32],
            c_ispeed: 0,
            c_ospeed: 0,
        };
        assert!(t.c_lflag & ECHO != 0);
        assert!(t.c_lflag & ICANON != 0);
    }

    #[test]
    fn test_termios_cc_vintr() {
        let mut t = Termios {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: 0,
            c_line: 0,
            c_cc: [0; 32],
            c_ispeed: 0,
            c_ospeed: 0,
        };
        t.c_cc[0] = 0x03; // Ctrl-C for VINTR
        t.c_cc[4] = 0x04; // Ctrl-D for VEOF
        assert_eq!(t.c_cc[0], 0x03);
        assert_eq!(t.c_cc[4], 0x04);
    }

    // =========================================================================
    // WinSize Structure Tests
    // =========================================================================

    #[test]
    fn test_winsize_size() {
        let size = core::mem::size_of::<WinSize>();
        assert_eq!(size, 8); // 4 x u16
    }

    #[test]
    fn test_winsize_standard() {
        let ws = WinSize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(ws.ws_row, 25);
        assert_eq!(ws.ws_col, 80);
        assert_eq!(ws.ws_row as usize * ws.ws_col as usize, 2000);
    }

    #[test]
    fn test_winsize_copy() {
        let ws1 = WinSize {
            ws_row: 50,
            ws_col: 120,
            ws_xpixel: 960,
            ws_ypixel: 800,
        };
        let ws2 = ws1;
        assert_eq!(ws1.ws_row, ws2.ws_row);
        assert_eq!(ws1.ws_col, ws2.ws_col);
    }

    // =========================================================================
    // PtyDirection Tests
    // =========================================================================

    #[test]
    fn test_pty_direction() {
        let mr = PtyDirection::MasterReads;
        let sr = PtyDirection::SlaveReads;
        
        assert!(matches!(mr, PtyDirection::MasterReads));
        assert!(matches!(sr, PtyDirection::SlaveReads));
    }

    // =========================================================================
    // PtyIoResult Tests
    // =========================================================================

    #[test]
    fn test_pty_io_result_bytes() {
        let result = PtyIoResult::Bytes(100);
        if let PtyIoResult::Bytes(n) = result {
            assert_eq!(n, 100);
        } else {
            panic!("Expected Bytes");
        }
    }

    #[test]
    fn test_pty_io_result_variants() {
        assert!(matches!(PtyIoResult::WouldBlock, PtyIoResult::WouldBlock));
        assert!(matches!(PtyIoResult::Eof, PtyIoResult::Eof));
    }
}
