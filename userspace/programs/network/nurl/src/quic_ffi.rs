//! QUIC FFI bindings for dynamic linking to libngtcp2.so (ntcp2)
//!
//! This module provides FFI declarations for dynamically linking against
//! libngtcp2.so (ntcp2) at runtime for QUIC transport used by HTTP/3.
//!
//! # Usage
//! ```rust
//! use nurl::quic_ffi::*;
//!
//! // Get version info
//! let info = unsafe { ngtcp2_version(0) };
//!
//! // Create settings
//! let mut settings: ngtcp2_settings = unsafe { std::mem::zeroed() };
//! unsafe { ngtcp2_settings_default(&mut settings) };
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_void};

// ============================================================================
// Type Definitions
// ============================================================================

pub type size_t = usize;
pub type ssize_t = isize;

// ============================================================================
// Opaque Types
// ============================================================================

/// Opaque ngtcp2_conn type
#[repr(C)]
pub struct ngtcp2_conn {
    _private: [u8; 0],
}

/// Opaque ngtcp2_crypto_conn_ref type
#[repr(C)]
pub struct ngtcp2_crypto_conn_ref {
    _private: [u8; 0],
}

// ============================================================================
// Data Structures
// ============================================================================

/// Version info structure
#[repr(C)]
pub struct ngtcp2_info {
    /// Age of this struct
    pub age: c_int,
    /// Library version number
    pub version_num: u32,
    /// Library version string
    pub version_str: *const c_char,
}

/// Connection ID structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ngtcp2_cid {
    /// Connection ID data
    pub data: [u8; NGTCP2_MAX_CIDLEN],
    /// Length of the connection ID
    pub datalen: size_t,
}

impl Default for ngtcp2_cid {
    fn default() -> Self {
        Self {
            data: [0u8; NGTCP2_MAX_CIDLEN],
            datalen: 0,
        }
    }
}

impl ngtcp2_cid {
    /// Create a new CID from bytes
    pub fn new(data: &[u8]) -> Self {
        let mut cid = Self::default();
        let len = data.len().min(NGTCP2_MAX_CIDLEN);
        cid.data[..len].copy_from_slice(&data[..len]);
        cid.datalen = len;
        cid
    }

    /// Generate a random CID
    pub fn random(len: usize) -> Self {
        let mut cid = Self::default();
        let len = len.min(NGTCP2_MAX_CIDLEN);
        // Simple random generation using timestamp and memory address
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0) as u64;
        for i in 0..len {
            cid.data[i] = ((seed >> (i * 8)) & 0xFF) as u8 ^ (i as u8 * 37);
        }
        cid.datalen = len;
        cid
    }
}

/// Socket address (generic for IPv4/IPv6)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ngtcp2_sockaddr {
    /// Address family
    pub sa_family: u16,
    /// Address data
    pub sa_data: [u8; 126],
}

impl Default for ngtcp2_sockaddr {
    fn default() -> Self {
        Self {
            sa_family: 0,
            sa_data: [0u8; 126],
        }
    }
}

/// IPv4 socket address
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ngtcp2_sockaddr_in {
    /// AF_INET = 2
    pub sin_family: u16,
    /// Port (network byte order)
    pub sin_port: u16,
    /// IPv4 address
    pub sin_addr: [u8; 4],
    /// Padding
    pub sin_zero: [u8; 8],
}

impl ngtcp2_sockaddr_in {
    /// Create from IP and port
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        Self {
            sin_family: AF_INET,
            sin_port: port.to_be(),
            sin_addr: ip,
            sin_zero: [0; 8],
        }
    }
}

/// IPv6 socket address
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ngtcp2_sockaddr_in6 {
    /// AF_INET6 = 10
    pub sin6_family: u16,
    /// Port (network byte order)
    pub sin6_port: u16,
    /// Flow info
    pub sin6_flowinfo: u32,
    /// IPv6 address
    pub sin6_addr: [u8; 16],
    /// Scope ID
    pub sin6_scope_id: u32,
}

impl ngtcp2_sockaddr_in6 {
    /// Create from IP and port
    pub fn new(ip: [u8; 16], port: u16) -> Self {
        Self {
            sin6_family: AF_INET6,
            sin6_port: port.to_be(),
            sin6_flowinfo: 0,
            sin6_addr: ip,
            sin6_scope_id: 0,
        }
    }
}

/// Address storage union
#[repr(C)]
#[derive(Clone, Copy)]
pub union ngtcp2_sockaddr_union {
    pub sa: ngtcp2_sockaddr_in,
    pub sa6: ngtcp2_sockaddr_in6,
}

