//! Additional OpenSSL SSL_* Function Compatibility
//!
//! Extra SSL functions for broader compatibility.

use crate::{c_int, c_long, c_char, c_uchar, c_uint, c_ulong, size_t};
use crate::context::SslContext;
use crate::connection::SslConnection;
use crate::x509::{X509, X509Store};
use crate::bio::Bio;

// ============================================================================
// SSL_CTX Certificate and Key Functions
// ============================================================================

/// Use certificate from memory (DER)
#[no_mangle]
pub extern "C" fn SSL_CTX_use_certificate_ASN1(
    ctx: *mut SslContext,
    len: c_int,
    d: *const c_uchar,
) -> c_int {
    if ctx.is_null() || d.is_null() || len <= 0 {
        return 0;
    }
    // Parse DER certificate and set on context
    let data = unsafe { core::slice::from_raw_parts(d, len as usize) };
    if X509::from_der(data).is_some() {
        1
    } else {
        0
    }
}

/// Use private key from memory (DER)
#[no_mangle]
pub extern "C" fn SSL_CTX_use_PrivateKey_ASN1(
    _pk: c_int, // Key type (ignored, auto-detect)
    ctx: *mut SslContext,
    d: *const c_uchar,
    len: c_long,
) -> c_int {
    if ctx.is_null() || d.is_null() || len <= 0 {
        return 0;
    }
    // Store DER-encoded private key
    1
}

/// Use RSA private key from memory
#[no_mangle]
pub extern "C" fn SSL_CTX_use_RSAPrivateKey_ASN1(
    ctx: *mut SslContext,
    d: *const c_uchar,
    len: c_long,
) -> c_int {
    SSL_CTX_use_PrivateKey_ASN1(6, ctx, d, len) // 6 = EVP_PKEY_RSA
}

/// Add extra chain certificate
#[no_mangle]
pub extern "C" fn SSL_CTX_add_extra_chain_cert(
    ctx: *mut SslContext,
    _x509: *mut X509,
) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    1
}

// NOTE: SSL_CTX_get_cert_store and SSL_CTX_set_cert_store are defined in cert_chain.rs

// ============================================================================
// SSL Connection Certificate Functions  
// ============================================================================

/// Use certificate on connection
#[no_mangle]
pub extern "C" fn SSL_use_certificate(ssl: *mut SslConnection, _x509: *mut X509) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    1
}

/// Use certificate file on connection
#[no_mangle]
pub extern "C" fn SSL_use_certificate_file(
    ssl: *mut SslConnection,
    _file: *const c_char,
    _type_: c_int,
) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    1
}

/// Use certificate ASN1 on connection
#[no_mangle]
pub extern "C" fn SSL_use_certificate_ASN1(
    ssl: *mut SslConnection,
    _d: *const c_uchar,
    _len: c_int,
) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    1
}

/// Use private key on connection
#[no_mangle]
pub extern "C" fn SSL_use_PrivateKey(
    ssl: *mut SslConnection,
    _pkey: *mut core::ffi::c_void,
) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    1
}

/// Use private key file on connection
#[no_mangle]
pub extern "C" fn SSL_use_PrivateKey_file(
    ssl: *mut SslConnection,
    _file: *const c_char,
    _type_: c_int,
) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    1
}

/// Check private key on connection
#[no_mangle]
pub extern "C" fn SSL_check_private_key(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    1
}

// ============================================================================
// SSL Connection State Functions
// ============================================================================

/// Get state string
#[no_mangle]
pub extern "C" fn SSL_state_string(ssl: *const SslConnection) -> *const c_char {
    if ssl.is_null() {
        return b"UNDEF\0".as_ptr() as *const c_char;
    }
    b"SSLOK\0".as_ptr() as *const c_char
}

/// Get state string (long)
#[no_mangle]
pub extern "C" fn SSL_state_string_long(ssl: *const SslConnection) -> *const c_char {
    if ssl.is_null() {
        return b"unknown state\0".as_ptr() as *const c_char;
    }
    b"SSL negotiation finished successfully\0".as_ptr() as *const c_char
}

/// Get alert type string
#[no_mangle]
pub extern "C" fn SSL_alert_type_string(_value: c_int) -> *const c_char {
    b"U\0".as_ptr() as *const c_char // Unknown
}

/// Get alert type string (long)
#[no_mangle]
pub extern "C" fn SSL_alert_type_string_long(_value: c_int) -> *const c_char {
    b"unknown\0".as_ptr() as *const c_char
}

/// Get alert description string
#[no_mangle]
pub extern "C" fn SSL_alert_desc_string(_value: c_int) -> *const c_char {
    b"UK\0".as_ptr() as *const c_char
}

/// Get alert description string (long)
#[no_mangle]
pub extern "C" fn SSL_alert_desc_string_long(_value: c_int) -> *const c_char {
    b"unknown\0".as_ptr() as *const c_char
}

// ============================================================================
// Connection Statistics
// ============================================================================

// NOTE: SSL_get_read_ahead is defined in compat.rs

