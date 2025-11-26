//! SMP (Symmetric Multi-Processing) Subsystem
//!
//! This module provides multi-core CPU support for NexaOS, including:
//! - AP (Application Processor) startup and initialization
//! - IPI (Inter-Processor Interrupt) mechanisms
//! - Per-CPU data structures
//! - CPU identification and management
//!
//! # Module Organization
//!
//! - `types`: Type definitions (CpuData, CpuStatus, CpuInfo, etc.)
//! - `state`: Global atomic state variables
//! - `cpu`: CPU management functions (current_cpu_id, cpu_count, etc.)
//! - `ipi`: IPI vector constants and send functions
//! - `trampoline`: AP trampoline installation and configuration
//! - `ap_startup`: AP core startup logic
//! - `init`: SMP subsystem initialization
//! - `alloc`: Dynamic allocation for per-CPU resources

pub mod alloc;
mod ap_startup;
mod cpu;
mod init;
mod ipi;
mod state;
mod trampoline;
mod types;

// Re-export types
pub use types::{CpuData, CpuStatus, MAX_CPUS};

// Re-export IPI constants
pub use ipi::{IPI_CALL_FUNCTION, IPI_HALT, IPI_RESCHEDULE, IPI_TLB_FLUSH};

// Re-export IPI functions
pub use ipi::{send_ipi_broadcast, send_reschedule_ipi, send_tlb_flush_ipi_all};

// Re-export CPU functions
pub use cpu::{cpu_count, current_cpu_data, current_cpu_id, online_cpus};

// Re-export initialization
pub use init::init;
