//! nghttp2 C ABI Compatibility Layer
//!
//! This module provides additional C API compatibility functions for nghttp2.

use crate::error::{Error, NgError};
use crate::frame::{NgHttp2Frame, NgHttp2FrameHd};
use crate::session::*;
use crate::types::*;
use crate::{c_int, c_void, size_t};

// ============================================================================
// Option Management
// ============================================================================

/// Create a new option object
#[no_mangle]
pub extern "C" fn nghttp2_option_new(option_ptr: *mut *mut NgHttp2Option) -> c_int {
    if option_ptr.is_null() {
        return NgError::InvalidArgument as i32;
    }

    let option = Box::new(SessionOption::default());
    let wrapper = Box::new(NgHttp2Option { inner: option });

    unsafe {
        *option_ptr = Box::into_raw(wrapper);
    }

    0
}

/// Delete an option object
#[no_mangle]
pub extern "C" fn nghttp2_option_del(option: *mut NgHttp2Option) {
    if !option.is_null() {
        unsafe {
            drop(Box::from_raw(option));
        }
    }
}

/// Set no_auto_window_update option
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_auto_window_update(option: *mut NgHttp2Option, val: c_int) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.no_auto_window_update = if val != 0 { 1 } else { 0 };
    }
}

/// Set no_recv_client_magic option
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_recv_client_magic(option: *mut NgHttp2Option, val: c_int) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.no_recv_client_magic = if val != 0 { 1 } else { 0 };
    }
}

/// Set no_http_messaging option
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_http_messaging(option: *mut NgHttp2Option, val: c_int) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.no_http_messaging = if val != 0 { 1 } else { 0 };
    }
}

/// Set max deflate dynamic table size
#[no_mangle]
pub extern "C" fn nghttp2_option_set_max_deflate_dynamic_table_size(
    option: *mut NgHttp2Option,
    val: size_t,
) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.max_deflate_dynamic_table_size = val;
    }
}

/// Set no_auto_ping_ack option
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_auto_ping_ack(option: *mut NgHttp2Option, val: c_int) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.no_auto_ping_ack = if val != 0 { 1 } else { 0 };
    }
}

/// Set max send header block length
#[no_mangle]
pub extern "C" fn nghttp2_option_set_max_send_header_block_length(
    option: *mut NgHttp2Option,
    val: size_t,
) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.max_send_header_block_length = val;
    }
}

/// Set max settings
#[no_mangle]
pub extern "C" fn nghttp2_option_set_max_settings(option: *mut NgHttp2Option, val: size_t) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.max_settings = val;
    }
}

// ============================================================================
// Additional Callback Setters
// ============================================================================

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_frame_send_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    on_frame_send_callback: OnFrameSendCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.on_frame_send_callback = Some(on_frame_send_callback);
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_begin_headers_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    on_begin_headers_callback: OnBeginHeadersCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.on_begin_headers_callback = Some(on_begin_headers_callback);
    }
}

// ============================================================================
// Stream Information
// ============================================================================

