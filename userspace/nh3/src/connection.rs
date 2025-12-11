//! HTTP/3 Connection Management
//!
//! This module implements the core HTTP/3 connection handling, providing both
//! a native Rust API and nghttp3-compatible C API.

use crate::constants::*;
use crate::error::{Error, ErrorCode, H3Error, Result};
use crate::frame::*;
use crate::qpack::{QpackDecoder, QpackEncoder};
use crate::stream::{Stream, StreamMap, StreamType};
use crate::types::*;
use crate::{c_int, c_void, size_t, ssize_t};

use std::collections::VecDeque;

// ============================================================================
// Connection Type
// ============================================================================

/// Connection type (client or server)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    Client,
    Server,
}

// ============================================================================
// Connection State
// ============================================================================

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is being established
    Connecting,
    /// Connection is established and ready
    Connected,
    /// Graceful shutdown initiated
    Draining,
    /// Connection is closed
    Closed,
}

// ============================================================================
// Connection Callbacks
// ============================================================================

/// Callback function types (nghttp3 compatible)
pub type AckedStreamDataCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    datalen: size_t,
    user_data: *mut c_void,
) -> c_int;

pub type StreamCloseCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

pub type RecvDataCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    data: *const u8,
    datalen: size_t,
    user_data: *mut c_void,
) -> c_int;

pub type DeferredConsumeCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    consumed: size_t,
    user_data: *mut c_void,
) -> c_int;

pub type BeginHeadersCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int;

pub type RecvHeaderCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    token: i32,
    name: *const nghttp3_rcbuf,
    value: *const nghttp3_rcbuf,
    flags: u8,
    user_data: *mut c_void,
) -> c_int;

pub type EndHeadersCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    fin: c_int,
    user_data: *mut c_void,
) -> c_int;

pub type EndStreamCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    user_data: *mut c_void,
) -> c_int;

pub type StopSendingCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

pub type ResetStreamCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    stream_id: StreamId,
    app_error_code: u64,
    user_data: *mut c_void,
) -> c_int;

pub type ShutdownCallback = extern "C" fn(
    conn: *mut Nghttp3Conn,
    id: i64,
    user_data: *mut c_void,
) -> c_int;

/// Connection callbacks structure
#[derive(Default, Clone)]
pub struct ConnectionCallbacks {
    pub acked_stream_data: Option<AckedStreamDataCallback>,
    pub stream_close: Option<StreamCloseCallback>,
    pub recv_data: Option<RecvDataCallback>,
    pub deferred_consume: Option<DeferredConsumeCallback>,
    pub begin_headers: Option<BeginHeadersCallback>,
    pub recv_header: Option<RecvHeaderCallback>,
    pub end_headers: Option<EndHeadersCallback>,
    pub end_stream: Option<EndStreamCallback>,
    pub stop_sending: Option<StopSendingCallback>,
    pub reset_stream: Option<ResetStreamCallback>,
    pub shutdown: Option<ShutdownCallback>,
}

/// nghttp3 compatible type alias
pub type nghttp3_callbacks = ConnectionCallbacks;

// ============================================================================
// C-compatible Connection Structure
// ============================================================================

/// nghttp3-compatible connection handle
#[repr(C)]
pub struct Nghttp3Conn {
    /// Inner connection wrapped in Arc<Mutex> for thread safety
    inner: *mut Connection,
}

/// C type alias
pub type nghttp3_conn = Nghttp3Conn;

// ============================================================================
// Connection
// ============================================================================

