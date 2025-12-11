//! QUIC Transport Layer Integration
//!
//! This module provides the integration between HTTP/3 (nh3) and QUIC (ntcp2).
//! It handles the setup and management of QUIC connections for HTTP/3 transport.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                     HTTP/3 Application                         │
//! │                    (uses nghttp3 C ABI)                        │
//! ├────────────────────────────────────────────────────────────────┤
//! │                     QuicTransport                              │
//! │         (bridges HTTP/3 connection to QUIC layer)             │
//! ├────────────────────────────────────────────────────────────────┤
//! │                     ntcp2 (ngtcp2 C ABI)                       │
//! │              (QUIC protocol implementation)                    │
//! ├────────────────────────────────────────────────────────────────┤
//! │                  nssl/ncryptolib                               │
//! │                  (TLS 1.3 / Crypto)                            │
//! └────────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::connection::nghttp3_conn;
use crate::error::{Error, ErrorCode, Result};
use crate::quic_ffi::{
    Ngtcp2Cid, Ngtcp2Settings, Ngtcp2TransportParams,
    ngtcp2_version,
};
use crate::types::Settings;
use crate::{c_int, c_void, size_t, ssize_t};

// ============================================================================
// Extended QUIC FFI Functions
// ============================================================================

#[link(name = "ngtcp2")]
extern "C" {
    // Connection creation/destruction
    fn ngtcp2_conn_client_new(
        pconn: *mut *mut NgtcpConn,
        dcid: *const Ngtcp2Cid,
        scid: *const Ngtcp2Cid,
        path: *const NgtcpPath,
        version: u32,
        callbacks: *const NgtcpCallbacks,
        settings: *const Ngtcp2Settings,
        params: *const Ngtcp2TransportParams,
        mem: *const c_void,
        user_data: *mut c_void,
    ) -> c_int;

    fn ngtcp2_conn_server_new(
        pconn: *mut *mut NgtcpConn,
        dcid: *const Ngtcp2Cid,
        scid: *const Ngtcp2Cid,
        path: *const NgtcpPath,
        version: u32,
        callbacks: *const NgtcpCallbacks,
        settings: *const Ngtcp2Settings,
        params: *const Ngtcp2TransportParams,
        mem: *const c_void,
        user_data: *mut c_void,
    ) -> c_int;

    fn ngtcp2_conn_del(conn: *mut NgtcpConn);

    // Stream operations
    fn ngtcp2_conn_open_bidi_stream(
        conn: *mut NgtcpConn,
        pstream_id: *mut i64,
        stream_user_data: *mut c_void,
    ) -> c_int;

    fn ngtcp2_conn_open_uni_stream(
        conn: *mut NgtcpConn,
        pstream_id: *mut i64,
        stream_user_data: *mut c_void,
    ) -> c_int;

    fn ngtcp2_conn_extend_max_stream_offset(
        conn: *mut NgtcpConn,
        stream_id: i64,
        datalen: u64,
    ) -> c_int;

    fn ngtcp2_conn_extend_max_offset(conn: *mut NgtcpConn, datalen: u64) -> c_int;

    fn ngtcp2_conn_shutdown_stream(
        conn: *mut NgtcpConn,
        flags: u32,
        stream_id: i64,
        app_error_code: u64,
    ) -> c_int;

    // Data transmission
    fn ngtcp2_conn_writev_stream(
        conn: *mut NgtcpConn,
        path: *mut NgtcpPath,
        pi: *mut NgtcpPacketInfo,
        dest: *mut u8,
        destlen: size_t,
        pdatalen: *mut ssize_t,
        flags: u32,
        stream_id: i64,
        datav: *const NgtcpVec,
        datavcnt: size_t,
        ts: u64,
    ) -> ssize_t;

    fn ngtcp2_conn_read_pkt(
        conn: *mut NgtcpConn,
        path: *const NgtcpPath,
        pi: *const NgtcpPacketInfo,
        pkt: *const u8,
        pktlen: size_t,
        ts: u64,
    ) -> c_int;

    // State queries
    fn ngtcp2_conn_get_handshake_completed(conn: *mut NgtcpConn) -> c_int;
    fn ngtcp2_conn_in_closing_period(conn: *const NgtcpConn) -> c_int;
    fn ngtcp2_conn_in_draining_period(conn: *const NgtcpConn) -> c_int;

    // Timeout management
    fn ngtcp2_conn_get_expiry(conn: *mut NgtcpConn) -> u64;
    fn ngtcp2_conn_handle_expiry(conn: *mut NgtcpConn, ts: u64) -> c_int;
}

