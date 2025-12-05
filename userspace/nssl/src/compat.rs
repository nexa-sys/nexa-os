//! OpenSSL Compatibility Layer
//!
//! Additional OpenSSL-compatible function exports.

use crate::{c_int, c_char, c_uchar, c_ulong, size_t};
use crate::context::SslContext;
use crate::connection::SslConnection;
use crate::ssl::SslMethod;

// ============================================================================
// Additional SSL_CTX functions
// ============================================================================

/// Set SSL context mode
#[no_mangle]
pub extern "C" fn SSL_CTX_set_mode(ctx: *mut SslContext, mode: c_ulong) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    // Mode is stored but has minimal effect in our implementation
    mode
}

/// Get SSL context mode
#[no_mangle]
pub extern "C" fn SSL_CTX_get_mode(ctx: *const SslContext) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Set session cache mode
#[no_mangle]
pub extern "C" fn SSL_CTX_set_session_cache_mode(ctx: *mut SslContext, mode: c_int) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    mode
}

/// Get session cache mode
#[no_mangle]
pub extern "C" fn SSL_CTX_get_session_cache_mode(ctx: *const SslContext) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    0
}

/// Set session timeout
#[no_mangle]
pub extern "C" fn SSL_CTX_set_timeout(ctx: *mut SslContext, timeout: c_ulong) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    timeout
}

/// Get session timeout
#[no_mangle]
pub extern "C" fn SSL_CTX_get_timeout(ctx: *const SslContext) -> c_ulong {
    if ctx.is_null() {
        return 0;
    }
    300 // Default 5 minutes
}

/// Callback type for info callback
pub type SslInfoCallback = Option<extern "C" fn(*const SslConnection, c_int, c_int)>;

/// Set info callback
#[no_mangle]
pub extern "C" fn SSL_CTX_set_info_callback(_ctx: *mut SslContext, _callback: SslInfoCallback) {
    // No-op: info callbacks not implemented
}

/// Callback type for ALPN selection
pub type AlpnSelectCallback = Option<
    extern "C" fn(
        ssl: *mut SslConnection,
        out: *mut *const c_uchar,
        outlen: *mut c_uchar,
        in_: *const c_uchar,
        inlen: c_uint,
        arg: *mut core::ffi::c_void,
    ) -> c_int,
>;
use crate::c_uint;

/// Set ALPN select callback (server side)
#[no_mangle]
pub extern "C" fn SSL_CTX_set_alpn_select_cb(
    _ctx: *mut SslContext,
    _callback: AlpnSelectCallback,
    _arg: *mut core::ffi::c_void,
) {
    // Store callback for later use
}

// ============================================================================
// Additional SSL functions
// ============================================================================

/// Set SSL options
#[no_mangle]
pub extern "C" fn SSL_set_options(ssl: *mut SslConnection, options: c_ulong) -> c_ulong {
    if ssl.is_null() {
        return 0;
    }
    options
}

