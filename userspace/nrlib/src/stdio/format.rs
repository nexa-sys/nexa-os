//! Printf format string parsing and formatting
//!
//! This module implements printf-style formatted output with support for:
//! - Width and precision specifiers (including * for dynamic values)
//! - Length modifiers (h, hh, l, ll, z, t, j)
//! - Format flags (-, +, space, #, 0)
//! - Integer formats (d, i, u, x, X, o, p)
//! - Floating point formats (f, F)
//! - String and character formats (s, c)

use core::{
    cmp,
    ffi::{c_void, VaListImpl},
    ptr, slice,
};

use crate::{get_errno, set_errno, EINVAL};

use super::constants::{
    DEFAULT_FLOAT_PRECISION, FLAG_ALT, FLAG_LEFT, FLAG_PLUS, FLAG_SPACE, FLAG_ZERO,
    FLOAT_BUFFER_SIZE, INT_BUFFER_SIZE, MAX_FLOAT_PRECISION,
};
use super::file::{file_write_byte, file_write_bytes, lock_stream, write_repeat, FILE};
use super::helpers::{format_unsigned, pow10, round_f64};

/// Length modifier for format specifiers
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LengthModifier {
    None,
    HH,
    H,
    L,
    LL,
    Z,
    T,
    J,
}

impl Default for LengthModifier {
    fn default() -> Self {
        Self::None
    }
}

/// Parsed format specifier
#[derive(Default)]
pub(crate) struct FormatSpec {
    pub(crate) flags: u8,
    pub(crate) width: Option<usize>,
    pub(crate) precision: Option<usize>,
    pub(crate) length: LengthModifier,
    pub(crate) specifier: u8,
}

/// Parse length modifier from format string
pub(crate) fn parse_length(fmt: &[u8], idx: &mut usize) -> LengthModifier {
    if *idx >= fmt.len() {
        return LengthModifier::None;
    }
    match fmt[*idx] {
        b'h' => {
            if *idx + 1 < fmt.len() && fmt[*idx + 1] == b'h' {
                *idx += 2;
                LengthModifier::HH
            } else {
                *idx += 1;
                LengthModifier::H
            }
        }
        b'l' => {
            if *idx + 1 < fmt.len() && fmt[*idx + 1] == b'l' {
                *idx += 2;
                LengthModifier::LL
            } else {
                *idx += 1;
                LengthModifier::L
            }
        }
        b'z' => {
            *idx += 1;
            LengthModifier::Z
        }
        b't' => {
            *idx += 1;
            LengthModifier::T
        }
        b'j' => {
            *idx += 1;
            LengthModifier::J
        }
        _ => LengthModifier::None,
    }
}

/// Read a signed integer argument based on length modifier
pub(crate) fn read_signed_arg(args: &mut VaListImpl<'_>, length: LengthModifier) -> i128 {
    unsafe {
        match length {
            LengthModifier::HH => args.arg::<i32>() as i8 as i128,
            LengthModifier::H => args.arg::<i32>() as i16 as i128,
            LengthModifier::L | LengthModifier::LL => args.arg::<i64>() as i128,
            LengthModifier::Z | LengthModifier::T => args.arg::<isize>() as i128,
            LengthModifier::J => args.arg::<i64>() as i128,
            LengthModifier::None => args.arg::<i32>() as i128,
        }
    }
}

/// Read an unsigned integer argument based on length modifier
pub(crate) fn read_unsigned_arg(args: &mut VaListImpl<'_>, length: LengthModifier) -> u128 {
    unsafe {
        match length {
            LengthModifier::HH => args.arg::<u32>() as u8 as u128,
            LengthModifier::H => args.arg::<u32>() as u16 as u128,
            LengthModifier::L | LengthModifier::LL => args.arg::<u64>() as u128,
            LengthModifier::Z | LengthModifier::T => args.arg::<usize>() as u128,
            LengthModifier::J => args.arg::<u64>() as u128,
            LengthModifier::None => args.arg::<u32>() as u128,
        }
    }
}

