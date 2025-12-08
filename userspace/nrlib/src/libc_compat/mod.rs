//! libc compatibility layer for std support
//!
//! This module provides necessary C ABI functions that std expects from libc.
//! It is split into submodules for better organization:
//!
//! - `types` - Common type definitions
//! - `pthread` - pthread mutex, attributes, and thread management
//! - `memory` - Memory allocation and mapping (mmap, brk, etc.)
//! - `io` - I/O operations (stat, readv, fcntl, etc.)
//! - `time_compat` - Time functions (clock_gettime, nanosleep, etc.)
//! - `env` - Environment functions (getenv, getcwd, etc.)
//! - `unwind` - Stack unwinding stubs for panic handling
//! - `signal` - Signal handling stubs
//! - `dl` - Dynamic linker API (dlopen, dlsym, dlclose, etc.)
//! - `clone` - Clone, futex, and thread ID functions
//! - `network` - Network functions (inet_*, byte order conversion)
//! - `process` - Process control (posix_spawn, wait, exec, etc.)
//! - `syscall_wrapper` - Variadic syscall function
//! - `elf` - ELF format definitions and parsing
//! - `rtld` - Runtime dynamic linker (library manager)
//! - `symbol` - Symbol lookup and resolution
//! - `reloc` - Relocation processing
//! - `fs` - Filesystem operations (mkdir, rmdir, getcwd, chdir, etc.)
//! - `string` - String functions and error handling (strerror, strcpy, etc.)
//!
//! Note: Basic functions (read, write, open, close, exit, getpid, memcpy, etc.)
//! are already defined in lib.rs. This module only adds additional functions
//! needed by std that are not in lib.rs.

pub mod types;
pub mod pthread;
pub mod memory;
pub mod io;
pub mod time_compat;
pub mod env;
pub mod unwind;
pub mod signal;
pub mod clone;
pub mod network;
pub mod process;
pub mod syscall_wrapper;
pub mod fs;
pub mod string;
pub mod math;

// Dynamic linking support modules
pub mod elf;
pub mod rtld;
pub mod symbol;
pub mod reloc;
pub mod loader;
pub mod dl;

// Directory operations
pub mod dirent;

// Re-export all public items from submodules
pub use types::*;
pub use pthread::*;
pub use memory::*;
pub use io::*;
pub use time_compat::*;
pub use env::*;
pub use unwind::*;
pub use signal::*;
pub use dl::*;
pub use clone::*;
pub use network::*;
pub use process::*;
pub use syscall_wrapper::*;
pub use fs::*;
pub use string::*;
pub use dirent::*;
pub use math::*;
