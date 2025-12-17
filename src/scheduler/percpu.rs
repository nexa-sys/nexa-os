//! Per-CPU Scheduler Data and Run Queues
//!
//! This module implements per-CPU scheduling data structures to improve
//! multiprocessor scalability by reducing global lock contention.
//!
//! ## Architecture
//!
//! Each CPU maintains:
//! - Local run queue for processes with affinity to this CPU
//! - Per-CPU statistics (context switches, idle time, etc.)
//! - Local EEVDF state (min_vruntime, etc.)
//! - Timer state (tick counter, next deadline)
//!
//! ## Lock Hierarchy
//!
//! To avoid deadlocks, locks must be acquired in this order:
//! 1. GLOBAL_PROCESS_TABLE (when needed for process lookup)
//! 2. Per-CPU run queue (for scheduling decisions)
//! 3. CpuData atomics (for statistics)

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::acpi::MAX_CPUS;
use crate::process::{Pid, ProcessState, MAX_PROCESSES};

use super::types::{CpuMask, ProcessEntry, SchedPolicy, SchedulerStats};

// ============================================================================
// Per-CPU Run Queue
// ============================================================================

/// Maximum processes per CPU run queue
/// This is typically smaller than global MAX_PROCESSES since processes
/// are distributed across CPUs
pub const PERCPU_RQ_SIZE: usize = 128;

/// Per-CPU run queue entry (lightweight reference to global process table)
#[derive(Clone, Copy)]
pub struct RunQueueEntry {
    /// Process ID
    pub pid: Pid,
    /// Index in global process table (for O(1) lookup)
    pub table_index: u16,
    /// EEVDF virtual deadline (cached for quick comparison)
    pub vdeadline: u64,
    /// EEVDF virtual runtime (cached)
    pub vruntime: u64,
    /// Scheduling policy
    pub policy: SchedPolicy,
    /// Priority (for realtime processes)
    pub priority: u8,
    /// Whether this entry is eligible (lag >= 0)
    pub eligible: bool,
}

impl RunQueueEntry {
    pub const fn empty() -> Self {
        Self {
            pid: 0,
            table_index: 0,
            vdeadline: 0,
            vruntime: 0,
            policy: SchedPolicy::Normal,
            priority: 128,
            eligible: true,
        }
    }
}

/// Per-CPU run queue
///
/// Maintains a sorted list of runnable processes for this CPU.
/// The queue is kept sorted by virtual deadline for EEVDF scheduling.
pub struct PerCpuRunQueue {
    /// Run queue entries (sorted by vdeadline for EEVDF)
    entries: [Option<RunQueueEntry>; PERCPU_RQ_SIZE],
    /// Number of entries in the queue
    count: usize,
    /// CPU ID this run queue belongs to
    cpu_id: u16,
    /// Local minimum vruntime (for new process placement)
    min_vruntime: u64,
    /// Currently running process (if any)
    current: Option<Pid>,
    /// Whether this CPU needs rescheduling
    need_resched: AtomicBool,
    /// NUMA node this CPU belongs to (cached)
    numa_node: u32,
}

impl PerCpuRunQueue {
    pub const fn new(cpu_id: u16) -> Self {
        Self {
            entries: [None; PERCPU_RQ_SIZE],
            count: 0,
            cpu_id,
            min_vruntime: 0,
            current: None,
            need_resched: AtomicBool::new(false),
            numa_node: 0,
        }
    }

    /// Initialize the run queue for a CPU
    pub fn init(&mut self, cpu_id: u16, numa_node: u32) {
        self.cpu_id = cpu_id;
        self.numa_node = numa_node;
        self.count = 0;
        self.current = None;
        self.min_vruntime = 0;
        self.need_resched.store(false, Ordering::Relaxed);
        for entry in self.entries.iter_mut() {
            *entry = None;
        }
    }

    /// Add a process to this CPU's run queue
    pub fn enqueue(&mut self, entry: RunQueueEntry) -> Result<(), &'static str> {
        if self.count >= PERCPU_RQ_SIZE {
            return Err("Per-CPU run queue full");
        }

