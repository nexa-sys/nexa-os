//! NexaOS Full Kernel Test Suite
//!
//! Tests the complete kernel with hardware mocks.
//! Build.rs preprocesses kernel source to remove #[global_allocator] and
//! #[alloc_error_handler] which conflict with std.
//!
//! # Module Organization
//!
//! Tests are organized by kernel subsystem:
//! - `fs/` - Filesystem tests (fstab, inodes, file descriptors)
//! - `mm/` - Memory management (allocator, paging, virtual memory)
//! - `net/` - Network stack (Ethernet, IPv4, UDP, ARP)
//! - `ipc/` - Inter-process communication (signals, pipes)
//! - `process/` - Process management (context, state, threads)
//! - `scheduler/` - Scheduler (EEVDF, per-CPU, SMP)
//! - `kmod/` - Kernel modules (crypto, PKCS#7, NKM format)
//! - `integration/` - Multi-subsystem integration tests
//! - `mock/` - Hardware emulation layer
//!
//! # How it works
//! 1. build.rs copies kernel source to build/kernel_src/, removing conflicting attributes
//! 2. We include the preprocessed kernel source via #[path]
//! 3. Hardware operations are mocked in the mock module
//! 4. The kernel's allocator logic runs but uses std's allocator underneath

#![feature(abi_x86_interrupt)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

// ===========================================================================
// Kernel logging macros - output to stderr for test visibility
// ===========================================================================

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => { eprintln!("[INFO] {}", format_args!($($arg)*)) }
}

#[macro_export]
macro_rules! ktrace {
    ($($arg:tt)*) => { () }
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => { eprintln!("[WARN] {}", format_args!($($arg)*)) }
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => { eprintln!("[ERROR] {}", format_args!($($arg)*)) }
}

#[macro_export]
macro_rules! kfatal {
    ($($arg:tt)*) => { eprintln!("[FATAL] {}", format_args!($($arg)*)) }
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => { () }
}

#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => { eprintln!("{}", format_args!($($arg)*)) }
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => { eprint!("{}", format_args!($($arg)*)) }
}

#[macro_export]
macro_rules! kpanic {
    ($($arg:tt)*) => { panic!("{}", format_args!($($arg)*)) }
}

// ===========================================================================
// Hardware mocks - NVM (NexaOS Virtual Machine) platform
// ===========================================================================

// Use NVM as the mock module for backward compatibility
pub use nvm as mock;

// ===========================================================================
// Import FULL kernel source (preprocessed by build.rs)
// ===========================================================================

// Architecture support
#[path = "../build/kernel_src/arch/mod.rs"]
pub mod arch;

// Safety utilities  
#[path = "../build/kernel_src/safety/mod.rs"]
pub mod safety;

// Memory management (allocator has #[global_allocator] removed)
#[path = "../build/kernel_src/mm/mod.rs"]
pub mod mm;

// Boot support
#[path = "../build/kernel_src/boot/mod.rs"]
pub mod boot;

// Scheduler
#[path = "../build/kernel_src/scheduler/mod.rs"]
pub mod scheduler;

// SMP support
#[path = "../build/kernel_src/smp/mod.rs"]
pub mod smp;

// IPC
#[path = "../build/kernel_src/ipc/mod.rs"]
pub mod ipc;

// Filesystem
#[path = "../build/kernel_src/fs/mod.rs"]
pub mod fs;

// Drivers
#[path = "../build/kernel_src/drivers/mod.rs"]
pub mod drivers;

// Networking
#[path = "../build/kernel_src/net/mod.rs"]
pub mod net;

// Kernel modules (kmod)
#[path = "../build/kernel_src/kmod/mod.rs"]
pub mod kmod;

// Process management
#[path = "../build/kernel_src/process/mod.rs"]
pub mod process;

// Interrupts
#[path = "../build/kernel_src/interrupts/mod.rs"]
pub mod interrupts;

// TTY
#[path = "../build/kernel_src/tty/mod.rs"]
pub mod tty;

// User-space driver framework
#[path = "../build/kernel_src/udrv/mod.rs"]
pub mod udrv;

// Security
#[path = "../build/kernel_src/security/mod.rs"]
pub mod security;

// System calls
#[path = "../build/kernel_src/syscalls/mod.rs"]
pub mod syscalls;

// Logger
#[path = "../build/kernel_src/logger.rs"]
pub mod logger;

// POSIX types
#[path = "../build/kernel_src/posix.rs"]
pub mod posix;

// ===========================================================================
// Module aliases (matching kernel's lib.rs re-exports)
// ===========================================================================

pub use arch::gdt;
pub use arch::lapic;
pub use boot::info as bootinfo;
pub use boot::init;
pub use boot::stages as boot_stages;
pub use boot::uefi as uefi_compat;
pub use drivers::acpi;
pub use drivers::framebuffer;
pub use drivers::keyboard;
pub use drivers::serial;
pub use drivers::vga as vga_buffer;
pub use fs::initramfs;
pub use ipc::pipe;
pub use ipc::signal;
pub use mm::allocator;
pub use mm::memory;
pub use mm::numa;
pub use mm::paging;
pub use mm::vmalloc;
pub use security::auth;
pub use security::elf;
pub use tty::vt;

// ===========================================================================
// Test Modules
// ===========================================================================
// Organized by kernel subsystem

/// Filesystem tests (fstab, inodes, file descriptors)
#[path = "fs/mod.rs"]
mod tests_fs;

/// Memory management tests (allocator, paging, virtual memory)
#[path = "mm/mod.rs"]
mod tests_mm;

/// Network protocol stack tests (Ethernet, IPv4, UDP, ARP)
#[path = "net/mod.rs"]
mod tests_net;

/// IPC and signal handling tests
#[path = "ipc/mod.rs"]
mod tests_ipc;

/// Process management tests (context, state, threads, PID)
#[path = "process/mod.rs"]
mod tests_process;

/// Scheduler tests (EEVDF, per-CPU, SMP)
#[path = "scheduler/mod.rs"]
mod tests_scheduler;

/// Kernel module tests (crypto, signing, NKM format)
#[path = "kmod/mod.rs"]
mod tests_kmod;

/// Multi-subsystem integration tests
#[path = "integration/mod.rs"]
mod tests_integration;

/// Interrupt handling tests
#[path = "interrupts.rs"]
mod tests_interrupts;

/// System call interface tests (legacy)
#[path = "syscalls.rs"]
mod tests_syscalls;

/// System call tests (organized by feature)
#[path = "syscalls/mod.rs"]
mod tests_syscalls_ext;

/// User-space driver framework tests
#[path = "udrv.rs"]
mod tests_udrv;

/// Security and ELF validation tests
#[path = "security/mod.rs"]
mod tests_security;