/// Emit a formatted integer with proper padding and prefixes
pub(crate) fn emit_formatted_integer(
    file: &mut FILE,
    spec: &FormatSpec,
    negative: bool,
    digits: &[u8],
    uppercase: bool,
    value_is_zero: bool,
) -> Result<usize, i32> {
    let sign_char = if negative {
        Some(b'-')
    } else if spec.flags & FLAG_PLUS != 0 {
        Some(b'+')
    } else if spec.flags & FLAG_SPACE != 0 {
        Some(b' ')
    } else {
        None
    };

    let mut precision_buf = [0u8; INT_BUFFER_SIZE];
    let mut digits_prec_slice = digits;
    if let Some(mut precision) = spec.precision {
        if precision > INT_BUFFER_SIZE {
            precision = INT_BUFFER_SIZE;
        }
        if precision == 0 && digits.len() == 1 && digits[0] == b'0' && spec.specifier != b'p' {
            digits_prec_slice = &[];
        } else if precision > digits.len() {
            let start = INT_BUFFER_SIZE - precision;
            let zero_count = precision - digits.len();
            for idx in 0..zero_count {
                precision_buf[start + idx] = b'0';
            }
            precision_buf[start + zero_count..start + precision].copy_from_slice(digits);
            digits_prec_slice = &precision_buf[start..start + precision];
        }
    }

    let mut uppercase_buf = [0u8; INT_BUFFER_SIZE];
    let digits_case = if uppercase {
        let start = INT_BUFFER_SIZE - digits_prec_slice.len();
        uppercase_buf[start..start + digits_prec_slice.len()].copy_from_slice(digits_prec_slice);
        for byte in &mut uppercase_buf[start..start + digits_prec_slice.len()] {
            byte.make_ascii_uppercase();
        }
        &uppercase_buf[start..start + digits_prec_slice.len()]
    } else {
        digits_prec_slice
    };

    let mut prefix: &[u8] = b"";
    if spec.flags & FLAG_ALT != 0 {
        match spec.specifier {
            b'x' if !value_is_zero && !digits_case.is_empty() => prefix = b"0x",
            b'X' if !value_is_zero && !digits_case.is_empty() => prefix = b"0X",
            b'o' => {
                if digits_case.is_empty() || digits_case[0] != b'0' {
                    prefix = b"0";
                }
            }
            _ => {}
        }
    } else if spec.specifier == b'p' {
        prefix = b"0x";
    }

    write_formatted_block(file, spec, sign_char, prefix, digits_case)
}

/// Write a formatted block with proper padding
pub(crate) fn write_formatted_block(
    file: &mut FILE,
    spec: &FormatSpec,
    sign: Option<u8>,
    prefix: &[u8],
    body: &[u8],
) -> Result<usize, i32> {
    let sign_len = sign.map(|_| 1).unwrap_or(0);
    let total_len = sign_len + prefix.len() + body.len();
    let width = spec.width.unwrap_or(0);
    let left = spec.flags & FLAG_LEFT != 0;
    let zero_allowed = spec.flags & FLAG_ZERO != 0 && !left && spec.precision.is_none();
    let pad_char = if zero_allowed
        && matches!(
            spec.specifier,
            b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'p'
        ) {
        b'0'
    } else {
        b' '
    };
    let padding = width.saturating_sub(total_len);

    if !left {
        if pad_char == b' ' {
            write_repeat(file, b' ', padding)?;
        }
        if let Some(ch) = sign {
            file_write_byte(file, ch)?;
        }
        if !prefix.is_empty() {
            file_write_bytes(file, prefix)?;
        }
        if pad_char == b'0' {
            write_repeat(file, b'0', padding)?;
        }
        if !body.is_empty() {
            file_write_bytes(file, body)?;
        }
    } else {
        if let Some(ch) = sign {
            file_write_byte(file, ch)?;
        }
        if !prefix.is_empty() {
            file_write_bytes(file, prefix)?;
        }
        if !body.is_empty() {
            file_write_bytes(file, body)?;
        }
        if padding > 0 {
            write_repeat(file, b' ', padding)?;
        }
    }

    Ok(total_len + padding)
}