        // Find insertion point (maintain sorted order by vdeadline)
        let mut insert_idx = self.count;
        for i in 0..self.count {
            if let Some(existing) = &self.entries[i] {
                if entry.vdeadline < existing.vdeadline {
                    insert_idx = i;
                    break;
                }
            }
        }

        // Shift entries to make room
        if insert_idx < self.count {
            for i in (insert_idx..self.count).rev() {
                self.entries[i + 1] = self.entries[i];
            }
        }

        self.entries[insert_idx] = Some(entry);
        self.count += 1;

        // Update min_vruntime if needed
        if entry.vruntime < self.min_vruntime || self.count == 1 {
            self.min_vruntime = entry.vruntime;
        }

        Ok(())
    }

    /// Remove a process from this CPU's run queue
    pub fn dequeue(&mut self, pid: Pid) -> Option<RunQueueEntry> {
        let mut found_idx = None;
        for i in 0..self.count {
            if let Some(entry) = &self.entries[i] {
                if entry.pid == pid {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        let idx = found_idx?;
        let entry = self.entries[idx].take();

        // Shift entries down
        for i in idx..(self.count - 1) {
            self.entries[i] = self.entries[i + 1];
        }
        self.entries[self.count - 1] = None;
        self.count = self.count.saturating_sub(1);

        // Recalculate min_vruntime if needed
        self.update_min_vruntime();

        entry
    }

    /// Pick the next process to run using EEVDF algorithm
    /// Returns the entry and its index in the queue
    pub fn pick_next(&mut self) -> Option<RunQueueEntry> {
        if self.count == 0 {
            return None;
        }

        let mut best_idx: Option<usize> = None;
        let mut best_entry: Option<&RunQueueEntry> = None;

        for i in 0..self.count {
            let Some(entry) = &self.entries[i] else {
                continue;
            };

            // Realtime processes always come first
            if entry.policy == SchedPolicy::Realtime {
                if let Some(best) = best_entry {
                    if best.policy != SchedPolicy::Realtime || entry.priority < best.priority {
                        best_idx = Some(i);
                        best_entry = Some(entry);
                    }
                } else {
                    best_idx = Some(i);
                    best_entry = Some(entry);
                }
                continue;
            }

            // Skip idle processes if we have normal ones
            if entry.policy == SchedPolicy::Idle {
                if best_entry.is_none() {
                    best_idx = Some(i);
                    best_entry = Some(entry);
                }
                continue;
            }

            // EEVDF: eligible process with earliest deadline wins
            let dominated = match best_entry {
                None => true,
                Some(best) => {
                    if best.policy == SchedPolicy::Realtime {
                        false // Never dominate RT
                    } else if best.policy == SchedPolicy::Idle {
                        true // Always dominate Idle
                    } else if entry.eligible && !best.eligible {
                        true
                    } else if !entry.eligible && best.eligible {
                        false
                    } else {
                        entry.vdeadline < best.vdeadline
                    }
                }
            };

            if dominated {
                best_idx = Some(i);
                best_entry = Some(entry);
            }
        }

        // Remove and return the selected entry
        if let Some(idx) = best_idx {
            let entry = self.entries[idx].take();

            // Shift entries down
            for i in idx..(self.count - 1) {
                self.entries[i] = self.entries[i + 1];
            }
            self.entries[self.count - 1] = None;
            self.count = self.count.saturating_sub(1);

            entry
        } else {
            None
        }
    }

    /// Update cached EEVDF state for an entry
    pub fn update_entry(&mut self, pid: Pid, vruntime: u64, vdeadline: u64, eligible: bool) {
        for entry in self.entries.iter_mut().take(self.count) {
            if let Some(e) = entry {
                if e.pid == pid {
                    e.vruntime = vruntime;
                    e.vdeadline = vdeadline;
                    e.eligible = eligible;
                    break;
                }
            }
        }
        self.update_min_vruntime();
    }

    /// Check if a process is in this run queue
    pub fn contains(&self, pid: Pid) -> bool {
        for i in 0..self.count {
            if let Some(entry) = &self.entries[i] {
                if entry.pid == pid {
                    return true;
                }
            }
        }
        false
    }

    /// Get the number of processes in the queue
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if the queue is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get the local minimum vruntime
    #[inline]
    pub fn min_vruntime(&self) -> u64 {
        self.min_vruntime
    }

    /// Set the need_resched flag
    pub fn set_need_resched(&self, value: bool) {
        self.need_resched.store(value, Ordering::Release);
    }

    /// Check and clear the need_resched flag
    pub fn check_need_resched(&self) -> bool {
        self.need_resched.swap(false, Ordering::AcqRel)
    }

    /// Get the currently running process
    #[inline]
    pub fn current(&self) -> Option<Pid> {
        self.current
    }

    /// Set the currently running process
    #[inline]
    pub fn set_current(&mut self, pid: Option<Pid>) {
        self.current = pid;
    }

    /// Update minimum vruntime from queue entries
    fn update_min_vruntime(&mut self) {
        let mut min = u64::MAX;
        for i in 0..self.count {
            if let Some(entry) = &self.entries[i] {
                if entry.vruntime < min {
                    min = entry.vruntime;
                }
            }
        }
        if min != u64::MAX {
            // Only allow min_vruntime to increase (prevents starvation)
            if min > self.min_vruntime {
                self.min_vruntime = min;
            }
        }
    }

    /// Get NUMA node for this CPU
    #[inline]
    pub fn numa_node(&self) -> u32 {
        self.numa_node
    }
}

// ============================================================================
// Per-CPU Scheduler State
// ============================================================================

/// Extended per-CPU scheduler data
///
/// This extends the basic CpuData with scheduler-specific state.
/// Cache-line aligned to prevent false sharing between CPUs.
#[repr(C, align(64))]
pub struct PerCpuSchedData {
    /// Run queue for this CPU (protected by local lock)
    pub run_queue: Mutex<PerCpuRunQueue>,

    /// Local tick counter (independent of global tick)
    pub local_tick: AtomicU64,

    /// Time of last scheduler tick (in ns, from TSC or other source)
    pub last_tick_ns: AtomicU64,

    /// Accumulated idle time (in nanoseconds)
    pub idle_ns: AtomicU64,

    /// Context switches on this CPU
    pub context_switches: AtomicU64,

    /// Voluntary context switches on this CPU
    pub voluntary_switches: AtomicU64,

    /// Preemptions on this CPU
    pub preemptions: AtomicU64,

    /// Number of processes migrated away from this CPU
    pub migrations_out: AtomicU64,

    /// Number of processes migrated to this CPU
    pub migrations_in: AtomicU64,

    /// Load average (fixed-point, scaled by 1024)
    pub load_avg: AtomicU64,

    /// Whether this CPU is idle
    pub is_idle: AtomicBool,

    /// Idle timestamp (when CPU became idle, for statistics)
    pub idle_start: AtomicU64,

    /// CPU index
    pub cpu_id: u16,

    /// NUMA node this CPU belongs to
    pub numa_node: u32,

    /// Padding to fill cache line
    _pad: [u8; 8],
}

impl PerCpuSchedData {
    pub const fn new(cpu_id: u16) -> Self {
        Self {
            run_queue: Mutex::new(PerCpuRunQueue::new(cpu_id)),
            local_tick: AtomicU64::new(0),
            last_tick_ns: AtomicU64::new(0),
            idle_ns: AtomicU64::new(0),
            context_switches: AtomicU64::new(0),
            voluntary_switches: AtomicU64::new(0),
            preemptions: AtomicU64::new(0),
            migrations_out: AtomicU64::new(0),
            migrations_in: AtomicU64::new(0),
            load_avg: AtomicU64::new(0),
            is_idle: AtomicBool::new(true),
            idle_start: AtomicU64::new(0),
            cpu_id,
            numa_node: 0,
            _pad: [0; 8],
        }
    }

    /// Initialize per-CPU scheduler data
    pub fn init(&mut self, cpu_id: u16, numa_node: u32) {
        self.cpu_id = cpu_id;
        self.numa_node = numa_node;
        self.local_tick.store(0, Ordering::Relaxed);
        self.last_tick_ns.store(0, Ordering::Relaxed);
        self.idle_ns.store(0, Ordering::Relaxed);
        self.context_switches.store(0, Ordering::Relaxed);
        self.voluntary_switches.store(0, Ordering::Relaxed);
        self.preemptions.store(0, Ordering::Relaxed);
        self.migrations_out.store(0, Ordering::Relaxed);
        self.migrations_in.store(0, Ordering::Relaxed);
        self.load_avg.store(0, Ordering::Relaxed);
        self.is_idle.store(true, Ordering::Relaxed);
        self.idle_start.store(0, Ordering::Relaxed);

        self.run_queue.lock().init(cpu_id, numa_node);
    }

    /// Record a context switch
    pub fn record_context_switch(&self, voluntary: bool) {
        self.context_switches.fetch_add(1, Ordering::Relaxed);
        if voluntary {
            self.voluntary_switches.fetch_add(1, Ordering::Relaxed);
        } else {
            self.preemptions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record entering idle state
    pub fn enter_idle(&self, current_ns: u64) {
        self.is_idle.store(true, Ordering::Release);
        self.idle_start.store(current_ns, Ordering::Relaxed);
    }

    /// Record leaving idle state
    pub fn exit_idle(&self, current_ns: u64) {
        let was_idle = self.is_idle.swap(false, Ordering::AcqRel);
        if was_idle {
            let start = self.idle_start.load(Ordering::Relaxed);
            if current_ns > start {
                self.idle_ns
                    .fetch_add(current_ns - start, Ordering::Relaxed);
            }
        }
    }

    /// Get load as percentage (0-100)
    pub fn load_percent(&self) -> u8 {
        // Load average is scaled by 1024
        let load = self.load_avg.load(Ordering::Relaxed);
        ((load * 100) / 1024).min(100) as u8
    }

    /// Update load average (exponential moving average)
    /// Called periodically (e.g., every 100ms)
    pub fn update_load_average(&self) {
        let rq_len = self.run_queue.lock().len() as u64;
        let is_running = !self.is_idle.load(Ordering::Relaxed);

        // Current load = number of runnable + currently running
        let current_load = rq_len + if is_running { 1 } else { 0 };

        // Scale by 1024 for fixed-point arithmetic
        let current_scaled = current_load * 1024;

        // Exponential moving average: new = old * 0.875 + current * 0.125
        // Using integer arithmetic: new = (old * 7 + current) / 8
        let old = self.load_avg.load(Ordering::Relaxed);
        let new = (old * 7 + current_scaled) / 8;
        self.load_avg.store(new, Ordering::Relaxed);
    }
}

// ============================================================================
// Global Per-CPU Data Array
// ============================================================================

/// Static per-CPU scheduler data for first few CPUs (BSP + some APs)
/// Additional CPUs use dynamic allocation through smp::alloc
const STATIC_PERCPU_SCHED_COUNT: usize = 8;

static mut PERCPU_SCHED_DATA: [PerCpuSchedData; STATIC_PERCPU_SCHED_COUNT] = {
    const INIT: PerCpuSchedData = PerCpuSchedData::new(0);
    [INIT; STATIC_PERCPU_SCHED_COUNT]
};

/// Track which per-CPU sched data entries are initialized
static PERCPU_SCHED_READY: [AtomicBool; MAX_CPUS] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; MAX_CPUS]
};

/// Initialize per-CPU scheduler data for a CPU
pub fn init_percpu_sched(cpu_id: usize) {
    if cpu_id >= MAX_CPUS {
        return;
    }

    // Determine NUMA node for this CPU
    let numa_node = if crate::numa::is_initialized() {
        let cpus = crate::acpi::cpus();
        if cpu_id < cpus.len() {
            crate::numa::cpu_to_node(cpus[cpu_id].apic_id as u32)
        } else {
            0
        }
    } else {
        0
    };

    unsafe {
        if cpu_id < STATIC_PERCPU_SCHED_COUNT {
            PERCPU_SCHED_DATA[cpu_id].init(cpu_id as u16, numa_node);
        }
        // For CPUs beyond static count, they would use dynamic allocation
        // which would be initialized through smp::alloc system
    }

    PERCPU_SCHED_READY[cpu_id].store(true, Ordering::Release);
    crate::kinfo!(
        "Per-CPU scheduler initialized for CPU {} (NUMA node {})",
        cpu_id,
        numa_node
    );
}

/// Get per-CPU scheduler data for a CPU
pub fn get_percpu_sched(cpu_id: usize) -> Option<&'static PerCpuSchedData> {
    if cpu_id >= MAX_CPUS || !PERCPU_SCHED_READY[cpu_id].load(Ordering::Acquire) {
        return None;
    }

    unsafe {
        if cpu_id < STATIC_PERCPU_SCHED_COUNT {
            Some(&PERCPU_SCHED_DATA[cpu_id])
        } else {
            // Would return from dynamic allocation
            None
        }
    }
}

/// Get per-CPU scheduler data for current CPU
pub fn current_percpu_sched() -> Option<&'static PerCpuSchedData> {
    let cpu_id = crate::smp::current_cpu_id() as usize;
    get_percpu_sched(cpu_id)
}

