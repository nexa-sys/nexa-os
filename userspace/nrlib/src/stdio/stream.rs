//! Stream operations for stdio
//!
//! High-level stream read/write/flush operations and line reading utilities.

use core::fmt::{self, Write};

use crate::{get_errno, set_errno};

use super::file::{
    file_flush, file_read_byte, file_write_bytes, lock_stream, stderr, stdin, stdout, FILE,
};
use super::helpers::{debug_log, format_isize, format_usize};
use crate::EINVAL;

// ============================================================================
// Stream-level operations (with locking)
// ============================================================================

/// Write bytes to a stream (with locking)
pub(crate) fn write_stream_bytes(stream: *mut FILE, bytes: &[u8]) -> Result<(), i32> {
    unsafe {
        let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
        let file = guard.file_mut();
        if file.error {
            set_errno(EINVAL);
            return Err(EINVAL);
        }
        file_write_bytes(file, bytes)
    }
}

/// Flush a stream (with locking)
pub(crate) fn flush_stream(stream: *mut FILE) -> Result<(), i32> {
    unsafe {
        let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
        let file = guard.file_mut();
        file_flush(file)
    }
}

/// Read a single byte from a stream (with locking)
pub(crate) fn read_stream_byte(stream: *mut FILE) -> Result<Option<u8>, i32> {
    unsafe {
        let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
        let file = guard.file_mut();
        file_read_byte(file)
    }
}

// ============================================================================
// Echo and line reading utilities
// ============================================================================

/// Echo bytes to stdout
fn echo_bytes(bytes: &[u8]) {
    let _ = write_stream_bytes(unsafe { stdout }, bytes);
}

/// Read a byte from stdin, blocking until available
fn read_blocking_byte() -> Result<u8, i32> {
    loop {
        match read_stream_byte(unsafe { stdin }) {
            Ok(Some(b)) => return Ok(b),
            Ok(None) => return Err(0),
            Err(err) => {
                if err == 0 || err == 4 || err == 11 {
                    continue;
                }
                return Err(err);
            }
        }
    }
}

/// Echo mode for line reading
pub(crate) enum EchoMode {
    /// No echo
    None,
    /// Echo characters as typed
    Plain,
    /// Echo with a mask character (e.g., for passwords)
    Mask(u8),
}

/// Internal line reading with configurable echo
pub(crate) fn read_line_internal(buf: &mut [u8], mode: EchoMode, skip_empty: bool) -> Result<usize, i32> {
    if buf.is_empty() {
        return Ok(0);
    }

    let max = buf.len().saturating_sub(1);
    let mut len = 0usize;

    loop {
        let byte = match read_blocking_byte() {
            Ok(b) => b,
            Err(err) => return Err(err),
        };

        match byte {
            b'\r' | b'\n' => {
                if len == 0 && skip_empty {
                    continue;
                }
                echo_bytes(b"\n");
                break;
            }
            8 | 127 => {
                // Backspace or DEL
                if len > 0 {
                    len -= 1;
                    buf[len] = 0;
                    if !matches!(mode, EchoMode::None) {
                        echo_bytes(b"\x08 \x08");
                    }
                }
            }
            b if (0x20..=0x7e).contains(&b) => {
                // Printable ASCII
                if len < max {
                    buf[len] = b;
                    len += 1;
                    match mode {
                        EchoMode::Plain => echo_bytes(&[b]),
                        EchoMode::Mask(mask) => echo_bytes(&[mask]),
                        EchoMode::None => {}
                    }
                }
            }
            _ => {}
        }
    }

    buf[len] = 0;
    Ok(len)
}

// ============================================================================
// Public stdout helpers
// ============================================================================

/// Write all bytes to stdout
pub fn stdout_write_all(buf: &[u8]) -> Result<(), i32> {
    write_stream_bytes(unsafe { stdout }, buf)
}

/// Write a string to stdout
pub fn stdout_write_str(s: &str) -> Result<(), i32> {
    stdout_write_all(s.as_bytes())
}

/// Write formatted output to stdout using Rust's fmt machinery
pub fn stdout_write_fmt(args: fmt::Arguments<'_>) -> Result<(), i32> {
    debug_log(b"[nrlib] stdout_write_fmt enter\n");
    struct StdoutWriter {
        error: Option<i32>,
    }

    impl Write for StdoutWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match stdout_write_str(s) {
                Ok(()) => Ok(()),
                Err(err) => {
                    self.error = Some(err);
                    Err(fmt::Error)
                }
            }
        }
    }

    let mut writer = StdoutWriter { error: None };
    let result = match writer.write_fmt(args) {
        Ok(()) => Ok(()),
        Err(_) => Err(writer.error.unwrap_or_else(get_errno)),
    };
    match result {
        Ok(()) => debug_log(b"[nrlib] stdout_write_fmt ok\n"),
        Err(_) => debug_log(b"[nrlib] stdout_write_fmt err\n"),
    }
    result
}

/// Flush stdout
pub fn stdout_flush() -> Result<(), i32> {
    flush_stream(unsafe { stdout })
}

// ============================================================================
// Public stderr helpers
// ============================================================================

/// Write all bytes to stderr
pub fn stderr_write_all(buf: &[u8]) -> Result<(), i32> {
    write_stream_bytes(unsafe { stderr }, buf)
}

/// Write a string to stderr
pub fn stderr_write_str(s: &str) -> Result<(), i32> {
    stderr_write_all(s.as_bytes())
}

/// Write an unsigned integer to stderr
pub fn stderr_write_usize(val: usize) -> Result<(), i32> {
    let mut buf = [0u8; 20];
    let s = format_usize(val, &mut buf);
    stderr_write_str(s)
}

/// Write a signed integer to stderr
pub fn stderr_write_isize(val: isize) -> Result<(), i32> {
    let mut buf = [0u8; 21];
    let s = format_isize(val, &mut buf);
    stderr_write_str(s)
}

/// Write an i32 to stderr
pub fn stderr_write_i32(val: i32) -> Result<(), i32> {
    stderr_write_isize(val as isize)
}

// ============================================================================
// Public stdin helpers
// ============================================================================

/// Read a line from stdin with echo
pub fn stdin_read_line(buf: &mut [u8], skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::Plain, skip_empty)
}

/// Read a line from stdin with masked echo (for passwords)
pub fn stdin_read_line_masked(buf: &mut [u8], mask: u8, skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::Mask(mask), skip_empty)
}

/// Read a line from stdin without echo
pub fn stdin_read_line_noecho(buf: &mut [u8], skip_empty: bool) -> Result<usize, i32> {
    read_line_internal(buf, EchoMode::None, skip_empty)
}
