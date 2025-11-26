//! SMP Global State
//!
//! This module contains global atomic state variables for SMP management,
//! including CPU counts and readiness flags.

use core::sync::atomic::{AtomicBool, AtomicUsize};

/// SMP subsystem ready flag
pub static SMP_READY: AtomicBool = AtomicBool::new(false);

/// Trampoline installed flag
pub static TRAMPOLINE_READY: AtomicBool = AtomicBool::new(false);

/// Total number of CPUs detected
pub static CPU_TOTAL: AtomicUsize = AtomicUsize::new(1);

/// Number of CPUs currently online
pub static ONLINE_CPUS: AtomicUsize = AtomicUsize::new(1);

/// Configuration: Enable AP startup (set to false to test BSP-only mode)
/// FIXED: IPI vector mismatch - was using 0xF9 (no handler) instead of 0xF0 (reschedule handler)
/// The crash was caused by sending IPI with unregistered vector, causing GP fault!
/// Now using correct IPI_RESCHEDULE (0xF0) which has handler in interrupts.rs
pub const ENABLE_AP_STARTUP: bool = true; // Re-enabled after disabling ALGN check

/// Configuration: Use parallel AP startup mode
/// When true, all APs are started simultaneously using per-CPU data regions
/// When false, APs are started sequentially (original behavior)
pub const PARALLEL_AP_STARTUP: bool = true;
