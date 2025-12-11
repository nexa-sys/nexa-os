//! nghttp3-compatible FFI bindings for dynamic linking to nh3
//!
//! This module provides FFI declarations for dynamically linking against
//! libnghttp3.so (nh3) at runtime. Instead of statically linking nh3, nurl can
//! load the library dynamically via the nghttp3-compatible C ABI.
//!
//! # Usage
//! ```rust
//! use nurl::nh3_ffi::*;
//!
//! // Get version info
//! let info = unsafe { nghttp3_version(0) };
//!
//! // Create connection callbacks
//! let mut callbacks: *mut nghttp3_callbacks = std::ptr::null_mut();
//! unsafe { nghttp3_callbacks_new(&mut callbacks) };
//!
//! // Create settings
//! let mut settings: nghttp3_settings = unsafe { std::mem::zeroed() };
//! unsafe { nghttp3_settings_default(&mut settings) };
//!
//! // Create client connection
//! let mut conn: *mut nghttp3_conn = std::ptr::null_mut();
//! unsafe { nghttp3_conn_client_new(&mut conn, callbacks, &settings, std::ptr::null(), std::ptr::null_mut()) };
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_void};

// ============================================================================
// Type Definitions
// ============================================================================

/// Size type
pub type size_t = usize;

/// Signed size type
pub type ssize_t = isize;

/// Stream ID type (62-bit QUIC stream ID)
pub type nghttp3_stream_id = i64;

// ============================================================================
// Opaque Types
// ============================================================================

/// Opaque nghttp3_conn type
#[repr(C)]
pub struct nghttp3_conn {
    _private: [u8; 0],
}

/// Opaque nghttp3_callbacks type
#[repr(C)]
pub struct nghttp3_callbacks {
    _private: [u8; 0],
}

// ============================================================================
// Data Structures
// ============================================================================

/// Version info structure
#[repr(C)]
pub struct nghttp3_info {
    /// Age of this struct
    pub age: c_int,
    /// Library version number
    pub version_num: c_int,
    /// Library version string
    pub version_str: *const c_char,
}

/// Name-value pair for headers
#[repr(C)]
#[derive(Debug, Clone)]
pub struct nghttp3_nv {
    /// Header name
    pub name: *const u8,
    /// Header value
    pub value: *const u8,
    /// Length of name
    pub namelen: size_t,
    /// Length of value
    pub valuelen: size_t,
    /// Flags (NGHTTP3_NV_FLAG_*)
    pub flags: u8,
}

impl nghttp3_nv {
    /// Create a new name-value pair from byte slices
    pub fn new(name: &[u8], value: &[u8]) -> Self {
        Self {
            name: name.as_ptr(),
            value: value.as_ptr(),
            namelen: name.len(),
            valuelen: value.len(),
            flags: NGHTTP3_NV_FLAG_NONE,
        }
    }

    /// Create from static strings
    pub fn from_static(name: &'static str, value: &'static str) -> Self {
        Self {
            name: name.as_ptr(),
            value: value.as_ptr(),
            namelen: name.len(),
            valuelen: value.len(),
            flags: NGHTTP3_NV_FLAG_NO_COPY_NAME | NGHTTP3_NV_FLAG_NO_COPY_VALUE,
        }
    }
}

/// Vector for writev operations
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct nghttp3_vec {
    /// Base pointer
    pub base: *mut u8,
    /// Length
    pub len: size_t,
}

impl Default for nghttp3_vec {
    fn default() -> Self {
        Self {
            base: std::ptr::null_mut(),
            len: 0,
        }
    }
}

/// HTTP/3 priority specification (RFC 9218)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct nghttp3_pri {
    /// Urgency (0-7, lower is more urgent)
    pub urgency: u8,
    /// Incremental flag
    pub inc: u8,
}

impl Default for nghttp3_pri {
    fn default() -> Self {
        Self {
            urgency: 3, // Default urgency
            inc: 0,
        }
    }
}

/// HTTP/3 settings
#[repr(C)]
#[derive(Debug, Clone)]
pub struct nghttp3_settings {
    /// Maximum field section size
    pub max_field_section_size: u64,
    /// QPACK maximum table capacity
    pub qpack_max_dtable_capacity: u64,
    /// QPACK blocked streams
    pub qpack_blocked_streams: u64,
    /// Enable CONNECT protocol (RFC 9220)
    pub enable_connect_protocol: u8,
    /// Enable H3 datagrams
    pub h3_datagram: u8,
}

