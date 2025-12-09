//! NexaOS SSL/TLS Library (nssl)
//!
//! A modern, libssl.so ABI-compatible TLS library for NexaOS.
//! 
//! ## Supported Protocols
//! - **TLS 1.3** (RFC 8446) - Recommended, default
//! - **TLS 1.2** (RFC 5246) - For legacy compatibility
//!
//! ## Cipher Suites (TLS 1.3)
//! - TLS_AES_256_GCM_SHA384 (0x1302)
//! - TLS_AES_128_GCM_SHA256 (0x1301)
//! - TLS_CHACHA20_POLY1305_SHA256 (0x1303)
//!
//! ## Cipher Suites (TLS 1.2)
//! - TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
//! - TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
//! - TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
//! - TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
//! - TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
//!
//! ## Key Exchange
//! - X25519 (preferred)
//! - P-256 (secp256r1)
//! - P-384 (secp384r1)
//!
//! ## Signature Algorithms
//! - Ed25519
//! - ECDSA with P-256/P-384
//! - RSA-PSS
//! - RSA PKCS#1 v1.5 (TLS 1.2 only)
//!
//! ## Features
//! - ALPN (Application-Layer Protocol Negotiation)
//! - SNI (Server Name Indication)
//! - Session Resumption (PSK)
//! - 0-RTT Early Data (TLS 1.3)
//! - OCSP Stapling
//! - Certificate Transparency
//!
//! ## Security Design
//! - **No SSLv2, SSLv3, TLS 1.0, TLS 1.1** - Deprecated protocols removed
//! - **No weak ciphers** - No RC4, DES, 3DES, MD5-based MACs
//! - **No static RSA** - Only ephemeral key exchange
//! - **No export ciphers** - All removed
//! - **Constant-time operations** - Side-channel resistant
//!
//! # Usage
//! ```rust
//! use nssl::{SslContext, SslMethod};
//!
//! // Create a TLS 1.3 client context
//! let ctx = SslContext::new(SslMethod::tls_client())?;
//! 
//! // Create connection and perform handshake
//! let ssl = ctx.new_ssl()?;
//! ssl.set_fd(socket_fd);
//! ssl.connect()?;
//! 
//! // Send/receive data
//! ssl.write(b"GET / HTTP/1.1\r\n\r\n")?;
//! let mut buf = [0u8; 4096];
//! let n = ssl.read(&mut buf)?;
//! ```

#![feature(linkage)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// FFI bindings to ncryptolib (libcrypto.so) - provides stable C ABI
pub mod crypto_ffi;

// Re-export crypto_ffi as ncryptolib for compatibility
// This allows existing code using crate::ncryptolib:: to work with minimal changes
pub use crypto_ffi as ncryptolib;

// ============================================================================
// Module Declarations
// ============================================================================

// Core SSL/TLS context and connection management
pub mod ssl;
pub mod context;
pub mod connection;

// TLS Protocol Implementation
pub mod tls;
pub mod record;
pub mod handshake;
pub mod alert;

// Cipher suites
pub mod cipher;
pub mod cipher_suites;

// Key exchange
pub mod kex;

// Certificate handling
pub mod x509;
pub mod x509_verify;
pub mod cert_verify;
pub mod cert_chain;

// Session management
pub mod session;

// Extensions
pub mod extensions;

// Error handling
pub mod error;

// BIO (Basic I/O) abstraction
pub mod bio;

// OpenSSL C ABI compatibility
pub mod compat;

// TLS 1.3 Early Data (0-RTT)
pub mod early_data;

// Post-handshake operations
pub mod post_handshake;

// Session tickets and PSK
pub mod tickets;

// Additional SSL functions
pub mod ssl_extra;

// ============================================================================
// C Type Definitions (OpenSSL compatible)
// ============================================================================


pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type c_char = i8;
pub type c_uchar = u8;
pub type size_t = usize;
pub type ssize_t = isize;

// ============================================================================
// Version Constants
// ============================================================================

/// Library version string
pub const NSSL_VERSION: &str = "nssl 1.0.0";

/// OpenSSL-compatible version number (format: 0xMNNFFPPS)
pub const OPENSSL_VERSION_NUMBER: u64 = 0x30000000; // Mimic OpenSSL 3.0.0

