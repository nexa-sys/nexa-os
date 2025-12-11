//! nghttp2-compatible FFI bindings for dynamic linking to nh2
//!
//! This module provides FFI declarations for dynamically linking against
//! libnh2.so at runtime. Instead of statically linking nh2, nurl can
//! load the library dynamically via the nghttp2-compatible C ABI.
//!
//! # Usage
//! ```rust
//! use nurl::nh2_ffi::*;
//!
//! // Get version info
//! let info = unsafe { nghttp2_version(0) };
//!
//! // Create session callbacks
//! let mut callbacks: *mut nghttp2_session_callbacks = std::ptr::null_mut();
//! unsafe { nghttp2_session_callbacks_new(&mut callbacks) };
//!
//! // Create client session
//! let mut session: *mut nghttp2_session = std::ptr::null_mut();
//! unsafe { nghttp2_session_client_new(&mut session, callbacks, std::ptr::null_mut()) };
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_uchar, c_void};

// ============================================================================
// Type Definitions
// ============================================================================

/// Size type
pub type size_t = usize;

/// Signed size type
pub type ssize_t = isize;

/// Stream ID type
pub type nghttp2_stream_id = i32;

// ============================================================================
// Opaque Types
// ============================================================================

/// Opaque nghttp2_session type
#[repr(C)]
pub struct nghttp2_session {
    _private: [u8; 0],
}

/// Opaque nghttp2_session_callbacks type
#[repr(C)]
pub struct nghttp2_session_callbacks {
    _private: [u8; 0],
}

/// Opaque nghttp2_option type
#[repr(C)]
pub struct nghttp2_option {
    _private: [u8; 0],
}

/// Opaque nghttp2_frame type
#[repr(C)]
pub struct nghttp2_frame {
    _private: [u8; 0],
}

// ============================================================================
// Data Structures
// ============================================================================

/// Version info structure
#[repr(C)]
pub struct nghttp2_info {
    /// Age of this struct
    pub age: c_int,
    /// Library version number
    pub version_num: c_int,
    /// Library version string
    pub version_str: *const c_char,
    /// Protocol version string
    pub proto_str: *const c_char,
}

/// Name-value pair for headers
#[repr(C)]
#[derive(Debug, Clone)]
pub struct nghttp2_nv {
    /// Header name
    pub name: *const u8,
    /// Header value
    pub value: *const u8,
    /// Length of name
    pub namelen: size_t,
    /// Length of value
    pub valuelen: size_t,
    /// Flags (NGHTTP2_NV_FLAG_*)
    pub flags: u8,
}

impl nghttp2_nv {
    /// Create a new name-value pair from byte slices
    pub fn new(name: &[u8], value: &[u8]) -> Self {
        Self {
            name: name.as_ptr(),
            value: value.as_ptr(),
            namelen: name.len(),
            valuelen: value.len(),
            flags: NGHTTP2_NV_FLAG_NONE,
        }
    }

    /// Create from static strings
    pub fn from_static(name: &'static str, value: &'static str) -> Self {
        Self {
            name: name.as_ptr(),
            value: value.as_ptr(),
            namelen: name.len(),
            valuelen: value.len(),
            flags: NGHTTP2_NV_FLAG_NO_COPY_NAME | NGHTTP2_NV_FLAG_NO_COPY_VALUE,
        }
    }
}

/// Priority specification
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct nghttp2_priority_spec {
    /// Stream ID of the dependency
    pub stream_id: nghttp2_stream_id,
    /// Weight (1-256)
    pub weight: i32,
    /// Exclusive flag
    pub exclusive: u8,
}

impl Default for nghttp2_priority_spec {
    fn default() -> Self {
        Self {
            stream_id: 0,
            weight: 16,
            exclusive: 0,
        }
    }
}

/// Settings entry
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct nghttp2_settings_entry {
    /// Settings ID
    pub settings_id: c_int,
    /// Settings value
    pub value: u32,
}

/// Data source for data provider
#[repr(C)]
pub union nghttp2_data_source {
    /// File descriptor
    pub fd: c_int,
    /// Arbitrary pointer
    pub ptr: *mut c_void,
}