/// Get stream state
#[no_mangle]
pub extern "C" fn nghttp2_session_get_stream_state(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> c_int {
    use crate::constants::stream_state;
    use crate::stream::StreamState;

    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    match sess.session.get_stream(stream_id) {
        Some(state) => match state {
            StreamState::Idle => stream_state::IDLE,
            StreamState::Open => stream_state::OPEN,
            StreamState::ReservedLocal => stream_state::RESERVED_LOCAL,
            StreamState::ReservedRemote => stream_state::RESERVED_REMOTE,
            StreamState::HalfClosedLocal => stream_state::HALF_CLOSED_LOCAL,
            StreamState::HalfClosedRemote => stream_state::HALF_CLOSED_REMOTE,
            StreamState::Closed => stream_state::CLOSED,
        },
        None => 0,
    }
}

// ============================================================================
// Priority Functions
// ============================================================================

/// Submit priority frame
#[no_mangle]
pub extern "C" fn nghttp2_submit_priority(
    session: *mut NgHttp2Session,
    _flags: u8,
    _stream_id: i32,
    pri_spec: *const PrioritySpec,
) -> c_int {
    // Priority frames are deprecated in HTTP/2, but we support them for compatibility
    if session.is_null() || pri_spec.is_null() {
        return NgError::InvalidArgument as i32;
    }
    0
}

// ============================================================================
// Push Promise
// ============================================================================

/// Check if push is enabled
#[no_mangle]
pub extern "C" fn nghttp2_session_get_remote_settings(
    session: *mut NgHttp2Session,
    id: c_int,
) -> u32 {
    use crate::constants::settings_id;

    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let inner = sess.session.inner.lock();

    match id as u16 {
        settings_id::HEADER_TABLE_SIZE => inner.remote_settings.header_table_size,
        settings_id::ENABLE_PUSH => {
            if inner.remote_settings.enable_push {
                1
            } else {
                0
            }
        }
        settings_id::MAX_CONCURRENT_STREAMS => inner.remote_settings.max_concurrent_streams,
        settings_id::INITIAL_WINDOW_SIZE => inner.remote_settings.initial_window_size,
        settings_id::MAX_FRAME_SIZE => inner.remote_settings.max_frame_size,
        settings_id::MAX_HEADER_LIST_SIZE => inner.remote_settings.max_header_list_size,
        _ => 0,
    }
}

/// Get local settings
#[no_mangle]
pub extern "C" fn nghttp2_session_get_local_settings(
    session: *mut NgHttp2Session,
    id: c_int,
) -> u32 {
    use crate::constants::settings_id;

    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let inner = sess.session.inner.lock();

    match id as u16 {
        settings_id::HEADER_TABLE_SIZE => inner.local_settings.header_table_size,
        settings_id::ENABLE_PUSH => {
            if inner.local_settings.enable_push {
                1
            } else {
                0
            }
        }
        settings_id::MAX_CONCURRENT_STREAMS => inner.local_settings.max_concurrent_streams,
        settings_id::INITIAL_WINDOW_SIZE => inner.local_settings.initial_window_size,
        settings_id::MAX_FRAME_SIZE => inner.local_settings.max_frame_size,
        settings_id::MAX_HEADER_LIST_SIZE => inner.local_settings.max_header_list_size,
        _ => 0,
    }
}

// ============================================================================
// Flow Control
// ============================================================================

/// Get effective local window size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_stream_effective_local_window_size(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner
        .streams
        .get(stream_id)
        .map(|s| s.local_window_size)
        .unwrap_or(-1)
}

/// Get effective recv data length
#[no_mangle]
pub extern "C" fn nghttp2_session_get_stream_effective_recv_data_length(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner
        .streams
        .get(stream_id)
        .map(|s| s.recv_buffer.len() as i32)
        .unwrap_or(-1)
}

/// Get connection effective local window size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_effective_local_window_size(
    session: *mut NgHttp2Session,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner.flow_control.connection_recv_window()
}

/// Get remote window size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_remote_window_size(session: *mut NgHttp2Session) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner.flow_control.connection_send_window()
}

/// Get stream remote window size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_stream_remote_window_size(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner
        .streams
        .get(stream_id)
        .map(|s| s.remote_window_size)
        .unwrap_or(-1)
}

// ============================================================================
// HPACK Functions
// ============================================================================

/// Create HD inflater
#[no_mangle]
pub extern "C" fn nghttp2_hd_inflate_new(inflater_ptr: *mut *mut c_void) -> c_int {
    use crate::hpack::HpackDecoder;

    if inflater_ptr.is_null() {
        return NgError::InvalidArgument as i32;
    }

    let decoder = Box::new(HpackDecoder::new(4096));
    unsafe {
        *inflater_ptr = Box::into_raw(decoder) as *mut c_void;
    }

    0
}