/// TLS version constants
pub const TLS1_VERSION: u16 = 0x0301;
pub const TLS1_1_VERSION: u16 = 0x0302;
pub const TLS1_2_VERSION: u16 = 0x0303;
pub const TLS1_3_VERSION: u16 = 0x0304;

/// Minimum supported TLS version
pub const TLS_MIN_VERSION: u16 = TLS1_2_VERSION;

/// Maximum supported TLS version
pub const TLS_MAX_VERSION: u16 = TLS1_3_VERSION;

// ============================================================================
// Error Codes
// ============================================================================

/// SSL error codes
pub mod ssl_error {
    pub const SSL_ERROR_NONE: i32 = 0;
    pub const SSL_ERROR_SSL: i32 = 1;
    pub const SSL_ERROR_WANT_READ: i32 = 2;
    pub const SSL_ERROR_WANT_WRITE: i32 = 3;
    pub const SSL_ERROR_WANT_X509_LOOKUP: i32 = 4;
    pub const SSL_ERROR_SYSCALL: i32 = 5;
    pub const SSL_ERROR_ZERO_RETURN: i32 = 6;
    pub const SSL_ERROR_WANT_CONNECT: i32 = 7;
    pub const SSL_ERROR_WANT_ACCEPT: i32 = 8;
    pub const SSL_ERROR_WANT_ASYNC: i32 = 9;
    pub const SSL_ERROR_WANT_ASYNC_JOB: i32 = 10;
    pub const SSL_ERROR_WANT_CLIENT_HELLO_CB: i32 = 11;
}

// ============================================================================
// SSL Options
// ============================================================================

pub mod ssl_options {
    // Session options
    pub const SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION: u64 = 0x00010000;
    
    // Protocol version options (we only support TLS 1.2+)
    pub const SSL_OP_NO_SSLv2: u64 = 0x01000000;   // Always set internally
    pub const SSL_OP_NO_SSLv3: u64 = 0x02000000;   // Always set internally
    pub const SSL_OP_NO_TLSv1: u64 = 0x04000000;   // Always set internally
    pub const SSL_OP_NO_TLSv1_1: u64 = 0x10000000; // Always set internally
    pub const SSL_OP_NO_TLSv1_2: u64 = 0x08000000;
    pub const SSL_OP_NO_TLSv1_3: u64 = 0x20000000;
    
    // Cipher preferences
    pub const SSL_OP_CIPHER_SERVER_PREFERENCE: u64 = 0x00400000;
    
    // Ticket options
    pub const SSL_OP_NO_TICKET: u64 = 0x00004000;
    
    // Compression (always disabled)
    pub const SSL_OP_NO_COMPRESSION: u64 = 0x00020000;
    
    // Renegotiation
    pub const SSL_OP_NO_RENEGOTIATION: u64 = 0x40000000;
    
    // Default secure options
    pub const SSL_OP_ALL: u64 = SSL_OP_NO_SSLv2 | SSL_OP_NO_SSLv3 | 
                                SSL_OP_NO_TLSv1 | SSL_OP_NO_TLSv1_1 |
                                SSL_OP_NO_COMPRESSION;
}

// ============================================================================
// SSL Mode
// ============================================================================

pub mod ssl_mode {
    pub const SSL_MODE_ENABLE_PARTIAL_WRITE: u64 = 0x00000001;
    pub const SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER: u64 = 0x00000002;
    pub const SSL_MODE_AUTO_RETRY: u64 = 0x00000004;
    pub const SSL_MODE_NO_AUTO_CHAIN: u64 = 0x00000008;
    pub const SSL_MODE_RELEASE_BUFFERS: u64 = 0x00000010;
    pub const SSL_MODE_SEND_FALLBACK_SCSV: u64 = 0x00000080;
}

// ============================================================================
// Verify Modes
// ============================================================================

pub mod ssl_verify {
    pub const SSL_VERIFY_NONE: i32 = 0x00;
    pub const SSL_VERIFY_PEER: i32 = 0x01;
    pub const SSL_VERIFY_FAIL_IF_NO_PEER_CERT: i32 = 0x02;
    pub const SSL_VERIFY_CLIENT_ONCE: i32 = 0x04;
    pub const SSL_VERIFY_POST_HANDSHAKE: i32 = 0x08;
}