// ============================================================================
// QUIC Type Definitions
// ============================================================================

/// Opaque QUIC connection handle
#[repr(C)]
pub struct NgtcpConn {
    _private: [u8; 0],
}

/// QUIC path (local and remote addresses)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct NgtcpPath {
    /// Local address
    pub local: NgtcpAddr,
    /// Remote address
    pub remote: NgtcpAddr,
}

/// QUIC address
#[repr(C)]
#[derive(Debug, Clone)]
pub struct NgtcpAddr {
    /// Address storage
    pub addr: [u8; 128], // sockaddr_storage size
    /// Address length
    pub addrlen: u32,
}

impl Default for NgtcpAddr {
    fn default() -> Self {
        Self {
            addr: [0u8; 128],
            addrlen: 0,
        }
    }
}

impl NgtcpAddr {
    /// Create from IPv4 address bytes and port
    /// Format: [ip0, ip1, ip2, ip3], port
    pub fn from_ipv4(ip: [u8; 4], port: u16) -> Self {
        let mut result = Self::default();
        // sockaddr_in layout: family(2) + port(2) + addr(4) + zero(8)
        result.addr[0] = 2; // AF_INET on most systems
        result.addr[1] = 0;
        result.addr[2] = (port >> 8) as u8;
        result.addr[3] = port as u8;
        result.addr[4..8].copy_from_slice(&ip);
        result.addrlen = 16;
        result
    }

    /// Create from IPv6 address bytes and port
    /// Format: 16-byte IPv6 address, port
    pub fn from_ipv6(ip: [u8; 16], port: u16) -> Self {
        let mut result = Self::default();
        // sockaddr_in6 layout: family(2) + port(2) + flowinfo(4) + addr(16) + scope_id(4)
        result.addr[0] = 10; // AF_INET6
        result.addr[1] = 0;
        result.addr[2] = (port >> 8) as u8;
        result.addr[3] = port as u8;
        // flowinfo = 0
        result.addr[4..8].fill(0);
        result.addr[8..24].copy_from_slice(&ip);
        result.addr[24..28].fill(0); // scope_id
        result.addrlen = 28;
        result
    }
}

impl Default for NgtcpPath {
    fn default() -> Self {
        Self {
            local: NgtcpAddr::default(),
            remote: NgtcpAddr::default(),
        }
    }
}

/// QUIC packet info
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct NgtcpPacketInfo {
    /// ECN value
    pub ecn: u32,
}

/// QUIC vector (iovec-like)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NgtcpVec {
    /// Base pointer
    pub base: *const u8,
    /// Length
    pub len: size_t,
}

impl NgtcpVec {
    /// Create from slice
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            base: data.as_ptr(),
            len: data.len(),
        }
    }
}

