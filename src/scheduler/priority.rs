//! Priority management functions
//!
//! This module contains functions for calculating and managing process priorities
//! in the MLFQ scheduler.

use core::sync::atomic::Ordering;

use crate::process::{Pid, ProcessState};

use super::table::{GLOBAL_TICK, PROCESS_TABLE};
use super::types::{SchedPolicy, BASE_TIME_SLICE_MS};

/// Calculate time slice based on priority level (MLFQ)
/// Higher levels get longer quanta to reduce context switching overhead
#[inline]
pub fn calculate_time_slice(quantum_level: u8) -> u64 {
    BASE_TIME_SLICE_MS * (1 << quantum_level.min(7))
}

/// Calculate dynamic priority based on wait time and CPU usage
/// Rewards I/O-bound processes and penalizes CPU-bound processes
#[inline]
pub fn calculate_dynamic_priority(base: u8, wait_time: u64, cpu_time: u64, nice: i8) -> u8 {
    let base = base as i32;
    let nice_offset = nice as i32; // -20 to 19

    // Priority boost for waiting (I/O bound processes)
    let wait_boost = (wait_time / 100).min(40) as i32;

    // Priority penalty for CPU usage
    let cpu_penalty = (cpu_time / 1000).min(40) as i32;

    let dynamic = base + nice_offset + cpu_penalty - wait_boost;
    dynamic.clamp(0, 255) as u8
}

/// Boost priority of a process (MLFQ priority boost mechanism)
/// This is called periodically to prevent starvation
pub fn boost_all_priorities() {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.state != ProcessState::Zombie {
                // Reset to highest priority level
                entry.quantum_level = match entry.policy {
                    SchedPolicy::Realtime => 0,
                    SchedPolicy::Normal => 2,
                    SchedPolicy::Batch => 4,
                    SchedPolicy::Idle => 6,
                };

                // Reset priority to base
                entry.priority = entry.base_priority;

                // Reset counters
                entry.preempt_count = 0;

                crate::kdebug!(
                    "Boosted priority for PID {} to level {}",
                    entry.process.pid,
                    entry.quantum_level
                );
            }
        }
    }
}

/// Set the scheduling policy for a process
pub fn set_process_policy(pid: Pid, policy: SchedPolicy, nice: i8) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.policy = policy;
                entry.nice = nice.clamp(-20, 19);

                // Adjust quantum level based on new policy
                entry.quantum_level = match policy {
                    SchedPolicy::Realtime => 0,
                    SchedPolicy::Normal => 4,
                    SchedPolicy::Batch => 6,
                    SchedPolicy::Idle => 7,
                };

                // Recalculate time slice
                entry.time_slice = calculate_time_slice(entry.quantum_level);

                crate::kinfo!(
                    "Process {} policy changed to {:?}, nice={}, quantum_level={}",
                    pid,
                    policy,
                    nice,
                    entry.quantum_level
                );
                return Ok(());
            }
        }
    }

    Err("Process not found")
}

/// Get process scheduling information
pub fn get_process_sched_info(pid: Pid) -> Option<(u8, u8, SchedPolicy, i8, u64, u64)> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some((
                    entry.priority,
                    entry.quantum_level,
                    entry.policy,
                    entry.nice,
                    entry.total_time,
                    entry.wait_time,
                ));
            }
        }
    }

    None
}

/// Adjust process priority dynamically (for syscalls like nice())
pub fn adjust_process_priority(pid: Pid, nice_delta: i8) -> Result<i8, &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                let old_nice = entry.nice;
                entry.nice = (entry.nice + nice_delta).clamp(-20, 19);

                // Recalculate priority
                entry.priority = calculate_dynamic_priority(
                    entry.base_priority,
                    entry.wait_time,
                    entry.total_time,
                    entry.nice,
                );

                crate::kdebug!(
                    "Process {} nice: {} -> {}, priority: {}",
                    pid,
                    old_nice,
                    entry.nice,
                    entry.priority
                );

                return Ok(entry.nice);
            }
        }
    }

    Err("Process not found")
}

/// Age all processes' wait times to prevent starvation
/// Called periodically by the scheduler (e.g., every 100ms)
pub fn age_process_priorities() {
    let mut table = PROCESS_TABLE.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.state == ProcessState::Ready {
                let wait_delta = current_tick.saturating_sub(entry.last_scheduled);

                // Age: reduce priority number (increase priority) for long-waiting processes
                if wait_delta > 100 && entry.priority > 0 {
                    entry.priority = entry.priority.saturating_sub(1);

                    // Also promote to higher quantum level for fairness
                    if entry.quantum_level > 0 && wait_delta > 500 {
                        entry.quantum_level -= 1;
                        crate::kdebug!(
                            "Aged process {}: promoted to quantum level {}",
                            entry.process.pid,
                            entry.quantum_level
                        );
                    }
                }
            }
        }
    }
}

/// Force reschedule by setting current process time slice to 0
/// Used for explicit yield or priority inversion handling
pub fn force_reschedule() {
    let mut table = PROCESS_TABLE.lock();
    let current = *super::table::CURRENT_PID.lock();

    if let Some(curr_pid) = current {
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == curr_pid {
                    entry.time_slice = 0;
                    crate::kdebug!("Force reschedule for PID {}", curr_pid);
                    break;
                }
            }
        }
    }
}