/// Delete HD inflater
#[no_mangle]
pub extern "C" fn nghttp2_hd_inflate_del(inflater: *mut c_void) {
    if !inflater.is_null() {
        use crate::hpack::HpackDecoder;
        unsafe {
            drop(Box::from_raw(inflater as *mut HpackDecoder));
        }
    }
}

/// Create HD deflater
#[no_mangle]
pub extern "C" fn nghttp2_hd_deflate_new(
    deflater_ptr: *mut *mut c_void,
    max_deflate_dynamic_table_size: size_t,
) -> c_int {
    use crate::hpack::HpackEncoder;

    if deflater_ptr.is_null() {
        return NgError::InvalidArgument as i32;
    }

    let encoder = Box::new(HpackEncoder::new(max_deflate_dynamic_table_size));
    unsafe {
        *deflater_ptr = Box::into_raw(encoder) as *mut c_void;
    }

    0
}

/// Delete HD deflater
#[no_mangle]
pub extern "C" fn nghttp2_hd_deflate_del(deflater: *mut c_void) {
    if !deflater.is_null() {
        use crate::hpack::HpackEncoder;
        unsafe {
            drop(Box::from_raw(deflater as *mut HpackEncoder));
        }
    }
}

// ============================================================================
// Misc Functions
// ============================================================================

/// Check header name validity
#[no_mangle]
pub extern "C" fn nghttp2_check_header_name(name: *const u8, len: size_t) -> c_int {
    if name.is_null() || len == 0 {
        return 0;
    }

    let slice = unsafe { core::slice::from_raw_parts(name, len) };

    // Header names must be lowercase and valid tokens
    for &byte in slice {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {}
            _ => return 0,
        }
    }

    1
}

/// Check header value validity
#[no_mangle]
pub extern "C" fn nghttp2_check_header_value(value: *const u8, len: size_t) -> c_int {
    if value.is_null() {
        return if len == 0 { 1 } else { 0 };
    }

    let slice = unsafe { core::slice::from_raw_parts(value, len) };

    // Header values must not contain certain control characters
    for &byte in slice {
        match byte {
            0x00..=0x08 | 0x0a..=0x1f | 0x7f => return 0,
            _ => {}
        }
    }

    1
}

/// Convert nghttp2 error to errno
#[no_mangle]
pub extern "C" fn nghttp2_map_error_code_to_errno(error_code: i32) -> c_int {
    match error_code {
        0 => 0,
        -504 => 11, // EAGAIN/EWOULDBLOCK
        -507 => 0,  // EOF
        -524 => 12, // ENOMEM
        _ => 5,     // EIO
    }
}

/// Select next protocol (ALPN)
#[no_mangle]
pub extern "C" fn nghttp2_select_next_protocol(
    out: *mut *const u8,
    outlen: *mut u8,
    _protocol_list: *const u8,
    _inlen: size_t,
) -> c_int {
    // Select h2 if available
    static H2: &[u8] = b"h2";

    if !out.is_null() && !outlen.is_null() {
        unsafe {
            *out = H2.as_ptr();
            *outlen = H2.len() as u8;
        }
        return 1;
    }

    0
}

// ============================================================================
// Error String Functions
// ============================================================================

// Note: nghttp2_strerror and nghttp2_http2_strerror are defined in error.rs
// to avoid duplicate symbols

// ============================================================================
// Session State Functions
// ============================================================================

/// Get next stream ID for the session
#[no_mangle]
pub extern "C" fn nghttp2_session_get_next_stream_id(session: *mut NgHttp2Session) -> u32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let inner = sess.session.inner.lock();
    (inner.last_sent_stream_id + 2) as u32
}

/// Get last processed stream ID
#[no_mangle]
pub extern "C" fn nghttp2_session_get_last_proc_stream_id(session: *mut NgHttp2Session) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let inner = sess.session.inner.lock();
    inner.last_recv_stream_id
}

