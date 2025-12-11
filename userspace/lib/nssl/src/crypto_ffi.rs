//! FFI bindings to ncryptolib (libcrypto.so)
//!
//! This module provides C ABI function declarations for dynamically linking
//! against ncryptolib's libcrypto.so. This ensures ABI stability across
//! library versions.

use core::ffi::c_void;

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
// FFI Function Declarations
// ============================================================================

#[link(name = "crypto")]
extern "C" {
    // ========================================================================
    // Version Functions
    // ========================================================================

    pub fn OpenSSL_version(t: c_int) -> *const c_char;
    pub fn OpenSSL_version_num() -> c_ulong;
    pub fn OPENSSL_init_crypto(opts: u64, settings: *const c_void) -> c_int;

    // ========================================================================
    // Hash Functions - SHA-256
    // ========================================================================

    /// One-shot SHA-256 hash
    pub fn SHA256(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;

    /// Initialize SHA-256 context
    pub fn SHA256_Init(ctx: *mut SHA256_CTX) -> c_int;

    /// Update SHA-256 context with data
    pub fn SHA256_Update(ctx: *mut SHA256_CTX, data: *const u8, len: size_t) -> c_int;

    /// Finalize SHA-256 and get digest
    pub fn SHA256_Final(md: *mut u8, ctx: *mut SHA256_CTX) -> c_int;

    // ========================================================================
    // Hash Functions - SHA-384
    // ========================================================================

    /// One-shot SHA-384 hash
    pub fn SHA384(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;

    // ========================================================================
    // Hash Functions - SHA-512
    // ========================================================================

    /// One-shot SHA-512 hash
    pub fn SHA512(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;

    // ========================================================================
    // Hash Functions - SHA-3
    // ========================================================================

    pub fn SHA3_256(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;
    pub fn SHA3_384(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;
    pub fn SHA3_512(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;

    // ========================================================================
    // HMAC Functions
    // ========================================================================

    /// HMAC-SHA256
    pub fn ncrypto_hmac_sha256(
        key: *const u8,
        key_len: size_t,
        data: *const u8,
        data_len: size_t,
        out: *mut u8,
    ) -> c_int;

    /// HMAC-SHA384
    pub fn ncrypto_hmac_sha384(
        key: *const u8,
        key_len: size_t,
        data: *const u8,
        data_len: size_t,
        out: *mut u8,
    ) -> c_int;

    /// HMAC-SHA512
    pub fn ncrypto_hmac_sha512(
        key: *const u8,
        key_len: size_t,
        data: *const u8,
        data_len: size_t,
        out: *mut u8,
    ) -> c_int;

    // ========================================================================
    // X25519 Key Exchange
    // ========================================================================

    /// X25519 ECDH key exchange
    pub fn X25519(out: *mut u8, private_key: *const u8, public_key: *const u8) -> c_int;

    /// Derive X25519 public key from private key
    pub fn X25519_public_from_private(out: *mut u8, private_key: *const u8) -> c_int;

    // ========================================================================
    // P-256 (secp256r1) Operations
    // ========================================================================

    /// Generate P-256 key pair
    pub fn ncrypto_p256_keygen(private_key: *mut u8, public_key: *mut u8) -> c_int;

    /// P-256 ECDH shared secret computation
    pub fn ncrypto_p256_ecdh(
        shared_secret: *mut u8,
        private_key: *const u8,
        peer_public_key: *const u8,
        peer_public_key_len: size_t,
    ) -> c_int;

    /// Parse P-256 uncompressed public key point
    pub fn ncrypto_p256_point_from_uncompressed(
        x: *mut u8,
        y: *mut u8,
        data: *const u8,
        len: size_t,
    ) -> c_int;

    /// P-256 ECDSA signature verification
    pub fn ncrypto_p256_verify(
        message_hash: *const u8,
        hash_len: size_t,
        signature: *const u8,
        sig_len: size_t,
        public_key: *const u8,
        pubkey_len: size_t,
    ) -> c_int;

    // ========================================================================
    // AES-GCM Encryption
    // ========================================================================

    /// AES-128-GCM encryption
    pub fn ncrypto_aes128_gcm_encrypt(
        key: *const u8,
        nonce: *const u8,
        nonce_len: size_t,
        plaintext: *const u8,
        plaintext_len: size_t,
        aad: *const u8,
        aad_len: size_t,
        ciphertext: *mut u8,
        tag: *mut u8,
    ) -> c_int;

    /// AES-128-GCM decryption
    pub fn ncrypto_aes128_gcm_decrypt(
        key: *const u8,
        nonce: *const u8,
        nonce_len: size_t,
        ciphertext: *const u8,
        ciphertext_len: size_t,
        aad: *const u8,
        aad_len: size_t,
        tag: *const u8,
        plaintext: *mut u8,
    ) -> c_int;

    /// AES-256-GCM encryption
    pub fn ncrypto_aes256_gcm_encrypt(
        key: *const u8,
        nonce: *const u8,
        nonce_len: size_t,
        plaintext: *const u8,
        plaintext_len: size_t,
        aad: *const u8,
        aad_len: size_t,
        ciphertext: *mut u8,
        tag: *mut u8,
    ) -> c_int;

    /// AES-256-GCM decryption
    pub fn ncrypto_aes256_gcm_decrypt(
        key: *const u8,
        nonce: *const u8,
        nonce_len: size_t,
        ciphertext: *const u8,
        ciphertext_len: size_t,
        aad: *const u8,
        aad_len: size_t,
        tag: *const u8,
        plaintext: *mut u8,
    ) -> c_int;

    // ========================================================================
    // ChaCha20-Poly1305
    // ========================================================================

    pub fn ncrypto_chacha20_poly1305_encrypt(
        key: *const u8,
        nonce: *const u8,
        plaintext: *const u8,
        plaintext_len: size_t,
        aad: *const u8,
        aad_len: size_t,
        ciphertext: *mut u8,
        tag: *mut u8,
    ) -> c_int;

    pub fn ncrypto_chacha20_poly1305_decrypt(
        key: *const u8,
        nonce: *const u8,
        ciphertext: *const u8,
        ciphertext_len: size_t,
        aad: *const u8,
        aad_len: size_t,
        tag: *const u8,
        plaintext: *mut u8,
    ) -> c_int;

    // ========================================================================
    // Random Number Generation
    // ========================================================================

    /// Get random bytes from kernel
    pub fn ncrypto_getrandom(buf: *mut u8, len: size_t, flags: c_uint) -> c_int;

    // ========================================================================
    // RSA Operations
    // ========================================================================

    /// RSA PKCS#1 v1.5 signature verification
    pub fn ncrypto_rsa_verify(
        message: *const u8,
        message_len: size_t,
        signature: *const u8,
        sig_len: size_t,
        n: *const u8,
        n_len: size_t,
        e: *const u8,
        e_len: size_t,
    ) -> c_int;

    /// RSA-PSS signature verification
    pub fn ncrypto_rsa_pss_verify(
        message: *const u8,
        message_len: size_t,
        signature: *const u8,
        sig_len: size_t,
        n: *const u8,
        n_len: size_t,
        e: *const u8,
        e_len: size_t,
    ) -> c_int;

    // ========================================================================
    // Base64 Encoding
    // ========================================================================

    /// Base64 encode
    pub fn ncrypto_base64_encode(
        input: *const u8,
        input_len: size_t,
        output: *mut u8,
        output_len: *mut size_t,
    ) -> c_int;

    /// Base64 decode
    pub fn ncrypto_base64_decode(
        input: *const u8,
        input_len: size_t,
        output: *mut u8,
        output_len: *mut size_t,
    ) -> c_int;

    // ========================================================================
    // Constant-time Operations
    // ========================================================================

    /// Constant-time memory comparison
    pub fn ncrypto_ct_eq(a: *const u8, b: *const u8, len: size_t) -> c_int;

    /// Secure memory zeroing
    pub fn ncrypto_secure_zero(ptr: *mut u8, len: size_t);

    // ========================================================================
    // Error Handling Functions (ERR_*)
    // ========================================================================

    /// Get and remove first error from error queue
    pub fn ERR_get_error() -> c_ulong;

    /// Peek at first error without removing
    pub fn ERR_peek_error() -> c_ulong;

    /// Peek at last error without removing
    pub fn ERR_peek_last_error() -> c_ulong;

    /// Clear the error queue
    pub fn ERR_clear_error();

    /// Get error string
    pub fn ERR_error_string(e: c_ulong, buf: *mut c_char) -> *const c_char;

    /// Get error string (safer, with length limit)
    pub fn ERR_error_string_n(e: c_ulong, buf: *mut c_char, len: size_t);

    /// Print error queue to file
    pub fn ERR_print_errors_fp(fp: *mut c_void);

    /// Get error library string
    pub fn ERR_lib_error_string(e: c_ulong) -> *const c_char;

    /// Get error reason string
    pub fn ERR_reason_error_string(e: c_ulong) -> *const c_char;
}

// ============================================================================
// Opaque Types (C structures)
// ============================================================================

/// SHA-256 context (matches ncryptolib's SHA256_CTX)
#[repr(C)]
pub struct SHA256_CTX {
    pub state: [u32; 8],
    pub buffer: [u8; 64],
    pub buffer_len: usize,
    pub total_bits: u64,
}

impl Default for SHA256_CTX {
    fn default() -> Self {
        Self {
            state: [0; 8],
            buffer: [0; 64],
            buffer_len: 0,
            total_bits: 0,
        }
    }
}

// ============================================================================
// Safe Rust Wrappers
// ============================================================================

/// Compute SHA-256 hash
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        SHA256(data.as_ptr(), data.len(), out.as_mut_ptr());
    }
    out
}

/// Compute SHA-384 hash
pub fn sha384(data: &[u8]) -> [u8; 48] {
    let mut out = [0u8; 48];
    unsafe {
        SHA384(data.as_ptr(), data.len(), out.as_mut_ptr());
    }
    out
}

/// Compute SHA-512 hash
pub fn sha512(data: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    unsafe {
        SHA512(data.as_ptr(), data.len(), out.as_mut_ptr());
    }
    out
}

/// Compute HMAC-SHA256
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        ncrypto_hmac_sha256(
            key.as_ptr(),
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
        );
    }
    out
}

/// Compute HMAC-SHA384
pub fn hmac_sha384(key: &[u8], data: &[u8]) -> [u8; 48] {
    let mut out = [0u8; 48];
    unsafe {
        ncrypto_hmac_sha384(
            key.as_ptr(),
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
        );
    }
    out
}

/// Compute HMAC-SHA512
pub fn hmac_sha512(key: &[u8], data: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    unsafe {
        ncrypto_hmac_sha512(
            key.as_ptr(),
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
        );
    }
    out
}

/// X25519 key exchange
pub fn x25519(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        X25519(out.as_mut_ptr(), private_key.as_ptr(), public_key.as_ptr());
    }
    out
}

/// X25519 public key derivation
pub fn x25519_public_from_private(private_key: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        X25519_public_from_private(out.as_mut_ptr(), private_key.as_ptr());
    }
    out
}

