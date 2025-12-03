//! Public C API functions for stdio
//!
//! This module provides the libc-compatible C API functions exported for userspace programs.

use core::{ffi::c_void, ptr, slice};

use crate::{set_errno, EINVAL};

use super::file::{
    file_read_byte, file_read_bytes, file_write_bytes, lock_stream,
    stderr, stdin, stdout, FILE,
};
use super::format::write_formatted;
use super::helpers::debug_log;
use super::stream::{flush_stream, read_stream_byte, write_stream_bytes};

// ============================================================================
// Character output functions
// ============================================================================

/// Write a character to stdout (libc compatible)
#[no_mangle]
pub extern "C" fn putchar(c: i32) -> i32 {
    let byte = c as u8;
    match write_stream_bytes(unsafe { stdout }, &[byte]) {
        Ok(()) => c,
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

/// Write a character to a stream (libc compatible)
#[no_mangle]
pub extern "C" fn fputc(c: i32, stream: *mut FILE) -> i32 {
    match write_stream_bytes(stream, &[c as u8]) {
        Ok(()) => c,
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

/// Write a string to stdout followed by newline (libc compatible)
#[no_mangle]
pub extern "C" fn puts(s: *const u8) -> i32 {
    if s.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    unsafe {
        let mut len = 0usize;
        while ptr::read(s.add(len)) != 0 {
            len += 1;
        }
        let slice = slice::from_raw_parts(s, len);
        if write_stream_bytes(stdout, slice).is_err()
            || write_stream_bytes(stdout, b"\n").is_err()
        {
            return -1;
        }
    }
    1
}

/// Write a string to a stream (libc compatible)
#[no_mangle]
pub extern "C" fn fputs(s: *const u8, stream: *mut FILE) -> i32 {
    if s.is_null() {
        set_errno(EINVAL);
        return -1;
    }
    unsafe {
        let mut len = 0usize;
        while ptr::read(s.add(len)) != 0 {
            len += 1;
        }
        let slice = slice::from_raw_parts(s, len);
        if write_stream_bytes(stream, slice).is_err() {
            return -1;
        }
    }
    0
}

// ============================================================================
// Character input functions
// ============================================================================

/// Read a character from stdin (libc compatible)
#[no_mangle]
pub extern "C" fn getchar() -> i32 {
    match read_stream_byte(unsafe { stdin }) {
        Ok(Some(byte)) => byte as i32,
        Ok(None) => {
            set_errno(0);
            -1
        }
        Err(err) => {
            set_errno(err);
            -1
        }
    }
}

/// Read a line from a stream (libc compatible)
#[no_mangle]
pub extern "C" fn fgets(buf: *mut u8, size: i32, stream: *mut FILE) -> *mut u8 {
    if buf.is_null() || size <= 1 {
        return ptr::null_mut();
    }

    unsafe {
        let mut guard = match lock_stream(stream) {
            Ok(g) => g,
            Err(()) => return ptr::null_mut(),
        };
        let file = guard.file_mut();
        let mut i = 0usize;
        while i < (size - 1) as usize {
            match file_read_byte(file) {
                Ok(Some(b)) => {
                    ptr::write(buf.add(i), b);
                    i += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    set_errno(err);
                    return ptr::null_mut();
                }
            }
        }
        ptr::write(buf.add(i), 0);
        if i == 0 {
            ptr::null_mut()
        } else {
            buf
        }
    }
}

/// Read a line from stdin (UNSAFE - no buffer size limit, deprecated)
#[no_mangle]
pub unsafe extern "C" fn gets(buf: *mut u8) -> *mut u8 {
    if buf.is_null() {
        return ptr::null_mut();
    }
    let mut i = 0usize;
    loop {
        let c = getchar();
        if c <= 0 {
            break;
        }
        if c as u8 == b'\n' {
            break;
        }
        ptr::write(buf.add(i), c as u8);
        i += 1;
    }
    ptr::write(buf.add(i), 0);
    if i == 0 {
        ptr::null_mut()
    } else {
        buf
    }
}

// ============================================================================
// Formatted output functions
// ============================================================================

/// Formatted print to stdout (libc compatible)
#[no_mangle]
pub unsafe extern "C" fn printf(fmt: *const u8, mut args: ...) -> i32 {
    match write_formatted(stdout, fmt, &mut args) {
        Ok(count) => count,
        Err(_) => -1,
    }
}

/// Formatted print to a stream (libc compatible)
#[no_mangle]
pub unsafe extern "C" fn fprintf(stream: *mut FILE, fmt: *const u8, mut args: ...) -> i32 {
    match write_formatted(stream, fmt, &mut args) {
        Ok(count) => count,
        Err(_) => -1,
    }
}

// ============================================================================
// Buffer flush functions
// ============================================================================

/// Flush a stream or all streams (libc compatible)
#[no_mangle]
pub unsafe extern "C" fn fflush(stream: *mut FILE) -> i32 {
    if stream.is_null() {
        let mut result = 0;
        let mut error_code = 0;
        debug_log(b"[nrlib] fflush(NULL) -> stdout\n");
        if let Err(err) = flush_stream(stdout) {
            result = -1;
            error_code = err;
            debug_log(b"[nrlib] fflush stdout error\n");
        } else {
            debug_log(b"[nrlib] fflush stdout ok\n");
        }
        debug_log(b"[nrlib] fflush(NULL) -> stderr\n");
        if let Err(err) = flush_stream(stderr) {
            result = -1;
            if error_code == 0 {
                error_code = err;
            }
            debug_log(b"[nrlib] fflush stderr error\n");
        } else {
            debug_log(b"[nrlib] fflush stderr ok\n");
        }
        if result == 0 {
            set_errno(0);
            debug_log(b"[nrlib] fflush(NULL) success\n");
        } else {
            set_errno(error_code);
            debug_log(b"[nrlib] fflush(NULL) failure\n");
        }
        return result;
    }

    debug_log(b"[nrlib] fflush(stream)\n");
    match flush_stream(stream) {
        Ok(()) => {
            set_errno(0);
            debug_log(b"[nrlib] fflush(stream) success\n");
            0
        }
        Err(err) => {
            set_errno(err);
            debug_log(b"[nrlib] fflush(stream) failure\n");
            -1
        }
    }
}

// ============================================================================
// Block I/O functions
// ============================================================================

/// Write blocks to a stream (libc compatible)
#[no_mangle]
pub unsafe extern "C" fn fwrite(
    ptr: *const c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if size == 0 || nmemb == 0 {
        return 0;
    }
    if ptr.is_null() {
        set_errno(EINVAL);
        return 0;
    }

    let total = match size.checked_mul(nmemb) {
        Some(v) => v,
        None => {
            set_errno(EINVAL);
            return 0;
        }
    };

    set_errno(0);
    let mut guard = match lock_stream(stream) {
        Ok(g) => g,
        Err(()) => return 0,
    };
    let file = guard.file_mut();

    let data = slice::from_raw_parts(ptr as *const u8, total);
    let mut written = 0usize;

    while written < total {
        match file_write_bytes(file, &data[written..]) {
            Ok(()) => {
                written = total;
            }
            Err(err) => {
                set_errno(err);
                break;
            }
        }
    }

    written / size
}

/// Read blocks from a stream (libc compatible)
#[no_mangle]
pub unsafe extern "C" fn fread(
    ptr: *mut c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if size == 0 || nmemb == 0 {
        return 0;
    }
    if ptr.is_null() {
        set_errno(EINVAL);
        return 0;
    }

    let total = match size.checked_mul(nmemb) {
        Some(v) => v,
        None => {
            set_errno(EINVAL);
            return 0;
        }
    };

    set_errno(0);
    let mut guard = match lock_stream(stream) {
        Ok(g) => g,
        Err(()) => return 0,
    };
    let file = guard.file_mut();

    let mut read_total = 0usize;
    while read_total < total {
        let buffer = slice::from_raw_parts_mut((ptr as *mut u8).add(read_total), total - read_total);
        match file_read_bytes(file, buffer) {
            Ok(0) => break,
            Ok(n) => read_total += n,
            Err(err) => {
                set_errno(err);
                break;
            }
        }
    }

    read_total / size
}

// ============================================================================
// File descriptor functions
// ============================================================================

/// Get file descriptor from FILE* - CRITICAL for Rust std::io initialization
/// Rust std calls fileno(stdout) to check if stdout is a terminal with isatty()
/// without fileno, Rust std's stdout initialization hangs or fails
#[no_mangle]
pub extern "C" fn fileno(stream: *mut FILE) -> i32 {
    if stream.is_null() {
        set_errno(EINVAL);
        return -1;
    }

    unsafe {
        let file = &*stream;
        file.fd
    }
}
