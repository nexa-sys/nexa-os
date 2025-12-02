//! Scheduler statistics and debugging functions
//!
//! This module contains functions for gathering statistics, debugging,
//! and monitoring the EEVDF scheduler.

use core::sync::atomic::Ordering;

use crate::process::{Pid, ProcessState, MAX_PROCESSES};

use super::table::{GLOBAL_TICK, PROCESS_TABLE, SCHED_STATS};
use super::types::{SchedPolicy, SchedulerStats};

/// Get scheduler statistics
pub fn get_stats() -> SchedulerStats {
    *SCHED_STATS.lock()
}

/// Convert SchedPolicy to display string
fn policy_str(policy: SchedPolicy) -> &'static str {
    match policy {
        SchedPolicy::Realtime => "RT",
        SchedPolicy::Normal => "Normal",
        SchedPolicy::Batch => "Batch",
        SchedPolicy::Idle => "Idle",
    }
}

/// Convert ProcessState to display string
fn state_str(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Ready => "Ready",
        ProcessState::Running => "Running",
        ProcessState::Sleeping => "Sleeping",
        ProcessState::Zombie => "Zombie",
    }
}

/// List all processes for debugging with EEVDF information
pub fn list_processes() {
    let table = PROCESS_TABLE.lock();
    crate::kinfo!("=== EEVDF Process List ===");
    crate::kinfo!(
        "{:<5} {:<5} {:<10} {:<7} {:<5} {:<8} {:<12} {:<12} {:<8} {:<10}",
        "PID", "PPID", "State", "Policy", "Nice", "Weight", "VRuntime", "VDeadline", "Lag", "CR3"
    );

    for slot in table.iter() {
        let Some(entry) = slot else { continue };

        crate::kinfo!(
            "{:<5} {:<5} {:<10} {:<7} {:<5} {:<8} {:<12} {:<12} {:<8} {:#010x}",
            entry.process.pid,
            entry.process.ppid,
            state_str(entry.process.state),
            policy_str(entry.policy),
            entry.nice,
            entry.weight,
            entry.vruntime / 1_000_000,  // Convert to ms for readability
            entry.vdeadline / 1_000_000,
            entry.lag / 1_000_000,
            entry.process.cr3
        );
    }

    let stats = SCHED_STATS.lock();
    crate::kinfo!("=== Scheduler Statistics ===");
    crate::kinfo!("Total context switches: {}", stats.total_context_switches);
    crate::kinfo!("Total preemptions: {}", stats.total_preemptions);
    crate::kinfo!("Total voluntary switches: {}", stats.total_voluntary_switches);
    crate::kinfo!("Idle time: {}ms", stats.idle_time);
}

/// Check if a process is potentially deadlocked (stuck in Sleeping)
fn check_sleeping_deadlock(entry: &super::types::ProcessEntry, current_tick: u64, threshold: u64) -> Option<Pid> {
    if entry.process.state != ProcessState::Sleeping {
        return None;
    }

    let wait_ticks = current_tick.saturating_sub(entry.last_scheduled);
    if wait_ticks <= threshold {
        return None;
    }

    crate::kwarn!(
        "Potential deadlock: PID {} sleeping for {} ticks (>{})",
        entry.process.pid, wait_ticks, threshold
    );
    Some(entry.process.pid)
}

/// Check if a process is potentially starving (stuck in Ready)
fn check_ready_starvation(entry: &super::types::ProcessEntry, threshold: u64) -> Option<Pid> {
    if entry.process.state != ProcessState::Ready || entry.wait_time <= threshold {
        return None;
    }

    crate::kwarn!(
        "Potential starvation: PID {} waiting in Ready state for {} ms",
        entry.process.pid, entry.wait_time
    );
    Some(entry.process.pid)
}