impl Default for nghttp3_settings {
    fn default() -> Self {
        Self {
            max_field_section_size: 0, // 0 means use library default
            qpack_max_dtable_capacity: 0,
            qpack_blocked_streams: 0,
            enable_connect_protocol: 0,
            h3_datagram: 0,
        }
    }
}

/// Reference-counted buffer
#[repr(C)]
pub struct nghttp3_rcbuf {
    /// Base pointer
    pub base: *const u8,
    /// Length
    pub len: size_t,
}

/// Data provider for request/response body
#[repr(C)]
#[derive(Clone)]
pub struct nghttp3_data_reader {
    /// Read callback
    pub read_data: Option<
        extern "C" fn(
            conn: *mut c_void,
            stream_id: nghttp3_stream_id,
            buf: *mut u8,
            buflen: size_t,
            pflags: *mut u32,
            user_data: *mut c_void,
        ) -> isize,
    >,
}

impl Default for nghttp3_data_reader {
    fn default() -> Self {
        Self { read_data: None }
    }
}

// ============================================================================
// NV Flags
// ============================================================================

/// No special flags
pub const NGHTTP3_NV_FLAG_NONE: u8 = 0x00;
/// Never index this header
pub const NGHTTP3_NV_FLAG_NEVER_INDEX: u8 = 0x01;
/// No copy for name
pub const NGHTTP3_NV_FLAG_NO_COPY_NAME: u8 = 0x02;
/// No copy for value
pub const NGHTTP3_NV_FLAG_NO_COPY_VALUE: u8 = 0x04;

// ============================================================================
// Data Provider Flags
// ============================================================================

/// End of data flag
pub const NGHTTP3_DATA_FLAG_EOF: u32 = 0x01;
/// No end stream flag
pub const NGHTTP3_DATA_FLAG_NO_END_STREAM: u32 = 0x02;

// ============================================================================
// Error Codes
// ============================================================================

/// No error
pub const NGHTTP3_ERR_OK: c_int = 0;
/// Invalid argument
pub const NGHTTP3_ERR_INVALID_ARGUMENT: c_int = -101;
/// No buffer space
pub const NGHTTP3_ERR_NOBUF: c_int = -102;
/// Invalid state
pub const NGHTTP3_ERR_INVALID_STATE: c_int = -103;
/// Would block
pub const NGHTTP3_ERR_WOULDBLOCK: c_int = -104;
/// Stream in use
pub const NGHTTP3_ERR_STREAM_IN_USE: c_int = -105;
/// Push ID blocked
pub const NGHTTP3_ERR_PUSH_ID_BLOCKED: c_int = -106;
/// Malformed HTTP header
pub const NGHTTP3_ERR_MALFORMED_HTTP_HEADER: c_int = -107;
/// Remove HTTP header
pub const NGHTTP3_ERR_REMOVE_HTTP_HEADER: c_int = -108;
/// Malformed HTTP messaging
pub const NGHTTP3_ERR_MALFORMED_HTTP_MESSAGING: c_int = -109;
/// QPACK fatal error
pub const NGHTTP3_ERR_QPACK_FATAL: c_int = -110;
/// QPACK header too large
pub const NGHTTP3_ERR_QPACK_HEADER_TOO_LARGE: c_int = -111;
/// Ignore stream
pub const NGHTTP3_ERR_IGNORE_STREAM: c_int = -112;
/// H3 frame unexpected
pub const NGHTTP3_ERR_H3_FRAME_UNEXPECTED: c_int = -113;
/// H3 frame error
pub const NGHTTP3_ERR_H3_FRAME_ERROR: c_int = -114;
/// H3 missing settings
pub const NGHTTP3_ERR_H3_MISSING_SETTINGS: c_int = -115;
/// H3 internal error
pub const NGHTTP3_ERR_H3_INTERNAL_ERROR: c_int = -116;
/// H3 closed critical stream
pub const NGHTTP3_ERR_H3_CLOSED_CRITICAL_STREAM: c_int = -117;
/// H3 general protocol error
pub const NGHTTP3_ERR_H3_GENERAL_PROTOCOL_ERROR: c_int = -118;
/// H3 ID error
pub const NGHTTP3_ERR_H3_ID_ERROR: c_int = -119;
/// H3 settings error
pub const NGHTTP3_ERR_H3_SETTINGS_ERROR: c_int = -120;
/// H3 stream creation error
pub const NGHTTP3_ERR_H3_STREAM_CREATION_ERROR: c_int = -121;
/// Fatal error
pub const NGHTTP3_ERR_FATAL: c_int = -501;
/// Out of memory
pub const NGHTTP3_ERR_NOMEM: c_int = -502;
/// Callback failure
pub const NGHTTP3_ERR_CALLBACK_FAILURE: c_int = -503;

