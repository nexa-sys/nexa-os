//! FFI bindings to nssl (libssl.so) and ncryptolib (libcrypto.so)
//!
//! This module provides C ABI function declarations for dynamically linking
//! against nssl's libssl.so and ncryptolib's libcrypto.so.
//!
//! nssl provides OpenSSL-compatible C ABI for TLS 1.2/1.3 support.
//! ncryptolib provides OpenSSL-compatible C ABI for cryptographic primitives.
//!
//! These bindings enable ntcp2 (QUIC library) to use TLS 1.3 for QUIC crypto.

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
// Opaque Types
// ============================================================================

/// SSL context (SSL_CTX)
#[repr(C)]
pub struct SSL_CTX {
    _private: [u8; 0],
}

/// SSL connection (SSL)
#[repr(C)]
pub struct SSL {
    _private: [u8; 0],
}

/// SSL method
#[repr(C)]
pub struct SSL_METHOD {
    _private: [u8; 0],
}

/// X509 certificate
#[repr(C)]
pub struct X509 {
    _private: [u8; 0],
}

/// X509 store context
#[repr(C)]
pub struct X509_STORE_CTX {
    _private: [u8; 0],
}

/// SHA-256 context
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
// FFI Function Declarations - libssl.so (nssl)
// ============================================================================

#[link(name = "ssl")]
extern "C" {
    // ========================================================================
    // Library Initialization
    // ========================================================================

    /// Initialize the SSL library
    pub fn SSL_library_init() -> c_int;

    /// Initialize SSL library (OpenSSL 1.1+ compatible)
    pub fn OPENSSL_init_ssl(opts: u64, settings: *const c_void) -> c_int;

    // ========================================================================
    // SSL_METHOD Functions
    // ========================================================================

    /// Get TLS client method (supports TLS 1.2 and 1.3)
    pub fn TLS_client_method() -> *const SSL_METHOD;

    /// Get TLS server method (supports TLS 1.2 and 1.3)
    pub fn TLS_server_method() -> *const SSL_METHOD;

    /// Get TLS method (auto-negotiate)
    pub fn TLS_method() -> *const SSL_METHOD;

    // ========================================================================
    // SSL_CTX Functions
    // ========================================================================

    /// Create a new SSL context
    pub fn SSL_CTX_new(method: *const SSL_METHOD) -> *mut SSL_CTX;

    /// Free an SSL context
    pub fn SSL_CTX_free(ctx: *mut SSL_CTX);

    /// Set SSL context options
    pub fn SSL_CTX_set_options(ctx: *mut SSL_CTX, options: c_ulong) -> c_ulong;

    /// Get SSL context options
    pub fn SSL_CTX_get_options(ctx: *const SSL_CTX) -> c_ulong;

    /// Set minimum protocol version
    pub fn SSL_CTX_set_min_proto_version(ctx: *mut SSL_CTX, version: c_int) -> c_int;

    /// Set maximum protocol version
    pub fn SSL_CTX_set_max_proto_version(ctx: *mut SSL_CTX, version: c_int) -> c_int;

    /// Set cipher list (TLS 1.2)
    pub fn SSL_CTX_set_cipher_list(ctx: *mut SSL_CTX, str: *const c_char) -> c_int;

    /// Set ciphersuites (TLS 1.3)
    pub fn SSL_CTX_set_ciphersuites(ctx: *mut SSL_CTX, str: *const c_char) -> c_int;

    /// Set verification mode
    pub fn SSL_CTX_set_verify(
        ctx: *mut SSL_CTX,
        mode: c_int,
        callback: Option<extern "C" fn(c_int, *mut X509_STORE_CTX) -> c_int>,
    );

    /// Set verification depth
    pub fn SSL_CTX_set_verify_depth(ctx: *mut SSL_CTX, depth: c_int);

    /// Load certificate file
    pub fn SSL_CTX_use_certificate_file(
        ctx: *mut SSL_CTX,
        file: *const c_char,
        type_: c_int,
    ) -> c_int;

    /// Load certificate chain file
    pub fn SSL_CTX_use_certificate_chain_file(ctx: *mut SSL_CTX, file: *const c_char) -> c_int;

    /// Load private key file
    pub fn SSL_CTX_use_PrivateKey_file(
        ctx: *mut SSL_CTX,
        file: *const c_char,
        type_: c_int,
    ) -> c_int;

    /// Check private key
    pub fn SSL_CTX_check_private_key(ctx: *const SSL_CTX) -> c_int;

    /// Load CA certificates from file
    pub fn SSL_CTX_load_verify_locations(
        ctx: *mut SSL_CTX,
        ca_file: *const c_char,
        ca_path: *const c_char,
    ) -> c_int;

    /// Set default verify paths
    pub fn SSL_CTX_set_default_verify_paths(ctx: *mut SSL_CTX) -> c_int;

    /// Set ALPN protocols
    pub fn SSL_CTX_set_alpn_protos(
        ctx: *mut SSL_CTX,
        protos: *const c_uchar,
        protos_len: c_uint,
    ) -> c_int;

    // ========================================================================
    // SSL Functions
    // ========================================================================

    /// Create a new SSL connection
    pub fn SSL_new(ctx: *mut SSL_CTX) -> *mut SSL;

    /// Free an SSL connection
    pub fn SSL_free(ssl: *mut SSL);

    /// Set the file descriptor
    pub fn SSL_set_fd(ssl: *mut SSL, fd: c_int) -> c_int;

    /// Get the file descriptor
    pub fn SSL_get_fd(ssl: *const SSL) -> c_int;

    /// Perform TLS handshake (client)
    pub fn SSL_connect(ssl: *mut SSL) -> c_int;

    /// Perform TLS handshake (server)
    pub fn SSL_accept(ssl: *mut SSL) -> c_int;

    /// Perform TLS handshake (generic)
    pub fn SSL_do_handshake(ssl: *mut SSL) -> c_int;

    /// Write data
    pub fn SSL_write(ssl: *mut SSL, buf: *const c_void, num: c_int) -> c_int;

    /// Read data
    pub fn SSL_read(ssl: *mut SSL, buf: *mut c_void, num: c_int) -> c_int;

    /// Shutdown TLS connection
    pub fn SSL_shutdown(ssl: *mut SSL) -> c_int;

    /// Get error code
    pub fn SSL_get_error(ssl: *const SSL, ret_code: c_int) -> c_int;

    /// Get selected ALPN protocol
    pub fn SSL_get0_alpn_selected(
        ssl: *const SSL,
        data: *mut *const c_uchar,
        len: *mut c_uint,
    );

    /// Set ALPN protocols for connection
    pub fn SSL_set_alpn_protos(ssl: *mut SSL, protos: *const c_uchar, protos_len: c_uint) -> c_int;

    /// Get peer certificate
    pub fn SSL_get_peer_certificate(ssl: *const SSL) -> *mut X509;

    /// Get verify result
    pub fn SSL_get_verify_result(ssl: *const SSL) -> c_long;

    /// Set connect state (client mode)
    pub fn SSL_set_connect_state(ssl: *mut SSL);

    /// Set accept state (server mode)
    pub fn SSL_set_accept_state(ssl: *mut SSL);

    /// Get current cipher
    pub fn SSL_get_current_cipher(ssl: *const SSL) -> *const c_void;

    /// Get cipher name
    pub fn SSL_CIPHER_get_name(cipher: *const c_void) -> *const c_char;

    /// Check if handshake is done
    pub fn SSL_is_init_finished(ssl: *const SSL) -> c_int;

    /// Get SSL version
    pub fn SSL_version(ssl: *const SSL) -> c_int;

    /// Set hostname for SNI
    pub fn SSL_set_tlsext_host_name(ssl: *mut SSL, name: *const c_char) -> c_int;
}

