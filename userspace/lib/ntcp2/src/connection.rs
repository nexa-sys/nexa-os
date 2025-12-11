//! QUIC Connection Management
//!
//! This module implements the QUIC connection layer, providing:
//! - Connection state machine (RFC 9000 Section 5)
//! - Connection callbacks for integration
//! - ngtcp2-compatible C API for drop-in replacement
//!
//! ## Connection Lifecycle
//!
//! ```text
//! Client:  Initial → Handshake → Established → Closing → Closed
//! Server:  Initial → Handshake → Established → Closing → Closed
//! ```

use crate::crypto::CryptoContext;
use crate::error::{Error, NgError, Result, TransportError};
use crate::frame::{AckFrame, Frame, StreamFrame};
use crate::packet::{Packet, PacketBuilder, PacketHeader, PacketType};
use crate::stream::{Stream, StreamManager};
use crate::types::{
    CongestionControlAlgorithm, EncryptionLevel, Path, Settings, SocketAddr, StreamId,
    TransportParams,
};
use crate::{
    c_int, c_uint, c_void, ngtcp2_cid, ngtcp2_settings, ngtcp2_transport_params, size_t, ssize_t,
    ConnectionId, Duration, Timestamp, NGTCP2_MAX_CIDLEN, NGTCP2_TSTAMP_MAX,
};

use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

// ============================================================================
// Connection State
// ============================================================================

/// Connection state machine states
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state - processing Initial packets
    Initial = 0,
    /// Handshake in progress
    Handshake = 1,
    /// Connection established - can send/receive application data
    Established = 2,
    /// Connection closing initiated
    Closing = 3,
    /// Draining period (waiting before final close)
    Draining = 4,
    /// Connection closed
    Closed = 5,
}

impl Default for ConnectionState {
    fn default() -> Self {
        ConnectionState::Initial
    }
}

// ============================================================================
// Connection Role
// ============================================================================

/// Role of this endpoint in the connection
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRole {
    /// Client role (initiates connection)
    Client = 0,
    /// Server role (accepts connection)
    Server = 1,
}

// ============================================================================
// Connection Callbacks (ngtcp2 compatible)
// ============================================================================

/// Type alias for client initial callback
pub type ClientInitialCallback =
    extern "C" fn(conn: *mut Connection, user_data: *mut c_void) -> c_int;

