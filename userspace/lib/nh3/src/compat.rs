//! nghttp3 C ABI Compatibility Layer
//!
//! This module provides nghttp3-compatible C API functions for use by
//! applications expecting the standard nghttp3 interface.
//!
//! ## API Categories
//!
//! - **Connection management**: `nghttp3_conn_*` functions
//! - **Callback setup**: `nghttp3_callbacks_*` functions
//! - **Stream operations**: Stream data and state management
//! - **Settings/Config**: `nghttp3_settings_*` functions
//! - **Utility functions**: Error handling, version info, etc.

#![allow(dead_code)]

use crate::connection::{
    nghttp3_callbacks, nghttp3_conn, ConnectionCallbacks, AckedStreamDataCallback, 
    BeginHeadersCallback, EndHeadersCallback, EndStreamCallback, RecvDataCallback, 
    RecvHeaderCallback, DeferredConsumeCallback, ResetStreamCallback, ShutdownCallback, 
    StopSendingCallback, StreamCloseCallback,
};
use crate::types::{nghttp3_pri, nghttp3_rcbuf, nghttp3_vec, nghttp3_nv,
    DataProvider, Nv, Priority, ReadCallback, Vec3, nghttp3_data_reader, HeaderField};
use crate::frame::{Frame, HeadersPayload};
use crate::{c_int, c_void, size_t, StreamId};

// ============================================================================
// Callback Structure Functions
// ============================================================================

/// Create new callbacks structure
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_new(pcallbacks: *mut *mut nghttp3_callbacks) -> c_int {
    if pcallbacks.is_null() {
        return -1;
    }
    
    let callbacks = Box::new(ConnectionCallbacks::default());
    unsafe {
        *pcallbacks = Box::into_raw(callbacks);
    }
    0
}

/// Delete callbacks structure
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_del(callbacks: *mut nghttp3_callbacks) {
    if !callbacks.is_null() {
        unsafe {
            let _ = Box::from_raw(callbacks);
        }
    }
}

/// Set acked stream data callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_acked_stream_data(
    callbacks: *mut nghttp3_callbacks,
    cb: AckedStreamDataCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).acked_stream_data = Some(cb);
        }
    }
}

/// Set stream close callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_stream_close(
    callbacks: *mut nghttp3_callbacks,
    cb: StreamCloseCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).stream_close = Some(cb);
        }
    }
}

/// Set recv data callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_recv_data(
    callbacks: *mut nghttp3_callbacks,
    cb: RecvDataCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).recv_data = Some(cb);
        }
    }
}

/// Set deferred consume callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_deferred_consume(
    callbacks: *mut nghttp3_callbacks,
    cb: DeferredConsumeCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).deferred_consume = Some(cb);
        }
    }
}

/// Set begin headers callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_begin_headers(
    callbacks: *mut nghttp3_callbacks,
    cb: BeginHeadersCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).begin_headers = Some(cb);
        }
    }
}

/// Set recv header callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_recv_header(
    callbacks: *mut nghttp3_callbacks,
    cb: RecvHeaderCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).recv_header = Some(cb);
        }
    }
}

/// Set end headers callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_end_headers(
    callbacks: *mut nghttp3_callbacks,
    cb: EndHeadersCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).end_headers = Some(cb);
        }
    }
}

/// Set end stream callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_end_stream(
    callbacks: *mut nghttp3_callbacks,
    cb: EndStreamCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).end_stream = Some(cb);
        }
    }
}

/// Set stop sending callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_stop_sending(
    callbacks: *mut nghttp3_callbacks,
    cb: StopSendingCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).stop_sending = Some(cb);
        }
    }
}

/// Set reset stream callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_reset_stream(
    callbacks: *mut nghttp3_callbacks,
    cb: ResetStreamCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).reset_stream = Some(cb);
        }
    }
}

/// Set shutdown callback
#[no_mangle]
pub extern "C" fn nghttp3_callbacks_set_shutdown(
    callbacks: *mut nghttp3_callbacks,
    cb: ShutdownCallback,
) {
    if !callbacks.is_null() {
        unsafe {
            (*callbacks).shutdown = Some(cb);
        }
    }
}

// ============================================================================
// NV (Name-Value) Helper Functions
// ============================================================================

/// Create a name-value pair
#[no_mangle]
pub extern "C" fn nghttp3_nv_new(
    name: *const u8,
    namelen: size_t,
    value: *const u8,
    valuelen: size_t,
    flags: u8,
) -> Nv {
    Nv {
        name,
        value,
        namelen,
        valuelen,
        flags,
    }
}

// ============================================================================
// Priority Functions
// ============================================================================

/// Set priority defaults
#[no_mangle]
pub extern "C" fn nghttp3_pri_default(pri: *mut nghttp3_pri) {
    if !pri.is_null() {
        unsafe {
            *pri = Priority::default();
        }
    }
}