// ============================================================================
// Filetype Constants
// ============================================================================

pub const SSL_FILETYPE_PEM: i32 = 1;
pub const SSL_FILETYPE_ASN1: i32 = 2;

// ============================================================================
// Re-exports for public API
// ============================================================================

pub use ssl::{SslMethod, SslMethodType};
pub use context::SslContext;
pub use connection::SslConnection;
pub use error::{SslError, SslResult};
pub use session::SslSession;
pub use bio::{Bio, BioMethod};
pub use x509::{X509, X509Store, X509VerifyParam};
pub use cipher::{SslCipher, CipherList};
pub use cipher_suites::{CipherSuite, TLS13_CIPHER_SUITES, TLS12_CIPHER_SUITES};

// ============================================================================
// C ABI Exports - Version Functions
// ============================================================================

/// Get SSL library version string (OpenSSL compatible)
#[no_mangle]
pub extern "C" fn SSL_version_str() -> *const c_char {
    b"nssl 1.0.0\0".as_ptr() as *const c_char
}

// NOTE: SSLeay() and SSLeay_version() are provided by ncryptolib

// ============================================================================
// C ABI Exports - Library Initialization
// ============================================================================

/// Initialize the SSL library
#[no_mangle]
pub extern "C" fn SSL_library_init() -> c_int {
    // Initialize crypto library
    unsafe { crate::ncryptolib::OPENSSL_init_crypto(0, core::ptr::null()) };
    1 // Success
}

/// Initialize SSL library (OpenSSL 1.1+ compatible)
#[no_mangle]
pub extern "C" fn OPENSSL_init_ssl(_opts: u64, _settings: *const core::ffi::c_void) -> c_int {
    SSL_library_init()
}

/// Load SSL error strings
#[no_mangle]
pub extern "C" fn SSL_load_error_strings() {
    // No-op: errors are always available
}

/// Add SSL algorithms (legacy, use ncryptolib's version if needed)
pub fn ssl_add_algorithms_internal() -> c_int {
    SSL_library_init()
}

// ============================================================================
// C ABI Exports - SSL_METHOD
// ============================================================================

/// Get TLS client method (supports TLS 1.2 and 1.3)
#[no_mangle]
pub extern "C" fn TLS_client_method() -> *const ssl::SslMethod {
    &ssl::TLS_CLIENT_METHOD as *const _
}

/// Get TLS server method (supports TLS 1.2 and 1.3)
#[no_mangle]
pub extern "C" fn TLS_server_method() -> *const ssl::SslMethod {
    &ssl::TLS_SERVER_METHOD as *const _
}

/// Get TLS method (auto-negotiate, supports TLS 1.2 and 1.3)
#[no_mangle]
pub extern "C" fn TLS_method() -> *const ssl::SslMethod {
    &ssl::TLS_METHOD as *const _
}

/// Get TLS 1.2 client method
#[no_mangle]
pub extern "C" fn TLSv1_2_client_method() -> *const ssl::SslMethod {
    &ssl::TLS12_CLIENT_METHOD as *const _
}

/// Get TLS 1.2 server method
#[no_mangle]
pub extern "C" fn TLSv1_2_server_method() -> *const ssl::SslMethod {
    &ssl::TLS12_SERVER_METHOD as *const _
}

// Legacy methods - return TLS method with warning
// SSLv2, SSLv3, TLS 1.0, TLS 1.1 are NOT supported

/// SSLv23_client_method - returns TLS method (legacy compatibility)
#[no_mangle]
pub extern "C" fn SSLv23_client_method() -> *const ssl::SslMethod {
    TLS_client_method()
}

/// SSLv23_server_method - returns TLS method (legacy compatibility)
#[no_mangle]
pub extern "C" fn SSLv23_server_method() -> *const ssl::SslMethod {
    TLS_server_method()
}

/// SSLv23_method - returns TLS method (legacy compatibility)
#[no_mangle]
pub extern "C" fn SSLv23_method() -> *const ssl::SslMethod {
    TLS_method()
}

// ============================================================================
// C ABI Exports - SSL_CTX
// ============================================================================