/// QUIC callbacks structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct NgtcpCallbacks {
    pub client_initial: Option<extern "C" fn(*mut NgtcpConn, *mut c_void) -> c_int>,
    pub recv_client_initial: Option<extern "C" fn(*mut NgtcpConn, *const Ngtcp2Cid, *mut c_void) -> c_int>,
    pub recv_crypto_data: Option<extern "C" fn(*mut NgtcpConn, u32, u64, *const u8, size_t, *mut c_void) -> c_int>,
    pub handshake_completed: Option<extern "C" fn(*mut NgtcpConn, *mut c_void) -> c_int>,
    pub recv_version_negotiation: Option<extern "C" fn(*mut NgtcpConn, *const u32, size_t, *mut c_void) -> c_int>,
    pub encrypt: Option<extern "C" fn(*mut u8, *const Ngtcp2Cid, *const u8, size_t, *const u8, size_t, *const u8, size_t, *mut c_void) -> c_int>,
    pub decrypt: Option<extern "C" fn(*mut u8, *const Ngtcp2Cid, *const u8, size_t, *const u8, size_t, *const u8, size_t, *mut c_void) -> c_int>,
    pub hp_mask: Option<extern "C" fn(*mut u8, *const Ngtcp2Cid, *const u8, *const u8, *mut c_void) -> c_int>,
    pub recv_stream_data: Option<extern "C" fn(*mut NgtcpConn, u32, i64, u64, *const u8, size_t, *mut c_void) -> c_int>,
    pub acked_stream_data_offset: Option<extern "C" fn(*mut NgtcpConn, i64, u64, u64, *mut c_void) -> c_int>,
    pub stream_open: Option<extern "C" fn(*mut NgtcpConn, i64, *mut c_void) -> c_int>,
    pub stream_close: Option<extern "C" fn(*mut NgtcpConn, u32, i64, u64, *mut c_void) -> c_int>,
    pub recv_stateless_reset: Option<extern "C" fn(*mut NgtcpConn, *const u8, *mut c_void) -> c_int>,
    pub recv_retry: Option<extern "C" fn(*mut NgtcpConn, *const u8, *mut c_void) -> c_int>,
    pub extend_max_local_streams_bidi: Option<extern "C" fn(*mut NgtcpConn, u64, *mut c_void) -> c_int>,
    pub extend_max_local_streams_uni: Option<extern "C" fn(*mut NgtcpConn, u64, *mut c_void) -> c_int>,
    pub rand: Option<extern "C" fn(*mut u8, size_t, *mut c_void) -> c_int>,
    pub get_new_connection_id: Option<extern "C" fn(*mut NgtcpConn, *mut Ngtcp2Cid, *mut u8, size_t, *mut c_void) -> c_int>,
    pub remove_connection_id: Option<extern "C" fn(*mut NgtcpConn, *const Ngtcp2Cid, *mut c_void) -> c_int>,
    pub update_key: Option<extern "C" fn(*mut NgtcpConn, *mut u8, *mut u8, *const u8, *const u8, size_t, *mut c_void) -> c_int>,
    pub path_validation: Option<extern "C" fn(*mut NgtcpConn, u32, *const NgtcpPath, *const NgtcpPath, *mut c_void) -> c_int>,
    pub select_preferred_addr: Option<extern "C" fn(*mut NgtcpConn, *mut NgtcpPath, *mut c_void) -> c_int>,
    pub stream_reset: Option<extern "C" fn(*mut NgtcpConn, i64, u64, u64, *mut c_void) -> c_int>,
    pub extend_max_remote_streams_bidi: Option<extern "C" fn(*mut NgtcpConn, u64, *mut c_void) -> c_int>,
    pub extend_max_remote_streams_uni: Option<extern "C" fn(*mut NgtcpConn, u64, *mut c_void) -> c_int>,
    pub extend_max_stream_data: Option<extern "C" fn(*mut NgtcpConn, i64, u64, *mut c_void) -> c_int>,
    pub dcid_status: Option<extern "C" fn(*mut NgtcpConn, u32, u64, *const Ngtcp2Cid, *const u8, *mut c_void) -> c_int>,
    pub handshake_confirmed: Option<extern "C" fn(*mut NgtcpConn, *mut c_void) -> c_int>,
    pub recv_new_token: Option<extern "C" fn(*mut NgtcpConn, *const u8, size_t, *mut c_void) -> c_int>,
    pub delete_crypto_aead_ctx: Option<extern "C" fn(*mut NgtcpConn, *mut c_void, *mut c_void) -> c_int>,
    pub delete_crypto_cipher_ctx: Option<extern "C" fn(*mut NgtcpConn, *mut c_void, *mut c_void) -> c_int>,
    pub recv_datagram: Option<extern "C" fn(*mut NgtcpConn, u32, *const u8, size_t, *mut c_void) -> c_int>,
    pub ack_datagram: Option<extern "C" fn(*mut NgtcpConn, u64, *mut c_void) -> c_int>,
    pub lost_datagram: Option<extern "C" fn(*mut NgtcpConn, u64, *mut c_void) -> c_int>,
    pub get_path_challenge_data: Option<extern "C" fn(*mut NgtcpConn, *mut u8, *mut c_void) -> c_int>,
    pub stream_stop_sending: Option<extern "C" fn(*mut NgtcpConn, i64, u64, *mut c_void) -> c_int>,
}

