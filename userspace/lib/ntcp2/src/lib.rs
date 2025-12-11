//! NexaOS QUIC Library (ntcp2)
//!
//! A modern, ngtcp2 ABI-compatible QUIC library for NexaOS with tokio async backend.
//!
//! ## Features
//! - **Full QUIC protocol support** (RFC 9000, RFC 9001, RFC 9002)
//! - **ngtcp2 C ABI compatibility** for drop-in replacement
//! - **Tokio async backend** for high-performance I/O
//! - **QPACK header compression** (RFC 9204)
//! - **Connection migration support**
//! - **0-RTT early data**
//! - **Multipath QUIC**
//! - **Datagram support** (RFC 9221)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Application Layer                        │
//! │  (ngtcp2-compatible C API or Native Rust API)              │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Connection Layer                         │
//! │  - Stream management                                        │
//! │  - Flow control                                             │
//! │  - Congestion control                                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Packet Layer                             │
//! │  - Packet serialization/deserialization                    │
//! │  - QPACK encoding/decoding                                 │
//! │  - Loss detection                                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Crypto Layer                             │
//! │  - TLS 1.3 handshake (via nssl)                            │
//! │  - AEAD encryption                                         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage (Rust API)
//!
//! ```rust,ignore
//! use ntcp2::{Connection, Config, StreamId};
//!
//! // Create configuration
//! let config = Config::new()?;
//! config.set_initial_max_data(10_000_000);
//!
//! // Create a client connection
//! let conn = Connection::client(&config)?;
//!
//! // Open a bidirectional stream
//! let stream_id = conn.open_stream()?;
//!
//! // Write data
//! conn.stream_write(stream_id, b"Hello, QUIC!", true)?;
//! ```
//!
//! ## Usage (C API - ngtcp2 compatible)
//!
//! ```c
//! #include <ngtcp2/ngtcp2.h>
//!
//! ngtcp2_conn *conn;
//! ngtcp2_settings settings;
//! ngtcp2_transport_params params;
//!
//! ngtcp2_settings_default(&settings);
//! ngtcp2_transport_params_default(&params);
//!
//! ngtcp2_conn_client_new(&conn, &dcid, &scid, &path,
//!                         NGTCP2_PROTO_VER_V1, &callbacks,
//!                         &settings, &params, NULL, user_data);
//! ```

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

// ============================================================================
// Module Declarations
// ============================================================================

// Core types and constants
pub mod constants;
pub mod error;
pub mod types;

// FFI bindings to nssl (libssl.so) and ncryptolib (libcrypto.so)
// Provides OpenSSL-compatible C ABI for TLS/crypto operations
pub mod ssl_ffi;

// Packet layer
pub mod crypto;
pub mod frame;
pub mod packet;

// Connection layer
pub mod connection;
pub mod stream;
pub mod flow_control;
pub mod congestion;
pub mod recovery;

// Header compression (QPACK - RFC 9204)
pub mod qpack;

// Async I/O backend (tokio-based)
pub mod async_io;

// ngtcp2 C ABI compatibility layer
pub mod compat;

// Path and address validation
pub mod path;

// ============================================================================
// Re-exports for convenience
// ============================================================================

pub use constants::*;
pub use crypto::CryptoContext;
pub use error::{CryptoError, Error, NgError, Result, TransportError};
pub use frame::{Frame, FrameType};
pub use packet::{PacketBuilder, PacketHeader, PacketType};
pub use types::*;

// Connection layer re-exports
pub use connection::{Connection, ConnectionCallbacks, ConnectionState};
pub use stream::{Stream, StreamManager, StreamSendState, StreamRecvState};
pub use flow_control::{ConnectionFlowController, StreamFlowController};
pub use congestion::{CongestionController, CongestionAlgorithm};
pub use recovery::{LossDetector, RttEstimator, SentPacket};

// QPACK re-exports
pub use qpack::{Encoder as QpackEncoder, Decoder as QpackDecoder, DynamicTable};

// Path validation re-exports
pub use path::{Path, PathId, PathManager, PathState, MtuDiscovery, AddressValidator};

// ============================================================================
// C Type Definitions (ngtcp2 compatible)
// ============================================================================

pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i64;
pub type c_ulong = u64;
pub type c_char = i8;
pub type c_uchar = u8;
pub type c_void = core::ffi::c_void;
pub type size_t = usize;
pub type ssize_t = isize;

// ============================================================================
// Version Constants
// ============================================================================

/// Library version string
pub const NTCP2_VERSION: &str = "1.0.0";
pub const NTCP2_VERSION_CSTR: &[u8] = b"1.0.0\0";

/// Library version number (0xMMmmpp format)
pub const NTCP2_VERSION_NUM: u32 = 0x010000; // 1.0.0

/// QUIC version 1 (RFC 9000)
pub const NGTCP2_PROTO_VER_V1: u32 = 0x00000001;

/// QUIC version 2 (RFC 9369)
pub const NGTCP2_PROTO_VER_V2: u32 = 0x6b3343cf;

/// Maximum QUIC packet size (UDP payload)
pub const NGTCP2_MAX_UDP_PAYLOAD_SIZE: usize = 65527;

/// Default max UDP payload size
pub const NGTCP2_DEFAULT_MAX_UDP_PAYLOAD_SIZE: usize = 1200;

/// Maximum CID length
pub const NGTCP2_MAX_CIDLEN: usize = 20;

/// Minimum CID length
pub const NGTCP2_MIN_CIDLEN: usize = 1;

/// Stateless reset token length
pub const NGTCP2_STATELESS_RESET_TOKENLEN: usize = 16;

// NOTE: ngtcp2_err_is_fatal and ngtcp2_version are now provided by compat.rs module

// ============================================================================
// Connection ID
// ============================================================================