/// Create a new SSL context
#[no_mangle]
pub extern "C" fn SSL_CTX_new(method: *const ssl::SslMethod) -> *mut context::SslContext {
    if method.is_null() {
        return core::ptr::null_mut();
    }
    
    match context::SslContext::new(unsafe { &*method }) {
        Ok(ctx) => Box::into_raw(Box::new(ctx)),
        Err(_) => core::ptr::null_mut(),
    }
}

/// Free an SSL context
#[no_mangle]
pub extern "C" fn SSL_CTX_free(ctx: *mut context::SslContext) {
    if !ctx.is_null() {
        unsafe { drop(Box::from_raw(ctx)); }
    }
}

/// Set SSL context options
#[no_mangle]
pub extern "C" fn SSL_CTX_set_options(ctx: *mut context::SslContext, options: c_ulong) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).set_options(options) }
}

/// Get SSL context options
#[no_mangle]
pub extern "C" fn SSL_CTX_get_options(ctx: *const context::SslContext) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).get_options() }
}

/// Clear SSL context options
#[no_mangle]
pub extern "C" fn SSL_CTX_clear_options(ctx: *mut context::SslContext, options: c_ulong) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).clear_options(options) }
}

/// Set minimum protocol version
#[no_mangle]
pub extern "C" fn SSL_CTX_set_min_proto_version(ctx: *mut context::SslContext, version: c_int) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    // Enforce minimum TLS 1.2
    let version = if (version as u16) < TLS1_2_VERSION {
        TLS1_2_VERSION as i32
    } else {
        version
    };
    unsafe { (*ctx).set_min_proto_version(version as u16) as c_int }
}

/// Set maximum protocol version
#[no_mangle]
pub extern "C" fn SSL_CTX_set_max_proto_version(ctx: *mut context::SslContext, version: c_int) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).set_max_proto_version(version as u16) as c_int }
}

/// Set cipher list (TLS 1.2)
#[no_mangle]
pub extern "C" fn SSL_CTX_set_cipher_list(ctx: *mut context::SslContext, str: *const c_char) -> c_int {
    if ctx.is_null() || str.is_null() {
        return 0;
    }
    unsafe {
        let cipher_str = core::ffi::CStr::from_ptr(str as *const i8);
        match cipher_str.to_str() {
            Ok(s) => (*ctx).set_cipher_list(s) as c_int,
            Err(_) => 0,
        }
    }
}

/// Set ciphersuites (TLS 1.3)
#[no_mangle]
pub extern "C" fn SSL_CTX_set_ciphersuites(ctx: *mut context::SslContext, str: *const c_char) -> c_int {
    if ctx.is_null() || str.is_null() {
        return 0;
    }
    unsafe {
        let cipher_str = core::ffi::CStr::from_ptr(str as *const i8);
        match cipher_str.to_str() {
            Ok(s) => (*ctx).set_ciphersuites(s) as c_int,
            Err(_) => 0,
        }
    }
}

/// Set verification mode
#[no_mangle]
pub extern "C" fn SSL_CTX_set_verify(
    ctx: *mut context::SslContext,
    mode: c_int,
    callback: Option<extern "C" fn(c_int, *mut x509::X509StoreCtx) -> c_int>
) {
    if ctx.is_null() {
        return;
    }
    unsafe { (*ctx).set_verify(mode, callback); }
}

/// Set verification depth
#[no_mangle]
pub extern "C" fn SSL_CTX_set_verify_depth(ctx: *mut context::SslContext, depth: c_int) {
    if ctx.is_null() {
        return;
    }
    unsafe { (*ctx).set_verify_depth(depth as usize); }
}

/// Load certificate file
#[no_mangle]
pub extern "C" fn SSL_CTX_use_certificate_file(
    ctx: *mut context::SslContext,
    file: *const c_char,
    type_: c_int
) -> c_int {
    if ctx.is_null() || file.is_null() {
        return 0;
    }
    unsafe {
        let path = core::ffi::CStr::from_ptr(file as *const i8);
        match path.to_str() {
            Ok(s) => (*ctx).use_certificate_file(s, type_) as c_int,
            Err(_) => 0,
        }
    }
}

