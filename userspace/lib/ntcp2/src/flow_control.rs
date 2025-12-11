//! QUIC Flow Control
//!
//! This module implements QUIC flow control (RFC 9000 Section 4).
//!
//! ## Flow Control Levels
//!
//! - **Connection-level**: Limits total data across all streams
//! - **Stream-level**: Limits data on individual streams
//!
//! ## Credit-based System
//!
//! Flow control uses a credit-based system where the receiver advertises
//! the maximum offset it can receive. The sender must not exceed this limit.

use crate::error::{Error, Result, TransportError};
use crate::types::StreamId;
use crate::Duration;

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ============================================================================
// Flow Control Constants
// ============================================================================

/// Default initial max data
pub const DEFAULT_INITIAL_MAX_DATA: u64 = 10 * 1024 * 1024; // 10 MB

/// Default initial max stream data
pub const DEFAULT_INITIAL_MAX_STREAM_DATA: u64 = 256 * 1024; // 256 KB

/// Auto-tuning window increment factor
pub const WINDOW_INCREMENT_FACTOR: f64 = 2.0;

/// Minimum window update threshold (fraction of window)
pub const WINDOW_UPDATE_THRESHOLD: f64 = 0.5;

// ============================================================================
// Connection Flow Controller
// ============================================================================

/// Connection-level flow control
///
/// Manages the total amount of data that can be in flight across all streams.
pub struct ConnectionFlowController {
    /// Maximum data offset the peer can send
    max_data_recv: AtomicU64,
    /// Maximum data offset we can send
    max_data_send: AtomicU64,
    /// Current receive offset (highest received)
    recv_offset: AtomicU64,
    /// Current send offset (total bytes sent)
    send_offset: AtomicU64,
    /// Bytes consumed by application
    consumed_offset: AtomicU64,
    /// Initial window size (for auto-tuning)
    initial_window: u64,
    /// Maximum window size (limit for auto-tuning)
    max_window: u64,
    /// Connection is blocked on flow control
    blocked: AtomicBool,
    /// Need to send MAX_DATA frame
    need_max_data: AtomicBool,
    /// Last RTT used for auto-tuning
    last_rtt: AtomicU64,
}

impl ConnectionFlowController {
    /// Create a new connection flow controller
    pub fn new(initial_max_data_recv: u64, initial_max_data_send: u64, max_window: u64) -> Self {
        Self {
            max_data_recv: AtomicU64::new(initial_max_data_recv),
            max_data_send: AtomicU64::new(initial_max_data_send),
            recv_offset: AtomicU64::new(0),
            send_offset: AtomicU64::new(0),
            consumed_offset: AtomicU64::new(0),
            initial_window: initial_max_data_recv,
            max_window,
            blocked: AtomicBool::new(false),
            need_max_data: AtomicBool::new(false),
            last_rtt: AtomicU64::new(0),
        }
    }

    /// Check if we can send the given amount of data
    pub fn can_send(&self, bytes: u64) -> bool {
        let send_offset = self.send_offset.load(Ordering::Acquire);
        let max_send = self.max_data_send.load(Ordering::Acquire);
        send_offset + bytes <= max_send
    }

    /// Get available send credit
    pub fn send_credit(&self) -> u64 {
        let send_offset = self.send_offset.load(Ordering::Acquire);
        let max_send = self.max_data_send.load(Ordering::Acquire);
        max_send.saturating_sub(send_offset)
    }

    /// Record that we sent data
    pub fn on_data_sent(&self, bytes: u64) -> Result<()> {
        let new_offset = self.send_offset.fetch_add(bytes, Ordering::AcqRel) + bytes;
        let max_send = self.max_data_send.load(Ordering::Acquire);

        if new_offset > max_send {
            self.blocked.store(true, Ordering::Release);
            return Err(Error::Transport(TransportError::FlowControlError));
        }

        Ok(())
    }

    /// Record that peer sent us data
    pub fn on_data_received(&self, offset: u64, len: u64) -> Result<()> {
        let end_offset = offset + len;
        let max_recv = self.max_data_recv.load(Ordering::Acquire);

        // Update highest received offset
        let _ = self
            .recv_offset
            .fetch_max(end_offset, Ordering::AcqRel);

        // Check if peer exceeded our limit
        if end_offset > max_recv {
            return Err(Error::Transport(TransportError::FlowControlError));
        }

        Ok(())
    }

