//! NexaOS Cryptographic Library (ncryptolib)
//!
//! A modern, libcrypto.so ABI-compatible cryptographic library for NexaOS.
//! 
//! This library provides commonly used cryptographic primitives:
//!
//! ## Hash Functions
//! - **SHA-2 Family**: SHA-256, SHA-384, SHA-512 (FIPS 180-4)
//! - **SHA-3 Family**: SHA3-256, SHA3-384, SHA3-512, SHAKE128, SHAKE256 (FIPS 202)
//! - **BLAKE2**: BLAKE2b, BLAKE2s - Modern, fast hash functions (RFC 7693)
//! - **Legacy Hashes**: MD5, SHA-1 (for file integrity only, NOT secure!)
//! - **Checksums**: CRC32, CRC32C - Fast non-cryptographic checksums
//!
//! ## Symmetric Ciphers
//! - **AES**: AES-128/256 in GCM, CTR, CBC modes (FIPS 197, SP 800-38D)
//! - **ChaCha20**: ChaCha20 stream cipher (RFC 8439)
//! - **ChaCha20-Poly1305**: AEAD cipher (RFC 8439)
//!
//! ## Asymmetric Cryptography
//! - **RSA**: RSA-2048/3072/4096 with PKCS#1 v1.5 and OAEP (RFC 8017)
//! - **Digital Signatures**: ECDSA (P-256, P-384), Ed25519 (RFC 8032)
//! - **Key Exchange**: X25519 (RFC 7748)
//!
//! ## Key Derivation
//! - **HKDF**: HMAC-based Key Derivation (RFC 5869)
//! - **PBKDF2**: Password-Based Key Derivation (RFC 8018)
//! - **Argon2**: Memory-hard password hashing (RFC 9106)
//! - **scrypt**: Memory-hard key derivation (RFC 7914)
//!
//! ## Message Authentication
//! - **HMAC**: HMAC-SHA256, HMAC-SHA384, HMAC-SHA512, HMAC-SHA3-256
//! - **Poly1305**: One-time authenticator (RFC 8439)
//!
//! ## Encoding
//! - **Base64**: Standard and URL-safe Base64 (RFC 4648)
//! - **Hex**: Hexadecimal encoding/decoding
//!
//! ## Utilities
//! - **Constant-time operations**: Timing-safe comparisons
//! - **Secure zeroization**: Memory wiping
//! - **Random**: CSPRNG based on getrandom syscall
//!
//! # Design Philosophy
//! - Modern, secure algorithms for cryptographic use
//! - Legacy algorithms (MD5, SHA-1) for file verification only
//! - Uses std for NexaOS userspace
//! - libcrypto.so ABI compatibility for drop-in replacement
//! - Clean Rust API alongside C ABI exports
//!
//! # Security Notes
//! **WARNING**: MD5 and SHA-1 are cryptographically broken. They are provided
//! ONLY for file integrity verification and legacy compatibility. For any
//! security-critical application, use SHA-256, SHA-3, or BLAKE2.

#![feature(linkage)]

// ============================================================================
// Module Declarations
// ============================================================================

// Core SHA-2 hash functions (FIPS 180-4)
pub mod hash;

// SHA-3 hash functions (FIPS 202)
pub mod sha3;

// BLAKE2 hash functions (RFC 7693)
pub mod blake2;

// Legacy hash functions (for file verification only)
pub mod md5;
pub mod sha1;

// Checksums (non-cryptographic)
pub mod crc32;

// Symmetric encryption
pub mod aes;
pub mod chacha20;

// Asymmetric cryptography
pub mod ecdsa;
pub mod x25519;
pub mod ed25519;
pub mod rsa;

// Key derivation
pub mod kdf;
pub mod argon2;
pub mod scrypt;

// Message authentication
pub mod hmac;

// Encoding
pub mod encoding;

// Utilities
pub mod constant_time;

// Random number generation
pub mod random;

// OpenSSL EVP compatibility layer
pub mod evp;

// Big integer arithmetic
pub mod bigint;

// ============================================================================
// C Type Definitions
// ============================================================================

pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type c_char = i8;
pub type c_uchar = u8;
pub type size_t = usize;

// ============================================================================
// Error Codes
// ============================================================================

/// Error codes compatible with OpenSSL
pub mod error {
    pub const ERR_R_PASSED_NULL_PARAMETER: i32 = 103;
    pub const ERR_R_MALLOC_FAILURE: i32 = 104;
    pub const ERR_R_SHOULD_NOT_HAVE_BEEN_CALLED: i32 = 105;
    pub const ERR_R_INTERNAL_ERROR: i32 = 111;
    pub const ERR_R_DISABLED: i32 = 114;
}

// ============================================================================
// Library Version Information
// ============================================================================

/// Library version string
pub const NCRYPTO_VERSION: &str = "ncryptolib 1.0.0";

