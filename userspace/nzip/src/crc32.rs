//! CRC32 Checksum (ISO 3309 / RFC 1952)
//!
//! Used by GZIP format for data integrity verification.

/// CRC32 polynomial (IEEE 802.3, used by GZIP)
pub const CRC32_POLYNOMIAL: u32 = 0xEDB88320;

/// Pre-computed CRC32 lookup table
static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ CRC32_POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// CRC32 hasher state
#[derive(Clone)]
pub struct Crc32 {
    state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    /// Create a new CRC32 hasher
    pub const fn new() -> Self {
        Self { state: 0xFFFFFFFF }
    }

    /// Create with initial CRC value
    pub const fn with_init(init: u32) -> Self {
        Self { state: !init }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.state = 0xFFFFFFFF;
    }

    /// Update CRC with input data
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let index = ((self.state ^ byte as u32) & 0xFF) as usize;
            self.state = CRC32_TABLE[index] ^ (self.state >> 8);
        }
    }

    /// Finalize and return the CRC32 checksum
    pub fn finalize(&self) -> u32 {
        self.state ^ 0xFFFFFFFF
    }
}

/// Calculate CRC32 of a byte slice in one call
pub fn crc32(data: &[u8]) -> u32 {
    let mut hasher = Crc32::new();
    hasher.update(data);
    hasher.finalize()
}

/// Update CRC32 with a byte slice (zlib-compatible)
pub fn crc32_slice(crc: u32, data: &[u8]) -> u32 {
    let mut hasher = Crc32::with_init(crc);
    hasher.update(data);
    hasher.finalize()
}

/// Combine two CRC32 values
/// crc1 is the CRC of the first block, crc2 is the CRC of the second block
/// len2 is the length of the second block
pub fn crc32_combine(crc1: u32, crc2: u32, len2: usize) -> u32 {
    crc32_combine_impl(crc1, crc2, len2)
}

/// GF(2) matrix multiplication for CRC combining
fn gf2_matrix_times(mat: &[u32; 32], vec: u32) -> u32 {
    let mut sum = 0u32;
    let mut v = vec;
    let mut i = 0;
    while v != 0 {
        if v & 1 != 0 {
            sum ^= mat[i];
        }
        v >>= 1;
        i += 1;
    }
    sum
}

/// Square a GF(2) matrix
fn gf2_matrix_square(square: &mut [u32; 32], mat: &[u32; 32]) {
    for n in 0..32 {
        square[n] = gf2_matrix_times(mat, mat[n]);
    }
}

/// Combine two CRC32 values implementation
pub fn crc32_combine_impl(crc1: u32, crc2: u32, mut len2: usize) -> u32 {
    if len2 == 0 {
        return crc1;
    }

    // Put operator for one zero bit in odd
    let mut odd = [0u32; 32];
    odd[0] = CRC32_POLYNOMIAL;
    let mut row = 1u32;
    for i in 1..32 {
        odd[i] = row;
        row <<= 1;
    }

    // Put operator for two zero bits in even
    let mut even = [0u32; 32];
    gf2_matrix_square(&mut even, &odd);
    gf2_matrix_square(&mut odd, &even);

    // Apply len2 zeros to crc1 using the operator
    let mut crc = crc1;
    loop {
        // Apply zeros operator for this bit of len2
        gf2_matrix_square(&mut even, &odd);
        if len2 & 1 != 0 {
            crc = gf2_matrix_times(&even, crc);
        }
        len2 >>= 1;

        if len2 == 0 {
            break;
        }

        gf2_matrix_square(&mut odd, &even);
        if len2 & 1 != 0 {
            crc = gf2_matrix_times(&odd, crc);
        }
        len2 >>= 1;

        if len2 == 0 {
            break;
        }
    }

    crc ^ crc2
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, c_ulong, Bytef, uInt};

/// Calculate CRC32 (zlib-compatible C ABI)
#[no_mangle]
pub extern "C" fn nzip_crc32(crc: c_ulong, buf: *const Bytef, len: uInt) -> c_ulong {
    if buf.is_null() || len == 0 {
        return crc;
    }
    
    unsafe {
        let data = core::slice::from_raw_parts(buf, len as usize);
        crc32_slice(crc as u32, data) as c_ulong
    }
}

/// Combine two CRC32 values (zlib-compatible C ABI)
#[no_mangle]
pub extern "C" fn nzip_crc32_combine(crc1: c_ulong, crc2: c_ulong, len2: c_int) -> c_ulong {
    if len2 < 0 {
        return crc1;
    }
    crc32_combine_impl(crc1 as u32, crc2 as u32, len2 as usize) as c_ulong
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn test_crc32_hello() {
        // Known CRC32 of "hello"
        assert_eq!(crc32(b"hello"), 0x3610a686);
    }

    #[test]
    fn test_crc32_incremental() {
        let full = crc32(b"hello world");
        
        let mut hasher = Crc32::new();
        hasher.update(b"hello");
        hasher.update(b" ");
        hasher.update(b"world");
        
        assert_eq!(hasher.finalize(), full);
    }
}
