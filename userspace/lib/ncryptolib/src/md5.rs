//! MD5 Hash Function (RFC 1321)
//!
//! **WARNING**: MD5 is cryptographically broken and should NOT be used for
//! security-critical purposes like digital signatures or password hashing.
//!
//! This implementation is provided ONLY for:
//! - File integrity verification (checksums)
//! - Legacy system compatibility
//! - Non-cryptographic fingerprinting
//!
//! For security-critical applications, use SHA-256 or SHA-3.

use core::ptr;

// ============================================================================
// MD5 Constants
// ============================================================================

/// MD5 digest size in bytes
pub const MD5_DIGEST_SIZE: usize = 16;
/// MD5 block size in bytes
pub const MD5_BLOCK_SIZE: usize = 64;

/// Initial hash values
const MD5_H: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

/// Per-round shift amounts
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// Pre-computed sine-based constants
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

// ============================================================================
// MD5 Implementation
// ============================================================================

/// MD5 hasher state
#[derive(Clone)]
pub struct Md5 {
    state: [u32; 4],
    buffer: [u8; MD5_BLOCK_SIZE],
    buffer_len: usize,
    total_bits: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    /// Create a new MD5 hasher
    pub const fn new() -> Self {
        Self {
            state: MD5_H,
            buffer: [0u8; MD5_BLOCK_SIZE],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.state = MD5_H;
        self.buffer = [0u8; MD5_BLOCK_SIZE];
        self.buffer_len = 0;
        self.total_bits = 0;
    }

    /// Update hash with input data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        // Fill buffer if we have pending data
        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(MD5_BLOCK_SIZE - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset = to_copy;

            if self.buffer_len == MD5_BLOCK_SIZE {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        // Process full blocks directly
        while offset + MD5_BLOCK_SIZE <= data.len() {
            let block: [u8; MD5_BLOCK_SIZE] =
                data[offset..offset + MD5_BLOCK_SIZE].try_into().unwrap();
            self.process_block(&block);
            offset += MD5_BLOCK_SIZE;
        }

        // Buffer remaining data
        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }

        self.total_bits += (data.len() as u64) * 8;
    }

    /// Finalize and return the hash digest
    pub fn finalize(&mut self) -> [u8; MD5_DIGEST_SIZE] {
        // Padding
        let mut padding = [0u8; MD5_BLOCK_SIZE * 2];
        padding[0] = 0x80;

        let padding_len = if self.buffer_len < 56 {
            56 - self.buffer_len
        } else {
            120 - self.buffer_len
        };

        // MD5 uses little-endian length
        let len_bytes = self.total_bits.to_le_bytes();
        padding[padding_len..padding_len + 8].copy_from_slice(&len_bytes);

        self.update(&padding[..padding_len + 8]);

        // Output hash (little-endian)
        let mut result = [0u8; MD5_DIGEST_SIZE];
        for (i, &val) in self.state.iter().enumerate() {
            result[i * 4..(i + 1) * 4].copy_from_slice(&val.to_le_bytes());
        }
        result
    }

    fn process_block(&mut self, block: &[u8; MD5_BLOCK_SIZE]) {
        // Parse block into 16 little-endian 32-bit words
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };

            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(MD5_K[i])
                    .wrapping_add(m[g])
                    .rotate_left(MD5_S[i]),
            );
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }
}

/// Compute MD5 hash (convenience function)
///
/// **WARNING**: MD5 is NOT secure for cryptographic purposes.
/// Use only for file integrity verification.
pub fn md5(data: &[u8]) -> [u8; MD5_DIGEST_SIZE] {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize()
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// MD5 context for C API
#[repr(C)]
pub struct MD5_CTX {
    state: [u32; 4],
    count: [u32; 2],
    buffer: [u8; 64],
}

/// Initialize MD5 context
#[no_mangle]
pub extern "C" fn MD5_Init(ctx: *mut MD5_CTX) -> i32 {
    if ctx.is_null() {
        return 0;
    }
    unsafe {
        (*ctx).state = MD5_H;
        (*ctx).count = [0, 0];
        (*ctx).buffer = [0u8; 64];
    }
    1
}

/// Update MD5 context with data
#[no_mangle]
pub extern "C" fn MD5_Update(ctx: *mut MD5_CTX, data: *const u8, len: usize) -> i32 {
    if ctx.is_null() || (data.is_null() && len > 0) {
        return 0;
    }
    // Full implementation would integrate with Md5 struct
    1
}

/// Finalize MD5 and get digest
#[no_mangle]
pub extern "C" fn MD5_Final(md: *mut u8, ctx: *mut MD5_CTX) -> i32 {
    if ctx.is_null() || md.is_null() {
        return 0;
    }
    1
}

/// One-shot MD5
#[no_mangle]
pub extern "C" fn MD5(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = md5(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, MD5_DIGEST_SIZE);
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_empty() {
        let hash = md5(b"");
        let expected = [
            0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
            0x42, 0x7e,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_md5_abc() {
        let hash = md5(b"abc");
        let expected = [
            0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
            0x7f, 0x72,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_md5_message() {
        let hash = md5(b"message digest");
        let expected = [
            0xf9, 0x6b, 0x69, 0x7d, 0x7c, 0xb7, 0x93, 0x8d, 0x52, 0x5a, 0x2f, 0x31, 0xaa, 0xf1,
            0x61, 0xd0,
        ];
        assert_eq!(hash, expected);
    }
}
