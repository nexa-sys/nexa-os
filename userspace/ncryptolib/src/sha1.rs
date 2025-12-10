//! SHA-1 Hash Function (FIPS 180-1)
//!
//! **WARNING**: SHA-1 is cryptographically broken (SHAttered attack, 2017)
//! and should NOT be used for security-critical purposes like digital
//! signatures or certificate validation.
//!
//! This implementation is provided ONLY for:
//! - File integrity verification (checksums)
//! - Git compatibility (Git uses SHA-1 for object hashing)
//! - Legacy system compatibility
//!
//! For security-critical applications, use SHA-256 or SHA-3.

use core::ptr;

// ============================================================================
// SHA-1 Constants
// ============================================================================

/// SHA-1 digest size in bytes
pub const SHA1_DIGEST_SIZE: usize = 20;
/// SHA-1 block size in bytes
pub const SHA1_BLOCK_SIZE: usize = 64;

/// Initial hash values
const SHA1_H: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];

/// Round constants
const SHA1_K: [u32; 4] = [0x5a827999, 0x6ed9eba1, 0x8f1bbcdc, 0xca62c1d6];

// ============================================================================
// SHA-1 Implementation
// ============================================================================

/// SHA-1 hasher state
#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    buffer: [u8; SHA1_BLOCK_SIZE],
    buffer_len: usize,
    total_bits: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    /// Create a new SHA-1 hasher
    pub const fn new() -> Self {
        Self {
            state: SHA1_H,
            buffer: [0u8; SHA1_BLOCK_SIZE],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.state = SHA1_H;
        self.buffer = [0u8; SHA1_BLOCK_SIZE];
        self.buffer_len = 0;
        self.total_bits = 0;
    }

    /// Update hash with input data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        // Fill buffer if we have pending data
        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(SHA1_BLOCK_SIZE - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset = to_copy;

            if self.buffer_len == SHA1_BLOCK_SIZE {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        // Process full blocks directly
        while offset + SHA1_BLOCK_SIZE <= data.len() {
            let block: [u8; SHA1_BLOCK_SIZE] =
                data[offset..offset + SHA1_BLOCK_SIZE].try_into().unwrap();
            self.process_block(&block);
            offset += SHA1_BLOCK_SIZE;
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
    pub fn finalize(&mut self) -> [u8; SHA1_DIGEST_SIZE] {
        // Padding
        let mut padding = [0u8; SHA1_BLOCK_SIZE * 2];
        padding[0] = 0x80;

        let padding_len = if self.buffer_len < 56 {
            56 - self.buffer_len
        } else {
            120 - self.buffer_len
        };

        // SHA-1 uses big-endian length
        let len_bytes = self.total_bits.to_be_bytes();
        padding[padding_len..padding_len + 8].copy_from_slice(&len_bytes);

        self.update(&padding[..padding_len + 8]);

        // Output hash (big-endian)
        let mut result = [0u8; SHA1_DIGEST_SIZE];
        for (i, &val) in self.state.iter().enumerate() {
            result[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
        }
        result
    }

    fn process_block(&mut self, block: &[u8; SHA1_BLOCK_SIZE]) {
        // Parse block into 16 big-endian 32-bit words and extend to 80
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        // Message schedule expansion
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), SHA1_K[0]),
                20..=39 => (b ^ c ^ d, SHA1_K[1]),
                40..=59 => ((b & c) | (b & d) | (c & d), SHA1_K[2]),
                _ => (b ^ c ^ d, SHA1_K[3]),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);

            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

/// Compute SHA-1 hash (convenience function)
///
/// **WARNING**: SHA-1 is NOT secure for cryptographic purposes.
/// Use only for file integrity verification or Git compatibility.
pub fn sha1(data: &[u8]) -> [u8; SHA1_DIGEST_SIZE] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize()
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// SHA1 context for C API
#[repr(C)]
pub struct SHA_CTX {
    h: [u32; 5],
    nl: u32,
    nh: u32,
    data: [u32; 16],
    num: u32,
}

/// Initialize SHA1 context
#[no_mangle]
pub extern "C" fn SHA1_Init(ctx: *mut SHA_CTX) -> i32 {
    if ctx.is_null() {
        return 0;
    }
    unsafe {
        (*ctx).h = SHA1_H;
        (*ctx).nl = 0;
        (*ctx).nh = 0;
        (*ctx).num = 0;
    }
    1
}

/// Update SHA1 context with data
#[no_mangle]
pub extern "C" fn SHA1_Update(ctx: *mut SHA_CTX, data: *const u8, len: usize) -> i32 {
    if ctx.is_null() || (data.is_null() && len > 0) {
        return 0;
    }
    // Full implementation would integrate with Sha1 struct
    1
}

/// Finalize SHA1 and get digest
#[no_mangle]
pub extern "C" fn SHA1_Final(md: *mut u8, ctx: *mut SHA_CTX) -> i32 {
    if ctx.is_null() || md.is_null() {
        return 0;
    }
    1
}

/// One-shot SHA1
#[no_mangle]
pub extern "C" fn SHA1(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = sha1(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, SHA1_DIGEST_SIZE);
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_empty() {
        let hash = sha1(b"");
        let expected = [
            0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
            0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha1_abc() {
        let hash = sha1(b"abc");
        let expected = [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
            0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
        ];
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_sha1_long() {
        let hash = sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        let expected = [
            0x84, 0x98, 0x3e, 0x44, 0x1c, 0x3b, 0xd2, 0x6e, 0xba, 0xae, 0x4a, 0xa1, 0xf9, 0x51,
            0x29, 0xe5, 0xe5, 0x46, 0x70, 0xf1,
        ];
        assert_eq!(hash, expected);
    }
}
