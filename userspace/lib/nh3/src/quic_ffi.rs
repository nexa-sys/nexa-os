//! FFI bindings to ntcp2 (libngtcp2.so)
//!
//! This module provides C ABI bindings to the QUIC layer implemented by ntcp2.
//! ntcp2 exports ngtcp2-compatible functions for QUIC protocol operations.

use crate::{c_char, c_int, c_void, size_t};

// ============================================================================
// External Functions from ntcp2 (libngtcp2.so)
// ============================================================================

#[link(name = "ngtcp2")]
extern "C" {
    // ------------------------------------------------------------------------
    // Version Info
    // ------------------------------------------------------------------------
    
    /// Get library version info
    pub fn ngtcp2_version(least_version: c_int) -> *const Ngtcp2Info;
    
    /// Check if error is fatal
    pub fn ngtcp2_err_is_fatal(error_code: c_int) -> c_int;
    
    /// Get error string
    pub fn ngtcp2_strerror(error_code: c_int) -> *const c_char;
    
    // ------------------------------------------------------------------------
    // Settings and Transport Parameters
    // ------------------------------------------------------------------------
    
    /// Initialize settings with defaults
    pub fn ngtcp2_settings_default(settings: *mut Ngtcp2Settings);
    
    /// Initialize transport parameters with defaults
    pub fn ngtcp2_transport_params_default(params: *mut Ngtcp2TransportParams);
}

// ============================================================================
// Type Definitions (from ntcp2)
// ============================================================================

/// Connection ID structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Ngtcp2Cid {
    /// Connection ID data
    pub data: [u8; 20], // NGTCP2_MAX_CIDLEN
    /// Length of the connection ID
    pub datalen: size_t,
}

impl Default for Ngtcp2Cid {
    fn default() -> Self {
        Self {
            data: [0u8; 20],
            datalen: 0,
        }
    }
}

/// Version info structure
#[repr(C)]
pub struct Ngtcp2Info {
    /// Age of this struct
    pub age: c_int,
    /// Version number
    pub version_num: c_int,
    /// Version string
    pub version_str: *const c_char,
}

/// Settings structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Ngtcp2Settings {
    /// QLOG callback (optional)
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
    /// ACK delay exponent
    pub ack_thresh: size_t,
    /// No PMTUD
    pub no_pmtud: u8,
    /// Initial packet number
    pub initial_pkt_num: u64,
}

impl Default for Ngtcp2Settings {
    fn default() -> Self {
        Self {
            qlog_write: None,
            cc_algo: 0,
            initial_rtt: 333_000_000,
            log_printf: None,
            max_window: 6 * 1024 * 1024,
            max_stream_window: 6 * 1024 * 1024,
            ack_thresh: 2,
            no_pmtud: 0,
            initial_pkt_num: 0,
        }
    }
}

/// Transport parameters structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Ngtcp2TransportParams {
    /// Original destination CID
    pub original_dcid: Ngtcp2Cid,
    /// Initial source CID
    pub initial_scid: Ngtcp2Cid,
    /// Retry source CID (server only)
    pub retry_scid: Ngtcp2Cid,
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
    pub stateless_reset_token: [u8; 16],
    /// Stateless reset token present
    pub stateless_reset_token_present: u8,
    /// Max datagram frame size
    pub max_datagram_frame_size: u64,
    /// Grease QUIC bit
    pub grease_quic_bit: u8,
}

impl Default for Ngtcp2TransportParams {
    fn default() -> Self {
        Self {
            original_dcid: Ngtcp2Cid::default(),
            initial_scid: Ngtcp2Cid::default(),
            retry_scid: Ngtcp2Cid::default(),
            preferred_addr_present: 0,
            max_idle_timeout: 30_000,
            max_udp_payload_size: 1200,
            initial_max_data: 10 * 1024 * 1024,
            initial_max_stream_data_bidi_local: 256 * 1024,
            initial_max_stream_data_bidi_remote: 256 * 1024,
            initial_max_stream_data_uni: 256 * 1024,
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
            ack_delay_exponent: 3,
            max_ack_delay: 25,
            disable_active_migration: 0,
            active_connection_id_limit: 2,
            stateless_reset_token: [0u8; 16],
            stateless_reset_token_present: 0,
            max_datagram_frame_size: 0,
            grease_quic_bit: 1,
        }
    }
}

// ============================================================================
// QUIC Constants (from ntcp2)
// ============================================================================

/// QUIC version 1 (RFC 9000)
pub const NGTCP2_PROTO_VER_V1: u32 = 0x00000001;

/// QUIC version 2 (RFC 9369)
pub const NGTCP2_PROTO_VER_V2: u32 = 0x6b3343cf;

/// Maximum CID length
pub const NGTCP2_MAX_CIDLEN: usize = 20;

/// Minimum CID length
pub const NGTCP2_MIN_CIDLEN: usize = 1;

/// Maximum UDP payload size
pub const NGTCP2_MAX_UDP_PAYLOAD_SIZE: usize = 65527;

/// Default max UDP payload size
pub const NGTCP2_DEFAULT_MAX_UDP_PAYLOAD_SIZE: usize = 1200;

// ============================================================================
// Helper Functions
// ============================================================================

/// Get QUIC library version string
pub fn get_quic_version() -> Option<&'static str> {
    unsafe {
        let info = ngtcp2_version(0);
        if info.is_null() {
            return None;
        }
        let c_str = (*info).version_str;
        if c_str.is_null() {
            return None;
        }
        // Convert C string to Rust string
        let mut len = 0;
        while *c_str.add(len) != 0 {
            len += 1;
        }
        let bytes = core::slice::from_raw_parts(c_str as *const u8, len);
        core::str::from_utf8(bytes).ok()
    }
}

/// Check if a QUIC error code is fatal
pub fn is_quic_error_fatal(error_code: i32) -> bool {
    unsafe { ngtcp2_err_is_fatal(error_code) != 0 }
}

/// Initialize default QUIC settings
pub fn init_default_settings() -> Ngtcp2Settings {
    let mut settings = Ngtcp2Settings::default();
    unsafe {
        ngtcp2_settings_default(&mut settings);
    }
    settings
}

/// Initialize default transport parameters
pub fn init_default_transport_params() -> Ngtcp2TransportParams {
    let mut params = Ngtcp2TransportParams::default();
    unsafe {
        ngtcp2_transport_params_default(&mut params);
    }
    params
}
