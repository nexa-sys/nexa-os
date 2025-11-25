//! Scheduler type definitions
//!
//! This module contains all type definitions used by the scheduler subsystem.

use crate::process::{Process, ProcessState};

/// Number of priority levels in the MLFQ scheduler
pub const NUM_PRIORITY_LEVELS: usize = 8; // 0 = highest, 7 = lowest

/// Base time slice for the highest priority level
pub const BASE_TIME_SLICE_MS: u64 = 5; // Base quantum for highest priority

/// Default time slice in milliseconds
pub const DEFAULT_TIME_SLICE: u64 = 10;

/// Scheduling policy for a process
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal,   // Standard priority-based scheduling
    Realtime, // Real-time priority (higher than normal)
    Batch,    // Background batch processing (lower priority)
    Idle,     // Only runs when nothing else is ready
}

/// Process control block with advanced scheduling info
#[derive(Clone, Copy)]
pub struct ProcessEntry {
    pub process: Process,
    pub priority: u8,            // Current dynamic priority (0 = highest, 255 = lowest)
    pub base_priority: u8,       // Base static priority
    pub time_slice: u64,         // Remaining time slice in ms
    pub total_time: u64,         // Total CPU time used in ms
    pub wait_time: u64,          // Time spent waiting in ready queue
    pub last_scheduled: u64,     // Last time this process was scheduled (in ticks)
    pub cpu_burst_count: u64,    // Number of CPU bursts
    pub avg_cpu_burst: u64,      // Average CPU burst length (for I/O vs CPU bound detection)
    pub policy: SchedPolicy,     // Scheduling policy
    pub nice: i8,                // Nice value (-20 to 19, POSIX compatible)
    pub quantum_level: u8,       // Current priority level in MLFQ (0-7)
    pub preempt_count: u64,      // Number of times preempted
    pub voluntary_switches: u64, // Number of voluntary context switches
    pub cpu_affinity: u32,       // CPU affinity mask (bit per CPU)
    pub last_cpu: u8,            // Last CPU this process ran on
}

impl ProcessEntry {
    /// Create an empty process entry (used for array initialization)
    #[allow(dead_code)]
    pub const fn empty() -> Self {
        Self {
            process: Process {
                pid: 0,
                ppid: 0,
                state: ProcessState::Ready,
                entry_point: 0,
                stack_top: 0,
                heap_start: 0,
                heap_end: 0,
                signal_state: crate::signal::SignalState::new(),
                context: crate::process::Context::zero(),
                has_entered_user: false,
                is_fork_child: false,
                cr3: 0,
                tty: 0,
                memory_base: 0,
                memory_size: 0,
                user_rip: 0,
                kernel_stack: 0,
                user_rsp: 0,
                user_rflags: 0,
                exit_code: 0,
            },
            priority: 128,
            base_priority: 128,
            time_slice: 0,
            total_time: 0,
            wait_time: 0,
            last_scheduled: 0,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            policy: SchedPolicy::Normal,
            nice: 0,
            quantum_level: 4, // Start at middle level
            preempt_count: 0,
            voluntary_switches: 0,
            cpu_affinity: 0xFFFFFFFF, // All CPUs by default
            last_cpu: 0,
        }
    }
}

/// Scheduler statistics structure
#[derive(Clone, Copy)]
pub struct SchedulerStats {
    pub total_context_switches: u64,
    pub total_preemptions: u64,
    pub total_voluntary_switches: u64,
    pub idle_time: u64,
    pub last_idle_start: u64,
    pub load_balance_count: u64, // Number of load balancing operations
    pub migration_count: u64,    // Number of process migrations
}

impl SchedulerStats {
    pub const fn new() -> Self {
        Self {
            total_context_switches: 0,
            total_preemptions: 0,
            total_voluntary_switches: 0,
            idle_time: 0,
            last_idle_start: 0,
            load_balance_count: 0,
            migration_count: 0,
        }
    }
}