// ============================================================================
// External Functions (linked from libnghttp3.so)
// ============================================================================

#[link(name = "nghttp3")]
extern "C" {
    // ========================================================================
    // Version and Info Functions
    // ========================================================================

    /// Get library version info
    pub fn nghttp3_version(least_version: c_int) -> *const nghttp3_info;

    /// Convert error code to string
    pub fn nghttp3_strerror(error_code: c_int) -> *const c_char;

    /// Check if error is fatal
    pub fn nghttp3_err_is_fatal(error_code: c_int) -> c_int;

    // ========================================================================
    // Callback Functions
    // ========================================================================

    /// Create new callbacks structure
    pub fn nghttp3_callbacks_new(pcallbacks: *mut *mut nghttp3_callbacks) -> c_int;

    /// Delete callbacks structure
    pub fn nghttp3_callbacks_del(callbacks: *mut nghttp3_callbacks);

    /// Set recv_data callback
    pub fn nghttp3_callbacks_set_recv_data(
        callbacks: *mut nghttp3_callbacks,
        cb: extern "C" fn(
            conn: *mut nghttp3_conn,
            stream_id: nghttp3_stream_id,
            data: *const u8,
            datalen: size_t,
            user_data: *mut c_void,
        ) -> c_int,
    );

    /// Set begin_headers callback
    pub fn nghttp3_callbacks_set_begin_headers(
        callbacks: *mut nghttp3_callbacks,
        cb: extern "C" fn(
            conn: *mut nghttp3_conn,
            stream_id: nghttp3_stream_id,
            user_data: *mut c_void,
        ) -> c_int,
    );

    /// Set recv_header callback
    pub fn nghttp3_callbacks_set_recv_header(
        callbacks: *mut nghttp3_callbacks,
        cb: extern "C" fn(
            conn: *mut nghttp3_conn,
            stream_id: nghttp3_stream_id,
            token: c_int,
            name: *const nghttp3_rcbuf,
            value: *const nghttp3_rcbuf,
            flags: u8,
            user_data: *mut c_void,
        ) -> c_int,
    );

    /// Set end_headers callback
    pub fn nghttp3_callbacks_set_end_headers(
        callbacks: *mut nghttp3_callbacks,
        cb: extern "C" fn(
            conn: *mut nghttp3_conn,
            stream_id: nghttp3_stream_id,
            fin: c_int,
            user_data: *mut c_void,
        ) -> c_int,
    );

    /// Set end_stream callback
    pub fn nghttp3_callbacks_set_end_stream(
        callbacks: *mut nghttp3_callbacks,
        cb: extern "C" fn(
            conn: *mut nghttp3_conn,
            stream_id: nghttp3_stream_id,
            user_data: *mut c_void,
        ) -> c_int,
    );

    /// Set stream_close callback
    pub fn nghttp3_callbacks_set_stream_close(
        callbacks: *mut nghttp3_callbacks,
        cb: extern "C" fn(
            conn: *mut nghttp3_conn,
            stream_id: nghttp3_stream_id,
            app_error_code: u64,
            user_data: *mut c_void,
        ) -> c_int,
    );

    // ========================================================================
    // Settings Functions
    // ========================================================================

    /// Initialize settings with default values
    pub fn nghttp3_settings_default(settings: *mut nghttp3_settings);

    // ========================================================================
    // Connection Functions
    // ========================================================================

    /// Create a new client connection
    pub fn nghttp3_conn_client_new(
        pconn: *mut *mut nghttp3_conn,
        callbacks: *const nghttp3_callbacks,
        settings: *const nghttp3_settings,
        mem: *const c_void,
        user_data: *mut c_void,
    ) -> c_int;

    /// Create a new server connection
    pub fn nghttp3_conn_server_new(
        pconn: *mut *mut nghttp3_conn,
        callbacks: *const nghttp3_callbacks,
        settings: *const nghttp3_settings,
        mem: *const c_void,
        user_data: *mut c_void,
    ) -> c_int;

    /// Delete a connection
    pub fn nghttp3_conn_del(conn: *mut nghttp3_conn);

    /// Bind control stream
    pub fn nghttp3_conn_bind_control_stream(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
    ) -> c_int;

    /// Bind QPACK streams
    pub fn nghttp3_conn_bind_qpack_streams(
        conn: *mut nghttp3_conn,
        qenc_stream_id: nghttp3_stream_id,
        qdec_stream_id: nghttp3_stream_id,
    ) -> c_int;

    /// Read stream data for sending
    pub fn nghttp3_conn_writev_stream(
        conn: *mut nghttp3_conn,
        pstream_id: *mut nghttp3_stream_id,
        pfin: *mut c_int,
        vec: *mut nghttp3_vec,
        veccnt: size_t,
    ) -> ssize_t;

    /// Acknowledge sent data
    pub fn nghttp3_conn_add_write_offset(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
        n: size_t,
    ) -> c_int;

    /// Process received data on a stream
    pub fn nghttp3_conn_read_stream(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
        data: *const u8,
        datalen: size_t,
        fin: c_int,
    ) -> ssize_t;

    /// Submit a request
    pub fn nghttp3_conn_submit_request(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
        nva: *const nghttp3_nv,
        nvlen: size_t,
        dr: *const nghttp3_data_reader,
        stream_user_data: *mut c_void,
    ) -> c_int;

    /// Submit response
    pub fn nghttp3_conn_submit_response(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
        nva: *const nghttp3_nv,
        nvlen: size_t,
        dr: *const nghttp3_data_reader,
    ) -> c_int;

    /// Shutdown connection
    pub fn nghttp3_conn_shutdown(conn: *mut nghttp3_conn) -> c_int;

    /// Close a stream
    pub fn nghttp3_conn_close_stream(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
        app_error_code: u64,
    ) -> c_int;

    /// Check if connection is client
    pub fn nghttp3_conn_is_client(conn: *const nghttp3_conn) -> c_int;

    // ========================================================================
    // RCBuf Functions
    // ========================================================================

    /// Get rcbuf data pointer
    pub fn nghttp3_rcbuf_get_buf(rcbuf: *const nghttp3_rcbuf) -> *const u8;

    /// Get rcbuf length
    pub fn nghttp3_rcbuf_get_len(rcbuf: *const nghttp3_rcbuf) -> size_t;

    // ========================================================================
    // Priority Functions
    // ========================================================================

    /// Set priority defaults
    pub fn nghttp3_pri_default(pri: *mut nghttp3_pri);

    // ========================================================================
    // Stream Response Data Functions (nh3 extension)
    // ========================================================================

    /// Get stream response data (headers and body)
    /// Returns a pointer to nghttp3_stream_response_data or null if stream not found
    /// Caller must free the returned data using nghttp3_stream_response_data_free
    pub fn nghttp3_conn_get_stream_response_data(
        conn: *mut nghttp3_conn,
        stream_id: nghttp3_stream_id,
    ) -> *mut nghttp3_stream_response_data;

    /// Free stream response data allocated by nghttp3_conn_get_stream_response_data
    pub fn nghttp3_stream_response_data_free(data: *mut nghttp3_stream_response_data);
}

