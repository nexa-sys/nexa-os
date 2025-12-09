//! Core type definitions for ntcp2
//!
//! This module defines the fundamental types used throughout the library,
//! maintaining compatibility with ngtcp2's C API types.

use crate::{c_int, c_uint, c_void, size_t, ConnectionId, Timestamp, Duration};

// ============================================================================
// Stream ID Type
// ============================================================================

/// Stream identifier type (62-bit)
/// 
/// Stream IDs encode both the initiator and directionality:
/// - Bit 0: Initiator (0 = client, 1 = server)
/// - Bit 1: Directionality (0 = bidirectional, 1 = unidirectional)
pub type StreamId = i64;

/// Maximum valid stream ID (2^62 - 1)
pub const MAX_STREAM_ID: StreamId = (1i64 << 62) - 1;

/// Check if stream is client-initiated
#[inline]
pub fn stream_is_client(stream_id: StreamId) -> bool {
    (stream_id & 0x01) == 0
}

/// Check if stream is server-initiated
#[inline]
pub fn stream_is_server(stream_id: StreamId) -> bool {
    (stream_id & 0x01) == 1
}

/// Check if stream is bidirectional
#[inline]
pub fn stream_is_bidi(stream_id: StreamId) -> bool {
    (stream_id & 0x02) == 0
}

/// Check if stream is unidirectional
#[inline]
pub fn stream_is_uni(stream_id: StreamId) -> bool {
    (stream_id & 0x02) == 2
}

// ============================================================================
// Stream Type
// ============================================================================

/// Stream type enumeration
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// Client-initiated bidirectional stream
    ClientBidi = 0x00,
    /// Server-initiated bidirectional stream
    ServerBidi = 0x01,
    /// Client-initiated unidirectional stream
    ClientUni = 0x02,
    /// Server-initiated unidirectional stream
    ServerUni = 0x03,
}

impl StreamType {
    /// Get the first stream ID for this type
    pub fn first_stream_id(self) -> StreamId {
        self as StreamId
    }
}

// ============================================================================
// Transport Parameters
// ============================================================================

/// Transport parameters (RFC 9000 Section 18.2)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct TransportParams {
    /// Preferred address for migration
    pub preferred_address_present: bool,
    /// Original destination connection ID
    pub original_dcid: ConnectionId,
    /// Initial source connection ID
    pub initial_scid: ConnectionId,
    /// Retry source connection ID
    pub retry_scid: ConnectionId,
    /// Retry source CID present
    pub retry_scid_present: bool,
    /// Original destination CID present
    pub original_dcid_present: bool,
    /// Initial maximum data
    pub initial_max_data: u64,
    /// Initial maximum data for local bidirectional streams
    pub initial_max_stream_data_bidi_local: u64,
    /// Initial maximum data for remote bidirectional streams
    pub initial_max_stream_data_bidi_remote: u64,
    /// Initial maximum data for unidirectional streams
    pub initial_max_stream_data_uni: u64,
    /// Initial maximum bidirectional streams
    pub initial_max_streams_bidi: u64,
    /// Initial maximum unidirectional streams
    pub initial_max_streams_uni: u64,
    /// Maximum idle timeout (milliseconds)
    pub max_idle_timeout: Duration,
    /// Maximum UDP payload size
    pub max_udp_payload_size: u64,
    /// Active connection ID limit
    pub active_connection_id_limit: u64,
    /// ACK delay exponent
    pub ack_delay_exponent: u64,
    /// Maximum ACK delay
    pub max_ack_delay: Duration,
    /// Disable active migration
    pub disable_active_migration: bool,
    /// Stateless reset token
    pub stateless_reset_token: [u8; 16],
    /// Stateless reset token present
    pub stateless_reset_token_present: bool,
    /// Maximum datagram frame size (RFC 9221)
    pub max_datagram_frame_size: u64,
    /// Grease QUIC bit (RFC 9287)
    pub grease_quic_bit: bool,
    /// Version information
    pub version_information_present: bool,
}

