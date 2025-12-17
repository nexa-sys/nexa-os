//! NexaOS Full Kernel Test Suite
//!
//! Tests the complete kernel with hardware mocks.
//! Build.rs preprocesses kernel source to remove #[global_allocator] and
//! #[alloc_error_handler] which conflict with std.
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

// Note: We use std's alloc, so no extern crate alloc needed

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
// Hardware mocks - simulates CPU/hardware for testing
// ===========================================================================

pub mod mock;

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
// Test modules - organized by subsystem
// ===========================================================================

#[path = "net/mod.rs"]
mod tests_net;

#[path = "kmod/mod.rs"]
mod tests_kmod;

#[path = "fs/mod.rs"]
mod tests_fs;

#[path = "ipc/mod.rs"]
mod tests_ipc;

#[path = "scheduler.rs"]
mod tests_scheduler_basic;

#[path = "scheduler/mod.rs"]
mod tests_scheduler;

#[path = "process.rs"]
mod tests_process;

#[path = "process/mod.rs"]
mod tests_process_detailed;

#[path = "safety.rs"]
mod tests_safety;

#[path = "mm.rs"]
mod tests_mm;

#[path = "integration/mod.rs"]
mod tests_integration;

#[path = "syscalls.rs"]
mod tests_syscalls;

#[path = "interrupts.rs"]
mod tests_interrupts;

#[path = "udrv.rs"]
mod tests_udrv;



