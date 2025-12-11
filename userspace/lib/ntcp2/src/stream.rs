//! QUIC Stream Management
//!
//! This module implements QUIC stream handling according to RFC 9000 Section 2-3.
//!
//! ## Stream Types
//!
//! - **Bidirectional streams**: Data flows in both directions
//! - **Unidirectional streams**: Data flows in one direction only
//!
//! ## Stream ID Encoding
//!
//! Stream IDs are 62-bit integers with the following encoding:
//! - Bit 0: Initiator (0 = client, 1 = server)
//! - Bit 1: Direction (0 = bidirectional, 1 = unidirectional)
//!
//! This gives four stream ID spaces:
//! - 0x00: Client-initiated bidirectional
//! - 0x01: Server-initiated bidirectional
//! - 0x02: Client-initiated unidirectional
//! - 0x03: Server-initiated unidirectional

use crate::error::{Error, NgError, Result, TransportError};
use crate::types::{StreamId, StreamType};
use crate::{c_int, c_void, size_t};

use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ============================================================================
// Stream State
// ============================================================================

/// Stream state machine states (RFC 9000 Section 3)
///
/// Sending states:
/// ```text
///   o
///   | Open Stream (Sending)
///   v
/// +-------+
/// | Ready | Send STREAM
/// |       |-----+
/// +-------+     |
///     |         v
///     |     +-------+
///     |     | Send  | Send STREAM
///     |     |       |-----+
///     |     +-------+     |
///     |         |         v
///     |         | Send STREAM with FIN
///     |         v
///     |     +-------+
///     +---->| Data  | Recv ACK
///           | Sent  |-----+
///           +-------+     |
///               |         v
///               v     (all data acked)
///           +-------+
///           | Data  |
///           | Recvd |
///           +-------+
/// ```
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSendState {
    /// Ready to send data
    Ready = 0,
    /// Sending data
    Send = 1,
    /// All data sent (FIN sent)
    DataSent = 2,
    /// All data received by peer (ACKed)
    DataRecvd = 3,
    /// Stream reset sent
    ResetSent = 4,
    /// Stream reset received (ACKed)
    ResetRecvd = 5,
}

/// Receiving states:
/// ```text
///   o
///   | Recv STREAM
///   v
/// +-------+
/// | Recv  | Recv STREAM
/// |       |-----+
/// +-------+     |
///     |         v
///     | Recv STREAM with FIN
///     v
/// +-------+
/// | Size  | Recv STREAM
/// | Known |-----+
/// +-------+     |
///     |         v
///     | (all data received)
///     v
/// +-------+
/// | Data  | Deliver to app
/// | Recvd |-----+
/// +-------+     |
///     |         v
///     v     (all data read)
/// +-------+
/// | Data  |
/// | Read  |
/// +-------+
/// ```
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRecvState {
    /// Receiving data
    Recv = 0,
    /// Final size known (FIN received)
    SizeKnown = 1,
    /// All data received
    DataRecvd = 2,
    /// All data read by application
    DataRead = 3,
    /// Reset received
    ResetRecvd = 4,
    /// Reset read
    ResetRead = 5,
}

// ============================================================================
// Stream Data Buffer
// ============================================================================

/// Buffer for stream data with gap handling
#[derive(Debug, Default)]
pub struct StreamBuffer {
    /// Buffered data chunks (offset, data)
    chunks: VecDeque<(u64, Vec<u8>)>,
    /// Next expected offset for reading
    read_offset: u64,
    /// Total bytes buffered
    buffered_bytes: usize,
    /// Maximum buffer size
    max_buffer_size: usize,
    /// Final offset (if known)
    final_offset: Option<u64>,
}

