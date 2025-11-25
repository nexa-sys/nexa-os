//! Scheduler statistics and debugging functions
//!
//! This module contains functions for gathering statistics, debugging,
//! and monitoring the scheduler.

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

/// List all processes for debugging with extended information
pub fn list_processes() {
    let table = PROCESS_TABLE.lock();
    crate::kinfo!("=== Process List (Extended) ===");
    crate::kinfo!(
        "{:<5} {:<5} {:<12} {:<8} {:<6} {:<5} {:<10} {:<10} {:<8} {:<10}",
        "PID", "PPID", "State", "Policy", "Nice", "QLvl", "CPU(ms)", "Wait(ms)", "Preempt", "CR3"
    );

    for slot in table.iter() {
        let Some(entry) = slot else { continue };

        crate::kinfo!(
            "{:<5} {:<5} {:<12} {:<8} {:<6} {:<5} {:<10} {:<10} {:<8} {:#010x}",
            entry.process.pid,
            entry.process.ppid,
            state_str(entry.process.state),
            policy_str(entry.policy),
            entry.nice,
            entry.quantum_level,
            entry.total_time,
            entry.wait_time,
            entry.preempt_count,
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