/// Data source read callback
pub type nghttp2_data_source_read_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
        buf: *mut u8,
        length: size_t,
        data_flags: *mut u32,
        source: *mut nghttp2_data_source,
        user_data: *mut c_void,
    ) -> ssize_t,
>;

/// Data provider structure
#[repr(C)]
pub struct nghttp2_data_provider {
    /// Data source
    pub source: nghttp2_data_source,
    /// Read callback
    pub read_callback: nghttp2_data_source_read_callback,
}

// ============================================================================
// Constants - NV Flags
// ============================================================================

/// No flags
pub const NGHTTP2_NV_FLAG_NONE: u8 = 0x00;
/// Do not index this header
pub const NGHTTP2_NV_FLAG_NO_INDEX: u8 = 0x01;
/// Never index this header
pub const NGHTTP2_NV_FLAG_NO_COPY_NAME: u8 = 0x04;
/// Do not copy value
pub const NGHTTP2_NV_FLAG_NO_COPY_VALUE: u8 = 0x08;

// ============================================================================
// Constants - Data Flags
// ============================================================================

/// No flags
pub const NGHTTP2_DATA_FLAG_NONE: u32 = 0x00;
/// End of file
pub const NGHTTP2_DATA_FLAG_EOF: u32 = 0x01;
/// Do not send END_STREAM
pub const NGHTTP2_DATA_FLAG_NO_END_STREAM: u32 = 0x02;
/// Do not copy data
pub const NGHTTP2_DATA_FLAG_NO_COPY: u32 = 0x04;

// ============================================================================
// Constants - Settings IDs
// ============================================================================

/// Header table size
pub const NGHTTP2_SETTINGS_HEADER_TABLE_SIZE: c_int = 0x01;
/// Enable push
pub const NGHTTP2_SETTINGS_ENABLE_PUSH: c_int = 0x02;
/// Max concurrent streams
pub const NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS: c_int = 0x03;
/// Initial window size
pub const NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE: c_int = 0x04;
/// Max frame size
pub const NGHTTP2_SETTINGS_MAX_FRAME_SIZE: c_int = 0x05;
/// Max header list size
pub const NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE: c_int = 0x06;

// ============================================================================
// Constants - Error Codes
// ============================================================================

/// No error
pub const NGHTTP2_ERR_NO_ERROR: c_int = 0;
/// Invalid argument
pub const NGHTTP2_ERR_INVALID_ARGUMENT: c_int = -501;
/// Buffer error
pub const NGHTTP2_ERR_BUFFER_ERROR: c_int = -502;
/// Unsupported version
pub const NGHTTP2_ERR_UNSUPPORTED_VERSION: c_int = -503;
/// Would block
pub const NGHTTP2_ERR_WOULDBLOCK: c_int = -504;
/// Protocol error
pub const NGHTTP2_ERR_PROTO: c_int = -505;
/// Invalid frame
pub const NGHTTP2_ERR_INVALID_FRAME: c_int = -506;
/// EOF
pub const NGHTTP2_ERR_EOF: c_int = -507;
/// Deferred
pub const NGHTTP2_ERR_DEFERRED: c_int = -508;
/// Stream ID not available
pub const NGHTTP2_ERR_STREAM_ID_NOT_AVAILABLE: c_int = -509;
/// Stream closed
pub const NGHTTP2_ERR_STREAM_CLOSED: c_int = -510;
/// Stream closing
pub const NGHTTP2_ERR_STREAM_CLOSING: c_int = -511;
/// Stream shut write
pub const NGHTTP2_ERR_STREAM_SHUT_WR: c_int = -512;
/// Invalid stream ID
pub const NGHTTP2_ERR_INVALID_STREAM_ID: c_int = -513;
/// Invalid stream state
pub const NGHTTP2_ERR_INVALID_STREAM_STATE: c_int = -514;
/// Data exists
pub const NGHTTP2_ERR_DATA_EXIST: c_int = -515;
/// Push disabled
pub const NGHTTP2_ERR_PUSH_DISABLED: c_int = -516;
/// Too many inflight settings
pub const NGHTTP2_ERR_TOO_MANY_INFLIGHT_SETTINGS: c_int = -517;
/// Invalid header block
pub const NGHTTP2_ERR_INVALID_HEADER_BLOCK: c_int = -518;
/// Flow control
pub const NGHTTP2_ERR_FLOW_CONTROL: c_int = -519;
/// Header compression error
pub const NGHTTP2_ERR_HEADER_COMP: c_int = -520;
/// Settings expected
pub const NGHTTP2_ERR_SETTINGS_EXPECTED: c_int = -521;
/// Internal error
pub const NGHTTP2_ERR_INTERNAL: c_int = -522;
/// Cancel
pub const NGHTTP2_ERR_CANCEL: c_int = -523;
/// No memory
pub const NGHTTP2_ERR_NOMEM: c_int = -524;
/// Callback failure
pub const NGHTTP2_ERR_CALLBACK_FAILURE: c_int = -525;
/// Bad client magic
pub const NGHTTP2_ERR_BAD_CLIENT_MAGIC: c_int = -526;
/// Flooded
pub const NGHTTP2_ERR_FLOODED: c_int = -527;
/// HTTP header
pub const NGHTTP2_ERR_HTTP_HEADER: c_int = -528;
/// HTTP messaging
pub const NGHTTP2_ERR_HTTP_MESSAGING: c_int = -529;
/// Refused stream
pub const NGHTTP2_ERR_REFUSED_STREAM: c_int = -530;
/// Fatal
pub const NGHTTP2_ERR_FATAL: c_int = -900;

