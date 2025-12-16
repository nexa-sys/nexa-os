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

// Re-export alloc crate for kernel code that uses alloc::vec, alloc::string, etc.
extern crate alloc;

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
// Kernel environment stubs - provides constants/types that kernel code needs
// These mirror the real kernel definitions for testing purposes
// ===========================================================================

/// ACPI stub - provides MAX_CPUS constant
pub mod acpi {
    /// Maximum number of CPUs supported (same as kernel)
    pub const MAX_CPUS: usize = 1024;
}

/// NUMA stub - provides NumaPolicy and NUMA_NO_NODE
pub mod numa {
    /// NUMA_NO_NODE indicates no preferred NUMA node
    pub const NUMA_NO_NODE: u32 = 0xFFFFFFFF;

    /// NUMA memory allocation policy
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum NumaPolicy {
        /// Allocate from the local node (default)
        Local,
        /// Allocate from the specified node
        Bind(u32),
        /// Interleave allocations across all nodes
        Interleave,
        /// Prefer local node but fall back to others
        Preferred(u32),
    }

    impl Default for NumaPolicy {
        fn default() -> Self {
            NumaPolicy::Local
        }
    }
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

// Process types (Process, ProcessState, Context, etc.)
#[path = "../../src/process/types.rs"]
pub mod process;

// Scheduler types (CpuMask, ProcessEntry, SchedPolicy, etc.)
#[path = "../../src/scheduler/types.rs"]
pub mod scheduler_types;

// IPC: Pipe (uses spin::Mutex - we provide via Cargo.toml dependency)
#[path = "../../src/ipc/pipe.rs"]
pub mod pipe;

// IPC: Core message channels
#[path = "../../src/ipc/core.rs"]
pub mod ipc_core;

// Filesystem traits and types
#[path = "../../src/fs/traits.rs"]
pub mod fs_traits;

// UDRV: Isolation classes (IC0, IC1, IC2)
#[path = "../../src/udrv/isolation.rs"]
pub mod udrv_isolation;

// Security: Authentication system
#[path = "../../src/security/auth.rs"]
pub mod security_auth;

// ===========================================================================
// Hardware-level mocks (simulates underlying hardware, NOT kernel functionality)
// ===========================================================================

pub mod mock;

// ===========================================================================
// Test modules
// ===========================================================================

#[cfg(test)]
mod tests;

// Re-export net module for convenience
pub mod net;


