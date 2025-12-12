//! Terminal (TTY) subsystem for NexaOS
//!
//! This module contains terminal-related functionality including:
//! - Virtual terminal management
//! - Console I/O

pub mod vt;
pub mod pty;

// Re-export commonly used items
pub use vt::{
    active_terminal, echo_input_backspace, echo_input_byte, echo_input_newline, init, switch_to,
    terminal_count, write_bytes, StreamKind,
};