    /// Record that application consumed data
    pub fn on_data_consumed(&self, bytes: u64) {
        let old_consumed = self.consumed_offset.fetch_add(bytes, Ordering::AcqRel);
        let new_consumed = old_consumed + bytes;
        let max_recv = self.max_data_recv.load(Ordering::Acquire);

        // Check if we should send MAX_DATA
        let window = max_recv - new_consumed;
        let threshold = (max_recv as f64 * WINDOW_UPDATE_THRESHOLD) as u64;

        if window < threshold {
            self.need_max_data.store(true, Ordering::Release);
        }
    }

    /// Update max data we can send (from MAX_DATA frame)
    pub fn update_max_data_send(&self, max_data: u64) {
        let old = self.max_data_send.fetch_max(max_data, Ordering::AcqRel);
        if max_data > old {
            self.blocked.store(false, Ordering::Release);
        }
    }

    /// Get new MAX_DATA value to send (if needed)
    pub fn get_max_data_update(&self) -> Option<u64> {
        if !self.need_max_data.swap(false, Ordering::AcqRel) {
            return None;
        }

        let consumed = self.consumed_offset.load(Ordering::Acquire);
        let current_max = self.max_data_recv.load(Ordering::Acquire);

        // Calculate new window (with auto-tuning)
        let current_window = current_max - consumed;
        let new_window = (current_window as f64 * WINDOW_INCREMENT_FACTOR) as u64;
        let new_window = new_window.min(self.max_window);

        let new_max = consumed + new_window;

        // Update our max
        self.max_data_recv.store(new_max, Ordering::Release);

        Some(new_max)
    }

    /// Check if connection is blocked
    pub fn is_blocked(&self) -> bool {
        self.blocked.load(Ordering::Acquire)
    }

    /// Update RTT for auto-tuning
    pub fn update_rtt(&self, rtt: Duration) {
        self.last_rtt.store(rtt, Ordering::Release);
    }

    /// Get current statistics
    pub fn stats(&self) -> FlowControlStats {
        FlowControlStats {
            max_data_recv: self.max_data_recv.load(Ordering::Acquire),
            max_data_send: self.max_data_send.load(Ordering::Acquire),
            recv_offset: self.recv_offset.load(Ordering::Acquire),
            send_offset: self.send_offset.load(Ordering::Acquire),
            consumed_offset: self.consumed_offset.load(Ordering::Acquire),
            is_blocked: self.blocked.load(Ordering::Acquire),
        }
    }
}

// ============================================================================
// Stream Flow Controller
// ============================================================================

/// Stream-level flow control
///
/// Manages flow control for a single stream.
pub struct StreamFlowController {
    /// Stream ID
    stream_id: StreamId,
    /// Maximum data offset the peer can send
    max_data_recv: AtomicU64,
    /// Maximum data offset we can send
    max_data_send: AtomicU64,
    /// Current receive offset
    recv_offset: AtomicU64,
    /// Current send offset
    send_offset: AtomicU64,
    /// Bytes consumed by application
    consumed_offset: AtomicU64,
    /// Stream is blocked on flow control
    blocked: AtomicBool,
    /// Need to send MAX_STREAM_DATA frame
    need_max_stream_data: AtomicBool,
    /// Maximum window size
    max_window: u64,
}

impl StreamFlowController {
    /// Create a new stream flow controller
    pub fn new(stream_id: StreamId, initial_max_recv: u64, initial_max_send: u64) -> Self {
        Self {
            stream_id,
            max_data_recv: AtomicU64::new(initial_max_recv),
            max_data_send: AtomicU64::new(initial_max_send),
            recv_offset: AtomicU64::new(0),
            send_offset: AtomicU64::new(0),
            consumed_offset: AtomicU64::new(0),
            blocked: AtomicBool::new(false),
            need_max_stream_data: AtomicBool::new(false),
            max_window: initial_max_recv * 4, // Allow auto-tuning up to 4x
        }
    }

    /// Check if we can send the given amount of data
    pub fn can_send(&self, bytes: u64) -> bool {
        let send_offset = self.send_offset.load(Ordering::Acquire);
        let max_send = self.max_data_send.load(Ordering::Acquire);
        send_offset + bytes <= max_send
    }

