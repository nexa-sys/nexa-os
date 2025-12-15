//! NexaOS Test Suite
//!
//! This crate tests kernel code by directly including kernel source files.
//! This bypasses no_std restrictions while testing the actual kernel logic.
//!
//! # How it works
//! We use `#[path = "..."]` to include kernel source files directly.
//! The `core::` references in kernel code work because std re-exports core.
//!
//! # Note
//! Only include files that don't depend on kernel-specific macros (ktrace, kinfo, etc.)

// Import kernel source files directly using #[path]
// These are pure logic modules that don't depend on kernel infrastructure

#[path = "../../src/net/ipv4.rs"]
pub mod ipv4;

// Test modules
#[cfg(test)]
mod tests;