/// Get random bytes
pub fn getrandom(buf: &mut [u8], flags: u32) -> Result<(), i32> {
    let ret = unsafe { ncrypto_getrandom(buf.as_mut_ptr(), buf.len(), flags) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

/// Secure memory zeroing
pub fn secure_zero(buf: &mut [u8]) {
    unsafe {
        ncrypto_secure_zero(buf.as_mut_ptr(), buf.len());
    }
}

/// Constant-time comparison
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    unsafe { ncrypto_ct_eq(a.as_ptr(), b.as_ptr(), a.len()) == 1 }
}

/// Base64 decode
pub fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let input_bytes = input.as_bytes();
    // Output is at most 3/4 of input size
    let max_output_len = (input_bytes.len() * 3) / 4 + 4;
    let mut output = vec![0u8; max_output_len];
    let mut output_len = max_output_len;

    let ret = unsafe {
        ncrypto_base64_decode(
            input_bytes.as_ptr(),
            input_bytes.len(),
            output.as_mut_ptr(),
            &mut output_len,
        )
    };

    if ret == 0 {
        Err("base64 decode failed")
    } else {
        output.truncate(output_len);
        Ok(output)
    }
}

// ============================================================================
// P-256 Wrappers
// ============================================================================

/// P-256 key pair generation
pub struct P256KeyPair {
    pub private_key: [u8; 32],
    pub public_key: [u8; 65], // Uncompressed format: 04 || x || y
}