/// HTTP/3 Connection
pub struct Connection {
    /// Connection type
    conn_type: ConnectionType,
    /// Connection state
    state: ConnectionState,
    /// Stream map
    streams: StreamMap,
    /// QPACK encoder
    qpack_encoder: QpackEncoder,
    /// QPACK decoder
    qpack_decoder: QpackDecoder,
    /// Settings
    local_settings: Settings,
    /// Remote settings (received from peer)
    remote_settings: Option<Settings>,
    /// Control stream ID (local)
    local_ctrl_stream_id: Option<StreamId>,
    /// Control stream ID (remote)
    remote_ctrl_stream_id: Option<StreamId>,
    /// QPACK encoder stream ID (local)
    local_qpack_enc_stream_id: Option<StreamId>,
    /// QPACK decoder stream ID (local)
    local_qpack_dec_stream_id: Option<StreamId>,
    /// QPACK encoder stream ID (remote)
    remote_qpack_enc_stream_id: Option<StreamId>,
    /// QPACK decoder stream ID (remote)
    remote_qpack_dec_stream_id: Option<StreamId>,
    /// Callbacks
    callbacks: ConnectionCallbacks,
    /// User data
    user_data: *mut c_void,
    /// Outgoing frames queue
    outgoing_frames: VecDeque<(StreamId, Frame)>,
    /// Settings received flag
    settings_received: bool,
    /// Goaway ID (stream ID or push ID we sent in GOAWAY)
    goaway_id: Option<u64>,
    /// Next push ID
    next_push_id: u64,
    /// Max push ID received from client
    max_push_id: Option<u64>,
}

impl Connection {
    /// Create a new client connection
    pub fn client(settings: &Settings, callbacks: &ConnectionCallbacks, user_data: *mut c_void) -> Self {
        Self {
            conn_type: ConnectionType::Client,
            state: ConnectionState::Connecting,
            streams: StreamMap::client(),
            qpack_encoder: QpackEncoder::new(
                settings.qpack_max_dtable_capacity as usize,
                settings.qpack_blocked_streams as usize,
            ),
            qpack_decoder: QpackDecoder::new(
                settings.qpack_max_dtable_capacity as usize,
                settings.qpack_blocked_streams as usize,
            ),
            local_settings: settings.clone(),
            remote_settings: None,
            local_ctrl_stream_id: None,
            remote_ctrl_stream_id: None,
            local_qpack_enc_stream_id: None,
            local_qpack_dec_stream_id: None,
            remote_qpack_enc_stream_id: None,
            remote_qpack_dec_stream_id: None,
            callbacks: callbacks.clone(),
            user_data,
            outgoing_frames: VecDeque::new(),
            settings_received: false,
            goaway_id: None,
            next_push_id: 0,
            max_push_id: None,
        }
    }
    
    /// Create a new server connection
    pub fn server(settings: &Settings, callbacks: &ConnectionCallbacks, user_data: *mut c_void) -> Self {
        Self {
            conn_type: ConnectionType::Server,
            state: ConnectionState::Connecting,
            streams: StreamMap::server(),
            qpack_encoder: QpackEncoder::new(
                settings.qpack_max_dtable_capacity as usize,
                settings.qpack_blocked_streams as usize,
            ),
            qpack_decoder: QpackDecoder::new(
                settings.qpack_max_dtable_capacity as usize,
                settings.qpack_blocked_streams as usize,
            ),
            local_settings: settings.clone(),
            remote_settings: None,
            local_ctrl_stream_id: None,
            remote_ctrl_stream_id: None,
            local_qpack_enc_stream_id: None,
            local_qpack_dec_stream_id: None,
            remote_qpack_enc_stream_id: None,
            remote_qpack_dec_stream_id: None,
            callbacks: callbacks.clone(),
            user_data,
            outgoing_frames: VecDeque::new(),
            settings_received: false,
            goaway_id: None,
            next_push_id: 0,
            max_push_id: None,
        }
    }
    
    /// Get connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }
    
    /// Check if connection is client
    pub fn is_client(&self) -> bool {
        self.conn_type == ConnectionType::Client
    }
    
    /// Bind the control stream
    pub fn bind_control_stream(&mut self, stream_id: StreamId) -> Result<()> {
        if self.local_ctrl_stream_id.is_some() {
            return Err(ErrorCode::StreamInUse.into());
        }
        
        // Create and register the stream
        let mut stream = Stream::control(stream_id);
        
        // Queue the stream type and SETTINGS frame
        let mut buf = Vec::new();
        encode_varint(&mut buf, stream_type::CONTROL);
        
        let settings_frame = Frame::Settings(if self.is_client() {
            SettingsPayload::default_client()
        } else {
            SettingsPayload::default_server()
        });
        settings_frame.encode(&mut buf)?;
        
        stream.send_buf = buf;
        self.streams.insert(stream);
        self.local_ctrl_stream_id = Some(stream_id);
        
        Ok(())
    }
    
