//! IPC (Inter-Process Communication) tests
//!
//! This module contains all IPC-related tests including:
//! - Signal delivery and handling
//! - Signal edge cases and POSIX compliance
//! - Futex operations for pthread support
//! - Pipes and ring buffers
//! - Message queues

mod comprehensive;
mod futex;
mod pipe;
mod signal;
mod signal_advanced;