impl P256KeyPair {
    /// Generate a new P-256 key pair
    pub fn generate() -> Option<Self> {
        let mut private_key = [0u8; 32];
        let mut public_key = [0u8; 65];

        let ret = unsafe { ncrypto_p256_keygen(private_key.as_mut_ptr(), public_key.as_mut_ptr()) };

        if ret == 1 {
            Some(Self {
                private_key,
                public_key,
            })
        } else {
            None
        }
    }

    /// Create from existing private key
    pub fn from_private_key(private_key: &[u8; 32]) -> Option<Self> {
        // For now, generate a new pair and replace private key
        // TODO: Add C ABI function to derive public from private for P-256
        let mut keypair = Self::generate()?;
        keypair.private_key.copy_from_slice(private_key);
        Some(keypair)
    }

    /// Get uncompressed public key
    pub fn public_key_uncompressed(&self) -> Vec<u8> {
        self.public_key.to_vec()
    }

    /// Compute ECDH shared secret
    pub fn ecdh(&self, peer_public: &P256Point) -> Option<[u8; 32]> {
        let mut shared_secret = [0u8; 32];
        let peer_bytes = peer_public.to_uncompressed();

        let ret = unsafe {
            ncrypto_p256_ecdh(
                shared_secret.as_mut_ptr(),
                self.private_key.as_ptr(),
                peer_bytes.as_ptr(),
                peer_bytes.len(),
            )
        };

        if ret == 1 {
            Some(shared_secret)
        } else {
            None
        }
    }
}

