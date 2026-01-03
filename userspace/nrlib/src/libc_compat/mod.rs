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
//! - `epoll` - Epoll and eventfd for async I/O
//! - `sched` - Scheduler functions (sched_getaffinity, sysconf)
//! - `user` - User database and hostname (getpwuid_r, gethostname)
//!
//! Note: Basic functions (read, write, open, close, exit, getpid, memcpy, etc.)
//! are already defined in lib.rs. This module only adds additional functions
//! needed by std that are not in lib.rs.

pub mod clone;
pub mod env;
pub mod epoll;
pub mod fs;
pub mod io;
pub mod math;
pub mod memory;
pub mod network;
pub mod process;
pub mod pthread;
pub mod sched;
pub mod signal;
pub mod string;
pub mod syscall_wrapper;
pub mod time_compat;
pub mod types;
pub mod unwind;
pub mod user;

// Dynamic linking support modules
pub mod dl;
pub mod elf;
pub mod loader;
pub mod reloc;
pub mod rtld;
pub mod symbol;

// Directory operations
pub mod dirent;

// Re-export all public items from submodules
pub use clone::*;
pub use dirent::*;
pub use dl::*;
pub use env::*;
pub use epoll::*;
pub use fs::*;
pub use io::*;
pub use math::*;
pub use memory::*;
pub use network::*;
pub use process::*;
pub use pthread::*;
pub use sched::*;
pub use signal::*;
pub use string::*;
pub use syscall_wrapper::*;
pub use time_compat::*;
pub use types::*;
pub use unwind::*;
pub use user::*;
