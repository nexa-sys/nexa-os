//! nssl FFI bindings for dynamic linking
//!
//! This module provides FFI declarations for dynamically linking against
//! libnssl.so at runtime. Instead of statically linking nssl, nurl can
//! load the library dynamically.
//!
//! # Usage
//! ```rust
//! use nurl::nssl_ffi::*;
//!
//! // Initialize SSL library
//! unsafe { SSL_library_init(); }
//!
//! // Create context and connection
//! let method = unsafe { TLS_client_method() };
//! let ctx = unsafe { SSL_CTX_new(method) };
//! let ssl = unsafe { SSL_new(ctx) };
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};

// ============================================================================
// Opaque Types
// ============================================================================

/// Opaque SSL_CTX type
#[repr(C)]
pub struct SSL_CTX {
    _private: [u8; 0],
}

/// Opaque SSL type
#[repr(C)]
pub struct SSL {
    _private: [u8; 0],
}

/// Opaque SSL_METHOD type
#[repr(C)]
pub struct SSL_METHOD {
    _private: [u8; 0],
}

/// Opaque SSL_CIPHER type
#[repr(C)]
pub struct SSL_CIPHER {
    _private: [u8; 0],
}

/// Opaque SSL_SESSION type
#[repr(C)]
pub struct SSL_SESSION {
    _private: [u8; 0],
}

/// Opaque BIO type
#[repr(C)]
pub struct BIO {
    _private: [u8; 0],
}

/// Opaque BIO_METHOD type
#[repr(C)]
pub struct BIO_METHOD {
    _private: [u8; 0],
}

/// Opaque X509 type
#[repr(C)]
pub struct X509 {
    _private: [u8; 0],
}

/// Opaque X509_NAME type
#[repr(C)]
pub struct X509_NAME {
    _private: [u8; 0],
}

/// Opaque X509_STORE_CTX type
#[repr(C)]
pub struct X509_STORE_CTX {
    _private: [u8; 0],
}

// ============================================================================
// Constants
// ============================================================================

/// PEM file type
pub const SSL_FILETYPE_PEM: c_int = 1;
/// ASN.1/DER file type
pub const SSL_FILETYPE_ASN1: c_int = 2;

/// No verification
pub const SSL_VERIFY_NONE: c_int = 0x00;
/// Verify peer certificate
pub const SSL_VERIFY_PEER: c_int = 0x01;
/// Fail if no peer certificate (server only)
pub const SSL_VERIFY_FAIL_IF_NO_PEER_CERT: c_int = 0x02;

/// No error
pub const SSL_ERROR_NONE: c_int = 0;
/// SSL error
pub const SSL_ERROR_SSL: c_int = 1;
/// Need more data to read
pub const SSL_ERROR_WANT_READ: c_int = 2;
/// Need more data to write
pub const SSL_ERROR_WANT_WRITE: c_int = 3;
/// System call error
pub const SSL_ERROR_SYSCALL: c_int = 5;
/// Connection closed cleanly
pub const SSL_ERROR_ZERO_RETURN: c_int = 6;

/// TLS 1.2 version
pub const TLS1_2_VERSION: u16 = 0x0303;
/// TLS 1.3 version
pub const TLS1_3_VERSION: u16 = 0x0304;

// ============================================================================
// Verification callback type
// ============================================================================

pub type VerifyCallback = Option<unsafe extern "C" fn(c_int, *mut X509_STORE_CTX) -> c_int>;

// ============================================================================
// External Functions (linked dynamically from libnssl.so and libncryptolib.so)
// ============================================================================