impl StreamBuffer {
    /// Create a new stream buffer
    pub fn new(max_size: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            read_offset: 0,
            buffered_bytes: 0,
            max_buffer_size: max_size,
            final_offset: None,
        }
    }

    /// Push data to the buffer
    pub fn push(&mut self, offset: u64, data: &[u8], fin: bool) -> Result<()> {
        if data.is_empty() && !fin {
            return Ok(());
        }

        // Check if this would exceed our buffer
        if self.buffered_bytes + data.len() > self.max_buffer_size {
            return Err(Error::Transport(TransportError::FlowControlError));
        }

        // Set final offset if FIN
        if fin {
            let end_offset = offset + data.len() as u64;
            if let Some(existing) = self.final_offset {
                if existing != end_offset {
                    return Err(Error::Transport(TransportError::FinalSizeError));
                }
            } else {
                self.final_offset = Some(end_offset);
            }
        }

        // Skip data already read
        if offset + data.len() as u64 <= self.read_offset {
            return Ok(());
        }

        // Trim overlapping data at the start
        let (actual_offset, actual_data) = if offset < self.read_offset {
            let skip = (self.read_offset - offset) as usize;
            (self.read_offset, &data[skip..])
        } else {
            (offset, data)
        };

        if actual_data.is_empty() {
            return Ok(());
        }

        // Insert in sorted order (simple implementation)
        // In production, would use a more efficient data structure
        let mut insert_idx = 0;
        for (i, (chunk_offset, _)) in self.chunks.iter().enumerate() {
            if *chunk_offset > actual_offset {
                break;
            }
            insert_idx = i + 1;
        }

        self.chunks
            .insert(insert_idx, (actual_offset, actual_data.to_vec()));
        self.buffered_bytes += actual_data.len();

        // Coalesce adjacent chunks (simplified)
        self.coalesce_chunks();

        Ok(())
    }

    /// Read data from the buffer
    pub fn read(&mut self, dest: &mut [u8]) -> (usize, bool) {
        let mut total_read = 0;

        while !self.chunks.is_empty() && total_read < dest.len() {
            if let Some((chunk_offset, chunk_data)) = self.chunks.front() {
                // Check for gap
                if *chunk_offset > self.read_offset {
                    break;
                }

                // Calculate how much to read from this chunk
                let skip = (self.read_offset - chunk_offset) as usize;
                let available = chunk_data.len() - skip;
                let to_read = available.min(dest.len() - total_read);

                dest[total_read..total_read + to_read]
                    .copy_from_slice(&chunk_data[skip..skip + to_read]);

                total_read += to_read;
                self.read_offset += to_read as u64;

                // Remove chunk if fully read
                if skip + to_read >= chunk_data.len() {
                    let removed = self.chunks.pop_front().unwrap();
                    self.buffered_bytes -= removed.1.len();
                }
            } else {
                break;
            }
        }

        // Check if we've read all data (including FIN)
        let fin = self
            .final_offset
            .map(|fo| self.read_offset >= fo)
            .unwrap_or(false);

        (total_read, fin)
    }

    /// Check if all data has been received
    pub fn is_complete(&self) -> bool {
        if let Some(final_offset) = self.final_offset {
            // Check if we have contiguous data from 0 to final_offset
            if self.chunks.is_empty() {
                return self.read_offset >= final_offset;
            }

            // Simple check - just verify read_offset
            self.read_offset >= final_offset
        } else {
            false
        }
    }

    /// Coalesce adjacent chunks
    fn coalesce_chunks(&mut self) {
        // Simplified: in production would merge overlapping/adjacent chunks
    }

    /// Get current read offset
    pub fn read_offset(&self) -> u64 {
        self.read_offset
    }

    /// Get buffered bytes count
    pub fn buffered_bytes(&self) -> usize {
        self.buffered_bytes
    }
}

// ============================================================================
// Send Buffer
// ============================================================================

/// Buffer for outgoing stream data
#[derive(Debug)]
pub struct SendBuffer {
    /// Pending data to send (offset, data, fin)
    pending: VecDeque<(u64, Vec<u8>, bool)>,
    /// Next write offset
    write_offset: u64,
    /// Acknowledged offset (all data before this has been ACKed)
    acked_offset: u64,
    /// Total pending bytes
    pending_bytes: usize,
    /// Maximum buffer size
    max_buffer_size: usize,
    /// FIN sent
    fin_sent: bool,
    /// FIN ACKed
    fin_acked: bool,
}