// ============================================================================
// Stream Response Data Structures (nh3 extension)
// ============================================================================

/// Header field for response data
#[repr(C)]
pub struct nghttp3_header_field {
    /// Header name pointer
    pub name: *mut u8,
    /// Header name length
    pub name_len: size_t,
    /// Header value pointer
    pub value: *mut u8,
    /// Header value length
    pub value_len: size_t,
}

/// Stream response data structure
#[repr(C)]
pub struct nghttp3_stream_response_data {
    /// Pointer to response headers array
    pub headers: *mut nghttp3_header_field,
    /// Number of response headers
    pub headers_len: size_t,
    /// Pointer to response body data
    pub body: *mut u8,
    /// Length of response body
    pub body_len: size_t,
    /// HTTP status code (0 if not found)
    pub status_code: u16,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Helper to check if an operation succeeded
#[inline]
pub fn nghttp3_is_ok(rv: c_int) -> bool {
    rv >= 0
}

/// Helper to check if would block
#[inline]
pub fn nghttp3_is_would_block(rv: c_int) -> bool {
    rv == NGHTTP3_ERR_WOULDBLOCK
}

/// Helper to check if error is fatal
#[inline]
pub fn nghttp3_is_fatal(rv: c_int) -> bool {
    unsafe { nghttp3_err_is_fatal(rv) != 0 }
}

/// Get version string safely
pub fn get_version_string() -> Option<&'static str> {
    let info = unsafe { nghttp3_version(0) };
    if info.is_null() {
        return None;
    }
    let version_str = unsafe { (*info).version_str };
    if version_str.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(version_str).to_str().ok() }
}

/// Get error message safely
pub fn get_error_string(error_code: c_int) -> String {
    let ptr = unsafe { nghttp3_strerror(error_code) };
    if ptr.is_null() {
        return format!("Unknown error ({})", error_code);
    }
    unsafe {
        std::ffi::CStr::from_ptr(ptr)
            .to_string_lossy()
            .into_owned()
    }
}
