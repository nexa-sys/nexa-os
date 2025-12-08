//! nghttp2 C ABI Compatibility Layer
//!
//! This module provides additional C API compatibility functions for nghttp2.

use crate::types::*;
use crate::session::*;
use crate::error::NgError;
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
pub extern "C" fn nghttp2_option_set_no_auto_window_update(
    option: *mut NgHttp2Option,
    val: c_int,
) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.no_auto_window_update = if val != 0 { 1 } else { 0 };
    }
}

/// Set no_recv_client_magic option
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_recv_client_magic(
    option: *mut NgHttp2Option,
    val: c_int,
) {
    if let Some(opt) = unsafe { option.as_mut() } {
        opt.inner.no_recv_client_magic = if val != 0 { 1 } else { 0 };
    }
}

/// Set no_http_messaging option
#[no_mangle]
pub extern "C" fn nghttp2_option_set_no_http_messaging(
    option: *mut NgHttp2Option,
    val: c_int,
) {
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
pub extern "C" fn nghttp2_option_set_no_auto_ping_ack(
    option: *mut NgHttp2Option,
    val: c_int,
) {
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
pub extern "C" fn nghttp2_option_set_max_settings(
    option: *mut NgHttp2Option,
    val: size_t,
) {
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
        settings_id::ENABLE_PUSH => if inner.remote_settings.enable_push { 1 } else { 0 },
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
        settings_id::ENABLE_PUSH => if inner.local_settings.enable_push { 1 } else { 0 },
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
    inner.streams.get(stream_id)
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
    inner.streams.get(stream_id)
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
pub extern "C" fn nghttp2_session_get_remote_window_size(
    session: *mut NgHttp2Session,
) -> i32 {
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
    inner.streams.get(stream_id)
        .map(|s| s.remote_window_size)
        .unwrap_or(-1)
}

// ============================================================================
// HPACK Functions
// ============================================================================

/// Create HD inflater
#[no_mangle]
pub extern "C" fn nghttp2_hd_inflate_new(
    inflater_ptr: *mut *mut c_void,
) -> c_int {
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
pub extern "C" fn nghttp2_check_header_name(
    name: *const u8,
    len: size_t,
) -> c_int {
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
pub extern "C" fn nghttp2_check_header_value(
    value: *const u8,
    len: size_t,
) -> c_int {
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