// ============================================================================
// FFI Function Declarations - libcrypto.so (ncryptolib)
// ============================================================================

#[link(name = "crypto")]
extern "C" {
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
    // Hash Functions - SHA-384/512
    // ========================================================================

    /// One-shot SHA-384 hash
    pub fn SHA384(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;

    /// One-shot SHA-512 hash
    pub fn SHA512(data: *const u8, len: size_t, md: *mut u8) -> *mut u8;

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

    // ========================================================================
    // HKDF Functions
    // ========================================================================

    /// HKDF-Extract using SHA-256
    pub fn ncrypto_hkdf_extract_sha256(
        salt: *const u8,
        salt_len: size_t,
        ikm: *const u8,
        ikm_len: size_t,
        prk: *mut u8,
    ) -> c_int;

    /// HKDF-Expand-Label (TLS 1.3 style)
    pub fn ncrypto_hkdf_expand_label_sha256(
        secret: *const u8,
        secret_len: size_t,
        label: *const u8,
        label_len: size_t,
        context: *const u8,
        context_len: size_t,
        out: *mut u8,
        out_len: size_t,
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
    // AES-ECB (for header protection)
    // ========================================================================

    /// AES-128-ECB single block encryption
    pub fn ncrypto_aes128_ecb_encrypt(
        key: *const u8,
        input: *const u8,
        output: *mut u8,
    ) -> c_int;

    /// AES-256-ECB single block encryption
    pub fn ncrypto_aes256_ecb_encrypt(
        key: *const u8,
        input: *const u8,
        output: *mut u8,
    ) -> c_int;

    // ========================================================================
    // ChaCha20-Poly1305
    // ========================================================================

    /// ChaCha20-Poly1305 encryption
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

    /// ChaCha20-Poly1305 decryption
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
    // ChaCha20 (for header protection)
    // ========================================================================

    /// ChaCha20 keystream generation (for header protection)
    pub fn ncrypto_chacha20_block(
        key: *const u8,
        counter: u32,
        nonce: *const u8,
        output: *mut u8,
        output_len: size_t,
    ) -> c_int;

    // ========================================================================
    // Random Number Generation
    // ========================================================================

    /// Get random bytes from kernel
    pub fn ncrypto_getrandom(buf: *mut u8, len: size_t, flags: c_uint) -> c_int;

    // ========================================================================
    // Constant-time Operations
    // ========================================================================

    /// Constant-time memory comparison
    pub fn ncrypto_ct_eq(a: *const u8, b: *const u8, len: size_t) -> c_int;

    /// Secure memory zeroing
    pub fn ncrypto_secure_zero(ptr: *mut u8, len: size_t);

    // ========================================================================
    // Error Handling Functions
    // ========================================================================

    /// Get and remove first error from error queue
    pub fn ERR_get_error() -> c_ulong;

    /// Peek at first error without removing
    pub fn ERR_peek_error() -> c_ulong;

    /// Clear the error queue
    pub fn ERR_clear_error();

    /// Get error string
    pub fn ERR_error_string(e: c_ulong, buf: *mut c_char) -> *const c_char;
}

// ============================================================================
// Safe Rust Wrappers
// ============================================================================

pub const SHA256_DIGEST_SIZE: usize = 32;
pub const SHA384_DIGEST_SIZE: usize = 48;
pub const AEAD_TAG_LEN: usize = 16;
pub const AES_BLOCK_SIZE: usize = 16;

/// Compute SHA-256 hash
pub fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut out = [0u8; SHA256_DIGEST_SIZE];
    unsafe {
        SHA256(data.as_ptr(), data.len(), out.as_mut_ptr());
    }
    out
}

/// Compute SHA-384 hash
pub fn sha384(data: &[u8]) -> [u8; SHA384_DIGEST_SIZE] {
    let mut out = [0u8; SHA384_DIGEST_SIZE];
    unsafe {
        SHA384(data.as_ptr(), data.len(), out.as_mut_ptr());
    }
    out
}

/// HMAC-SHA256
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut out = [0u8; SHA256_DIGEST_SIZE];
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

/// HKDF-Extract using SHA-256
pub fn hkdf_extract_sha256(salt: &[u8], ikm: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut prk = [0u8; SHA256_DIGEST_SIZE];
    unsafe {
        ncrypto_hkdf_extract_sha256(
            salt.as_ptr(),
            salt.len(),
            ikm.as_ptr(),
            ikm.len(),
            prk.as_mut_ptr(),
        );
    }
    prk
}

/// HKDF-Expand-Label using SHA-256 (TLS 1.3 style)
pub fn hkdf_expand_label(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    let mut out = vec![0u8; length];
    unsafe {
        ncrypto_hkdf_expand_label_sha256(
            secret.as_ptr(),
            secret.len(),
            label.as_ptr(),
            label.len(),
            context.as_ptr(),
            context.len(),
            out.as_mut_ptr(),
            length,
        );
    }
    out
}

/// AES-128-GCM encryption
pub fn aes128_gcm_encrypt(
    key: &[u8; 16],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> (Vec<u8>, [u8; AEAD_TAG_LEN]) {
    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut tag = [0u8; AEAD_TAG_LEN];
    unsafe {
        ncrypto_aes128_gcm_encrypt(
            key.as_ptr(),
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
    (ciphertext, tag)
}

/// AES-128-GCM decryption
pub fn aes128_gcm_decrypt(
    key: &[u8; 16],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
    tag: &[u8; AEAD_TAG_LEN],
) -> Option<Vec<u8>> {
    let mut plaintext = vec![0u8; ciphertext.len()];
    let ret = unsafe {
        ncrypto_aes128_gcm_decrypt(
            key.as_ptr(),
            nonce.as_ptr(),
            nonce.len(),
            ciphertext.as_ptr(),
            ciphertext.len(),
            aad.as_ptr(),
            aad.len(),
            tag.as_ptr(),
            plaintext.as_mut_ptr(),
        )
    };
    if ret == 1 {
        Some(plaintext)
    } else {
        None
    }
}

/// AES-256-GCM encryption
pub fn aes256_gcm_encrypt(
    key: &[u8; 32],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> (Vec<u8>, [u8; AEAD_TAG_LEN]) {
    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut tag = [0u8; AEAD_TAG_LEN];
    unsafe {
        ncrypto_aes256_gcm_encrypt(
            key.as_ptr(),
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
    (ciphertext, tag)
}

/// AES-256-GCM decryption
pub fn aes256_gcm_decrypt(
    key: &[u8; 32],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
    tag: &[u8; AEAD_TAG_LEN],
) -> Option<Vec<u8>> {
    let mut plaintext = vec![0u8; ciphertext.len()];
    let ret = unsafe {
        ncrypto_aes256_gcm_decrypt(
            key.as_ptr(),
            nonce.as_ptr(),
            nonce.len(),
            ciphertext.as_ptr(),
            ciphertext.len(),
            aad.as_ptr(),
            aad.len(),
            tag.as_ptr(),
            plaintext.as_mut_ptr(),
        )
    };
    if ret == 1 {
        Some(plaintext)
    } else {
        None
    }
}

/// AES-128-ECB single block encryption (for header protection)
pub fn aes128_ecb_encrypt(key: &[u8; 16], input: &[u8; AES_BLOCK_SIZE]) -> [u8; AES_BLOCK_SIZE] {
    let mut output = [0u8; AES_BLOCK_SIZE];
    unsafe {
        ncrypto_aes128_ecb_encrypt(key.as_ptr(), input.as_ptr(), output.as_mut_ptr());
    }
    output
}

/// AES-256-ECB single block encryption (for header protection)
pub fn aes256_ecb_encrypt(key: &[u8; 32], input: &[u8; AES_BLOCK_SIZE]) -> [u8; AES_BLOCK_SIZE] {
    let mut output = [0u8; AES_BLOCK_SIZE];
    unsafe {
        ncrypto_aes256_ecb_encrypt(key.as_ptr(), input.as_ptr(), output.as_mut_ptr());
    }
    output
}

/// ChaCha20-Poly1305 encryption
pub fn chacha20_poly1305_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    plaintext: &[u8],
    aad: &[u8],
) -> Vec<u8> {
    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut tag = [0u8; AEAD_TAG_LEN];
    unsafe {
        ncrypto_chacha20_poly1305_encrypt(
            key.as_ptr(),
            nonce.as_ptr(),
            plaintext.as_ptr(),
            plaintext.len(),
            aad.as_ptr(),
            aad.len(),
            ciphertext.as_mut_ptr(),
            tag.as_mut_ptr(),
        );
    }
    // Append tag to ciphertext
    ciphertext.extend_from_slice(&tag);
    ciphertext
}

/// ChaCha20-Poly1305 decryption
pub fn chacha20_poly1305_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext_with_tag: &[u8],
    aad: &[u8],
) -> Option<Vec<u8>> {
    if ciphertext_with_tag.len() < AEAD_TAG_LEN {
        return None;
    }
    let ct_len = ciphertext_with_tag.len() - AEAD_TAG_LEN;
    let ciphertext = &ciphertext_with_tag[..ct_len];
    let tag = &ciphertext_with_tag[ct_len..];

    let mut plaintext = vec![0u8; ct_len];
    let ret = unsafe {
        ncrypto_chacha20_poly1305_decrypt(
            key.as_ptr(),
            nonce.as_ptr(),
            ciphertext.as_ptr(),
            ct_len,
            aad.as_ptr(),
            aad.len(),
            tag.as_ptr(),
            plaintext.as_mut_ptr(),
        )
    };
    if ret == 1 {
        Some(plaintext)
    } else {
        None
    }
}

/// ChaCha20 block for header protection
pub fn chacha20_hp_mask(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 5] {
    let mut output = [0u8; 5];
    unsafe {
        ncrypto_chacha20_block(key.as_ptr(), counter, nonce.as_ptr(), output.as_mut_ptr(), 5);
    }
    output
}

/// Get random bytes
pub fn getrandom(buf: &mut [u8]) -> Result<(), i32> {
    let ret = unsafe { ncrypto_getrandom(buf.as_mut_ptr(), buf.len(), 0) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

/// Constant-time comparison
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    unsafe { ncrypto_ct_eq(a.as_ptr(), b.as_ptr(), a.len()) == 1 }
}

/// Secure memory zeroing
pub fn secure_zero(buf: &mut [u8]) {
    unsafe {
        ncrypto_secure_zero(buf.as_mut_ptr(), buf.len());
    }
}