/// Load certificate chain file
#[no_mangle]
pub extern "C" fn SSL_CTX_use_certificate_chain_file(
    ctx: *mut context::SslContext,
    file: *const c_char
) -> c_int {
    if ctx.is_null() || file.is_null() {
        return 0;
    }
    unsafe {
        let path = core::ffi::CStr::from_ptr(file as *const i8);
        match path.to_str() {
            Ok(s) => (*ctx).use_certificate_chain_file(s) as c_int,
            Err(_) => 0,
        }
    }
}

/// Load private key file
#[no_mangle]
pub extern "C" fn SSL_CTX_use_PrivateKey_file(
    ctx: *mut context::SslContext,
    file: *const c_char,
    type_: c_int
) -> c_int {
    if ctx.is_null() || file.is_null() {
        return 0;
    }
    unsafe {
        let path = core::ffi::CStr::from_ptr(file as *const i8);
        match path.to_str() {
            Ok(s) => (*ctx).use_private_key_file(s, type_) as c_int,
            Err(_) => 0,
        }
    }
}

/// Check private key
#[no_mangle]
pub extern "C" fn SSL_CTX_check_private_key(ctx: *const context::SslContext) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).check_private_key() as c_int }
}

/// Load CA certificates from file
#[no_mangle]
pub extern "C" fn SSL_CTX_load_verify_locations(
    ctx: *mut context::SslContext,
    ca_file: *const c_char,
    ca_path: *const c_char
) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    
    let file = if ca_file.is_null() {
        None
    } else {
        unsafe {
            core::ffi::CStr::from_ptr(ca_file as *const i8)
                .to_str()
                .ok()
        }
    };
    
    let path = if ca_path.is_null() {
        None
    } else {
        unsafe {
            core::ffi::CStr::from_ptr(ca_path as *const i8)
                .to_str()
                .ok()
        }
    };
    
    unsafe { (*ctx).load_verify_locations(file, path) as c_int }
}

/// Set default verify paths
#[no_mangle]
pub extern "C" fn SSL_CTX_set_default_verify_paths(ctx: *mut context::SslContext) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    unsafe { (*ctx).set_default_verify_paths() as c_int }
}

/// Set ALPN protocols
#[no_mangle]
pub extern "C" fn SSL_CTX_set_alpn_protos(
    ctx: *mut context::SslContext,
    protos: *const c_uchar,
    protos_len: c_uint
) -> c_int {
    if ctx.is_null() || protos.is_null() {
        return 1; // Error
    }
    unsafe {
        let proto_slice = core::slice::from_raw_parts(protos, protos_len as usize);
        if (*ctx).set_alpn_protos(proto_slice) {
            0 // Success
        } else {
            1 // Error
        }
    }
}

// ============================================================================
// C ABI Exports - SSL
// ============================================================================

/// Create a new SSL connection
#[no_mangle]
pub extern "C" fn SSL_new(ctx: *mut context::SslContext) -> *mut connection::SslConnection {
    if ctx.is_null() {
        return core::ptr::null_mut();
    }
    
    match unsafe { (*ctx).new_ssl() } {
        Ok(ssl) => Box::into_raw(Box::new(ssl)),
        Err(_) => core::ptr::null_mut(),
    }
}

/// Free an SSL connection
#[no_mangle]
pub extern "C" fn SSL_free(ssl: *mut connection::SslConnection) {
    if !ssl.is_null() {
        unsafe { drop(Box::from_raw(ssl)); }
    }
}

/// Set the file descriptor
#[no_mangle]
pub extern "C" fn SSL_set_fd(ssl: *mut connection::SslConnection, fd: c_int) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    unsafe { (*ssl).set_fd(fd) as c_int }
}

/// Get the file descriptor
#[no_mangle]
pub extern "C" fn SSL_get_fd(ssl: *const connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return -1;
    }
    unsafe { (*ssl).get_fd() }
}

/// Perform TLS handshake (client)
#[no_mangle]
pub extern "C" fn SSL_connect(ssl: *mut connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return -1;
    }
    unsafe { (*ssl).connect() }
}

/// Perform TLS handshake (server)
#[no_mangle]
pub extern "C" fn SSL_accept(ssl: *mut connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return -1;
    }
    unsafe { (*ssl).accept() }
}

/// Perform TLS handshake (auto-detect client/server)
#[no_mangle]
pub extern "C" fn SSL_do_handshake(ssl: *mut connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return -1;
    }
    unsafe { (*ssl).do_handshake() }
}