    /// Get available send credit
    pub fn send_credit(&self) -> u64 {
        let send_offset = self.send_offset.load(Ordering::Acquire);
        let max_send = self.max_data_send.load(Ordering::Acquire);
        max_send.saturating_sub(send_offset)
    }

    /// Record that we sent data
    pub fn on_data_sent(&self, bytes: u64) -> Result<()> {
        let new_offset = self.send_offset.fetch_add(bytes, Ordering::AcqRel) + bytes;
        let max_send = self.max_data_send.load(Ordering::Acquire);

        if new_offset > max_send {
            self.blocked.store(true, Ordering::Release);
            return Err(Error::Transport(TransportError::FlowControlError));
        }

        Ok(())
    }

    /// Record that peer sent us data
    pub fn on_data_received(&self, offset: u64, len: u64) -> Result<()> {
        let end_offset = offset + len;
        let max_recv = self.max_data_recv.load(Ordering::Acquire);

        // Update highest received offset
        let _ = self.recv_offset.fetch_max(end_offset, Ordering::AcqRel);

        // Check if peer exceeded our limit
        if end_offset > max_recv {
            return Err(Error::Transport(TransportError::FlowControlError));
        }

        Ok(())
    }

    /// Record that application consumed data
    pub fn on_data_consumed(&self, bytes: u64) {
        let old_consumed = self.consumed_offset.fetch_add(bytes, Ordering::AcqRel);
        let new_consumed = old_consumed + bytes;
        let max_recv = self.max_data_recv.load(Ordering::Acquire);

        // Check if we should send MAX_STREAM_DATA
        let window = max_recv - new_consumed;
        let threshold = (max_recv as f64 * WINDOW_UPDATE_THRESHOLD) as u64;

        if window < threshold {
            self.need_max_stream_data.store(true, Ordering::Release);
        }
    }

    /// Update max data we can send (from MAX_STREAM_DATA frame)
    pub fn update_max_data_send(&self, max_data: u64) {
        let old = self.max_data_send.fetch_max(max_data, Ordering::AcqRel);
        if max_data > old {
            self.blocked.store(false, Ordering::Release);
        }
    }

    /// Get new MAX_STREAM_DATA value to send (if needed)
    pub fn get_max_stream_data_update(&self) -> Option<u64> {
        if !self.need_max_stream_data.swap(false, Ordering::AcqRel) {
            return None;
        }

        let consumed = self.consumed_offset.load(Ordering::Acquire);
        let current_max = self.max_data_recv.load(Ordering::Acquire);

        // Calculate new window
        let current_window = current_max - consumed;
        let new_window = (current_window as f64 * WINDOW_INCREMENT_FACTOR) as u64;
        let new_window = new_window.min(self.max_window);

        let new_max = consumed + new_window;

        // Update our max
        self.max_data_recv.store(new_max, Ordering::Release);

        Some(new_max)
    }

    /// Check if stream is blocked
    pub fn is_blocked(&self) -> bool {
        self.blocked.load(Ordering::Acquire)
    }

    /// Get stream ID
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }
}

// ============================================================================
// Flow Control Manager
// ============================================================================

/// Manages flow control for a connection and all its streams
pub struct FlowControlManager {
    /// Connection-level flow control
    connection: ConnectionFlowController,
    /// Stream-level flow controllers
    streams: RwLock<HashMap<StreamId, StreamFlowController>>,
    /// Default initial max stream data (bidi local)
    default_max_stream_data_bidi_local: u64,
    /// Default initial max stream data (bidi remote)
    default_max_stream_data_bidi_remote: u64,
    /// Default initial max stream data (uni)
    default_max_stream_data_uni: u64,
    /// Maximum window for auto-tuning
    max_window: u64,
}

impl FlowControlManager {
    /// Create a new flow control manager
    pub fn new(
        initial_max_data: u64,
        max_data_send: u64,
        max_stream_data_bidi_local: u64,
        max_stream_data_bidi_remote: u64,
        max_stream_data_uni: u64,
        max_window: u64,
    ) -> Self {
        Self {
            connection: ConnectionFlowController::new(initial_max_data, max_data_send, max_window),
            streams: RwLock::new(HashMap::new()),
            default_max_stream_data_bidi_local: max_stream_data_bidi_local,
            default_max_stream_data_bidi_remote: max_stream_data_bidi_remote,
            default_max_stream_data_uni: max_stream_data_uni,
            max_window,
        }
    }