/// P-256 curve point
pub struct P256Point {
    pub x: [u8; 32],
    pub y: [u8; 32],
}

impl P256Point {
    /// Parse from uncompressed format (04 || x || y)
    pub fn from_uncompressed(data: &[u8]) -> Option<Self> {
        if data.len() != 65 || data[0] != 0x04 {
            return None;
        }

        let mut x = [0u8; 32];
        let mut y = [0u8; 32];

        let ret = unsafe {
            ncrypto_p256_point_from_uncompressed(
                x.as_mut_ptr(),
                y.as_mut_ptr(),
                data.as_ptr(),
                data.len(),
            )
        };

        if ret == 1 {
            Some(Self { x, y })
        } else {
            None
        }
    }

    /// Convert to uncompressed format
    pub fn to_uncompressed(&self) -> [u8; 65] {
        let mut out = [0u8; 65];
        out[0] = 0x04;
        out[1..33].copy_from_slice(&self.x);
        out[33..65].copy_from_slice(&self.y);
        out
    }
}

/// P-256 signature (for verification)
pub struct P256Signature {
    pub r: [u8; 32],
    pub s: [u8; 32],
}

impl P256Signature {
    /// Parse from DER format
    pub fn from_der(data: &[u8]) -> Option<Self> {
        // Simple DER parsing for ECDSA signature
        // SEQUENCE { INTEGER r, INTEGER s }
        if data.len() < 8 || data[0] != 0x30 {
            return None;
        }

        let mut pos = 2; // Skip SEQUENCE tag and length

        // Parse r
        if data[pos] != 0x02 {
            return None;
        }
        pos += 1;
        let r_len = data[pos] as usize;
        pos += 1;

        let mut r = [0u8; 32];
        if r_len > 33 {
            return None;
        }
        let r_start = if r_len == 33 && data[pos] == 0 {
            pos + 1
        } else {
            pos
        };
        let r_copy_len = core::cmp::min(32, data.len() - r_start);
        let r_dest_start = 32 - core::cmp::min(r_len, 32);
        r[r_dest_start..]
            .copy_from_slice(&data[r_start..r_start + (32 - r_dest_start).min(r_copy_len)]);
        pos += r_len;

        // Parse s
        if pos >= data.len() || data[pos] != 0x02 {
            return None;
        }
        pos += 1;
        let s_len = data[pos] as usize;
        pos += 1;

        let mut s = [0u8; 32];
        if s_len > 33 {
            return None;
        }
        let s_start = if s_len == 33 && data[pos] == 0 {
            pos + 1
        } else {
            pos
        };
        let s_copy_len = core::cmp::min(32, data.len() - s_start);
        let s_dest_start = 32 - core::cmp::min(s_len, 32);
        s[s_dest_start..]
            .copy_from_slice(&data[s_start..s_start + (32 - s_dest_start).min(s_copy_len)]);

        Some(Self { r, s })
    }

