//! scrypt Password-Based Key Derivation Function
//!
//! RFC 7914 compliant scrypt implementation.
//! scrypt is a memory-hard password-based key derivation function.

use std::vec::Vec;

use crate::kdf::pbkdf2_sha256;

// ============================================================================
// Constants
// ============================================================================

/// Default scrypt parameters (moderate security)
pub const SCRYPT_N_DEFAULT: u64 = 16384; // 2^14
pub const SCRYPT_R_DEFAULT: u32 = 8;
pub const SCRYPT_P_DEFAULT: u32 = 1;

/// scrypt parameters for high security (2024+)
pub const SCRYPT_N_HIGH: u64 = 1048576; // 2^20
pub const SCRYPT_R_HIGH: u32 = 8;
pub const SCRYPT_P_HIGH: u32 = 1;

// ============================================================================
// Salsa20/8 Core
// ============================================================================

#[inline]
fn rotl32(x: u32, n: u32) -> u32 {
    x.rotate_left(n)
}

/// Salsa20/8 core function
fn salsa20_8(b: &mut [u8; 64]) {
    let mut x = [0u32; 16];

    // Load from little-endian bytes
    for i in 0..16 {
        x[i] = u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
    }

    let original = x;

    // 8 rounds (4 double rounds)
    for _ in 0..4 {
        // Odd round
        x[4] ^= rotl32(x[0].wrapping_add(x[12]), 7);
        x[8] ^= rotl32(x[4].wrapping_add(x[0]), 9);
        x[12] ^= rotl32(x[8].wrapping_add(x[4]), 13);
        x[0] ^= rotl32(x[12].wrapping_add(x[8]), 18);

        x[9] ^= rotl32(x[5].wrapping_add(x[1]), 7);
        x[13] ^= rotl32(x[9].wrapping_add(x[5]), 9);
        x[1] ^= rotl32(x[13].wrapping_add(x[9]), 13);
        x[5] ^= rotl32(x[1].wrapping_add(x[13]), 18);

        x[14] ^= rotl32(x[10].wrapping_add(x[6]), 7);
        x[2] ^= rotl32(x[14].wrapping_add(x[10]), 9);
        x[6] ^= rotl32(x[2].wrapping_add(x[14]), 13);
        x[10] ^= rotl32(x[6].wrapping_add(x[2]), 18);

        x[3] ^= rotl32(x[15].wrapping_add(x[11]), 7);
        x[7] ^= rotl32(x[3].wrapping_add(x[15]), 9);
        x[11] ^= rotl32(x[7].wrapping_add(x[3]), 13);
        x[15] ^= rotl32(x[11].wrapping_add(x[7]), 18);

        // Even round
        x[1] ^= rotl32(x[0].wrapping_add(x[3]), 7);
        x[2] ^= rotl32(x[1].wrapping_add(x[0]), 9);
        x[3] ^= rotl32(x[2].wrapping_add(x[1]), 13);
        x[0] ^= rotl32(x[3].wrapping_add(x[2]), 18);

        x[6] ^= rotl32(x[5].wrapping_add(x[4]), 7);
        x[7] ^= rotl32(x[6].wrapping_add(x[5]), 9);
        x[4] ^= rotl32(x[7].wrapping_add(x[6]), 13);
        x[5] ^= rotl32(x[4].wrapping_add(x[7]), 18);

        x[11] ^= rotl32(x[10].wrapping_add(x[9]), 7);
        x[8] ^= rotl32(x[11].wrapping_add(x[10]), 9);
        x[9] ^= rotl32(x[8].wrapping_add(x[11]), 13);
        x[10] ^= rotl32(x[9].wrapping_add(x[8]), 18);

        x[12] ^= rotl32(x[15].wrapping_add(x[14]), 7);
        x[13] ^= rotl32(x[12].wrapping_add(x[15]), 9);
        x[14] ^= rotl32(x[13].wrapping_add(x[12]), 13);
        x[15] ^= rotl32(x[14].wrapping_add(x[13]), 18);
    }

    // Add original values
    for i in 0..16 {
        x[i] = x[i].wrapping_add(original[i]);
    }

    // Store to little-endian bytes
    for i in 0..16 {
        let bytes = x[i].to_le_bytes();
        b[i * 4] = bytes[0];
        b[i * 4 + 1] = bytes[1];
        b[i * 4 + 2] = bytes[2];
        b[i * 4 + 3] = bytes[3];
    }
}

// ============================================================================
// Block Mixing Functions
// ============================================================================

/// XOR two blocks
fn xor_blocks(a: &mut [u8], b: &[u8]) {
    for i in 0..a.len() {
        a[i] ^= b[i];
    }
}

/// scryptBlockMix
fn scrypt_block_mix(b: &[u8], r: u32) -> Vec<u8> {
    let block_size = 128 * r as usize;
    let num_blocks = 2 * r as usize;

    // X = B[2r-1]
    let mut x = [0u8; 64];
    x.copy_from_slice(&b[(num_blocks - 1) * 64..num_blocks * 64]);

    let mut y = vec![0u8; block_size];

    for i in 0..num_blocks {
        // T = X XOR B[i]
        let mut t = [0u8; 64];
        t.copy_from_slice(&x);
        xor_blocks(&mut t, &b[i * 64..(i + 1) * 64]);

        // X = Salsa20/8(T)
        salsa20_8(&mut t);
        x = t;

        // Store: even indices go to first half, odd to second half
        let dest_idx = if i % 2 == 0 {
            i / 2
        } else {
            r as usize + i / 2
        };
        y[dest_idx * 64..(dest_idx + 1) * 64].copy_from_slice(&x);
    }

    y
}