    /// Get connection flow controller
    pub fn connection(&self) -> &ConnectionFlowController {
        &self.connection
    }

    /// Register a new stream
    pub fn register_stream(&self, stream_id: StreamId, is_bidi: bool, is_local: bool) {
        let (max_recv, max_send) = if is_bidi {
            if is_local {
                (
                    self.default_max_stream_data_bidi_local,
                    self.default_max_stream_data_bidi_remote,
                )
            } else {
                (
                    self.default_max_stream_data_bidi_remote,
                    self.default_max_stream_data_bidi_local,
                )
            }
        } else {
            if is_local {
                (0, self.default_max_stream_data_uni)
            } else {
                (self.default_max_stream_data_uni, 0)
            }
        };

        let controller = StreamFlowController::new(stream_id, max_recv, max_send);
        self.streams.write().insert(stream_id, controller);
    }

    /// Remove a stream
    pub fn remove_stream(&self, stream_id: StreamId) {
        self.streams.write().remove(&stream_id);
    }

    /// Check if we can send data on a stream
    pub fn can_send(&self, stream_id: StreamId, bytes: u64) -> bool {
        // Check connection-level
        if !self.connection.can_send(bytes) {
            return false;
        }

        // Check stream-level
        let streams = self.streams.read();
        if let Some(stream) = streams.get(&stream_id) {
            stream.can_send(bytes)
        } else {
            false
        }
    }

    /// Get available send credit for a stream
    pub fn send_credit(&self, stream_id: StreamId) -> u64 {
        let conn_credit = self.connection.send_credit();

        let streams = self.streams.read();
        if let Some(stream) = streams.get(&stream_id) {
            conn_credit.min(stream.send_credit())
        } else {
            0
        }
    }

    /// Record data sent on a stream
    pub fn on_data_sent(&self, stream_id: StreamId, bytes: u64) -> Result<()> {
        // Update connection-level
        self.connection.on_data_sent(bytes)?;

        // Update stream-level
        let streams = self.streams.read();
        if let Some(stream) = streams.get(&stream_id) {
            stream.on_data_sent(bytes)?;
        }

        Ok(())
    }

    /// Record data received on a stream
    pub fn on_data_received(&self, stream_id: StreamId, offset: u64, len: u64) -> Result<()> {
        // Update connection-level (using offset 0 for simplicity)
        self.connection.on_data_received(0, len)?;

        // Update stream-level
        let streams = self.streams.read();
        if let Some(stream) = streams.get(&stream_id) {
            stream.on_data_received(offset, len)?;
        } else {
            // Stream not found - might be a new stream
            drop(streams);
            // Register the stream and try again
            let is_bidi = (stream_id & 0x02) == 0;
            self.register_stream(stream_id, is_bidi, false);

            let streams = self.streams.read();
            if let Some(stream) = streams.get(&stream_id) {
                stream.on_data_received(offset, len)?;
            }
        }

        Ok(())
    }

    /// Record data consumed on a stream
    pub fn on_data_consumed(&self, stream_id: StreamId, bytes: u64) {
        // Update connection-level
        self.connection.on_data_consumed(bytes);

        // Update stream-level
        let streams = self.streams.read();
        if let Some(stream) = streams.get(&stream_id) {
            stream.on_data_consumed(bytes);
        }
    }

    /// Update max data from MAX_DATA frame
    pub fn update_max_data(&self, max_data: u64) {
        self.connection.update_max_data_send(max_data);
    }

    /// Update max stream data from MAX_STREAM_DATA frame
    pub fn update_max_stream_data(&self, stream_id: StreamId, max_data: u64) {
        let streams = self.streams.read();
        if let Some(stream) = streams.get(&stream_id) {
            stream.update_max_data_send(max_data);
        }
    }

    /// Get pending MAX_DATA frame value (if needed)
    pub fn get_max_data_update(&self) -> Option<u64> {
        self.connection.get_max_data_update()
    }