// ============================================================================
// Per-CPU Scheduling Operations
// ============================================================================

/// Enqueue a process on its preferred CPU's run queue
pub fn percpu_enqueue(entry: &ProcessEntry) -> Result<u16, &'static str> {
    let cpu_count = crate::smp::cpu_count();

    // Determine target CPU
    let target_cpu = if entry.cpu_affinity.is_empty() {
        0
    } else {
        // Use last_cpu if it's in affinity mask, otherwise find first available
        if entry.cpu_affinity.is_set(entry.last_cpu as usize) {
            entry.last_cpu
        } else {
            entry.cpu_affinity.first_set().unwrap_or(0) as u16
        }
    };

    let target_cpu = (target_cpu as usize).min(cpu_count.saturating_sub(1)) as u16;

    let sched = get_percpu_sched(target_cpu as usize).ok_or("Per-CPU scheduler not initialized")?;

    let rq_entry = RunQueueEntry {
        pid: entry.process.pid,
        table_index: 0, // Would be set by caller with actual index
        vdeadline: entry.vdeadline,
        vruntime: entry.vruntime,
        policy: entry.policy,
        priority: entry.priority,
        eligible: entry.lag >= 0,
    };

    sched.run_queue.lock().enqueue(rq_entry)?;
    Ok(target_cpu)
}