/// Type alias for recv_client_initial callback
pub type RecvClientInitialCallback = extern "C" fn(
    conn: *mut Connection,
    dcid: *const ngtcp2_cid,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for recv_crypto_data callback
pub type RecvCryptoDataCallback = extern "C" fn(
    conn: *mut Connection,
    encryption_level: u32,
    offset: u64,
    data: *const u8,
    datalen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for handshake_completed callback
pub type HandshakeCompletedCallback =
    extern "C" fn(conn: *mut Connection, user_data: *mut c_void) -> c_int;

/// Type alias for recv_stream_data callback
pub type RecvStreamDataCallback = extern "C" fn(
    conn: *mut Connection,
    flags: u32,
    stream_id: i64,
    offset: u64,
    data: *const u8,
    datalen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for stream_open callback
pub type StreamOpenCallback =
    extern "C" fn(conn: *mut Connection, stream_id: i64, user_data: *mut c_void) -> c_int;

/// Type alias for stream_close callback
pub type StreamCloseCallback = extern "C" fn(
    conn: *mut Connection,
    flags: u32,
    stream_id: i64,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for acked_stream_data_offset callback
pub type AckedStreamDataOffsetCallback = extern "C" fn(
    conn: *mut Connection,
    stream_id: i64,
    offset: u64,
    datalen: u64,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for rand callback
pub type RandCallback =
    extern "C" fn(dest: *mut u8, destlen: size_t, rand_ctx: *mut c_void) -> c_int;

/// Type alias for get_new_connection_id callback
pub type GetNewConnectionIdCallback = extern "C" fn(
    conn: *mut Connection,
    cid: *mut ngtcp2_cid,
    token: *mut u8,
    cidlen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for remove_connection_id callback
pub type RemoveConnectionIdCallback =
    extern "C" fn(conn: *mut Connection, cid: *const ngtcp2_cid, user_data: *mut c_void) -> c_int;

/// Type alias for path_validation callback
pub type PathValidationCallback = extern "C" fn(
    conn: *mut Connection,
    flags: u32,
    path: *const Path,
    old_path: *const Path,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for extend_max_streams callback
pub type ExtendMaxStreamsCallback =
    extern "C" fn(conn: *mut Connection, max_streams: u64, user_data: *mut c_void) -> c_int;

/// Type alias for encrypt callback
pub type EncryptCallback = extern "C" fn(
    dest: *mut u8,
    nonce: *const u8,
    noncelen: size_t,
    ad: *const u8,
    adlen: size_t,
    plaintext: *const u8,
    plaintextlen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for decrypt callback
pub type DecryptCallback = extern "C" fn(
    dest: *mut u8,
    nonce: *const u8,
    noncelen: size_t,
    ad: *const u8,
    adlen: size_t,
    ciphertext: *const u8,
    ciphertextlen: size_t,
    user_data: *mut c_void,
) -> c_int;

/// Type alias for hp_mask callback
pub type HpMaskCallback = extern "C" fn(
    dest: *mut u8,
    hp_key: *const u8,
    sample: *const u8,
    user_data: *mut c_void,
) -> c_int;

/// ngtcp2-compatible connection callbacks structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ConnectionCallbacks {
    /// Called to initiate client handshake
    pub client_initial: Option<ClientInitialCallback>,
    /// Called when server receives client initial
    pub recv_client_initial: Option<RecvClientInitialCallback>,
    /// Called when crypto data is received
    pub recv_crypto_data: Option<RecvCryptoDataCallback>,
    /// Called when handshake is completed
    pub handshake_completed: Option<HandshakeCompletedCallback>,
    /// Called when stream data is received
    pub recv_stream_data: Option<RecvStreamDataCallback>,
    /// Called when a new stream is opened
    pub stream_open: Option<StreamOpenCallback>,
    /// Called when a stream is closed
    pub stream_close: Option<StreamCloseCallback>,
    /// Called when stream data is acknowledged
    pub acked_stream_data_offset: Option<AckedStreamDataOffsetCallback>,
    /// Random number generator callback
    pub rand: Option<RandCallback>,
    /// Get new connection ID callback
    pub get_new_connection_id: Option<GetNewConnectionIdCallback>,
    /// Remove connection ID callback
    pub remove_connection_id: Option<RemoveConnectionIdCallback>,
    /// Path validation callback
    pub path_validation: Option<PathValidationCallback>,
    /// Extend max streams callback (bidirectional)
    pub extend_max_streams_bidi: Option<ExtendMaxStreamsCallback>,
    /// Extend max streams callback (unidirectional)
    pub extend_max_streams_uni: Option<ExtendMaxStreamsCallback>,
    /// Encrypt callback
    pub encrypt: Option<EncryptCallback>,
    /// Decrypt callback
    pub decrypt: Option<DecryptCallback>,
    /// Header protection mask callback
    pub hp_mask: Option<HpMaskCallback>,
}

impl Default for ConnectionCallbacks {
    fn default() -> Self {
        Self {
            client_initial: None,
            recv_client_initial: None,
            recv_crypto_data: None,
            handshake_completed: None,
            recv_stream_data: None,
            stream_open: None,
            stream_close: None,
            acked_stream_data_offset: None,
            rand: None,
            get_new_connection_id: None,
            remove_connection_id: None,
            path_validation: None,
            extend_max_streams_bidi: None,
            extend_max_streams_uni: None,
            encrypt: None,
            decrypt: None,
            hp_mask: None,
        }
    }
}

/// ngtcp2-compatible callback structure alias
pub type ngtcp2_callbacks = ConnectionCallbacks;

// ============================================================================
// Pending Packet
// ============================================================================

/// Packet waiting to be sent
#[derive(Debug, Clone)]
struct PendingPacket {
    /// Packet data
    data: Vec<u8>,
    /// Packet number
    pkt_num: u64,
    /// Encryption level
    level: EncryptionLevel,
    /// Timestamp when queued
    queued_at: Timestamp,
    /// Is retransmittable
    is_ack_eliciting: bool,
}

// ============================================================================
// Connection Statistics
// ============================================================================

/// Connection statistics
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_recv: u64,
    /// Packets sent
    pub pkts_sent: u64,
    /// Packets received
    pub pkts_recv: u64,
    /// Packets retransmitted
    pub pkts_retransmitted: u64,
    /// Packets lost
    pub pkts_lost: u64,
    /// Bytes in flight
    pub bytes_in_flight: u64,
    /// Smoothed RTT (nanoseconds)
    pub smoothed_rtt: u64,
    /// RTT variance (nanoseconds)
    pub rttvar: u64,
    /// Minimum RTT (nanoseconds)
    pub min_rtt: u64,
    /// Latest RTT (nanoseconds)
    pub latest_rtt: u64,
    /// Congestion window
    pub cwnd: u64,
    /// Slow start threshold
    pub ssthresh: u64,
    /// Handshake duration (nanoseconds)
    pub handshake_duration: u64,
}

/// ngtcp2-compatible stats alias
pub type ngtcp2_conn_stat = ConnectionStats;

// ============================================================================
// Connection
// ============================================================================

/// QUIC Connection
///
/// Implements the full QUIC connection state machine with ngtcp2 compatibility.
pub struct Connection {
    /// Connection role (client/server)
    role: ConnectionRole,
    /// Connection state
    state: RwLock<ConnectionState>,
    /// Source connection ID
    scid: RwLock<ConnectionId>,
    /// Destination connection ID
    dcid: RwLock<ConnectionId>,
    /// Original destination CID (for validation)
    odcid: ConnectionId,
    /// QUIC version
    version: u32,
    /// Current path
    path: RwLock<Path>,
    /// Transport parameters (local)
    local_transport_params: RwLock<TransportParams>,
    /// Transport parameters (remote)
    remote_transport_params: RwLock<Option<TransportParams>>,
    /// Settings
    settings: Settings,
    /// Callbacks
    callbacks: ConnectionCallbacks,
    /// User data pointer
    user_data: *mut c_void,
    /// Crypto context
    crypto: Mutex<CryptoContext>,
    /// Stream manager
    streams: RwLock<StreamManager>,
    /// Pending outgoing packets
    pending_packets: Mutex<VecDeque<PendingPacket>>,
    /// Next packet number per encryption level
    next_pkt_num: [AtomicU64; 4],
    /// Largest acknowledged packet number per level
    largest_acked_pkt_num: [AtomicU64; 4],
    /// Connection statistics
    stats: RwLock<ConnectionStats>,
    /// Idle timeout (nanoseconds since epoch)
    idle_timeout_ts: AtomicU64,
    /// Handshake completed
    handshake_completed: AtomicBool,
    /// Connection closed
    is_closed: AtomicBool,
    /// Close error code (if closed with error)
    close_error_code: AtomicU64,
    /// Maximum datagram size
    max_datagram_size: AtomicU64,
    /// Initial packet token
    token: RwLock<Vec<u8>>,
    /// Retry token received
    retry_token: RwLock<Vec<u8>>,
    /// Application error code on close
    app_error_code: AtomicU64,
    /// Close reason
    close_reason: RwLock<Vec<u8>>,
}

// SAFETY: Connection uses internal synchronization (RwLock, Mutex, Atomic*)
// and user_data is only accessed via callbacks controlled by the user
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

impl Connection {
    /// Create a new client connection
    pub fn client(
        dcid: &ConnectionId,
        scid: &ConnectionId,
        path: &Path,
        version: u32,
        callbacks: &ConnectionCallbacks,
        settings: &Settings,
        transport_params: &TransportParams,
        user_data: *mut c_void,
    ) -> Result<Box<Self>> {
        let mut conn = Box::new(Self::new_internal(
            ConnectionRole::Client,
            dcid,
            scid,
            path,
            version,
            callbacks,
            settings,
            transport_params,
            user_data,
        )?);

        // Derive initial secrets for client
        conn.crypto
            .lock()
            .derive_initial_secrets(dcid, true)?;

        Ok(conn)
    }

    /// Create a new server connection
    pub fn server(
        dcid: &ConnectionId,
        scid: &ConnectionId,
        path: &Path,
        version: u32,
        callbacks: &ConnectionCallbacks,
        settings: &Settings,
        transport_params: &TransportParams,
        user_data: *mut c_void,
    ) -> Result<Box<Self>> {
        let mut conn = Box::new(Self::new_internal(
            ConnectionRole::Server,
            dcid,
            scid,
            path,
            version,
            callbacks,
            settings,
            transport_params,
            user_data,
        )?);

        // Derive initial secrets for server
        conn.crypto
            .lock()
            .derive_initial_secrets(dcid, false)?;

        Ok(conn)
    }

    /// Internal constructor
    fn new_internal(
        role: ConnectionRole,
        dcid: &ConnectionId,
        scid: &ConnectionId,
        path: &Path,
        version: u32,
        callbacks: &ConnectionCallbacks,
        settings: &Settings,
        transport_params: &TransportParams,
        user_data: *mut c_void,
    ) -> Result<Self> {
        let is_client = matches!(role, ConnectionRole::Client);

        Ok(Self {
            role,
            state: RwLock::new(ConnectionState::Initial),
            scid: RwLock::new(*scid),
            dcid: RwLock::new(*dcid),
            odcid: *dcid,
            version,
            path: RwLock::new(path.clone()),
            local_transport_params: RwLock::new(transport_params.clone()),
            remote_transport_params: RwLock::new(None),
            settings: settings.clone(),
            callbacks: callbacks.clone(),
            user_data,
            crypto: Mutex::new(CryptoContext::new(version)),
            streams: RwLock::new(StreamManager::new(is_client)),
            pending_packets: Mutex::new(VecDeque::new()),
            next_pkt_num: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            largest_acked_pkt_num: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            stats: RwLock::new(ConnectionStats::default()),
            idle_timeout_ts: AtomicU64::new(NGTCP2_TSTAMP_MAX),
            handshake_completed: AtomicBool::new(false),
            is_closed: AtomicBool::new(false),
            close_error_code: AtomicU64::new(0),
            max_datagram_size: AtomicU64::new(1200),
            token: RwLock::new(Vec::new()),
            retry_token: RwLock::new(Vec::new()),
            app_error_code: AtomicU64::new(0),
            close_reason: RwLock::new(Vec::new()),
        })
    }

    // ========================================================================
    // State accessors
    // ========================================================================

    /// Get connection state
    pub fn get_state(&self) -> ConnectionState {
        *self.state.read()
    }

    /// Check if handshake is completed
    pub fn is_handshake_completed(&self) -> bool {
        self.handshake_completed.load(Ordering::Acquire)
    }

    /// Check if connection is closed
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Acquire)
    }

    /// Get connection role
    pub fn get_role(&self) -> ConnectionRole {
        self.role
    }

    /// Get source connection ID
    pub fn get_scid(&self) -> ConnectionId {
        *self.scid.read()
    }

    /// Get destination connection ID
    pub fn get_dcid(&self) -> ConnectionId {
        *self.dcid.read()
    }

    /// Get QUIC version
    pub fn get_version(&self) -> u32 {
        self.version
    }

    /// Get user data
    pub fn get_user_data(&self) -> *mut c_void {
        self.user_data
    }

    /// Get connection statistics
    pub fn get_stats(&self) -> ConnectionStats {
        self.stats.read().clone()
    }

    /// Get current path
    pub fn get_path(&self) -> Path {
        self.path.read().clone()
    }

    // ========================================================================
    // Packet handling
    // ========================================================================

    /// Feed a received packet to the connection
    pub fn read_pkt(&mut self, path: &Path, pi: *const PacketInfo, data: &[u8], ts: Timestamp) -> Result<ssize_t> {
        if self.is_closed() {
            return Err(Error::Ng(NgError::InvalidState));
        }

        // Parse packet header
        let dcid_len = self.scid.read().datalen;
        let (header, header_len) = crate::packet::parse_header(data, dcid_len)?;

        // Verify destination CID matches our source CID (for non-Initial)
        if header.pkt_type != PacketType::Initial {
            if header.dcid != *self.scid.read() {
                return Err(Error::Ng(NgError::Proto));
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.bytes_recv += data.len() as u64;
            stats.pkts_recv += 1;
        }

        // Process based on packet type
        match header.pkt_type {
            PacketType::Initial => self.handle_initial_packet(&header, &data[header_len..], ts),
            PacketType::Handshake => self.handle_handshake_packet(&header, &data[header_len..], ts),
            PacketType::Short => self.handle_short_packet(&header, &data[header_len..], ts),
            PacketType::ZeroRtt => self.handle_0rtt_packet(&header, &data[header_len..], ts),
            PacketType::Retry => self.handle_retry_packet(&header, &data[header_len..], ts),
            PacketType::VersionNegotiation => {
                self.handle_version_negotiation(&header, &data[header_len..], ts)
            }
        }
    }

    /// Handle Initial packet
    fn handle_initial_packet(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
        ts: Timestamp,
    ) -> Result<ssize_t> {
        let state = *self.state.read();
        
        if state != ConnectionState::Initial && state != ConnectionState::Handshake {
            // Initial packets are only valid during handshake
            return Ok(0);
        }

        // Server: update DCID to client's SCID
        if self.role == ConnectionRole::Server && state == ConnectionState::Initial {
            *self.dcid.write() = header.scid;

            // Call recv_client_initial callback
            if let Some(cb) = self.callbacks.recv_client_initial {
                let result = cb(self as *mut _, &header.dcid, self.user_data);
                if result != 0 {
                    return Err(Error::Ng(NgError::CallbackFailure));
                }
            }
        }

        // Decrypt and process frames
        self.process_encrypted_payload(EncryptionLevel::Initial, payload, ts)?;

        Ok(payload.len() as ssize_t)
    }

    /// Handle Handshake packet
    fn handle_handshake_packet(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
        ts: Timestamp,
    ) -> Result<ssize_t> {
        let state = *self.state.read();

        if state != ConnectionState::Handshake && state != ConnectionState::Initial {
            return Ok(0);
        }

        // Transition to Handshake state
        if state == ConnectionState::Initial {
            *self.state.write() = ConnectionState::Handshake;
        }

        // Decrypt and process frames
        self.process_encrypted_payload(EncryptionLevel::Handshake, payload, ts)?;

        Ok(payload.len() as ssize_t)
    }

    /// Handle 1-RTT (short header) packet
    fn handle_short_packet(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
        ts: Timestamp,
    ) -> Result<ssize_t> {
        let state = *self.state.read();

        if state != ConnectionState::Established && state != ConnectionState::Handshake {
            return Err(Error::Ng(NgError::InvalidState));
        }

        // Transition to Established if needed
        if state == ConnectionState::Handshake {
            self.complete_handshake(ts)?;
        }

        // Decrypt and process frames
        self.process_encrypted_payload(EncryptionLevel::OneRtt, payload, ts)?;

        Ok(payload.len() as ssize_t)
    }

    /// Handle 0-RTT packet
    fn handle_0rtt_packet(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
        ts: Timestamp,
    ) -> Result<ssize_t> {
        // 0-RTT is only valid for server receiving early data
        if self.role != ConnectionRole::Server {
            return Ok(0);
        }

        // Decrypt and process frames
        self.process_encrypted_payload(EncryptionLevel::ZeroRtt, payload, ts)?;

        Ok(payload.len() as ssize_t)
    }

    /// Handle Retry packet
    fn handle_retry_packet(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
        _ts: Timestamp,
    ) -> Result<ssize_t> {
        // Only client handles Retry
        if self.role != ConnectionRole::Client {
            return Ok(0);
        }

        // Validate retry tag (last 16 bytes)
        if payload.len() < 16 {
            return Err(Error::Ng(NgError::Proto));
        }

        // Update DCID to server's new SCID
        *self.dcid.write() = header.scid;

        // Store retry token
        let token_end = payload.len() - 16;
        *self.retry_token.write() = payload[..token_end].to_vec();

        // Re-derive initial secrets with new DCID
        self.crypto
            .lock()
            .derive_initial_secrets(&header.scid, true)?;

        Ok(payload.len() as ssize_t)
    }

    /// Handle Version Negotiation packet
    fn handle_version_negotiation(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
        _ts: Timestamp,
    ) -> Result<ssize_t> {
        // Only client handles Version Negotiation
        if self.role != ConnectionRole::Client {
            return Ok(0);
        }

        // Check if our version is in the list
        let mut versions = Vec::new();
        let mut offset = 0;
        while offset + 4 <= payload.len() {
            let version = u32::from_be_bytes([
                payload[offset],
                payload[offset + 1],
                payload[offset + 2],
                payload[offset + 3],
            ]);
            versions.push(version);
            offset += 4;
        }

        // If our version is supported, this is a downgrade attack
        if versions.contains(&self.version) {
            return Err(Error::Transport(TransportError::ProtocolViolation));
        }

        // Close connection - version not supported
        Err(Error::Transport(TransportError::ConnectionRefused))
    }

    /// Process encrypted payload (decrypt and parse frames)
    fn process_encrypted_payload(
        &mut self,
        level: EncryptionLevel,
        _encrypted: &[u8],
        ts: Timestamp,
    ) -> Result<()> {
        // TODO: Decrypt payload using crypto context
        // For now, assume payload is already decrypted frames

        // Parse and handle frames
        // This would call recv_crypto_data, recv_stream_data callbacks etc.

        // Update idle timeout
        self.update_idle_timeout(ts);

        Ok(())
    }

    /// Complete handshake and transition to Established state
    fn complete_handshake(&mut self, ts: Timestamp) -> Result<()> {
        *self.state.write() = ConnectionState::Established;
        self.handshake_completed.store(true, Ordering::Release);

        // Calculate handshake duration
        // (would need start timestamp to calculate)

        // Call handshake_completed callback
        if let Some(cb) = self.callbacks.handshake_completed {
            let result = cb(self as *mut _, self.user_data);
            if result != 0 {
                return Err(Error::Ng(NgError::CallbackFailure));
            }
        }

        Ok(())
    }

    /// Update idle timeout
    fn update_idle_timeout(&self, ts: Timestamp) {
        let timeout_duration = self
            .local_transport_params
            .read()
            .max_idle_timeout;

        if timeout_duration > 0 {
            self.idle_timeout_ts
                .store(ts + timeout_duration, Ordering::Release);
        }
    }

    // ========================================================================
    // Packet writing
    // ========================================================================

    /// Write packets to send
    pub fn write_pkt(
        &mut self,
        path: *mut Path,
        pi: *mut PacketInfo,
        dest: &mut [u8],
        ts: Timestamp,
    ) -> Result<ssize_t> {
        if self.is_closed() {
            return Ok(0);
        }

        // Check for pending packets
        if let Some(pending) = self.pending_packets.lock().pop_front() {
            if dest.len() >= pending.data.len() {
                dest[..pending.data.len()].copy_from_slice(&pending.data);

                // Update stats
                {
                    let mut stats = self.stats.write();
                    stats.bytes_sent += pending.data.len() as u64;
                    stats.pkts_sent += 1;
                }

                return Ok(pending.data.len() as ssize_t);
            }
        }

        // Generate new packets based on state
        let state = *self.state.read();
        match state {
            ConnectionState::Initial => self.write_initial_packet(dest, ts),
            ConnectionState::Handshake => self.write_handshake_packet(dest, ts),
            ConnectionState::Established => self.write_1rtt_packet(dest, ts),
            _ => Ok(0),
        }
    }

    /// Write Initial packet
    fn write_initial_packet(&mut self, dest: &mut [u8], _ts: Timestamp) -> Result<ssize_t> {
        let mut builder = PacketBuilder::new(dest.len());

        let scid = *self.scid.read();
        let dcid = *self.dcid.read();
        let token = self.token.read().clone();

        builder.start_initial(self.version, &dcid, &scid, &token)?;

        // Add CRYPTO frames, etc.

        let len = builder.finish(dest)?;
        Ok(len as ssize_t)
    }

    /// Write Handshake packet
    fn write_handshake_packet(&mut self, dest: &mut [u8], _ts: Timestamp) -> Result<ssize_t> {
        let mut builder = PacketBuilder::new(dest.len());

        let scid = *self.scid.read();
        let dcid = *self.dcid.read();

        builder.start_handshake(self.version, &dcid, &scid)?;

        // Add CRYPTO frames, etc.

        let len = builder.finish(dest)?;
        Ok(len as ssize_t)
    }

    /// Write 1-RTT packet
    fn write_1rtt_packet(&mut self, dest: &mut [u8], _ts: Timestamp) -> Result<ssize_t> {
        let mut builder = PacketBuilder::new(dest.len());

        let dcid = *self.dcid.read();
        builder.start_short(&dcid)?;

        // Add stream data frames, ACKs, etc.

        let len = builder.finish(dest)?;
        Ok(len as ssize_t)
    }

    // ========================================================================
    // Stream operations
    // ========================================================================

    /// Open a new bidirectional stream
    pub fn open_bidi_stream(&self) -> Result<StreamId> {
        if !self.is_handshake_completed() {
            return Err(Error::Ng(NgError::InvalidState));
        }

        self.streams.write().open_bidi_stream()
    }

    /// Open a new unidirectional stream
    pub fn open_uni_stream(&self) -> Result<StreamId> {
        if !self.is_handshake_completed() {
            return Err(Error::Ng(NgError::InvalidState));
        }

        self.streams.write().open_uni_stream()
    }

    /// Write data to a stream
    pub fn stream_write(&self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<usize> {
        let mut streams = self.streams.write();
        streams.write_data(stream_id, data, fin)
    }

    /// Read data from a stream
    pub fn stream_read(&self, stream_id: StreamId, dest: &mut [u8]) -> Result<(usize, bool)> {
        let mut streams = self.streams.write();
        streams.read_data(stream_id, dest)
    }

    /// Shutdown a stream (send FIN)
    pub fn stream_shutdown(&self, stream_id: StreamId, flags: u32) -> Result<()> {
        let mut streams = self.streams.write();
        streams.shutdown(stream_id, flags)
    }

    /// Close a stream with error
    pub fn stream_close(&self, stream_id: StreamId, error_code: u64) -> Result<()> {
        let mut streams = self.streams.write();
        streams.close(stream_id, error_code)
    }

    // ========================================================================
    // Connection close
    // ========================================================================

    /// Close connection with transport error
    pub fn close(&mut self, error_code: TransportError, reason: &[u8]) -> Result<()> {
        if self.is_closed() {
            return Ok(());
        }

        *self.state.write() = ConnectionState::Closing;
        self.close_error_code
            .store(error_code.as_u64(), Ordering::Release);
        *self.close_reason.write() = reason.to_vec();

        Ok(())
    }

    /// Close connection with application error
    pub fn close_app(&mut self, error_code: u64, reason: &[u8]) -> Result<()> {
        if self.is_closed() {
            return Ok(());
        }

        *self.state.write() = ConnectionState::Closing;
        self.app_error_code.store(error_code, Ordering::Release);
        *self.close_reason.write() = reason.to_vec();

        Ok(())
    }

    /// Get expiry timestamp (for timers)
    pub fn get_expiry(&self) -> Timestamp {
        // Return earliest of: idle timeout, loss detection timer, etc.
        self.idle_timeout_ts.load(Ordering::Acquire)
    }

    /// Handle timer expiry
    pub fn handle_expiry(&mut self, ts: Timestamp) -> Result<()> {
        let idle_timeout = self.idle_timeout_ts.load(Ordering::Acquire);

        if ts >= idle_timeout {
            // Idle timeout - close connection
            self.close(TransportError::NoError, b"idle timeout")?;
        }

        // Handle loss detection timers, etc.

        Ok(())
    }
}

// ============================================================================
// Packet Info (ngtcp2 compatible)
// ============================================================================

/// Packet metadata
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct PacketInfo {
    /// ECN information
    pub ecn: u8,
}

/// ngtcp2-compatible alias
pub type ngtcp2_pkt_info = PacketInfo;

// ============================================================================
// ngtcp2 C API Compatibility Layer
// ============================================================================

/// ngtcp2_conn type alias (opaque pointer to Connection)
pub type ngtcp2_conn = Connection;

/// Create a new client connection (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_client_new(
    pconn: *mut *mut ngtcp2_conn,
    dcid: *const ngtcp2_cid,
    scid: *const ngtcp2_cid,
    path: *const Path,
    version: u32,
    callbacks: *const ngtcp2_callbacks,
    settings: *const ngtcp2_settings,
    transport_params: *const ngtcp2_transport_params,
    _mem: *const c_void,
    user_data: *mut c_void,
) -> c_int {
    if pconn.is_null() || dcid.is_null() || scid.is_null() || path.is_null() {
        return -201; // ERR_INVALID_ARGUMENT
    }

    unsafe {
        let dcid = &*dcid;
        let scid = &*scid;
        let path = &*path;
        let callbacks = callbacks.as_ref().cloned().unwrap_or_default();
        let settings = settings
            .as_ref()
            .cloned()
            .unwrap_or_default();
        let transport_params = transport_params
            .as_ref()
            .map(|tp| TransportParams {
                original_dcid: tp.original_dcid,
                initial_scid: tp.initial_scid,
                initial_max_data: tp.initial_max_data as u64,
                initial_max_stream_data_bidi_local: tp.initial_max_stream_data_bidi_local as u64,
                initial_max_stream_data_bidi_remote: tp.initial_max_stream_data_bidi_remote as u64,
                initial_max_stream_data_uni: tp.initial_max_stream_data_uni as u64,
                initial_max_streams_bidi: tp.initial_max_streams_bidi as u64,
                initial_max_streams_uni: tp.initial_max_streams_uni as u64,
                max_idle_timeout: tp.max_idle_timeout * 1_000_000, // ms to ns
                max_udp_payload_size: tp.max_udp_payload_size as u64,
                ..Default::default()
            })
            .unwrap_or_default();

        match Connection::client(
            dcid,
            scid,
            path,
            version,
            &callbacks,
            &settings,
            &transport_params,
            user_data,
        ) {
            Ok(conn) => {
                *pconn = Box::into_raw(conn);
                0
            }
            Err(_) => -502, // ERR_NOMEM
        }
    }
}

/// Create a new server connection (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_server_new(
    pconn: *mut *mut ngtcp2_conn,
    dcid: *const ngtcp2_cid,
    scid: *const ngtcp2_cid,
    path: *const Path,
    version: u32,
    callbacks: *const ngtcp2_callbacks,
    settings: *const ngtcp2_settings,
    transport_params: *const ngtcp2_transport_params,
    _mem: *const c_void,
    user_data: *mut c_void,
) -> c_int {
    if pconn.is_null() || dcid.is_null() || scid.is_null() || path.is_null() {
        return -201;
    }

    unsafe {
        let dcid = &*dcid;
        let scid = &*scid;
        let path = &*path;
        let callbacks = callbacks.as_ref().cloned().unwrap_or_default();
        let settings = settings
            .as_ref()
            .cloned()
            .unwrap_or_default();
        let transport_params = transport_params
            .as_ref()
            .map(|tp| TransportParams {
                original_dcid: tp.original_dcid,
                initial_scid: tp.initial_scid,
                initial_max_data: tp.initial_max_data as u64,
                ..Default::default()
            })
            .unwrap_or_default();

        match Connection::server(
            dcid,
            scid,
            path,
            version,
            &callbacks,
            &settings,
            &transport_params,
            user_data,
        ) {
            Ok(conn) => {
                *pconn = Box::into_raw(conn);
                0
            }
            Err(_) => -502,
        }
    }
}

/// Delete a connection (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_del(conn: *mut ngtcp2_conn) {
    if !conn.is_null() {
        unsafe {
            drop(Box::from_raw(conn));
        }
    }
}

/// Read packet (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_read_pkt(
    conn: *mut ngtcp2_conn,
    path: *const Path,
    pi: *const PacketInfo,
    pkt: *const u8,
    pktlen: size_t,
    ts: Timestamp,
) -> ssize_t {
    if conn.is_null() || pkt.is_null() {
        return -201;
    }

    unsafe {
        let conn = &mut *conn;
        let path = &*path;
        let data = std::slice::from_raw_parts(pkt, pktlen);

        match conn.read_pkt(path, pi, data, ts) {
            Ok(n) => n,
            Err(e) => match e {
                Error::Ng(NgError::Proto) => -203,
                Error::Ng(NgError::InvalidState) => -204,
                _ => -501,
            },
        }
    }
}

/// Write packet (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_write_pkt(
    conn: *mut ngtcp2_conn,
    path: *mut Path,
    pi: *mut PacketInfo,
    dest: *mut u8,
    destlen: size_t,
    ts: Timestamp,
) -> ssize_t {
    if conn.is_null() || dest.is_null() {
        return -201;
    }

    unsafe {
        let conn = &mut *conn;
        let dest = std::slice::from_raw_parts_mut(dest, destlen);

        match conn.write_pkt(path, pi, dest, ts) {
            Ok(n) => n,
            Err(_) => -501,
        }
    }
}

/// Write stream data (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_write_stream(
    conn: *mut ngtcp2_conn,
    path: *mut Path,
    pi: *mut PacketInfo,
    dest: *mut u8,
    destlen: size_t,
    pdatalen: *mut ssize_t,
    flags: u32,
    stream_id: i64,
    data: *const u8,
    datalen: size_t,
    ts: Timestamp,
) -> ssize_t {
    if conn.is_null() || dest.is_null() {
        return -201;
    }

    unsafe {
        let conn = &mut *conn;

        // First write stream data
        if !data.is_null() && datalen > 0 {
            let data = std::slice::from_raw_parts(data, datalen);
            let fin = (flags & 0x01) != 0;

            match conn.stream_write(stream_id, data, fin) {
                Ok(n) => {
                    if !pdatalen.is_null() {
                        *pdatalen = n as ssize_t;
                    }
                }
                Err(_) => return -220, // ERR_STREAM_NOT_FOUND
            }
        }

        // Then write packet
        let dest = std::slice::from_raw_parts_mut(dest, destlen);
        match conn.write_pkt(path, pi, dest, ts) {
            Ok(n) => n,
            Err(_) => -501,
        }
    }
}

