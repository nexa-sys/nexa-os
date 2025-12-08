//! HTTP/2 Session Management
//!
//! This module implements the core HTTP/2 session handling, providing both
//! a native Rust API and nghttp2-compatible C API.

use crate::types::*;
use crate::error::{Error, Result, ErrorCode, NgError};
use crate::frame::*;
use crate::hpack::{Hpack, HpackEncoder, HpackDecoder, HeaderField};
use crate::stream::{Stream, StreamMap, StreamState};
use crate::flow_control::FlowControl;
use crate::priority::PriorityTree;
use crate::constants::*;
use crate::{c_int, c_void, size_t, ssize_t, NGHTTP2_CLIENT_MAGIC};

use std::collections::VecDeque;
use parking_lot::Mutex;
use std::sync::Arc;

// ============================================================================
// Session Type
// ============================================================================

/// Session type (client or server)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Client,
    Server,
}

// ============================================================================
// Session Callbacks
// ============================================================================

/// Callback function types (nghttp2 compatible)
pub type SendCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    data: *const u8,
    length: size_t,
    flags: c_int,
    user_data: *mut c_void,
) -> ssize_t;

pub type RecvCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    buf: *mut u8,
    length: size_t,
    flags: c_int,
    user_data: *mut c_void,
) -> ssize_t;

pub type OnFrameRecvCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    user_data: *mut c_void,
) -> c_int;

pub type OnFrameSendCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    user_data: *mut c_void,
) -> c_int;

pub type OnStreamCloseCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    stream_id: i32,
    error_code: u32,
    user_data: *mut c_void,
) -> c_int;

pub type OnDataChunkRecvCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    flags: u8,
    stream_id: i32,
    data: *const u8,
    len: size_t,
    user_data: *mut c_void,
) -> c_int;

pub type OnHeaderCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    name: *const u8,
    namelen: size_t,
    value: *const u8,
    valuelen: size_t,
    flags: u8,
    user_data: *mut c_void,
) -> c_int;

pub type OnBeginHeadersCallback = extern "C" fn(
    session: *mut NgHttp2Session,
    frame: *const NgHttp2Frame,
    user_data: *mut c_void,
) -> c_int;

/// Session callbacks structure
#[derive(Default)]
pub struct SessionCallbacks {
    pub send_callback: Option<SendCallback>,
    pub recv_callback: Option<RecvCallback>,
    pub on_frame_recv_callback: Option<OnFrameRecvCallback>,
    pub on_frame_send_callback: Option<OnFrameSendCallback>,
    pub on_stream_close_callback: Option<OnStreamCloseCallback>,
    pub on_data_chunk_recv_callback: Option<OnDataChunkRecvCallback>,
    pub on_header_callback: Option<OnHeaderCallback>,
    pub on_begin_headers_callback: Option<OnBeginHeadersCallback>,
}

impl SessionCallbacks {
    pub fn new() -> Self {
        Self::default()
    }
}

// ============================================================================
// Session Settings
// ============================================================================

/// Session settings
#[derive(Debug, Clone)]
pub struct SessionSettings {
    pub header_table_size: u32,
    pub enable_push: bool,
    pub max_concurrent_streams: u32,
    pub initial_window_size: u32,
    pub max_frame_size: u32,
    pub max_header_list_size: u32,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            enable_push: true,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            initial_window_size: DEFAULT_INITIAL_WINDOW_SIZE,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_header_list_size: buffer::DEFAULT_MAX_HEADER_LIST_SIZE as u32,
        }
    }
}

// ============================================================================
// Session Inner State
// ============================================================================

pub(crate) struct SessionInner {
    /// Session type
    pub(crate) session_type: SessionType,
    /// Stream management
    pub(crate) streams: StreamMap,
    /// HPACK codec
    pub(crate) hpack: Hpack,
    /// Flow control
    pub(crate) flow_control: FlowControl,
    /// Priority tree
    pub(crate) priority: PriorityTree,
    /// Local settings
    pub(crate) local_settings: SessionSettings,
    /// Remote settings
    pub(crate) remote_settings: SessionSettings,
    /// Pending settings ACK
    pub(crate) pending_settings: bool,
    /// Output buffer
    pub(crate) send_buffer: VecDeque<u8>,
    /// Input buffer
    pub(crate) recv_buffer: Vec<u8>,
    /// Last received stream ID
    pub(crate) last_recv_stream_id: StreamId,
    /// Last sent stream ID
    pub(crate) last_sent_stream_id: StreamId,
    /// GOAWAY sent
    pub(crate) goaway_sent: bool,
    /// GOAWAY received
    pub(crate) goaway_received: bool,
    /// Last stream ID in GOAWAY
    pub(crate) goaway_last_stream_id: StreamId,
    /// Connection preface sent
    pub(crate) preface_sent: bool,
    /// Connection preface received
    pub(crate) preface_received: bool,
    /// Frame parser
    pub(crate) frame_parser: FrameParser,
    /// Frame builder
    pub(crate) frame_builder: FrameBuilder,
    /// Callbacks
    pub(crate) callbacks: SessionCallbacks,
    /// User data
    pub(crate) user_data: *mut c_void,
}