/// Dequeue a process from its current CPU's run queue
pub fn percpu_dequeue(pid: Pid, cpu_id: u16) -> Option<RunQueueEntry> {
    let sched = get_percpu_sched(cpu_id as usize)?;
    sched.run_queue.lock().dequeue(pid)
}

/// Pick next process to run on current CPU
pub fn percpu_pick_next() -> Option<RunQueueEntry> {
    let sched = current_percpu_sched()?;
    sched.run_queue.lock().pick_next()
}

/// Set need_resched flag on a specific CPU
pub fn set_need_resched(cpu_id: u16) {
    if let Some(sched) = get_percpu_sched(cpu_id as usize) {
        sched.run_queue.lock().set_need_resched(true);
    }
}

/// Check and clear need_resched on current CPU
pub fn check_need_resched() -> bool {
    current_percpu_sched()
        .map(|s| s.run_queue.lock().check_need_resched())
        .unwrap_or(false)
}

/// Get load for a specific CPU
pub fn get_cpu_load(cpu_id: u16) -> u8 {
    get_percpu_sched(cpu_id as usize)
        .map(|s| s.load_percent())
        .unwrap_or(0)
}

/// Get queue length for a specific CPU (more accurate than load percent)
pub fn get_cpu_queue_len(cpu_id: u16) -> usize {
    get_percpu_sched(cpu_id as usize)
        .map(|s| s.run_queue.lock().len())
        .unwrap_or(0)
}