// Link both nssl and ncryptolib since nssl depends on ncryptolib
// and the dynamic linker doesn't recursively load library dependencies
#[link(name = "ncryptolib")]
#[link(name = "nssl")]
extern "C" {
    // ========================================================================
    // Library Initialization
    // ========================================================================

    /// Initialize SSL library
    pub fn SSL_library_init() -> c_int;

    /// Initialize SSL library (OpenSSL 1.1+ style)
    pub fn OPENSSL_init_ssl(opts: u64, settings: *const c_void) -> c_int;

    /// Load SSL error strings
    pub fn SSL_load_error_strings();

    /// Add all algorithms
    pub fn OpenSSL_add_all_algorithms();

    // ========================================================================
    // SSL_METHOD Functions
    // ========================================================================

    /// Get TLS client method
    pub fn TLS_client_method() -> *const SSL_METHOD;

    /// Get TLS server method
    pub fn TLS_server_method() -> *const SSL_METHOD;

    /// Get TLS method (auto)
    pub fn TLS_method() -> *const SSL_METHOD;

    /// Get TLS 1.2 client method
    pub fn TLSv1_2_client_method() -> *const SSL_METHOD;

    /// Get TLS 1.2 server method
    pub fn TLSv1_2_server_method() -> *const SSL_METHOD;

    /// Legacy SSLv23 client method
    pub fn SSLv23_client_method() -> *const SSL_METHOD;

    /// Legacy SSLv23 server method
    pub fn SSLv23_server_method() -> *const SSL_METHOD;

    /// Legacy SSLv23 method
    pub fn SSLv23_method() -> *const SSL_METHOD;

    // ========================================================================
    // SSL_CTX Functions
    // ========================================================================

    /// Create new SSL context
    pub fn SSL_CTX_new(method: *const SSL_METHOD) -> *mut SSL_CTX;

    /// Free SSL context
    pub fn SSL_CTX_free(ctx: *mut SSL_CTX);

    /// Set SSL context options
    pub fn SSL_CTX_set_options(ctx: *mut SSL_CTX, options: c_ulong) -> c_ulong;

    /// Get SSL context options
    pub fn SSL_CTX_get_options(ctx: *const SSL_CTX) -> c_ulong;

    /// Clear SSL context options
    pub fn SSL_CTX_clear_options(ctx: *mut SSL_CTX, options: c_ulong) -> c_ulong;

    /// Set minimum protocol version
    pub fn SSL_CTX_set_min_proto_version(ctx: *mut SSL_CTX, version: c_int) -> c_int;

    /// Set maximum protocol version
    pub fn SSL_CTX_set_max_proto_version(ctx: *mut SSL_CTX, version: c_int) -> c_int;

    /// Set cipher list (TLS 1.2)
    pub fn SSL_CTX_set_cipher_list(ctx: *mut SSL_CTX, str: *const c_char) -> c_int;

    /// Set ciphersuites (TLS 1.3)
    pub fn SSL_CTX_set_ciphersuites(ctx: *mut SSL_CTX, str: *const c_char) -> c_int;

    /// Set verification mode
    pub fn SSL_CTX_set_verify(ctx: *mut SSL_CTX, mode: c_int, callback: VerifyCallback);

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

    /// Load CA verify locations
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

    /// Set SSL context mode
    pub fn SSL_CTX_set_mode(ctx: *mut SSL_CTX, mode: c_ulong) -> c_ulong;

    /// Get SSL context mode
    pub fn SSL_CTX_get_mode(ctx: *const SSL_CTX) -> c_ulong;

    /// Set session cache mode
    pub fn SSL_CTX_set_session_cache_mode(ctx: *mut SSL_CTX, mode: c_int) -> c_int;

    /// Get session cache mode
    pub fn SSL_CTX_get_session_cache_mode(ctx: *const SSL_CTX) -> c_int;

    /// Set session timeout
    pub fn SSL_CTX_set_timeout(ctx: *mut SSL_CTX, timeout: c_ulong) -> c_ulong;

    /// Get session timeout
    pub fn SSL_CTX_get_timeout(ctx: *const SSL_CTX) -> c_ulong;

    // ========================================================================
    // SSL Functions
    // ========================================================================

    /// Create new SSL connection
    pub fn SSL_new(ctx: *mut SSL_CTX) -> *mut SSL;

    /// Free SSL connection
    pub fn SSL_free(ssl: *mut SSL);

    /// Set socket file descriptor
    pub fn SSL_set_fd(ssl: *mut SSL, fd: c_int) -> c_int;

    /// Get socket file descriptor
    pub fn SSL_get_fd(ssl: *const SSL) -> c_int;

    /// Perform TLS handshake (client)
    pub fn SSL_connect(ssl: *mut SSL) -> c_int;

    /// Perform TLS handshake (server)
    pub fn SSL_accept(ssl: *mut SSL) -> c_int;

    /// Perform TLS handshake (auto)
    pub fn SSL_do_handshake(ssl: *mut SSL) -> c_int;

    /// Read data
    pub fn SSL_read(ssl: *mut SSL, buf: *mut c_uchar, num: c_int) -> c_int;

    /// Write data
    pub fn SSL_write(ssl: *mut SSL, buf: *const c_uchar, num: c_int) -> c_int;

    /// Shutdown connection
    pub fn SSL_shutdown(ssl: *mut SSL) -> c_int;

    /// Get error code
    pub fn SSL_get_error(ssl: *const SSL, ret: c_int) -> c_int;

    /// Get protocol version
    pub fn SSL_version(ssl: *const SSL) -> c_int;

    /// Get protocol version string
    pub fn SSL_get_version(ssl: *const SSL) -> *const c_char;

    /// Get current cipher
    pub fn SSL_get_current_cipher(ssl: *const SSL) -> *const SSL_CIPHER;

    /// Get cipher name
    pub fn SSL_CIPHER_get_name(cipher: *const SSL_CIPHER) -> *const c_char;

    /// Set SNI hostname
    pub fn SSL_set_tlsext_host_name(ssl: *mut SSL, name: *const c_char) -> c_int;

    /// Get peer certificate
    pub fn SSL_get_peer_certificate(ssl: *const SSL) -> *mut X509;

    /// Get verification result
    pub fn SSL_get_verify_result(ssl: *const SSL) -> c_long;

    /// Set connect state
    pub fn SSL_set_connect_state(ssl: *mut SSL);

    /// Set accept state
    pub fn SSL_set_accept_state(ssl: *mut SSL);

    /// Get selected ALPN protocol
    pub fn SSL_get0_alpn_selected(ssl: *const SSL, data: *mut *const c_uchar, len: *mut c_uint);

    /// Check if session was resumed
    pub fn SSL_session_reused(ssl: *const SSL) -> c_int;

    /// Get SSL session
    pub fn SSL_get_session(ssl: *const SSL) -> *mut SSL_SESSION;

    /// Set SSL session
    pub fn SSL_set_session(ssl: *mut SSL, session: *mut SSL_SESSION) -> c_int;

    /// Set SSL options
    pub fn SSL_set_options(ssl: *mut SSL, options: c_ulong) -> c_ulong;

    /// Get SSL options
    pub fn SSL_get_options(ssl: *const SSL) -> c_ulong;

    /// Clear SSL options
    pub fn SSL_clear_options(ssl: *mut SSL, options: c_ulong) -> c_ulong;

    /// Set SSL mode
    pub fn SSL_set_mode(ssl: *mut SSL, mode: c_ulong) -> c_ulong;

    /// Get SSL mode
    pub fn SSL_get_mode(ssl: *const SSL) -> c_ulong;

    /// Get pending bytes
    pub fn SSL_pending(ssl: *const SSL) -> c_int;

    /// Check if has pending data
    pub fn SSL_has_pending(ssl: *const SSL) -> c_int;

    /// Get shutdown state
    pub fn SSL_get_shutdown(ssl: *const SSL) -> c_int;

    /// Set shutdown state
    pub fn SSL_set_shutdown(ssl: *mut SSL, mode: c_int);

    /// Set verification mode on connection
    pub fn SSL_set_verify(ssl: *mut SSL, mode: c_int, callback: VerifyCallback);

    /// Set verification depth on connection
    pub fn SSL_set_verify_depth(ssl: *mut SSL, depth: c_int);

    /// Set cipher list on connection
    pub fn SSL_set_cipher_list(ssl: *mut SSL, str: *const c_char) -> c_int;

    /// Set ciphersuites on connection (TLS 1.3)
    pub fn SSL_set_ciphersuites(ssl: *mut SSL, str: *const c_char) -> c_int;

    // ========================================================================
    // BIO Functions
    // ========================================================================

    /// Create new BIO
    pub fn BIO_new(method: *const BIO_METHOD) -> *mut BIO;

    /// Free BIO
    pub fn BIO_free(bio: *mut BIO) -> c_int;

    /// Free all BIOs in chain
    pub fn BIO_free_all(bio: *mut BIO);

    /// Create socket BIO
    pub fn BIO_new_socket(sock: c_int, close_flag: c_int) -> *mut BIO;

    /// Create file BIO
    pub fn BIO_new_file(filename: *const c_char, mode: *const c_char) -> *mut BIO;

    /// Create memory BIO
    pub fn BIO_new_mem_buf(buf: *const c_uchar, len: c_int) -> *mut BIO;

    /// Read from BIO
    pub fn BIO_read(bio: *mut BIO, buf: *mut c_uchar, len: c_int) -> c_int;

    /// Write to BIO
    pub fn BIO_write(bio: *mut BIO, buf: *const c_uchar, len: c_int) -> c_int;

    /// Set SSL for BIO
    pub fn SSL_set_bio(ssl: *mut SSL, rbio: *mut BIO, wbio: *mut BIO);

    // ========================================================================
    // X509 Functions
    // ========================================================================

    /// Free X509 certificate
    pub fn X509_free(x509: *mut X509);

    /// Get subject name
    pub fn X509_get_subject_name(x509: *const X509) -> *mut X509_NAME;

    /// Get issuer name
    pub fn X509_get_issuer_name(x509: *const X509) -> *mut X509_NAME;

    /// Get name as one-line string
    pub fn X509_NAME_oneline(name: *const X509_NAME, buf: *mut c_char, size: c_int) -> *mut c_char;

    // ========================================================================
    // Session Functions
    // ========================================================================

    /// Free SSL session
    pub fn SSL_SESSION_free(session: *mut SSL_SESSION);

    /// Get session timeout
    pub fn SSL_SESSION_get_timeout(session: *const SSL_SESSION) -> c_ulong;

    /// Set session timeout
    pub fn SSL_SESSION_set_timeout(session: *mut SSL_SESSION, timeout: c_ulong) -> c_ulong;

    // ========================================================================
    // Error Functions
    // ========================================================================

    /// Get last error
    pub fn ERR_get_error() -> c_ulong;

    /// Peek at last error
    pub fn ERR_peek_error() -> c_ulong;

    /// Clear error queue
    pub fn ERR_clear_error();

    /// Get error string
    pub fn ERR_error_string(e: c_ulong, buf: *mut c_char) -> *const c_char;

    /// Get error string (safer version)
    pub fn ERR_error_string_n(e: c_ulong, buf: *mut c_char, len: usize);

    /// Print errors to file
    pub fn ERR_print_errors_fp(fp: *mut c_void);

    // ========================================================================
    // Version Functions
    // ========================================================================

    /// Get version string
    pub fn SSL_version_str() -> *const c_char;

    /// Get OpenSSL version number
    pub fn OpenSSL_version_num() -> c_ulong;

    /// Get OpenSSL version string
    pub fn OpenSSL_version(type_: c_int) -> *const c_char;

    // ========================================================================
    // Extended Functions
    // ========================================================================

    /// SSL read with extended error handling
    pub fn SSL_read_ex(
        ssl: *mut SSL,
        buf: *mut c_uchar,
        num: usize,
        readbytes: *mut usize,
    ) -> c_int;

    /// SSL write with extended error handling
    pub fn SSL_write_ex(
        ssl: *mut SSL,
        buf: *const c_uchar,
        num: usize,
        written: *mut usize,
    ) -> c_int;

    /// Set minimum protocol version on connection
    pub fn SSL_set_min_proto_version(ssl: *mut SSL, version: c_int) -> c_int;

    /// Set maximum protocol version on connection
    pub fn SSL_set_max_proto_version(ssl: *mut SSL, version: c_int) -> c_int;

    /// Get minimum protocol version from connection
    pub fn SSL_get_min_proto_version(ssl: *const SSL) -> c_int;

    /// Get maximum protocol version from connection
    pub fn SSL_get_max_proto_version(ssl: *const SSL) -> c_int;

    /// Get peer certificate (OpenSSL 3.0 name)
    pub fn SSL_get1_peer_certificate(ssl: *const SSL) -> *mut X509;

    /// Get certificate store from context
    pub fn SSL_CTX_get_cert_store(ctx: *const SSL_CTX) -> *mut c_void;

    /// Check if SSL is server
    pub fn SSL_is_server(ssl: *const SSL) -> c_int;

    /// Get verification mode
    pub fn SSL_get_verify_mode(ssl: *const SSL) -> c_int;

    /// Get verification depth
    pub fn SSL_get_verify_depth(ssl: *const SSL) -> c_int;

    /// Get SSL context from connection
    pub fn SSL_get_SSL_CTX(ssl: *const SSL) -> *mut SSL_CTX;
}
