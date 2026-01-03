//! TTY Subsystem Tests
//!
//! Tests for virtual terminals, PTY, and character handling.
//! Uses REAL kernel code - no simulated implementations.
//!
//! ## Coverage Areas
//! - Virtual terminal constants
//! - Cell and color handling
//! - Terminal buffer operations
//! - Stream kinds (stdout, stderr, input)

mod vt;
mod pty;

pub use vt::*;
pub use pty::*;