/// Address wrapper with length
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ngtcp2_addr {
    /// Address pointer
    pub addr: *mut ngtcp2_sockaddr,
    /// Address length
    pub addrlen: u32,
}

impl ngtcp2_addr {
    /// Create from socket address
    pub fn new(addr: *mut ngtcp2_sockaddr, addrlen: u32) -> Self {
        Self { addr, addrlen }
    }
}

/// Path (local + remote address)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ngtcp2_path {
    /// Local address
    pub local: ngtcp2_addr,
    /// Remote address
    pub remote: ngtcp2_addr,
    /// User data
    pub user_data: *mut c_void,
}

/// Path storage (with embedded addresses)
#[repr(C)]
pub struct ngtcp2_path_storage {
    /// Path
    pub path: ngtcp2_path,
    /// Local address storage
    pub local_addrbuf: [u8; 128],
    /// Remote address storage
    pub remote_addrbuf: [u8; 128],
}

/// QUIC settings
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ngtcp2_settings {
    /// QLOG callback
    pub qlog_write: Option<extern "C" fn(*mut c_void, u32, *const c_void, size_t)>,
    /// CC algorithm
    pub cc_algo: u32,
    /// Initial RTT (nanoseconds)
    pub initial_rtt: u64,
    /// Log printf callback
    pub log_printf: Option<extern "C" fn(*mut c_void, *const c_char, ...)>,
    /// Maximum window size
    pub max_window: u64,
    /// Maximum stream window
    pub max_stream_window: u64,
    /// ACK threshold
    pub ack_thresh: size_t,
    /// No PMTUD
    pub no_pmtud: u8,
    /// Initial packet number
    pub initial_pkt_num: u64,
    /// Maximum TX UDP payload size
    pub max_tx_udp_payload_size: size_t,
    /// Handshake timeout (nanoseconds)
    pub handshake_timeout: u64,
    /// Preferred versions
    pub preferred_versions: *const u32,
    /// Number of preferred versions
    pub preferred_versionslen: size_t,
    /// Available versions
    pub available_versions: *const u32,
    /// Number of available versions
    pub available_versionslen: size_t,
    /// Original version
    pub original_version: u32,
    /// No UDP ECN
    pub no_udp_payload_size_shaping: u8,
    /// Token
    pub token: *const u8,
    /// Token length
    pub tokenlen: size_t,
    /// Initial timestamp
    pub initial_ts: u64,
    /// Retry token
    pub retry_token: *const u8,
    /// Retry token length
    pub retry_tokenlen: size_t,
    /// RAND context
    pub rand_ctx: *mut c_void,
}

impl Default for ngtcp2_settings {
    fn default() -> Self {
        Self {
            qlog_write: None,
            cc_algo: NGTCP2_CC_ALGO_CUBIC,
            initial_rtt: 333_000_000, // 333ms
            log_printf: None,
            max_window: 6 * 1024 * 1024,
            max_stream_window: 6 * 1024 * 1024,
            ack_thresh: 2,
            no_pmtud: 0,
            initial_pkt_num: 0,
            max_tx_udp_payload_size: 1200,
            handshake_timeout: 0,
            preferred_versions: std::ptr::null(),
            preferred_versionslen: 0,
            available_versions: std::ptr::null(),
            available_versionslen: 0,
            original_version: 0,
            no_udp_payload_size_shaping: 0,
            token: std::ptr::null(),
            tokenlen: 0,
            initial_ts: 0,
            retry_token: std::ptr::null(),
            retry_tokenlen: 0,
            rand_ctx: std::ptr::null_mut(),
        }
    }
}

