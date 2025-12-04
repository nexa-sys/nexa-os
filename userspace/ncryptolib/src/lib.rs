//! NexaOS Cryptographic Library (ncryptolib)
//!
//! A modern, libcrypto.so ABI-compatible cryptographic library for NexaOS.
//! 
//! This library provides commonly used modern cryptographic primitives:
//! - **Hash Functions**: SHA-256, SHA-384, SHA-512
//! - **Symmetric Ciphers**: AES-128/256 in GCM, CTR, CBC modes
//! - **Digital Signatures**: ECDSA (P-256, P-384), Ed25519
//! - **Key Exchange**: X25519 (Curve25519 ECDH)
//! - **Key Derivation**: HKDF, PBKDF2
//! - **Random**: CSPRNG based on getrandom syscall
//!
//! # Design Philosophy
//! - Only modern, secure algorithms (no MD5, SHA-1, DES, RC4, etc.)
//! - Uses std for NexaOS userspace
//! - libcrypto.so ABI compatibility for drop-in replacement
//! - Clean Rust API alongside C ABI exports
//!
//! # Supported Standards
//! - FIPS 180-4 (SHA-2)
//! - FIPS 197 (AES)
//! - SP 800-38D (AES-GCM)
//! - RFC 7748 (X25519)
//! - RFC 8032 (Ed25519)
//! - RFC 5869 (HKDF)
//! - RFC 8018 (PBKDF2)

#![feature(linkage)]

// ============================================================================
// Module Declarations
// ============================================================================

pub mod hash;
pub mod aes;
pub mod ecdsa;
pub mod x25519;
pub mod ed25519;
pub mod kdf;
pub mod random;
pub mod evp;      // OpenSSL EVP compatibility layer
pub mod bigint;   // Big integer arithmetic

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

pub use hash::{sha256, sha384, sha512, Sha256, Sha384, Sha512};
pub use aes::{Aes128, Aes256, AesGcm, AesCtr, AesCbc};
pub use random::{getrandom, RngState};
pub use kdf::{hkdf, pbkdf2_sha256};
