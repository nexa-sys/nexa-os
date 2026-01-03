//! SMP (Symmetric Multi-Processing) Subsystem Tests
//!
//! Tests for SMP types, per-CPU data structures, and CPU management.
//! Uses REAL kernel code - no simulated implementations.
//!
//! ## Coverage Areas
//! - CpuData structure and operations
//! - CpuStatus enumeration
//! - CpuInfo structure
//! - Preemption control
//! - Per-CPU configuration constants

mod types;
mod cpu_data;
mod cpu_status;
mod constants;

pub use types::*;
pub use cpu_data::*;
pub use cpu_status::*;
pub use constants::*;