/// Connection ID structure
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionId {
    /// Connection ID data
    pub data: [u8; NGTCP2_MAX_CIDLEN],
    /// Length of the connection ID
    pub datalen: usize,
}

impl ConnectionId {
    /// Create a new connection ID
    pub fn new(data: &[u8]) -> Self {
        let mut cid = Self {
            data: [0u8; NGTCP2_MAX_CIDLEN],
            datalen: data.len().min(NGTCP2_MAX_CIDLEN),
        };
        cid.data[..cid.datalen].copy_from_slice(&data[..cid.datalen]);
        cid
    }

    /// Create an empty connection ID
    pub fn empty() -> Self {
        Self {
            data: [0u8; NGTCP2_MAX_CIDLEN],
            datalen: 0,
        }
    }

    /// Get the connection ID as a slice
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.datalen]
    }

    /// Check if the connection ID is empty
    pub fn is_empty(&self) -> bool {
        self.datalen == 0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::empty()
    }
}

/// C-compatible connection ID type alias
pub type ngtcp2_cid = ConnectionId;

// ============================================================================
// Timestamp
// ============================================================================

/// Timestamp type (nanoseconds since epoch)
pub type Timestamp = u64;
pub type ngtcp2_tstamp = Timestamp;

/// Duration type (nanoseconds)
pub type Duration = u64;
pub type ngtcp2_duration = Duration;

/// Invalid timestamp constant
pub const NGTCP2_TSTAMP_MAX: Timestamp = u64::MAX;

// ============================================================================
// Unit Conversion Helpers
// ============================================================================

/// Convert seconds to nanoseconds
#[inline]
pub const fn seconds_to_nanos(secs: u64) -> Duration {
    secs * 1_000_000_000
}

/// Convert milliseconds to nanoseconds
#[inline]
pub const fn millis_to_nanos(ms: u64) -> Duration {
    ms * 1_000_000
}

/// Convert microseconds to nanoseconds
#[inline]
pub const fn micros_to_nanos(us: u64) -> Duration {
    us * 1_000
}

/// Convert nanoseconds to seconds
#[inline]
pub const fn nanos_to_seconds(nanos: Duration) -> u64 {
    nanos / 1_000_000_000
}

/// Convert nanoseconds to milliseconds
#[inline]
pub const fn nanos_to_millis(nanos: Duration) -> u64 {
    nanos / 1_000_000
}

// NOTE: ngtcp2_strerror is now provided by compat.rs module

// ============================================================================
// Settings and Transport Parameters (ngtcp2 compatible)
// ============================================================================

/// ngtcp2 settings structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ngtcp2_settings {
    /// QLOG callback (optional)
    pub qlog_write: Option<extern "C" fn(*mut c_void, u32, *const c_void, usize)>,
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
    pub ack_thresh: usize,
    /// No PMTUD
    pub no_pmtud: u8,
    /// Initial packet number
    pub initial_pkt_num: u64,
}

impl Default for ngtcp2_settings {
    fn default() -> Self {
        Self {
            qlog_write: None,
            cc_algo: 0,               // Cubic
            initial_rtt: 333_000_000, // 333ms
            log_printf: None,
            max_window: 6 * 1024 * 1024,        // 6MB
            max_stream_window: 6 * 1024 * 1024, // 6MB
            ack_thresh: 2,
            no_pmtud: 0,
            initial_pkt_num: 0,
        }
    }
}

// NOTE: ngtcp2_settings_default is now provided by compat.rs module

/// ngtcp2 transport parameters structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ngtcp2_transport_params {
    /// Original destination CID
    pub original_dcid: ngtcp2_cid,
    /// Initial source CID
    pub initial_scid: ngtcp2_cid,
    /// Retry source CID (server only)
    pub retry_scid: ngtcp2_cid,
    /// Preferred address (optional)
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
}

impl Default for ngtcp2_transport_params {
    fn default() -> Self {
        Self {
            original_dcid: ngtcp2_cid::empty(),
            initial_scid: ngtcp2_cid::empty(),
            retry_scid: ngtcp2_cid::empty(),
            preferred_addr_present: 0,
            max_idle_timeout: 30_000, // 30 seconds
            max_udp_payload_size: NGTCP2_DEFAULT_MAX_UDP_PAYLOAD_SIZE as u64,
            initial_max_data: 10 * 1024 * 1024,             // 10MB
            initial_max_stream_data_bidi_local: 256 * 1024, // 256KB
            initial_max_stream_data_bidi_remote: 256 * 1024,
            initial_max_stream_data_uni: 256 * 1024,
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
            ack_delay_exponent: 3,
            max_ack_delay: 25, // 25ms
            disable_active_migration: 0,
            active_connection_id_limit: 2,
            stateless_reset_token: [0u8; NGTCP2_STATELESS_RESET_TOKENLEN],
            stateless_reset_token_present: 0,
            max_datagram_frame_size: 0,
            grease_quic_bit: 1,
        }
    }
}

// NOTE: ngtcp2_transport_params_default is now provided by compat.rs module

// NOTE: Full connection C API is now provided by compat.rs module.
// Type re-export for backward compatibility
pub use compat::ngtcp2_conn;

// ============================================================================
// Simple test/example
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id() {
        let cid = ConnectionId::new(b"test_cid");
        assert_eq!(cid.as_slice(), b"test_cid");
        assert!(!cid.is_empty());
    }

    #[test]
    fn test_version() {
        let info = unsafe { &*ngtcp2_version(0) };
        assert_eq!(info.version_num, NTCP2_VERSION_NUM as c_int);
    }

    #[test]
    fn test_settings_default() {
        let mut settings = ngtcp2_settings::default();
        ngtcp2_settings_default(&mut settings);
        assert_eq!(settings.cc_algo, 0);
    }
}
