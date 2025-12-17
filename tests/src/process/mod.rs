//! Process Management Tests
//!
//! Tests for process creation, state management, and lifecycle.
//! This module includes:
//! - Process context (register state)
//! - Process state transitions  
//! - Thread management
//! - PID allocation and tree management
//! - Fork/clone edge cases
//! - Comprehensive lifecycle tests

mod comprehensive;
mod context;
mod fork;
mod state;
mod thread;
mod types;

pub use comprehensive::*;
pub use context::*;
pub use fork::*;
pub use state::*;
pub use thread::*;
pub use types::*;