    /// Verify signature against a public key point and message hash
    pub fn verify(&self, point: &P256Point, message_hash: &[u8]) -> bool {
        // Encode signature as raw r || s format for C ABI
        let mut sig_raw = [0u8; 64];
        sig_raw[..32].copy_from_slice(&self.r);
        sig_raw[32..].copy_from_slice(&self.s);

        let pubkey = point.to_uncompressed();

        let ret = unsafe {
            ncrypto_p256_verify(
                message_hash.as_ptr(),
                message_hash.len(),
                sig_raw.as_ptr(),
                64,
                pubkey.as_ptr(),
                65,
            )
        };

        ret == 1
    }
}

// ============================================================================
// AES-GCM Wrappers
// ============================================================================

/// AES-GCM cipher
pub struct AesGcm {
    key: Vec<u8>,
}

impl AesGcm {
    /// Create AES-128-GCM cipher
    pub fn new_128(key: &[u8; 16]) -> Self {
        Self { key: key.to_vec() }
    }

    /// Create AES-256-GCM cipher
    pub fn new_256(key: &[u8; 32]) -> Self {
        Self { key: key.to_vec() }
    }

    /// Encrypt with AEAD
    pub fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let mut ciphertext = vec![0u8; plaintext.len()];
        let mut tag = [0u8; 16];

        if self.key.len() == 16 {
            unsafe {
                ncrypto_aes128_gcm_encrypt(
                    self.key.as_ptr(),
                    nonce.as_ptr(),
                    nonce.len(),
                    plaintext.as_ptr(),
                    plaintext.len(),
                    aad.as_ptr(),
                    aad.len(),
                    ciphertext.as_mut_ptr(),
                    tag.as_mut_ptr(),
                );
            }
        } else if self.key.len() == 32 {
            unsafe {
                ncrypto_aes256_gcm_encrypt(
                    self.key.as_ptr(),
                    nonce.as_ptr(),
                    nonce.len(),
                    plaintext.as_ptr(),
                    plaintext.len(),
                    aad.as_ptr(),
                    aad.len(),
                    ciphertext.as_mut_ptr(),
                    tag.as_mut_ptr(),
                );
            }
        }

        (ciphertext, tag)
    }

    /// Decrypt with AEAD
    pub fn decrypt(
        &self,
        nonce: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
        tag: &[u8; 16],
    ) -> Option<Vec<u8>> {
        let mut plaintext = vec![0u8; ciphertext.len()];

        let ret = if self.key.len() == 16 {
            unsafe {
                ncrypto_aes128_gcm_decrypt(
                    self.key.as_ptr(),
                    nonce.as_ptr(),
                    nonce.len(),
                    ciphertext.as_ptr(),
                    ciphertext.len(),
                    aad.as_ptr(),
                    aad.len(),
                    tag.as_ptr(),
                    plaintext.as_mut_ptr(),
                )
            }
        } else {
            let key32: &[u8; 32] = self.key.as_slice().try_into().ok()?;
            unsafe {
                ncrypto_aes256_gcm_decrypt(
                    key32.as_ptr(),
                    nonce.as_ptr(),
                    nonce.len(),
                    ciphertext.as_ptr(),
                    ciphertext.len(),
                    aad.as_ptr(),
                    aad.len(),
                    tag.as_ptr(),
                    plaintext.as_mut_ptr(),
                )
            }
        };

        if ret == 1 {
            Some(plaintext)
        } else {
            None
        }
    }
}

