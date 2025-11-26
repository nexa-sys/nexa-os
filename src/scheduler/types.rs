//! Scheduler type definitions
//!
//! This module contains all type definitions used by the scheduler subsystem.
//! Implements EEVDF (Earliest Eligible Virtual Deadline First) scheduling.

use crate::process::{Process, ProcessState};

/// Minimum granularity - minimum time slice a process can get (in ns)
pub const SCHED_GRANULARITY_NS: u64 = 1_000_000; // 1ms

/// Default time slice in nanoseconds (for EEVDF request size)
pub const BASE_SLICE_NS: u64 = 4_000_000; // 4ms

/// Default time slice in milliseconds (legacy compatibility)
pub const DEFAULT_TIME_SLICE: u64 = 4;

/// Base time slice for the highest priority level (legacy compatibility)
pub const BASE_TIME_SLICE_MS: u64 = 4;

/// Number of priority levels (legacy compatibility)
pub const NUM_PRIORITY_LEVELS: usize = 8;

/// Weight for nice value 0 (base weight)
pub const NICE_0_WEIGHT: u64 = 1024;

/// Precomputed weights for nice values -20 to +19
/// Formula: weight = 1024 * 1.25^(-nice)
/// Nice -20 has highest weight, Nice +19 has lowest
pub const NICE_TO_WEIGHT: [u64; 40] = [
    // -20 to -11
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916,
    // -10 to -1
    9548, 7620, 6100, 4904, 3906, 3121, 2501, 1991, 1586, 1277,
    // 0 to 9
    1024, 820, 655, 526, 423, 335, 272, 215, 172, 137,
    // 10 to 19
    110, 87, 70, 56, 45, 36, 29, 23, 18, 15,
];

/// Get weight for a nice value (-20 to +19)
#[inline]
pub const fn nice_to_weight(nice: i8) -> u64 {
    let idx = nice as i32 + 20;
    let idx = if idx < 0 { 0 } else if idx > 39 { 39 } else { idx as usize };
    NICE_TO_WEIGHT[idx]
}

/// Scheduling policy for a process
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal,   // SCHED_NORMAL: Standard CFS/EEVDF scheduling
    Realtime, // SCHED_FIFO/RR: Real-time priority (bypasses EEVDF)
    Batch,    // SCHED_BATCH: Background batch processing (longer slices)
    Idle,     // SCHED_IDLE: Only runs when nothing else is ready
}

/// Process control block with EEVDF scheduling info
#[derive(Clone, Copy)]
pub struct ProcessEntry {
    pub process: Process,
    
    // === EEVDF core fields ===
    /// Virtual runtime - accumulated weighted CPU time (in nanoseconds)
    pub vruntime: u64,
    /// Virtual deadline - vruntime + request/weight (in nanoseconds)
    pub vdeadline: u64,
    /// Lag - difference between ideal and actual CPU time (can be negative)
    /// Positive lag means the process deserves more CPU time
    pub lag: i64,
    /// Weight based on nice value (higher weight = more CPU share)
    pub weight: u64,
    /// Current time slice request (in nanoseconds)
    pub slice_ns: u64,
    /// Time slice remaining (in nanoseconds)
    pub slice_remaining_ns: u64,
    
    // === Legacy/compatibility fields ===
    pub priority: u8,            // Mapped from nice for backward compatibility
    pub base_priority: u8,       // Base static priority
    pub time_slice: u64,         // Remaining time slice in ms (for compatibility)
    pub total_time: u64,         // Total CPU time used in ms
    pub wait_time: u64,          // Time spent waiting in ready queue
    pub last_scheduled: u64,     // Last time this process was scheduled (in ticks)
    pub cpu_burst_count: u64,    // Number of CPU bursts
    pub avg_cpu_burst: u64,      // Average CPU burst length
    pub policy: SchedPolicy,     // Scheduling policy
    pub nice: i8,                // Nice value (-20 to 19, POSIX compatible)
    pub quantum_level: u8,       // Kept for compatibility, not used in EEVDF
    pub preempt_count: u64,      // Number of times preempted
    pub voluntary_switches: u64, // Number of voluntary context switches
    pub cpu_affinity: u32,       // CPU affinity mask (bit per CPU)
    pub last_cpu: u8,            // Last CPU this process ran on
    
    // === NUMA fields ===
    /// Preferred NUMA node for this process (NUMA_NO_NODE = no preference)
    pub numa_preferred_node: u32,
    /// NUMA policy for memory allocation
    pub numa_policy: crate::numa::NumaPolicy,
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
            // EEVDF fields
            vruntime: 0,
            vdeadline: 0,
            lag: 0,
            weight: NICE_0_WEIGHT, // Nice 0 weight
            slice_ns: BASE_SLICE_NS,
            slice_remaining_ns: BASE_SLICE_NS,
            // Legacy fields
            priority: 128,
            base_priority: 128,
            time_slice: DEFAULT_TIME_SLICE,
            total_time: 0,
            wait_time: 0,
            last_scheduled: 0,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            policy: SchedPolicy::Normal,
            nice: 0,
            quantum_level: 0,
            preempt_count: 0,
            voluntary_switches: 0,
            cpu_affinity: 0xFFFFFFFF, // All CPUs by default
            last_cpu: 0,
            // NUMA fields
            numa_preferred_node: crate::numa::NUMA_NO_NODE,
            numa_policy: crate::numa::NumaPolicy::Local,
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
