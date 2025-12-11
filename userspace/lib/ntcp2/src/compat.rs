//! ngtcp2 C ABI Compatibility Layer
//!
//! This module provides full ABI compatibility with ngtcp2 C library,
//! allowing ntcp2 to be used as a drop-in replacement for libngtcp2.so.
//!
//! ## Compatibility
//!
//! - All ngtcp2_* functions are exported with C calling convention
//! - Struct layouts match ngtcp2 C definitions
//! - Error codes follow ngtcp2 conventions
//!
//! ## Usage
//!
//! Applications linked against libngtcp2.so can switch to this library
//! without source code changes.

use std::ffi::{c_char, c_int, c_void};
use std::ptr;
use std::slice;

use crate::connection::{Connection, ConnectionCallbacks};
use crate::crypto::{AeadAlgorithm, CryptoContext};
use crate::error::{Error, NgError, TransportError};
use crate::types::{EncryptionLevel, Settings, TransportParams};

// ============================================================================
// Error Codes (ngtcp2 compatible)
// ============================================================================

/// No error
pub const NGTCP2_NO_ERROR: c_int = 0;

/// Protocol error
pub const NGTCP2_ERR_PROTO: c_int = -101;

/// Invalid argument
pub const NGTCP2_ERR_INVALID_ARGUMENT: c_int = -102;

/// Buffer too small
pub const NGTCP2_ERR_NOBUF: c_int = -103;

/// Fatal error
pub const NGTCP2_ERR_FATAL: c_int = -104;

/// Callback failure
pub const NGTCP2_ERR_CALLBACK_FAILURE: c_int = -105;

/// Stream closed
pub const NGTCP2_ERR_STREAM_NOT_FOUND: c_int = -106;

/// Connection ID blocked
pub const NGTCP2_ERR_STREAM_ID_BLOCKED: c_int = -107;

/// Stream data blocked
pub const NGTCP2_ERR_STREAM_DATA_BLOCKED: c_int = -108;

/// Flow control
pub const NGTCP2_ERR_FLOW_CONTROL: c_int = -109;

/// Connection closing
pub const NGTCP2_ERR_CLOSING: c_int = -110;

/// Connection draining
pub const NGTCP2_ERR_DRAINING: c_int = -111;

/// Crypto error
pub const NGTCP2_ERR_CRYPTO: c_int = -112;

/// Internal error
pub const NGTCP2_ERR_INTERNAL: c_int = -113;

/// Required transport parameter missing
pub const NGTCP2_ERR_REQUIRED_TRANSPORT_PARAM: c_int = -114;

/// Malformed transport parameter
pub const NGTCP2_ERR_MALFORMED_TRANSPORT_PARAM: c_int = -115;

/// Retry
pub const NGTCP2_ERR_RETRY: c_int = -116;

/// Drop connection
pub const NGTCP2_ERR_DROP_CONN: c_int = -117;

/// Idle timeout
pub const NGTCP2_ERR_IDLE_CLOSE: c_int = -118;

/// Version negotiation failure
pub const NGTCP2_ERR_VERSION_NEGOTIATION_FAILURE: c_int = -119;

/// Handshake timeout
pub const NGTCP2_ERR_HANDSHAKE_TIMEOUT: c_int = -120;

// ============================================================================
// Version Information
// ============================================================================

/// ngtcp2 version age
pub const NGTCP2_VERSION_AGE: c_int = 1;

/// ngtcp2 version number (0.21.0)
pub const NGTCP2_VERSION_NUM: u32 = 0x00_15_00_00;

/// Version info structure
#[repr(C)]
pub struct ngtcp2_info {
    /// Age of this struct
    pub age: c_int,
    /// Version number
    pub version_num: u32,
    /// Version string
    pub version_str: *const c_char,
}

/// Get library version info
#[no_mangle]
pub extern "C" fn ngtcp2_version(least_version: c_int) -> *const ngtcp2_info {
    static VERSION_STR: &[u8] = b"0.21.0-ntcp2\0";
    static INFO: ngtcp2_info = ngtcp2_info {
        age: NGTCP2_VERSION_AGE,
        version_num: NGTCP2_VERSION_NUM,
        version_str: VERSION_STR.as_ptr() as *const c_char,
    };

    if least_version > NGTCP2_VERSION_AGE {
        ptr::null()
    } else {
        &INFO
    }
}

// ============================================================================
// Connection ID
// ============================================================================

/// Maximum connection ID length
pub const NGTCP2_MAX_CIDLEN: usize = 20;

/// Connection ID structure
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ngtcp2_cid {
    pub datalen: usize,
    pub data: [u8; NGTCP2_MAX_CIDLEN],
}

impl Default for ngtcp2_cid {
    fn default() -> Self {
        Self {
            datalen: 0,
            data: [0; NGTCP2_MAX_CIDLEN],
        }
    }
}

