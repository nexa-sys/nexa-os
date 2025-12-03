//! Constants for stdio module
//!
//! System call numbers, file descriptors, error codes, and buffer sizes.

// System call numbers
pub(crate) const SYS_READ: u64 = 0;
pub(crate) const SYS_WRITE: u64 = 1;

// Standard file descriptors
pub(crate) const STDIN: i32 = 0;
pub(crate) const STDOUT: i32 = 1;
pub(crate) const STDERR: i32 = 2;

// Error codes
pub(crate) const EAGAIN: i32 = 11; // Resource temporarily unavailable (POSIX error code)

// Buffer sizes
pub(crate) const BUFFER_CAPACITY: usize = 512;
pub(crate) const INT_BUFFER_SIZE: usize = 128;
pub(crate) const FLOAT_BUFFER_SIZE: usize = 128;

// Float formatting defaults
pub(crate) const DEFAULT_FLOAT_PRECISION: usize = 6;
pub(crate) const MAX_FLOAT_PRECISION: usize = 18;

// Printf format flags
pub(crate) const FLAG_LEFT: u8 = 0x01;
pub(crate) const FLAG_PLUS: u8 = 0x02;
pub(crate) const FLAG_SPACE: u8 = 0x04;
pub(crate) const FLAG_ALT: u8 = 0x08;
pub(crate) const FLAG_ZERO: u8 = 0x10;