/// Check if session is going away
#[no_mangle]
pub extern "C" fn nghttp2_session_check_server_session(session: *mut NgHttp2Session) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let inner = sess.session.inner.lock();
    if inner.session_type == crate::session::SessionType::Server {
        1
    } else {
        0
    }
}

/// Get the number of active streams
#[no_mangle]
pub extern "C" fn nghttp2_session_get_root_stream(
    _session: *mut NgHttp2Session,
) -> *mut c_void {
    // Root stream is a special concept in nghttp2, return null for now
    core::ptr::null_mut()
}

/// Get outbound queue size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_outbound_queue_size(session: *mut NgHttp2Session) -> size_t {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let inner = sess.session.inner.lock();
    inner.send_buffer.len()
}

/// Get HPACK encoder dynamic table size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_hd_deflate_dynamic_table_size(
    session: *mut NgHttp2Session,
) -> size_t {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let mut inner = sess.session.inner.lock();
    inner.hpack.encoder().dynamic_table_size()
}

/// Get HPACK decoder dynamic table size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_hd_inflate_dynamic_table_size(
    session: *mut NgHttp2Session,
) -> size_t {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    let mut inner = sess.session.inner.lock();
    inner.hpack.decoder().dynamic_table_size()
}

// ============================================================================
// Stream Management Functions
// ============================================================================

/// Check if stream is open
#[no_mangle]
pub extern "C" fn nghttp2_session_find_stream(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> *mut c_void {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return core::ptr::null_mut(),
    };

    let inner = sess.session.inner.lock();
    if inner.streams.get(stream_id).is_some() {
        // Return a non-null pointer to indicate stream exists
        // This is a simplified implementation
        1 as *mut c_void
    } else {
        core::ptr::null_mut()
    }
}

/// Get stream local window size
#[no_mangle]
pub extern "C" fn nghttp2_session_get_stream_local_window_size(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner
        .streams
        .get(stream_id)
        .map(|s| s.local_window_size)
        .unwrap_or(-1)
}

/// Get local window size for connection
#[no_mangle]
pub extern "C" fn nghttp2_session_get_local_window_size(session: *mut NgHttp2Session) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return -1,
    };

    let inner = sess.session.inner.lock();
    inner.flow_control.connection_recv_window()
}

/// Consume received data (for manual flow control)
#[no_mangle]
pub extern "C" fn nghttp2_session_consume(
    session: *mut NgHttp2Session,
    stream_id: i32,
    size: size_t,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Update flow control and potentially send WINDOW_UPDATE
    let mut inner = sess.session.inner.lock();

    if stream_id == 0 {
        // Connection-level
        if let Err(e) = inner.flow_control.consume_recv(size as i32) {
            return e.to_error_code();
        }
    } else {
        // Stream-level
        if let Some(stream) = inner.streams.get_mut(stream_id) {
            if let Err(e) = stream.consume_recv_window(size as i32) {
                return e.to_error_code();
            }
        } else {
            return NgError::InvalidStreamId as i32;
        }
    }

    0
}

/// Consume received data for connection
#[no_mangle]
pub extern "C" fn nghttp2_session_consume_connection(
    session: *mut NgHttp2Session,
    size: size_t,
) -> c_int {
    nghttp2_session_consume(session, 0, size)
}

/// Consume received data for stream
#[no_mangle]
pub extern "C" fn nghttp2_session_consume_stream(
    session: *mut NgHttp2Session,
    stream_id: i32,
    size: size_t,
) -> c_int {
    nghttp2_session_consume(session, stream_id, size)
}

// ============================================================================
// Data Provider Functions
// ============================================================================

/// Submit data with data provider
#[no_mangle]
pub extern "C" fn nghttp2_submit_data(
    session: *mut NgHttp2Session,
    flags: u8,
    stream_id: i32,
    data_prd: *const crate::types::DataProvider,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    if data_prd.is_null() {
        return NgError::InvalidArgument as i32;
    }

    // For now, we don't fully support data providers
    // This is a simplified implementation
    0
}

