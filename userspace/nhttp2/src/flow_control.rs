//! Flow Control for HTTP/2
//!
//! Implements the flow control mechanism defined in RFC 7540 Section 5.2.

use crate::types::StreamId;
use crate::error::{Error, Result, ErrorCode};
use crate::constants::{DEFAULT_INITIAL_WINDOW_SIZE, MAX_WINDOW_SIZE, DEFAULT_WINDOW_UPDATE_THRESHOLD};

/// Flow control manager
#[derive(Debug)]
pub struct FlowControl {
    /// Connection-level send window
    connection_send_window: i32,
    /// Connection-level receive window
    connection_recv_window: i32,
    /// Initial window size for streams
    initial_window_size: i32,
    /// Window update threshold
    window_update_threshold: i32,
    /// Track consumed receive window
    consumed_recv: i32,
}

impl FlowControl {
    /// Create new flow control with default settings
    pub fn new() -> Self {
        Self {
            connection_send_window: DEFAULT_INITIAL_WINDOW_SIZE as i32,
            connection_recv_window: DEFAULT_INITIAL_WINDOW_SIZE as i32,
            initial_window_size: DEFAULT_INITIAL_WINDOW_SIZE as i32,
            window_update_threshold: DEFAULT_WINDOW_UPDATE_THRESHOLD as i32,
            consumed_recv: 0,
        }
    }

    /// Get connection send window
    pub fn connection_send_window(&self) -> i32 {
        self.connection_send_window
    }

    /// Get connection receive window
    pub fn connection_recv_window(&self) -> i32 {
        self.connection_recv_window
    }

    /// Get initial window size
    pub fn initial_window_size(&self) -> i32 {
        self.initial_window_size
    }

    /// Set initial window size
    pub fn set_initial_window_size(&mut self, size: i32) -> Result<()> {
        if size < 0 || size > MAX_WINDOW_SIZE as i32 {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.initial_window_size = size;
        Ok(())
    }

    /// Consume connection send window
    pub fn consume_send(&mut self, size: i32) -> Result<()> {
        if size > self.connection_send_window {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.connection_send_window -= size;
        Ok(())
    }

    /// Consume connection receive window
    pub fn consume_recv(&mut self, size: i32) -> Result<()> {
        if size > self.connection_recv_window {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.connection_recv_window -= size;
        self.consumed_recv += size;
        Ok(())
    }

    /// Update connection send window (from WINDOW_UPDATE)
    pub fn update_send(&mut self, increment: i32) -> Result<()> {
        let new_size = self.connection_send_window.saturating_add(increment);
        if new_size > MAX_WINDOW_SIZE as i32 {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.connection_send_window = new_size;
        Ok(())
    }

    /// Update connection receive window
    pub fn update_recv(&mut self, increment: i32) -> Result<()> {
        let new_size = self.connection_recv_window.saturating_add(increment);
        if new_size > MAX_WINDOW_SIZE as i32 {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.connection_recv_window = new_size;
        Ok(())
    }

    /// Check if we should send a connection-level WINDOW_UPDATE
    pub fn should_send_window_update(&self) -> Option<i32> {
        if self.consumed_recv >= self.window_update_threshold {
            Some(self.consumed_recv)
        } else {
            None
        }
    }

    /// Mark WINDOW_UPDATE as sent
    pub fn window_update_sent(&mut self) {
        self.connection_recv_window += self.consumed_recv;
        self.consumed_recv = 0;
    }

    /// Calculate available send capacity
    pub fn available_send(&self, stream_window: i32) -> i32 {
        core::cmp::min(self.connection_send_window, stream_window)
    }

    /// Set window update threshold
    pub fn set_window_update_threshold(&mut self, threshold: i32) {
        self.window_update_threshold = threshold;
    }
}

impl Default for FlowControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-stream flow control
#[derive(Debug, Clone, Copy)]
pub struct StreamFlowControl {
    /// Stream send window
    pub send_window: i32,
    /// Stream receive window
    pub recv_window: i32,
    /// Consumed receive bytes
    pub consumed: i32,
}

impl StreamFlowControl {
    /// Create new stream flow control
    pub fn new(initial_window_size: i32) -> Self {
        Self {
            send_window: initial_window_size,
            recv_window: initial_window_size,
            consumed: 0,
        }
    }

    /// Consume send window
    pub fn consume_send(&mut self, size: i32) -> Result<()> {
        if size > self.send_window {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.send_window -= size;
        Ok(())
    }

    /// Consume receive window
    pub fn consume_recv(&mut self, size: i32) -> Result<()> {
        if size > self.recv_window {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.recv_window -= size;
        self.consumed += size;
        Ok(())
    }

    /// Update send window
    pub fn update_send(&mut self, increment: i32) -> Result<()> {
        let new_size = self.send_window.saturating_add(increment);
        if new_size > MAX_WINDOW_SIZE as i32 {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.send_window = new_size;
        Ok(())
    }

    /// Update receive window
    pub fn update_recv(&mut self, increment: i32) -> Result<()> {
        let new_size = self.recv_window.saturating_add(increment);
        if new_size > MAX_WINDOW_SIZE as i32 {
            return Err(Error::Protocol(ErrorCode::FlowControlError));
        }
        self.recv_window = new_size;
        Ok(())
    }

    /// Check if should send WINDOW_UPDATE
    pub fn should_send_update(&self, threshold: i32) -> Option<i32> {
        if self.consumed >= threshold {
            Some(self.consumed)
        } else {
            None
        }
    }

    /// Mark WINDOW_UPDATE as sent
    pub fn update_sent(&mut self) {
        self.recv_window += self.consumed;
        self.consumed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_control_consume() {
        let mut fc = FlowControl::new();
        
        fc.consume_send(1000).unwrap();
        assert_eq!(fc.connection_send_window(), 65535 - 1000);
        
        fc.consume_recv(2000).unwrap();
        assert_eq!(fc.connection_recv_window(), 65535 - 2000);
    }

    #[test]
    fn test_flow_control_update() {
        let mut fc = FlowControl::new();
        
        fc.consume_send(10000).unwrap();
        fc.update_send(5000).unwrap();
        assert_eq!(fc.connection_send_window(), 65535 - 10000 + 5000);
    }

    #[test]
    fn test_window_update_threshold() {
        let mut fc = FlowControl::new();
        
        // Consume less than threshold
        fc.consume_recv(16384).unwrap();
        assert!(fc.should_send_window_update().is_none());
        
        // Consume past threshold
        fc.consume_recv(16385).unwrap();
        assert!(fc.should_send_window_update().is_some());
    }
}