/// Handle floating point format specifiers (f, F)
pub(crate) fn handle_float(
    spec: &FormatSpec,
    file: &mut FILE,
    value: f64,
    uppercase: bool,
) -> Result<usize, i32> {
    if value.is_nan() {
        let txt = if uppercase { b"NAN" } else { b"nan" };
        return write_formatted_block(file, spec, None, b"", txt);
    }
    if value.is_infinite() {
        let sign = if value.is_sign_negative() {
            Some(b'-')
        } else if spec.flags & FLAG_PLUS != 0 {
            Some(b'+')
        } else if spec.flags & FLAG_SPACE != 0 {
            Some(b' ')
        } else {
            None
        };
        let txt = if uppercase { b"INF" } else { b"inf" };
        return write_formatted_block(file, spec, sign, b"", txt);
    }

    let precision = spec
        .precision
        .unwrap_or(DEFAULT_FLOAT_PRECISION)
        .min(MAX_FLOAT_PRECISION);
    let decimal_point = precision > 0 || (spec.flags & FLAG_ALT != 0);

    let negative = value.is_sign_negative();
    let abs_value = if negative { -value } else { value };

    let scale = pow10(precision);
    let scaled = round_f64(abs_value * scale as f64).min(u128::MAX as f64);
    let scaled_u128 = scaled as u128;

    let int_part = scaled_u128 / scale;
    let frac_part = scaled_u128 % scale;

    let (int_buf, int_idx) = format_unsigned(int_part, 10, 1);
    let integer_digits = &int_buf[int_idx..];

    let mut float_buf = [0u8; FLOAT_BUFFER_SIZE];
    let mut cursor = 0usize;
    float_buf[cursor..cursor + integer_digits.len()].copy_from_slice(integer_digits);
    cursor += integer_digits.len();

    if decimal_point {
        float_buf[cursor] = b'.';
        cursor += 1;
        if precision > 0 {
            let (frac_buf, _) = format_unsigned(frac_part, 10, precision.max(1));
            let start = INT_BUFFER_SIZE - precision;
            float_buf[cursor..cursor + precision]
                .copy_from_slice(&frac_buf[start..start + precision]);
            cursor += precision;
        }
    }

    let mut body_slice = &float_buf[..cursor];
    let mut upper_buf = [0u8; FLOAT_BUFFER_SIZE];
    if uppercase {
        upper_buf[..body_slice.len()].copy_from_slice(body_slice);
        for byte in &mut upper_buf[..body_slice.len()] {
            byte.make_ascii_uppercase();
        }
        body_slice = &upper_buf[..body_slice.len()];
    }

    let sign = if negative {
        Some(b'-')
    } else if spec.flags & FLAG_PLUS != 0 {
        Some(b'+')
    } else if spec.flags & FLAG_SPACE != 0 {
        Some(b' ')
    } else {
        None
    };

    write_formatted_block(file, spec, sign, b"", body_slice)
}

