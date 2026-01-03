//! SMP (Symmetric Multi-Processing) Subsystem
//!
//! This module provides multi-core CPU support for NexaOS, including:
//! - AP (Application Processor) startup and initialization
//! - IPI (Inter-Processor Interrupt) mechanisms
//! - Per-CPU data structures and isolation
//! - CPU identification and management
//! - Preemption control and interrupt context tracking
//!
//! # Per-CPU Isolation
//!
//! Each CPU has dedicated resources to minimize lock contention:
//! - Per-CPU GDT/TSS for independent segment handling
//! - Per-CPU IDT for interrupt isolation
//! - Per-CPU run queues for scheduler scalability
//! - Per-CPU statistics (context switches, interrupts, etc.)
//! - Per-CPU preemption and interrupt state
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
pub mod types;

// Re-export types
pub use types::{
    CpuData, CpuStatus, CpuInfo, ApBootArgs, PerCpuTrampolineData, PerCpuGsData,
    MAX_CPUS, TRAMPOLINE_BASE, TRAMPOLINE_MAX_SIZE, TRAMPOLINE_VECTOR,
    PER_CPU_DATA_SIZE, AP_STACK_SIZE, STARTUP_WAIT_LOOPS, STARTUP_RETRY_MAX,
    STATIC_CPU_COUNT,
};

// Re-export IPI constants
pub use ipi::{IPI_CALL_FUNCTION, IPI_HALT, IPI_RESCHEDULE, IPI_TLB_FLUSH};

// Re-export IPI functions
pub use ipi::{send_ipi_broadcast, send_reschedule_ipi, send_tlb_flush_ipi_all};

// Re-export CPU functions
pub use cpu::{
    cpu_count, current_cpu_data, current_cpu_id, current_gs_data_ptr, get_cpu_data,
    gs_data_ptr_for_cpu, online_cpus,
};

// Re-export per-CPU preemption and interrupt state management
pub use cpu::{
    can_preempt, clear_need_resched, current_numa_node, ensure_kernel_gs_base, enter_interrupt,
    in_interrupt, leave_interrupt, need_resched, preempt_disable, preempt_disabled, preempt_enable,
    record_interrupt, record_syscall, set_need_resched,
};

// Re-export initialization
pub use init::init;