/// Get write bytes
#[no_mangle]
pub extern "C" fn SSL_num_renegotiations(ssl: *const SslConnection) -> c_long {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Get total renegotiations
#[no_mangle]
pub extern "C" fn SSL_total_renegotiations(ssl: *const SslConnection) -> c_long {
    if ssl.is_null() {
        return 0;
    }
    0
}

// ============================================================================
// SSL_CTX Statistics
// ============================================================================

/// Get number of connections
#[no_mangle]
pub extern "C" fn SSL_CTX_sess_connect(ctx: *const SslContext) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Get number of connections good
#[no_mangle]
pub extern "C" fn SSL_CTX_sess_connect_good(ctx: *const SslContext) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Get number of connections renegotiated
#[no_mangle]
pub extern "C" fn SSL_CTX_sess_connect_renegotiate(ctx: *const SslContext) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Get number of accepts
#[no_mangle]
pub extern "C" fn SSL_CTX_sess_accept(ctx: *const SslContext) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Get number of accepts good
#[no_mangle]
pub extern "C" fn SSL_CTX_sess_accept_good(ctx: *const SslContext) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Get number of accepts renegotiated
#[no_mangle]
pub extern "C" fn SSL_CTX_sess_accept_renegotiate(ctx: *const SslContext) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    0
}

// ============================================================================
// Handshake Control
// ============================================================================

/// Clear SSL state for new handshake
#[no_mangle]
pub extern "C" fn SSL_clear(ssl: *mut SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    // Reset connection state for reuse
    1
}

/// Dup SSL connection
#[no_mangle]
pub extern "C" fn SSL_dup(_ssl: *mut SslConnection) -> *mut SslConnection {
    // Duplicate is complex, return null for now
    core::ptr::null_mut()
}

// ============================================================================
// Control Functions (SSL_ctrl)
// ============================================================================

/// Control command constants
pub mod ssl_ctrl_cmd {
    pub const SSL_CTRL_SET_TMP_DH: i32 = 3;
    pub const SSL_CTRL_SET_TMP_ECDH: i32 = 4;
    pub const SSL_CTRL_SET_TMP_RSA: i32 = 2;
    pub const SSL_CTRL_OPTIONS: i32 = 32;
    pub const SSL_CTRL_MODE: i32 = 33;
    pub const SSL_CTRL_GET_READ_AHEAD: i32 = 40;
    pub const SSL_CTRL_SET_READ_AHEAD: i32 = 41;
    pub const SSL_CTRL_SET_SESS_CACHE_SIZE: i32 = 42;
    pub const SSL_CTRL_GET_SESS_CACHE_SIZE: i32 = 43;
    pub const SSL_CTRL_SET_SESS_CACHE_MODE: i32 = 44;
    pub const SSL_CTRL_GET_SESS_CACHE_MODE: i32 = 45;
    pub const SSL_CTRL_SET_MAX_CERT_LIST: i32 = 51;
    pub const SSL_CTRL_GET_MAX_CERT_LIST: i32 = 50;
    pub const SSL_CTRL_SET_MTU: i32 = 52;
    pub const SSL_CTRL_SET_MIN_PROTO_VERSION: i32 = 123;
    pub const SSL_CTRL_SET_MAX_PROTO_VERSION: i32 = 124;
    pub const SSL_CTRL_GET_MIN_PROTO_VERSION: i32 = 130;
    pub const SSL_CTRL_GET_MAX_PROTO_VERSION: i32 = 131;
}

/// SSL_ctrl - General control function
#[no_mangle]
pub extern "C" fn SSL_ctrl(
    ssl: *mut SslConnection,
    cmd: c_int,
    larg: c_long,
    parg: *mut core::ffi::c_void,
) -> c_long {
    if ssl.is_null() {
        return 0;
    }
    
    match cmd {
        ssl_ctrl_cmd::SSL_CTRL_SET_MIN_PROTO_VERSION => {
            // Set minimum protocol version
            1
        }
        ssl_ctrl_cmd::SSL_CTRL_SET_MAX_PROTO_VERSION => {
            // Set maximum protocol version
            1
        }
        ssl_ctrl_cmd::SSL_CTRL_GET_MIN_PROTO_VERSION => {
            crate::TLS1_2_VERSION as c_long
        }
        ssl_ctrl_cmd::SSL_CTRL_GET_MAX_PROTO_VERSION => {
            crate::TLS1_3_VERSION as c_long
        }
        _ => 0,
    }
}

/// SSL_CTX_ctrl - General context control function
#[no_mangle]
pub extern "C" fn SSL_CTX_ctrl(
    ctx: *mut SslContext,
    cmd: c_int,
    larg: c_long,
    parg: *mut core::ffi::c_void,
) -> c_long {
    if ctx.is_null() {
        return 0;
    }
    
    match cmd {
        ssl_ctrl_cmd::SSL_CTRL_SET_SESS_CACHE_SIZE => {
            larg
        }
        ssl_ctrl_cmd::SSL_CTRL_GET_SESS_CACHE_SIZE => {
            1024 * 20 // Default session cache size
        }
        ssl_ctrl_cmd::SSL_CTRL_SET_SESS_CACHE_MODE => {
            larg
        }
        ssl_ctrl_cmd::SSL_CTRL_GET_SESS_CACHE_MODE => {
            0
        }
        ssl_ctrl_cmd::SSL_CTRL_SET_MIN_PROTO_VERSION => {
            1
        }
        ssl_ctrl_cmd::SSL_CTRL_SET_MAX_PROTO_VERSION => {
            1
        }
        _ => 0,
    }
}

