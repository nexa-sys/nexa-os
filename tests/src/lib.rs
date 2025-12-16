//! NexaOS Test Suite
//!
//! This crate tests kernel code by directly including kernel source files.
//! This bypasses no_std restrictions while testing the actual kernel logic.
//!
//! # How it works
//! 1. We define stub macros (kinfo!, ktrace!, etc.) that map to println! or no-op
//! 2. We use `#[path = "..."]` to include kernel source files directly
//! 3. The `core::` references in kernel code work because std re-exports core
//!
//! This allows testing real kernel code without running in QEMU.

// ===========================================================================
// Kernel macro stubs - these replace the kernel's logging macros for testing
// ===========================================================================

/// Stub for kernel's kinfo! macro - prints to stdout in tests
#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {{
        #[cfg(test)]
        eprintln!("[INFO] {}", format_args!($($arg)*));
    }};
}

/// Stub for kernel's ktrace! macro - no-op in tests (too verbose)
#[macro_export]
macro_rules! ktrace {
    ($($arg:tt)*) => {{}};
}

/// Stub for kernel's kwarn! macro - prints to stderr in tests
#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {{
        #[cfg(test)]
        eprintln!("[WARN] {}", format_args!($($arg)*));
    }};
}

/// Stub for kernel's kerror! macro - prints to stderr in tests
#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {{
        #[cfg(test)]
        eprintln!("[ERROR] {}", format_args!($($arg)*));
    }};
}

/// Stub for kernel's kfatal! macro - prints to stderr in tests
#[macro_export]
macro_rules! kfatal {
    ($($arg:tt)*) => {{
        #[cfg(test)]
        eprintln!("[FATAL] {}", format_args!($($arg)*));
    }};
}

/// Stub for kernel's kdebug! macro - no-op in tests
#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {{}};
}

// ===========================================================================
// Import kernel source files directly using #[path]
// ===========================================================================

// Network modules (pure protocol logic)
#[path = "../../src/net/ipv4.rs"]
pub mod ipv4;

#[path = "../../src/net/ethernet.rs"]
pub mod ethernet;

#[path = "../../src/net/arp.rs"]
pub mod arp;

// POSIX types and errno definitions
#[path = "../../src/posix.rs"]
pub mod posix;

// IPC: Signal handling (pure logic)
#[path = "../../src/ipc/signal.rs"]
pub mod signal;

// ===========================================================================
// Local test implementations (for algorithms/data structures not in kernel)
// ===========================================================================

pub mod algorithms;
pub mod data_structures;
pub mod mock;

// ===========================================================================
// Test modules
// ===========================================================================

#[cfg(test)]
mod tests;

// Re-export net module for convenience
pub mod net;