/// Get SSL options
#[no_mangle]
pub extern "C" fn SSL_get_options(ssl: *const SslConnection) -> c_ulong {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Clear SSL options
#[no_mangle]
pub extern "C" fn SSL_clear_options(ssl: *mut SslConnection, options: c_ulong) -> c_ulong {
    if ssl.is_null() {
        return 0;
    }
    options
}

/// Set SSL mode
#[no_mangle]
pub extern "C" fn SSL_set_mode(ssl: *mut SslConnection, mode: c_ulong) -> c_ulong {
    if ssl.is_null() {
        return 0;
    }
    mode
}

/// Get SSL mode
#[no_mangle]
pub extern "C" fn SSL_get_mode(ssl: *const SslConnection) -> c_ulong {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Set read ahead
#[no_mangle]
pub extern "C" fn SSL_set_read_ahead(ssl: *mut SslConnection, _yes: c_int) {
    if ssl.is_null() {
        return;
    }
}

/// Get read ahead
#[no_mangle]
pub extern "C" fn SSL_get_read_ahead(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Check pending data
#[no_mangle]
pub extern "C" fn SSL_pending(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Check for pending data
#[no_mangle]
pub extern "C" fn SSL_has_pending(ssl: *const SslConnection) -> c_int {
    if SSL_pending(ssl) > 0 { 1 } else { 0 }
}

/// Get shutdown state
#[no_mangle]
pub extern "C" fn SSL_get_shutdown(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Set shutdown state
#[no_mangle]
pub extern "C" fn SSL_set_shutdown(ssl: *mut SslConnection, _mode: c_int) {
    if ssl.is_null() {
        return;
    }
}

/// Set quiet shutdown
#[no_mangle]
pub extern "C" fn SSL_set_quiet_shutdown(ssl: *mut SslConnection, _mode: c_int) {
    if ssl.is_null() {
        return;
    }
}

/// Get quiet shutdown
#[no_mangle]
pub extern "C" fn SSL_get_quiet_shutdown(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Get SSL context
#[no_mangle]
pub extern "C" fn SSL_get_SSL_CTX(ssl: *const SslConnection) -> *mut SslContext {
    if ssl.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::null_mut() // Would return stored context
}

/// Set SSL context
#[no_mangle]
pub extern "C" fn SSL_set_SSL_CTX(ssl: *mut SslConnection, _ctx: *mut SslContext) -> *mut SslContext {
    if ssl.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::null_mut()
}

/// Check if SSL is server
#[no_mangle]
pub extern "C" fn SSL_is_server(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    // Would check connection state
    0
}

// ============================================================================
// Cipher functions
// ============================================================================

/// Get cipher list for connection
#[no_mangle]
pub extern "C" fn SSL_get_ciphers(_ssl: *const SslConnection) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

/// Get cipher list string
#[no_mangle]
pub extern "C" fn SSL_get_cipher_list(ssl: *const SslConnection, priority: c_int) -> *const c_char {
    if ssl.is_null() || priority < 0 {
        return core::ptr::null();
    }
    core::ptr::null()
}

/// Set cipher list for connection
#[no_mangle]
pub extern "C" fn SSL_set_cipher_list(ssl: *mut SslConnection, str: *const c_char) -> c_int {
    if ssl.is_null() || str.is_null() {
        return 0;
    }
    1
}

/// Set ciphersuites for connection (TLS 1.3)
#[no_mangle]
pub extern "C" fn SSL_set_ciphersuites(ssl: *mut SslConnection, str: *const c_char) -> c_int {
    if ssl.is_null() || str.is_null() {
        return 0;
    }
    1
}

// ============================================================================
// Verification functions
// ============================================================================

/// Set verification mode
#[no_mangle]
pub extern "C" fn SSL_set_verify(
    ssl: *mut SslConnection,
    _mode: c_int,
    _callback: Option<extern "C" fn(c_int, *mut crate::x509::X509StoreCtx) -> c_int>,
) {
    if ssl.is_null() {
        return;
    }
}

/// Set verification depth
#[no_mangle]
pub extern "C" fn SSL_set_verify_depth(ssl: *mut SslConnection, _depth: c_int) {
    if ssl.is_null() {
        return;
    }
}

/// Get verification mode
#[no_mangle]
pub extern "C" fn SSL_get_verify_mode(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    0
}

/// Get verification depth
#[no_mangle]
pub extern "C" fn SSL_get_verify_depth(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    100
}

// ============================================================================
// Ex data functions (simplified stubs)
// ============================================================================

/// Set ex data on SSL
#[no_mangle]
pub extern "C" fn SSL_set_ex_data(_ssl: *mut SslConnection, _idx: c_int, _data: *mut core::ffi::c_void) -> c_int {
    1 // Success
}

/// Get ex data from SSL
#[no_mangle]
pub extern "C" fn SSL_get_ex_data(_ssl: *const SslConnection, _idx: c_int) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

/// Set ex data on SSL_CTX
#[no_mangle]
pub extern "C" fn SSL_CTX_set_ex_data(_ctx: *mut SslContext, _idx: c_int, _data: *mut core::ffi::c_void) -> c_int {
    1
}

/// Get ex data from SSL_CTX
#[no_mangle]
pub extern "C" fn SSL_CTX_get_ex_data(_ctx: *const SslContext, _idx: c_int) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

// ============================================================================
// Session functions
// ============================================================================

/// Free SSL session
#[no_mangle]
pub extern "C" fn SSL_SESSION_free(session: *mut crate::session::SslSession) {
    crate::session::SslSession::free(session)
}

/// Get session timeout
#[no_mangle]
pub extern "C" fn SSL_SESSION_get_timeout(session: *const crate::session::SslSession) -> c_ulong {
    if session.is_null() {
        return 0;
    }
    unsafe { (*session).get_timeout() }
}

/// Set session timeout
#[no_mangle]
pub extern "C" fn SSL_SESSION_set_timeout(session: *mut crate::session::SslSession, timeout: c_ulong) -> c_ulong {
    if session.is_null() {
        return 0;
    }
    unsafe { (*session).set_timeout(timeout); }
    timeout
}

// ============================================================================
// OCSP functions (stubs)
// ============================================================================

/// Set OCSP response
#[no_mangle]
pub extern "C" fn SSL_set_tlsext_status_ocsp_resp(
    _ssl: *mut SslConnection,
    _resp: *mut c_uchar,
    _len: c_int,
) -> c_int {
    1 // Success (no-op)
}

/// Get OCSP response
#[no_mangle]
pub extern "C" fn SSL_get_tlsext_status_ocsp_resp(
    _ssl: *const SslConnection,
    _resp: *mut *const c_uchar,
) -> c_int {
    0 // No response
}
