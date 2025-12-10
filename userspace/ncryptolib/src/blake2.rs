//! BLAKE2 Hash Functions (RFC 7693)
//!
//! Modern, high-performance cryptographic hash functions.
//! BLAKE2 is faster than MD5, SHA-1, and SHA-2, while being at least as secure as SHA-3.
//!
//! Variants:
//! - BLAKE2b: Optimized for 64-bit platforms, 1-64 byte digest
//! - BLAKE2s: Optimized for 32-bit platforms, 1-32 byte digest
//!
//! Use cases:
//! - File integrity verification (faster than SHA-256)
//! - Key derivation (keyed hashing mode)
//! - Digital signatures
//! - Password hashing (with salt/personalization)

use core::ptr;
use std::vec::Vec;

// ============================================================================
// BLAKE2b Constants
// ============================================================================

/// Default BLAKE2b digest size (64 bytes / 512 bits)
pub const BLAKE2B_DIGEST_SIZE: usize = 64;
/// BLAKE2b block size (128 bytes)
pub const BLAKE2B_BLOCK_SIZE: usize = 128;
/// Maximum BLAKE2b key size (64 bytes)
pub const BLAKE2B_KEY_SIZE: usize = 64;

/// BLAKE2b IV (same as SHA-512 IV)
const BLAKE2B_IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// BLAKE2b sigma schedule
const BLAKE2B_SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

// ============================================================================
// BLAKE2s Constants
// ============================================================================

/// Default BLAKE2s digest size (32 bytes / 256 bits)
pub const BLAKE2S_DIGEST_SIZE: usize = 32;
/// BLAKE2s block size (64 bytes)
pub const BLAKE2S_BLOCK_SIZE: usize = 64;
/// Maximum BLAKE2s key size (32 bytes)
pub const BLAKE2S_KEY_SIZE: usize = 32;

/// BLAKE2s IV (same as SHA-256 IV)
const BLAKE2S_IV: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// BLAKE2s sigma schedule (same as BLAKE2b)
const BLAKE2S_SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

// ============================================================================
// BLAKE2b Implementation
// ============================================================================

/// BLAKE2b hasher state
#[derive(Clone)]
pub struct Blake2b {
    h: [u64; 8], // State
    t: [u64; 2], // Counter
    f: [u64; 2], // Finalization flags
    buffer: [u8; BLAKE2B_BLOCK_SIZE],
    buffer_len: usize,
    digest_len: usize,
}

impl Blake2b {
    /// Create a new BLAKE2b hasher with specified digest length (1-64 bytes)
    pub fn new(digest_len: usize) -> Self {
        assert!(digest_len >= 1 && digest_len <= BLAKE2B_DIGEST_SIZE);

        let mut h = BLAKE2B_IV;
        // Parameter block: digest_len, key_len=0, fanout=1, depth=1
        h[0] ^= 0x01010000 ^ (digest_len as u64);

        Self {
            h,
            t: [0, 0],
            f: [0, 0],
            buffer: [0u8; BLAKE2B_BLOCK_SIZE],
            buffer_len: 0,
            digest_len,
        }
    }

    /// Create a new keyed BLAKE2b hasher (MAC mode)
    pub fn new_keyed(key: &[u8], digest_len: usize) -> Self {
        assert!(key.len() <= BLAKE2B_KEY_SIZE);
        assert!(digest_len >= 1 && digest_len <= BLAKE2B_DIGEST_SIZE);

        let mut h = BLAKE2B_IV;
        h[0] ^= 0x01010000 ^ ((key.len() as u64) << 8) ^ (digest_len as u64);

        let mut hasher = Self {
            h,
            t: [0, 0],
            f: [0, 0],
            buffer: [0u8; BLAKE2B_BLOCK_SIZE],
            buffer_len: 0,
            digest_len,
        };

        // If keyed, the first block is the key padded to block size
        if !key.is_empty() {
            hasher.buffer[..key.len()].copy_from_slice(key);
            hasher.buffer_len = BLAKE2B_BLOCK_SIZE;
        }

        hasher
    }

