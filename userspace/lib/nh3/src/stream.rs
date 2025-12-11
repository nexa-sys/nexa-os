//! HTTP/3 Stream Management
//!
//! This module implements HTTP/3 stream handling, including both
//! request/response streams and control streams.

use crate::error::{Error, ErrorCode, H3Error, Result};
use crate::types::{HeaderField, Priority, StreamId};
use std::collections::HashMap;

// ============================================================================
// Stream Type
// ============================================================================

/// HTTP/3 stream type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// Control stream
    Control,
    /// QPACK encoder stream
    QpackEncoder,
    /// QPACK decoder stream
    QpackDecoder,
    /// Request stream (bidirectional)
    Request,
    /// Push stream (unidirectional, server-initiated)
    Push,
    /// Unknown unidirectional stream
    Unknown(u64),
}

impl StreamType {
    /// Get the stream type code for unidirectional streams
    pub fn type_code(&self) -> Option<u64> {
        match self {
            StreamType::Control => Some(0x00),
            StreamType::QpackEncoder => Some(0x02),
            StreamType::QpackDecoder => Some(0x03),
            StreamType::Push => Some(0x01),
            StreamType::Unknown(code) => Some(*code),
            StreamType::Request => None, // Bidi streams don't have a type code
        }
    }
    
    /// Create from stream type code
    pub fn from_code(code: u64) -> Self {
        match code {
            0x00 => StreamType::Control,
            0x01 => StreamType::Push,
            0x02 => StreamType::QpackEncoder,
            0x03 => StreamType::QpackDecoder,
            other => StreamType::Unknown(other),
        }
    }
    
    /// Check if this is a critical stream type
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            StreamType::Control | StreamType::QpackEncoder | StreamType::QpackDecoder
        )
    }
}

// ============================================================================
// Stream State
// ============================================================================

/// Stream state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Stream is idle (not yet used)
    Idle,
    /// Stream is open
    Open,
    /// Local side has sent FIN
    HalfClosedLocal,
    /// Remote side has sent FIN
    HalfClosedRemote,
    /// Stream is closed
    Closed,
    /// Stream was reset
    Reset,
}

// ============================================================================
// Stream
// ============================================================================

/// An HTTP/3 stream
#[derive(Debug)]
pub struct Stream {
    /// Stream ID
    pub id: StreamId,
    /// Stream type
    pub stream_type: StreamType,
    /// Stream state
    pub state: StreamState,
    /// Priority
    pub priority: Priority,
    /// Received data buffer
    pub recv_buf: Vec<u8>,
    /// Send data buffer
    pub send_buf: Vec<u8>,
    /// Received headers (for request streams)
    pub headers: Vec<HeaderField>,
    /// Received trailers (for request streams)
    pub trailers: Vec<HeaderField>,
    /// User data pointer
    pub user_data: *mut core::ffi::c_void,
    /// FIN received flag
    pub fin_received: bool,
    /// FIN sent flag
    pub fin_sent: bool,
    /// Headers received flag
    pub headers_received: bool,
    /// Body started flag
    pub body_started: bool,
}

impl Stream {
    /// Create a new stream
    pub fn new(id: StreamId, stream_type: StreamType) -> Self {
        Self {
            id,
            stream_type,
            state: StreamState::Open,
            priority: Priority::default(),
            recv_buf: Vec::new(),
            send_buf: Vec::new(),
            headers: Vec::new(),
            trailers: Vec::new(),
            user_data: core::ptr::null_mut(),
            fin_received: false,
            fin_sent: false,
            headers_received: false,
            body_started: false,
        }
    }
    
    /// Create a request stream
    pub fn request(id: StreamId) -> Self {
        Self::new(id, StreamType::Request)
    }
    
    /// Create a control stream
    pub fn control(id: StreamId) -> Self {
        Self::new(id, StreamType::Control)
    }
    
    /// Check if stream is open
    pub fn is_open(&self) -> bool {
        matches!(
            self.state,
            StreamState::Open | StreamState::HalfClosedLocal | StreamState::HalfClosedRemote
        )
    }
    
    /// Check if stream can send
    pub fn can_send(&self) -> bool {
        matches!(self.state, StreamState::Open | StreamState::HalfClosedRemote)
    }
    
    /// Check if stream can receive
    pub fn can_receive(&self) -> bool {
        matches!(self.state, StreamState::Open | StreamState::HalfClosedLocal)
    }
    
    /// Close local side (send FIN)
    pub fn close_local(&mut self) {
        self.fin_sent = true;
        match self.state {
            StreamState::Open => self.state = StreamState::HalfClosedLocal,
            StreamState::HalfClosedRemote => self.state = StreamState::Closed,
            _ => {}
        }
    }
    
    /// Close remote side (receive FIN)
    pub fn close_remote(&mut self) {
        self.fin_received = true;
        match self.state {
            StreamState::Open => self.state = StreamState::HalfClosedRemote,
            StreamState::HalfClosedLocal => self.state = StreamState::Closed,
            _ => {}
        }
    }
    
    /// Reset the stream
    pub fn reset(&mut self) {
        self.state = StreamState::Reset;
    }
    
    /// Receive data into the stream buffer
    pub fn recv_data(&mut self, data: &[u8]) -> Result<()> {
        if !self.can_receive() {
            return Err(Error::H3Error(H3Error::StreamCreationError));
        }
        self.recv_buf.extend_from_slice(data);
        Ok(())
    }
    
    /// Take data from the send buffer
    pub fn take_send_data(&mut self, max_len: usize) -> Vec<u8> {
        let len = self.send_buf.len().min(max_len);
        self.send_buf.drain(..len).collect()
    }
    
