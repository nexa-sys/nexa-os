//! NexaOS Test Suite
//!
//! This crate provides unit tests for NexaOS kernel components.
//! Tests run in a standard Rust environment (with std), not bare-metal.
//!
//! The design philosophy:
//! - Keep tests separate from kernel code (no `#[cfg(test)]` pollution)
//! - Test pure logic that doesn't require hardware (algorithms, data structures)
//! - Mock hardware-dependent code when necessary
//!
//! # Structure
//!
//! - `posix/` - POSIX types and error codes
//! - `algorithms/` - Core algorithms (bitmap, ring buffer, etc.)
//! - `data_structures/` - Data structure implementations
//! - `mock/` - Mock implementations for testing

pub mod posix;
pub mod algorithms;
pub mod data_structures;
pub mod mock;
