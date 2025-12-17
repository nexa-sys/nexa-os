//! Comprehensive Scheduler Test Suite
//!
//! Tests for the EEVDF scheduler and per-CPU scheduling infrastructure.
//! This module includes:
//! - Basic scheduler types (CpuMask, SchedPolicy)
//! - EEVDF algorithm and vruntime calculation
//! - Per-CPU queue management
//! - SMP load balancing
//! - Stress tests

mod basic;
mod eevdf;
mod eevdf_vruntime;
mod percpu;
mod smp;
mod smp_comprehensive;
mod stress;
mod types;

pub use basic::*;
pub use eevdf::*;
pub use eevdf_vruntime::*;
pub use percpu::*;
pub use smp::*;
pub use smp_comprehensive::*;
pub use stress::*;
pub use types::*;