/// Main printf implementation - parse format string and write formatted output
pub(crate) unsafe fn write_formatted(
    stream: *mut FILE,
    fmt_ptr: *const u8,
    args: &mut VaListImpl<'_>,
) -> Result<i32, i32> {
    use super::helpers::format_signed;

    if fmt_ptr.is_null() {
        set_errno(EINVAL);
        return Err(EINVAL);
    }

    let len = {
        let mut idx = 0usize;
        while ptr::read(fmt_ptr.add(idx)) != 0 {
            idx += 1;
        }
        idx
    };

    let fmt = slice::from_raw_parts(fmt_ptr, len);
    let mut total_written = 0i32;

    let mut guard = lock_stream(stream).map_err(|_| get_errno())?;
    let file = guard.file_mut();

    let mut i = 0usize;
    while i < fmt.len() {
        let ch = fmt[i];
        if ch != b'%' {
            file_write_byte(file, ch)?;
            total_written += 1;
            i += 1;
            continue;
        }

        i += 1;
        if i >= fmt.len() {
            break;
        }

        let mut spec = FormatSpec::default();

        // Parse flags
        while i < fmt.len() {
            let flag = fmt[i];
            let recognized = match flag {
                b'-' => {
                    spec.flags |= FLAG_LEFT;
                    true
                }
                b'+' => {
                    spec.flags |= FLAG_PLUS;
                    true
                }
                b' ' => {
                    spec.flags |= FLAG_SPACE;
                    true
                }
                b'#' => {
                    spec.flags |= FLAG_ALT;
                    true
                }
                b'0' => {
                    spec.flags |= FLAG_ZERO;
                    true
                }
                _ => false,
            };
            if recognized {
                i += 1;
            } else {
                break;
            }
        }

        // Parse width
        if i < fmt.len() && fmt[i] == b'*' {
            i += 1;
            let w: i32 = args.arg();
            if w < 0 {
                spec.flags |= FLAG_LEFT;
                spec.width = Some((-w) as usize);
            } else {
                spec.width = Some(w as usize);
            }
        } else {
            let mut width: usize = 0;
            let mut has_digit = false;
            while i < fmt.len() {
                let ch = fmt[i];
                if !(b'0'..=b'9').contains(&ch) {
                    break;
                }
                has_digit = true;
                width = width
                    .saturating_mul(10)
                    .saturating_add((ch - b'0') as usize);
                i += 1;
            }
            if has_digit {
                spec.width = Some(width);
            }
        }

        // Parse precision
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            if i < fmt.len() && fmt[i] == b'*' {
                i += 1;
                let p: i32 = args.arg();
                if p >= 0 {
                    spec.precision = Some(p as usize);
                } else {
                    spec.precision = None;
                }
            } else {
                let mut precision: usize = 0;
                let mut has_digit = false;
                while i < fmt.len() {
                    let ch = fmt[i];
                    if !(b'0'..=b'9').contains(&ch) {
                        break;
                    }
                    has_digit = true;
                    precision = precision
                        .saturating_mul(10)
                        .saturating_add((ch - b'0') as usize);
                    i += 1;
                }
                spec.precision = if has_digit { Some(precision) } else { Some(0) };
            }
        }

        // Parse length modifier
        spec.length = parse_length(fmt, &mut i);

        if i >= fmt.len() {
            break;
        }

        spec.specifier = fmt[i];
        i += 1;

        // Handle format specifier
        match spec.specifier {
            b'%' => {
                let percent = [b'%'];
                let written = write_formatted_block(file, &spec, None, b"", &percent)?;
                total_written += written as i32;
            }
            b'c' => {
                let v: i32 = args.arg();
                let byte = (v & 0xFF) as u8;
                let written = write_formatted_block(file, &spec, None, b"", &[byte])?;
                total_written += written as i32;
            }
            b's' => {
                let ptr: *const u8 = args.arg();
                let slice = if ptr.is_null() {
                    b"(null)"
                } else {
                    let mut len = 0usize;
                    while ptr::read(ptr.add(len)) != 0 {
                        len += 1;
                    }
                    slice::from_raw_parts(ptr, len)
                };
                let truncated = if let Some(precision) = spec.precision {
                    &slice[..cmp::min(precision, slice.len())]
                } else {
                    slice
                };
                let written = write_formatted_block(file, &spec, None, b"", truncated)?;
                total_written += written as i32;
            }
            b'd' | b'i' => {
                let value = read_signed_arg(args, spec.length);
                let (negative, buf, idx) = format_signed(value, 10, 1);
                let digits = &buf[idx..];
                let written =
                    emit_formatted_integer(file, &spec, negative, digits, false, value == 0)?;
                total_written += written as i32;
            }
            b'u' => {
                let value = read_unsigned_arg(args, spec.length);
                let (buf, idx) = format_unsigned(value, 10, 1);
                let digits = &buf[idx..];
                let written =
                    emit_formatted_integer(file, &spec, false, digits, false, value == 0)?;
                total_written += written as i32;
            }
            b'x' | b'X' => {
                let value = read_unsigned_arg(args, spec.length);
                let uppercase = spec.specifier == b'X';
                let (buf, idx) = format_unsigned(value, 16, 1);
                let digits = &buf[idx..];
                let written =
                    emit_formatted_integer(file, &spec, false, digits, uppercase, value == 0)?;
                total_written += written as i32;
            }
            b'o' => {
                let value = read_unsigned_arg(args, spec.length);
                let (buf, idx) = format_unsigned(value, 8, 1);
                let digits = &buf[idx..];
                let written =
                    emit_formatted_integer(file, &spec, false, digits, false, value == 0)?;
                total_written += written as i32;
            }
            b'p' => {
                let value: *const c_void = args.arg();
                let addr = value as usize as u128;
                let (buf, idx) = format_unsigned(addr, 16, 1);
                let digits = &buf[idx..];
                let mut pointer_spec = spec;
                pointer_spec.specifier = b'p';
                pointer_spec.flags &= !(FLAG_ALT | FLAG_ZERO);
                let written =
                    emit_formatted_integer(file, &pointer_spec, false, digits, false, addr == 0)?;
                total_written += written as i32;
            }
            b'f' | b'F' => {
                let value = args.arg::<f64>();
                let uppercase = spec.specifier == b'F';
                let written = handle_float(&spec, file, value, uppercase)?;
                total_written += written as i32;
            }
            _ => {
                file_write_byte(file, spec.specifier)?;
                total_written += 1;
            }
        }
    }

    Ok(total_written)
}