    /// Get pending MAX_STREAM_DATA updates
    pub fn get_max_stream_data_updates(&self) -> Vec<(StreamId, u64)> {
        let streams = self.streams.read();
        let mut updates = Vec::new();

        for (stream_id, controller) in streams.iter() {
            if let Some(max_data) = controller.get_max_stream_data_update() {
                updates.push((*stream_id, max_data));
            }
        }

        updates
    }

    /// Get blocked streams
    pub fn get_blocked_streams(&self) -> Vec<StreamId> {
        let streams = self.streams.read();
        streams
            .iter()
            .filter(|(_, c)| c.is_blocked())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Check if connection is blocked
    pub fn is_connection_blocked(&self) -> bool {
        self.connection.is_blocked()
    }

    /// Update RTT for auto-tuning
    pub fn update_rtt(&self, rtt: Duration) {
        self.connection.update_rtt(rtt);
    }
}

// ============================================================================
// Flow Control Statistics
// ============================================================================

/// Flow control statistics
#[derive(Debug, Clone, Default)]
pub struct FlowControlStats {
    /// Maximum data we can receive
    pub max_data_recv: u64,
    /// Maximum data we can send
    pub max_data_send: u64,
    /// Current receive offset
    pub recv_offset: u64,
    /// Current send offset
    pub send_offset: u64,
    /// Consumed offset
    pub consumed_offset: u64,
    /// Whether we're blocked
    pub is_blocked: bool,
}

// ============================================================================
// Stream Limits
// ============================================================================

/// Stream limit controller
///
/// Manages the number of streams that can be opened.
pub struct StreamLimitController {
    /// Maximum local bidirectional streams
    max_local_bidi: AtomicU64,
    /// Maximum remote bidirectional streams
    max_remote_bidi: AtomicU64,
    /// Maximum local unidirectional streams
    max_local_uni: AtomicU64,
    /// Maximum remote unidirectional streams
    max_remote_uni: AtomicU64,
    /// Current local bidirectional streams
    current_local_bidi: AtomicU64,
    /// Current remote bidirectional streams
    current_remote_bidi: AtomicU64,
    /// Current local unidirectional streams
    current_local_uni: AtomicU64,
    /// Current remote unidirectional streams
    current_remote_uni: AtomicU64,
    /// Need to send MAX_STREAMS (bidi)
    need_max_streams_bidi: AtomicBool,
    /// Need to send MAX_STREAMS (uni)
    need_max_streams_uni: AtomicBool,
}

impl StreamLimitController {
    /// Create a new stream limit controller
    pub fn new(
        max_local_bidi: u64,
        max_remote_bidi: u64,
        max_local_uni: u64,
        max_remote_uni: u64,
    ) -> Self {
        Self {
            max_local_bidi: AtomicU64::new(max_local_bidi),
            max_remote_bidi: AtomicU64::new(max_remote_bidi),
            max_local_uni: AtomicU64::new(max_local_uni),
            max_remote_uni: AtomicU64::new(max_remote_uni),
            current_local_bidi: AtomicU64::new(0),
            current_remote_bidi: AtomicU64::new(0),
            current_local_uni: AtomicU64::new(0),
            current_remote_uni: AtomicU64::new(0),
            need_max_streams_bidi: AtomicBool::new(false),
            need_max_streams_uni: AtomicBool::new(false),
        }
    }

    /// Check if we can open a local bidirectional stream
    pub fn can_open_local_bidi(&self) -> bool {
        let current = self.current_local_bidi.load(Ordering::Acquire);
        let max = self.max_local_bidi.load(Ordering::Acquire);
        current < max
    }

    /// Check if we can open a local unidirectional stream
    pub fn can_open_local_uni(&self) -> bool {
        let current = self.current_local_uni.load(Ordering::Acquire);
        let max = self.max_local_uni.load(Ordering::Acquire);
        current < max
    }