// ============================================================================
// Session
// ============================================================================

/// HTTP/2 session
pub struct Session {
    pub(crate) inner: Mutex<SessionInner>,
}

impl Session {
    /// Create a new client session
    pub fn client(callbacks: SessionCallbacks, user_data: *mut c_void) -> Self {
        Self::new(SessionType::Client, callbacks, user_data)
    }

    /// Create a new server session
    pub fn server(callbacks: SessionCallbacks, user_data: *mut c_void) -> Self {
        Self::new(SessionType::Server, callbacks, user_data)
    }

    fn new(session_type: SessionType, callbacks: SessionCallbacks, user_data: *mut c_void) -> Self {
        let is_client = session_type == SessionType::Client;
        let settings = SessionSettings::default();

        Self {
            inner: Mutex::new(SessionInner {
                session_type,
                streams: StreamMap::new(is_client, settings.initial_window_size as i32),
                hpack: Hpack::new(settings.header_table_size as usize),
                flow_control: FlowControl::new(),
                priority: PriorityTree::new(),
                local_settings: settings.clone(),
                remote_settings: settings,
                pending_settings: false,
                send_buffer: VecDeque::new(),
                recv_buffer: Vec::new(),
                last_recv_stream_id: 0,
                last_sent_stream_id: 0,
                goaway_sent: false,
                goaway_received: false,
                goaway_last_stream_id: 0,
                preface_sent: false,
                preface_received: false,
                frame_parser: FrameParser::new(DEFAULT_MAX_FRAME_SIZE),
                frame_builder: FrameBuilder::new(DEFAULT_MAX_FRAME_SIZE),
                callbacks,
                user_data,
            }),
        }
    }

    /// Submit a request (client-side)
    pub fn submit_request(
        &self,
        priority: Option<&PrioritySpec>,
        headers: &[HeaderField],
        data_provider: Option<&DataProvider>,
    ) -> Result<StreamId> {
        let mut inner = self.inner.lock();

        if inner.session_type != SessionType::Client {
            return Err(Error::InvalidState("submit_request is for clients only"));
        }

        // Get next stream ID
        let stream_id = inner.streams.next_stream_id();

        // Create stream
        let stream = inner.streams.create(stream_id)?;
        stream.open()?;
        stream.request_headers = headers.to_vec();

        // Add to priority tree
        if let Some(pri) = priority {
            inner.priority.add_with_spec(stream_id, pri);
        } else {
            inner.priority.add(stream_id);
        }

        // Encode headers
        let mut header_block = Vec::new();
        inner.hpack.encode(headers, &mut header_block)?;

        // Create HEADERS frame
        let end_stream = data_provider.is_none();
        let frame = inner.frame_builder.headers(
            stream_id,
            &header_block,
            end_stream,
            true, // END_HEADERS
            priority.cloned(),
        )?;

        // Serialize and queue
        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::Headers(frame), &mut buf)?;
        inner.send_buffer.extend(buf);

        if end_stream {
            if let Some(s) = inner.streams.get_mut(stream_id) {
                s.close_local()?;
            }
        }

        inner.last_sent_stream_id = stream_id;