// ============================================================================
// Constants - HTTP/2 Error Codes (RFC 7540)
// ============================================================================

/// No error
pub const NGHTTP2_NO_ERROR: u32 = 0x00;
/// Protocol error
pub const NGHTTP2_PROTOCOL_ERROR: u32 = 0x01;
/// Internal error
pub const NGHTTP2_INTERNAL_ERROR: u32 = 0x02;
/// Flow control error
pub const NGHTTP2_FLOW_CONTROL_ERROR: u32 = 0x03;
/// Settings timeout
pub const NGHTTP2_SETTINGS_TIMEOUT: u32 = 0x04;
/// Stream closed
pub const NGHTTP2_STREAM_CLOSED: u32 = 0x05;
/// Frame size error
pub const NGHTTP2_FRAME_SIZE_ERROR: u32 = 0x06;
/// Refused stream
pub const NGHTTP2_REFUSED_STREAM: u32 = 0x07;
/// Cancel
pub const NGHTTP2_CANCEL: u32 = 0x08;
/// Compression error
pub const NGHTTP2_COMPRESSION_ERROR: u32 = 0x09;
/// Connect error
pub const NGHTTP2_CONNECT_ERROR: u32 = 0x0a;
/// Enhance your calm
pub const NGHTTP2_ENHANCE_YOUR_CALM: u32 = 0x0b;
/// Inadequate security
pub const NGHTTP2_INADEQUATE_SECURITY: u32 = 0x0c;
/// HTTP/1.1 required
pub const NGHTTP2_HTTP_1_1_REQUIRED: u32 = 0x0d;

// ============================================================================
// Constants - Stream States
// ============================================================================

/// Idle state
pub const NGHTTP2_STREAM_STATE_IDLE: c_int = 1;
/// Open state
pub const NGHTTP2_STREAM_STATE_OPEN: c_int = 2;
/// Reserved local state
pub const NGHTTP2_STREAM_STATE_RESERVED_LOCAL: c_int = 3;
/// Reserved remote state
pub const NGHTTP2_STREAM_STATE_RESERVED_REMOTE: c_int = 4;
/// Half closed local state
pub const NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL: c_int = 5;
/// Half closed remote state
pub const NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE: c_int = 6;
/// Closed state
pub const NGHTTP2_STREAM_STATE_CLOSED: c_int = 7;

// ============================================================================
// Callback Types
// ============================================================================

/// Send callback
pub type nghttp2_send_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        data: *const u8,
        length: size_t,
        flags: c_int,
        user_data: *mut c_void,
    ) -> ssize_t,