    /// Record opening a local bidirectional stream
    pub fn on_open_local_bidi(&self) -> Result<()> {
        if !self.can_open_local_bidi() {
            return Err(Error::Transport(TransportError::StreamLimitError));
        }
        self.current_local_bidi.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Record opening a local unidirectional stream
    pub fn on_open_local_uni(&self) -> Result<()> {
        if !self.can_open_local_uni() {
            return Err(Error::Transport(TransportError::StreamLimitError));
        }
        self.current_local_uni.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Check if remote can open a bidirectional stream
    pub fn can_open_remote_bidi(&self) -> bool {
        let current = self.current_remote_bidi.load(Ordering::Acquire);
        let max = self.max_remote_bidi.load(Ordering::Acquire);
        current < max
    }

    /// Check if remote can open a unidirectional stream
    pub fn can_open_remote_uni(&self) -> bool {
        let current = self.current_remote_uni.load(Ordering::Acquire);
        let max = self.max_remote_uni.load(Ordering::Acquire);
        current < max
    }

    /// Record remote opening a bidirectional stream
    pub fn on_open_remote_bidi(&self) -> Result<()> {
        if !self.can_open_remote_bidi() {
            return Err(Error::Transport(TransportError::StreamLimitError));
        }
        self.current_remote_bidi.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Record remote opening a unidirectional stream
    pub fn on_open_remote_uni(&self) -> Result<()> {
        if !self.can_open_remote_uni() {
            return Err(Error::Transport(TransportError::StreamLimitError));
        }
        self.current_remote_uni.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Record closing a local stream
    pub fn on_close_local(&self, bidi: bool) {
        if bidi {
            self.current_local_bidi.fetch_sub(1, Ordering::AcqRel);
        } else {
            self.current_local_uni.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// Record closing a remote stream
    pub fn on_close_remote(&self, bidi: bool) {
        if bidi {
            let current = self.current_remote_bidi.fetch_sub(1, Ordering::AcqRel);
            let max = self.max_remote_bidi.load(Ordering::Acquire);
            // Check if we should increase limit
            if current <= max / 2 {
                self.need_max_streams_bidi.store(true, Ordering::Release);
            }
        } else {
            let current = self.current_remote_uni.fetch_sub(1, Ordering::AcqRel);
            let max = self.max_remote_uni.load(Ordering::Acquire);
            if current <= max / 2 {
                self.need_max_streams_uni.store(true, Ordering::Release);
            }
        }
    }

    /// Update max local streams from MAX_STREAMS frame
    pub fn update_max_local(&self, max: u64, bidi: bool) {
        if bidi {
            self.max_local_bidi.fetch_max(max, Ordering::AcqRel);
        } else {
            self.max_local_uni.fetch_max(max, Ordering::AcqRel);
        }
    }

    /// Get MAX_STREAMS update (if needed)
    pub fn get_max_streams_update(&self, bidi: bool) -> Option<u64> {
        let need = if bidi {
            &self.need_max_streams_bidi
        } else {
            &self.need_max_streams_uni
        };

        if !need.swap(false, Ordering::AcqRel) {
            return None;
        }

        let max = if bidi {
            &self.max_remote_bidi
        } else {
            &self.max_remote_uni
        };

        // Increase by 2x
        let old_max = max.load(Ordering::Acquire);
        let new_max = old_max * 2;
        max.store(new_max, Ordering::Release);

        Some(new_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_flow_control() {
        let fc = ConnectionFlowController::new(1000, 1000, 10000);

        assert!(fc.can_send(500));
        fc.on_data_sent(500).unwrap();

        assert!(fc.can_send(500));
        assert!(!fc.can_send(501));

        fc.on_data_sent(500).unwrap();
        assert!(!fc.can_send(1));
        assert!(fc.is_blocked());
    }

    #[test]
    fn test_stream_flow_control() {
        let fc = StreamFlowController::new(0, 1000, 1000);

        assert!(fc.can_send(500));
        fc.on_data_sent(500).unwrap();

        assert_eq!(fc.send_credit(), 500);
    }

    #[test]
    fn test_flow_control_manager() {
        let fcm = FlowControlManager::new(10000, 10000, 1000, 1000, 1000, 100000);

        fcm.register_stream(0, true, true);

        assert!(fcm.can_send(0, 500));
        fcm.on_data_sent(0, 500).unwrap();

        assert_eq!(fcm.send_credit(0), 500);
    }

    #[test]
    fn test_stream_limits() {
        let slc = StreamLimitController::new(10, 10, 5, 5);

        assert!(slc.can_open_local_bidi());
        slc.on_open_local_bidi().unwrap();

        assert!(slc.can_open_remote_uni());
        slc.on_open_remote_uni().unwrap();
    }
}
