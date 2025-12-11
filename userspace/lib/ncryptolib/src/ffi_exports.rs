//! C ABI Exports for nssl and ntcp2 Dynamic Linking
//!
//! This module provides additional C ABI functions that nssl and ntcp2 require
//! for dynamic linking against libcrypto.so with stable ABI.

use crate::aes::{Aes128, Aes256, AesGcm, GCM_TAG_SIZE};
use crate::chacha20::{ChaCha20, ChaCha20Poly1305};
use crate::hash::{hmac_sha256, SHA256_DIGEST_SIZE};
use crate::kdf::{hkdf_expand_label as rust_hkdf_expand_label, hkdf_extract_sha256 as rust_hkdf_extract_sha256};
use crate::p256::{P256KeyPair, P256Point};
use crate::random::getrandom as sys_getrandom;
use crate::{c_int, c_uint, size_t};

// ============================================================================
// HMAC-SHA256 C ABI (missing from hmac.rs)
// ============================================================================

/// HMAC-SHA256 (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hmac_sha256(
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

    let mac = hmac_sha256(key_slice, data_slice);
    core::ptr::copy_nonoverlapping(mac.as_ptr(), output, 32);

    0
}

// ============================================================================
// Random Number Generation
// ============================================================================

/// Get random bytes from kernel (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_getrandom(buf: *mut u8, len: size_t, flags: c_uint) -> c_int {
    if buf.is_null() {
        return -1;
    }

    let buf_slice = core::slice::from_raw_parts_mut(buf, len);
    match sys_getrandom(buf_slice, flags) {
        Ok(n) => n as c_int,
        Err(e) => e,
    }
}

// ============================================================================
// P-256 Operations
// ============================================================================

/// Generate P-256 key pair (C ABI)
///
/// # Arguments
/// * `private_key` - Output buffer for 32-byte private key
/// * `public_key` - Output buffer for 65-byte uncompressed public key (04 || x || y)
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_p256_keygen(private_key: *mut u8, public_key: *mut u8) -> c_int {
    if private_key.is_null() || public_key.is_null() {
        return 0;
    }

    match P256KeyPair::generate() {
        Some(keypair) => {
            core::ptr::copy_nonoverlapping(keypair.private_key.as_ptr(), private_key, 32);
            let pubkey = keypair.public_key_uncompressed();
            core::ptr::copy_nonoverlapping(pubkey.as_ptr(), public_key, 65);
            1
        }
        None => 0,
    }
}

/// P-256 ECDH shared secret computation (C ABI)
///
/// # Arguments
/// * `shared_secret` - Output buffer for 32-byte shared secret
/// * `private_key` - 32-byte private key
/// * `peer_public_key` - Peer's public key (uncompressed format)
/// * `peer_public_key_len` - Length of peer's public key (65 for uncompressed)
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_p256_ecdh(
    shared_secret: *mut u8,
    private_key: *const u8,
    peer_public_key: *const u8,
    peer_public_key_len: size_t,
) -> c_int {
    if shared_secret.is_null() || private_key.is_null() || peer_public_key.is_null() {
        return 0;
    }

    let mut priv_arr = [0u8; 32];
    core::ptr::copy_nonoverlapping(private_key, priv_arr.as_mut_ptr(), 32);

    let peer_slice = core::slice::from_raw_parts(peer_public_key, peer_public_key_len);

    // Parse peer public key
    let peer_point = if peer_public_key_len == 65 && peer_slice[0] == 0x04 {
        match P256Point::from_uncompressed(peer_slice) {
            Some(p) => p,
            None => return 0,
        }
    } else if peer_public_key_len == 64 {
        // Raw x || y format
        let mut uncompressed = [0u8; 65];
        uncompressed[0] = 0x04;
        uncompressed[1..].copy_from_slice(peer_slice);
        match P256Point::from_uncompressed(&uncompressed) {
            Some(p) => p,
            None => return 0,
        }
    } else {
        return 0;
    };

    // Create keypair and compute ECDH
    match P256KeyPair::from_private_key(&priv_arr) {
        Some(keypair) => match keypair.ecdh(&peer_point) {
            Some(secret) => {
                core::ptr::copy_nonoverlapping(secret.as_ptr(), shared_secret, 32);
                1
            }
            None => 0,
        },
        None => 0,
    }
}