/// Detect potential deadlocks by analyzing process wait states
/// Returns list of PIDs that might be in a deadlock
pub fn detect_potential_deadlocks() -> [Option<Pid>; MAX_PROCESSES] {
    let table = PROCESS_TABLE.lock();
    let mut potential_deadlocks = [None; MAX_PROCESSES];
    let mut count = 0;
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    const DEADLOCK_THRESHOLD_TICKS: u64 = 10000;

    for slot in table.iter() {
        let Some(entry) = slot else { continue };

        let pid = check_sleeping_deadlock(entry, current_tick, DEADLOCK_THRESHOLD_TICKS)
            .or_else(|| check_ready_starvation(entry, DEADLOCK_THRESHOLD_TICKS));

        if let Some(pid) = pid {
            if count < MAX_PROCESSES {
                potential_deadlocks[count] = Some(pid);
                count += 1;
            }
        }
    }

    potential_deadlocks
}

/// Get total number of processes in each state
pub fn get_process_counts() -> (usize, usize, usize, usize) {
    let table = PROCESS_TABLE.lock();
    let (mut ready, mut running, mut sleeping, mut zombie) = (0, 0, 0, 0);

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        match entry.process.state {
            ProcessState::Ready => ready += 1,
            ProcessState::Running => running += 1,
            ProcessState::Sleeping => sleeping += 1,
            ProcessState::Zombie => zombie += 1,
        }
    }

    (ready, running, sleeping, zombie)
}

/// Calculate system load average (simplified)
/// Returns (1-min load, 5-min load, 15-min load) - currently just returns ready+running count
pub fn get_load_average() -> (f32, f32, f32) {
    let (ready, running, _, _) = get_process_counts();
    let load = (ready + running) as f32;
    // In a real implementation, these would be exponentially-weighted moving averages
    (load, load, load)
}

/// Per-CPU statistics for monitoring
#[derive(Clone, Copy, Debug)]
pub struct PerCpuStats {
    pub cpu_id: u16,
    pub numa_node: u32,
    pub local_ticks: u64,
    pub context_switches: u64,
    pub voluntary_switches: u64,
    pub preemptions: u64,
    pub interrupts_handled: u64,
    pub syscalls_handled: u64,
    pub ipi_received: u64,
    pub ipi_sent: u64,
    pub idle_ns: u64,
    pub busy_ns: u64,
    pub is_idle: bool,
    pub in_interrupt: bool,
    pub preempt_disabled: bool,
    pub current_pid: u32,
    pub runqueue_len: usize,
}

impl PerCpuStats {
    pub const fn empty() -> Self {
        Self {
            cpu_id: 0,
            numa_node: 0,
            local_ticks: 0,
            context_switches: 0,
            voluntary_switches: 0,
            preemptions: 0,
            interrupts_handled: 0,
            syscalls_handled: 0,
            ipi_received: 0,
            ipi_sent: 0,
            idle_ns: 0,
            busy_ns: 0,
            is_idle: true,
            in_interrupt: false,
            preempt_disabled: false,
            current_pid: 0,
            runqueue_len: 0,
        }
    }
}

/// Get per-CPU statistics for a specific CPU
pub fn get_percpu_stats(cpu_id: usize) -> Option<PerCpuStats> {
    // Use the safe get_cpu_data function from smp module
    let cpu_data = crate::smp::get_cpu_data(cpu_id)?;
    
    // Get scheduler data if available
    let rq_len = crate::scheduler::get_percpu_sched(cpu_id)
        .map(|s| s.run_queue.lock().len())
        .unwrap_or(0);
    
    Some(PerCpuStats {
        cpu_id: cpu_data.cpu_id,
        numa_node: cpu_data.numa_node,
        local_ticks: cpu_data.local_tick.load(Ordering::Relaxed),
        context_switches: cpu_data.context_switches.load(Ordering::Relaxed),
        voluntary_switches: cpu_data.voluntary_switches.load(Ordering::Relaxed),
        preemptions: cpu_data.preemptions.load(Ordering::Relaxed),
        interrupts_handled: cpu_data.interrupts_handled.load(Ordering::Relaxed),
        syscalls_handled: cpu_data.syscalls_handled.load(Ordering::Relaxed),
        ipi_received: cpu_data.ipi_received.load(Ordering::Relaxed),
        ipi_sent: cpu_data.ipi_sent.load(Ordering::Relaxed),
        idle_ns: cpu_data.idle_time.load(Ordering::Relaxed),
        busy_ns: cpu_data.busy_time.load(Ordering::Relaxed),
        is_idle: false, // Would need is_idle from PerCpuSchedData
        in_interrupt: cpu_data.in_interrupt.load(Ordering::Relaxed),
        preempt_disabled: cpu_data.preempt_count.load(Ordering::Relaxed) > 0,
        current_pid: cpu_data.current_pid.load(Ordering::Relaxed),
        runqueue_len: rq_len,
    })
}