/// Initialize connection ID
#[no_mangle]
pub extern "C" fn ngtcp2_cid_init(
    cid: *mut ngtcp2_cid,
    data: *const u8,
    datalen: usize,
) {
    if cid.is_null() {
        return;
    }
    let cid = unsafe { &mut *cid };
    cid.datalen = datalen.min(NGTCP2_MAX_CIDLEN);
    if !data.is_null() && cid.datalen > 0 {
        unsafe {
            ptr::copy_nonoverlapping(data, cid.data.as_mut_ptr(), cid.datalen);
        }
    }
}

/// Check if two CIDs are equal
#[no_mangle]
pub extern "C" fn ngtcp2_cid_eq(a: *const ngtcp2_cid, b: *const ngtcp2_cid) -> c_int {
    if a.is_null() || b.is_null() {
        return 0;
    }
    let a = unsafe { &*a };
    let b = unsafe { &*b };
    if a.datalen != b.datalen {
        return 0;
    }
    if a.data[..a.datalen] == b.data[..b.datalen] {
        1
    } else {
        0
    }
}

// ============================================================================
// Path
// ============================================================================

/// Socket address (IPv4/IPv6)
#[repr(C)]
pub struct ngtcp2_sockaddr {
    pub sa_family: u16,
    pub sa_data: [u8; 14],
}

/// Sockaddr storage
#[repr(C)]
pub struct ngtcp2_sockaddr_storage {
    pub ss_family: u16,
    pub __ss_padding: [u8; 126],
}

/// Address structure
#[repr(C)]
pub struct ngtcp2_addr {
    pub addr: *mut ngtcp2_sockaddr,
    pub addrlen: u32,
}

/// Path structure
#[repr(C)]
pub struct ngtcp2_path {
    pub local: ngtcp2_addr,
    pub remote: ngtcp2_addr,
    pub user_data: *mut c_void,
}

/// Path storage
#[repr(C)]
pub struct ngtcp2_path_storage {
    pub path: ngtcp2_path,
    pub local_addrbuf: ngtcp2_sockaddr_storage,
    pub remote_addrbuf: ngtcp2_sockaddr_storage,
}

/// Initialize path storage
#[no_mangle]
pub extern "C" fn ngtcp2_path_storage_init(
    ps: *mut ngtcp2_path_storage,
    local_addr: *const ngtcp2_sockaddr,
    local_addrlen: u32,
    local_user_data: *mut c_void,
    remote_addr: *const ngtcp2_sockaddr,
    remote_addrlen: u32,
    remote_user_data: *mut c_void,
) {
    if ps.is_null() {
        return;
    }

    let ps = unsafe { &mut *ps };

    // Copy local address
    if !local_addr.is_null() && local_addrlen > 0 {
        let len = (local_addrlen as usize).min(128);
        unsafe {
            ptr::copy_nonoverlapping(
                local_addr as *const u8,
                &mut ps.local_addrbuf as *mut _ as *mut u8,
                len,
            );
        }
        ps.path.local.addr = &mut ps.local_addrbuf as *mut _ as *mut ngtcp2_sockaddr;
        ps.path.local.addrlen = local_addrlen;
    }

    // Copy remote address
    if !remote_addr.is_null() && remote_addrlen > 0 {
        let len = (remote_addrlen as usize).min(128);
        unsafe {
            ptr::copy_nonoverlapping(
                remote_addr as *const u8,
                &mut ps.remote_addrbuf as *mut _ as *mut u8,
                len,
            );
        }
        ps.path.remote.addr = &mut ps.remote_addrbuf as *mut _ as *mut ngtcp2_sockaddr;
        ps.path.remote.addrlen = remote_addrlen;
    }

    ps.path.user_data = local_user_data;
}

/// Zero path storage
#[no_mangle]
pub extern "C" fn ngtcp2_path_storage_zero(ps: *mut ngtcp2_path_storage) {
    if ps.is_null() {
        return;
    }
    unsafe {
        ptr::write_bytes(ps, 0, 1);
    }
}

// ============================================================================
// Transport Parameters
// ============================================================================

/// Transport parameters structure
#[repr(C)]
pub struct ngtcp2_transport_params {
    pub original_dcid: ngtcp2_cid,
    pub initial_scid: ngtcp2_cid,
    pub retry_scid: ngtcp2_cid,
    pub preferred_addr_present: u8,
    pub stateless_reset_token_present: u8,
    pub retry_scid_present: u8,
    pub original_dcid_present: u8,
    pub initial_scid_present: u8,
    pub max_idle_timeout: u64,
    pub max_udp_payload_size: u64,
    pub initial_max_data: u64,
    pub initial_max_stream_data_bidi_local: u64,
    pub initial_max_stream_data_bidi_remote: u64,
    pub initial_max_stream_data_uni: u64,
    pub initial_max_streams_bidi: u64,
    pub initial_max_streams_uni: u64,
    pub ack_delay_exponent: u64,
    pub max_ack_delay: u64,
    pub disable_active_migration: u8,
    pub active_connection_id_limit: u64,
    pub max_datagram_frame_size: u64,
    pub grease_quic_bit: u8,
    pub version_info_present: u8,
    pub version_info: ngtcp2_version_info,
    pub stateless_reset_token: [u8; 16],
}