impl SendBuffer {
    /// Create a new send buffer
    pub fn new(max_size: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            write_offset: 0,
            acked_offset: 0,
            pending_bytes: 0,
            max_buffer_size: max_size,
            fin_sent: false,
            fin_acked: false,
        }
    }

    /// Write data to the buffer
    pub fn write(&mut self, data: &[u8], fin: bool) -> Result<usize> {
        if self.fin_sent {
            return Err(Error::Ng(NgError::StreamShutWr));
        }

        // Check buffer space
        let available = self.max_buffer_size.saturating_sub(self.pending_bytes);
        let to_write = data.len().min(available);

        if to_write > 0 || fin {
            let offset = self.write_offset;
            self.pending
                .push_back((offset, data[..to_write].to_vec(), fin));
            self.write_offset += to_write as u64;
            self.pending_bytes += to_write;

            if fin {
                self.fin_sent = true;
            }
        }

        Ok(to_write)
    }

    /// Get next chunk to send
    pub fn next_chunk(&self, max_len: usize) -> Option<(u64, &[u8], bool)> {
        self.pending.front().map(|(offset, data, fin)| {
            let len = data.len().min(max_len);
            (*offset, &data[..len], *fin && len == data.len())
        })
    }

    /// Mark data as sent (move to in-flight)
    pub fn mark_sent(&mut self, len: usize) {
        if let Some((_, data, _)) = self.pending.front_mut() {
            if len >= data.len() {
                let removed = self.pending.pop_front().unwrap();
                self.pending_bytes -= removed.1.len();
            } else {
                // Partial send - split the chunk
                let remaining = data.split_off(len);
                let offset = self.pending[0].0 + len as u64;
                let fin = self.pending[0].2;
                self.pending[0].1 = remaining;
                self.pending[0].0 = offset;
                self.pending_bytes -= len;
            }
        }
    }

    /// Mark data as ACKed
    pub fn mark_acked(&mut self, offset: u64, len: u64, fin: bool) {
        let new_acked = offset + len;
        if new_acked > self.acked_offset {
            self.acked_offset = new_acked;
        }
        if fin {
            self.fin_acked = true;
        }
    }

    /// Check if all data has been ACKed
    pub fn is_complete(&self) -> bool {
        self.fin_sent && self.fin_acked && self.acked_offset >= self.write_offset
    }

    /// Get write offset
    pub fn write_offset(&self) -> u64 {
        self.write_offset
    }

    /// Get ACKed offset
    pub fn acked_offset(&self) -> u64 {
        self.acked_offset
    }
}

// ============================================================================
// Stream
// ============================================================================

/// A QUIC stream
pub struct Stream {
    /// Stream ID
    id: StreamId,
    /// Stream type
    stream_type: StreamType,
    /// Send state
    send_state: RwLock<StreamSendState>,
    /// Receive state
    recv_state: RwLock<StreamRecvState>,
    /// Receive buffer
    recv_buffer: RwLock<StreamBuffer>,
    /// Send buffer
    send_buffer: RwLock<SendBuffer>,
    /// Flow control: max data we can send
    max_send_data: AtomicU64,
    /// Flow control: max data we can receive
    max_recv_data: AtomicU64,
    /// Bytes sent
    bytes_sent: AtomicU64,
    /// Bytes received
    bytes_recv: AtomicU64,
    /// User data
    user_data: *mut c_void,
    /// Stream is blocked (waiting for flow control)
    blocked: AtomicBool,
    /// Error code (if reset)
    error_code: AtomicU64,
}