/// Open a bidirectional stream (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_open_bidi_stream(
    conn: *mut ngtcp2_conn,
    pstream_id: *mut i64,
    stream_user_data: *mut c_void,
) -> c_int {
    if conn.is_null() || pstream_id.is_null() {
        return -201;
    }

    unsafe {
        let conn = &*conn;
        match conn.open_bidi_stream() {
            Ok(stream_id) => {
                *pstream_id = stream_id;
                0
            }
            Err(_) => -211, // ERR_STREAM_LIMIT
        }
    }
}

/// Open a unidirectional stream (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_open_uni_stream(
    conn: *mut ngtcp2_conn,
    pstream_id: *mut i64,
    stream_user_data: *mut c_void,
) -> c_int {
    if conn.is_null() || pstream_id.is_null() {
        return -201;
    }

    unsafe {
        let conn = &*conn;
        match conn.open_uni_stream() {
            Ok(stream_id) => {
                *pstream_id = stream_id;
                0
            }
            Err(_) => -211,
        }
    }
}

/// Get connection statistics (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_get_conn_stat(conn: *mut ngtcp2_conn, stat: *mut ngtcp2_conn_stat) {
    if conn.is_null() || stat.is_null() {
        return;
    }

    unsafe {
        let conn = &*conn;
        *stat = conn.get_stats();
    }
}