/// Read data from SSL connection
#[no_mangle]
pub extern "C" fn SSL_read(ssl: *mut connection::SslConnection, buf: *mut c_uchar, num: c_int) -> c_int {
    if ssl.is_null() || buf.is_null() || num <= 0 {
        return -1;
    }
    unsafe {
        let slice = core::slice::from_raw_parts_mut(buf, num as usize);
        (*ssl).read(slice)
    }
}

/// Write data to SSL connection
#[no_mangle]
pub extern "C" fn SSL_write(ssl: *mut connection::SslConnection, buf: *const c_uchar, num: c_int) -> c_int {
    if ssl.is_null() || buf.is_null() || num <= 0 {
        return -1;
    }
    unsafe {
        let slice = core::slice::from_raw_parts(buf, num as usize);
        (*ssl).write(slice)
    }
}

/// Shutdown SSL connection
#[no_mangle]
pub extern "C" fn SSL_shutdown(ssl: *mut connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return -1;
    }
    unsafe { (*ssl).shutdown() }
}

/// Get SSL error code
#[no_mangle]
pub extern "C" fn SSL_get_error(ssl: *const connection::SslConnection, ret: c_int) -> c_int {
    if ssl.is_null() {
        return ssl_error::SSL_ERROR_SSL;
    }
    unsafe { (*ssl).get_error(ret) }
}

/// Get negotiated protocol version
#[no_mangle]
pub extern "C" fn SSL_version(ssl: *const connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    unsafe { (*ssl).version() as c_int }
}

/// Get protocol version string
#[no_mangle]
pub extern "C" fn SSL_get_version(ssl: *const connection::SslConnection) -> *const c_char {
    if ssl.is_null() {
        return b"unknown\0".as_ptr() as *const c_char;
    }
    unsafe { (*ssl).get_version_string() }
}

/// Get current cipher
#[no_mangle]
pub extern "C" fn SSL_get_current_cipher(ssl: *const connection::SslConnection) -> *const cipher::SslCipher {
    if ssl.is_null() {
        return core::ptr::null();
    }
    unsafe { (*ssl).get_current_cipher() }
}

/// Get cipher name
#[no_mangle]
pub extern "C" fn SSL_CIPHER_get_name(cipher: *const cipher::SslCipher) -> *const c_char {
    if cipher.is_null() {
        return b"(NONE)\0".as_ptr() as *const c_char;
    }
    unsafe { (*cipher).get_name() }
}

/// Set SNI hostname
#[no_mangle]
pub extern "C" fn SSL_set_tlsext_host_name(ssl: *mut connection::SslConnection, name: *const c_char) -> c_int {
    if ssl.is_null() || name.is_null() {
        return 0;
    }
    unsafe {
        let hostname = core::ffi::CStr::from_ptr(name as *const i8);
        match hostname.to_str() {
            Ok(s) => (*ssl).set_hostname(s) as c_int,
            Err(_) => 0,
        }
    }
}

/// Get peer certificate
#[no_mangle]
pub extern "C" fn SSL_get_peer_certificate(ssl: *const connection::SslConnection) -> *mut x509::X509 {
    if ssl.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { (*ssl).get_peer_certificate() }
}

// NOTE: SSL_get_peer_cert_chain is defined in cert_chain.rs

/// Get verify result
#[no_mangle]
pub extern "C" fn SSL_get_verify_result(ssl: *const connection::SslConnection) -> c_long {
    if ssl.is_null() {
        return x509::X509_V_ERR_UNSPECIFIED as c_long;
    }
    unsafe { (*ssl).get_verify_result() as c_long }
}

/// Set connect state (client mode)
#[no_mangle]
pub extern "C" fn SSL_set_connect_state(ssl: *mut connection::SslConnection) {
    if !ssl.is_null() {
        unsafe { (*ssl).set_connect_state(); }
    }
}

/// Set accept state (server mode)
#[no_mangle]
pub extern "C" fn SSL_set_accept_state(ssl: *mut connection::SslConnection) {
    if !ssl.is_null() {
        unsafe { (*ssl).set_accept_state(); }
    }
}

