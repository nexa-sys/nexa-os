//! Scheduler subsystem
//!
//! This module provides the process scheduler for NexaOS.
//! It implements EEVDF (Earliest Eligible Virtual Deadline First) scheduling,
//! the same algorithm used in Linux 6.6+.
//!
//! ## EEVDF Key Features:
//! - **Virtual Runtime (vruntime)**: Tracks weighted CPU time consumption
//! - **Virtual Deadline**: Provides latency guarantees (vruntime + slice/weight)
//! - **Lag**: Measures fairness (ideal_time - actual_time)
//! - **Eligibility**: Only processes with lag >= 0 can preempt
//!
//! ## Per-CPU Architecture
//!
//! The scheduler uses per-CPU run queues to minimize lock contention:
//! - Each CPU maintains its own run queue of runnable processes
//! - Processes are assigned to CPUs based on affinity and load balancing
//! - Per-CPU statistics track context switches, idle time, etc.
//! - IPI is used for cross-CPU rescheduling requests
//!
//! ## Module Organization
//!
//! - `types`: Type definitions (SchedPolicy, ProcessEntry, EEVDF constants)
//! - `table`: Process table and global state management
//! - `percpu`: Per-CPU run queues and scheduler state
//! - `process`: Process management functions (add, remove, state changes)
//! - `priority`: EEVDF core algorithms (vruntime, deadline, eligibility)
//! - `core`: Main scheduling loop (schedule, tick, do_schedule)
//! - `context`: Low-level context switch implementation
//! - `smp`: SMP and CPU affinity functions
//! - `stats`: Statistics and debugging functions

extern crate alloc;

mod context;
mod core;
pub mod percpu;
mod priority;
mod process;
mod smp;
mod stats;
mod table;
mod types;

// Re-export types for external use
pub use types::{nice_to_weight, BASE_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS};
pub use types::{CpuMask, ProcessEntry, SchedPolicy, SchedulerStats};
pub use types::{BASE_TIME_SLICE_MS, DEFAULT_TIME_SLICE, NUM_PRIORITY_LEVELS};

// Re-export table functions
pub use table::{
    current_cr3, current_pid, get_current_pid, get_tick, process_table_lock, set_current_pid,
    update_current_user_context,
};

// Re-export process management functions
pub use process::{
    add_process,
    add_process_with_policy,
    find_child_with_state,
    get_child_state,
    get_process,
    // Thread management functions
    get_tgid,
    get_thread_group_members,
    handle_thread_exit,
    is_thread,
    mark_process_as_forked_child,
    remove_process,
    set_current_process_state,
    set_process_exit_code,
    set_process_state,
    set_process_term_signal,
    terminate_thread_group,
    thread_group_count,
    update_process_cr3,
    wake_process,
};

// Re-export priority functions (EEVDF core)
pub use priority::{
    adjust_process_priority,
    age_process_priorities,
    boost_all_priorities,
    calc_vdeadline,
    force_reschedule,
    // EEVDF specific exports
    get_eevdf_info,
    get_min_vruntime,
    get_process_sched_info,
    is_eligible,
    set_process_policy,
};

// Re-export core scheduling functions
pub use core::{do_schedule, do_schedule_from_interrupt, init, schedule, tick};

// Re-export SMP functions
pub use smp::{
    balance_load,
    get_cpu_affinity,
    // NUMA-aware functions
    get_numa_preferred_node,
    get_preferred_cpu,
    set_cpu_affinity,
    set_numa_policy,
    set_numa_preferred_node,
};

// Re-export per-CPU scheduler functions
pub use percpu::{
    balance_runqueues, check_need_resched, current_percpu_sched, find_best_cpu_numa,
    find_least_loaded_cpu, get_cpu_load, get_cpu_queue_len, get_percpu_sched, init_percpu_sched,
    set_need_resched, update_all_load_averages,
};

// Re-export statistics functions
pub use stats::{
    detect_potential_deadlocks,
    get_eevdf_stats,
    get_load_average,
    get_percpu_stats,
    get_process_counts,
    get_stats,
    list_percpu_stats,
    list_processes,
    print_eevdf_stats,
    print_process_eevdf_info,
    // EEVDF-specific stats
    EevdfStats,
    // Per-CPU stats
    PerCpuStats,
};