/// Parse P-256 uncompressed public key point (C ABI)
///
/// # Arguments
/// * `x` - Output buffer for 32-byte x coordinate
/// * `y` - Output buffer for 32-byte y coordinate
/// * `data` - Input uncompressed point (04 || x || y)
/// * `len` - Length of input (must be 65)
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_p256_point_from_uncompressed(
    x: *mut u8,
    y: *mut u8,
    data: *const u8,
    len: size_t,
) -> c_int {
    if x.is_null() || y.is_null() || data.is_null() || len != 65 {
        return 0;
    }

    let data_slice = core::slice::from_raw_parts(data, len);
    if data_slice[0] != 0x04 {
        return 0;
    }

    match P256Point::from_uncompressed(data_slice) {
        Some(point) => {
            let uncompressed = point.to_uncompressed();
            // uncompressed is 04 || x || y
            core::ptr::copy_nonoverlapping(uncompressed[1..33].as_ptr(), x, 32);
            core::ptr::copy_nonoverlapping(uncompressed[33..65].as_ptr(), y, 32);
            1
        }
        None => 0,
    }
}

/// P-256 ECDSA signature verification (C ABI)
///
/// # Arguments
/// * `message_hash` - Message hash to verify
/// * `hash_len` - Length of message hash
/// * `signature` - Signature (DER-encoded or raw r||s format)
/// * `sig_len` - Length of signature (use 64 for raw r||s, otherwise DER)
/// * `public_key` - Public key (uncompressed format)
/// * `pubkey_len` - Length of public key
///
/// # Returns
/// 1 if signature is valid, 0 if invalid
#[no_mangle]
pub unsafe extern "C" fn ncrypto_p256_verify(
    message_hash: *const u8,
    hash_len: size_t,
    signature: *const u8,
    sig_len: size_t,
    public_key: *const u8,
    pubkey_len: size_t,
) -> c_int {
    if message_hash.is_null() || signature.is_null() || public_key.is_null() {
        return 0;
    }

    let hash_slice = core::slice::from_raw_parts(message_hash, hash_len);
    let sig_slice = core::slice::from_raw_parts(signature, sig_len);
    let pubkey_slice = core::slice::from_raw_parts(public_key, pubkey_len);

    // Parse public key
    let point = match P256Point::from_uncompressed(pubkey_slice) {
        Some(p) => p,
        None => return 0,
    };

    // Parse signature - support both raw r||s and DER formats
    let sig = if sig_len == 64 {
        // Raw r || s format
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_slice[..32]);
        s.copy_from_slice(&sig_slice[32..]);
        crate::p256::P256Signature { r, s }
    } else {
        // DER format
        match crate::p256::P256Signature::from_der(sig_slice) {
            Some(s) => s,
            None => return 0,
        }
    };

    // Verify
    if sig.verify(&point, hash_slice) {
        1
    } else {
        0
    }
}

// ============================================================================
// AES-GCM Operations
// ============================================================================

/// AES-128-GCM encryption (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_aes128_gcm_encrypt(
    key: *const u8,
    nonce: *const u8,
    nonce_len: size_t,
    plaintext: *const u8,
    plaintext_len: size_t,
    aad: *const u8,
    aad_len: size_t,
    ciphertext: *mut u8,
    tag: *mut u8,
) -> c_int {
    if key.is_null() || nonce.is_null() || ciphertext.is_null() || tag.is_null() {
        return 0;
    }

    let key_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 16);
        arr
    };

    let nonce_slice = core::slice::from_raw_parts(nonce, nonce_len);
    let plaintext_slice = if plaintext.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(plaintext, plaintext_len)
    };
    let aad_slice = if aad.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(aad, aad_len)
    };

    let aes = AesGcm::new_128(&key_arr);
    let (ct, t) = aes.encrypt(nonce_slice, plaintext_slice, aad_slice);
    core::ptr::copy_nonoverlapping(ct.as_ptr(), ciphertext, ct.len());
    core::ptr::copy_nonoverlapping(t.as_ptr(), tag, GCM_TAG_SIZE);
    1
}

/// AES-128-GCM decryption (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_aes128_gcm_decrypt(
    key: *const u8,
    nonce: *const u8,
    nonce_len: size_t,
    ciphertext: *const u8,
    ciphertext_len: size_t,
    aad: *const u8,
    aad_len: size_t,
    tag: *const u8,
    plaintext: *mut u8,
) -> c_int {
    if key.is_null() || nonce.is_null() || tag.is_null() || plaintext.is_null() {
        return 0;
    }

    let key_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 16);
        arr
    };

    let nonce_slice = core::slice::from_raw_parts(nonce, nonce_len);
    let ciphertext_slice = if ciphertext.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(ciphertext, ciphertext_len)
    };
    let aad_slice = if aad.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(aad, aad_len)
    };

    let tag_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(tag, arr.as_mut_ptr(), 16);
        arr
    };

    let aes = AesGcm::new_128(&key_arr);
    match aes.decrypt(nonce_slice, ciphertext_slice, aad_slice, &tag_arr) {
        Some(pt) => {
            core::ptr::copy_nonoverlapping(pt.as_ptr(), plaintext, pt.len());
            1
        }
        None => 0,
    }
}