/// Find least loaded CPU that the process can run on
/// Considers NUMA topology for better cache locality
pub fn find_least_loaded_cpu(affinity: &CpuMask) -> u16 {
    let cpu_count = crate::smp::cpu_count();
    let mut best_cpu = 0u16;
    let mut min_load = u8::MAX;

    for cpu in affinity.iter_set() {
        if cpu >= cpu_count {
            break;
        }
        let load = get_cpu_load(cpu as u16);
        if load < min_load {
            min_load = load;
            best_cpu = cpu as u16;
        }
    }

    best_cpu
}

/// Find best CPU considering NUMA topology and load
/// Prefers CPUs on the same NUMA node as the process's preferred node
pub fn find_best_cpu_numa(affinity: &CpuMask, preferred_node: u32) -> u16 {
    let cpu_count = crate::smp::cpu_count();
    let mut best_cpu = 0u16;
    let mut best_score = u64::MAX;

    for cpu in affinity.iter_set() {
        if cpu >= cpu_count {
            break;
        }

        // Get CPU's NUMA node
        let cpu_node = if let Some(sched) = get_percpu_sched(cpu) {
            sched.numa_node
        } else {
            continue;
        };

        // Calculate score: lower is better
        // NUMA locality bonus: prefer CPUs on same node
        let numa_penalty = if cpu_node == preferred_node {
            0u64
        } else {
            100
        };

        // Load factor
        let load = get_cpu_load(cpu as u16) as u64;

        let score = numa_penalty + load;

        if score < best_score {
            best_score = score;
            best_cpu = cpu as u16;
        }
    }

    best_cpu
}