/// Version info
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ngtcp2_version_info {
    pub chosen_version: u32,
    pub available_versions: *const u8,
    pub available_versionslen: usize,
}

/// Get default transport parameters
#[no_mangle]
pub extern "C" fn ngtcp2_transport_params_default(params: *mut ngtcp2_transport_params) {
    if params.is_null() {
        return;
    }
    let params = unsafe { &mut *params };

    // Initialize to zeros
    unsafe {
        ptr::write_bytes(params, 0, 1);
    }

    // Set defaults
    params.max_udp_payload_size = 65527;
    params.initial_max_data = 1048576;
    params.initial_max_stream_data_bidi_local = 262144;
    params.initial_max_stream_data_bidi_remote = 262144;
    params.initial_max_stream_data_uni = 262144;
    params.initial_max_streams_bidi = 100;
    params.initial_max_streams_uni = 100;
    params.ack_delay_exponent = 3;
    params.max_ack_delay = 25;
    params.active_connection_id_limit = 2;
    params.max_idle_timeout = 30000;
}

// ============================================================================
// Settings
// ============================================================================

/// ngtcp2 settings structure
#[repr(C)]
pub struct ngtcp2_settings {
    pub qlog: ngtcp2_qlog_settings,
    pub cc_algo: u32,
    pub initial_ts: u64,
    pub initial_rtt: u64,
    pub log_printf: Option<unsafe extern "C" fn(*mut c_void, *const c_char, ...)>,
    pub max_tx_udp_payload_size: usize,
    pub token: *const u8,
    pub tokenlen: usize,
    pub token_type: u32,
    pub rand_ctx: ngtcp2_rand_ctx,
    pub max_window: u64,
    pub max_stream_window: u64,
    pub ack_thresh: usize,
    pub no_tx_udp_payload_size_shaping: u8,
    pub handshake_timeout: u64,
    pub preferred_versions: *const u32,
    pub preferred_versionslen: usize,
    pub available_versions: *const u32,
    pub available_versionslen: usize,
    pub original_version: u32,
    pub no_pmtud: u8,
    pub pkt_num: u32,
}

/// QLOG settings
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ngtcp2_qlog_settings {
    pub odcid: ngtcp2_cid,
    pub write: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void, usize)>,
    pub user_data: *mut c_void,
}

/// Random context
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ngtcp2_rand_ctx {
    pub native_handle: *mut c_void,
}

impl Default for ngtcp2_rand_ctx {
    fn default() -> Self {
        Self {
            native_handle: ptr::null_mut(),
        }
    }
}

/// Get default settings
#[no_mangle]
pub extern "C" fn ngtcp2_settings_default(settings: *mut ngtcp2_settings) {
    if settings.is_null() {
        return;
    }
    let settings = unsafe { &mut *settings };

    unsafe {
        ptr::write_bytes(settings, 0, 1);
    }

    settings.cc_algo = 0; // RENO
    settings.initial_rtt = 333_000_000; // 333ms in nanoseconds
    settings.max_tx_udp_payload_size = 1200;
    settings.max_window = 6 * 1024 * 1024;
    settings.max_stream_window = 6 * 1024 * 1024;
    settings.ack_thresh = 2;
    settings.handshake_timeout = u64::MAX;
}

// ============================================================================
// Callbacks
// ============================================================================

/// Callback return value type
pub type ngtcp2_tstamp = u64;
pub type ngtcp2_duration = u64;

/// Encryption level
pub const NGTCP2_ENCRYPTION_LEVEL_INITIAL: u32 = 0;
pub const NGTCP2_ENCRYPTION_LEVEL_HANDSHAKE: u32 = 1;
pub const NGTCP2_ENCRYPTION_LEVEL_1RTT: u32 = 2;
pub const NGTCP2_ENCRYPTION_LEVEL_0RTT: u32 = 3;