    /// Bind QPACK encoder and decoder streams
    pub fn bind_qpack_streams(
        &mut self,
        encoder_stream_id: StreamId,
        decoder_stream_id: StreamId,
    ) -> Result<()> {
        if self.local_qpack_enc_stream_id.is_some() || self.local_qpack_dec_stream_id.is_some() {
            return Err(ErrorCode::StreamInUse.into());
        }
        
        // Create encoder stream
        let mut enc_stream = Stream::new(encoder_stream_id, StreamType::QpackEncoder);
        let mut enc_buf = Vec::new();
        encode_varint(&mut enc_buf, stream_type::QPACK_ENCODER);
        enc_stream.send_buf = enc_buf;
        self.streams.insert(enc_stream);
        self.local_qpack_enc_stream_id = Some(encoder_stream_id);
        
        // Create decoder stream
        let mut dec_stream = Stream::new(decoder_stream_id, StreamType::QpackDecoder);
        let mut dec_buf = Vec::new();
        encode_varint(&mut dec_buf, stream_type::QPACK_DECODER);
        dec_stream.send_buf = dec_buf;
        self.streams.insert(dec_stream);
        self.local_qpack_dec_stream_id = Some(decoder_stream_id);
        
        // Mark connection as connected once all streams are bound
        if self.local_ctrl_stream_id.is_some() {
            self.state = ConnectionState::Connected;
        }
        
        Ok(())
    }
    
    /// Submit a request (client only)
    pub fn submit_request(
        &mut self,
        headers: &[HeaderField],
        data_provider: Option<&DataProvider>,
    ) -> Result<StreamId> {
        if !self.is_client() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        if self.state != ConnectionState::Connected {
            return Err(ErrorCode::InvalidState.into());
        }
        
        // Allocate a new request stream
        let stream_id = self.streams.alloc_request_stream()?;
        
        // Encode headers using QPACK
        let mut header_block = Vec::new();
        self.qpack_encoder.encode(headers, &mut header_block)?;
        
        // Create HEADERS frame
        let headers_frame = Frame::Headers(HeadersPayload { header_block });
        
        // Queue the frame
        self.outgoing_frames.push_back((stream_id, headers_frame));
        
        // Store headers in stream
        if let Some(stream) = self.streams.get_mut(stream_id) {
            stream.headers = headers.to_vec();
            
            // If no data provider, close send side
            if data_provider.is_none() {
                stream.close_local();
            }
        }
        
        Ok(stream_id)
    }
    
    /// Submit a response (server only)
    pub fn submit_response(
        &mut self,
        stream_id: StreamId,
        headers: &[HeaderField],
        data_provider: Option<&DataProvider>,
    ) -> Result<()> {
        if self.is_client() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        // Encode headers using QPACK
        let mut header_block = Vec::new();
        self.qpack_encoder.encode(headers, &mut header_block)?;
        
        // Create HEADERS frame
        let headers_frame = Frame::Headers(HeadersPayload { header_block });
        
        // Queue the frame
        self.outgoing_frames.push_back((stream_id, headers_frame));
        
        // If no data provider, close send side
        if data_provider.is_none() {
            if let Some(stream) = self.streams.get_mut(stream_id) {
                stream.close_local();
            }
        }
        
        Ok(())
    }
    