/// AES-256-GCM encryption (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_aes256_gcm_encrypt(
    key: *const u8,
    nonce: *const u8,
    nonce_len: size_t,
    plaintext: *const u8,
    plaintext_len: size_t,
    aad: *const u8,
    aad_len: size_t,
    ciphertext: *mut u8,
    tag: *mut u8,
) -> c_int {
    if key.is_null() || nonce.is_null() || ciphertext.is_null() || tag.is_null() {
        return 0;
    }

    let key_arr: [u8; 32] = {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 32);
        arr
    };

    let nonce_slice = core::slice::from_raw_parts(nonce, nonce_len);
    let plaintext_slice = if plaintext.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(plaintext, plaintext_len)
    };
    let aad_slice = if aad.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(aad, aad_len)
    };

    let aes = AesGcm::new_256(&key_arr);
    let (ct, t) = aes.encrypt(nonce_slice, plaintext_slice, aad_slice);
    core::ptr::copy_nonoverlapping(ct.as_ptr(), ciphertext, ct.len());
    core::ptr::copy_nonoverlapping(t.as_ptr(), tag, GCM_TAG_SIZE);
    1
}

/// AES-256-GCM decryption (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_aes256_gcm_decrypt(
    key: *const u8,
    nonce: *const u8,
    nonce_len: size_t,
    ciphertext: *const u8,
    ciphertext_len: size_t,
    aad: *const u8,
    aad_len: size_t,
    tag: *const u8,
    plaintext: *mut u8,
) -> c_int {
    if key.is_null() || nonce.is_null() || tag.is_null() || plaintext.is_null() {
        return 0;
    }

    let key_arr: [u8; 32] = {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 32);
        arr
    };

    let nonce_slice = core::slice::from_raw_parts(nonce, nonce_len);
    let ciphertext_slice = if ciphertext.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(ciphertext, ciphertext_len)
    };
    let aad_slice = if aad.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(aad, aad_len)
    };

    let tag_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(tag, arr.as_mut_ptr(), 16);
        arr
    };

    let aes = AesGcm::new_256(&key_arr);
    match aes.decrypt(nonce_slice, ciphertext_slice, aad_slice, &tag_arr) {
        Some(pt) => {
            core::ptr::copy_nonoverlapping(pt.as_ptr(), plaintext, pt.len());
            1
        }
        None => 0,
    }
}

// Note: RSA C ABI functions are already defined in rsa.rs:
// - ncrypto_rsa_verify
// - ncrypto_rsa_pss_verify

// ============================================================================
// HKDF Functions (for ntcp2 QUIC crypto)
// ============================================================================

/// HKDF-Extract using SHA-256 (C ABI)
///
/// # Arguments
/// * `salt` - Salt value (can be NULL for no salt)
/// * `salt_len` - Length of salt
/// * `ikm` - Input keying material
/// * `ikm_len` - Length of IKM
/// * `prk` - Output buffer for 32-byte PRK
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hkdf_extract_sha256(
    salt: *const u8,
    salt_len: size_t,
    ikm: *const u8,
    ikm_len: size_t,
    prk: *mut u8,
) -> c_int {
    if ikm.is_null() || prk.is_null() {
        return 0;
    }

    let salt_slice = if salt.is_null() || salt_len == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(salt, salt_len)
    };
    let ikm_slice = core::slice::from_raw_parts(ikm, ikm_len);

    let result = rust_hkdf_extract_sha256(salt_slice, ikm_slice);
    core::ptr::copy_nonoverlapping(result.as_ptr(), prk, SHA256_DIGEST_SIZE);
    1
}

