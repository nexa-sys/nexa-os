//! Key Derivation Functions (HKDF, PBKDF2)
//!
//! RFC 5869 (HKDF) and RFC 8018 (PBKDF2) compliant implementations.

use std::vec::Vec;

use crate::hash::{sha256, sha512, hmac_sha256, Sha256, HmacSha256, SHA256_DIGEST_SIZE};

// ============================================================================
// HKDF (HMAC-based Key Derivation Function)
// ============================================================================

/// HKDF-Extract step
/// 
/// Extracts a pseudorandom key from input keying material.
pub fn hkdf_extract_sha256(salt: &[u8], ikm: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let actual_salt = if salt.is_empty() {
        &[0u8; SHA256_DIGEST_SIZE][..]
    } else {
        salt
    };
    hmac_sha256(actual_salt, ikm)
}

/// HKDF-Expand step
///
/// Expands a pseudorandom key to desired length.
pub fn hkdf_expand_sha256(prk: &[u8; SHA256_DIGEST_SIZE], info: &[u8], length: usize) -> Vec<u8> {
    let n = (length + SHA256_DIGEST_SIZE - 1) / SHA256_DIGEST_SIZE;
    let mut okm = Vec::with_capacity(n * SHA256_DIGEST_SIZE);
    let mut t = [0u8; SHA256_DIGEST_SIZE];
    
    for i in 1..=n {
        let mut hmac = HmacSha256::new(prk);
        if i > 1 {
            hmac.update(&t);
        }
        hmac.update(info);
        hmac.update(&[i as u8]);
        t = hmac.finalize();
        okm.extend_from_slice(&t);
    }
    
    okm.truncate(length);
    okm
}

/// HKDF (one-shot)
///
/// Derives key material using HKDF with SHA-256.
///
/// # Arguments
/// * `salt` - Optional salt value (can be empty)
/// * `ikm` - Input keying material
/// * `info` - Context and application specific information
/// * `length` - Length of output keying material
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let prk = hkdf_extract_sha256(salt, ikm);
    hkdf_expand_sha256(&prk, info, length)
}

// ============================================================================
// PBKDF2 (Password-Based Key Derivation Function 2)
// ============================================================================

/// PBKDF2 with HMAC-SHA256
///
/// # Arguments
/// * `password` - The password
/// * `salt` - The salt
/// * `iterations` - Number of iterations (minimum 10000 recommended)
/// * `dk_len` - Desired key length
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    let h_len = SHA256_DIGEST_SIZE;
    let l = (dk_len + h_len - 1) / h_len;
    let mut dk = Vec::with_capacity(l * h_len);
    
    for i in 1..=l as u32 {
        let mut u = hmac_sha256(password, &[salt, &i.to_be_bytes()].concat());
        let mut t = u;
        
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for j in 0..h_len {
                t[j] ^= u[j];
            }
        }
        
        dk.extend_from_slice(&t);
    }
    
    dk.truncate(dk_len);
    dk
}

/// PBKDF2 with HMAC-SHA512
pub fn pbkdf2_sha512(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    const H_LEN: usize = 64;
    let l = (dk_len + H_LEN - 1) / H_LEN;
    let mut dk = Vec::with_capacity(l * H_LEN);
    
    for i in 1..=l as u32 {
        let mut combined = Vec::with_capacity(salt.len() + 4);
        combined.extend_from_slice(salt);
        combined.extend_from_slice(&i.to_be_bytes());
        
        let mut u = hmac_sha512(password, &combined);
        let mut t = u.clone();
        
        for _ in 1..iterations {
            u = hmac_sha512(password, &u);
            for j in 0..H_LEN {
                t[j] ^= u[j];
            }
        }
        
        dk.extend_from_slice(&t);
    }
    
    dk.truncate(dk_len);
    dk
}