// SAFETY: Stream uses internal synchronization and user_data is controlled by user
unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl Stream {
    /// Create a new stream
    pub fn new(
        id: StreamId,
        max_send_data: u64,
        max_recv_data: u64,
        user_data: *mut c_void,
    ) -> Self {
        let stream_type = StreamType::from_stream_id(id);
        let recv_buffer_size = max_recv_data as usize;
        let send_buffer_size = max_send_data as usize;

        Self {
            id,
            stream_type,
            send_state: RwLock::new(StreamSendState::Ready),
            recv_state: RwLock::new(StreamRecvState::Recv),
            recv_buffer: RwLock::new(StreamBuffer::new(recv_buffer_size)),
            send_buffer: RwLock::new(SendBuffer::new(send_buffer_size)),
            max_send_data: AtomicU64::new(max_send_data),
            max_recv_data: AtomicU64::new(max_recv_data),
            bytes_sent: AtomicU64::new(0),
            bytes_recv: AtomicU64::new(0),
            user_data,
            blocked: AtomicBool::new(false),
            error_code: AtomicU64::new(0),
        }
    }

    /// Get stream ID
    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Get stream type
    pub fn stream_type(&self) -> StreamType {
        self.stream_type
    }

    /// Check if stream is bidirectional
    pub fn is_bidi(&self) -> bool {
        matches!(
            self.stream_type,
            StreamType::ClientBidi | StreamType::ServerBidi
        )
    }

    /// Check if stream is unidirectional
    pub fn is_uni(&self) -> bool {
        !self.is_bidi()
    }

    /// Check if we can send on this stream
    pub fn can_send(&self) -> bool {
        match *self.send_state.read() {
            StreamSendState::Ready | StreamSendState::Send => true,
            _ => false,
        }
    }

    /// Check if we can receive on this stream
    pub fn can_recv(&self) -> bool {
        match *self.recv_state.read() {
            StreamRecvState::Recv | StreamRecvState::SizeKnown | StreamRecvState::DataRecvd => true,
            _ => false,
        }
    }

    /// Write data to the stream
    pub fn write(&self, data: &[u8], fin: bool) -> Result<usize> {
        if !self.can_send() {
            return Err(Error::Ng(NgError::StreamState));
        }

        // Check flow control
        let current_sent = self.bytes_sent.load(Ordering::Acquire);
        let max_data = self.max_send_data.load(Ordering::Acquire);
        let available = max_data.saturating_sub(current_sent) as usize;

        if available == 0 && !data.is_empty() {
            self.blocked.store(true, Ordering::Release);
            return Err(Error::Ng(NgError::StreamDataBlocked));
        }

        let to_write = data.len().min(available);
        let written = self.send_buffer.write().write(&data[..to_write], fin)?;

        self.bytes_sent.fetch_add(written as u64, Ordering::AcqRel);

        // Update send state
        if fin {
            *self.send_state.write() = StreamSendState::DataSent;
        } else if written > 0 {
            *self.send_state.write() = StreamSendState::Send;
        }

        Ok(written)
    }

    /// Read data from the stream
    pub fn read(&self, dest: &mut [u8]) -> Result<(usize, bool)> {
        if !self.can_recv() {
            return Err(Error::Ng(NgError::StreamState));
        }

        let (read, fin) = self.recv_buffer.write().read(dest);

        // Update recv state
        if fin {
            *self.recv_state.write() = StreamRecvState::DataRead;
        }

        Ok((read, fin))
    }

    /// Receive data (called when STREAM frame arrives)
    pub fn recv(&self, offset: u64, data: &[u8], fin: bool) -> Result<()> {
        // Check flow control
        let end_offset = offset + data.len() as u64;
        let max_data = self.max_recv_data.load(Ordering::Acquire);

        if end_offset > max_data {
            return Err(Error::Transport(TransportError::FlowControlError));
        }

        // Push to buffer
        self.recv_buffer.write().push(offset, data, fin)?;
        self.bytes_recv.fetch_add(data.len() as u64, Ordering::AcqRel);

        // Update recv state
        if fin {
            *self.recv_state.write() = StreamRecvState::SizeKnown;
        }

        if self.recv_buffer.read().is_complete() {
            *self.recv_state.write() = StreamRecvState::DataRecvd;
        }

        Ok(())
    }

    /// Mark data as ACKed
    pub fn ack(&self, offset: u64, len: u64, fin: bool) {
        self.send_buffer.write().mark_acked(offset, len, fin);

        // Update send state if all data ACKed
        if self.send_buffer.read().is_complete() {
            *self.send_state.write() = StreamSendState::DataRecvd;
        }
    }

    /// Reset the stream (send RESET_STREAM)
    pub fn reset(&self, error_code: u64) {
        self.error_code.store(error_code, Ordering::Release);
        *self.send_state.write() = StreamSendState::ResetSent;
    }

    /// Handle STOP_SENDING frame
    pub fn stop_sending(&self, error_code: u64) {
        self.error_code.store(error_code, Ordering::Release);
        *self.send_state.write() = StreamSendState::ResetSent;
    }

    /// Handle RESET_STREAM frame
    pub fn recv_reset(&self, error_code: u64, final_size: u64) -> Result<()> {
        // Validate final size
        if let Some(current_final) = self.recv_buffer.read().final_offset {
            if current_final != final_size {
                return Err(Error::Transport(TransportError::FinalSizeError));
            }
        }

        self.error_code.store(error_code, Ordering::Release);
        *self.recv_state.write() = StreamRecvState::ResetRecvd;

        Ok(())
    }

    /// Update max send data (from MAX_STREAM_DATA)
    pub fn update_max_send_data(&self, max_data: u64) {
        let current = self.max_send_data.load(Ordering::Acquire);
        if max_data > current {
            self.max_send_data.store(max_data, Ordering::Release);
            self.blocked.store(false, Ordering::Release);
        }
    }

    /// Get user data
    pub fn user_data(&self) -> *mut c_void {
        self.user_data
    }

    /// Set user data
    pub fn set_user_data(&self, user_data: *mut c_void) {
        // Note: This would need interior mutability in real impl
    }

    /// Check if stream is finished (all data sent and ACKed, or reset)
    pub fn is_finished(&self) -> bool {
        let send_done = matches!(
            *self.send_state.read(),
            StreamSendState::DataRecvd | StreamSendState::ResetRecvd
        );
        let recv_done = matches!(
            *self.recv_state.read(),
            StreamRecvState::DataRead | StreamRecvState::ResetRead
        );

        if self.is_uni() {
            // Unidirectional: only one direction matters
            send_done || recv_done
        } else {
            // Bidirectional: both directions must be done
            send_done && recv_done
        }
    }
}

