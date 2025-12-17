//! Comprehensive Scheduler Test Suite
//!
//! Tests for the EEVDF scheduler and per-CPU scheduling infrastructure.
//! This module includes:
//! - Basic scheduler types (CpuMask, SchedPolicy)
//! - EEVDF algorithm and vruntime calculation
//! - EEVDF nice value weights
//! - EEVDF edge cases and potential bugs
//! - Per-CPU queue management
//! - SMP load balancing
//! - Stress tests

mod basic;
mod consistency;
mod eevdf;
mod eevdf_boundary;
mod eevdf_comprehensive;
mod eevdf_edge_cases;
mod eevdf_priority;
mod eevdf_vruntime;
mod eevdf_weights;
mod percpu;
mod priority_tests;
mod smp;
mod smp_comprehensive;
mod stress;
mod types;

pub use basic::*;
pub use eevdf::*;
pub use eevdf_vruntime::*;
pub use eevdf_weights::*;
pub use percpu::*;
pub use smp::*;
pub use smp_comprehensive::*;
pub use stress::*;
pub use types::*;
