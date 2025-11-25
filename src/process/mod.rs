//! Process management subsystem
//!
//! This module provides the process abstraction for NexaOS, including
//! process creation, ELF loading, and user-mode execution.
//!
//! ## Module Organization
//!
//! - `types`: Type definitions (Pid, ProcessState, Context, Process) and constants
//! - `stack`: User stack building utilities
//! - `loader`: ELF loading for process creation
//! - `execution`: Process execution and user-mode transition

extern crate alloc;

mod execution;
mod loader;
mod stack;
mod types;

// Re-export all types for external use
pub use types::{
    allocate_pid, Context, Pid, Process, ProcessState, DEFAULT_ARGV0, HEAP_BASE, HEAP_SIZE,
    INTERP_BASE, INTERP_REGION_SIZE, KERNEL_STACK_ALIGN, KERNEL_STACK_SIZE, MAX_PROCESSES,
    MAX_PROCESS_ARGS, STACK_BASE, STACK_SIZE, USER_PHYS_BASE, USER_REGION_SIZE, USER_VIRT_BASE,
};

// Re-export execution functions
pub use execution::{
    get_user_entry, get_user_stack, jump_to_usermode, jump_to_usermode_with_cr3,
};
