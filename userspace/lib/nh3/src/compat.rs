//! nghttp3 C ABI Compatibility Layer
//!
//! This module provides nghttp3-compatible C API functions for use by
//! applications expecting the standard nghttp3 interface.

use crate::connection::{
    nghttp3_callbacks, ConnectionCallbacks, AckedStreamDataCallback, BeginHeadersCallback,
    EndHeadersCallback, EndStreamCallback, RecvDataCallback, RecvHeaderCallback,
    DeferredConsumeCallback, ResetStreamCallback, ShutdownCallback, StopSendingCallback,
    StreamCloseCallback,
};
use crate::types::{nghttp3_pri, nghttp3_rcbuf, nghttp3_vec, DataProvider, 
    Nv, Priority, ReadCallback, Vec3, nghttp3_data_reader};
use crate::{c_int, size_t, StreamId};

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