    /// Submit data on a stream
    pub fn submit_data(&mut self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<()> {
        let stream = self.streams.get_mut(stream_id)
            .ok_or(ErrorCode::InvalidArgument)?;
        
        if !stream.can_send() {
            return Err(ErrorCode::InvalidState.into());
        }
        
        // Create DATA frame
        let data_frame = Frame::Data(DataPayload { data: data.to_vec() });
        self.outgoing_frames.push_back((stream_id, data_frame));
        
        if fin {
            stream.close_local();
        }
        
        Ok(())
    }
    
    /// Read data from a stream
    pub fn read_stream(&mut self, stream_id: StreamId, buf: &mut [u8]) -> Result<usize> {
        let stream = self.streams.get_mut(stream_id)
            .ok_or(ErrorCode::InvalidArgument)?;
        
        let len = stream.recv_buf.len().min(buf.len());
        buf[..len].copy_from_slice(&stream.recv_buf[..len]);
        stream.recv_buf.drain(..len);
        
        Ok(len)
    }
    
    /// Write data from stream for sending
    pub fn write_stream(&mut self, stream_id: StreamId, buf: &mut [u8]) -> Result<usize> {
        let stream = self.streams.get_mut(stream_id)
            .ok_or(ErrorCode::InvalidArgument)?;
        
        let data = stream.take_send_data(buf.len());
        buf[..data.len()].copy_from_slice(&data);
        
        Ok(data.len())
    }
    
    /// Process incoming data on a stream
    pub fn recv_stream_data(&mut self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<()> {
        // Check if this is a new unidirectional stream (need to read type)
        if StreamMap::is_uni(stream_id) && !self.streams.contains(stream_id) {
            if data.is_empty() {
                return Err(ErrorCode::NoBuf.into());
            }
            
            // Decode stream type
            let (stream_type_code, consumed) = decode_varint(data)?;
            let stream_type = StreamType::from_code(stream_type_code);
            
            // Create stream
            let stream = Stream::new(stream_id, stream_type);
            self.streams.insert(stream);
            
            // Handle stream type
            match stream_type {
                StreamType::Control => {
                    if self.remote_ctrl_stream_id.is_some() {
                        return Err(Error::H3Error(H3Error::StreamCreationError));
                    }
                    self.remote_ctrl_stream_id = Some(stream_id);
                }
                StreamType::QpackEncoder => {
                    if self.remote_qpack_enc_stream_id.is_some() {
                        return Err(Error::H3Error(H3Error::StreamCreationError));
                    }
                    self.remote_qpack_enc_stream_id = Some(stream_id);
                }
                StreamType::QpackDecoder => {
                    if self.remote_qpack_dec_stream_id.is_some() {
                        return Err(Error::H3Error(H3Error::StreamCreationError));
                    }
                    self.remote_qpack_dec_stream_id = Some(stream_id);
                }
                _ => {}
            }
            
            // Process remaining data
            if consumed < data.len() {
                return self.process_stream_data(stream_id, &data[consumed..], fin);
            }
            return Ok(());
        }
        
        // Create request stream if needed (for server)
        if !self.streams.contains(stream_id) && StreamMap::is_bidi(stream_id) {
            let stream = Stream::request(stream_id);
            self.streams.insert(stream);
        }
        
        self.process_stream_data(stream_id, data, fin)
    }
    
    /// Process data on an existing stream
    fn process_stream_data(&mut self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<()> {
        let stream_type = self.streams.get(stream_id)
            .map(|s| s.stream_type)
            .ok_or(ErrorCode::InvalidArgument)?;
        
        match stream_type {
            StreamType::Control => self.process_control_stream(stream_id, data)?,
            StreamType::QpackEncoder => self.process_qpack_encoder_stream(data)?,
            StreamType::QpackDecoder => self.process_qpack_decoder_stream(data)?,
            StreamType::Request => self.process_request_stream(stream_id, data, fin)?,
            StreamType::Push => self.process_push_stream(stream_id, data, fin)?,
            StreamType::Unknown(_) => {
                // Ignore unknown stream types
            }
        }
        
        if fin {
            if let Some(stream) = self.streams.get_mut(stream_id) {
                stream.close_remote();
            }
        }
        
        Ok(())
    }
    
    /// Process control stream data
    fn process_control_stream(&mut self, _stream_id: StreamId, data: &[u8]) -> Result<()> {
        let mut pos = 0;
        
        while pos < data.len() {
            let (frame, consumed) = Frame::decode(&data[pos..])?;
            pos += consumed;
            
            match frame {
                Frame::Settings(payload) => {
                    if self.settings_received {
                        return Err(Error::H3Error(H3Error::FrameUnexpected));
                    }
                    self.settings_received = true;
                    
                    // Parse settings
                    let mut settings = Settings::default();
                    for (id, value) in payload.settings {
                        match id {
                            settings_id::MAX_FIELD_SECTION_SIZE => {
                                settings.max_field_section_size = value;
                            }
                            settings_id::QPACK_MAX_TABLE_CAPACITY => {
                                settings.qpack_max_dtable_capacity = value;
                            }
                            settings_id::QPACK_BLOCKED_STREAMS => {
                                settings.qpack_blocked_streams = value;
                            }
                            settings_id::ENABLE_CONNECT_PROTOCOL => {
                                settings.enable_connect_protocol = value != 0;
                            }
                            settings_id::H3_DATAGRAM => {
                                settings.h3_datagram = value != 0;
                            }
                            _ => {
                                // Unknown settings are ignored
                            }
                        }
                    }
                    self.remote_settings = Some(settings);
                    
                    // Update connection state
                    if self.local_ctrl_stream_id.is_some()
                        && self.local_qpack_enc_stream_id.is_some()
                        && self.local_qpack_dec_stream_id.is_some()
                    {
                        self.state = ConnectionState::Connected;
                    }
                }
                Frame::Goaway(payload) => {
                    self.state = ConnectionState::Draining;
                    
                    // Call shutdown callback
                    if let Some(cb) = self.callbacks.shutdown {
                        let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                        cb(conn_ptr, payload.id as i64, self.user_data);
                    }
                }
                Frame::MaxPushId(payload) => {
                    if self.is_client() {
                        return Err(Error::H3Error(H3Error::FrameUnexpected));
                    }
                    self.max_push_id = Some(payload.push_id);
                }
                Frame::CancelPush(_) => {
                    // Handle push cancellation
                }
                _ => {
                    // Unexpected frame on control stream
                    return Err(Error::H3Error(H3Error::FrameUnexpected));
                }
            }
        }
        
        Ok(())
    }
    
    /// Process QPACK encoder stream data
    fn process_qpack_encoder_stream(&mut self, data: &[u8]) -> Result<()> {
        self.qpack_decoder.process_encoder_instruction(data)?;
        Ok(())
    }
    
    /// Process QPACK decoder stream data
    fn process_qpack_decoder_stream(&mut self, data: &[u8]) -> Result<()> {
        // Process acknowledgments and stream cancellations
        let mut pos = 0;
        while pos < data.len() {
            let first = data[pos];
            
            if (first & 0x80) != 0 {
                // Section Acknowledgment
                let (stream_id, consumed) = decode_prefixed_varint(&data[pos..], 7)?;
                pos += consumed;
                self.qpack_encoder.process_ack(stream_id as i64, 0);
            } else if (first & 0x40) != 0 {
                // Stream Cancellation
                let (_stream_id, consumed) = decode_prefixed_varint(&data[pos..], 6)?;
                pos += consumed;
            } else {
                // Insert Count Increment
                let (_increment, consumed) = decode_prefixed_varint(&data[pos..], 6)?;
                pos += consumed;
            }
        }
        Ok(())
    }
    
    /// Process request stream data
    fn process_request_stream(&mut self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<()> {
        let mut pos = 0;
        
        while pos < data.len() {
            let (frame, consumed) = Frame::decode(&data[pos..])?;
            pos += consumed;
            
            match frame {
                Frame::Headers(payload) => {
                    // Decode headers using QPACK
                    let headers = self.qpack_decoder.decode(&payload.header_block)?;
                    
                    // Call begin_headers callback
                    if let Some(cb) = self.callbacks.begin_headers {
                        let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                        let ret = cb(conn_ptr, stream_id, self.user_data);
                        if ret != 0 {
                            return Err(ErrorCode::CallbackFailure.into());
                        }
                    }
                    
                    // Call recv_header callback for each header
                    if let Some(cb) = self.callbacks.recv_header {
                        let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                        for header in &headers {
                            let name_buf = RcBuf::from_slice(&header.name);
                            let value_buf = RcBuf::from_slice(&header.value);
                            let flags = if header.never_index { 1 } else { 0 };
                            
                            let ret = cb(
                                conn_ptr,
                                stream_id,
                                -1, // token
                                &name_buf,
                                &value_buf,
                                flags,
                                self.user_data,
                            );
                            if ret != 0 {
                                return Err(ErrorCode::CallbackFailure.into());
                            }
                        }
                    }
                    
                    // Call end_headers callback
                    if let Some(cb) = self.callbacks.end_headers {
                        let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                        let fin_flag = if fin && pos >= data.len() { 1 } else { 0 };
                        let ret = cb(conn_ptr, stream_id, fin_flag, self.user_data);
                        if ret != 0 {
                            return Err(ErrorCode::CallbackFailure.into());
                        }
                    }
                    
                    // Store headers in stream
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.headers = headers;
                        stream.headers_received = true;
                    }
                }
                Frame::Data(payload) => {
                    // Mark body started
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.body_started = true;
                        stream.recv_buf.extend_from_slice(&payload.data);
                    }
                    
                    // Call recv_data callback
                    if let Some(cb) = self.callbacks.recv_data {
                        let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                        let ret = cb(
                            conn_ptr,
                            stream_id,
                            payload.data.as_ptr(),
                            payload.data.len(),
                            self.user_data,
                        );
                        if ret != 0 {
                            return Err(ErrorCode::CallbackFailure.into());
                        }
                    }
                }
                _ => {
                    // Other frames are not allowed on request streams
                    return Err(Error::H3Error(H3Error::FrameUnexpected));
                }
            }
        }
        
        // Handle FIN
        if fin {
            if let Some(cb) = self.callbacks.end_stream {
                let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                cb(conn_ptr, stream_id, self.user_data);
            }
        }
        
        Ok(())
    }
    
    /// Process push stream data
    fn process_push_stream(&mut self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<()> {
        // Similar to request stream but for server push
        self.process_request_stream(stream_id, data, fin)
    }
    
    /// Initiate graceful shutdown
    pub fn shutdown(&mut self) -> Result<()> {
        if self.state == ConnectionState::Closed {
            return Err(ErrorCode::InvalidState.into());
        }
        
        self.state = ConnectionState::Draining;
        
        // Send GOAWAY frame
        let goaway_id = if self.is_client() {
            // Client sends highest push ID
            self.next_push_id
        } else {
            // Server sends highest stream ID we will process
            // For simplicity, use the next expected stream ID
            if let Some(stream) = self.streams.request_streams().last() {
                (stream.id + 4) as u64
            } else {
                0
            }
        };
        
        self.goaway_id = Some(goaway_id);
        
        let goaway_frame = Frame::Goaway(GoawayPayload { id: goaway_id });
        
        if let Some(ctrl_id) = self.local_ctrl_stream_id {
            self.outgoing_frames.push_back((ctrl_id, goaway_frame));
        }
        
        Ok(())
    }
    
    /// Close a stream
    pub fn close_stream(&mut self, stream_id: StreamId, app_error_code: u64) -> Result<()> {
        if let Some(stream) = self.streams.get_mut(stream_id) {
            stream.reset();
            
            // Call stream_close callback
            if let Some(cb) = self.callbacks.stream_close {
                let conn_ptr = self as *mut Connection as *mut Nghttp3Conn;
                cb(conn_ptr, stream_id, app_error_code, self.user_data);
            }
        }
        
        Ok(())
    }
    
    /// Get next outgoing frame
    pub fn poll_frame(&mut self) -> Option<(StreamId, Vec<u8>)> {
        if let Some((stream_id, frame)) = self.outgoing_frames.pop_front() {
            let mut buf = Vec::new();
            if frame.encode(&mut buf).is_ok() {
                return Some((stream_id, buf));
            }
        }
        None
    }
    
    /// Check if there are frames to send
    pub fn has_pending_data(&self) -> bool {
        !self.outgoing_frames.is_empty()
            || self.streams.open_streams().any(|s| !s.send_buf.is_empty())
    }
}

// SAFETY: Connection's user_data is a raw pointer managed by the caller
unsafe impl Send for Connection {}

// ============================================================================
// Helper Functions
// ============================================================================

/// Decode a prefixed variable-length integer
fn decode_prefixed_varint(data: &[u8], prefix_bits: u8) -> Result<(usize, usize)> {
    if data.is_empty() {
        return Err(ErrorCode::NoBuf.into());
    }
    
    let max_prefix = (1usize << prefix_bits) - 1;
    let mask = max_prefix as u8;
    
    let mut value = (data[0] & mask) as usize;
    let mut pos = 1;
    
    if value < max_prefix {
        return Ok((value, pos));
    }
    
    let mut shift = 0usize;
    loop {
        if pos >= data.len() {
            return Err(ErrorCode::NoBuf.into());
        }
        
        let b = data[pos] as usize;
        pos += 1;
        
        value += (b & 0x7F) << shift;
        shift += 7;
        
        if (b & 0x80) == 0 {
            break;
        }
    }
    
    Ok((value, pos))
}

// ============================================================================
// C API Functions (nghttp3 compatible)
// ============================================================================

/// Create a new client connection
#[no_mangle]
pub extern "C" fn nghttp3_conn_client_new(
    pconn: *mut *mut nghttp3_conn,
    callbacks: *const nghttp3_callbacks,
    settings: *const nghttp3_settings,
    _mem: *const c_void,
    user_data: *mut c_void,
) -> c_int {
    if pconn.is_null() || callbacks.is_null() || settings.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let settings = unsafe { &*settings };
    let callbacks = unsafe { &*callbacks };
    
    let conn = Box::new(Connection::client(settings, callbacks, user_data));
    let conn_ptr = Box::into_raw(conn);
    
    let handle = Box::new(Nghttp3Conn { inner: conn_ptr });
    
    unsafe {
        *pconn = Box::into_raw(handle);
    }
    
    0
}

/// Create a new server connection
#[no_mangle]
pub extern "C" fn nghttp3_conn_server_new(
    pconn: *mut *mut nghttp3_conn,
    callbacks: *const nghttp3_callbacks,
    settings: *const nghttp3_settings,
    _mem: *const c_void,
    user_data: *mut c_void,
) -> c_int {
    if pconn.is_null() || callbacks.is_null() || settings.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let settings = unsafe { &*settings };
    let callbacks = unsafe { &*callbacks };
    
    let conn = Box::new(Connection::server(settings, callbacks, user_data));
    let conn_ptr = Box::into_raw(conn);
    
    let handle = Box::new(Nghttp3Conn { inner: conn_ptr });
    
    unsafe {
        *pconn = Box::into_raw(handle);
    }
    
    0
}

/// Delete a connection
#[no_mangle]
pub extern "C" fn nghttp3_conn_del(conn: *mut nghttp3_conn) {
    if !conn.is_null() {
        unsafe {
            let handle = Box::from_raw(conn);
            if !handle.inner.is_null() {
                let _ = Box::from_raw(handle.inner);
            }
        }
    }
}

/// Bind control stream
#[no_mangle]
pub extern "C" fn nghttp3_conn_bind_control_stream(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
) -> c_int {
    if conn.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    match inner.bind_control_stream(stream_id) {
        Ok(()) => 0,
        Err(e) => e.code() as c_int,
    }
}

/// Bind QPACK streams
#[no_mangle]
pub extern "C" fn nghttp3_conn_bind_qpack_streams(
    conn: *mut nghttp3_conn,
    qenc_stream_id: StreamId,
    qdec_stream_id: StreamId,
) -> c_int {
    if conn.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    match inner.bind_qpack_streams(qenc_stream_id, qdec_stream_id) {
        Ok(()) => 0,
        Err(e) => e.code() as c_int,
    }
}

/// Read data from stream for sending
#[no_mangle]
pub extern "C" fn nghttp3_conn_writev_stream(
    conn: *mut nghttp3_conn,
    pstream_id: *mut StreamId,
    pfin: *mut c_int,
    vec: *mut nghttp3_vec,
    veccnt: size_t,
) -> ssize_t {
    if conn.is_null() || vec.is_null() || veccnt == 0 {
        return ErrorCode::InvalidArgument as ssize_t;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    
    // Get next frame to send
    if let Some((stream_id, data)) = inner.poll_frame() {
        unsafe {
            if !pstream_id.is_null() {
                *pstream_id = stream_id;
            }
            if !pfin.is_null() {
                *pfin = 0;
            }
            
            // Copy data to vec
            let vec_slice = std::slice::from_raw_parts_mut(vec, veccnt);
            if !vec_slice.is_empty() {
                // Leak the data so it stays valid until consumed
                let leaked = Box::leak(data.into_boxed_slice());
                vec_slice[0].base = leaked.as_mut_ptr();
                vec_slice[0].len = leaked.len();
                return 1;
            }
        }
    }
    
    0 // No data to send
}

/// Acknowledge sent data
#[no_mangle]
pub extern "C" fn nghttp3_conn_add_write_offset(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    n: size_t,
) -> c_int {
    if conn.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    
    // Call acked callback
    if let Some(cb) = inner.callbacks.acked_stream_data {
        let conn_ptr = conn;
        cb(conn_ptr, stream_id, n, inner.user_data);
    }
    
    0
}

/// Receive data on a stream
#[no_mangle]
pub extern "C" fn nghttp3_conn_read_stream(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    data: *const u8,
    datalen: size_t,
    fin: c_int,
) -> ssize_t {
    if conn.is_null() || (datalen > 0 && data.is_null()) {
        return ErrorCode::InvalidArgument as ssize_t;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    let data_slice = if datalen > 0 {
        unsafe { std::slice::from_raw_parts(data, datalen) }
    } else {
        &[]
    };
    
    match inner.recv_stream_data(stream_id, data_slice, fin != 0) {
        Ok(()) => datalen as ssize_t,
        Err(e) => e.code() as ssize_t,
    }
}

/// Submit a request
#[no_mangle]
pub extern "C" fn nghttp3_conn_submit_request(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    nva: *const nghttp3_nv,
    nvlen: size_t,
    dr: *const DataProvider,
    _stream_user_data: *mut c_void,
) -> c_int {
    if conn.is_null() || nva.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    
    // Convert nva to HeaderField
    let headers: Vec<HeaderField> = unsafe {
        std::slice::from_raw_parts(nva, nvlen)
            .iter()
            .map(|nv| {
                let name = std::slice::from_raw_parts(nv.name, nv.namelen).to_vec();
                let value = std::slice::from_raw_parts(nv.value, nv.valuelen).to_vec();
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
    
    // Note: For this API, the caller provides the stream_id
    // We need to register it
    if !inner.streams.contains(stream_id) {
        let stream = Stream::request(stream_id);
        inner.streams.insert(stream);
    }
    
    // Encode headers and queue
    let mut header_block = Vec::new();
    if let Err(e) = inner.qpack_encoder.encode(&headers, &mut header_block) {
        return e.code() as c_int;
    }
    
    let headers_frame = Frame::Headers(HeadersPayload { header_block });
    inner.outgoing_frames.push_back((stream_id, headers_frame));
    
    if data_provider.is_none() {
        if let Some(stream) = inner.streams.get_mut(stream_id) {
            stream.close_local();
        }
    }
    
    0
}

/// Shutdown the connection
#[no_mangle]
pub extern "C" fn nghttp3_conn_shutdown(conn: *mut nghttp3_conn) -> c_int {
    if conn.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    match inner.shutdown() {
        Ok(()) => 0,
        Err(e) => e.code() as c_int,
    }
}

/// Close a stream
#[no_mangle]
pub extern "C" fn nghttp3_conn_close_stream(
    conn: *mut nghttp3_conn,
    stream_id: StreamId,
    app_error_code: u64,
) -> c_int {
    if conn.is_null() {
        return ErrorCode::InvalidArgument as c_int;
    }
    
    let inner = unsafe { &mut *(*conn).inner };
    match inner.close_stream(stream_id, app_error_code) {
        Ok(()) => 0,
        Err(e) => e.code() as c_int,
    }
}

/// Check if connection is client
#[no_mangle]
pub extern "C" fn nghttp3_conn_is_client(conn: *const nghttp3_conn) -> c_int {
    if conn.is_null() {
        return 0;
    }
    
    let inner = unsafe { &*(*conn).inner };
    if inner.is_client() { 1 } else { 0 }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_connection_client_new() {
        let settings = Settings::default();
        let callbacks = ConnectionCallbacks::default();
        let conn = Connection::client(&settings, &callbacks, core::ptr::null_mut());
        
        assert!(conn.is_client());
        assert_eq!(conn.state(), ConnectionState::Connecting);
    }
    
    #[test]
    fn test_connection_server_new() {
        let settings = Settings::default();
        let callbacks = ConnectionCallbacks::default();
        let conn = Connection::server(&settings, &callbacks, core::ptr::null_mut());
        
        assert!(!conn.is_client());
        assert_eq!(conn.state(), ConnectionState::Connecting);
    }
}