/// Transport parameters
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ngtcp2_transport_params {
    /// Original destination CID
    pub original_dcid: ngtcp2_cid,
    /// Initial source CID
    pub initial_scid: ngtcp2_cid,
    /// Retry source CID
    pub retry_scid: ngtcp2_cid,
    /// Preferred address present
    pub preferred_addr_present: u8,
    /// Max idle timeout (ms)
    pub max_idle_timeout: u64,
    /// Max UDP payload size
    pub max_udp_payload_size: u64,
    /// Initial max data
    pub initial_max_data: u64,
    /// Initial max stream data (bidi local)
    pub initial_max_stream_data_bidi_local: u64,
    /// Initial max stream data (bidi remote)
    pub initial_max_stream_data_bidi_remote: u64,
    /// Initial max stream data (uni)
    pub initial_max_stream_data_uni: u64,
    /// Initial max streams (bidi)
    pub initial_max_streams_bidi: u64,
    /// Initial max streams (uni)
    pub initial_max_streams_uni: u64,
    /// ACK delay exponent
    pub ack_delay_exponent: u64,
    /// Max ACK delay (ms)
    pub max_ack_delay: u64,
    /// Disable active migration
    pub disable_active_migration: u8,
    /// Active CID limit
    pub active_connection_id_limit: u64,
    /// Stateless reset token
    pub stateless_reset_token: [u8; NGTCP2_STATELESS_RESET_TOKENLEN],
    /// Stateless reset token present
    pub stateless_reset_token_present: u8,
    /// Max datagram frame size
    pub max_datagram_frame_size: u64,
    /// Grease QUIC bit
    pub grease_quic_bit: u8,
    /// Version info
    pub version_info_present: u8,
    /// Version info
    pub version_info: ngtcp2_version_info,
}

impl Default for ngtcp2_transport_params {
    fn default() -> Self {
        Self {
            original_dcid: ngtcp2_cid::default(),
            initial_scid: ngtcp2_cid::default(),
            retry_scid: ngtcp2_cid::default(),
            preferred_addr_present: 0,
            max_idle_timeout: 30_000,
            max_udp_payload_size: 65527,
            initial_max_data: 10 * 1024 * 1024,
            initial_max_stream_data_bidi_local: 256 * 1024,
            initial_max_stream_data_bidi_remote: 256 * 1024,
            initial_max_stream_data_uni: 256 * 1024,
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
            ack_delay_exponent: 3,
            max_ack_delay: 25,
            disable_active_migration: 0,
            active_connection_id_limit: 8,
            stateless_reset_token: [0u8; NGTCP2_STATELESS_RESET_TOKENLEN],
            stateless_reset_token_present: 0,
            max_datagram_frame_size: 0,
            grease_quic_bit: 1,
            version_info_present: 0,
            version_info: ngtcp2_version_info::default(),
        }
    }
}

/// Version info
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct ngtcp2_version_info {
    /// Chosen version
    pub chosen_version: u32,
    /// Available versions
    pub available_versions: *const u8,
    /// Available versions length
    pub available_versionslen: size_t,
}

/// Packet info
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct ngtcp2_pkt_info {
    /// ECN
    pub ecn: u32,
}

/// Vector (iovec-like)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ngtcp2_vec {
    /// Base pointer
    pub base: *const u8,
    /// Length
    pub len: size_t,
}

impl ngtcp2_vec {
    /// Create from slice
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            base: data.as_ptr(),
            len: data.len(),
        }
    }
}

// ============================================================================
// Constants
// ============================================================================

/// Maximum CID length
pub const NGTCP2_MAX_CIDLEN: usize = 20;

/// Minimum CID length
pub const NGTCP2_MIN_CIDLEN: usize = 1;

/// Stateless reset token length
pub const NGTCP2_STATELESS_RESET_TOKENLEN: usize = 16;

/// QUIC version 1 (RFC 9000)
pub const NGTCP2_PROTO_VER_V1: u32 = 0x00000001;

/// QUIC version 2 (RFC 9369)
pub const NGTCP2_PROTO_VER_V2: u32 = 0x6b3343cf;

/// Address family: IPv4
pub const AF_INET: u16 = 2;

/// Address family: IPv6
pub const AF_INET6: u16 = 10;

/// CC algorithm: Reno
pub const NGTCP2_CC_ALGO_RENO: u32 = 0;

/// CC algorithm: Cubic
pub const NGTCP2_CC_ALGO_CUBIC: u32 = 1;

/// CC algorithm: BBR
pub const NGTCP2_CC_ALGO_BBR: u32 = 2;

// ============================================================================
// Error Codes
// ============================================================================

/// No error
pub const NGTCP2_NO_ERROR: c_int = 0;

/// Invalid argument
pub const NGTCP2_ERR_INVALID_ARGUMENT: c_int = -101;

/// Buffer too small
pub const NGTCP2_ERR_NOBUF: c_int = -102;

/// Protocol error
pub const NGTCP2_ERR_PROTO: c_int = -103;

/// Internal error
pub const NGTCP2_ERR_INTERNAL: c_int = -104;

/// Callback failure
pub const NGTCP2_ERR_CALLBACK_FAILURE: c_int = -105;

