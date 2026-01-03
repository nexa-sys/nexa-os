//! Boot Subsystem Tests
//!
//! Tests for the boot stage management and configuration parsing.
//! Uses REAL kernel code - no simulated implementations.
//!
//! ## Coverage Areas
//! - BootStage enumeration and transitions
//! - BootConfig parsing and defaults
//! - Filesystem mount state tracking
//! - Init system runlevels and service management

mod config;
mod init;
mod stages;

pub use config::*;
pub use stages::*;