        Ok(stream_id)
    }

    /// Submit a response (server-side)
    pub fn submit_response(
        &self,
        stream_id: StreamId,
        headers: &[HeaderField],
        data_provider: Option<&DataProvider>,
    ) -> Result<()> {
        let mut inner = self.inner.lock();

        if inner.session_type != SessionType::Server {
            return Err(Error::InvalidState("submit_response is for servers only"));
        }

        let stream = inner.streams.get_mut(stream_id)
            .ok_or(Error::StreamNotFound(stream_id))?;

        stream.response_headers = headers.to_vec();

        // Encode headers
        let mut header_block = Vec::new();
        inner.hpack.encode(headers, &mut header_block)?;

        // Create HEADERS frame
        let end_stream = data_provider.is_none();
        let frame = inner.frame_builder.headers(
            stream_id,
            &header_block,
            end_stream,
            true,
            None,
        )?;

        // Serialize and queue
        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::Headers(frame), &mut buf)?;
        inner.send_buffer.extend(buf);

        if end_stream {
            if let Some(s) = inner.streams.get_mut(stream_id) {
                s.close_local()?;
            }
        }

        Ok(())
    }

    /// Submit data for a stream
    pub fn submit_data(&self, stream_id: StreamId, data: &[u8], end_stream: bool) -> Result<()> {
        let mut inner = self.inner.lock();

        let stream = inner.streams.get_mut(stream_id)
            .ok_or(Error::StreamNotFound(stream_id))?;

        if !stream.state.can_send() {
            return Err(Error::InvalidState("stream cannot send"));
        }

        // Create DATA frame
        let frame = inner.frame_builder.data(stream_id, data, end_stream)?;

        // Serialize and queue
        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::Data(frame), &mut buf)?;
        inner.send_buffer.extend(buf);

        // Update flow control
        inner.flow_control.consume_send(data.len() as i32)?;
        if let Some(s) = inner.streams.get_mut(stream_id) {
            s.consume_send_window(data.len() as i32)?;
            if end_stream {
                s.close_local()?;
            }
        }

        Ok(())
    }

    /// Submit SETTINGS frame
    pub fn submit_settings(&self, entries: &[SettingsEntry]) -> Result<()> {
        let mut inner = self.inner.lock();

        let frame = FrameBuilder::settings(entries, false);

        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::Settings(frame), &mut buf)?;
        inner.send_buffer.extend(buf);
        inner.pending_settings = true;

        Ok(())
    }

    /// Submit PING frame
    pub fn submit_ping(&self, opaque_data: [u8; 8]) -> Result<()> {
        let mut inner = self.inner.lock();

        let frame = FrameBuilder::ping(opaque_data, false);

        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::Ping(frame), &mut buf)?;
        inner.send_buffer.extend(buf);

        Ok(())
    }

    /// Submit GOAWAY frame
    pub fn submit_goaway(&self, error_code: ErrorCode, debug_data: &[u8]) -> Result<()> {
        let mut inner = self.inner.lock();

        let last_stream_id = inner.last_recv_stream_id;
        let frame = FrameBuilder::goaway(last_stream_id, error_code, debug_data);

        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::Goaway(frame), &mut buf)?;
        inner.send_buffer.extend(buf);
        inner.goaway_sent = true;
        inner.goaway_last_stream_id = last_stream_id;

        Ok(())
    }

    /// Submit RST_STREAM frame
    pub fn submit_rst_stream(&self, stream_id: StreamId, error_code: ErrorCode) -> Result<()> {
        let mut inner = self.inner.lock();

        let frame = FrameBuilder::rst_stream(stream_id, error_code);

        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::RstStream(frame), &mut buf)?;
        inner.send_buffer.extend(buf);

        // Reset the stream
        if let Some(stream) = inner.streams.get_mut(stream_id) {
            stream.reset(error_code);
        }

        Ok(())
    }

    /// Submit WINDOW_UPDATE frame
    pub fn submit_window_update(&self, stream_id: StreamId, increment: u32) -> Result<()> {
        let mut inner = self.inner.lock();

        let frame = FrameBuilder::window_update(stream_id, increment)?;

        let mut buf = Vec::new();
        FrameSerializer::serialize(&Frame::WindowUpdate(frame), &mut buf)?;
        inner.send_buffer.extend(buf);

        Ok(())
    }

    /// Get data to send
    pub fn mem_send(&self) -> Vec<u8> {
        let mut inner = self.inner.lock();

        // First time: send connection preface for client
        if !inner.preface_sent && inner.session_type == SessionType::Client {
            let mut preface = NGHTTP2_CLIENT_MAGIC.to_vec();

            // Send initial SETTINGS
            let settings_entries = vec![
                SettingsEntry {
                    settings_id: settings_id::INITIAL_WINDOW_SIZE as i32,
                    value: inner.local_settings.initial_window_size,
                },
                SettingsEntry {
                    settings_id: settings_id::MAX_CONCURRENT_STREAMS as i32,
                    value: inner.local_settings.max_concurrent_streams,
                },
            ];
            let frame = FrameBuilder::settings(&settings_entries, false);
            let mut buf = Vec::new();
            if FrameSerializer::serialize(&Frame::Settings(frame), &mut buf).is_ok() {
                preface.extend(buf);
            }

            inner.preface_sent = true;
            inner.pending_settings = true;

            // Add any pending data
            preface.extend(inner.send_buffer.drain(..));
            return preface;
        }

        // Server sends SETTINGS as first frame
        if !inner.preface_sent && inner.session_type == SessionType::Server {
            let settings_entries = vec![
                SettingsEntry {
                    settings_id: settings_id::INITIAL_WINDOW_SIZE as i32,
                    value: inner.local_settings.initial_window_size,
                },
                SettingsEntry {
                    settings_id: settings_id::MAX_CONCURRENT_STREAMS as i32,
                    value: inner.local_settings.max_concurrent_streams,
                },
            ];
            let frame = FrameBuilder::settings(&settings_entries, false);
            let mut buf = Vec::new();
            if FrameSerializer::serialize(&Frame::Settings(frame), &mut buf).is_ok() {
                inner.send_buffer.extend(buf.iter().rev());
                for b in buf.into_iter().rev() {
                    inner.send_buffer.push_front(b);
                }
            }
            inner.preface_sent = true;
            inner.pending_settings = true;
        }

        inner.send_buffer.drain(..).collect()
    }

    /// Process received data
    pub fn mem_recv(&self, data: &[u8]) -> Result<usize> {
        let mut inner = self.inner.lock();

        inner.recv_buffer.extend_from_slice(data);
        let mut total_consumed = 0;

        // Client preface check for server
        if !inner.preface_received && inner.session_type == SessionType::Server {
            if inner.recv_buffer.len() >= NGHTTP2_CLIENT_MAGIC.len() {
                if &inner.recv_buffer[..NGHTTP2_CLIENT_MAGIC.len()] != NGHTTP2_CLIENT_MAGIC {
                    return Err(Error::Library(NgError::BadClientMagic));
                }
                inner.recv_buffer.drain(..NGHTTP2_CLIENT_MAGIC.len());
                inner.preface_received = true;
                total_consumed += NGHTTP2_CLIENT_MAGIC.len();
            } else {
                return Ok(0);
            }
        }

        // Parse frames
        loop {
            if inner.recv_buffer.len() < FRAME_HEADER_LENGTH {
                break;
            }

            match inner.frame_parser.parse(&inner.recv_buffer) {
                Ok((frame, consumed)) => {
                    inner.recv_buffer.drain(..consumed);
                    total_consumed += consumed;

                    // Process the frame
                    Self::process_frame_inner(&mut inner, frame)?;
                }
                Err(Error::BufferTooSmall) => break,
                Err(e) => return Err(e),
            }
        }

        // Client preface is implicit after receiving SETTINGS
        if !inner.preface_received && inner.session_type == SessionType::Client {
            inner.preface_received = true;
        }

        Ok(total_consumed)
    }

    fn process_frame_inner(inner: &mut SessionInner, frame: Frame) -> Result<()> {
        match frame {
            Frame::Data(f) => {
                let stream_id = f.header.stream_id;
                
                // Update flow control
                inner.flow_control.consume_recv(f.data.len() as i32)?;
                
                if let Some(stream) = inner.streams.get_mut(stream_id) {
                    stream.consume_recv_window(f.data.len() as i32)?;
                    stream.recv_buffer.extend_from_slice(&f.data);
                    
                    if f.header.flags.end_stream() {
                        stream.close_remote()?;
                    }
                }

                // Send WINDOW_UPDATE if needed
                if let Some(increment) = inner.flow_control.should_send_window_update() {
                    let frame = FrameBuilder::window_update(0, increment as u32)?;
                    let mut buf = Vec::new();
                    FrameSerializer::serialize(&Frame::WindowUpdate(frame), &mut buf)?;
                    inner.send_buffer.extend(buf);
                    inner.flow_control.window_update_sent();
                }
            }
            Frame::Headers(f) => {
                let stream_id = f.header.stream_id;
                
                // Decode headers
                let headers = inner.hpack.decode(&f.header_block)?;
                
                // Get or create stream
                let stream = inner.streams.get_or_create(stream_id)?;
                
                if stream.state == StreamState::Idle {
                    stream.open()?;
                }
                
                if inner.session_type == SessionType::Server {
                    stream.request_headers = headers;
                } else {
                    stream.response_headers = headers;
                }
                
                // Handle priority
                if let Some(pri) = f.priority {
                    inner.priority.add_with_spec(stream_id, &pri);
                }
                
                if f.header.flags.end_stream() {
                    stream.close_remote()?;
                }
                
                inner.last_recv_stream_id = stream_id;
            }
            Frame::Priority(f) => {
                inner.priority.update(f.header.stream_id, &f.priority);
            }
            Frame::RstStream(f) => {
                if let Some(stream) = inner.streams.get_mut(f.header.stream_id) {
                    stream.reset(f.error_code);
                }
            }
            Frame::Settings(f) => {
                if f.header.flags.ack() {
                    // SETTINGS ACK received
                    inner.pending_settings = false;
                } else {
                    // Apply remote settings
                    for entry in &f.entries {
                        match entry.settings_id as u16 {
                            settings_id::HEADER_TABLE_SIZE => {
                                inner.remote_settings.header_table_size = entry.value;
                                inner.hpack.encoder().set_max_table_size(entry.value as usize);
                            }
                            settings_id::ENABLE_PUSH => {
                                inner.remote_settings.enable_push = entry.value != 0;
                            }
                            settings_id::MAX_CONCURRENT_STREAMS => {
                                inner.remote_settings.max_concurrent_streams = entry.value;
                                inner.streams.set_max_concurrent_remote(entry.value);
                            }
                            settings_id::INITIAL_WINDOW_SIZE => {
                                let delta = entry.value as i32 - inner.remote_settings.initial_window_size as i32;
                                inner.remote_settings.initial_window_size = entry.value;
                                inner.streams.set_initial_window_size(entry.value as i32)?;
                            }
                            settings_id::MAX_FRAME_SIZE => {
                                inner.remote_settings.max_frame_size = entry.value;
                            }
                            settings_id::MAX_HEADER_LIST_SIZE => {
                                inner.remote_settings.max_header_list_size = entry.value;
                            }
                            _ => {} // Ignore unknown settings
                        }
                    }

                    // Send SETTINGS ACK
                    let ack_frame = FrameBuilder::settings(&[], true);
                    let mut buf = Vec::new();
                    FrameSerializer::serialize(&Frame::Settings(ack_frame), &mut buf)?;
                    inner.send_buffer.extend(buf);
                }
            }
            Frame::Ping(f) => {
                if !f.header.flags.ack() {
                    // Send PING ACK
                    let ack_frame = FrameBuilder::ping(f.opaque_data, true);
                    let mut buf = Vec::new();
                    FrameSerializer::serialize(&Frame::Ping(ack_frame), &mut buf)?;
                    inner.send_buffer.extend(buf);
                }
            }
            Frame::Goaway(f) => {
                inner.goaway_received = true;
                inner.goaway_last_stream_id = f.last_stream_id;
            }
            Frame::WindowUpdate(f) => {
                if f.header.stream_id == 0 {
                    inner.flow_control.update_send(f.window_size_increment as i32)?;
                } else if let Some(stream) = inner.streams.get_mut(f.header.stream_id) {
                    stream.update_send_window(f.window_size_increment as i32)?;
                }
            }
            Frame::Continuation(f) => {
                // TODO: Handle continuation frames
            }
            _ => {} // Ignore unknown frames
        }

        Ok(())
    }

    /// Check if session wants to read
    pub fn want_read(&self) -> bool {
        let inner = self.inner.lock();
        !inner.goaway_received
    }

    /// Check if session wants to write
    pub fn want_write(&self) -> bool {
        let inner = self.inner.lock();
        !inner.send_buffer.is_empty() || !inner.preface_sent
    }

    /// Get a stream
    pub fn get_stream(&self, stream_id: StreamId) -> Option<StreamState> {
        let inner = self.inner.lock();
        inner.streams.get(stream_id).map(|s| s.state)
    }

    /// Terminate the session
    pub fn terminate(&self, error_code: ErrorCode) -> Result<()> {
        self.submit_goaway(error_code, &[])
    }
}

