//! System Call Tests
//!
//! Tests for syscall implementations including:
//! - Memory management (mmap, mprotect, munmap, brk)
//! - File operations
//! - Process operations
//! - Network operations
//! - Clone/thread operations
//! - Exec context race conditions
//! - POSIX type definitions
//! - Syscall number assignments

mod clone_comprehensive;
mod clone_edge_cases;
mod exec_context;
mod memory;
mod memory_tests;
mod mmap_edge_cases;
mod numbers;
mod parameter_stress;
mod parameter_validation;
mod types;
