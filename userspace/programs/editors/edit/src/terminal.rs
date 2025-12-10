//! Terminal handling for the editor

use std::io::{self, Read, Write};

/// ANSI escape codes
pub mod ansi {
    pub const ESC: &str = "\x1b";
    pub const CSI: &str = "\x1b[";

    // Cursor movement
    pub const CURSOR_HOME: &str = "\x1b[H";
    pub const CURSOR_SAVE: &str = "\x1b[s";
    pub const CURSOR_RESTORE: &str = "\x1b[u";
    pub const CURSOR_HIDE: &str = "\x1b[?25l";
    pub const CURSOR_SHOW: &str = "\x1b[?25h";

    // Screen operations
    pub const CLEAR_SCREEN: &str = "\x1b[2J";
    pub const CLEAR_LINE: &str = "\x1b[2K";
    pub const CLEAR_LINE_FROM_CURSOR: &str = "\x1b[K";

    // Text attributes
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";
    pub const UNDERLINE: &str = "\x1b[4m";
    pub const BLINK: &str = "\x1b[5m";
    pub const REVERSE: &str = "\x1b[7m";

    // Foreground colors
    pub const FG_BLACK: &str = "\x1b[30m";
    pub const FG_RED: &str = "\x1b[31m";
    pub const FG_GREEN: &str = "\x1b[32m";
    pub const FG_YELLOW: &str = "\x1b[33m";
    pub const FG_BLUE: &str = "\x1b[34m";
    pub const FG_MAGENTA: &str = "\x1b[35m";
    pub const FG_CYAN: &str = "\x1b[36m";
    pub const FG_WHITE: &str = "\x1b[37m";
    pub const FG_DEFAULT: &str = "\x1b[39m";

    // Background colors
    pub const BG_BLACK: &str = "\x1b[40m";
    pub const BG_RED: &str = "\x1b[41m";
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
    pub const BG_MAGENTA: &str = "\x1b[45m";
    pub const BG_CYAN: &str = "\x1b[46m";
    pub const BG_WHITE: &str = "\x1b[47m";
    pub const BG_DEFAULT: &str = "\x1b[49m";

    // Alternate screen buffer
    pub const ALT_SCREEN_ON: &str = "\x1b[?1049h";
    pub const ALT_SCREEN_OFF: &str = "\x1b[?1049l";

    // Mouse support
    pub const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1006h";
    pub const MOUSE_OFF: &str = "\x1b[?1000l\x1b[?1006l";
}

/// Terminal size
#[derive(Debug, Clone, Copy)]
pub struct TermSize {
    pub rows: usize,
    pub cols: usize,
}

impl Default for TermSize {
    fn default() -> Self {
        TermSize { rows: 24, cols: 80 }
    }
}

/// Terminal state manager
pub struct Terminal {
    original_termios: Option<libc::termios>,
    pub size: TermSize,
    output_buffer: String,
}

// Minimal libc bindings for terminal control
mod libc {
    use std::os::raw::{c_int, c_ulong, c_ushort};

    pub const STDIN_FILENO: c_int = 0;
    pub const STDOUT_FILENO: c_int = 1;

    // termios flags
    pub const ICANON: c_ulong = 0x0002;
    pub const ECHO: c_ulong = 0x0008;
    pub const ISIG: c_ulong = 0x0001;
    pub const IXON: c_ulong = 0x0400;
    pub const ICRNL: c_ulong = 0x0100;
    pub const OPOST: c_ulong = 0x0001;
    pub const BRKINT: c_ulong = 0x0002;
    pub const INPCK: c_ulong = 0x0010;
    pub const ISTRIP: c_ulong = 0x0020;
    pub const CS8: c_ulong = 0x0030;

    pub const TCSAFLUSH: c_int = 2;

    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;

    pub const TIOCGWINSZ: c_ulong = 0x5413;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct termios {
        pub c_iflag: c_ulong,
        pub c_oflag: c_ulong,
        pub c_cflag: c_ulong,
        pub c_lflag: c_ulong,
        pub c_line: u8,
        pub c_cc: [u8; 32],
        pub c_ispeed: c_ulong,
        pub c_ospeed: c_ulong,
    }

    impl Default for termios {
        fn default() -> Self {
            termios {
                c_iflag: 0,
                c_oflag: 0,
                c_cflag: 0,
                c_lflag: 0,
                c_line: 0,
                c_cc: [0; 32],
                c_ispeed: 0,
                c_ospeed: 0,
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct winsize {
        pub ws_row: c_ushort,
        pub ws_col: c_ushort,
        pub ws_xpixel: c_ushort,
        pub ws_ypixel: c_ushort,
    }

    extern "C" {
        pub fn tcgetattr(fd: c_int, termios_p: *mut termios) -> c_int;
        pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios_p: *const termios) -> c_int;
        pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }
}

impl Terminal {
    /// Create a new terminal handler
    pub fn new() -> io::Result<Self> {
        let mut term = Terminal {
            original_termios: None,
            size: TermSize::default(),
            output_buffer: String::with_capacity(8192),
        };

        term.update_size();

        Ok(term)
    }

    /// Enable raw mode for the terminal
    pub fn enable_raw_mode(&mut self) -> io::Result<()> {
        let mut termios = libc::termios::default();

        unsafe {
            if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        self.original_termios = Some(termios);

        // Modify flags for raw mode
        termios.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        termios.c_oflag &= !libc::OPOST;
        termios.c_cflag |= libc::CS8;
        termios.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG);

        termios.c_cc[libc::VMIN] = 0; // Return immediately
        termios.c_cc[libc::VTIME] = 1; // 100ms timeout

        unsafe {
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &termios) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(())
    }