/// Fatal error
pub const NGTCP2_ERR_FATAL: c_int = -106;

/// Crypto error
pub const NGTCP2_ERR_CRYPTO: c_int = -107;

/// Stream not found
pub const NGTCP2_ERR_STREAM_NOT_FOUND: c_int = -108;

/// Stream closed
pub const NGTCP2_ERR_STREAM_SHUT_WR: c_int = -109;

/// Stream ID blocked
pub const NGTCP2_ERR_STREAM_ID_BLOCKED: c_int = -110;

/// Flow control error
pub const NGTCP2_ERR_FLOW_CONTROL: c_int = -111;

/// Connection closing
pub const NGTCP2_ERR_CLOSING: c_int = -112;

/// Connection draining
pub const NGTCP2_ERR_DRAINING: c_int = -113;

/// Idle timeout
pub const NGTCP2_ERR_IDLE_CLOSE: c_int = -114;

/// Required transport param missing
pub const NGTCP2_ERR_REQUIRED_TRANSPORT_PARAM: c_int = -115;

/// Malformed transport param
pub const NGTCP2_ERR_MALFORMED_TRANSPORT_PARAM: c_int = -116;

/// Version negotiation
pub const NGTCP2_ERR_RECV_VERSION_NEGOTIATION: c_int = -117;

/// Retry
pub const NGTCP2_ERR_RETRY: c_int = -118;

/// Drop connection
pub const NGTCP2_ERR_DROP_CONN: c_int = -119;

// ============================================================================
// External Functions (linked from libngtcp2.so)
// ============================================================================