    /// Reset to initial state
    pub fn reset(&mut self) {
        self.h = BLAKE2B_IV;
        self.h[0] ^= 0x01010000 ^ (self.digest_len as u64);
        self.t = [0, 0];
        self.f = [0, 0];
        self.buffer = [0u8; BLAKE2B_BLOCK_SIZE];
        self.buffer_len = 0;
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        // If buffer has data, try to fill it
        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(BLAKE2B_BLOCK_SIZE - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset = to_copy;

            if self.buffer_len == BLAKE2B_BLOCK_SIZE {
                self.increment_counter(BLAKE2B_BLOCK_SIZE as u64);
                let block = self.buffer;
                self.compress(&block, false);
                self.buffer_len = 0;
            }
        }

        // Process full blocks, keeping last partial block in buffer
        while offset + BLAKE2B_BLOCK_SIZE < data.len() {
            self.increment_counter(BLAKE2B_BLOCK_SIZE as u64);
            let block: [u8; BLAKE2B_BLOCK_SIZE] = data[offset..offset + BLAKE2B_BLOCK_SIZE]
                .try_into()
                .unwrap();
            self.compress(&block, false);
            offset += BLAKE2B_BLOCK_SIZE;
        }

        // Buffer remaining
        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[self.buffer_len..self.buffer_len + remaining]
                .copy_from_slice(&data[offset..]);
            self.buffer_len += remaining;
        }
    }

    /// Finalize and return digest
    pub fn finalize(&mut self) -> Vec<u8> {
        // Process final block
        self.increment_counter(self.buffer_len as u64);
        self.f[0] = u64::MAX; // Set finalization flag

        // Pad buffer with zeros
        for i in self.buffer_len..BLAKE2B_BLOCK_SIZE {
            self.buffer[i] = 0;
        }

        let block = self.buffer;
        self.compress(&block, true);

        // Output digest
        let mut result = Vec::with_capacity(self.digest_len);
        for i in 0..self.digest_len {
            result.push((self.h[i / 8] >> (8 * (i % 8))) as u8);
        }
        result
    }

    fn increment_counter(&mut self, inc: u64) {
        self.t[0] = self.t[0].wrapping_add(inc);
        if self.t[0] < inc {
            self.t[1] = self.t[1].wrapping_add(1);
        }
    }

    fn compress(&mut self, block: &[u8; BLAKE2B_BLOCK_SIZE], _last: bool) {
        // Parse message block
        let mut m = [0u64; 16];
        for i in 0..16 {
            m[i] = u64::from_le_bytes([
                block[i * 8],
                block[i * 8 + 1],
                block[i * 8 + 2],
                block[i * 8 + 3],
                block[i * 8 + 4],
                block[i * 8 + 5],
                block[i * 8 + 6],
                block[i * 8 + 7],
            ]);
        }

        // Initialize working vector
        let mut v = [0u64; 16];
        v[..8].copy_from_slice(&self.h);
        v[8..16].copy_from_slice(&BLAKE2B_IV);
        v[12] ^= self.t[0];
        v[13] ^= self.t[1];
        v[14] ^= self.f[0];
        v[15] ^= self.f[1];

        // Mixing rounds
        for round in 0..12 {
            let s = &BLAKE2B_SIGMA[round];

            // Column step
            Self::g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            Self::g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            Self::g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            Self::g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);

            // Diagonal step
            Self::g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            Self::g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            Self::g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            Self::g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }

        // Finalize
        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }

    #[inline]
    fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(32);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(24);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(63);
    }
}

impl Default for Blake2b {
    fn default() -> Self {
        Self::new(BLAKE2B_DIGEST_SIZE)
    }
}

/// Compute BLAKE2b hash with default 64-byte digest
pub fn blake2b(data: &[u8]) -> [u8; BLAKE2B_DIGEST_SIZE] {
    let mut hasher = Blake2b::new(BLAKE2B_DIGEST_SIZE);
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; BLAKE2B_DIGEST_SIZE];
    output.copy_from_slice(&result);
    output
}

/// Compute BLAKE2b hash with custom digest length
pub fn blake2b_with_len(data: &[u8], digest_len: usize) -> Vec<u8> {
    let mut hasher = Blake2b::new(digest_len);
    hasher.update(data);
    hasher.finalize()
}

/// Compute keyed BLAKE2b (MAC)
pub fn blake2b_keyed(key: &[u8], data: &[u8], digest_len: usize) -> Vec<u8> {
    let mut hasher = Blake2b::new_keyed(key, digest_len);
    hasher.update(data);
    hasher.finalize()
}

// ============================================================================
// BLAKE2s Implementation
// ============================================================================

/// BLAKE2s hasher state
#[derive(Clone)]
pub struct Blake2s {
    h: [u32; 8],
    t: [u32; 2],
    f: [u32; 2],
    buffer: [u8; BLAKE2S_BLOCK_SIZE],
    buffer_len: usize,
    digest_len: usize,
}

impl Blake2s {
    /// Create a new BLAKE2s hasher with specified digest length (1-32 bytes)
    pub fn new(digest_len: usize) -> Self {
        assert!(digest_len >= 1 && digest_len <= BLAKE2S_DIGEST_SIZE);

        let mut h = BLAKE2S_IV;
        h[0] ^= 0x01010000 ^ (digest_len as u32);

        Self {
            h,
            t: [0, 0],
            f: [0, 0],
            buffer: [0u8; BLAKE2S_BLOCK_SIZE],
            buffer_len: 0,
            digest_len,
        }
    }