/// Get selected ALPN protocol
#[no_mangle]
pub extern "C" fn SSL_get0_alpn_selected(
    ssl: *const connection::SslConnection,
    data: *mut *const c_uchar,
    len: *mut c_uint
) {
    if ssl.is_null() || data.is_null() || len.is_null() {
        return;
    }
    unsafe {
        let (proto, proto_len) = (*ssl).get_alpn_selected();
        *data = proto;
        *len = proto_len as c_uint;
    }
}

/// Check if session is resumable
#[no_mangle]
pub extern "C" fn SSL_session_reused(ssl: *const connection::SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    unsafe { (*ssl).session_reused() as c_int }
}

/// Get session
#[no_mangle]
pub extern "C" fn SSL_get_session(ssl: *const connection::SslConnection) -> *mut session::SslSession {
    if ssl.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { (*ssl).get_session() }
}

/// Set session
#[no_mangle]
pub extern "C" fn SSL_set_session(ssl: *mut connection::SslConnection, session: *mut session::SslSession) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    unsafe { (*ssl).set_session(session) as c_int }
}

// ============================================================================
// C ABI Exports - BIO
// ============================================================================

/// Create new BIO
#[no_mangle]
pub extern "C" fn BIO_new(method: *const bio::BioMethod) -> *mut bio::Bio {
    bio::Bio::new(method)
}

/// Free BIO
#[no_mangle]
pub extern "C" fn BIO_free(bio: *mut bio::Bio) -> c_int {
    bio::Bio::free(bio)
}

/// Free all BIOs in chain
#[no_mangle]
pub extern "C" fn BIO_free_all(bio: *mut bio::Bio) {
    bio::Bio::free_all(bio)
}

/// Create socket BIO
#[no_mangle]
pub extern "C" fn BIO_new_socket(sock: c_int, close_flag: c_int) -> *mut bio::Bio {
    bio::Bio::new_socket(sock, close_flag)
}

/// Create file BIO
#[no_mangle]
pub extern "C" fn BIO_new_file(filename: *const c_char, mode: *const c_char) -> *mut bio::Bio {
    bio::Bio::new_file(filename, mode)
}

/// Create memory BIO
#[no_mangle]
pub extern "C" fn BIO_new_mem_buf(buf: *const c_uchar, len: c_int) -> *mut bio::Bio {
    bio::Bio::new_mem_buf(buf, len)
}

/// Read from BIO
#[no_mangle]
pub extern "C" fn BIO_read(bio: *mut bio::Bio, buf: *mut c_uchar, len: c_int) -> c_int {
    if bio.is_null() {
        return -1;
    }
    unsafe { (*bio).read(buf, len) }
}

/// Write to BIO
#[no_mangle]
pub extern "C" fn BIO_write(bio: *mut bio::Bio, buf: *const c_uchar, len: c_int) -> c_int {
    if bio.is_null() {
        return -1;
    }
    unsafe { (*bio).write(buf, len) }
}

/// Set SSL for BIO
#[no_mangle]
pub extern "C" fn SSL_set_bio(ssl: *mut connection::SslConnection, rbio: *mut bio::Bio, wbio: *mut bio::Bio) {
    if !ssl.is_null() {
        unsafe { (*ssl).set_bio(rbio, wbio); }
    }
}

// ============================================================================
// C ABI Exports - X509
// ============================================================================

/// Free X509 certificate
#[no_mangle]
pub extern "C" fn X509_free(x509: *mut x509::X509) {
    x509::X509::free(x509)
}

/// Get X509 subject name
#[no_mangle]
pub extern "C" fn X509_get_subject_name(x509: *const x509::X509) -> *mut x509::X509Name {
    if x509.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { (*x509).get_subject_name() }
}

/// Get X509 issuer name
#[no_mangle]
pub extern "C" fn X509_get_issuer_name(x509: *const x509::X509) -> *mut x509::X509Name {
    if x509.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { (*x509).get_issuer_name() }
}

/// Get X509 name entry
#[no_mangle]
pub extern "C" fn X509_NAME_oneline(
    name: *const x509::X509Name,
    buf: *mut c_char,
    size: c_int
) -> *mut c_char {
    x509::X509Name::oneline(name, buf, size)
}

// ============================================================================
// C ABI Exports - Error Handling
// ============================================================================
// Note: ERR_* functions are provided by ncryptolib, not duplicated here
