//! HMAC (Hash-based Message Authentication Code)
//!
//! RFC 2104 compliant HMAC implementation.
//! Supports multiple hash functions: SHA-256, SHA-384, SHA-512, SHA3-256.

use std::vec::Vec;

use crate::hash::{Sha384, Sha512, SHA384_BLOCK_SIZE, SHA512_BLOCK_SIZE};
use crate::sha3::Sha3;

// ============================================================================
// HMAC Trait
// ============================================================================

/// Generic HMAC interface
pub trait Hmac: Sized {
    /// Output size in bytes
    const OUTPUT_SIZE: usize;
    /// Block size in bytes
    const BLOCK_SIZE: usize;

    /// Create new HMAC instance with key
    fn new(key: &[u8]) -> Self;
    /// Update with data
    fn update(&mut self, data: &[u8]);
    /// Finalize and get MAC
    fn finalize(self) -> Vec<u8>;

    /// One-shot HMAC computation
    fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut hmac = Self::new(key);
        hmac.update(data);
        hmac.finalize()
    }
}

// ============================================================================
// HMAC-SHA384
// ============================================================================

/// HMAC-SHA384
pub struct HmacSha384 {
    inner: Sha384,
    outer_key: Vec<u8>,
}

impl Hmac for HmacSha384 {
    const OUTPUT_SIZE: usize = 48;
    const BLOCK_SIZE: usize = SHA384_BLOCK_SIZE;

    fn new(key: &[u8]) -> Self {
        let mut padded_key = vec![0u8; Self::BLOCK_SIZE];

        if key.len() > Self::BLOCK_SIZE {
            let hash = crate::hash::sha384(key);
            padded_key[..hash.len()].copy_from_slice(&hash);
        } else {
            padded_key[..key.len()].copy_from_slice(key);
        }

        // Inner key = key XOR ipad (0x36)
        let mut inner_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            inner_key[i] = padded_key[i] ^ 0x36;
        }

        // Outer key = key XOR opad (0x5c)
        let mut outer_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            outer_key[i] = padded_key[i] ^ 0x5c;
        }

        let mut inner = Sha384::new();
        inner.update(&inner_key);

        Self { inner, outer_key }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha384::new();
        outer.update(&self.outer_key);
        outer.update(&inner_hash);
        outer.finalize().to_vec()
    }
}

/// Compute HMAC-SHA384
pub fn hmac_sha384(key: &[u8], data: &[u8]) -> [u8; 48] {
    let result = HmacSha384::mac(key, data);
    let mut out = [0u8; 48];
    out.copy_from_slice(&result);
    out
}

// ============================================================================
// HMAC-SHA512
// ============================================================================

/// HMAC-SHA512
pub struct HmacSha512 {
    inner: Sha512,
    outer_key: Vec<u8>,
}

impl Hmac for HmacSha512 {
    const OUTPUT_SIZE: usize = 64;
    const BLOCK_SIZE: usize = SHA512_BLOCK_SIZE;

    fn new(key: &[u8]) -> Self {
        let mut padded_key = vec![0u8; Self::BLOCK_SIZE];

        if key.len() > Self::BLOCK_SIZE {
            let hash = crate::hash::sha512(key);
            padded_key[..hash.len()].copy_from_slice(&hash);
        } else {
            padded_key[..key.len()].copy_from_slice(key);
        }

        let mut inner_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            inner_key[i] = padded_key[i] ^ 0x36;
        }

        let mut outer_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            outer_key[i] = padded_key[i] ^ 0x5c;
        }

        let mut inner = Sha512::new();
        inner.update(&inner_key);

        Self { inner, outer_key }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha512::new();
        outer.update(&self.outer_key);
        outer.update(&inner_hash);
        outer.finalize().to_vec()
    }
}

/// Compute HMAC-SHA512
pub fn hmac_sha512(key: &[u8], data: &[u8]) -> [u8; 64] {
    let result = HmacSha512::mac(key, data);
    let mut out = [0u8; 64];
    out.copy_from_slice(&result);
    out
}

// ============================================================================
// HMAC-SHA3-256
// ============================================================================

/// SHA3-256 block size (rate)
const SHA3_256_BLOCK_SIZE: usize = 136;

/// HMAC-SHA3-256
pub struct HmacSha3_256 {
    inner: Sha3,
    outer_key: Vec<u8>,
}

impl Hmac for HmacSha3_256 {
    const OUTPUT_SIZE: usize = 32;
    const BLOCK_SIZE: usize = SHA3_256_BLOCK_SIZE;

    fn new(key: &[u8]) -> Self {
        let mut padded_key = vec![0u8; Self::BLOCK_SIZE];

        if key.len() > Self::BLOCK_SIZE {
            let hash = crate::sha3::sha3_256(key);
            padded_key[..hash.len()].copy_from_slice(&hash);
        } else {
            padded_key[..key.len()].copy_from_slice(key);
        }

        let mut inner_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            inner_key[i] = padded_key[i] ^ 0x36;
        }

        let mut outer_key = vec![0u8; Self::BLOCK_SIZE];
        for i in 0..Self::BLOCK_SIZE {
            outer_key[i] = padded_key[i] ^ 0x5c;
        }

        let mut inner = Sha3::new_256();
        inner.update(&inner_key);

        Self { inner, outer_key }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(mut self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha3::new_256();
        outer.update(&self.outer_key);
        outer.update(&inner_hash);
        outer.finalize().to_vec()
    }
}

/// Compute HMAC-SHA3-256
pub fn hmac_sha3_256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let result = HmacSha3_256::mac(key, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// HMAC-SHA384 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha384(
    key: *const u8,
    key_len: size_t,
    data: *const u8,
    data_len: size_t,
    output: *mut u8,
) -> c_int {
    if key.is_null() || data.is_null() || output.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, key_len);
    let data_slice = core::slice::from_raw_parts(data, data_len);

    let mac = hmac_sha384(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 48);

    0
}

/// HMAC-SHA512 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha512(
    key: *const u8,
    key_len: size_t,
    data: *const u8,
    data_len: size_t,
    output: *mut u8,
) -> c_int {
    if key.is_null() || data.is_null() || output.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, key_len);
    let data_slice = core::slice::from_raw_parts(data, data_len);

    let mac = hmac_sha512(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 64);

    0
}

/// HMAC-SHA3-256 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha3_256(
    key: *const u8,
    key_len: size_t,
    data: *const u8,
    data_len: size_t,
    output: *mut u8,
) -> c_int {
    if key.is_null() || data.is_null() || output.is_null() {
        return -1;
    }

    let key_slice = core::slice::from_raw_parts(key, key_len);
    let data_slice = core::slice::from_raw_parts(data, data_len);

    let mac = hmac_sha3_256(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 32);

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha384() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = hmac_sha384(key, data);
        assert_eq!(mac.len(), 48);
    }

    #[test]
    fn test_hmac_sha512() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = hmac_sha512(key, data);
        assert_eq!(mac.len(), 64);
    }

    #[test]
    fn test_hmac_sha3_256() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = hmac_sha3_256(key, data);
        assert_eq!(mac.len(), 32);
    }

    #[test]
    fn test_hmac_deterministic() {
        let key = b"secret_key";
        let data = b"test data";

        let mac1 = hmac_sha512(key, data);
        let mac2 = hmac_sha512(key, data);
        assert_eq!(mac1, mac2);
    }
}