impl Default for NgtcpCallbacks {
    fn default() -> Self {
        Self {
            client_initial: None,
            recv_client_initial: None,
            recv_crypto_data: None,
            handshake_completed: None,
            recv_version_negotiation: None,
            encrypt: None,
            decrypt: None,
            hp_mask: None,
            recv_stream_data: None,
            acked_stream_data_offset: None,
            stream_open: None,
            stream_close: None,
            recv_stateless_reset: None,
            recv_retry: None,
            extend_max_local_streams_bidi: None,
            extend_max_local_streams_uni: None,
            rand: None,
            get_new_connection_id: None,
            remove_connection_id: None,
            update_key: None,
            path_validation: None,
            select_preferred_addr: None,
            stream_reset: None,
            extend_max_remote_streams_bidi: None,
            extend_max_remote_streams_uni: None,
            extend_max_stream_data: None,
            dcid_status: None,
            handshake_confirmed: None,
            recv_new_token: None,
            delete_crypto_aead_ctx: None,
            delete_crypto_cipher_ctx: None,
            recv_datagram: None,
            ack_datagram: None,
            lost_datagram: None,
            get_path_challenge_data: None,
            stream_stop_sending: None,
        }
    }
}

// ============================================================================
// QUIC Transport State
// ============================================================================

/// QUIC transport state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    /// Initial state, handshake not started
    Initial,
    /// Handshake in progress
    Handshaking,
    /// Handshake completed, ready for data
    Connected,
    /// Connection closing
    Closing,
    /// Connection draining
    Draining,
    /// Connection closed
    Closed,
}

impl TransportState {
    fn as_i32(&self) -> i32 {
        match self {
            TransportState::Initial => 0,
            TransportState::Handshaking => 1,
            TransportState::Connected => 2,
            TransportState::Closing => 3,
            TransportState::Draining => 4,
            TransportState::Closed => 5,
        }
    }

    fn from_i32(val: i32) -> Self {
        match val {
            0 => TransportState::Initial,
            1 => TransportState::Handshaking,
            2 => TransportState::Connected,
            3 => TransportState::Closing,
            4 => TransportState::Draining,
            _ => TransportState::Closed,
        }
    }
}

// ============================================================================
// QUIC Transport
// ============================================================================

/// QUIC transport wrapper for HTTP/3
///
/// This struct manages the QUIC connection lifecycle and provides
/// methods for stream operations required by HTTP/3.
pub struct QuicTransport {
    /// QUIC connection handle
    quic_conn: *mut NgtcpConn,
    /// HTTP/3 connection handle
    h3_conn: *mut nghttp3_conn,
    /// Transport state (atomic for thread safety in no_std)
    state: AtomicI32,
    /// Local IPv4 address [ip0, ip1, ip2, ip3, port_hi, port_lo]
    local_addr: [u8; 6],
    /// Remote IPv4 address
    remote_addr: [u8; 6],
    /// User data for callbacks
    user_data: *mut c_void,
    /// Control stream ID (-1 if not set)
    ctrl_stream_id: i64,
    /// QPACK encoder stream ID (-1 if not set)
    qenc_stream_id: i64,
    /// QPACK decoder stream ID (-1 if not set)
    qdec_stream_id: i64,
}

// SAFETY: QuicTransport raw pointers are managed properly
unsafe impl Send for QuicTransport {}
unsafe impl Sync for QuicTransport {}

impl QuicTransport {
    /// Maximum packet buffer size
    pub const MAX_PACKET_SIZE: usize = 1500;