    /// Queue data for sending
    pub fn queue_send_data(&mut self, data: &[u8]) -> Result<()> {
        if !self.can_send() {
            return Err(Error::H3Error(H3Error::StreamCreationError));
        }
        self.send_buf.extend_from_slice(data);
        Ok(())
    }
}

// SAFETY: Stream's user_data is a raw pointer managed by the caller
// The caller is responsible for ensuring thread safety
unsafe impl Send for Stream {}

// ============================================================================
// Stream Map
// ============================================================================

/// Map of stream ID to stream
#[derive(Debug, Default)]
pub struct StreamMap {
    /// Map of streams
    streams: HashMap<StreamId, Stream>,
    /// Next client-initiated bidirectional stream ID
    next_client_bidi: StreamId,
    /// Next client-initiated unidirectional stream ID
    next_client_uni: StreamId,
    /// Next server-initiated bidirectional stream ID
    next_server_bidi: StreamId,
    /// Next server-initiated unidirectional stream ID
    next_server_uni: StreamId,
    /// Is this a client connection?
    is_client: bool,
}

impl StreamMap {
    /// Create a new stream map for a client
    pub fn client() -> Self {
        Self {
            streams: HashMap::new(),
            next_client_bidi: 0,    // Client bidi: 0, 4, 8, ...
            next_client_uni: 2,     // Client uni: 2, 6, 10, ...
            next_server_bidi: 1,    // Server bidi: 1, 5, 9, ...
            next_server_uni: 3,     // Server uni: 3, 7, 11, ...
            is_client: true,
        }
    }
    
    /// Create a new stream map for a server
    pub fn server() -> Self {
        Self {
            streams: HashMap::new(),
            next_client_bidi: 0,
            next_client_uni: 2,
            next_server_bidi: 1,
            next_server_uni: 3,
            is_client: false,
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
    
    /// Insert a stream
    pub fn insert(&mut self, stream: Stream) {
        self.streams.insert(stream.id, stream);
    }
    
    /// Remove a stream
    pub fn remove(&mut self, id: StreamId) -> Option<Stream> {
        self.streams.remove(&id)
    }
    
    /// Check if a stream exists
    pub fn contains(&self, id: StreamId) -> bool {
        self.streams.contains_key(&id)
    }
    
    /// Get the number of streams
    pub fn len(&self) -> usize {
        self.streams.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }
    
    /// Allocate a new request stream ID (client-initiated bidi)
    pub fn alloc_request_stream(&mut self) -> Result<StreamId> {
        if !self.is_client {
            return Err(ErrorCode::InvalidState.into());
        }
        let id = self.next_client_bidi;
        self.next_client_bidi += 4;
        let stream = Stream::request(id);
        self.streams.insert(id, stream);
        Ok(id)
    }
    
    /// Allocate a new unidirectional stream ID
    pub fn alloc_uni_stream(&mut self, stream_type: StreamType) -> Result<StreamId> {
        let id = if self.is_client {
            let id = self.next_client_uni;
            self.next_client_uni += 4;
            id
        } else {
            let id = self.next_server_uni;
            self.next_server_uni += 4;
            id
        };
        let stream = Stream::new(id, stream_type);
        self.streams.insert(id, stream);
        Ok(id)
    }
    
    /// Get all open streams
    pub fn open_streams(&self) -> impl Iterator<Item = &Stream> {
        self.streams.values().filter(|s| s.is_open())
    }
    
    /// Get all request streams
    pub fn request_streams(&self) -> impl Iterator<Item = &Stream> {
        self.streams
            .values()
            .filter(|s| s.stream_type == StreamType::Request)
    }
    
    /// Check if stream ID is client-initiated
    pub fn is_client_initiated(id: StreamId) -> bool {
        (id & 0x01) == 0
    }
    
    /// Check if stream ID is bidirectional
    pub fn is_bidi(id: StreamId) -> bool {
        (id & 0x02) == 0
    }
    
    /// Check if stream ID is unidirectional
    pub fn is_uni(id: StreamId) -> bool {
        (id & 0x02) != 0
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_stream_state() {
        let mut stream = Stream::request(0);
        assert!(stream.is_open());
        assert!(stream.can_send());
        assert!(stream.can_receive());
        
        stream.close_local();
        assert_eq!(stream.state, StreamState::HalfClosedLocal);
        assert!(!stream.can_send());
        assert!(stream.can_receive());
        
        stream.close_remote();
        assert_eq!(stream.state, StreamState::Closed);
        assert!(!stream.is_open());
    }
    
    #[test]
    fn test_stream_map() {
        let mut map = StreamMap::client();
        
        let id1 = map.alloc_request_stream().unwrap();
        assert_eq!(id1, 0);
        
        let id2 = map.alloc_request_stream().unwrap();
        assert_eq!(id2, 4);
        
        let ctrl_id = map.alloc_uni_stream(StreamType::Control).unwrap();
        assert_eq!(ctrl_id, 2);
        
        assert!(map.contains(id1));
        assert!(map.contains(id2));
        assert!(map.contains(ctrl_id));
    }
    
    #[test]
    fn test_stream_id_classification() {
        // Client bidi: 0, 4, 8, ...
        assert!(StreamMap::is_client_initiated(0));
        assert!(StreamMap::is_bidi(0));
        
        // Server bidi: 1, 5, 9, ...
        assert!(!StreamMap::is_client_initiated(1));
        assert!(StreamMap::is_bidi(1));
        
        // Client uni: 2, 6, 10, ...
        assert!(StreamMap::is_client_initiated(2));
        assert!(StreamMap::is_uni(2));
        
        // Server uni: 3, 7, 11, ...
        assert!(!StreamMap::is_client_initiated(3));
        assert!(StreamMap::is_uni(3));
    }
}