/// Check if handshake is completed (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_get_handshake_completed(conn: *const ngtcp2_conn) -> c_int {
    if conn.is_null() {
        return 0;
    }

    unsafe {
        let conn = &*conn;
        if conn.is_handshake_completed() { 1 } else { 0 }
    }
}

/// Get expiry timestamp (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_get_expiry(conn: *const ngtcp2_conn) -> Timestamp {
    if conn.is_null() {
        return NGTCP2_TSTAMP_MAX;
    }

    unsafe {
        let conn = &*conn;
        conn.get_expiry()
    }
}

/// Handle timeout (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_handle_expiry(conn: *mut ngtcp2_conn, ts: Timestamp) -> c_int {
    if conn.is_null() {
        return -201;
    }

    unsafe {
        let conn = &mut *conn;
        match conn.handle_expiry(ts) {
            Ok(_) => 0,
            Err(_) => -501,
        }
    }
}

/// Shutdown stream (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_shutdown_stream(
    conn: *mut ngtcp2_conn,
    flags: u32,
    stream_id: i64,
    app_error_code: u64,
) -> c_int {
    if conn.is_null() {
        return -201;
    }

    unsafe {
        let conn = &*conn;
        match conn.stream_shutdown(stream_id, flags) {
            Ok(_) => 0,
            Err(_) => -220,
        }
    }
}

/// Set local transport parameters (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_set_local_transport_params(
    conn: *mut ngtcp2_conn,
    params: *const ngtcp2_transport_params,
) -> c_int {
    if conn.is_null() || params.is_null() {
        return -201;
    }

    // TODO: Convert and set transport params
    0
}

/// Get remote transport parameters (ngtcp2 compatible)
#[no_mangle]
pub extern "C" fn ngtcp2_conn_get_remote_transport_params(
    conn: *const ngtcp2_conn,
    params: *mut ngtcp2_transport_params,
) -> c_int {
    if conn.is_null() || params.is_null() {
        return -201;
    }

    // TODO: Get remote transport params
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state() {
        assert_eq!(
            ConnectionState::default(),
            ConnectionState::Initial
        );
    }

    #[test]
    fn test_callbacks_default() {
        let callbacks = ConnectionCallbacks::default();
        assert!(callbacks.client_initial.is_none());
    }
}