/// Print per-CPU statistics for all CPUs
pub fn list_percpu_stats() {
    let cpu_count = crate::smp::cpu_count();
    let online = crate::smp::online_cpus();
    
    crate::kinfo!("=== Per-CPU Statistics ({}/{} CPUs online) ===", online, cpu_count);
    crate::kinfo!(
        "{:<4} {:<4} {:<8} {:<10} {:<8} {:<8} {:<8} {:<8} {:<6} {:<6} Flags",
        "CPU", "NUMA", "Ticks", "CtxSwitch", "Preempt", "Intrs", "Syscalls", "IPIs", "RQ", "PID"
    );
    
    for cpu in 0..cpu_count {
        if let Some(stats) = get_percpu_stats(cpu) {
            // Build flags without format! macro
            let int_flag: &str = if stats.in_interrupt { "I" } else { "-" };
            let pre_flag: &str = if stats.preempt_disabled { "P" } else { "-" };
            
            crate::kinfo!(
                "{:<4} {:<4} {:<8} {:<10} {:<8} {:<8} {:<8} {:<8} {:<6} {:<6} {}{}",
                stats.cpu_id,
                stats.numa_node,
                stats.local_ticks,
                stats.context_switches,
                stats.preemptions,
                stats.interrupts_handled,
                stats.syscalls_handled,
                stats.ipi_received,
                stats.runqueue_len,
                stats.current_pid,
                int_flag,
                pre_flag
            );
        } else {
            crate::kinfo!("{:<4} (offline)", cpu);
        }
    }
    
    // Print global scheduler stats as well
    let stats = SCHED_STATS.lock();
    crate::kinfo!("=== Global Scheduler Statistics ===");
    crate::kinfo!("Total context switches: {}", stats.total_context_switches);
    crate::kinfo!("Total preemptions: {}", stats.total_preemptions);
    crate::kinfo!("Total voluntary switches: {}", stats.total_voluntary_switches);
    crate::kinfo!("Load balance operations: {}", stats.load_balance_count);
    crate::kinfo!("Process migrations: {}", stats.migration_count);
}

/// EEVDF-specific statistics for debugging and tuning
#[derive(Clone, Copy, Debug, Default)]
pub struct EevdfStats {
    /// Minimum vruntime across all processes
    pub min_vruntime_ns: u64,
    /// Maximum vruntime across all processes
    pub max_vruntime_ns: u64,
    /// Average vruntime
    pub avg_vruntime_ns: u64,
    /// Number of eligible processes
    pub eligible_count: usize,
    /// Number of non-eligible processes
    pub non_eligible_count: usize,
    /// Total lag across all processes (signed)
    pub total_lag_ns: i64,
    /// Maximum positive lag (most deserving process)
    pub max_positive_lag_ns: i64,
    /// Maximum negative lag (most over-scheduled process)
    pub max_negative_lag_ns: i64,
    /// Total weight of runnable processes
    pub total_weight: u64,
    /// Average slice remaining
    pub avg_slice_remaining_ns: u64,
}

