//! Base64 Encoding/Decoding
//!
//! RFC 4648 compliant Base64 and Base64URL implementation.

use std::vec::Vec;

// ============================================================================
// Constants
// ============================================================================

/// Standard Base64 alphabet (RFC 4648)
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// URL-safe Base64 alphabet (RFC 4648)
const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Padding character
const PADDING: u8 = b'=';

/// Invalid character marker
const INVALID: u8 = 0xFF;

// ============================================================================
// Decode Tables
// ============================================================================

/// Generate decode table from alphabet
const fn generate_decode_table(alphabet: &[u8; 64]) -> [u8; 256] {
    let mut table = [INVALID; 256];
    let mut i = 0;
    while i < 64 {
        table[alphabet[i] as usize] = i as u8;
        i += 1;
    }
    table
}

const BASE64_DECODE: [u8; 256] = generate_decode_table(BASE64_ALPHABET);
const BASE64URL_DECODE: [u8; 256] = generate_decode_table(BASE64URL_ALPHABET);

// ============================================================================
// Base64 Encoding
// ============================================================================

/// Encode bytes to Base64 string
pub fn base64_encode(data: &[u8]) -> String {
    encode_with_alphabet(data, BASE64_ALPHABET, true)
}

/// Encode bytes to Base64 string without padding
pub fn base64_encode_nopad(data: &[u8]) -> String {
    encode_with_alphabet(data, BASE64_ALPHABET, false)
}

/// Encode bytes to URL-safe Base64 string
pub fn base64url_encode(data: &[u8]) -> String {
    encode_with_alphabet(data, BASE64URL_ALPHABET, false)
}

/// Encode bytes to URL-safe Base64 string with padding
pub fn base64url_encode_padded(data: &[u8]) -> String {
    encode_with_alphabet(data, BASE64URL_ALPHABET, true)
}

fn encode_with_alphabet(data: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    if data.is_empty() {
        return String::new();
    }

    let output_len = if pad {
        ((data.len() + 2) / 3) * 4
    } else {
        (data.len() * 4 + 2) / 3
    };

    let mut output = Vec::with_capacity(output_len);
    let mut i = 0;

    // Process 3 bytes at a time
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        output.push(alphabet[((n >> 18) & 0x3F) as usize]);
        output.push(alphabet[((n >> 12) & 0x3F) as usize]);
        output.push(alphabet[((n >> 6) & 0x3F) as usize]);
        output.push(alphabet[(n & 0x3F) as usize]);
        i += 3;
    }

    // Handle remaining bytes
    let remaining = data.len() - i;
    if remaining == 1 {
        let n = (data[i] as u32) << 16;
        output.push(alphabet[((n >> 18) & 0x3F) as usize]);
        output.push(alphabet[((n >> 12) & 0x3F) as usize]);
        if pad {
            output.push(PADDING);
            output.push(PADDING);
        }
    } else if remaining == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        output.push(alphabet[((n >> 18) & 0x3F) as usize]);
        output.push(alphabet[((n >> 12) & 0x3F) as usize]);
        output.push(alphabet[((n >> 6) & 0x3F) as usize]);
        if pad {
            output.push(PADDING);
        }
    }

    // SAFETY: output contains only ASCII characters
    unsafe { String::from_utf8_unchecked(output) }
}

// ============================================================================
// Base64 Decoding
// ============================================================================

/// Decode Base64 string to bytes
pub fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    decode_with_table(input, &BASE64_DECODE)
}

/// Decode URL-safe Base64 string to bytes
pub fn base64url_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    decode_with_table(input, &BASE64URL_DECODE)
}

fn decode_with_table(input: &str, decode_table: &[u8; 256]) -> Result<Vec<u8>, &'static str> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let bytes = input.as_bytes();

    // Count valid characters
    let mut valid_len = 0;

    for &b in bytes {
        if b == PADDING {
            break;
        }
        if decode_table[b as usize] == INVALID {
            // Skip whitespace
            if b == b' ' || b == b'\n' || b == b'\r' || b == b'\t' {
                continue;
            }
            return Err("Invalid Base64 character");
        }
        valid_len += 1;
    }

    // Calculate output size
    let output_len = (valid_len * 3) / 4;
    let mut output = Vec::with_capacity(output_len);

    // Decode
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for &b in bytes {
        if b == PADDING {
            break;
        }

        let value = decode_table[b as usize];
        if value == INVALID {
            // Skip whitespace
            continue;
        }

        buffer = (buffer << 6) | (value as u32);
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }

    Ok(output)
}

// ============================================================================
// Hex Encoding/Decoding
// ============================================================================

/// Hex alphabet (lowercase)
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";
/// Hex alphabet (uppercase)
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// Encode bytes to lowercase hex string
pub fn hex_encode(data: &[u8]) -> String {
    let mut output = Vec::with_capacity(data.len() * 2);
    for &b in data {
        output.push(HEX_LOWER[(b >> 4) as usize]);
        output.push(HEX_LOWER[(b & 0x0F) as usize]);
    }
    unsafe { String::from_utf8_unchecked(output) }
}