    /// Create a new client transport
    pub fn client_ipv4(
        local_ip: [u8; 4],
        local_port: u16,
        remote_ip: [u8; 4],
        remote_port: u16,
    ) -> Result<Box<Self>> {
        let mut local_addr = [0u8; 6];
        local_addr[0..4].copy_from_slice(&local_ip);
        local_addr[4] = (local_port >> 8) as u8;
        local_addr[5] = local_port as u8;

        let mut remote_addr = [0u8; 6];
        remote_addr[0..4].copy_from_slice(&remote_ip);
        remote_addr[4] = (remote_port >> 8) as u8;
        remote_addr[5] = remote_port as u8;

        let transport = Box::new(Self {
            quic_conn: core::ptr::null_mut(),
            h3_conn: core::ptr::null_mut(),
            state: AtomicI32::new(TransportState::Initial.as_i32()),
            local_addr,
            remote_addr,
            user_data: core::ptr::null_mut(),
            ctrl_stream_id: -1,
            qenc_stream_id: -1,
            qdec_stream_id: -1,
        });
        
        // Note: Actual QUIC connection setup will be done in connect()
        Ok(transport)
    }

    /// Create a new server transport
    pub fn server_ipv4(local_ip: [u8; 4], local_port: u16) -> Result<Box<Self>> {
        let mut local_addr = [0u8; 6];
        local_addr[0..4].copy_from_slice(&local_ip);
        local_addr[4] = (local_port >> 8) as u8;
        local_addr[5] = local_port as u8;

        let transport = Box::new(Self {
            quic_conn: core::ptr::null_mut(),
            h3_conn: core::ptr::null_mut(),
            state: AtomicI32::new(TransportState::Initial.as_i32()),
            local_addr,
            remote_addr: [0; 6],
            user_data: core::ptr::null_mut(),
            ctrl_stream_id: -1,
            qenc_stream_id: -1,
            qdec_stream_id: -1,
        });
        
        Ok(transport)
    }

    /// Get current state
    pub fn state(&self) -> TransportState {
        TransportState::from_i32(self.state.load(Ordering::SeqCst))
    }

    /// Set state
    pub fn set_state(&self, state: TransportState) {
        self.state.store(state.as_i32(), Ordering::SeqCst);
    }

    /// Check if handshake is completed
    pub fn is_handshake_completed(&self) -> bool {
        if self.quic_conn.is_null() {
            return false;
        }
        unsafe { ngtcp2_conn_get_handshake_completed(self.quic_conn) != 0 }
    }

    /// Check if connection is closing
    pub fn is_closing(&self) -> bool {
        if self.quic_conn.is_null() {
            return false;
        }
        unsafe { ngtcp2_conn_in_closing_period(self.quic_conn) != 0 }
    }

    /// Check if connection is draining
    pub fn is_draining(&self) -> bool {
        if self.quic_conn.is_null() {
            return false;
        }
        unsafe { ngtcp2_conn_in_draining_period(self.quic_conn) != 0 }
    }

    /// Open a bidirectional stream
    pub fn open_bidi_stream(&self, user_data: *mut c_void) -> Result<i64> {
        if self.quic_conn.is_null() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        let mut stream_id: i64 = 0;
        let ret = unsafe {
            ngtcp2_conn_open_bidi_stream(self.quic_conn, &mut stream_id, user_data)
        };
        
        if ret < 0 {
            Err(Error::QuicError(ret))
        } else {
            Ok(stream_id)
        }
    }

    /// Open a unidirectional stream
    pub fn open_uni_stream(&self, user_data: *mut c_void) -> Result<i64> {
        if self.quic_conn.is_null() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        let mut stream_id: i64 = 0;
        let ret = unsafe {
            ngtcp2_conn_open_uni_stream(self.quic_conn, &mut stream_id, user_data)
        };
        
        if ret < 0 {
            Err(Error::QuicError(ret))
        } else {
            Ok(stream_id)
        }
    }

    /// Extend max stream data offset
    pub fn extend_max_stream_offset(&self, stream_id: i64, datalen: u64) -> Result<()> {
        if self.quic_conn.is_null() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        let ret = unsafe {
            ngtcp2_conn_extend_max_stream_offset(self.quic_conn, stream_id, datalen)
        };
        
        if ret < 0 {
            Err(Error::QuicError(ret))
        } else {
            Ok(())
        }
    }

    /// Extend max connection data offset
    pub fn extend_max_offset(&self, datalen: u64) -> Result<()> {
        if self.quic_conn.is_null() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        let ret = unsafe {
            ngtcp2_conn_extend_max_offset(self.quic_conn, datalen)
        };
        
        if ret < 0 {
            Err(Error::QuicError(ret))
        } else {
            Ok(())
        }
    }