#[link(name = "ngtcp2")]
extern "C" {
    // ========================================================================
    // Version and Info Functions
    // ========================================================================

    /// Get library version info
    pub fn ngtcp2_version(least_version: c_int) -> *const ngtcp2_info;

    /// Convert error code to string
    pub fn ngtcp2_strerror(error_code: c_int) -> *const c_char;

    /// Check if error is fatal
    pub fn ngtcp2_err_is_fatal(error_code: c_int) -> c_int;

    // ========================================================================
    // Settings Functions
    // ========================================================================

    /// Initialize settings with default values
    pub fn ngtcp2_settings_default(settings: *mut ngtcp2_settings);

    /// Initialize transport params with default values
    pub fn ngtcp2_transport_params_default(params: *mut ngtcp2_transport_params);

    // ========================================================================
    // Path Storage Functions
    // ========================================================================

    /// Initialize path storage
    pub fn ngtcp2_path_storage_init(
        ps: *mut ngtcp2_path_storage,
        local_addr: *const ngtcp2_sockaddr,
        local_addrlen: u32,
        remote_addr: *const ngtcp2_sockaddr,
        remote_addrlen: u32,
        user_data: *mut c_void,
    );

    /// Zero-initialize path storage
    pub fn ngtcp2_path_storage_zero(ps: *mut ngtcp2_path_storage);

    // ========================================================================
    // Connection Functions
    // ========================================================================

    /// Create a new client connection
    pub fn ngtcp2_conn_client_new(
        pconn: *mut *mut ngtcp2_conn,
        dcid: *const ngtcp2_cid,
        scid: *const ngtcp2_cid,
        path: *const ngtcp2_path,
        version: u32,
        callbacks: *const ngtcp2_callbacks,
        settings: *const ngtcp2_settings,
        params: *const ngtcp2_transport_params,
        mem: *const c_void,
        user_data: *mut c_void,
    ) -> c_int;

    /// Delete a connection
    pub fn ngtcp2_conn_del(conn: *mut ngtcp2_conn);

    /// Check if handshake is complete
    pub fn ngtcp2_conn_get_handshake_completed(conn: *mut ngtcp2_conn) -> c_int;

    /// Check if connection is in closing period
    pub fn ngtcp2_conn_in_closing_period(conn: *const ngtcp2_conn) -> c_int;

    /// Check if connection is in draining period
    pub fn ngtcp2_conn_in_draining_period(conn: *const ngtcp2_conn) -> c_int;

    /// Open a bidirectional stream
    pub fn ngtcp2_conn_open_bidi_stream(
        conn: *mut ngtcp2_conn,
        pstream_id: *mut i64,
        stream_user_data: *mut c_void,
    ) -> c_int;

    /// Open a unidirectional stream
    pub fn ngtcp2_conn_open_uni_stream(
        conn: *mut ngtcp2_conn,
        pstream_id: *mut i64,
        stream_user_data: *mut c_void,
    ) -> c_int;

    /// Extend max stream offset
    pub fn ngtcp2_conn_extend_max_stream_offset(
        conn: *mut ngtcp2_conn,
        stream_id: i64,
        datalen: u64,
    ) -> c_int;

    /// Extend max connection offset
    pub fn ngtcp2_conn_extend_max_offset(conn: *mut ngtcp2_conn, datalen: u64) -> c_int;

    /// Write stream data
    pub fn ngtcp2_conn_writev_stream(
        conn: *mut ngtcp2_conn,
        path: *mut ngtcp2_path,
        pi: *mut ngtcp2_pkt_info,
        dest: *mut u8,
        destlen: size_t,
        pdatalen: *mut ssize_t,
        flags: u32,
        stream_id: i64,
        datav: *const ngtcp2_vec,
        datavcnt: size_t,
        ts: u64,
    ) -> ssize_t;

    /// Read received packet
    pub fn ngtcp2_conn_read_pkt(
        conn: *mut ngtcp2_conn,
        path: *const ngtcp2_path,
        pi: *const ngtcp2_pkt_info,
        pkt: *const u8,
        pktlen: size_t,
        ts: u64,
    ) -> c_int;

    /// Shutdown stream write
    pub fn ngtcp2_conn_shutdown_stream_write(
        conn: *mut ngtcp2_conn,
        flags: u32,
        stream_id: i64,
        app_error_code: u64,
    ) -> c_int;

    /// Shutdown stream read
    pub fn ngtcp2_conn_shutdown_stream_read(
        conn: *mut ngtcp2_conn,
        flags: u32,
        stream_id: i64,
        app_error_code: u64,
    ) -> c_int;

    /// Get next expiry timestamp
    pub fn ngtcp2_conn_get_expiry(conn: *mut ngtcp2_conn) -> u64;

    /// Handle timeout
    pub fn ngtcp2_conn_handle_expiry(conn: *mut ngtcp2_conn, ts: u64) -> c_int;

    /// Write connection close
    pub fn ngtcp2_conn_write_connection_close(
        conn: *mut ngtcp2_conn,
        path: *mut ngtcp2_path,
        pi: *mut ngtcp2_pkt_info,
        dest: *mut u8,
        destlen: size_t,
        error_code: u64,
        reason: *const u8,
        reasonlen: size_t,
        ts: u64,
    ) -> ssize_t;

    /// Get connection error
    pub fn ngtcp2_conn_get_ccerr(conn: *mut ngtcp2_conn) -> *const ngtcp2_ccerr;

    /// Get local transport params
    pub fn ngtcp2_conn_get_local_transport_params(
        conn: *mut ngtcp2_conn,
        params: *mut ngtcp2_transport_params,
    );

    /// Get remote transport params
    pub fn ngtcp2_conn_get_remote_transport_params(
        conn: *mut ngtcp2_conn,
        params: *mut ngtcp2_transport_params,
    );

    /// Set stream user data
    pub fn ngtcp2_conn_set_stream_user_data(
        conn: *mut ngtcp2_conn,
        stream_id: i64,
        stream_user_data: *mut c_void,
    ) -> c_int;
}

/// Connection close error
#[repr(C)]
pub struct ngtcp2_ccerr {
    /// Error type
    pub error_type: u32,
    /// Error code
    pub error_code: u64,
    /// Reason phrase
    pub reason: *const u8,
    /// Reason length
    pub reasonlen: size_t,
}

/// Callbacks structure (opaque, created by ntcp2)
#[repr(C)]
pub struct ngtcp2_callbacks {
    _private: [u8; 0],
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if an operation succeeded
#[inline]
pub fn ngtcp2_is_ok(rv: c_int) -> bool {
    rv >= 0
}

/// Check if error is fatal
#[inline]
pub fn ngtcp2_is_fatal(rv: c_int) -> bool {
    unsafe { ngtcp2_err_is_fatal(rv) != 0 }
}

/// Get version string safely
pub fn quic_get_version_string() -> Option<&'static str> {
    let info = unsafe { ngtcp2_version(0) };
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
pub fn quic_get_error_string(error_code: c_int) -> String {
    let ptr = unsafe { ngtcp2_strerror(error_code) };
    if ptr.is_null() {
        return format!("Unknown QUIC error ({})", error_code);
    }
    unsafe {
        std::ffi::CStr::from_ptr(ptr)
            .to_string_lossy()
            .into_owned()
    }
}

/// Get current timestamp in nanoseconds
pub fn get_timestamp_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