// ============================================================================
// Stream Type Extension
// ============================================================================

impl StreamType {
    /// Get stream type from stream ID
    pub fn from_stream_id(id: StreamId) -> Self {
        match id & 0x03 {
            0x00 => StreamType::ClientBidi,
            0x01 => StreamType::ServerBidi,
            0x02 => StreamType::ClientUni,
            0x03 => StreamType::ServerUni,
            _ => unreachable!(),
        }
    }
}

// ============================================================================
// Stream Manager
// ============================================================================

/// Manages all streams for a connection
pub struct StreamManager {
    /// All streams
    streams: HashMap<StreamId, Stream>,
    /// Is this the client side?
    is_client: bool,
    /// Next client-initiated bidi stream ID
    next_client_bidi: AtomicU64,
    /// Next server-initiated bidi stream ID
    next_server_bidi: AtomicU64,
    /// Next client-initiated uni stream ID
    next_client_uni: AtomicU64,
    /// Next server-initiated uni stream ID
    next_server_uni: AtomicU64,
    /// Maximum local bidi streams
    max_local_bidi_streams: AtomicU64,
    /// Maximum remote bidi streams
    max_remote_bidi_streams: AtomicU64,
    /// Maximum local uni streams
    max_local_uni_streams: AtomicU64,
    /// Maximum remote uni streams
    max_remote_uni_streams: AtomicU64,
    /// Default max stream data (bidi local)
    default_max_stream_data_bidi_local: u64,
    /// Default max stream data (bidi remote)
    default_max_stream_data_bidi_remote: u64,
    /// Default max stream data (uni)
    default_max_stream_data_uni: u64,
}