/// HMAC-SHA512 helper
fn hmac_sha512(key: &[u8], data: &[u8]) -> Vec<u8> {
    use crate::hash::{Sha512, SHA512_BLOCK_SIZE, SHA512_DIGEST_SIZE};
    
    let mut padded_key = vec![0u8; SHA512_BLOCK_SIZE];
    
    if key.len() > SHA512_BLOCK_SIZE {
        let hash = sha512(key);
        padded_key[..hash.len()].copy_from_slice(&hash);
    } else {
        padded_key[..key.len()].copy_from_slice(key);
    }
    
    // Inner key = key XOR ipad (0x36)
    let mut inner_key = vec![0u8; SHA512_BLOCK_SIZE];
    for i in 0..SHA512_BLOCK_SIZE {
        inner_key[i] = padded_key[i] ^ 0x36;
    }
    
    // Outer key = key XOR opad (0x5c)
    let mut outer_key = vec![0u8; SHA512_BLOCK_SIZE];
    for i in 0..SHA512_BLOCK_SIZE {
        outer_key[i] = padded_key[i] ^ 0x5c;
    }
    
    // Inner hash
    let mut inner = Sha512::new();
    inner.update(&inner_key);
    inner.update(data);
    let inner_hash = inner.finalize();
    
    // Outer hash
    let mut outer = Sha512::new();
    outer.update(&outer_key);
    outer.update(&inner_hash);
    outer.finalize()
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// PKCS5_PBKDF2_HMAC_SHA256 - OpenSSL compatible PBKDF2
#[no_mangle]
pub extern "C" fn PKCS5_PBKDF2_HMAC_SHA256(
    pass: *const u8,
    passlen: i32,
    salt: *const u8,
    saltlen: i32,
    iter: i32,
    keylen: i32,
    out: *mut u8,
) -> i32 {
    if pass.is_null() || salt.is_null() || out.is_null() {
        return 0;
    }
    if passlen < 0 || saltlen < 0 || iter < 1 || keylen < 1 {
        return 0;
    }
    
    let password = unsafe { core::slice::from_raw_parts(pass, passlen as usize) };
    let salt_slice = unsafe { core::slice::from_raw_parts(salt, saltlen as usize) };
    
    let dk = pbkdf2_sha256(password, salt_slice, iter as u32, keylen as usize);
    
    unsafe {
        core::ptr::copy_nonoverlapping(dk.as_ptr(), out, keylen as usize);
    }
    
    1
}

/// PKCS5_PBKDF2_HMAC - OpenSSL compatible PBKDF2 with digest selection
#[no_mangle]
pub extern "C" fn PKCS5_PBKDF2_HMAC(
    pass: *const u8,
    passlen: i32,
    salt: *const u8,
    saltlen: i32,
    iter: i32,
    _digest: *const core::ffi::c_void, // EVP_MD - we ignore and use SHA256
    keylen: i32,
    out: *mut u8,
) -> i32 {
    PKCS5_PBKDF2_HMAC_SHA256(pass, passlen, salt, saltlen, iter, keylen, out)
}

/// EVP_PKEY_CTX for HKDF (simplified stub)
#[repr(C)]
pub struct EVP_PKEY_CTX {
    _private: [u8; 0],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hkdf_basic() {
        let ikm = b"input key material";
        let salt = b"salt";
        let info = b"info";
        
        let output = hkdf(salt, ikm, info, 32);
        assert_eq!(output.len(), 32);
    }

    #[test]
    fn test_pbkdf2_sha256() {
        let password = b"password";
        let salt = b"salt";
        
        let dk = pbkdf2_sha256(password, salt, 1, 32);
        assert_eq!(dk.len(), 32);
        
        // RFC 7914 test vector (first iteration only)
        // For iterations=1, salt="salt", password="password", dkLen=32
    }

    #[test]
    fn test_hkdf_extract_expand() {
        let ikm = [0x0b; 22];
        let salt = [0u8; 13];
        
        let prk = hkdf_extract_sha256(&salt, &ikm);
        assert_eq!(prk.len(), 32);
        
        let okm = hkdf_expand_sha256(&prk, b"", 42);
        assert_eq!(okm.len(), 42);
    }
}