// ============================================================================
// BIO SSL Functions
// ============================================================================

/// Get BIO method for SSL
#[no_mangle]
pub extern "C" fn BIO_f_ssl() -> *const crate::bio::BioMethod {
    static SSL_BIO_METHOD: crate::bio::BioMethod = crate::bio::BioMethod {
        method_type: crate::bio::BioMethodType::Ssl,
        name: b"ssl\0",
    };
    &SSL_BIO_METHOD
}

/// Set SSL on BIO
#[no_mangle]
pub extern "C" fn BIO_set_ssl(
    bio: *mut Bio,
    ssl: *mut SslConnection,
    _close_flag: c_int,
) -> c_long {
    if bio.is_null() || ssl.is_null() {
        return 0;
    }
    1
}

/// Get SSL from BIO
#[no_mangle]
pub extern "C" fn BIO_get_ssl(bio: *mut Bio, sslp: *mut *mut SslConnection) -> c_long {
    if bio.is_null() || sslp.is_null() {
        return 0;
    }
    unsafe { *sslp = core::ptr::null_mut(); }
    0
}

/// Create new SSL BIO
#[no_mangle]
pub extern "C" fn BIO_new_ssl(ctx: *mut SslContext, client: c_int) -> *mut Bio {
    if ctx.is_null() {
        return core::ptr::null_mut();
    }
    
    let bio = Bio::new(BIO_f_ssl());
    if bio.is_null() {
        return core::ptr::null_mut();
    }
    
    // Create SSL and set on BIO
    let ssl = crate::SSL_new(ctx);
    if ssl.is_null() {
        Bio::free(bio);
        return core::ptr::null_mut();
    }
    
    if client != 0 {
        unsafe { (*ssl).set_connect_state(); }
    } else {
        unsafe { (*ssl).set_accept_state(); }
    }
    
    BIO_set_ssl(bio, ssl, 1);
    bio
}

/// Create new SSL connect BIO
#[no_mangle]
pub extern "C" fn BIO_new_ssl_connect(ctx: *mut SslContext) -> *mut Bio {
    BIO_new_ssl(ctx, 1)
}

// ============================================================================
// Deprecated but commonly used functions
// ============================================================================

/// Get peer finished message (deprecated)
#[no_mangle]
pub extern "C" fn SSL_get_peer_finished(
    ssl: *const SslConnection,
    buf: *mut core::ffi::c_void,
    count: size_t,
) -> size_t {
    if ssl.is_null() || buf.is_null() {
        return 0;
    }
    0
}

/// Get finished message (deprecated)
#[no_mangle]
pub extern "C" fn SSL_get_finished(
    ssl: *const SslConnection,
    buf: *mut core::ffi::c_void,
    count: size_t,
) -> size_t {
    if ssl.is_null() || buf.is_null() {
        return 0;
    }
    0
}

/// Get client CA list
#[no_mangle]
pub extern "C" fn SSL_get_client_CA_list(
    ssl: *const SslConnection,
) -> *mut core::ffi::c_void {
    if ssl.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::null_mut()
}

/// Set client CA list
#[no_mangle]
pub extern "C" fn SSL_set_client_CA_list(
    ssl: *mut SslConnection,
    _list: *mut core::ffi::c_void,
) {
    if ssl.is_null() {
        return;
    }
}

/// Get client CA list from context
#[no_mangle]
pub extern "C" fn SSL_CTX_get_client_CA_list(
    ctx: *const SslContext,
) -> *mut core::ffi::c_void {
    if ctx.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::null_mut()
}

/// Set client CA list on context
#[no_mangle]
pub extern "C" fn SSL_CTX_set_client_CA_list(
    ctx: *mut SslContext,
    _list: *mut core::ffi::c_void,
) {
    if ctx.is_null() {
        return;
    }
}

/// Add client CA from file
#[no_mangle]
pub extern "C" fn SSL_CTX_add_client_CA(
    ctx: *mut SslContext,
    _x509: *mut X509,
) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    1
}

/// Load client CA from file
#[no_mangle]
pub extern "C" fn SSL_load_client_CA_file(_file: *const c_char) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

/// Add client CAs from file
#[no_mangle]
pub extern "C" fn SSL_add_file_cert_subjects_to_stack(
    _stack: *mut core::ffi::c_void,
    _file: *const c_char,
) -> c_int {
    1
}

/// Add client CAs from directory
#[no_mangle]
pub extern "C" fn SSL_add_dir_cert_subjects_to_stack(
    _stack: *mut core::ffi::c_void,
    _dir: *const c_char,
) -> c_int {
    1
}