impl StreamManager {
    /// Create a new stream manager
    pub fn new(is_client: bool) -> Self {
        Self {
            streams: HashMap::new(),
            is_client,
            next_client_bidi: AtomicU64::new(0),
            next_server_bidi: AtomicU64::new(1),
            next_client_uni: AtomicU64::new(2),
            next_server_uni: AtomicU64::new(3),
            max_local_bidi_streams: AtomicU64::new(100),
            max_remote_bidi_streams: AtomicU64::new(100),
            max_local_uni_streams: AtomicU64::new(100),
            max_remote_uni_streams: AtomicU64::new(100),
            default_max_stream_data_bidi_local: 256 * 1024,
            default_max_stream_data_bidi_remote: 256 * 1024,
            default_max_stream_data_uni: 256 * 1024,
        }
    }

    /// Configure stream limits from transport parameters
    pub fn configure(
        &mut self,
        max_bidi_streams: u64,
        max_uni_streams: u64,
        max_stream_data_bidi_local: u64,
        max_stream_data_bidi_remote: u64,
        max_stream_data_uni: u64,
    ) {
        self.max_local_bidi_streams
            .store(max_bidi_streams, Ordering::Release);
        self.max_local_uni_streams
            .store(max_uni_streams, Ordering::Release);
        self.default_max_stream_data_bidi_local = max_stream_data_bidi_local;
        self.default_max_stream_data_bidi_remote = max_stream_data_bidi_remote;
        self.default_max_stream_data_uni = max_stream_data_uni;
    }

    /// Open a new bidirectional stream
    pub fn open_bidi_stream(&mut self) -> Result<StreamId> {
        let counter = if self.is_client {
            &self.next_client_bidi
        } else {
            &self.next_server_bidi
        };

        let stream_num = counter.fetch_add(4, Ordering::AcqRel);
        let stream_id = stream_num as StreamId;

        // Check stream limit
        let max_streams = self.max_local_bidi_streams.load(Ordering::Acquire);
        if (stream_num >> 2) >= max_streams {
            return Err(Error::Ng(NgError::StreamIdBlocked));
        }

        // Create the stream
        let stream = Stream::new(
            stream_id,
            self.default_max_stream_data_bidi_remote,
            self.default_max_stream_data_bidi_local,
            std::ptr::null_mut(),
        );

        self.streams.insert(stream_id, stream);
        Ok(stream_id)
    }

    /// Open a new unidirectional stream
    pub fn open_uni_stream(&mut self) -> Result<StreamId> {
        let counter = if self.is_client {
            &self.next_client_uni
        } else {
            &self.next_server_uni
        };

        let stream_num = counter.fetch_add(4, Ordering::AcqRel);
        let stream_id = stream_num as StreamId;

        // Check stream limit
        let max_streams = self.max_local_uni_streams.load(Ordering::Acquire);
        if (stream_num >> 2) >= max_streams {
            return Err(Error::Ng(NgError::StreamIdBlocked));
        }

        // Create the stream
        let stream = Stream::new(
            stream_id,
            self.default_max_stream_data_uni,
            0, // Cannot receive on local uni stream
            std::ptr::null_mut(),
        );

        self.streams.insert(stream_id, stream);
        Ok(stream_id)
    }

    /// Get a stream by ID
    pub fn get(&self, stream_id: StreamId) -> Option<&Stream> {
        self.streams.get(&stream_id)
    }

    /// Get a mutable stream by ID
    pub fn get_mut(&mut self, stream_id: StreamId) -> Option<&mut Stream> {
        self.streams.get_mut(&stream_id)
    }