    /// Disable raw mode (restore original settings)
    pub fn disable_raw_mode(&mut self) -> io::Result<()> {
        if let Some(ref termios) = self.original_termios {
            unsafe {
                if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, termios) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
        }
        Ok(())
    }

    /// Update terminal size
    pub fn update_size(&mut self) {
        let mut ws = libc::winsize::default();

        unsafe {
            if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 {
                if ws.ws_row > 0 && ws.ws_col > 0 {
                    self.size.rows = ws.ws_row as usize;
                    self.size.cols = ws.ws_col as usize;
                }
            }
        }
    }

    /// Get terminal size
    pub fn get_size(&self) -> TermSize {
        self.size
    }

    /// Enter alternate screen buffer
    pub fn enter_alt_screen(&mut self) {
        self.output_buffer.push_str(ansi::ALT_SCREEN_ON);
    }

    /// Leave alternate screen buffer
    pub fn leave_alt_screen(&mut self) {
        self.output_buffer.push_str(ansi::ALT_SCREEN_OFF);
    }

    /// Hide cursor
    pub fn hide_cursor(&mut self) {
        self.output_buffer.push_str(ansi::CURSOR_HIDE);
    }

    /// Show cursor
    pub fn show_cursor(&mut self) {
        self.output_buffer.push_str(ansi::CURSOR_SHOW);
    }

    /// Move cursor to position (1-indexed)
    pub fn move_cursor(&mut self, row: usize, col: usize) {
        use std::fmt::Write;
        let _ = write!(self.output_buffer, "\x1b[{};{}H", row, col);
    }

    /// Clear the screen
    pub fn clear_screen(&mut self) {
        self.output_buffer.push_str(ansi::CLEAR_SCREEN);
    }

    /// Clear current line
    pub fn clear_line(&mut self) {
        self.output_buffer.push_str(ansi::CLEAR_LINE);
    }

    /// Clear from cursor to end of line
    pub fn clear_to_eol(&mut self) {
        self.output_buffer.push_str(ansi::CLEAR_LINE_FROM_CURSOR);
    }

    /// Set text color
    pub fn set_fg_color(&mut self, color: Color) {
        use std::fmt::Write;
        match color {
            Color::Default => self.output_buffer.push_str(ansi::FG_DEFAULT),
            Color::Black => self.output_buffer.push_str(ansi::FG_BLACK),
            Color::Red => self.output_buffer.push_str(ansi::FG_RED),
            Color::Green => self.output_buffer.push_str(ansi::FG_GREEN),
            Color::Yellow => self.output_buffer.push_str(ansi::FG_YELLOW),
            Color::Blue => self.output_buffer.push_str(ansi::FG_BLUE),
            Color::Magenta => self.output_buffer.push_str(ansi::FG_MAGENTA),
            Color::Cyan => self.output_buffer.push_str(ansi::FG_CYAN),
            Color::White => self.output_buffer.push_str(ansi::FG_WHITE),
            Color::Rgb(r, g, b) => {
                let _ = write!(self.output_buffer, "\x1b[38;2;{};{};{}m", r, g, b);
            }
            Color::Indexed(n) => {
                let _ = write!(self.output_buffer, "\x1b[38;5;{}m", n);
            }
        }
    }

    /// Set background color
    pub fn set_bg_color(&mut self, color: Color) {
        use std::fmt::Write;
        match color {
            Color::Default => self.output_buffer.push_str(ansi::BG_DEFAULT),
            Color::Black => self.output_buffer.push_str(ansi::BG_BLACK),
            Color::Red => self.output_buffer.push_str(ansi::BG_RED),
            Color::Green => self.output_buffer.push_str(ansi::BG_GREEN),
            Color::Yellow => self.output_buffer.push_str(ansi::BG_YELLOW),
            Color::Blue => self.output_buffer.push_str(ansi::BG_BLUE),
            Color::Magenta => self.output_buffer.push_str(ansi::BG_MAGENTA),
            Color::Cyan => self.output_buffer.push_str(ansi::BG_CYAN),
            Color::White => self.output_buffer.push_str(ansi::BG_WHITE),
            Color::Rgb(r, g, b) => {
                let _ = write!(self.output_buffer, "\x1b[48;2;{};{};{}m", r, g, b);
            }
            Color::Indexed(n) => {
                let _ = write!(self.output_buffer, "\x1b[48;5;{}m", n);
            }
        }
    }

    /// Reset text attributes
    pub fn reset_style(&mut self) {
        self.output_buffer.push_str(ansi::RESET);
    }

    /// Set bold attribute
    pub fn set_bold(&mut self) {
        self.output_buffer.push_str(ansi::BOLD);
    }

    /// Set reverse video attribute
    pub fn set_reverse(&mut self) {
        self.output_buffer.push_str(ansi::REVERSE);
    }

    /// Write a string to the output buffer
    pub fn write_str(&mut self, s: &str) {
        self.output_buffer.push_str(s);
    }

    /// Write a character to the output buffer
    pub fn write_char(&mut self, c: char) {
        self.output_buffer.push(c);
    }

    /// Flush the output buffer to stdout
    pub fn flush(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        stdout.write_all(self.output_buffer.as_bytes())?;
        stdout.flush()?;
        self.output_buffer.clear();
        Ok(())
    }

    /// Read a single byte from stdin (non-blocking)
    pub fn read_byte(&self) -> io::Result<Option<u8>> {
        let mut buf = [0u8; 1];
        let mut stdin = io::stdin();

        match stdin.read(&mut buf) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(buf[0])),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read bytes from stdin with timeout
    pub fn read_bytes(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut stdin = io::stdin();
        stdin.read(buf)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.disable_raw_mode();
    }
}

/// Terminal color
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Rgb(u8, u8, u8),
    Indexed(u8),
}
