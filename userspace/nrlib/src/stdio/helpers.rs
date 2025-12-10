//! Helper functions for stdio module
//!
//! Math utilities and number formatting helpers.

use crate::{get_errno, translate_ret_isize};
use core::arch::asm;

use super::constants::{INT_BUFFER_SIZE, SYS_READ, SYS_WRITE};

/// Calculate 10^n for precision scaling
pub(crate) fn pow10(n: usize) -> u128 {
    let mut result = 1u128;
    for _ in 0..n {
        result *= 10;
    }
    result
}

/// Truncate a floating point number toward zero
pub(crate) fn trunc_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    if x >= 0.0 {
        (x as i64) as f64
    } else {
        -(((-x) as i64) as f64)
    }
}

/// Round a floating point number to nearest integer
pub(crate) fn round_f64(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let truncated = trunc_f64(x);
    let frac = x - truncated;
    if frac.abs() >= 0.5 {
        if x >= 0.0 {
            truncated + 1.0
        } else {
            truncated - 1.0
        }
    } else {
        truncated
    }
}

/// Format an unsigned integer to a buffer
pub(crate) fn format_usize(val: usize, buf: &mut [u8]) -> &str {
    if val == 0 {
        return "0";
    }
    let mut n = val;
    let mut idx = buf.len();
    while n > 0 && idx > 0 {
        idx -= 1;
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    unsafe { core::str::from_utf8_unchecked(&buf[idx..]) }
}

/// Format a signed integer to a buffer
pub(crate) fn format_isize(val: isize, buf: &mut [u8]) -> &str {
    if val == 0 {
        return "0";
    }
    let neg = val < 0;
    let mut n = if neg {
        (val as i64).abs() as usize
    } else {
        val as usize
    };
    let mut idx = buf.len();
    while n > 0 && idx > 0 {
        idx -= 1;
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if neg && idx > 0 {
        idx -= 1;
        buf[idx] = b'-';
    }
    unsafe { core::str::from_utf8_unchecked(&buf[idx..]) }
}

/// Format unsigned integer with specified base and minimum digits
pub(crate) fn format_unsigned(
    mut value: u128,
    base: u32,
    min_digits: usize,
) -> ([u8; INT_BUFFER_SIZE], usize) {
    let mut buf = [0u8; INT_BUFFER_SIZE];
    let mut index = INT_BUFFER_SIZE;
    if value == 0 {
        index -= 1;
        buf[index] = b'0';
    } else {
        while value > 0 {
            let digit = (value % base as u128) as u8;
            let ch = if digit < 10 {
                b'0' + digit
            } else {
                b'a' + (digit - 10)
            };
            index -= 1;
            buf[index] = ch;
            value /= base as u128;
        }
    }
    let digits = INT_BUFFER_SIZE - index;
    if min_digits > digits {
        let zeros = min_digits - digits;
        for _ in 0..zeros {
            index -= 1;
            buf[index] = b'0';
        }
    }
    (buf, index)
}

/// Format signed integer with specified base and minimum digits
pub(crate) fn format_signed(
    value: i128,
    base: u32,
    min_digits: usize,
) -> (bool, [u8; INT_BUFFER_SIZE], usize) {
    if value < 0 {
        let unsigned = value.wrapping_neg() as u128;
        let (buf, index) = format_unsigned(unsigned, base, min_digits);
        (true, buf, index)
    } else {
        let (buf, index) = format_unsigned(value as u128, base, min_digits);
        (false, buf, index)
    }
}

// ============================================================================
// Low-level syscall wrappers
// ============================================================================

#[inline(always)]
pub(crate) fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64"),
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub(crate) fn write_fd(fd: i32, buf: *const u8, len: usize) -> isize {
    translate_ret_isize(syscall3(SYS_WRITE, fd as u64, buf as u64, len as u64))
}

#[inline(always)]
pub(crate) fn read_fd(fd: i32, buf: *mut u8, len: usize) -> isize {
    translate_ret_isize(syscall3(SYS_READ, fd as u64, buf as u64, len as u64))
}

/// Write all bytes to a file descriptor, handling partial writes
pub(crate) fn write_all_fd(fd: i32, mut buf: &[u8]) -> Result<(), i32> {
    while !buf.is_empty() {
        let written = write_fd(fd, buf.as_ptr(), buf.len());
        if written < 0 {
            return Err(get_errno());
        }
        if written == 0 {
            return Err(get_errno());
        }
        buf = &buf[written as usize..];
    }
    Ok(())
}

/// Debug logging using direct syscall (bypasses stdio locking)
pub(crate) fn debug_log(msg: &[u8]) {
    let _ = syscall3(SYS_WRITE, 2, msg.as_ptr() as u64, msg.len() as u64);
}
