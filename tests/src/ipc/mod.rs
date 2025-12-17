//! IPC (Inter-Process Communication) tests
//!
//! This module contains all IPC-related tests including:
//! - Signal delivery and handling
//! - Signal edge cases and POSIX compliance
//! - Futex operations for pthread support
//! - Pipes and ring buffers
//! - Socketpair bidirectional communication
//! - Message queues

mod comprehensive;
mod futex;
mod futex_edge_cases;
mod pipe;
mod pipe_boundary;
mod pipe_edge_cases;
mod signal;
mod signal_advanced;
mod signal_boundary;
mod signal_comprehensive;
mod signal_edge_cases;
mod socketpair;