/// scryptROMix
fn scrypt_romix(b: &[u8], n: u64, r: u32) -> Vec<u8> {
    let block_size = 128 * r as usize;

    // Allocate V array
    let mut v: Vec<Vec<u8>> = Vec::with_capacity(n as usize);

    let mut x = b.to_vec();

    // Fill V
    for _ in 0..n {
        v.push(x.clone());
        x = scrypt_block_mix(&x, r);
    }

    // Mix
    for _ in 0..n {
        // j = Integerify(X) mod N
        let j_bytes = &x[block_size - 64..block_size - 60];
        let j = u32::from_le_bytes([j_bytes[0], j_bytes[1], j_bytes[2], j_bytes[3]]) as u64 % n;

        // X = BlockMix(X XOR V[j])
        let vj = &v[j as usize];
        for i in 0..block_size {
            x[i] ^= vj[i];
        }
        x = scrypt_block_mix(&x, r);
    }

    x
}

// ============================================================================
// Main scrypt Implementation
// ============================================================================

/// scrypt parameters
#[derive(Clone, Debug)]
pub struct ScryptParams {
    /// CPU/memory cost parameter (must be power of 2)
    pub n: u64,
    /// Block size parameter
    pub r: u32,
    /// Parallelization parameter
    pub p: u32,
}

impl Default for ScryptParams {
    fn default() -> Self {
        Self {
            n: SCRYPT_N_DEFAULT,
            r: SCRYPT_R_DEFAULT,
            p: SCRYPT_P_DEFAULT,
        }
    }
}

impl ScryptParams {
    /// Create new scrypt parameters
    pub fn new(n: u64, r: u32, p: u32) -> Result<Self, &'static str> {
        // n must be power of 2
        if n == 0 || (n & (n - 1)) != 0 {
            return Err("N must be a power of 2");
        }
        if r == 0 {
            return Err("r must be positive");
        }
        if p == 0 {
            return Err("p must be positive");
        }
        // Check for overflow: n * r * 128 must fit in memory
        if n.checked_mul(r as u64)
            .and_then(|x| x.checked_mul(128))
            .is_none()
        {
            return Err("Parameters too large");
        }
        Ok(Self { n, r, p })
    }

    /// Create high-security parameters
    pub fn high_security() -> Self {
        Self {
            n: SCRYPT_N_HIGH,
            r: SCRYPT_R_HIGH,
            p: SCRYPT_P_HIGH,
        }
    }

    /// Create moderate security parameters (faster)
    pub fn moderate() -> Self {
        Self {
            n: SCRYPT_N_DEFAULT,
            r: SCRYPT_R_DEFAULT,
            p: SCRYPT_P_DEFAULT,
        }
    }

    /// Create low security parameters (very fast, for testing)
    pub fn low() -> Self {
        Self {
            n: 1024,
            r: 8,
            p: 1,
        }
    }
}

/// scrypt key derivation function
pub fn scrypt(
    password: &[u8],
    salt: &[u8],
    params: &ScryptParams,
    output_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let n = params.n;
    let r = params.r;
    let p = params.p;

    // Validate parameters
    if n == 0 || (n & (n - 1)) != 0 {
        return Err("N must be a power of 2");
    }
    if r == 0 || p == 0 {
        return Err("r and p must be positive");
    }

    let block_size = 128 * r as usize;

    // B = PBKDF2-SHA256(password, salt, 1, p * 128 * r)
    let b = pbkdf2_sha256(password, salt, 1, p as usize * block_size);

    // Mix each block
    let mut mixed = Vec::with_capacity(p as usize * block_size);
    for i in 0..p as usize {
        let block = &b[i * block_size..(i + 1) * block_size];
        let mixed_block = scrypt_romix(block, n, r);
        mixed.extend_from_slice(&mixed_block);
    }

    // Derive output key
    let output = pbkdf2_sha256(password, &mixed, 1, output_len);

    Ok(output)
}

/// scrypt with default parameters
pub fn scrypt_simple(
    password: &[u8],
    salt: &[u8],
    output_len: usize,
) -> Result<Vec<u8>, &'static str> {
    scrypt(password, salt, &ScryptParams::default(), output_len)
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// scrypt key derivation (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_scrypt(
    password: *const u8,
    password_len: size_t,
    salt: *const u8,
    salt_len: size_t,
    n: u64,
    r: u32,
    p: u32,
    output: *mut u8,
    output_len: size_t,
) -> c_int {
    if password.is_null() || salt.is_null() || output.is_null() {
        return -1;
    }

    let password_slice = core::slice::from_raw_parts(password, password_len);
    let salt_slice = core::slice::from_raw_parts(salt, salt_len);

    let params = match ScryptParams::new(n, r, p) {
        Ok(p) => p,
        Err(_) => return -2,
    };

    match scrypt(password_slice, salt_slice, &params, output_len) {
        Ok(key) => {
            core::ptr::copy_nonoverlapping(key.as_ptr(), output, key.len());
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrypt_basic() {
        let password = b"password";
        let salt = b"NaCl";
        let params = ScryptParams::low();

        let result = scrypt(password, salt, &params, 32);
        assert!(result.is_ok());
        let key = result.unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_scrypt_deterministic() {
        let password = b"test_password";
        let salt = b"test_salt";
        let params = ScryptParams::low();

        let key1 = scrypt(password, salt, &params, 32).unwrap();
        let key2 = scrypt(password, salt, &params, 32).unwrap();

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_scrypt_different_passwords() {
        let salt = b"same_salt";
        let params = ScryptParams::low();

        let key1 = scrypt(b"password1", salt, &params, 32).unwrap();
        let key2 = scrypt(b"password2", salt, &params, 32).unwrap();

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_scrypt_invalid_n() {
        let result = ScryptParams::new(1000, 8, 1); // Not power of 2
        assert!(result.is_err());
    }
}
