//! Scheduler subsystem
//!
//! This module provides the process scheduler for NexaOS.
//! It implements a Multi-Level Feedback Queue (MLFQ) scheduler with SMP support.
//!
//! ## Module Organization
//!
//! - `types`: Type definitions (SchedPolicy, ProcessEntry, SchedulerStats)
//! - `table`: Process table and global state management
//! - `process`: Process management functions (add, remove, state changes)
//! - `priority`: Priority calculation and management
//! - `core`: Core scheduling algorithms (schedule, tick, do_schedule)
//! - `context`: Low-level context switch implementation
//! - `smp`: SMP and CPU affinity functions
//! - `stats`: Statistics and debugging functions

extern crate alloc;

mod context;
mod core;
mod priority;
mod process;
mod smp;
mod stats;
mod table;
mod types;

// Re-export types for external use
pub use types::{ProcessEntry, SchedPolicy, SchedulerStats};
pub use types::{BASE_TIME_SLICE_MS, DEFAULT_TIME_SLICE, NUM_PRIORITY_LEVELS};

// Re-export table functions
pub use table::{
    current_cr3, current_pid, get_current_pid, get_tick, process_table_lock, set_current_pid,
    update_current_user_context,
};

// Re-export process management functions
pub use process::{
    add_process, add_process_with_policy, find_child_with_state, get_child_state, get_process,
    mark_process_as_forked_child, remove_process, set_current_process_state, set_process_exit_code,
    set_process_state, update_process_cr3, wake_process,
};

// Re-export priority functions
pub use priority::{
    adjust_process_priority, age_process_priorities, boost_all_priorities, force_reschedule,
    get_process_sched_info, set_process_policy,
};

// Re-export core scheduling functions
pub use core::{do_schedule, do_schedule_from_interrupt, init, schedule, tick};

// Re-export SMP functions
pub use smp::{balance_load, get_cpu_affinity, get_preferred_cpu, set_cpu_affinity};

// Re-export statistics functions
pub use stats::{
    detect_potential_deadlocks, get_load_average, get_process_counts, get_stats, list_processes,
};
