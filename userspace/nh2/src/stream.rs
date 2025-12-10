//! HTTP/2 Stream Management
//!
//! This module implements HTTP/2 stream state machine and management.

use crate::error::{Error, ErrorCode, Result};
use crate::hpack::HeaderField;
use crate::types::{PrioritySpec, StreamId};
use std::collections::HashMap;

// ============================================================================
// Stream State
// ============================================================================

/// Stream state machine states (RFC 7540 Section 5.1)
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Stream is idle (not yet opened)
    Idle = 1,
    /// Stream is open for bidirectional communication
    Open = 2,
    /// Stream is reserved for server push (local)
    ReservedLocal = 3,
    /// Stream is reserved for server push (remote)
    ReservedRemote = 4,
    /// Local side closed (can still receive)
    HalfClosedLocal = 5,
    /// Remote side closed (can still send)
    HalfClosedRemote = 6,
    /// Stream is closed
    Closed = 7,
}

impl StreamState {
    /// Check if stream can send data
    pub fn can_send(&self) -> bool {
        matches!(self, StreamState::Open | StreamState::HalfClosedRemote)
    }

    /// Check if stream can receive data
    pub fn can_recv(&self) -> bool {
        matches!(self, StreamState::Open | StreamState::HalfClosedLocal)
    }

    /// Check if stream is active
    pub fn is_active(&self) -> bool {
        !matches!(self, StreamState::Idle | StreamState::Closed)
    }
}

// ============================================================================
// Stream
// ============================================================================

/// HTTP/2 stream
#[derive(Debug)]
pub struct Stream {
    /// Stream identifier
    pub id: StreamId,
    /// Current state
    pub state: StreamState,
    /// Local window size (for receiving)
    pub local_window_size: i32,
    /// Remote window size (for sending)
    pub remote_window_size: i32,
    /// Priority specification
    pub priority: PrioritySpec,
    /// Request headers
    pub request_headers: Vec<HeaderField>,
    /// Response headers
    pub response_headers: Vec<HeaderField>,
    /// Pending data to send
    pub send_buffer: Vec<u8>,
    /// Received data
    pub recv_buffer: Vec<u8>,
    /// Weight for priority scheduling
    pub weight: i32,
    /// Exclusive dependency flag
    pub exclusive: bool,
    /// Dependent stream ID
    pub dependency: StreamId,
    /// Whether END_STREAM has been sent
    pub end_stream_sent: bool,
    /// Whether END_STREAM has been received
    pub end_stream_received: bool,
    /// Error code if stream was reset
    pub error_code: Option<ErrorCode>,
    /// User data pointer
    pub user_data: Option<*mut core::ffi::c_void>,
}

impl Stream {
    /// Create a new stream
    pub fn new(id: StreamId, initial_window_size: i32) -> Self {
        Self {
            id,
            state: StreamState::Idle,
            local_window_size: initial_window_size,
            remote_window_size: initial_window_size,
            priority: PrioritySpec::default(),
            request_headers: Vec::new(),
            response_headers: Vec::new(),
            send_buffer: Vec::new(),
            recv_buffer: Vec::new(),
            weight: 16,
            exclusive: false,
            dependency: 0,
            end_stream_sent: false,
            end_stream_received: false,
            error_code: None,
            user_data: None,
        }
    }

    /// Open the stream (from Idle state)
    pub fn open(&mut self) -> Result<()> {
        match self.state {
            StreamState::Idle => {
                self.state = StreamState::Open;
                Ok(())
            }
            StreamState::ReservedLocal => {
                self.state = StreamState::HalfClosedRemote;
                Ok(())
            }
            StreamState::ReservedRemote => {
                self.state = StreamState::HalfClosedLocal;
                Ok(())
            }
            _ => Err(Error::InvalidState("cannot open stream from current state")),
        }
    }

    /// Send END_STREAM
    pub fn close_local(&mut self) -> Result<()> {
        match self.state {
            StreamState::Open => {
                self.state = StreamState::HalfClosedLocal;
                self.end_stream_sent = true;
                Ok(())
            }
            StreamState::HalfClosedRemote => {
                self.state = StreamState::Closed;
                self.end_stream_sent = true;
                Ok(())
            }
            _ => Err(Error::InvalidState("cannot close local from current state")),
        }
    }

