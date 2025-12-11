//! Adler-32 Checksum (RFC 1950)
//!
//! Used by ZLIB format for data integrity verification.
//! Faster than CRC32 but slightly weaker error detection.

/// Adler-32 modulo value
const ADLER_MOD: u32 = 65521;

/// Number of bytes we can process before overflow check
/// (5552 is the largest n such that 255n(n+1)/2 + (n+1)(BASE-1) <= 2^32-1)
const NMAX: usize = 5552;

/// Adler-32 hasher state
#[derive(Clone)]
pub struct Adler32 {
    a: u32, // Sum of all bytes
    b: u32, // Sum of all a values
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Adler32 {
    /// Create a new Adler-32 hasher
    pub const fn new() -> Self {
        Self { a: 1, b: 0 }
    }

    /// Create with initial value
    pub const fn with_init(adler: u32) -> Self {
        Self {
            a: adler & 0xFFFF,
            b: (adler >> 16) & 0xFFFF,
        }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.a = 1;
        self.b = 0;
    }

    /// Update checksum with input data
    pub fn update(&mut self, data: &[u8]) {
        let mut a = self.a;
        let mut b = self.b;
        let mut offset = 0;

        while offset < data.len() {
            let chunk_len = core::cmp::min(NMAX, data.len() - offset);

            for i in 0..chunk_len {
                a += data[offset + i] as u32;
                b += a;
            }

            a %= ADLER_MOD;
            b %= ADLER_MOD;
            offset += chunk_len;
        }

        self.a = a;
        self.b = b;
    }

    /// Finalize and return the Adler-32 checksum
    pub fn finalize(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

/// Calculate Adler-32 of a byte slice in one call
pub fn adler32(data: &[u8]) -> u32 {
    let mut hasher = Adler32::new();
    hasher.update(data);
    hasher.finalize()
}

/// Update Adler-32 with a byte slice (zlib-compatible)
pub fn adler32_slice(adler: u32, data: &[u8]) -> u32 {
    let mut hasher = Adler32::with_init(adler);
    hasher.update(data);
    hasher.finalize()
}

/// Combine two Adler-32 values
/// adler1 is the checksum of the first block, adler2 is the checksum of the second block
/// len2 is the length of the second block
pub fn adler32_combine(adler1: u32, adler2: u32, len2: usize) -> u32 {
    adler32_combine_impl(adler1, adler2, len2)
}

/// Combine two Adler-32 values implementation
pub fn adler32_combine_impl(adler1: u32, adler2: u32, len2: usize) -> u32 {
    let a1 = adler1 & 0xFFFF;
    let b1 = (adler1 >> 16) & 0xFFFF;
    let a2 = adler2 & 0xFFFF;
    let b2 = (adler2 >> 16) & 0xFFFF;

    // Combine:
    // a_combined = (a1 + a2 - 1) mod BASE
    // b_combined = (b1 + b2 + a1 * len2 - len2) mod BASE
    let mut a = (a1 + a2) % ADLER_MOD;
    if a >= 1 {
        a -= 1;
    } else {
        a += ADLER_MOD - 1;
    }

    let rem = (len2 % ADLER_MOD as usize) as u32;
    let mut b = (b1 + b2 + a1 * rem) % ADLER_MOD;
    if b >= rem {
        b -= rem;
    } else {
        b += ADLER_MOD - rem;
    }

    (b << 16) | a
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, c_ulong, uInt, Bytef};

/// Calculate Adler-32 (zlib-compatible C ABI)
#[no_mangle]
pub extern "C" fn nzip_adler32(adler: c_ulong, buf: *const Bytef, len: uInt) -> c_ulong {
    if buf.is_null() {
        return 1; // Initial adler32 value
    }

    if len == 0 {
        return adler;
    }

    unsafe {
        let data = core::slice::from_raw_parts(buf, len as usize);
        adler32_slice(adler as u32, data) as c_ulong
    }
}

/// Combine two Adler-32 values (zlib-compatible C ABI)
#[no_mangle]
pub extern "C" fn nzip_adler32_combine(adler1: c_ulong, adler2: c_ulong, len2: c_int) -> c_ulong {
    if len2 < 0 {
        return adler1;
    }
    adler32_combine_impl(adler1 as u32, adler2 as u32, len2 as usize) as c_ulong
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adler32_empty() {
        // Adler-32 of empty data should be 1
        assert_eq!(adler32(&[]), 1);
    }

    #[test]
    fn test_adler32_hello() {
        // Known Adler-32 of "hello"
        assert_eq!(adler32(b"hello"), 0x062c0215);
    }

    #[test]
    fn test_adler32_incremental() {
        let full = adler32(b"hello world");

        let mut hasher = Adler32::new();
        hasher.update(b"hello");
        hasher.update(b" ");
        hasher.update(b"world");

        assert_eq!(hasher.finalize(), full);
    }
}