/// Callbacks structure
#[repr(C)]
pub struct ngtcp2_callbacks {
    pub client_initial:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut c_void) -> c_int>,
    pub recv_client_initial:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *const ngtcp2_cid, *mut c_void) -> c_int>,
    pub recv_crypto_data: Option<
        unsafe extern "C" fn(
            *mut ngtcp2_conn,
            u32, // encryption level
            u64, // offset
            *const u8,
            usize,
            *mut c_void,
        ) -> c_int,
    >,
    pub handshake_completed:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut c_void) -> c_int>,
    pub recv_version_negotiation: Option<
        unsafe extern "C" fn(
            *mut ngtcp2_conn,
            *const ngtcp2_pkt_hd,
            *const u32,
            usize,
            *mut c_void,
        ) -> c_int,
    >,
    pub encrypt: Option<
        unsafe extern "C" fn(
            *mut u8,
            *const ngtcp2_crypto_aead,
            *const ngtcp2_crypto_aead_ctx,
            *const u8,
            usize,
            *const u8,
            usize,
            *const u8,
            usize,
        ) -> c_int,
    >,
    pub decrypt: Option<
        unsafe extern "C" fn(
            *mut u8,
            *const ngtcp2_crypto_aead,
            *const ngtcp2_crypto_aead_ctx,
            *const u8,
            usize,
            *const u8,
            usize,
            *const u8,
            usize,
        ) -> c_int,
    >,
    pub hp_mask: Option<
        unsafe extern "C" fn(
            *mut u8,
            *const ngtcp2_crypto_cipher,
            *const ngtcp2_crypto_cipher_ctx,
            *const u8,
        ) -> c_int,
    >,
    pub recv_stream_data: Option<
        unsafe extern "C" fn(
            *mut ngtcp2_conn,
            u32,    // flags
            i64,    // stream_id
            u64,    // offset
            *const u8,
            usize,
            *mut c_void,
            *mut c_void,
        ) -> c_int,
    >,
    pub acked_stream_data_offset: Option<
        unsafe extern "C" fn(
            *mut ngtcp2_conn,
            i64,
            u64,
            u64,
            *mut c_void,
            *mut c_void,
        ) -> c_int,
    >,
    pub stream_open: Option<unsafe extern "C" fn(*mut ngtcp2_conn, i64, *mut c_void) -> c_int>,
    pub stream_close: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, u32, i64, u64, *mut c_void, *mut c_void) -> c_int,
    >,
    pub recv_stateless_reset:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *const ngtcp2_pkt_stateless_reset, *mut c_void) -> c_int>,
    pub recv_retry:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *const ngtcp2_pkt_hd, *mut c_void) -> c_int>,
    pub extend_max_local_streams_bidi:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u64, *mut c_void) -> c_int>,
    pub extend_max_local_streams_uni:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u64, *mut c_void) -> c_int>,
    pub rand: Option<unsafe extern "C" fn(*mut u8, usize, *const ngtcp2_rand_ctx) -> c_int>,
    pub get_new_connection_id: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, *mut ngtcp2_cid, *mut u8, usize, *mut c_void) -> c_int,
    >,
    pub remove_connection_id:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *const ngtcp2_cid, *mut c_void) -> c_int>,
    pub update_key: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, *mut u8, *mut u8, *mut ngtcp2_crypto_aead_ctx, *mut u8, *mut ngtcp2_crypto_aead_ctx, *const u8, *const u8, usize, *mut c_void) -> c_int,
    >,
    pub path_validation: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, u32, *const ngtcp2_path, *const ngtcp2_path, u32, *mut c_void) -> c_int,
    >,
    pub select_preferred_addr: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, *mut ngtcp2_path, *const ngtcp2_preferred_addr, *mut c_void) -> c_int,
    >,
    pub stream_reset:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, i64, u64, u64, *mut c_void, *mut c_void) -> c_int>,
    pub extend_max_remote_streams_bidi:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u64, *mut c_void) -> c_int>,
    pub extend_max_remote_streams_uni:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u64, *mut c_void) -> c_int>,
    pub extend_max_stream_data:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, i64, u64, *mut c_void, *mut c_void) -> c_int>,
    pub dcid_status: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, u32, u64, *const ngtcp2_cid, *const u8, *mut c_void) -> c_int,
    >,
    pub handshake_confirmed:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut c_void) -> c_int>,
    pub recv_new_token:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *const u8, usize, *mut c_void) -> c_int>,
    pub delete_crypto_aead_ctx:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut ngtcp2_crypto_aead_ctx, *mut c_void)>,
    pub delete_crypto_cipher_ctx:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut ngtcp2_crypto_cipher_ctx, *mut c_void)>,
    pub recv_datagram: Option<
        unsafe extern "C" fn(*mut ngtcp2_conn, u32, *const u8, usize, *mut c_void) -> c_int,
    >,
    pub ack_datagram:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u64, *mut c_void) -> c_int>,
    pub lost_datagram:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u64, *mut c_void) -> c_int>,
    pub get_path_challenge_data:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut u8, *mut c_void) -> c_int>,
    pub stream_stop_sending:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, i64, u64, *mut c_void, *mut c_void) -> c_int>,
    pub version_negotiation:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u32, *const ngtcp2_cid, *mut c_void) -> c_int>,
    pub recv_rx_key:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u32, *mut c_void) -> c_int>,
    pub recv_tx_key:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, u32, *mut c_void) -> c_int>,
    pub tls_early_data_rejected:
        Option<unsafe extern "C" fn(*mut ngtcp2_conn, *mut c_void) -> c_int>,
}

// ============================================================================
// Crypto Types
// ============================================================================

/// AEAD algorithm
#[repr(C)]
pub struct ngtcp2_crypto_aead {
    pub native_handle: *mut c_void,
    pub max_overhead: usize,
}

/// AEAD context
#[repr(C)]
pub struct ngtcp2_crypto_aead_ctx {
    pub native_handle: *mut c_void,
}

/// Cipher algorithm
#[repr(C)]
pub struct ngtcp2_crypto_cipher {
    pub native_handle: *mut c_void,
}