/// HKDF-Expand-Label using SHA-256 (TLS 1.3 style) (C ABI)
///
/// # Arguments
/// * `secret` - Secret value
/// * `secret_len` - Length of secret (padded/truncated to 32 bytes internally)
/// * `label` - Label bytes
/// * `label_len` - Length of label
/// * `context` - Context bytes
/// * `context_len` - Length of context
/// * `out` - Output buffer
/// * `out_len` - Desired output length
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_hkdf_expand_label_sha256(
    secret: *const u8,
    secret_len: size_t,
    label: *const u8,
    label_len: size_t,
    context: *const u8,
    context_len: size_t,
    out: *mut u8,
    out_len: size_t,
) -> c_int {
    if secret.is_null() || out.is_null() {
        return 0;
    }

    // Convert secret to fixed 32-byte array (pad or truncate)
    let mut fixed_secret = [0u8; SHA256_DIGEST_SIZE];
    let secret_slice = core::slice::from_raw_parts(secret, secret_len);
    let copy_len = secret_len.min(SHA256_DIGEST_SIZE);
    fixed_secret[..copy_len].copy_from_slice(&secret_slice[..copy_len]);

    let label_slice = if label.is_null() || label_len == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(label, label_len)
    };

    let context_slice = if context.is_null() || context_len == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(context, context_len)
    };

    let result = rust_hkdf_expand_label(&fixed_secret, label_slice, context_slice, out_len);
    core::ptr::copy_nonoverlapping(result.as_ptr(), out, result.len());
    1
}

// ============================================================================
// AES-ECB Operations (for QUIC header protection)
// ============================================================================

/// AES-128-ECB single block encryption (C ABI)
///
/// Encrypts a single 16-byte block using AES-128-ECB.
/// Used for QUIC header protection with AES-128-GCM cipher suites.
///
/// # Arguments
/// * `key` - 16-byte AES key
/// * `input` - 16-byte plaintext block
/// * `output` - 16-byte output buffer for ciphertext
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_aes128_ecb_encrypt(
    key: *const u8,
    input: *const u8,
    output: *mut u8,
) -> c_int {
    if key.is_null() || input.is_null() || output.is_null() {
        return 0;
    }

    let key_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 16);
        arr
    };

    let input_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(input, arr.as_mut_ptr(), 16);
        arr
    };

    let aes = Aes128::new(&key_arr);
    let encrypted = aes.encrypt_block(&input_arr);
    core::ptr::copy_nonoverlapping(encrypted.as_ptr(), output, 16);
    1
}

/// AES-256-ECB single block encryption (C ABI)
///
/// Encrypts a single 16-byte block using AES-256-ECB.
/// Used for QUIC header protection with AES-256-GCM cipher suites.
///
/// # Arguments
/// * `key` - 32-byte AES key
/// * `input` - 16-byte plaintext block
/// * `output` - 16-byte output buffer for ciphertext
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_aes256_ecb_encrypt(
    key: *const u8,
    input: *const u8,
    output: *mut u8,
) -> c_int {
    if key.is_null() || input.is_null() || output.is_null() {
        return 0;
    }

    let key_arr: [u8; 32] = {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 32);
        arr
    };

    let input_arr: [u8; 16] = {
        let mut arr = [0u8; 16];
        core::ptr::copy_nonoverlapping(input, arr.as_mut_ptr(), 16);
        arr
    };

    let aes = Aes256::new(&key_arr);
    let encrypted = aes.encrypt_block(&input_arr);
    core::ptr::copy_nonoverlapping(encrypted.as_ptr(), output, 16);
    1
}

// ============================================================================
// ChaCha20 Operations (for ntcp2 QUIC header protection)
// Note: ChaCha20-Poly1305 encrypt/decrypt are already exported in chacha20.rs
// ============================================================================

/// ChaCha20 block for header protection (C ABI)
///
/// Generates ChaCha20 keystream for QUIC header protection.
///
/// # Arguments
/// * `key` - 32-byte key
/// * `counter` - 32-bit counter (from sample[0..4])
/// * `nonce` - 12-byte nonce (from sample[4..16])
/// * `output` - Output buffer for keystream
/// * `output_len` - Desired output length
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "C" fn ncrypto_chacha20_block(
    key: *const u8,
    counter: u32,
    nonce: *const u8,
    output: *mut u8,
    output_len: size_t,
) -> c_int {
    if key.is_null() || nonce.is_null() || output.is_null() {
        return 0;
    }

    let key_arr: [u8; 32] = {
        let mut arr = [0u8; 32];
        core::ptr::copy_nonoverlapping(key, arr.as_mut_ptr(), 32);
        arr
    };

    let nonce_arr: [u8; 12] = {
        let mut arr = [0u8; 12];
        core::ptr::copy_nonoverlapping(nonce, arr.as_mut_ptr(), 12);
        arr
    };

    // Use ChaCha20 to generate keystream
    let mut chacha = ChaCha20::new(&key_arr, &nonce_arr);
    chacha.set_counter(counter);

    // Generate keystream by XORing with zeros
    let output_slice = core::slice::from_raw_parts_mut(output, output_len);
    output_slice.fill(0);
    chacha.apply_keystream(output_slice);

    1
}

// Note: Constant-time operations (ncrypto_ct_eq, ncrypto_secure_zero) are already
// exported in constant_time.rs