/// Load balance threshold: imbalance ratio to trigger migration
#[allow(dead_code)]
const LOAD_BALANCE_THRESHOLD: u64 = 2;

/// Minimum queue length difference to justify migration
const MIN_IMBALANCE: usize = 2;

/// Trigger load balance check with work-stealing
/// Returns number of processes migrated
pub fn balance_runqueues() -> usize {
    let cpu_count = crate::smp::cpu_count();
    if cpu_count <= 1 {
        return 0;
    }

    let migrations = 0usize;

    // Collect load information for all CPUs
    let mut loads: [usize; MAX_CPUS] = [0; MAX_CPUS];
    let mut total_load = 0usize;

    for cpu in 0..cpu_count {
        if let Some(sched) = get_percpu_sched(cpu) {
            loads[cpu] = sched.run_queue.lock().len();
            total_load += loads[cpu];
        }
    }

    let avg_load = total_load / cpu_count;

    // Find overloaded (donors) and underloaded (recipients) CPUs
    let mut donors: [Option<usize>; MAX_CPUS] = [None; MAX_CPUS];
    let mut recipients: [Option<usize>; MAX_CPUS] = [None; MAX_CPUS];
    let mut donor_count = 0usize;
    let mut recipient_count = 0usize;

    for cpu in 0..cpu_count {
        if loads[cpu] > avg_load + MIN_IMBALANCE {
            donors[donor_count] = Some(cpu);
            donor_count += 1;
        } else if loads[cpu] < avg_load.saturating_sub(1) {
            recipients[recipient_count] = Some(cpu);
            recipient_count += 1;
        }
    }

    // Attempt work stealing: move processes from overloaded to underloaded CPUs
    // In a full implementation, we would:
    // 1. Lock the donor's run queue
    // 2. Find a migratable process (check affinity)
    // 3. Update process's last_cpu and move to recipient's queue
    // 4. Send IPI to recipient if it's idle

    // Log imbalance for debugging
    if donor_count > 0 && recipient_count > 0 {
        crate::kdebug!(
            "Load balance: {} donors, {} recipients, avg_load={}",
            donor_count,
            recipient_count,
            avg_load
        );

        // Update statistics
        if let Some(sched) = current_percpu_sched() {
            // Note: actual migration would increment this
            let _ = sched.load_avg.load(Ordering::Relaxed);
        }
    }

    migrations
}

/// Update load averages for all CPUs (should be called periodically)
pub fn update_all_load_averages() {
    let cpu_count = crate::smp::cpu_count();
    for cpu in 0..cpu_count {
        if let Some(sched) = get_percpu_sched(cpu) {
            sched.update_load_average();
        }
    }
}