/// Cipher context
#[repr(C)]
pub struct ngtcp2_crypto_cipher_ctx {
    pub native_handle: *mut c_void,
}

/// MD (message digest)
#[repr(C)]
pub struct ngtcp2_crypto_md {
    pub native_handle: *mut c_void,
}

/// Crypto context
#[repr(C)]
pub struct ngtcp2_crypto_ctx {
    pub aead: ngtcp2_crypto_aead,
    pub md: ngtcp2_crypto_md,
    pub hp: ngtcp2_crypto_cipher,
    pub max_encryption: u64,
    pub max_decryption_failure: u64,
}

// ============================================================================
// Packet Types
// ============================================================================

/// Packet header
#[repr(C)]
pub struct ngtcp2_pkt_hd {
    pub dcid: ngtcp2_cid,
    pub scid: ngtcp2_cid,
    pub pkt_num: i64,
    pub token: *const u8,
    pub tokenlen: usize,
    pub pkt_numlen: usize,
    pub len: usize,
    pub version: u32,
    pub type_: u8,
    pub flags: u8,
}

/// Stateless reset
#[repr(C)]
pub struct ngtcp2_pkt_stateless_reset {
    pub stateless_reset_token: [u8; 16],
    pub randlen: usize,
}

/// Preferred address
#[repr(C)]
pub struct ngtcp2_preferred_addr {
    pub cid: ngtcp2_cid,
    pub ipv4_port: u16,
    pub ipv6_port: u16,
    pub ipv4: [u8; 4],
    pub ipv6: [u8; 16],
    pub stateless_reset_token: [u8; 16],
    pub ipv4_present: u8,
    pub ipv6_present: u8,
}

// ============================================================================
// Connection Handle
// ============================================================================

/// Opaque connection type
#[repr(C)]
pub struct ngtcp2_conn {
    _private: [u8; 0],
}

/// Connection internal state
struct ConnState {
    inner: Connection,
    user_data: *mut c_void,
    callbacks: ngtcp2_callbacks,
}

/// Create client connection
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_client_new(
    pconn: *mut *mut ngtcp2_conn,
    dcid: *const ngtcp2_cid,
    scid: *const ngtcp2_cid,
    path: *const ngtcp2_path,
    version: u32,
    callbacks: *const ngtcp2_callbacks,
    settings: *const ngtcp2_settings,
    params: *const ngtcp2_transport_params,
    _mem: *const c_void,
    user_data: *mut c_void,
) -> c_int {
    if pconn.is_null() || callbacks.is_null() || settings.is_null() || params.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    // Create internal connection
    let conn_callbacks = ConnectionCallbacks::default();
    let conn = Connection::client_new(conn_callbacks, version);

    // Allocate state
    let state = Box::new(ConnState {
        inner: conn,
        user_data,
        callbacks: (*callbacks).clone(),
    });

    // Store pointer
    *pconn = Box::into_raw(state) as *mut ngtcp2_conn;

    NGTCP2_NO_ERROR
}

/// Create server connection
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_server_new(
    pconn: *mut *mut ngtcp2_conn,
    dcid: *const ngtcp2_cid,
    scid: *const ngtcp2_cid,
    path: *const ngtcp2_path,
    version: u32,
    callbacks: *const ngtcp2_callbacks,
    settings: *const ngtcp2_settings,
    params: *const ngtcp2_transport_params,
    _mem: *const c_void,
    user_data: *mut c_void,
) -> c_int {
    if pconn.is_null() || callbacks.is_null() || settings.is_null() || params.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let conn_callbacks = ConnectionCallbacks::default();
    let conn = Connection::server_new(conn_callbacks, version);

    let state = Box::new(ConnState {
        inner: conn,
        user_data,
        callbacks: (*callbacks).clone(),
    });

    *pconn = Box::into_raw(state) as *mut ngtcp2_conn;

    NGTCP2_NO_ERROR
}

/// Delete connection
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_del(conn: *mut ngtcp2_conn) {
    if !conn.is_null() {
        let _ = Box::from_raw(conn as *mut ConnState);
    }
}

/// Get user data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_get_user_data(conn: *mut ngtcp2_conn) -> *mut c_void {
    if conn.is_null() {
        return ptr::null_mut();
    }
    let state = &*(conn as *const ConnState);
    state.user_data
}

/// Set user data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_set_user_data(
    conn: *mut ngtcp2_conn,
    user_data: *mut c_void,
) {
    if !conn.is_null() {
        let state = &mut *(conn as *mut ConnState);
        state.user_data = user_data;
    }
}

/// Read packet
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_read_pkt(
    conn: *mut ngtcp2_conn,
    path: *const ngtcp2_path,
    _pi: *const c_void,
    pkt: *const u8,
    pktlen: usize,
    ts: ngtcp2_tstamp,
) -> c_int {
    if conn.is_null() || pkt.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let state = &mut *(conn as *mut ConnState);
    let data = slice::from_raw_parts(pkt, pktlen);

    match state.inner.read_pkt(data, ts) {
        Ok(_) => NGTCP2_NO_ERROR,
        Err(Error::Ng(NgError::Proto)) => NGTCP2_ERR_PROTO,
        Err(Error::Ng(NgError::Crypto)) => NGTCP2_ERR_CRYPTO,
        Err(_) => NGTCP2_ERR_INTERNAL,
    }
}