/// Gather EEVDF-specific statistics for analysis
pub fn get_eevdf_stats() -> EevdfStats {
    let table = PROCESS_TABLE.lock();
    
    let mut stats = EevdfStats::default();
    stats.min_vruntime_ns = u64::MAX;
    stats.max_negative_lag_ns = i64::MAX;
    
    let mut vruntime_sum: u128 = 0;
    let mut slice_sum: u64 = 0;
    let mut count = 0usize;
    
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        
        // Only count runnable processes
        if entry.process.state != ProcessState::Ready && 
           entry.process.state != ProcessState::Running {
            continue;
        }
        
        count += 1;
        
        // Track vruntime range
        if entry.vruntime < stats.min_vruntime_ns {
            stats.min_vruntime_ns = entry.vruntime;
        }
        if entry.vruntime > stats.max_vruntime_ns {
            stats.max_vruntime_ns = entry.vruntime;
        }
        vruntime_sum += entry.vruntime as u128;
        
        // Track eligibility
        if entry.lag >= 0 {
            stats.eligible_count += 1;
        } else {
            stats.non_eligible_count += 1;
        }
        
        // Track lag
        stats.total_lag_ns = stats.total_lag_ns.saturating_add(entry.lag);
        if entry.lag > stats.max_positive_lag_ns {
            stats.max_positive_lag_ns = entry.lag;
        }
        if entry.lag < stats.max_negative_lag_ns {
            stats.max_negative_lag_ns = entry.lag;
        }
        
        // Track weight and slice
        stats.total_weight += entry.weight;
        slice_sum += entry.slice_remaining_ns;
    }
    
    if count > 0 {
        stats.avg_vruntime_ns = (vruntime_sum / count as u128) as u64;
        stats.avg_slice_remaining_ns = slice_sum / count as u64;
    }
    
    if stats.min_vruntime_ns == u64::MAX {
        stats.min_vruntime_ns = 0;
    }
    if stats.max_negative_lag_ns == i64::MAX {
        stats.max_negative_lag_ns = 0;
    }
    
    stats
}

/// Print EEVDF statistics for debugging
pub fn print_eevdf_stats() {
    let stats = get_eevdf_stats();
    
    crate::kinfo!("=== EEVDF Scheduler Statistics ===");
    crate::kinfo!(
        "Vruntime: min={}ms, max={}ms, avg={}ms (spread: {}ms)",
        stats.min_vruntime_ns / 1_000_000,
        stats.max_vruntime_ns / 1_000_000,
        stats.avg_vruntime_ns / 1_000_000,
        stats.max_vruntime_ns.saturating_sub(stats.min_vruntime_ns) / 1_000_000
    );
    crate::kinfo!(
        "Eligibility: {} eligible, {} non-eligible (total lag: {}ms)",
        stats.eligible_count,
        stats.non_eligible_count,
        stats.total_lag_ns / 1_000_000
    );
    crate::kinfo!(
        "Lag range: max_positive={}ms, max_negative={}ms",
        stats.max_positive_lag_ns / 1_000_000,
        stats.max_negative_lag_ns / 1_000_000
    );
    crate::kinfo!(
        "Total runnable weight: {}, avg slice remaining: {}ms",
        stats.total_weight,
        stats.avg_slice_remaining_ns / 1_000_000
    );
}

/// Print detailed EEVDF info for a specific process
pub fn print_process_eevdf_info(pid: Pid) {
    let table = PROCESS_TABLE.lock();
    
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }
        
        crate::kinfo!("=== EEVDF Info for PID {} ===", pid);
        crate::kinfo!("Policy: {:?}, Nice: {}, Weight: {}", entry.policy, entry.nice, entry.weight);
        crate::kinfo!(
            "Vruntime: {}ms, Vdeadline: {}ms",
            entry.vruntime / 1_000_000,
            entry.vdeadline / 1_000_000
        );
        crate::kinfo!(
            "Lag: {}ms ({})",
            entry.lag / 1_000_000,
            if entry.lag >= 0 { "eligible" } else { "non-eligible" }
        );
        crate::kinfo!(
            "Slice: {}ms total, {}ms remaining",
            entry.slice_ns / 1_000_000,
            entry.slice_remaining_ns / 1_000_000
        );
        crate::kinfo!(
            "CPU usage: total_time={}ms, wait_time={}ms, preempts={}",
            entry.total_time,
            entry.wait_time,
            entry.preempt_count
        );
        crate::kinfo!(
            "Bursts: count={}, avg={}ms",
            entry.cpu_burst_count,
            entry.avg_cpu_burst
        );
        crate::kinfo!(
            "Affinity: {:?}, last_cpu={}",
            entry.cpu_affinity,
            entry.last_cpu
        );
        return;
    }
    
    crate::kwarn!("Process {} not found", pid);
}
