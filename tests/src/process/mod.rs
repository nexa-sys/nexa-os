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
mod context_switch;
mod elf_loader;
mod fork;
mod memory_layout;
mod memory_layout_validation;
mod pid_edge_cases;
mod pid_tree;
mod state;
mod state_machine;
mod state_machine_validation;
mod thread;
mod types;

pub use comprehensive::*;
pub use context::*;
pub use context_switch::*;
pub use elf_loader::*;
pub use fork::*;
pub use memory_layout::*;
pub use pid_tree::*;
pub use state::*;
pub use state_machine::*;
pub use thread::*;
pub use types::*;
