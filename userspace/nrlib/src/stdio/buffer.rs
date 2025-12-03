//! Buffer management for stdio streams
//!
//! This module provides the internal buffering types and modes used by FILE streams.

use super::constants::BUFFER_CAPACITY;

/// Buffering mode for a FILE stream
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BufferMode {
    /// No buffering - write/read immediately
    Unbuffered = 0,
    /// Line buffered - flush on newline
    Line = 1,
    /// Fully buffered - flush when buffer is full
    Full = 2,
}

/// Last operation performed on a stream (for read/write mode switching)
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LastOp {
    None = 0,
    Read = 1,
    Write = 2,
}

/// Internal buffer for FILE streams
pub(crate) struct FileBuffer {
    pub(crate) data: [u8; BUFFER_CAPACITY],
    /// Current read position in buffer
    pub(crate) pos: usize,
    /// Number of valid bytes in buffer
    pub(crate) len: usize,
}

impl FileBuffer {
    /// Create a new empty buffer
    pub(crate) const fn new() -> Self {
        Self {
            data: [0; BUFFER_CAPACITY],
            pos: 0,
            len: 0,
        }
    }

    /// Reset buffer to empty state
    pub(crate) fn clear(&mut self) {
        self.pos = 0;
        self.len = 0;
    }
}