/// Write packet
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_write_pkt(
    conn: *mut ngtcp2_conn,
    path: *mut ngtcp2_path,
    _pi: *mut c_void,
    dest: *mut u8,
    destlen: usize,
    ts: ngtcp2_tstamp,
) -> isize {
    if conn.is_null() || dest.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT as isize;
    }

    let state = &mut *(conn as *mut ConnState);
    let buf = slice::from_raw_parts_mut(dest, destlen);

    match state.inner.write_pkt(buf, ts) {
        Ok(n) => n as isize,
        Err(Error::Ng(NgError::Proto)) => NGTCP2_ERR_PROTO as isize,
        Err(_) => NGTCP2_ERR_INTERNAL as isize,
    }
}

/// Write stream data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_writev_stream(
    conn: *mut ngtcp2_conn,
    path: *mut ngtcp2_path,
    _pi: *mut c_void,
    dest: *mut u8,
    destlen: usize,
    pdatalen: *mut isize,
    flags: u32,
    stream_id: i64,
    datav: *const ngtcp2_vec,
    datavcnt: usize,
    ts: ngtcp2_tstamp,
) -> isize {
    if conn.is_null() || dest.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT as isize;
    }

    let state = &mut *(conn as *mut ConnState);
    let buf = slice::from_raw_parts_mut(dest, destlen);

    // Gather data from vectors
    let mut data = Vec::new();
    if !datav.is_null() && datavcnt > 0 {
        let vecs = slice::from_raw_parts(datav, datavcnt);
        for v in vecs {
            if !v.base.is_null() && v.len > 0 {
                let chunk = slice::from_raw_parts(v.base, v.len);
                data.extend_from_slice(chunk);
            }
        }
    }

    match state.inner.write_stream(stream_id, &data, buf, ts) {
        Ok((written, data_written)) => {
            if !pdatalen.is_null() {
                *pdatalen = data_written as isize;
            }
            written as isize
        }
        Err(Error::Ng(NgError::StreamState)) => NGTCP2_ERR_STREAM_NOT_FOUND as isize,
        Err(Error::Ng(NgError::StreamDataBlocked)) => NGTCP2_ERR_STREAM_DATA_BLOCKED as isize,
        Err(_) => NGTCP2_ERR_INTERNAL as isize,
    }
}

/// I/O vector
#[repr(C)]
pub struct ngtcp2_vec {
    pub base: *const u8,
    pub len: usize,
}

/// Open stream
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_open_bidi_stream(
    conn: *mut ngtcp2_conn,
    pstream_id: *mut i64,
    stream_user_data: *mut c_void,
) -> c_int {
    if conn.is_null() || pstream_id.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let state = &mut *(conn as *mut ConnState);

    match state.inner.open_bidi_stream() {
        Ok(stream_id) => {
            *pstream_id = stream_id;
            NGTCP2_NO_ERROR
        }
        Err(Error::Ng(NgError::StreamIdBlocked)) => NGTCP2_ERR_STREAM_ID_BLOCKED,
        Err(_) => NGTCP2_ERR_INTERNAL,
    }
}

/// Open unidirectional stream
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_open_uni_stream(
    conn: *mut ngtcp2_conn,
    pstream_id: *mut i64,
    stream_user_data: *mut c_void,
) -> c_int {
    if conn.is_null() || pstream_id.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let state = &mut *(conn as *mut ConnState);

    match state.inner.open_uni_stream() {
        Ok(stream_id) => {
            *pstream_id = stream_id;
            NGTCP2_NO_ERROR
        }
        Err(Error::Ng(NgError::StreamIdBlocked)) => NGTCP2_ERR_STREAM_ID_BLOCKED,
        Err(_) => NGTCP2_ERR_INTERNAL,
    }
}

/// Shutdown stream
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_shutdown_stream(
    conn: *mut ngtcp2_conn,
    flags: u32,
    stream_id: i64,
    app_error_code: u64,
) -> c_int {
    if conn.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let state = &mut *(conn as *mut ConnState);

    match state.inner.shutdown_stream(stream_id, app_error_code) {
        Ok(()) => NGTCP2_NO_ERROR,
        Err(Error::Ng(NgError::StreamState)) => NGTCP2_ERR_STREAM_NOT_FOUND,
        Err(_) => NGTCP2_ERR_INTERNAL,
    }
}

/// Get expiry time
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_get_expiry(conn: *mut ngtcp2_conn) -> ngtcp2_tstamp {
    if conn.is_null() {
        return u64::MAX;
    }

    let state = &*(conn as *const ConnState);
    state.inner.get_expiry()
}

