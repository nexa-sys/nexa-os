//! Enhanced stdio support for NexaOS userspace (no_std)
//!
//! This module provides a buffered libc-compatible stdio layer for NexaOS
//! userspace programs. The implementation keeps the following guarantees:
//! - Standard `FILE` abstraction with per-stream buffering, error and EOF state
//! - Configurable buffering modes (unbuffered, line buffered, fully buffered)
//! - Correct `fflush` semantics and line-buffer flushing on newline writes
//! - A printf-style formatter with width/precision/length modifiers, pointer
//!   formatting and basic floating-point output
//! - Minimal spinlock-based synchronisation for use in multi-threaded programs
//!
//! # Module Structure
//!
//! - `constants` - System call numbers, file descriptors, buffer sizes, format flags
//! - `helpers` - Math utilities, number formatting, low-level syscall wrappers
//! - `buffer` - Buffer types and modes for FILE streams
//! - `file` - FILE structure and internal file operations
//! - `stream` - High-level stream operations (read/write/flush)
//! - `format` - Printf format parsing and output
//! - `api` - Public C API functions (printf, fwrite, etc.)

mod api;
mod buffer;
mod constants;
mod file;
mod format;
mod helpers;
mod stream;

// Re-export FILE type for external use
pub use file::FILE;

// Re-export global stdio streams
pub use file::{stderr, stdin, stdout};

// Re-export C API functions
pub use api::{
    fflush, fgets, fileno, fprintf, fputc, fputs, fread, fwrite, getchar, gets, printf, putchar,
    puts,
};

// Re-export Rust-friendly stream helpers
pub use stream::{
    stderr_write_all, stderr_write_i32, stderr_write_isize, stderr_write_str, stderr_write_usize,
    stdin_read_line, stdin_read_line_masked, stdin_read_line_noecho, stdout_flush,
    stdout_write_all, stdout_write_fmt, stdout_write_str,
};