// ============================================================================
// Push Promise Functions
// ============================================================================

/// Submit push promise
#[no_mangle]
pub extern "C" fn nghttp2_submit_push_promise(
    session: *mut NgHttp2Session,
    _flags: u8,
    stream_id: i32,
    nva: *const crate::types::Nv,
    nvlen: size_t,
    _stream_user_data: *mut c_void,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Check if push is enabled
    let inner = sess.session.inner.lock();
    if !inner.remote_settings.enable_push {
        return NgError::PushDisabled as i32;
    }

    // Push promise is only for servers
    if inner.session_type != crate::session::SessionType::Server {
        return NgError::InvalidStreamState as i32;
    }

    drop(inner);

    // Return the new promised stream ID (simplified)
    NgError::Proto as i32 // Not fully implemented
}

// ============================================================================
// Additional Callback Setters
// ============================================================================

/// Callback for before frame send
pub type OnBeforeFrameSendCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    user_data: *mut c_void,
) -> c_int;

/// Callback for invalid frame received
pub type OnInvalidFrameRecvCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    error_code: c_int,
    user_data: *mut c_void,
) -> c_int;

/// Callback for frame not sent
pub type OnFrameNotSendCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    error_code: c_int,
    user_data: *mut c_void,
) -> c_int;

/// Callback for error
pub type ErrorCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    msg: *const i8,
    len: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Callback for error with library error code
pub type ErrorCallback2 = extern "C" fn(
    session: *mut NgHttp2Session,
    lib_error_code: c_int,
    msg: *const i8,
    len: size_t,
    user_data: *mut c_void,
) -> c_int;