    /// Shutdown a stream
    pub fn shutdown_stream(&self, stream_id: i64, app_error_code: u64) -> Result<()> {
        if self.quic_conn.is_null() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        let ret = unsafe {
            ngtcp2_conn_shutdown_stream(self.quic_conn, 0, stream_id, app_error_code)
        };
        
        if ret < 0 {
            Err(Error::QuicError(ret))
        } else {
            Ok(())
        }
    }

    /// Get the next expiry timestamp
    pub fn get_expiry(&self) -> u64 {
        if self.quic_conn.is_null() {
            return u64::MAX;
        }
        unsafe { ngtcp2_conn_get_expiry(self.quic_conn) }
    }

    /// Handle timer expiry
    pub fn handle_expiry(&self, ts: u64) -> Result<()> {
        if self.quic_conn.is_null() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        let ret = unsafe { ngtcp2_conn_handle_expiry(self.quic_conn, ts) };
        
        if ret < 0 {
            Err(Error::QuicError(ret))
        } else {
            Ok(())
        }
    }

    /// Set HTTP/3 connection
    pub fn set_h3_conn(&mut self, h3_conn: *mut nghttp3_conn) {
        self.h3_conn = h3_conn;
    }

    /// Get HTTP/3 connection
    pub fn get_h3_conn(&self) -> *mut nghttp3_conn {
        self.h3_conn
    }
}

impl Drop for QuicTransport {
    fn drop(&mut self) {
        if !self.quic_conn.is_null() {
            unsafe {
                ngtcp2_conn_del(self.quic_conn);
            }
        }
    }
}

// ============================================================================
// Integrated Client
// ============================================================================

/// HTTP/3 + QUIC integrated client
///
/// This provides a high-level interface for HTTP/3 requests over QUIC,
/// combining both the HTTP/3 layer (nh3) and QUIC layer (ntcp2).
pub struct Http3Client {
    /// QUIC transport (boxed for ownership)
    transport: Option<Box<QuicTransport>>,
    /// HTTP/3 settings
    h3_settings: Settings,
    /// Connection state
    connected: bool,
}

impl Http3Client {
    /// Create a new HTTP/3 client
    pub fn new() -> Result<Self> {
        Ok(Self {
            transport: None,
            h3_settings: Settings::default(),
            connected: false,
        })
    }

    /// Check if QUIC/HTTP3 support is available
    pub fn is_available() -> bool {
        unsafe {
            let info = ngtcp2_version(0);
            !info.is_null()
        }
    }
}

// ============================================================================
// C ABI Functions
// ============================================================================

/// Create a QUIC transport for HTTP/3
#[no_mangle]
pub extern "C" fn nghttp3_quic_transport_client_new(
    local_addr: *const NgtcpAddr,
    remote_addr: *const NgtcpAddr,
    _alpn: *const u8,
    _alpnlen: size_t,
) -> *mut QuicTransport {
    if local_addr.is_null() || remote_addr.is_null() {
        return core::ptr::null_mut();
    }
    // Implementation would create and return a QuicTransport
    // For now, return null as placeholder
    core::ptr::null_mut()
}

/// Delete a QUIC transport
#[no_mangle]
pub extern "C" fn nghttp3_quic_transport_del(transport: *mut QuicTransport) {
    if !transport.is_null() {
        unsafe {
            let _ = Box::from_raw(transport);
        }
    }
}

/// Get QUIC transport state
#[no_mangle]
pub extern "C" fn nghttp3_quic_transport_get_state(transport: *const QuicTransport) -> c_int {
    if transport.is_null() {
        return -1;
    }
    unsafe { (*transport).state() as c_int }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ngtcp_addr_from_ipv4() {
        let addr = NgtcpAddr::from_ipv4([127, 0, 0, 1], 443);
        assert_eq!(addr.addrlen, 16);
    }

    #[test]
    fn test_ngtcp_addr_from_ipv6() {
        let addr = NgtcpAddr::from_ipv6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], 443);
        assert_eq!(addr.addrlen, 28);
    }

    #[test]
    fn test_transport_state() {
        assert_eq!(TransportState::Initial.as_i32(), 0);
    }
}
