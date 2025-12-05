//! TLS 1.3 Early Data (0-RTT) Support
//!
//! Provides functions for TLS 1.3 early data handling.

use crate::{c_int, c_uchar};
use crate::connection::SslConnection;
use crate::context::SslContext;

/// Early data status
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarlyDataStatus {
    /// Early data was not sent/received
    NotSent = 0,
    /// Early data was rejected by server
    Rejected = 1,
    /// Early data was accepted
    Accepted = 2,
}

/// Maximum early data size (default: 16KB)
pub const SSL_MAX_EARLY_DATA_SIZE: u32 = 16384;

// ============================================================================
// SSL_CTX Early Data Functions
// ============================================================================

/// Set maximum early data size for context
#[no_mangle]
pub extern "C" fn SSL_CTX_set_max_early_data(ctx: *mut SslContext, max_early_data: u32) -> c_int {
    if ctx.is_null() {
        return 0;
    }
    // Store max early data size (simplified: always accept up to 16KB)
    if max_early_data <= SSL_MAX_EARLY_DATA_SIZE {
        1
    } else {
        0
    }
}

/// Get maximum early data size from context
#[no_mangle]
pub extern "C" fn SSL_CTX_get_max_early_data(ctx: *const SslContext) -> u32 {
    if ctx.is_null() {
        return 0;
    }
    SSL_MAX_EARLY_DATA_SIZE
}

// ============================================================================
// SSL Early Data Functions
// ============================================================================

/// Set maximum early data size for connection
#[no_mangle]
pub extern "C" fn SSL_set_max_early_data(ssl: *mut SslConnection, max_early_data: u32) -> c_int {
    if ssl.is_null() {
        return 0;
    }
    if max_early_data <= SSL_MAX_EARLY_DATA_SIZE {
        1
    } else {
        0
    }
}

/// Get maximum early data size from connection
#[no_mangle]
pub extern "C" fn SSL_get_max_early_data(ssl: *const SslConnection) -> u32 {
    if ssl.is_null() {
        return 0;
    }
    SSL_MAX_EARLY_DATA_SIZE
}

/// Write early data (client)
#[no_mangle]
pub extern "C" fn SSL_write_early_data(
    ssl: *mut SslConnection,
    buf: *const c_uchar,
    num: usize,
    written: *mut usize,
) -> c_int {
    if ssl.is_null() || buf.is_null() || written.is_null() {
        return 0;
    }

    // TLS 1.3 early data write
    // Simplified: delegate to regular write with early data flag
    // In real implementation, would send early data before handshake completes

    unsafe {
        // For now, indicate that early data is not supported in this simplified impl
        *written = 0;
    }

    // Return 0 to indicate early data could not be sent
    // Caller should fall back to regular handshake
    0
}

/// Read early data (server)
#[no_mangle]
pub extern "C" fn SSL_read_early_data(
    ssl: *mut SslConnection,
    buf: *mut c_uchar,
    num: usize,
    readbytes: *mut usize,
) -> c_int {
    if ssl.is_null() || buf.is_null() || readbytes.is_null() {
        return 0; // SSL_READ_EARLY_DATA_ERROR
    }

    unsafe {
        *readbytes = 0;
    }

    // Return status indicating end of early data (simplified)
    2 // SSL_READ_EARLY_DATA_FINISH
}

/// Get early data status
#[no_mangle]
pub extern "C" fn SSL_get_early_data_status(ssl: *const SslConnection) -> c_int {
    if ssl.is_null() {
        return EarlyDataStatus::NotSent as c_int;
    }
    // Return status (simplified: always not sent)
    EarlyDataStatus::NotSent as c_int
}

// ============================================================================
// Read/Write Early Data Constants
// ============================================================================

/// Early data read/write return codes
pub mod early_data_status {
    pub const SSL_READ_EARLY_DATA_ERROR: i32 = 0;
    pub const SSL_READ_EARLY_DATA_SUCCESS: i32 = 1;
    pub const SSL_READ_EARLY_DATA_FINISH: i32 = 2;

    pub const SSL_EARLY_DATA_NOT_SENT: i32 = 0;
    pub const SSL_EARLY_DATA_REJECTED: i32 = 1;
    pub const SSL_EARLY_DATA_ACCEPTED: i32 = 2;
}