>;

/// Receive callback
pub type nghttp2_recv_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        buf: *mut u8,
        length: size_t,
        flags: c_int,
        user_data: *mut c_void,
    ) -> ssize_t,
>;

/// On frame receive callback
pub type nghttp2_on_frame_recv_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        frame: *const nghttp2_frame,
        user_data: *mut c_void,
    ) -> c_int,
>;

/// On stream close callback
pub type nghttp2_on_stream_close_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
        error_code: u32,
        user_data: *mut c_void,
    ) -> c_int,
>;

/// On data chunk receive callback
pub type nghttp2_on_data_chunk_recv_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        flags: u8,
        stream_id: nghttp2_stream_id,
        data: *const u8,
        len: size_t,
        user_data: *mut c_void,
    ) -> c_int,
>;

/// On header callback
pub type nghttp2_on_header_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        frame: *const nghttp2_frame,
        name: *const u8,
        namelen: size_t,
        value: *const u8,
        valuelen: size_t,
        flags: u8,
        user_data: *mut c_void,
    ) -> c_int,
>;

/// On begin headers callback
pub type nghttp2_on_begin_headers_callback = Option<
    extern "C" fn(
        session: *mut nghttp2_session,
        frame: *const nghttp2_frame,
        user_data: *mut c_void,
    ) -> c_int,
>;

// ============================================================================
// External Functions (linked dynamically from libnh2.so)
// ============================================================================