impl Default for TransportParams {
    fn default() -> Self {
        Self {
            preferred_address_present: false,
            original_dcid: ConnectionId::empty(),
            initial_scid: ConnectionId::empty(),
            retry_scid: ConnectionId::empty(),
            retry_scid_present: false,
            original_dcid_present: false,
            initial_max_data: 0,
            initial_max_stream_data_bidi_local: 0,
            initial_max_stream_data_bidi_remote: 0,
            initial_max_stream_data_uni: 0,
            initial_max_streams_bidi: 0,
            initial_max_streams_uni: 0,
            max_idle_timeout: 0,
            max_udp_payload_size: 65527,
            active_connection_id_limit: 2,
            ack_delay_exponent: 3,
            max_ack_delay: 25 * 1_000_000, // 25ms in nanoseconds
            disable_active_migration: false,
            stateless_reset_token: [0u8; 16],
            stateless_reset_token_present: false,
            max_datagram_frame_size: 0,
            grease_quic_bit: false,
            version_information_present: false,
        }
    }
}

/// ngtcp2 compatible alias
pub type ngtcp2_transport_params = TransportParams;

// ============================================================================
// Settings
// ============================================================================

/// Connection settings
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Settings {
    /// Transport parameters
    pub transport_params: TransportParams,
    /// Initial RTT estimate (nanoseconds)
    pub initial_rtt: Duration,
    /// CC algorithm
    pub cc_algo: CongestionControlAlgorithm,
    /// Log callback
    pub log_printf: Option<LogCallback>,
    /// Maximum window size
    pub max_window: u64,
    /// Maximum stream window size
    pub max_stream_window: u64,
    /// ACK threshold
    pub ack_thresh: usize,
    /// Disable path MTU discovery
    pub no_pmtud: bool,
    /// QUIC version
    pub qlog: bool,
    /// Handshake timeout
    pub handshake_timeout: Duration,
    /// Initial packet number space
    pub initial_pkt_num: u64,
    /// Token
    pub token: Vec<u8>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            transport_params: TransportParams::default(),
            initial_rtt: 333 * 1_000_000, // 333ms in nanoseconds
            cc_algo: CongestionControlAlgorithm::Cubic,
            log_printf: None,
            max_window: 24 * 1024 * 1024, // 24MB
            max_stream_window: 16 * 1024 * 1024, // 16MB
            ack_thresh: 2,
            no_pmtud: false,
            qlog: false,
            handshake_timeout: 10 * 1_000_000_000, // 10 seconds
            initial_pkt_num: 0,
            token: Vec::new(),
        }
    }
}

/// ngtcp2 compatible alias
pub type ngtcp2_settings = Settings;

// ============================================================================
// Congestion Control
// ============================================================================

/// Congestion control algorithm
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionControlAlgorithm {
    /// New Reno
    NewReno = 0,
    /// Cubic
    Cubic = 1,
    /// BBR
    Bbr = 2,
    /// BBRv2
    Bbr2 = 3,
}

impl Default for CongestionControlAlgorithm {
    fn default() -> Self {
        Self::Cubic
    }
}

/// ngtcp2 compatible alias
pub type ngtcp2_cc_algo = CongestionControlAlgorithm;

// ============================================================================
// Log Callback
// ============================================================================

/// Log callback type
pub type LogCallback = extern "C" fn(
    user_data: *mut c_void,
    fmt: *const i8,
    ...
);

// ============================================================================
// Path
// ============================================================================

/// Network path information
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Path {
    /// Local address
    pub local: SocketAddr,
    /// Remote address  
    pub remote: SocketAddr,
    /// User data
    pub user_data: *mut c_void,
}

/// Socket address (simplified)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SocketAddr {
    /// Address family
    pub family: u16,
    /// Port (network byte order)
    pub port: u16,
    /// IPv4 address or IPv6 address
    pub addr: [u8; 16],
    /// Address length
    pub addrlen: usize,
}