/// Encode bytes to uppercase hex string
pub fn hex_encode_upper(data: &[u8]) -> String {
    let mut output = Vec::with_capacity(data.len() * 2);
    for &b in data {
        output.push(HEX_UPPER[(b >> 4) as usize]);
        output.push(HEX_UPPER[(b & 0x0F) as usize]);
    }
    unsafe { String::from_utf8_unchecked(output) }
}

/// Decode hex string to bytes
pub fn hex_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let bytes = input.as_bytes();

    // Filter out whitespace and count valid characters
    let filtered: Vec<u8> = bytes
        .iter()
        .filter(|&&b| b != b' ' && b != b'\n' && b != b'\r' && b != b'\t')
        .copied()
        .collect();

    if filtered.len() % 2 != 0 {
        return Err("Hex string must have even length");
    }

    let mut output = Vec::with_capacity(filtered.len() / 2);

    for chunk in filtered.chunks(2) {
        let high = hex_char_to_nibble(chunk[0])?;
        let low = hex_char_to_nibble(chunk[1])?;
        output.push((high << 4) | low);
    }

    Ok(output)
}

fn hex_char_to_nibble(c: u8) -> Result<u8, &'static str> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err("Invalid hex character"),
    }
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_char, c_int, size_t};

/// Base64 encode (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_base64_encode(
    input: *const u8,
    input_len: size_t,
    output: *mut c_char,
    output_len: *mut size_t,
) -> c_int {
    if input.is_null() || output.is_null() || output_len.is_null() {
        return -1;
    }

    let input_slice = core::slice::from_raw_parts(input, input_len);
    let encoded = base64_encode(input_slice);

    core::ptr::copy_nonoverlapping(encoded.as_ptr() as *const c_char, output, encoded.len());
    *output_len = encoded.len();

    0
}

/// Base64 decode (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_base64_decode(
    input: *const c_char,
    input_len: size_t,
    output: *mut u8,
    output_len: *mut size_t,
) -> c_int {
    if input.is_null() || output.is_null() || output_len.is_null() {
        return -1;
    }

    let input_slice = core::slice::from_raw_parts(input as *const u8, input_len);
    let input_str = match core::str::from_utf8(input_slice) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match base64_decode(input_str) {
        Ok(decoded) => {
            core::ptr::copy_nonoverlapping(decoded.as_ptr(), output, decoded.len());
            *output_len = decoded.len();
            0
        }
        Err(_) => -1,
    }
}

/// Hex encode (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hex_encode(
    input: *const u8,
    input_len: size_t,
    output: *mut c_char,
    output_len: *mut size_t,
) -> c_int {
    if input.is_null() || output.is_null() || output_len.is_null() {
        return -1;
    }

    let input_slice = core::slice::from_raw_parts(input, input_len);
    let encoded = hex_encode(input_slice);

    core::ptr::copy_nonoverlapping(encoded.as_ptr() as *const c_char, output, encoded.len());
    *output_len = encoded.len();

    0
}

/// Hex decode (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hex_decode(
    input: *const c_char,
    input_len: size_t,
    output: *mut u8,
    output_len: *mut size_t,
) -> c_int {
    if input.is_null() || output.is_null() || output_len.is_null() {
        return -1;
    }

    let input_slice = core::slice::from_raw_parts(input as *const u8, input_len);
    let input_str = match core::str::from_utf8(input_slice) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match hex_decode(input_str) {
        Ok(decoded) => {
            core::ptr::copy_nonoverlapping(decoded.as_ptr(), output, decoded.len());
            *output_len = decoded.len();
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_base64_decode() {
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn test_base64url_encode() {
        // Test URL-safe characters
        let data = &[0xfb, 0xff, 0xfe];
        let encoded = base64url_encode(data);
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(b""), "");
        assert_eq!(hex_encode(b"\x00"), "00");
        assert_eq!(hex_encode(b"\xff"), "ff");
        assert_eq!(hex_encode(b"Hello"), "48656c6c6f");
    }

    #[test]
    fn test_hex_decode() {
        assert_eq!(hex_decode("").unwrap(), b"");
        assert_eq!(hex_decode("00").unwrap(), b"\x00");
        assert_eq!(hex_decode("FF").unwrap(), b"\xff");
        assert_eq!(hex_decode("ff").unwrap(), b"\xff");
        assert_eq!(hex_decode("48656c6c6f").unwrap(), b"Hello");
    }

    #[test]
    fn test_roundtrip() {
        let data = b"The quick brown fox jumps over the lazy dog";
        assert_eq!(base64_decode(&base64_encode(data)).unwrap(), data);
        assert_eq!(hex_decode(&hex_encode(data)).unwrap(), data);
    }
}