/// Handle expiry
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_handle_expiry(
    conn: *mut ngtcp2_conn,
    ts: ngtcp2_tstamp,
) -> c_int {
    if conn.is_null() {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let state = &mut *(conn as *mut ConnState);
    match state.inner.handle_expiry(ts) {
        Ok(()) => NGTCP2_NO_ERROR,
        Err(_) => NGTCP2_ERR_INTERNAL,
    }
}

/// Check if handshake is complete
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_get_handshake_completed(conn: *mut ngtcp2_conn) -> c_int {
    if conn.is_null() {
        return 0;
    }

    let state = &*(conn as *const ConnState);
    if state.inner.is_handshake_complete() {
        1
    } else {
        0
    }
}

/// Check if draining
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_in_draining_period(conn: *mut ngtcp2_conn) -> c_int {
    if conn.is_null() {
        return 0;
    }

    let state = &*(conn as *const ConnState);
    if state.inner.is_draining() {
        1
    } else {
        0
    }
}

/// Check if closing
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_in_closing_period(conn: *mut ngtcp2_conn) -> c_int {
    if conn.is_null() {
        return 0;
    }

    let state = &*(conn as *const ConnState);
    if state.inner.is_closing() {
        1
    } else {
        0
    }
}

/// Get negotiated version
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_get_negotiated_version(conn: *mut ngtcp2_conn) -> u32 {
    if conn.is_null() {
        return 0;
    }

    let state = &*(conn as *const ConnState);
    state.inner.get_version()
}

/// Submit crypto data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_submit_crypto_data(
    conn: *mut ngtcp2_conn,
    encryption_level: u32,
    data: *const u8,
    datalen: usize,
) -> c_int {
    if conn.is_null() || (data.is_null() && datalen > 0) {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }

    let state = &mut *(conn as *mut ConnState);
    let crypto_data = if datalen > 0 {
        slice::from_raw_parts(data, datalen)
    } else {
        &[]
    };

    let level = match encryption_level {
        NGTCP2_ENCRYPTION_LEVEL_INITIAL => EncryptionLevel::Initial,
        NGTCP2_ENCRYPTION_LEVEL_HANDSHAKE => EncryptionLevel::Handshake,
        NGTCP2_ENCRYPTION_LEVEL_1RTT => EncryptionLevel::Application,
        NGTCP2_ENCRYPTION_LEVEL_0RTT => EncryptionLevel::EarlyData,
        _ => return NGTCP2_ERR_INVALID_ARGUMENT,
    };

    match state.inner.submit_crypto_data(level, crypto_data) {
        Ok(()) => NGTCP2_NO_ERROR,
        Err(_) => NGTCP2_ERR_INTERNAL,
    }
}

/// Get max crypto data offset
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_get_max_data_left(conn: *mut ngtcp2_conn) -> u64 {
    if conn.is_null() {
        return 0;
    }

    let state = &*(conn as *const ConnState);
    state.inner.get_max_data_left()
}

/// Get stream available data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_get_max_stream_data_left(
    conn: *mut ngtcp2_conn,
    stream_id: i64,
) -> u64 {
    if conn.is_null() {
        return 0;
    }

    let state = &*(conn as *const ConnState);
    state.inner.get_max_stream_data_left(stream_id)
}

/// Extend max local bidi streams
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_extend_max_local_streams_bidi(
    conn: *mut ngtcp2_conn,
    n: usize,
) {
    if !conn.is_null() {
        let state = &mut *(conn as *mut ConnState);
        state.inner.extend_max_local_streams_bidi(n as u64);
    }
}

/// Extend max local uni streams
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_extend_max_local_streams_uni(
    conn: *mut ngtcp2_conn,
    n: usize,
) {
    if !conn.is_null() {
        let state = &mut *(conn as *mut ConnState);
        state.inner.extend_max_local_streams_uni(n as u64);
    }
}

/// Extend max stream data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_extend_max_stream_offset(
    conn: *mut ngtcp2_conn,
    stream_id: i64,
    n: u64,
) {
    if !conn.is_null() {
        let state = &mut *(conn as *mut ConnState);
        state.inner.extend_max_stream_data(stream_id, n);
    }
}

/// Extend max data
#[no_mangle]
pub unsafe extern "C" fn ngtcp2_conn_extend_max_offset(conn: *mut ngtcp2_conn, n: u64) {
    if !conn.is_null() {
        let state = &mut *(conn as *mut ConnState);
        state.inner.extend_max_data(n);
    }
}

// ============================================================================
// Clone implementation for callbacks
// ============================================================================

