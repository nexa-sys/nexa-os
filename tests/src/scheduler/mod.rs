//! Comprehensive Scheduler Test Suite
//!
//! Tests for the EEVDF scheduler and per-CPU scheduling infrastructure.
//! These tests use the hardware emulation layer to test the real kernel code.

mod basic;
mod eevdf;
mod eevdf_vruntime;
mod percpu;
mod smp;
mod stress;

pub use basic::*;
pub use eevdf::*;
pub use eevdf_vruntime::*;
pub use percpu::*;
pub use smp::*;
pub use stress::*;