// ============================================================================
// Session Builder
// ============================================================================

/// Builder for creating sessions
pub struct SessionBuilder {
    session_type: Option<SessionType>,
    callbacks: SessionCallbacks,
    user_data: *mut c_void,
    settings: SessionSettings,
}

impl SessionBuilder {
    /// Create a new session builder
    pub fn new() -> Self {
        Self {
            session_type: None,
            callbacks: SessionCallbacks::new(),
            user_data: core::ptr::null_mut(),
            settings: SessionSettings::default(),
        }
    }

    /// Set as client
    pub fn client(mut self) -> Self {
        self.session_type = Some(SessionType::Client);
        self
    }

    /// Set as server
    pub fn server(mut self) -> Self {
        self.session_type = Some(SessionType::Server);
        self
    }

    /// Set callbacks
    pub fn callbacks(mut self, callbacks: SessionCallbacks) -> Self {
        self.callbacks = callbacks;
        self
    }

    /// Set user data
    pub fn user_data(mut self, user_data: *mut c_void) -> Self {
        self.user_data = user_data;
        self
    }

    /// Set settings
    pub fn settings(mut self, settings: SessionSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Build the session
    pub fn build(self) -> Result<Session> {
        let session_type = self.session_type
            .ok_or(Error::InvalidState("session type not set"))?;

        Ok(Session::new(session_type, self.callbacks, self.user_data))
    }
}

impl Default for SessionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// C API Types
// ============================================================================

/// nghttp2_session opaque type
#[repr(C)]
pub struct NgHttp2Session {
    _private: [u8; 0],
}

/// nghttp2_session_callbacks opaque type
#[repr(C)]
pub struct NgHttp2SessionCallbacks {
    pub(crate) inner: Box<SessionCallbacks>,
}

/// nghttp2_option opaque type
#[repr(C)]
pub struct NgHttp2Option {
    pub(crate) inner: Box<SessionOption>,
}

// C API callback wrapper storage
pub(crate) struct CApiSession {
    pub(crate) session: Session,
    pub(crate) callbacks: SessionCallbacks,
}

// ============================================================================
// C API Functions
// ============================================================================

// Callback management

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_new(
    callbacks_ptr: *mut *mut NgHttp2SessionCallbacks,
) -> c_int {
    if callbacks_ptr.is_null() {
        return NgError::InvalidArgument as i32;
    }

    let callbacks = Box::new(SessionCallbacks::new());
    let wrapper = Box::new(NgHttp2SessionCallbacks { inner: callbacks });
    
    unsafe {
        *callbacks_ptr = Box::into_raw(wrapper);
    }
    
    0
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_del(callbacks: *mut NgHttp2SessionCallbacks) {
    if !callbacks.is_null() {
        unsafe {
            drop(Box::from_raw(callbacks));
        }
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_send_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    send_callback: SendCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.send_callback = Some(send_callback);
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_recv_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    recv_callback: RecvCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.recv_callback = Some(recv_callback);
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_frame_recv_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    on_frame_recv_callback: OnFrameRecvCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.on_frame_recv_callback = Some(on_frame_recv_callback);
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_stream_close_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    on_stream_close_callback: OnStreamCloseCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.on_stream_close_callback = Some(on_stream_close_callback);
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_data_chunk_recv_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    on_data_chunk_recv_callback: OnDataChunkRecvCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.on_data_chunk_recv_callback = Some(on_data_chunk_recv_callback);
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_callbacks_set_on_header_callback(
    callbacks: *mut NgHttp2SessionCallbacks,
    on_header_callback: OnHeaderCallback,
) {
    if let Some(cb) = unsafe { callbacks.as_mut() } {
        cb.inner.on_header_callback = Some(on_header_callback);
    }
}

// Session creation

#[no_mangle]
pub extern "C" fn nghttp2_session_client_new(
    session_ptr: *mut *mut NgHttp2Session,
    callbacks: *const NgHttp2SessionCallbacks,
    user_data: *mut c_void,
) -> c_int {
    nghttp2_session_client_new2(session_ptr, callbacks, user_data, core::ptr::null())
}

#[no_mangle]
pub extern "C" fn nghttp2_session_client_new2(
    session_ptr: *mut *mut NgHttp2Session,
    callbacks: *const NgHttp2SessionCallbacks,
    user_data: *mut c_void,
    _option: *const NgHttp2Option,
) -> c_int {
    if session_ptr.is_null() {
        return NgError::InvalidArgument as i32;
    }

    let cb = if callbacks.is_null() {
        SessionCallbacks::new()
    } else {
        unsafe { (*callbacks).inner.as_ref().clone() }
    };

    let session = Session::client(cb.clone(), user_data);
    let wrapper = Box::new(CApiSession {
        session,
        callbacks: cb,
    });

    unsafe {
        *session_ptr = Box::into_raw(wrapper) as *mut NgHttp2Session;
    }

    0
}

#[no_mangle]
pub extern "C" fn nghttp2_session_server_new(
    session_ptr: *mut *mut NgHttp2Session,
    callbacks: *const NgHttp2SessionCallbacks,
    user_data: *mut c_void,
) -> c_int {
    nghttp2_session_server_new2(session_ptr, callbacks, user_data, core::ptr::null())
}

#[no_mangle]
pub extern "C" fn nghttp2_session_server_new2(
    session_ptr: *mut *mut NgHttp2Session,
    callbacks: *const NgHttp2SessionCallbacks,
    user_data: *mut c_void,
    _option: *const NgHttp2Option,
) -> c_int {
    if session_ptr.is_null() {
        return NgError::InvalidArgument as i32;
    }

    let cb = if callbacks.is_null() {
        SessionCallbacks::new()
    } else {
        unsafe { (*callbacks).inner.as_ref().clone() }
    };

    let session = Session::server(cb.clone(), user_data);
    let wrapper = Box::new(CApiSession {
        session,
        callbacks: cb,
    });

    unsafe {
        *session_ptr = Box::into_raw(wrapper) as *mut NgHttp2Session;
    }

    0
}

#[no_mangle]
pub extern "C" fn nghttp2_session_del(session: *mut NgHttp2Session) {
    if !session.is_null() {
        unsafe {
            drop(Box::from_raw(session as *mut CApiSession));
        }
    }
}

// Session operations

#[no_mangle]
pub extern "C" fn nghttp2_session_send(session: *mut NgHttp2Session) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    let data = sess.session.mem_send();
    if data.is_empty() {
        return 0;
    }

    // Call send callback if set
    if let Some(callback) = sess.callbacks.send_callback {
        let inner = sess.session.inner.lock();
        let result = callback(
            session,
            data.as_ptr(),
            data.len(),
            0,
            inner.user_data,
        );
        if result < 0 {
            return result as i32;
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn nghttp2_session_recv(session: *mut NgHttp2Session) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Call recv callback if set
    if let Some(callback) = sess.callbacks.recv_callback {
        let mut buf = [0u8; 16384];
        let inner = sess.session.inner.lock();
        let result = callback(
            session,
            buf.as_mut_ptr(),
            buf.len(),
            0,
            inner.user_data,
        );
        drop(inner);

        if result > 0 {
            match sess.session.mem_recv(&buf[..result as usize]) {
                Ok(_) => {}
                Err(e) => return e.to_error_code(),
            }
        } else if result < 0 {
            return result as i32;
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn nghttp2_session_mem_send(
    session: *mut NgHttp2Session,
    data_ptr: *mut *const u8,
) -> ssize_t {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as isize,
    };

    let data = sess.session.mem_send();
    let len = data.len();

    if len == 0 {
        return 0;
    }

    // Leak the data and return pointer
    let boxed = data.into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *const u8;

    unsafe {
        *data_ptr = ptr;
    }

    len as ssize_t
}

#[no_mangle]
pub extern "C" fn nghttp2_session_mem_recv(
    session: *mut NgHttp2Session,
    data: *const u8,
    datalen: size_t,
) -> ssize_t {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as isize,
    };

    let slice = unsafe { core::slice::from_raw_parts(data, datalen) };

    match sess.session.mem_recv(slice) {
        Ok(consumed) => consumed as ssize_t,
        Err(e) => e.to_error_code() as ssize_t,
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_want_read(session: *mut NgHttp2Session) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    if sess.session.want_read() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_want_write(session: *mut NgHttp2Session) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return 0,
    };

    if sess.session.want_write() { 1 } else { 0 }
}

// Submit functions

#[no_mangle]
pub extern "C" fn nghttp2_submit_request(
    session: *mut NgHttp2Session,
    pri_spec: *const PrioritySpec,
    nva: *const Nv,
    nvlen: size_t,
    data_prd: *const DataProvider,
    stream_user_data: *mut c_void,
) -> i32 {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Convert headers
    let headers: Vec<HeaderField> = if nva.is_null() || nvlen == 0 {
        Vec::new()
    } else {
        unsafe {
            core::slice::from_raw_parts(nva, nvlen)
                .iter()
                .map(|nv| HeaderField::new(
                    core::slice::from_raw_parts(nv.name, nv.namelen).to_vec(),
                    core::slice::from_raw_parts(nv.value, nv.valuelen).to_vec(),
                ))
                .collect()
        }
    };

    let pri = if pri_spec.is_null() {
        None
    } else {
        Some(unsafe { &*pri_spec })
    };

    let data = if data_prd.is_null() {
        None
    } else {
        Some(unsafe { &*data_prd })
    };

    match sess.session.submit_request(pri, &headers, data) {
        Ok(stream_id) => stream_id,
        Err(e) => e.to_error_code(),
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_submit_response(
    session: *mut NgHttp2Session,
    stream_id: i32,
    nva: *const Nv,
    nvlen: size_t,
    data_prd: *const DataProvider,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    // Convert headers
    let headers: Vec<HeaderField> = if nva.is_null() || nvlen == 0 {
        Vec::new()
    } else {
        unsafe {
            core::slice::from_raw_parts(nva, nvlen)
                .iter()
                .map(|nv| HeaderField::new(
                    core::slice::from_raw_parts(nv.name, nv.namelen).to_vec(),
                    core::slice::from_raw_parts(nv.value, nv.valuelen).to_vec(),
                ))
                .collect()
        }
    };

    let data = if data_prd.is_null() {
        None
    } else {
        Some(unsafe { &*data_prd })
    };

    match sess.session.submit_response(stream_id, &headers, data) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_submit_settings(
    session: *mut NgHttp2Session,
    flags: u8,
    iv: *const SettingsEntry,
    niv: size_t,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    let entries = if iv.is_null() || niv == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(iv, niv) }
    };

    match sess.session.submit_settings(entries) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_submit_ping(
    session: *mut NgHttp2Session,
    flags: u8,
    opaque_data: *const u8,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    let data = if opaque_data.is_null() {
        [0u8; 8]
    } else {
        let mut arr = [0u8; 8];
        unsafe {
            arr.copy_from_slice(core::slice::from_raw_parts(opaque_data, 8));
        }
        arr
    };

    match sess.session.submit_ping(data) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_submit_goaway(
    session: *mut NgHttp2Session,
    flags: u8,
    last_stream_id: i32,
    error_code: u32,
    opaque_data: *const u8,
    opaque_data_len: size_t,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    let debug_data = if opaque_data.is_null() || opaque_data_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(opaque_data, opaque_data_len) }
    };

    match sess.session.submit_goaway(ErrorCode::from_u32(error_code), debug_data) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_submit_rst_stream(
    session: *mut NgHttp2Session,
    flags: u8,
    stream_id: i32,
    error_code: u32,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    match sess.session.submit_rst_stream(stream_id, ErrorCode::from_u32(error_code)) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_submit_window_update(
    session: *mut NgHttp2Session,
    flags: u8,
    stream_id: i32,
    window_size_increment: i32,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    if window_size_increment <= 0 {
        return NgError::InvalidArgument as i32;
    }

    match sess.session.submit_window_update(stream_id, window_size_increment as u32) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

// Utility functions

#[no_mangle]
pub extern "C" fn nghttp2_session_get_stream_user_data(
    session: *mut NgHttp2Session,
    stream_id: i32,
) -> *mut c_void {
    let sess = match unsafe { (session as *mut CApiSession).as_ref() } {
        Some(s) => s,
        None => return core::ptr::null_mut(),
    };

    let inner = sess.session.inner.lock();
    inner.streams.get(stream_id)
        .and_then(|s| s.user_data)
        .unwrap_or(core::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn nghttp2_session_set_stream_user_data(
    session: *mut NgHttp2Session,
    stream_id: i32,
    stream_user_data: *mut c_void,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    let mut inner = sess.session.inner.lock();
    if let Some(stream) = inner.streams.get_mut(stream_id) {
        stream.user_data = Some(stream_user_data);
        0
    } else {
        NgError::InvalidStreamId as i32
    }
}

#[no_mangle]
pub extern "C" fn nghttp2_session_terminate_session(
    session: *mut NgHttp2Session,
    error_code: u32,
) -> c_int {
    let sess = match unsafe { (session as *mut CApiSession).as_mut() } {
        Some(s) => s,
        None => return NgError::InvalidArgument as i32,
    };

    match sess.session.terminate(ErrorCode::from_u32(error_code)) {
        Ok(()) => 0,
        Err(e) => e.to_error_code(),
    }
}

impl Clone for SessionCallbacks {
    fn clone(&self) -> Self {
        Self {
            send_callback: self.send_callback,
            recv_callback: self.recv_callback,
            on_frame_recv_callback: self.on_frame_recv_callback,
            on_frame_send_callback: self.on_frame_send_callback,
            on_stream_close_callback: self.on_stream_close_callback,
            on_data_chunk_recv_callback: self.on_data_chunk_recv_callback,
            on_header_callback: self.on_header_callback,
            on_begin_headers_callback: self.on_begin_headers_callback,
        }
    }
}