impl Default for SocketAddr {
    fn default() -> Self {
        Self {
            family: 0,
            port: 0,
            addr: [0u8; 16],
            addrlen: 0,
        }
    }
}

impl Default for Path {
    fn default() -> Self {
        Self {
            local: SocketAddr::default(),
            remote: SocketAddr::default(),
            user_data: core::ptr::null_mut(),
        }
    }
}

/// ngtcp2 compatible alias
pub type ngtcp2_path = Path;
pub type ngtcp2_sockaddr = SocketAddr;

// ============================================================================
// Packet Information
// ============================================================================

/// Packet metadata
#[repr(C)]
#[derive(Debug, Clone)]
pub struct PacketInfo {
    /// ECN (Explicit Congestion Notification) value
    pub ecn: u32,
}

impl Default for PacketInfo {
    fn default() -> Self {
        Self { ecn: 0 }
    }
}

/// ngtcp2 compatible alias
pub type ngtcp2_pkt_info = PacketInfo;

// ============================================================================
// ECN Constants
// ============================================================================

/// ECN not-ECT
pub const NGTCP2_ECN_NOT_ECT: u32 = 0x0;
/// ECN ECT(1)
pub const NGTCP2_ECN_ECT_1: u32 = 0x1;
/// ECN ECT(0)
pub const NGTCP2_ECN_ECT_0: u32 = 0x2;
/// ECN CE (Congestion Experienced)
pub const NGTCP2_ECN_CE: u32 = 0x3;

// ============================================================================
// Connection State
// ============================================================================

/// Connection close information
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ConnectionCloseInfo {
    /// Error code
    pub error_code: u64,
    /// Frame type that caused the error (if applicable)
    pub frame_type: u64,
    /// Reason phrase
    pub reason: Vec<u8>,
}

impl Default for ConnectionCloseInfo {
    fn default() -> Self {
        Self {
            error_code: 0,
            frame_type: 0,
            reason: Vec::new(),
        }
    }
}

// ============================================================================
// Version Negotiation
// ============================================================================

/// Supported QUIC versions
pub const SUPPORTED_VERSIONS: &[u32] = &[
    0x00000001, // QUIC v1 (RFC 9000)
    0x6b3343cf, // QUIC v2 (RFC 9369)
];

/// Check if a version is supported
pub fn is_version_supported(version: u32) -> bool {
    SUPPORTED_VERSIONS.contains(&version)
}

// ============================================================================
// RTT Statistics
// ============================================================================

/// RTT statistics
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct RttStats {
    /// Latest RTT sample
    pub latest_rtt: Duration,
    /// Minimum RTT observed
    pub min_rtt: Duration,
    /// Smoothed RTT
    pub smoothed_rtt: Duration,
    /// RTT variance
    pub rttvar: Duration,
    /// Maximum ACK delay
    pub max_ack_delay: Duration,
    /// PTO count
    pub pto_count: usize,
}

/// ngtcp2 compatible alias
pub type ngtcp2_conn_stat = RttStats;

// ============================================================================
// Crypto Level
// ============================================================================

/// Encryption level (crypto level)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EncryptionLevel {
    /// Initial encryption (AEAD_AES_128_GCM with derived keys)
    Initial = 0,
    /// Handshake encryption
    Handshake = 1,
    /// 0-RTT encryption
    ZeroRtt = 2,
    /// 1-RTT encryption (application data)
    OneRtt = 3,
}

impl Default for EncryptionLevel {
    fn default() -> Self {
        Self::Initial
    }
}

/// ngtcp2 compatible alias
pub type ngtcp2_encryption_level = EncryptionLevel;

// ============================================================================
// Write Stream Result
// ============================================================================

/// Result of writing stream data
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct WriteStreamResult {
    /// Number of bytes written
    pub bytes_written: usize,
    /// Stream finished
    pub fin: bool,
}
