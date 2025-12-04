//! Scheduler type definitions
//!
//! This module contains all type definitions used by the scheduler subsystem.
//! Implements EEVDF (Earliest Eligible Virtual Deadline First) scheduling.

use crate::acpi::MAX_CPUS;
use crate::process::{Process, ProcessState, MAX_CMDLINE_SIZE};

/// Number of u64 words needed to represent MAX_CPUS bits (1024 CPUs = 16 u64s)
const CPU_MASK_WORDS: usize = (MAX_CPUS + 63) / 64;

/// CPU affinity mask supporting up to MAX_CPUS (1024) processors
///
/// This is a bitmap where each bit represents a CPU core.
/// Bit N being set means the process can run on CPU N.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CpuMask {
    bits: [u64; CPU_MASK_WORDS],
}

impl CpuMask {
    /// Create a new empty CPU mask (no CPUs allowed)
    pub const fn empty() -> Self {
        Self {
            bits: [0; CPU_MASK_WORDS],
        }
    }

    /// Create a CPU mask with all CPUs allowed
    pub const fn all() -> Self {
        Self {
            bits: [u64::MAX; CPU_MASK_WORDS],
        }
    }

    /// Create a CPU mask from a u32 (for backward compatibility, supports CPUs 0-31)
    pub const fn from_u32(mask: u32) -> Self {
        let mut bits = [0u64; CPU_MASK_WORDS];
        bits[0] = mask as u64;
        Self { bits }
    }

    /// Set a specific CPU bit
    #[inline]
    pub fn set(&mut self, cpu: usize) {
        if cpu < MAX_CPUS {
            let word = cpu / 64;
            let bit = cpu % 64;
            self.bits[word] |= 1u64 << bit;
        }
    }

    /// Clear a specific CPU bit
    #[inline]
    pub fn clear(&mut self, cpu: usize) {
        if cpu < MAX_CPUS {
            let word = cpu / 64;
            let bit = cpu % 64;
            self.bits[word] &= !(1u64 << bit);
        }
    }

    /// Check if a specific CPU is set
    #[inline]
    pub const fn is_set(&self, cpu: usize) -> bool {
        if cpu >= MAX_CPUS {
            return false;
        }
        let word = cpu / 64;
        let bit = cpu % 64;
        (self.bits[word] & (1u64 << bit)) != 0
    }

    /// Check if the mask is empty (no CPUs allowed)
    pub const fn is_empty(&self) -> bool {
        let mut i = 0;
        while i < CPU_MASK_WORDS {
            if self.bits[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// Count the number of CPUs set in the mask
    pub const fn count(&self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < CPU_MASK_WORDS {
            count += self.bits[i].count_ones() as usize;
            i += 1;
        }
        count
    }

    /// Find the first set CPU (returns None if empty)
    pub const fn first_set(&self) -> Option<usize> {
        let mut i = 0;
        while i < CPU_MASK_WORDS {
            if self.bits[i] != 0 {
                return Some(i * 64 + self.bits[i].trailing_zeros() as usize);
            }
            i += 1;
        }
        None
    }

    /// Get the raw bits for display/debug purposes (first 64 CPUs)
    pub const fn as_u64(&self) -> u64 {
        self.bits[0]
    }

    /// Iterate over all set CPU indices
    pub fn iter_set(&self) -> impl Iterator<Item = usize> + '_ {
        CpuMaskIter {
            mask: self,
            current_word: 0,
            current_bits: self.bits[0],
        }
    }
}

impl Default for CpuMask {
    fn default() -> Self {
        Self::all()
    }
}

impl core::fmt::Debug for CpuMask {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "CpuMask({:#018x}...)", self.bits[0])
    }
}

/// Iterator over set CPU indices in a CpuMask
struct CpuMaskIter<'a> {
    mask: &'a CpuMask,
    current_word: usize,
    current_bits: u64,
}

impl<'a> Iterator for CpuMaskIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.current_bits == 0 {
            self.current_word += 1;
            if self.current_word >= CPU_MASK_WORDS {
                return None;
            }
            self.current_bits = self.mask.bits[self.current_word];
        }

        let bit = self.current_bits.trailing_zeros() as usize;
        self.current_bits &= self.current_bits - 1; // Clear lowest set bit
        Some(self.current_word * 64 + bit)
    }
}

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
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, // -10 to -1
    9548, 7620, 6100, 4904, 3906, 3121, 2501, 1991, 1586, 1277, // 0 to 9
    1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, // 10 to 19
    110, 87, 70, 56, 45, 36, 29, 23, 18, 15,
];

/// Get weight for a nice value (-20 to +19)
#[inline]
pub const fn nice_to_weight(nice: i8) -> u64 {
    let idx = nice as i32 + 20;
    let idx = if idx < 0 {
        0
    } else if idx > 39 {
        39
    } else {
        idx as usize
    };
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
    pub cpu_affinity: CpuMask,   // CPU affinity mask (supports up to 1024 CPUs)
    pub last_cpu: u16,           // Last CPU this process ran on (u16 for 1024 CPUs)

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
                tgid: 0,        // Thread group ID (0 for empty)
                state: ProcessState::Ready,
                entry_point: 0,
                stack_top: 0,
                heap_start: 0,
                heap_end: 0,
                signal_state: crate::signal::SignalState::new(),
                context: crate::process::Context::zero(),
                has_entered_user: false,
                context_valid: false,
                is_fork_child: false,
                is_thread: false,       // Not a thread
                cr3: 0,
                tty: 0,
                memory_base: 0,
                memory_size: 0,
                user_rip: 0,
                kernel_stack: 0,
                user_rsp: 0,
                user_rflags: 0,
                exit_code: 0,
                term_signal: None,
                fs_base: 0,
                clear_child_tid: 0,     // No clear_child_tid
                cmdline: [0u8; MAX_CMDLINE_SIZE],
                cmdline_len: 0,
                open_fds: 0,
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
            cpu_affinity: CpuMask::all(), // All CPUs by default
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