#[link(name = "nh2")]
extern "C" {
    // ========================================================================
    // Version and Info
    // ========================================================================

    /// Get library version info
    pub fn nghttp2_version(least_version: c_int) -> *const nghttp2_info;

    /// Check if error is fatal
    pub fn nghttp2_is_fatal(error_code: c_int) -> c_int;

    /// Get error string
    pub fn nghttp2_strerror(error_code: c_int) -> *const c_char;

    /// Get HTTP/2 error string
    pub fn nghttp2_http2_strerror(error_code: u32) -> *const c_char;

    // ========================================================================
    // Session Callbacks
    // ========================================================================

    /// Create new session callbacks
    pub fn nghttp2_session_callbacks_new(
        callbacks_ptr: *mut *mut nghttp2_session_callbacks,
    ) -> c_int;

    /// Delete session callbacks
    pub fn nghttp2_session_callbacks_del(callbacks: *mut nghttp2_session_callbacks);

    /// Set send callback
    pub fn nghttp2_session_callbacks_set_send_callback(
        callbacks: *mut nghttp2_session_callbacks,
        send_callback: nghttp2_send_callback,
    );

    /// Set receive callback
    pub fn nghttp2_session_callbacks_set_recv_callback(
        callbacks: *mut nghttp2_session_callbacks,
        recv_callback: nghttp2_recv_callback,
    );

    /// Set on frame receive callback
    pub fn nghttp2_session_callbacks_set_on_frame_recv_callback(
        callbacks: *mut nghttp2_session_callbacks,
        on_frame_recv_callback: nghttp2_on_frame_recv_callback,
    );

    /// Set on stream close callback
    pub fn nghttp2_session_callbacks_set_on_stream_close_callback(
        callbacks: *mut nghttp2_session_callbacks,
        on_stream_close_callback: nghttp2_on_stream_close_callback,
    );

    /// Set on data chunk receive callback
    pub fn nghttp2_session_callbacks_set_on_data_chunk_recv_callback(
        callbacks: *mut nghttp2_session_callbacks,
        on_data_chunk_recv_callback: nghttp2_on_data_chunk_recv_callback,
    );

    /// Set on header callback
    pub fn nghttp2_session_callbacks_set_on_header_callback(
        callbacks: *mut nghttp2_session_callbacks,
        on_header_callback: nghttp2_on_header_callback,
    );

    /// Set on begin headers callback
    pub fn nghttp2_session_callbacks_set_on_begin_headers_callback(
        callbacks: *mut nghttp2_session_callbacks,
        on_begin_headers_callback: nghttp2_on_begin_headers_callback,
    );

    // ========================================================================
    // Session Option
    // ========================================================================

    /// Create new option
    pub fn nghttp2_option_new(option_ptr: *mut *mut nghttp2_option) -> c_int;

    /// Delete option
    pub fn nghttp2_option_del(option: *mut nghttp2_option);

    /// Set no auto window update
    pub fn nghttp2_option_set_no_auto_window_update(option: *mut nghttp2_option, val: c_int);

    /// Set no recv client magic
    pub fn nghttp2_option_set_no_recv_client_magic(option: *mut nghttp2_option, val: c_int);

    /// Set no http messaging
    pub fn nghttp2_option_set_no_http_messaging(option: *mut nghttp2_option, val: c_int);

    /// Set max deflate dynamic table size
    pub fn nghttp2_option_set_max_deflate_dynamic_table_size(
        option: *mut nghttp2_option,
        val: size_t,
    );

    /// Set no auto ping ack
    pub fn nghttp2_option_set_no_auto_ping_ack(option: *mut nghttp2_option, val: c_int);

    /// Set max send header block length
    pub fn nghttp2_option_set_max_send_header_block_length(
        option: *mut nghttp2_option,
        val: size_t,
    );

    /// Set max settings
    pub fn nghttp2_option_set_max_settings(option: *mut nghttp2_option, val: size_t);

    // ========================================================================
    // Session Management
    // ========================================================================

    /// Create new client session
    pub fn nghttp2_session_client_new(
        session_ptr: *mut *mut nghttp2_session,
        callbacks: *const nghttp2_session_callbacks,
        user_data: *mut c_void,
    ) -> c_int;

    /// Create new client session with options
    pub fn nghttp2_session_client_new2(
        session_ptr: *mut *mut nghttp2_session,
        callbacks: *const nghttp2_session_callbacks,
        user_data: *mut c_void,
        option: *const nghttp2_option,
    ) -> c_int;

    /// Create new server session
    pub fn nghttp2_session_server_new(
        session_ptr: *mut *mut nghttp2_session,
        callbacks: *const nghttp2_session_callbacks,
        user_data: *mut c_void,
    ) -> c_int;

    /// Create new server session with options
    pub fn nghttp2_session_server_new2(
        session_ptr: *mut *mut nghttp2_session,
        callbacks: *const nghttp2_session_callbacks,
        user_data: *mut c_void,
        option: *const nghttp2_option,
    ) -> c_int;

    /// Delete session
    pub fn nghttp2_session_del(session: *mut nghttp2_session);

    // ========================================================================
    // Session Operations
    // ========================================================================

    /// Send data
    pub fn nghttp2_session_send(session: *mut nghttp2_session) -> c_int;

    /// Receive data
    pub fn nghttp2_session_recv(session: *mut nghttp2_session) -> c_int;

    /// Send data to memory buffer
    pub fn nghttp2_session_mem_send(
        session: *mut nghttp2_session,
        data_ptr: *mut *const u8,
    ) -> ssize_t;

    /// Receive data from memory buffer
    pub fn nghttp2_session_mem_recv(
        session: *mut nghttp2_session,
        data: *const u8,
        datalen: size_t,
    ) -> ssize_t;

    /// Check if session wants to read
    pub fn nghttp2_session_want_read(session: *mut nghttp2_session) -> c_int;

    /// Check if session wants to write
    pub fn nghttp2_session_want_write(session: *mut nghttp2_session) -> c_int;

    /// Terminate session
    pub fn nghttp2_session_terminate_session(session: *mut nghttp2_session, error_code: u32)
        -> c_int;

    // ========================================================================
    // Submit Functions
    // ========================================================================

    /// Submit request
    pub fn nghttp2_submit_request(
        session: *mut nghttp2_session,
        pri_spec: *const nghttp2_priority_spec,
        nva: *const nghttp2_nv,
        nvlen: size_t,
        data_prd: *const nghttp2_data_provider,
        stream_user_data: *mut c_void,
    ) -> nghttp2_stream_id;

    /// Submit response
    pub fn nghttp2_submit_response(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
        nva: *const nghttp2_nv,
        nvlen: size_t,
        data_prd: *const nghttp2_data_provider,
    ) -> c_int;

    /// Submit settings
    pub fn nghttp2_submit_settings(
        session: *mut nghttp2_session,
        flags: u8,
        iv: *const nghttp2_settings_entry,
        niv: size_t,
    ) -> c_int;

    /// Submit ping
    pub fn nghttp2_submit_ping(
        session: *mut nghttp2_session,
        flags: u8,
        opaque_data: *const u8,
    ) -> c_int;

    /// Submit goaway
    pub fn nghttp2_submit_goaway(
        session: *mut nghttp2_session,
        flags: u8,
        last_stream_id: nghttp2_stream_id,
        error_code: u32,
        opaque_data: *const u8,
        opaque_data_len: size_t,
    ) -> c_int;

    /// Submit rst_stream
    pub fn nghttp2_submit_rst_stream(
        session: *mut nghttp2_session,
        flags: u8,
        stream_id: nghttp2_stream_id,
        error_code: u32,
    ) -> c_int;

    /// Submit window update
    pub fn nghttp2_submit_window_update(
        session: *mut nghttp2_session,
        flags: u8,
        stream_id: nghttp2_stream_id,
        window_size_increment: i32,
    ) -> c_int;

    /// Submit priority
    pub fn nghttp2_submit_priority(
        session: *mut nghttp2_session,
        flags: u8,
        stream_id: nghttp2_stream_id,
        pri_spec: *const nghttp2_priority_spec,
    ) -> c_int;

    // ========================================================================
    // Stream Information
    // ========================================================================

    /// Get stream state
    pub fn nghttp2_session_get_stream_state(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
    ) -> c_int;

    /// Get stream user data
    pub fn nghttp2_session_get_stream_user_data(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
    ) -> *mut c_void;

    /// Set stream user data
    pub fn nghttp2_session_set_stream_user_data(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
        stream_user_data: *mut c_void,
    ) -> c_int;

    /// Get next stream ID
    pub fn nghttp2_session_get_next_stream_id(session: *mut nghttp2_session) -> u32;

    /// Get last processed stream ID
    pub fn nghttp2_session_get_last_proc_stream_id(session: *mut nghttp2_session)
        -> nghttp2_stream_id;

    // ========================================================================
    // Settings and Flow Control
    // ========================================================================

    /// Get remote settings
    pub fn nghttp2_session_get_remote_settings(session: *mut nghttp2_session, id: c_int) -> u32;

    /// Get local settings
    pub fn nghttp2_session_get_local_settings(session: *mut nghttp2_session, id: c_int) -> u32;

    /// Get effective local window size
    pub fn nghttp2_session_get_effective_local_window_size(session: *mut nghttp2_session) -> i32;

    /// Get remote window size
    pub fn nghttp2_session_get_remote_window_size(session: *mut nghttp2_session) -> i32;

    /// Get stream effective local window size
    pub fn nghttp2_session_get_stream_effective_local_window_size(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
    ) -> i32;

    /// Get stream remote window size
    pub fn nghttp2_session_get_stream_remote_window_size(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
    ) -> i32;

    /// Get stream local window size
    pub fn nghttp2_session_get_stream_local_window_size(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
    ) -> i32;

    /// Get local window size
    pub fn nghttp2_session_get_local_window_size(session: *mut nghttp2_session) -> i32;

    /// Consume received data
    pub fn nghttp2_session_consume(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
        size: size_t,
    ) -> c_int;

    /// Consume connection data
    pub fn nghttp2_session_consume_connection(
        session: *mut nghttp2_session,
        size: size_t,
    ) -> c_int;

    /// Consume stream data
    pub fn nghttp2_session_consume_stream(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
        size: size_t,
    ) -> c_int;

    // ========================================================================
    // HPACK Functions
    // ========================================================================

    /// Create HD inflater
    pub fn nghttp2_hd_inflate_new(inflater_ptr: *mut *mut c_void) -> c_int;

    /// Delete HD inflater
    pub fn nghttp2_hd_inflate_del(inflater: *mut c_void);

    /// Create HD deflater
    pub fn nghttp2_hd_deflate_new(
        deflater_ptr: *mut *mut c_void,
        max_deflate_dynamic_table_size: size_t,
    ) -> c_int;

    /// Delete HD deflater
    pub fn nghttp2_hd_deflate_del(deflater: *mut c_void);

    /// Get HPACK encoder dynamic table size
    pub fn nghttp2_session_get_hd_deflate_dynamic_table_size(
        session: *mut nghttp2_session,
    ) -> size_t;

    /// Get HPACK decoder dynamic table size
    pub fn nghttp2_session_get_hd_inflate_dynamic_table_size(
        session: *mut nghttp2_session,
    ) -> size_t;

    // ========================================================================
    // Validation Functions
    // ========================================================================

    /// Check header name validity
    pub fn nghttp2_check_header_name(name: *const u8, len: size_t) -> c_int;

    /// Check header value validity
    pub fn nghttp2_check_header_value(value: *const u8, len: size_t) -> c_int;

    /// Select next protocol (ALPN)
    pub fn nghttp2_select_next_protocol(
        out: *mut *const u8,
        outlen: *mut u8,
        protocol_list: *const u8,
        inlen: size_t,
    ) -> c_int;

    // ========================================================================
    // Outbound Queue
    // ========================================================================

    /// Get outbound queue size
    pub fn nghttp2_session_get_outbound_queue_size(session: *mut nghttp2_session) -> size_t;

    // ========================================================================
    // Upgrade Functions (h2c)
    // ========================================================================

    /// Upgrade session
    pub fn nghttp2_session_upgrade(
        session: *mut nghttp2_session,
        settings_payload: *const u8,
        settings_payloadlen: size_t,
        head_request: c_int,
        stream_user_data: *mut c_void,
    ) -> c_int;

    /// Upgrade session version 2
    pub fn nghttp2_session_upgrade2(
        session: *mut nghttp2_session,
        settings_payload: *const u8,
        settings_payloadlen: size_t,
        head_request: c_int,
        stream_user_data: *mut c_void,
    ) -> c_int;

    /// Pack settings payload
    pub fn nghttp2_pack_settings_payload(
        buf: *mut u8,
        buflen: size_t,
        iv: *const nghttp2_settings_entry,
        niv: size_t,
    ) -> ssize_t;

    // ========================================================================
    // Stream Response Data Functions (nh2 extension)
    // ========================================================================

    /// Get stream response data (headers and body)
    /// Returns a pointer to nghttp2_stream_response_data or null if stream not found
    /// Caller must free the returned data using nghttp2_stream_response_data_free
    pub fn nghttp2_session_get_stream_response_data(
        session: *mut nghttp2_session,
        stream_id: nghttp2_stream_id,
    ) -> *mut nghttp2_stream_response_data;

    /// Free stream response data allocated by nghttp2_session_get_stream_response_data
    pub fn nghttp2_stream_response_data_free(data: *mut nghttp2_stream_response_data);
}

// ============================================================================
// Stream Response Data Structures (nh2 extension)
// ============================================================================

/// Header field for response data
#[repr(C)]
pub struct nghttp2_header_field {
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
pub struct nghttp2_stream_response_data {
    /// Pointer to response headers array
    pub headers: *mut nghttp2_header_field,
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
pub fn nghttp2_is_ok(rv: c_int) -> bool {
    rv >= 0
}

/// Helper to check if would block
#[inline]
pub fn nghttp2_is_would_block(rv: c_int) -> bool {
    rv == NGHTTP2_ERR_WOULDBLOCK
}

/// Helper to check if EOF
#[inline]
pub fn nghttp2_is_eof(rv: c_int) -> bool {
    rv == NGHTTP2_ERR_EOF
}

/// Get version string safely
pub fn get_version_string() -> Option<&'static str> {
    let info = unsafe { nghttp2_version(0) };
    if info.is_null() {
        return None;
    }
    let version_str = unsafe { (*info).version_str };
    if version_str.is_null() {
        return None;
    }
    unsafe {
        std::ffi::CStr::from_ptr(version_str)
            .to_str()
            .ok()
    }
}