// ============================================================================
// x25519 Module - Compatibility with crate::ncryptolib::x25519::
// ============================================================================

pub mod x25519 {
    use super::*;

    /// X25519 key exchange
    pub fn x25519(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
        super::x25519(private_key, public_key)
    }

    /// X25519 public key derivation (base point multiplication)
    pub fn x25519_base(private_key: &[u8; 32]) -> [u8; 32] {
        super::x25519_public_from_private(private_key)
    }
}

// ============================================================================
// p256 Module - Compatibility with crate::ncryptolib::p256::
// ============================================================================

pub mod p256 {
    pub use super::P256KeyPair;
    pub use super::P256Point;
    pub use super::P256Signature;
}

// ============================================================================
// hash Module - Compatibility with crate::ncryptolib::hash::
// ============================================================================

pub mod hash {
    use super::*;
    use std::vec::Vec;

    /// SHA-256 hasher state (for incremental hashing)
    pub struct Sha256 {
        ctx: SHA256_CTX,
    }

    impl Sha256 {
        pub fn new() -> Self {
            let mut ctx = SHA256_CTX::default();
            unsafe { SHA256_Init(&mut ctx) };
            Self { ctx }
        }

        pub fn update(&mut self, data: &[u8]) {
            unsafe {
                SHA256_Update(&mut self.ctx, data.as_ptr(), data.len());
            }
        }

        pub fn finalize(mut self) -> [u8; 32] {
            let mut out = [0u8; 32];
            unsafe {
                SHA256_Final(out.as_mut_ptr(), &mut self.ctx);
            }
            out
        }
    }

    impl Default for Sha256 {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ============================================================================
// rsa Module - Compatibility with crate::ncryptolib::rsa::
// ============================================================================

pub mod rsa {
    use super::bigint::BigInt;
    use super::*;
    use std::vec::Vec;

    /// RSA public key (for signature verification)
    pub struct RsaPublicKey {
        pub n: BigInt,
        pub e: BigInt,
    }

    impl RsaPublicKey {
        /// Create RSA public key from modulus and exponent
        pub fn new(n: BigInt, e: BigInt) -> Self {
            Self { n, e }
        }
    }

    /// RSA PKCS#1 v1.5 signature verification
    pub fn rsa_verify(message: &[u8], signature: &[u8], pubkey: &RsaPublicKey) -> Result<bool, ()> {
        let n_bytes = pubkey.n.to_bytes_be();
        let e_bytes = pubkey.e.to_bytes_be();

        let ret = unsafe {
            ncrypto_rsa_verify(
                message.as_ptr(),
                message.len(),
                signature.as_ptr(),
                signature.len(),
                n_bytes.as_ptr(),
                n_bytes.len(),
                e_bytes.as_ptr(),
                e_bytes.len(),
            )
        };
        Ok(ret == 1)
    }

    /// RSA-PSS signature verification
    pub fn rsa_pss_verify(
        message: &[u8],
        signature: &[u8],
        pubkey: &RsaPublicKey,
    ) -> Result<bool, ()> {
        let n_bytes = pubkey.n.to_bytes_be();
        let e_bytes = pubkey.e.to_bytes_be();

        let ret = unsafe {
            ncrypto_rsa_pss_verify(
                message.as_ptr(),
                message.len(),
                signature.as_ptr(),
                signature.len(),
                n_bytes.as_ptr(),
                n_bytes.len(),
                e_bytes.as_ptr(),
                e_bytes.len(),
            )
        };
        Ok(ret == 1)
    }
}

// ============================================================================
// bigint Module - Compatibility with crate::ncryptolib::bigint::
// ============================================================================

pub mod bigint {
    use std::vec::Vec;

    /// Big integer representation (for RSA)
    #[derive(Clone)]
    pub struct BigInt {
        data: Vec<u8>,
    }

    impl BigInt {
        pub fn from_bytes_be(bytes: &[u8]) -> Self {
            Self {
                data: bytes.to_vec(),
            }
        }

        pub fn to_bytes_be(&self) -> Vec<u8> {
            self.data.clone()
        }
    }
}