    /// Receive END_STREAM
    pub fn close_remote(&mut self) -> Result<()> {
        match self.state {
            StreamState::Open => {
                self.state = StreamState::HalfClosedRemote;
                self.end_stream_received = true;
                Ok(())
            }
            StreamState::HalfClosedLocal => {
                self.state = StreamState::Closed;
                self.end_stream_received = true;
                Ok(())
            }
            _ => Err(Error::InvalidState(
                "cannot close remote from current state",
            )),
        }
    }

    /// Reset the stream
    pub fn reset(&mut self, error_code: ErrorCode) {
        self.state = StreamState::Closed;
        self.error_code = Some(error_code);
    }

    /// Reserve for server push (local)
    pub fn reserve_local(&mut self) -> Result<()> {
        if self.state == StreamState::Idle {
            self.state = StreamState::ReservedLocal;
            Ok(())
        } else {
            Err(Error::InvalidState("cannot reserve from current state"))
        }
    }

    /// Reserve for server push (remote)
    pub fn reserve_remote(&mut self) -> Result<()> {
        if self.state == StreamState::Idle {
            self.state = StreamState::ReservedRemote;
            Ok(())
        } else {
            Err(Error::InvalidState("cannot reserve from current state"))
        }
    }

    /// Update send window size
    pub fn update_send_window(&mut self, delta: i32) -> Result<()> {
        let new_size = self.remote_window_size.saturating_add(delta);
        if new_size > 0x7FFFFFFF {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.remote_window_size = new_size;
        Ok(())
    }

    /// Update receive window size
    pub fn update_recv_window(&mut self, delta: i32) -> Result<()> {
        let new_size = self.local_window_size.saturating_add(delta);
        if new_size > 0x7FFFFFFF {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.local_window_size = new_size;
        Ok(())
    }

    /// Consume send window
    pub fn consume_send_window(&mut self, size: i32) -> Result<()> {
        if size > self.remote_window_size {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.remote_window_size -= size;
        Ok(())
    }

    /// Consume receive window
    pub fn consume_recv_window(&mut self, size: i32) -> Result<()> {
        if size > self.local_window_size {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.local_window_size -= size;
        Ok(())
    }

    /// Get available send window
    pub fn available_send_window(&self) -> i32 {
        self.remote_window_size
    }

    /// Get available receive window
    pub fn available_recv_window(&self) -> i32 {
        self.local_window_size
    }
}

// ============================================================================
// Stream Map
// ============================================================================

/// Manages all streams in a session
#[derive(Debug)]
pub struct StreamMap {
    streams: HashMap<StreamId, Stream>,
    /// Next stream ID for client-initiated streams
    next_client_stream_id: StreamId,
    /// Next stream ID for server-initiated streams
    next_server_stream_id: StreamId,
    /// Maximum concurrent streams (local limit)
    max_concurrent_local: u32,
    /// Maximum concurrent streams (remote limit)
    max_concurrent_remote: u32,
    /// Initial window size for new streams
    initial_window_size: i32,
    /// Whether this is a client (odd stream IDs) or server (even stream IDs)
    is_client: bool,
}

impl StreamMap {
    /// Create a new stream map
    pub fn new(is_client: bool, initial_window_size: i32) -> Self {
        Self {
            streams: HashMap::new(),
            next_client_stream_id: 1,
            next_server_stream_id: 2,
            max_concurrent_local: 100,
            max_concurrent_remote: 100,
            initial_window_size,
            is_client,
        }
    }

    /// Get the next stream ID for local-initiated streams
    pub fn next_stream_id(&mut self) -> StreamId {
        if self.is_client {
            let id = self.next_client_stream_id;
            self.next_client_stream_id += 2;
            id
        } else {
            let id = self.next_server_stream_id;
            self.next_server_stream_id += 2;
            id
        }
    }

    /// Get a stream by ID
    pub fn get(&self, id: StreamId) -> Option<&Stream> {
        self.streams.get(&id)
    }

    /// Get a mutable stream by ID
    pub fn get_mut(&mut self, id: StreamId) -> Option<&mut Stream> {
        self.streams.get_mut(&id)
    }

    /// Create a new stream
    pub fn create(&mut self, id: StreamId) -> Result<&mut Stream> {
        // Check if stream already exists
        if self.streams.contains_key(&id) {
            return Err(Error::InvalidState("stream already exists"));
        }

        // Check concurrent stream limit
        let active_count = self
            .streams
            .values()
            .filter(|s| s.state.is_active())
            .count() as u32;
        if active_count >= self.max_concurrent_local {
            return Err(Error::Protocol(ErrorCode::RefusedStream));
        }

        let stream = Stream::new(id, self.initial_window_size);
        self.streams.insert(id, stream);
        Ok(self.streams.get_mut(&id).unwrap())
    }

    /// Get or create a stream
    pub fn get_or_create(&mut self, id: StreamId) -> Result<&mut Stream> {
        if !self.streams.contains_key(&id) {
            self.create(id)?;
        }
        Ok(self.streams.get_mut(&id).unwrap())
    }

    /// Remove a closed stream
    pub fn remove(&mut self, id: StreamId) -> Option<Stream> {
        self.streams.remove(&id)
    }

    /// Get the number of active streams
    pub fn active_count(&self) -> usize {
        self.streams
            .values()
            .filter(|s| s.state.is_active())
            .count()
    }

    /// Check if a stream ID is valid for local initiation
    pub fn is_local_stream(&self, id: StreamId) -> bool {
        if self.is_client {
            id % 2 == 1 // Odd IDs for client
        } else {
            id % 2 == 0 // Even IDs for server
        }
    }

    /// Set max concurrent streams (local)
    pub fn set_max_concurrent_local(&mut self, max: u32) {
        self.max_concurrent_local = max;
    }

    /// Set max concurrent streams (remote)
    pub fn set_max_concurrent_remote(&mut self, max: u32) {
        self.max_concurrent_remote = max;
    }

    /// Set initial window size for new streams
    pub fn set_initial_window_size(&mut self, size: i32) -> Result<()> {
        let delta = size - self.initial_window_size;
        self.initial_window_size = size;

        // Update all active streams
        for stream in self.streams.values_mut() {
            if stream.state.is_active() {
                stream.update_send_window(delta)?;
            }
        }

        Ok(())
    }

    /// Iterate over all streams
    pub fn iter(&self) -> impl Iterator<Item = (&StreamId, &Stream)> {
        self.streams.iter()
    }

    /// Iterate over all streams mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&StreamId, &mut Stream)> {
        self.streams.iter_mut()
    }

    /// Get all stream IDs
    pub fn stream_ids(&self) -> Vec<StreamId> {
        self.streams.keys().copied().collect()
    }

    /// Clean up closed streams
    pub fn cleanup_closed(&mut self) {
        self.streams
            .retain(|_, stream| stream.state != StreamState::Closed);
    }
}

// ============================================================================
// C API Compatibility
// ============================================================================

/// nghttp2_stream structure (C API compatible)
#[repr(C)]
pub struct NgHttp2Stream {
    pub stream_id: i32,
    pub state: i32,
}

impl From<&Stream> for NgHttp2Stream {
    fn from(stream: &Stream) -> Self {
        Self {
            stream_id: stream.id,
            state: stream.state as i32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_state_transitions() {
        let mut stream = Stream::new(1, 65535);
        assert_eq!(stream.state, StreamState::Idle);

        stream.open().unwrap();
        assert_eq!(stream.state, StreamState::Open);

        stream.close_local().unwrap();
        assert_eq!(stream.state, StreamState::HalfClosedLocal);

        stream.close_remote().unwrap();
        assert_eq!(stream.state, StreamState::Closed);
    }

    #[test]
    fn test_stream_map() {
        let mut map = StreamMap::new(true, 65535);

        let id = map.next_stream_id();
        assert_eq!(id, 1);

        map.create(id).unwrap();
        assert!(map.get(id).is_some());

        let id2 = map.next_stream_id();
        assert_eq!(id2, 3);
    }

    #[test]
    fn test_window_management() {
        let mut stream = Stream::new(1, 65535);

        stream.consume_send_window(1000).unwrap();
        assert_eq!(stream.available_send_window(), 64535);

        stream.update_send_window(500).unwrap();
        assert_eq!(stream.available_send_window(), 65035);
    }
}