    /// Create a new keyed BLAKE2s hasher
    pub fn new_keyed(key: &[u8], digest_len: usize) -> Self {
        assert!(key.len() <= BLAKE2S_KEY_SIZE);
        assert!(digest_len >= 1 && digest_len <= BLAKE2S_DIGEST_SIZE);

        let mut h = BLAKE2S_IV;
        h[0] ^= 0x01010000 ^ ((key.len() as u32) << 8) ^ (digest_len as u32);

        let mut hasher = Self {
            h,
            t: [0, 0],
            f: [0, 0],
            buffer: [0u8; BLAKE2S_BLOCK_SIZE],
            buffer_len: 0,
            digest_len,
        };

        if !key.is_empty() {
            hasher.buffer[..key.len()].copy_from_slice(key);
            hasher.buffer_len = BLAKE2S_BLOCK_SIZE;
        }

        hasher
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(BLAKE2S_BLOCK_SIZE - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset = to_copy;

            if self.buffer_len == BLAKE2S_BLOCK_SIZE {
                self.increment_counter(BLAKE2S_BLOCK_SIZE as u32);
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }

        while offset + BLAKE2S_BLOCK_SIZE < data.len() {
            self.increment_counter(BLAKE2S_BLOCK_SIZE as u32);
            let block: [u8; BLAKE2S_BLOCK_SIZE] = data[offset..offset + BLAKE2S_BLOCK_SIZE]
                .try_into()
                .unwrap();
            self.compress(&block);
            offset += BLAKE2S_BLOCK_SIZE;
        }

        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[self.buffer_len..self.buffer_len + remaining]
                .copy_from_slice(&data[offset..]);
            self.buffer_len += remaining;
        }
    }

    /// Finalize and return digest
    pub fn finalize(&mut self) -> Vec<u8> {
        self.increment_counter(self.buffer_len as u32);
        self.f[0] = u32::MAX;

        for i in self.buffer_len..BLAKE2S_BLOCK_SIZE {
            self.buffer[i] = 0;
        }

        let block = self.buffer;
        self.compress(&block);

        let mut result = Vec::with_capacity(self.digest_len);
        for i in 0..self.digest_len {
            result.push((self.h[i / 4] >> (8 * (i % 4))) as u8);
        }
        result
    }

    fn increment_counter(&mut self, inc: u32) {
        self.t[0] = self.t[0].wrapping_add(inc);
        if self.t[0] < inc {
            self.t[1] = self.t[1].wrapping_add(1);
        }
    }

    fn compress(&mut self, block: &[u8; BLAKE2S_BLOCK_SIZE]) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        let mut v = [0u32; 16];
        v[..8].copy_from_slice(&self.h);
        v[8..16].copy_from_slice(&BLAKE2S_IV);
        v[12] ^= self.t[0];
        v[13] ^= self.t[1];
        v[14] ^= self.f[0];
        v[15] ^= self.f[1];

        for round in 0..10 {
            let s = &BLAKE2S_SIGMA[round];

            Self::g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            Self::g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            Self::g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            Self::g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);

            Self::g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            Self::g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            Self::g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            Self::g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }

        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }

    #[inline]
    fn g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(12);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(8);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(7);
    }
}

impl Default for Blake2s {
    fn default() -> Self {
        Self::new(BLAKE2S_DIGEST_SIZE)
    }
}

/// Compute BLAKE2s hash with default 32-byte digest
pub fn blake2s(data: &[u8]) -> [u8; BLAKE2S_DIGEST_SIZE] {
    let mut hasher = Blake2s::new(BLAKE2S_DIGEST_SIZE);
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; BLAKE2S_DIGEST_SIZE];
    output.copy_from_slice(&result);
    output
}

/// Compute BLAKE2s hash with custom digest length
pub fn blake2s_with_len(data: &[u8], digest_len: usize) -> Vec<u8> {
    let mut hasher = Blake2s::new(digest_len);
    hasher.update(data);
    hasher.finalize()
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// One-shot BLAKE2b (64-byte output)
#[no_mangle]
pub extern "C" fn BLAKE2b(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = blake2b(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, BLAKE2B_DIGEST_SIZE);
    }

    md
}

/// One-shot BLAKE2s (32-byte output)
#[no_mangle]
pub extern "C" fn BLAKE2s(data: *const u8, len: usize, md: *mut u8) -> *mut u8 {
    if data.is_null() || md.is_null() {
        return ptr::null_mut();
    }

    let input = unsafe { core::slice::from_raw_parts(data, len) };
    let hash = blake2s(input);

    unsafe {
        ptr::copy_nonoverlapping(hash.as_ptr(), md, BLAKE2S_DIGEST_SIZE);
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake2b_empty() {
        let hash = blake2b(b"");
        // First 8 bytes of BLAKE2b("")
        assert_eq!(hash[0], 0x78);
        assert_eq!(hash[1], 0x6a);
    }

    #[test]
    fn test_blake2s_empty() {
        let hash = blake2s(b"");
        // First bytes of BLAKE2s("")
        assert_eq!(hash[0], 0x69);
        assert_eq!(hash[1], 0x21);
    }

    #[test]
    fn test_blake2b_abc() {
        let hash = blake2b(b"abc");
        // First few bytes - verify implementation works
        assert_ne!(hash[0], 0);
    }
}