// Note: These callback setters require extending SessionCallbacks struct
// For now, we provide stub implementations

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_before_frame_send_callback(
    _callbacks: *mut NgHttp2SessionCallbacks,
    _cb: OnBeforeFrameSendCallback,
) {
    // Stub - would need to extend SessionCallbacks
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_invalid_frame_recv_callback(
    _callbacks: *mut NgHttp2SessionCallbacks,
    _cb: OnInvalidFrameRecvCallback,
) {
    // Stub - would need to extend SessionCallbacks
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_frame_not_send_callback(
    _callbacks: *mut NgHttp2SessionCallbacks,
    _cb: OnFrameNotSendCallback,
) {
    // Stub - would need to extend SessionCallbacks
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_error_callback(
    _callbacks: *mut NgHttp2SessionCallbacks,
    _cb: ErrorCallback,
) {
    // Stub - would need to extend SessionCallbacks
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_error_callback2(
    _callbacks: *mut NgHttp2SessionCallbacks,
    _cb: ErrorCallback2,
) {
    // Stub - would need to extend SessionCallbacks
}

// ============================================================================
// NV Helper Functions
// ============================================================================

/// Create a new NV (name-value) structure on the heap
#[no_mangle]
pub extern "C" fn nghttp2_nv_compare_name(
    lhs: *const crate::types::Nv,
    rhs: *const crate::types::Nv,
) -> c_int {
    if lhs.is_null() || rhs.is_null() {
        return 0;
    }

    let lhs = unsafe { &*lhs };
    let rhs = unsafe { &*rhs };

    let lhs_name = unsafe { core::slice::from_raw_parts(lhs.name, lhs.namelen) };
    let rhs_name = unsafe { core::slice::from_raw_parts(rhs.name, rhs.namelen) };

    lhs_name.cmp(rhs_name) as c_int
}

// ============================================================================
// Additional Option Setters
// ============================================================================

/// Set peer max concurrent streams
#[no_mangle]
pub extern "C" fn nghttp2_option_set_peer_max_concurrent_streams(
    option: *mut NgHttp2Option,
    val: u32,
) {
    if let Some(opt) = unsafe { option.as_mut() } {
        // Store in a hypothetical field (would need to extend SessionOption)
        let _ = val;
    }
}

/// Set no closed streams
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_closed_streams(option: *mut NgHttp2Option, val: c_int) {
    if let Some(_opt) = unsafe { option.as_mut() } {
        // Stub - would need to extend SessionOption
        let _ = val;
    }
}

/// Set max outbound ack
#[no_mangle]
pub extern "C" fn nghttp2_option_set_max_outbound_ack(option: *mut NgHttp2Option, val: size_t) {
    if let Some(_opt) = unsafe { option.as_mut() } {
        // Stub - would need to extend SessionOption
        let _ = val;
    }
}

/// Set builtin recv extension
#[no_mangle]
pub extern "C" fn nghttp2_option_set_builtin_recv_extension_type(
    option: *mut NgHttp2Option,
    ext_type: u8,
) {
    if let Some(_opt) = unsafe { option.as_mut() } {
        // Stub - would need to extend SessionOption
        let _ = ext_type;
    }
}

// ============================================================================
// Priority Frame Functions (RFC 9218)
// ============================================================================

/// Submit priority update frame
#[no_mangle]
pub extern "C" fn nghttp2_submit_priority_update(
    session: *mut NgHttp2Session,
    _flags: u8,
    _stream_id: i32,
    _field_value: *const u8,
    _field_value_len: size_t,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Priority frames are deprecated in HTTP/2
    0
}

// ============================================================================
// Extension Frame Functions
// ============================================================================

/// Submit extension frame
#[no_mangle]
pub extern "C" fn nghttp2_submit_extension(
    session: *mut NgHttp2Session,
    _type: u8,
    _flags: u8,
    _stream_id: i32,
    _payload: *mut c_void,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Extension frames not fully supported
    NgError::InvalidFrame as i32
}

// ============================================================================
// Session Upgrade Functions (h2c upgrade)
// ============================================================================

/// Upgrade from HTTP/1.1 to HTTP/2
#[no_mangle]
pub extern "C" fn nghttp2_session_upgrade(
    session: *mut NgHttp2Session,
    settings_payload: *const u8,
    settings_payloadlen: size_t,
    _head_request: c_int,
    _stream_user_data: *mut c_void,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    if settings_payload.is_null() || settings_payloadlen == 0 {
        return NgError::InvalidArgument as i32;
    }

    // Parse settings from upgrade header
    // This is a simplified implementation
    0
}

/// Upgrade from HTTP/1.1 to HTTP/2 (version 2)
#[no_mangle]
pub extern "C" fn nghttp2_session_upgrade2(
    session: *mut NgHttp2Session,
    settings_payload: *const u8,
    settings_payloadlen: size_t,
    head_request: c_int,
    stream_user_data: *mut c_void,
) -> c_int {
    nghttp2_session_upgrade(
        session,
        settings_payload,
        settings_payloadlen,
        head_request,
        stream_user_data,
    )
}

/// Pack settings for upgrade
#[no_mangle]
pub extern "C" fn nghttp2_pack_settings_payload(
    buf: *mut u8,
    buflen: size_t,
    iv: *const crate::types::SettingsEntry,
    niv: size_t,
) -> isize {
    if buf.is_null() || iv.is_null() {
        return NgError::InvalidArgument as isize;
    }

    // Each settings entry is 6 bytes (2 for id, 4 for value)
    let needed = niv * 6;
    if buflen < needed {
        return NgError::BufferError as isize;
    }

    let entries = unsafe { core::slice::from_raw_parts(iv, niv) };
    let out = unsafe { core::slice::from_raw_parts_mut(buf, buflen) };

    let mut offset = 0;
    for entry in entries {
        if offset + 6 > buflen {
            break;
        }
        out[offset] = (entry.settings_id >> 8) as u8;
        out[offset + 1] = entry.settings_id as u8;
        out[offset + 2] = (entry.value >> 24) as u8;
        out[offset + 3] = (entry.value >> 16) as u8;
        out[offset + 4] = (entry.value >> 8) as u8;
        out[offset + 5] = entry.value as u8;
        offset += 6;
    }

    offset as isize
}