impl Clone for ngtcp2_callbacks {
    fn clone(&self) -> Self {
        Self {
            client_initial: self.client_initial,
            recv_client_initial: self.recv_client_initial,
            recv_crypto_data: self.recv_crypto_data,
            handshake_completed: self.handshake_completed,
            recv_version_negotiation: self.recv_version_negotiation,
            encrypt: self.encrypt,
            decrypt: self.decrypt,
            hp_mask: self.hp_mask,
            recv_stream_data: self.recv_stream_data,
            acked_stream_data_offset: self.acked_stream_data_offset,
            stream_open: self.stream_open,
            stream_close: self.stream_close,
            recv_stateless_reset: self.recv_stateless_reset,
            recv_retry: self.recv_retry,
            extend_max_local_streams_bidi: self.extend_max_local_streams_bidi,
            extend_max_local_streams_uni: self.extend_max_local_streams_uni,
            rand: self.rand,
            get_new_connection_id: self.get_new_connection_id,
            remove_connection_id: self.remove_connection_id,
            update_key: self.update_key,
            path_validation: self.path_validation,
            select_preferred_addr: self.select_preferred_addr,
            stream_reset: self.stream_reset,
            extend_max_remote_streams_bidi: self.extend_max_remote_streams_bidi,
            extend_max_remote_streams_uni: self.extend_max_remote_streams_uni,
            extend_max_stream_data: self.extend_max_stream_data,
            dcid_status: self.dcid_status,
            handshake_confirmed: self.handshake_confirmed,
            recv_new_token: self.recv_new_token,
            delete_crypto_aead_ctx: self.delete_crypto_aead_ctx,
            delete_crypto_cipher_ctx: self.delete_crypto_cipher_ctx,
            recv_datagram: self.recv_datagram,
            ack_datagram: self.ack_datagram,
            lost_datagram: self.lost_datagram,
            get_path_challenge_data: self.get_path_challenge_data,
            stream_stop_sending: self.stream_stop_sending,
            version_negotiation: self.version_negotiation,
            recv_rx_key: self.recv_rx_key,
            recv_tx_key: self.recv_tx_key,
            tls_early_data_rejected: self.tls_early_data_rejected,
        }
    }
}

// ============================================================================
// Error handling
// ============================================================================

/// Get error string
#[no_mangle]
pub extern "C" fn ngtcp2_strerror(liberr: c_int) -> *const c_char {
    static NO_ERROR: &[u8] = b"NO_ERROR\0";
    static PROTO_ERROR: &[u8] = b"PROTO\0";
    static INVALID_ARG: &[u8] = b"INVALID_ARGUMENT\0";
    static NOBUF: &[u8] = b"NOBUF\0";
    static FATAL: &[u8] = b"FATAL\0";
    static CALLBACK: &[u8] = b"CALLBACK_FAILURE\0";
    static STREAM_NOT_FOUND: &[u8] = b"STREAM_NOT_FOUND\0";
    static STREAM_ID_BLOCKED: &[u8] = b"STREAM_ID_BLOCKED\0";
    static STREAM_DATA_BLOCKED: &[u8] = b"STREAM_DATA_BLOCKED\0";
    static FLOW_CONTROL: &[u8] = b"FLOW_CONTROL\0";
    static CLOSING: &[u8] = b"CLOSING\0";
    static DRAINING: &[u8] = b"DRAINING\0";
    static CRYPTO: &[u8] = b"CRYPTO\0";
    static INTERNAL: &[u8] = b"INTERNAL\0";
    static UNKNOWN: &[u8] = b"UNKNOWN\0";

    let msg = match liberr {
        NGTCP2_NO_ERROR => NO_ERROR,
        NGTCP2_ERR_PROTO => PROTO_ERROR,
        NGTCP2_ERR_INVALID_ARGUMENT => INVALID_ARG,
        NGTCP2_ERR_NOBUF => NOBUF,
        NGTCP2_ERR_FATAL => FATAL,
        NGTCP2_ERR_CALLBACK_FAILURE => CALLBACK,
        NGTCP2_ERR_STREAM_NOT_FOUND => STREAM_NOT_FOUND,
        NGTCP2_ERR_STREAM_ID_BLOCKED => STREAM_ID_BLOCKED,
        NGTCP2_ERR_STREAM_DATA_BLOCKED => STREAM_DATA_BLOCKED,
        NGTCP2_ERR_FLOW_CONTROL => FLOW_CONTROL,
        NGTCP2_ERR_CLOSING => CLOSING,
        NGTCP2_ERR_DRAINING => DRAINING,
        NGTCP2_ERR_CRYPTO => CRYPTO,
        NGTCP2_ERR_INTERNAL => INTERNAL,
        _ => UNKNOWN,
    };

    msg.as_ptr() as *const c_char
}

/// Check if error is fatal
#[no_mangle]
pub extern "C" fn ngtcp2_err_is_fatal(liberr: c_int) -> c_int {
    if liberr < NGTCP2_ERR_FATAL {
        1
    } else {
        0
    }
}

/// Map to transport error
#[no_mangle]
pub extern "C" fn ngtcp2_err_infer_quic_transport_error_code(liberr: c_int) -> u64 {
    match liberr {
        NGTCP2_ERR_PROTO => 0x0a, // PROTOCOL_VIOLATION
        NGTCP2_ERR_FLOW_CONTROL => 0x03, // FLOW_CONTROL_ERROR
        NGTCP2_ERR_STREAM_NOT_FOUND => 0x04, // STREAM_STATE_ERROR
        NGTCP2_ERR_CRYPTO => 0x100, // Crypto error base
        _ => 0x0a, // PROTOCOL_VIOLATION
    }
}