/// OpenSSL-compatible version number (format: 0xMNNFFPPS)
/// M = major, NN = minor, FF = fix, PP = patch, S = status
pub const OPENSSL_VERSION_NUMBER: u64 = 0x30000000; // Mimic OpenSSL 3.0.0

// ============================================================================
// C ABI Exports - Version Functions
// ============================================================================

/// Get library version string (OpenSSL compatible)
#[no_mangle]
pub extern "C" fn OpenSSL_version(t: c_int) -> *const c_char {
    match t {
        0 => b"ncryptolib 1.0.0\0".as_ptr() as *const c_char,
        _ => b"ncryptolib\0".as_ptr() as *const c_char,
    }
}

/// Get library version number (OpenSSL compatible)
#[no_mangle]
pub extern "C" fn OpenSSL_version_num() -> c_ulong {
    OPENSSL_VERSION_NUMBER
}

/// Alias for OpenSSL_version (SSLeay compatibility)
#[no_mangle]
pub extern "C" fn SSLeay_version(t: c_int) -> *const c_char {
    OpenSSL_version(t)
}

/// Alias for OpenSSL_version_num (SSLeay compatibility)
#[no_mangle]
pub extern "C" fn SSLeay() -> c_ulong {
    OpenSSL_version_num()
}

// ============================================================================
// C ABI Exports - Library Initialization
// ============================================================================

/// Initialize the crypto library (no-op for ncryptolib)
#[no_mangle]
pub extern "C" fn OPENSSL_init_crypto(_opts: u64, _settings: *const core::ffi::c_void) -> c_int {
    1 // Success
}

/// Add all algorithms (no-op for ncryptolib)
#[no_mangle]
pub extern "C" fn OpenSSL_add_all_algorithms() {
    // No-op: all algorithms are always available
}

/// Add all ciphers (no-op for ncryptolib)
#[no_mangle]
pub extern "C" fn OpenSSL_add_all_ciphers() {
    // No-op
}

/// Add all digests (no-op for ncryptolib)
#[no_mangle]
pub extern "C" fn OpenSSL_add_all_digests() {
    // No-op
}

/// Cleanup (no-op for ncryptolib)
#[no_mangle]
pub extern "C" fn EVP_cleanup() {
    // No-op
}

/// Cleanup crypto library (no-op)
#[no_mangle]
pub extern "C" fn CRYPTO_cleanup_all_ex_data() {
    // No-op
}

/// Free all error strings (no-op)
#[no_mangle]
pub extern "C" fn ERR_free_strings() {
    // No-op
}

// ============================================================================
// Re-exports
// ============================================================================

// SHA-2 family
pub use hash::{sha256, sha384, sha512, Sha256, Sha384, Sha512};
pub use hash::{hmac_sha256, HmacSha256};

// SHA-3 family
pub use sha3::{sha3_256, sha3_384, sha3_512, Sha3, Sha3_256, Sha3_384, Sha3_512};
pub use sha3::{Shake128, Shake256};

// BLAKE2 family
pub use blake2::{blake2b, blake2s, Blake2b, Blake2s};
pub use blake2::{blake2b_with_len, blake2s_with_len, blake2b_keyed};

// Legacy hashes (file verification only)
pub use md5::{md5, Md5, MD5_DIGEST_SIZE};
pub use sha1::{sha1, Sha1, SHA1_DIGEST_SIZE};

// Checksums
pub use crc32::{crc32, crc32c, Crc32, Crc32c};

// Symmetric encryption
pub use aes::{Aes128, Aes256, AesGcm, AesCtr, AesCbc};
pub use chacha20::{ChaCha20, ChaCha20Poly1305, Poly1305};
pub use chacha20::{chacha20_encrypt, chacha20_decrypt};
pub use chacha20::{chacha20_poly1305_encrypt, chacha20_poly1305_decrypt};

// Asymmetric cryptography
pub use rsa::{RsaPublicKey, RsaPrivateKey};
pub use rsa::{rsa_encrypt, rsa_decrypt, rsa_sign, rsa_verify};
pub use rsa::{rsa_oaep_encrypt, rsa_oaep_decrypt, generate_keypair};

// Random
pub use random::{getrandom, RngState};

// Key derivation
pub use kdf::{hkdf, pbkdf2_sha256};
pub use argon2::{argon2, argon2id, argon2i, argon2d, Argon2Params, Argon2Variant};
pub use scrypt::{scrypt, scrypt_simple, ScryptParams};

// Message authentication
pub use hmac::{hmac_sha384, hmac_sha512, hmac_sha3_256};
pub use hmac::{HmacSha384, HmacSha512, HmacSha3_256, Hmac};

// Encoding
pub use encoding::{base64_encode, base64_decode, base64url_encode, base64url_decode};
pub use encoding::{hex_encode, hex_decode};

// Constant-time utilities
pub use constant_time::{ct_eq, secure_zero};