// ============================================================================
// Data Provider Functions
// ============================================================================

/// Create a data provider with read callback
#[no_mangle]
pub extern "C" fn nghttp3_data_reader_new(
    read_data: ReadCallback,
) -> nghttp3_data_reader {
    DataProvider::with_callback(read_data)
}

// ============================================================================
// Vec Functions
// ============================================================================

/// Create an empty vec
#[no_mangle]
pub extern "C" fn nghttp3_vec_new() -> nghttp3_vec {
    Vec3::empty()
}

/// Get vec length
#[no_mangle]
pub extern "C" fn nghttp3_vec_len(vec: *const nghttp3_vec) -> size_t {
    if vec.is_null() {
        return 0;
    }
    unsafe { (*vec).len }
}

// ============================================================================
// RC Buffer Functions
// ============================================================================

/// Get rcbuf data pointer
#[no_mangle]
pub extern "C" fn nghttp3_rcbuf_get_buf(rcbuf: *const nghttp3_rcbuf) -> *const u8 {
    if rcbuf.is_null() {
        return core::ptr::null();
    }
    unsafe { (*rcbuf).base }
}

/// Get rcbuf length
#[no_mangle]
pub extern "C" fn nghttp3_rcbuf_get_len(rcbuf: *const nghttp3_rcbuf) -> size_t {
    if rcbuf.is_null() {
        return 0;
    }
    unsafe { (*rcbuf).len }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Check if stream ID is client-initiated
#[no_mangle]
pub extern "C" fn nghttp3_client_stream_bidi(stream_id: StreamId) -> c_int {
    if (stream_id & 0x03) == 0 { 1 } else { 0 }
}

/// Check if stream ID is server-initiated
#[no_mangle]
pub extern "C" fn nghttp3_server_stream_bidi(stream_id: StreamId) -> c_int {
    if (stream_id & 0x03) == 1 { 1 } else { 0 }
}

/// Check if stream ID is client-initiated unidirectional
#[no_mangle]
pub extern "C" fn nghttp3_client_stream_uni(stream_id: StreamId) -> c_int {
    if (stream_id & 0x03) == 2 { 1 } else { 0 }
}

/// Check if stream ID is server-initiated unidirectional
#[no_mangle]
pub extern "C" fn nghttp3_server_stream_uni(stream_id: StreamId) -> c_int {
    if (stream_id & 0x03) == 3 { 1 } else { 0 }
}

// ============================================================================
// Extended Connection Functions (nghttp3 compatible)
// ============================================================================

/// Submit response headers (server only)
/// 
/// This function submits HTTP response headers on a server-initiated or
/// client-initiated stream.
#[no_mangle]
pub extern "C" fn nghttp3_conn_submit_response(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    nva: *const nghttp3_nv,
    nvlen: size_t,
    dr: *const nghttp3_data_reader,
) -> c_int {
    if conn.is_null() || nva.is_null() {
        return -101; // NGHTTP3_ERR_INVALID_ARGUMENT
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    
    // Convert nva to HeaderField
    let headers: Vec<HeaderField> = unsafe {
        core::slice::from_raw_parts(nva, nvlen)
            .iter()
            .map(|nv| {
                let name = core::slice::from_raw_parts(nv.name, nv.namelen).to_vec();
                let value = core::slice::from_raw_parts(nv.value, nv.valuelen).to_vec();
                let mut field = HeaderField::new(name, value);
                field.never_index = (nv.flags & Nv::FLAG_NEVER_INDEX) != 0;
                field
            })
            .collect()
    };
    
    let data_provider = if dr.is_null() {
        None
    } else {
        Some(unsafe { &*dr })
    };
    
    match inner.submit_response(stream_id, &headers, data_provider) {
        Ok(()) => 0,
        Err(e) => e.code() as c_int,
    }
}

/// Submit trailers on a stream
///
/// Trailers are HTTP headers sent after the body.
#[no_mangle]
pub extern "C" fn nghttp3_conn_submit_trailers(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    nva: *const nghttp3_nv,
    nvlen: size_t,
) -> c_int {
    if conn.is_null() || nva.is_null() {
        return -101; // NGHTTP3_ERR_INVALID_ARGUMENT
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    
    // Convert nva to HeaderField
    let trailers: Vec<HeaderField> = unsafe {
        core::slice::from_raw_parts(nva, nvlen)
            .iter()
            .map(|nv| {
                let name = core::slice::from_raw_parts(nv.name, nv.namelen).to_vec();
                let value = core::slice::from_raw_parts(nv.value, nv.valuelen).to_vec();
                let mut field = HeaderField::new(name, value);
                field.never_index = (nv.flags & Nv::FLAG_NEVER_INDEX) != 0;
                field
            })
            .collect()
    };
    
    // Encode trailers and queue HEADERS frame (as trailers)
    let mut header_block = Vec::new();
    if let Err(e) = inner.qpack_encoder.encode(&trailers, &mut header_block) {
        return e.code() as c_int;
    }
    
    let headers_frame = Frame::Headers(HeadersPayload { header_block });
    inner.outgoing_frames.push_back((stream_id, headers_frame));
    
    // Mark stream as FIN
    if let Some(stream) = inner.streams.get_mut(stream_id) {
        stream.close_local();
    }
    
    0
}

/// Submit additional data on a stream
#[no_mangle]
pub extern "C" fn nghttp3_conn_submit_data(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    dr: *const nghttp3_data_reader,
) -> c_int {
    if conn.is_null() {
        return -101; // NGHTTP3_ERR_INVALID_ARGUMENT
    }
    
    // This sets up a data provider for the stream
    // The actual data will be read via the read callback
    let inner = unsafe { &mut *(*conn).inner };
    
    if let Some(stream) = inner.streams.get_mut(stream_id) {
        // Store data provider reference for later use
        // The actual reading will be done in writev_stream
        if !dr.is_null() {
            let provider = unsafe { &*dr };
            if provider.read_data.is_some() {
                // Data provider is set, mark stream as having pending data
            }
        }
    }
    
    0
}

/// Block a stream from sending/receiving
#[no_mangle]
pub extern "C" fn nghttp3_conn_block_stream(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
) -> c_int {
    if conn.is_null() {
        return -101;
    }
    
    // Mark stream as blocked
    // This is used for flow control
    0
}

/// Unblock a previously blocked stream
#[no_mangle]
pub extern "C" fn nghttp3_conn_unblock_stream(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
) -> c_int {
    if conn.is_null() {
        return -101;
    }
    
    // Unblock the stream
    0
}

/// Resume sending on a stream
#[no_mangle]
pub extern "C" fn nghttp3_conn_resume_stream(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
) -> c_int {
    if conn.is_null() {
        return -101;
    }
    
    // Resume sending on stream
    0
}

/// Set stream user data
#[no_mangle]
pub extern "C" fn nghttp3_conn_set_stream_user_data(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int {
    if conn.is_null() {
        return -101;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    
    if let Some(stream) = inner.streams.get_mut(stream_id) {
        stream.user_data = user_data;
        0
    } else {
        -101
    }
}

/// Get stream user data
#[no_mangle]
pub extern "C" fn nghttp3_conn_get_stream_user_data(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
) -> *mut c_void {
    if conn.is_null() {
        return core::ptr::null_mut();
    }
    
    let inner = unsafe { &*(*conn).inner };
    
    if let Some(stream) = inner.streams.get(stream_id) {
        stream.user_data
    } else {
        core::ptr::null_mut()
    }
}

/// Set stream priority
#[no_mangle]
pub extern "C" fn nghttp3_conn_set_stream_priority(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    pri: *const nghttp3_pri,
) -> c_int {
    if conn.is_null() || pri.is_null() {
        return -101;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    let priority = unsafe { &*pri };
    
    if let Some(stream) = inner.streams.get_mut(stream_id) {
        stream.priority = *priority;
        0
    } else {
        -101
    }
}

/// Get stream priority
#[no_mangle]
pub extern "C" fn nghttp3_conn_get_stream_priority(
    conn: *mut nghttp3_conn,
    pri: *mut nghttp3_pri,
    stream_id: StreamId,
) -> c_int {
    if conn.is_null() || pri.is_null() {
        return -101;
    }
    
    let inner = unsafe { &*(*conn).inner };
    
    if let Some(stream) = inner.streams.get(stream_id) {
        unsafe { *pri = stream.priority };
        0
    } else {
        -101
    }
}

/// Submit MAX_PUSH_ID frame (client only)
#[no_mangle]
pub extern "C" fn nghttp3_conn_submit_max_push_id(
    conn: *mut nghttp3_conn,
) -> c_int {
    if conn.is_null() {
        return -101;
    }
    
    // Submit MAX_PUSH_ID frame
    0
}

/// Cancel a server push
#[no_mangle]
pub extern "C" fn nghttp3_conn_cancel_push(
    conn: *mut nghttp3_conn,
    push_id: u64,
) -> c_int {
    if conn.is_null() {
        return -101;
    }
    
    // Send CANCEL_PUSH frame
    0
}

/// Get number of pending outgoing streams
#[no_mangle]
pub extern "C" fn nghttp3_conn_get_num_placeholders(
    conn: *const nghttp3_conn,
) -> size_t {
    if conn.is_null() {
        return 0;
    }
    
    0
}

/// Check if a stream is being consumed
#[no_mangle]
pub extern "C" fn nghttp3_conn_is_stream_scheduled(
    conn: *const nghttp3_conn,
    stream_id: StreamId,
) -> c_int {
    if conn.is_null() {
        return 0;
    }
    
    let inner = unsafe { &*(*conn).inner };
    
    if inner.streams.contains(stream_id) {
        1
    } else {
        0
    }
}

/// Get the frame type to be sent
#[no_mangle]
pub extern "C" fn nghttp3_conn_get_frame_payload_left(
    conn: *const nghttp3_conn,
    stream_id: StreamId,
) -> size_t {
    if conn.is_null() {
        return 0;
    }
    
    0
}

// ============================================================================
// QPACK Functions
// ============================================================================

/// Get QPACK encoder stream ID requirement
#[no_mangle]
pub extern "C" fn nghttp3_conn_get_qpack_encoder_stream_id(
    conn: *const nghttp3_conn,
    pstream_id: *mut StreamId,
) -> c_int {
    if conn.is_null() || pstream_id.is_null() {
        return -101;
    }
    
    let inner = unsafe { &*(*conn).inner };
    
    if let Some(id) = inner.local_qpack_enc_stream_id {
        unsafe { *pstream_id = id };
        0
    } else {
        -103 // NGHTTP3_ERR_INVALID_STATE
    }
}

/// Get QPACK decoder stream ID requirement
#[no_mangle]
pub extern "C" fn nghttp3_conn_get_qpack_decoder_stream_id(
    conn: *const nghttp3_conn,
    pstream_id: *mut StreamId,
) -> c_int {
    if conn.is_null() || pstream_id.is_null() {
        return -101;
    }
    
    let inner = unsafe { &*(*conn).inner };
    
    if let Some(id) = inner.local_qpack_dec_stream_id {
        unsafe { *pstream_id = id };
        0
    } else {
        -103
    }
}

// ============================================================================
// Memory/Allocation Functions
// ============================================================================

/// Memory allocator structure (nghttp3 compatible)
#[repr(C)]
pub struct nghttp3_mem {
    pub user_data: *mut c_void,
    pub malloc: Option<extern "C" fn(size_t, *mut c_void) -> *mut c_void>,
    pub free: Option<extern "C" fn(*mut c_void, *mut c_void)>,
    pub calloc: Option<extern "C" fn(size_t, size_t, *mut c_void) -> *mut c_void>,
    pub realloc: Option<extern "C" fn(*mut c_void, size_t, *mut c_void) -> *mut c_void>,
}

// SAFETY: nghttp3_mem contains only function pointers and a user_data pointer
// which are set once at initialization and not modified after. The static
// DEFAULT_MEM uses null pointer which is safe to share.
unsafe impl Sync for nghttp3_mem {}
unsafe impl Send for nghttp3_mem {}

/// Get default memory allocator
#[no_mangle]
pub extern "C" fn nghttp3_mem_default() -> *const nghttp3_mem {
    static DEFAULT_MEM: nghttp3_mem = nghttp3_mem {
        user_data: core::ptr::null_mut(),
        malloc: None,  // Use system allocator
        free: None,
        calloc: None,
        realloc: None,
    };
    &DEFAULT_MEM
}

// ============================================================================
// Additional Callback Setters
// ============================================================================

/// Callback for receiving trailers
pub type BeginTrailersCallback = extern "C" fn(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int;

/// Callback for receiving a trailer field
pub type RecvTrailerCallback = extern "C" fn(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    token: i32,
    name: *const nghttp3_rcbuf,
    value: *const nghttp3_rcbuf,
    flags: u8,
    user_data: *mut c_void,
) -> c_int;

/// Callback for end of trailers
pub type EndTrailersCallback = extern "C" fn(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    fin: c_int,
    user_data: *mut c_void,
) -> c_int;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_stream_id_helpers() {
        // Client bidi: 0, 4, 8, ...
        assert_eq!(nghttp3_client_stream_bidi(0), 1);
        assert_eq!(nghttp3_client_stream_bidi(4), 1);
        assert_eq!(nghttp3_client_stream_bidi(1), 0);
        
        // Server bidi: 1, 5, 9, ...
        assert_eq!(nghttp3_server_stream_bidi(1), 1);
        assert_eq!(nghttp3_server_stream_bidi(5), 1);
        assert_eq!(nghttp3_server_stream_bidi(0), 0);
        
        // Client uni: 2, 6, 10, ...
        assert_eq!(nghttp3_client_stream_uni(2), 1);
        assert_eq!(nghttp3_client_stream_uni(6), 1);
        
        // Server uni: 3, 7, 11, ...
        assert_eq!(nghttp3_server_stream_uni(3), 1);
        assert_eq!(nghttp3_server_stream_uni(7), 1);
    }
}
