//! Process Management Tests
//!
//! Tests for process creation, state management, and lifecycle.
//! This module includes:
//! - Process context (register state)
//! - Process state transitions and state machine
//! - Thread management
//! - PID allocation and tree management
//! - Fork/clone edge cases
//! - Comprehensive lifecycle tests

mod comprehensive;
mod context;
mod fork;
mod pid_tree;
mod state;
mod state_machine;
mod thread;
mod types;

pub use comprehensive::*;
pub use context::*;
pub use fork::*;
pub use pid_tree::*;
pub use state::*;
pub use state_machine::*;
pub use thread::*;
pub use types::*;