    /// Create a stream for incoming data (if not exists)
    pub fn get_or_create(&mut self, stream_id: StreamId) -> Result<&mut Stream> {
        if !self.streams.contains_key(&stream_id) {
            // Validate stream ID
            let stream_type = StreamType::from_stream_id(stream_id);
            let is_local = match stream_type {
                StreamType::ClientBidi | StreamType::ClientUni => self.is_client,
                StreamType::ServerBidi | StreamType::ServerUni => !self.is_client,
            };

            if is_local {
                // Peer can't open our local streams
                return Err(Error::Transport(TransportError::StreamStateError));
            }

            // Check stream limit
            let max_streams = match stream_type {
                StreamType::ClientBidi | StreamType::ServerBidi => {
                    self.max_remote_bidi_streams.load(Ordering::Acquire)
                }
                StreamType::ClientUni | StreamType::ServerUni => {
                    self.max_remote_uni_streams.load(Ordering::Acquire)
                }
            };

            let stream_num = (stream_id >> 2) as u64;
            if stream_num >= max_streams {
                return Err(Error::Transport(TransportError::StreamLimitError));
            }

            // Create the stream
            let (max_send, max_recv) = match stream_type {
                StreamType::ClientBidi | StreamType::ServerBidi => (
                    self.default_max_stream_data_bidi_local,
                    self.default_max_stream_data_bidi_remote,
                ),
                StreamType::ClientUni | StreamType::ServerUni => {
                    (0, self.default_max_stream_data_uni)
                }
            };

            let stream = Stream::new(stream_id, max_send, max_recv, std::ptr::null_mut());
            self.streams.insert(stream_id, stream);
        }

        Ok(self.streams.get_mut(&stream_id).unwrap())
    }

    /// Write data to a stream
    pub fn write_data(&mut self, stream_id: StreamId, data: &[u8], fin: bool) -> Result<usize> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or(Error::Ng(NgError::StreamNotFound))?;
        stream.write(data, fin)
    }

    /// Read data from a stream
    pub fn read_data(&mut self, stream_id: StreamId, dest: &mut [u8]) -> Result<(usize, bool)> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or(Error::Ng(NgError::StreamNotFound))?;
        stream.read(dest)
    }

    /// Shutdown a stream
    pub fn shutdown(&mut self, stream_id: StreamId, _flags: u32) -> Result<()> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or(Error::Ng(NgError::StreamNotFound))?;

        // Mark as finished sending
        // In full implementation, would send FIN
        Ok(())
    }

    /// Close a stream with error
    pub fn close(&mut self, stream_id: StreamId, error_code: u64) -> Result<()> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or(Error::Ng(NgError::StreamNotFound))?;

        stream.reset(error_code);
        Ok(())
    }

    /// Remove finished streams
    pub fn garbage_collect(&mut self) {
        self.streams.retain(|_, stream| !stream.is_finished());
    }

    /// Get count of active streams
    pub fn active_stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Update max streams from MAX_STREAMS frame
    pub fn update_max_streams(&self, max_streams: u64, bidi: bool) {
        if bidi {
            self.max_local_bidi_streams
                .fetch_max(max_streams, Ordering::AcqRel);
        } else {
            self.max_local_uni_streams
                .fetch_max(max_streams, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_buffer() {
        let mut buffer = StreamBuffer::new(1024);

        // Push some data
        buffer.push(0, b"hello", false).unwrap();
        buffer.push(5, b" world", true).unwrap();

        // Read data
        let mut dest = [0u8; 20];
        let (read, fin) = buffer.read(&mut dest);

        assert_eq!(read, 11);
        assert!(fin);
        assert_eq!(&dest[..11], b"hello world");
    }

    #[test]
    fn test_stream_manager() {
        let mut manager = StreamManager::new(true); // client

        // Open streams
        let bidi = manager.open_bidi_stream().unwrap();
        assert_eq!(bidi, 0);

        let uni = manager.open_uni_stream().unwrap();
        assert_eq!(uni, 2);

        // Write data
        manager.write_data(bidi, b"test", false).unwrap();
    }

    #[test]
    fn test_stream_type() {
        assert_eq!(
            StreamType::from_stream_id(0),
            StreamType::ClientBidi
        );
        assert_eq!(
            StreamType::from_stream_id(1),
            StreamType::ServerBidi
        );
        assert_eq!(
            StreamType::from_stream_id(2),
            StreamType::ClientUni
        );
        assert_eq!(
            StreamType::from_stream_id(3),
            StreamType::ServerUni
        );
    }
}
