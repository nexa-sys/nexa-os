//! EEVDF (Earliest Eligible Virtual Deadline First) Scheduler Core
//!
//! This module implements the EEVDF scheduling algorithm, which is the scheduler
//! used in Linux 6.6+. EEVDF improves upon CFS by providing better latency
//! guarantees through virtual deadline-based scheduling.
//!
//! ## Key Concepts:
//! - **vruntime**: Virtual runtime, weighted CPU time consumption
//! - **vdeadline**: Virtual deadline = vruntime + request/weight  
//! - **lag**: Difference between ideal and actual CPU time (eligibility check)
//! - **weight**: Derived from nice value, determines CPU share
//!
//! ## Algorithm:
//! 1. A process is "eligible" if its lag >= 0 (hasn't consumed more than its share)
//! 2. Among eligible processes, pick the one with earliest virtual deadline
//! 3. Update vruntime as process runs: vruntime += delta * NICE_0_WEIGHT / weight

use core::sync::atomic::Ordering;

use crate::process::{Pid, ProcessState, MAX_PROCESSES};

use super::table::{GLOBAL_TICK, PROCESS_TABLE};
use super::types::{nice_to_weight, SchedPolicy, BASE_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS};

/// Minimum vruntime in the system (used to prevent new processes from starving)
static MIN_VRUNTIME: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Convert milliseconds to nanoseconds
#[inline]
pub const fn ms_to_ns(ms: u64) -> u64 {
    ms * 1_000_000
}

/// Convert nanoseconds to milliseconds
#[inline]
pub const fn ns_to_ms(ns: u64) -> u64 {
    ns / 1_000_000
}

/// Calculate time slice based on weight (for backward compatibility)
/// In EEVDF, this returns the default slice converted to ms
#[inline]
pub fn calculate_time_slice(_quantum_level: u8) -> u64 {
    ns_to_ms(BASE_SLICE_NS)
}

/// Calculate the weighted vruntime delta
/// delta_vruntime = delta_exec * NICE_0_WEIGHT / weight
#[inline]
pub fn calc_delta_vruntime(delta_exec_ns: u64, weight: u64) -> u64 {
    if weight == 0 {
        return delta_exec_ns;
    }
    // Use u128 to prevent overflow
    ((delta_exec_ns as u128 * NICE_0_WEIGHT as u128) / weight as u128) as u64
}

/// Calculate virtual deadline for a process
/// vdeadline = vruntime + slice_ns * NICE_0_WEIGHT / weight
#[inline]
pub fn calc_vdeadline(vruntime: u64, slice_ns: u64, weight: u64) -> u64 {
    if weight == 0 {
        return vruntime.saturating_add(slice_ns);
    }
    let delta = ((slice_ns as u128 * NICE_0_WEIGHT as u128) / weight as u128) as u64;
    vruntime.saturating_add(delta)
}

/// Check if a process is eligible to run (EEVDF eligibility)
/// A process is eligible if lag >= 0 (hasn't consumed more than its fair share)
#[inline]
pub fn is_eligible(entry: &super::types::ProcessEntry) -> bool {
    entry.lag >= 0
}

/// Calculate dynamic priority based on nice value (for backward compatibility)
/// In EEVDF, priority is derived from nice value
#[inline]
pub fn calculate_dynamic_priority(_base: u8, _wait_time: u64, _cpu_time: u64, nice: i8) -> u8 {
    // Map nice (-20 to +19) to priority (0-255)
    // nice -20 -> priority 0 (highest)
    // nice +19 -> priority 255 (lowest)
    let priority = ((nice as i32 + 20) * 255 / 39) as u8;
    priority
}

/// Get the minimum vruntime in the system
pub fn get_min_vruntime() -> u64 {
    MIN_VRUNTIME.load(Ordering::Relaxed)
}

/// Update the global minimum vruntime
pub fn update_min_vruntime() {
    let table = PROCESS_TABLE.lock();
    
    let mut min_vrt = u64::MAX;
    
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.state == ProcessState::Zombie {
            continue;
        }
        if entry.vruntime < min_vrt {
            min_vrt = entry.vruntime;
        }
    }
    
    if min_vrt != u64::MAX {
        MIN_VRUNTIME.store(min_vrt, Ordering::Relaxed);
    }
}

/// Place a new/waking process on the runqueue
/// Sets initial vruntime to prevent new processes from starving existing ones
pub fn place_entity(entry: &mut super::types::ProcessEntry, is_new: bool) {
    let min_vrt = get_min_vruntime();
    
    if is_new {
        // New processes start at min_vruntime to get fair share quickly
        // but not zero (which would let them monopolize CPU)
        entry.vruntime = min_vrt;
        entry.lag = 0;
    } else {
        // Waking process: adjust vruntime if it's too far behind
        // This prevents sleeping processes from getting unfair advantage
        if entry.vruntime < min_vrt {
            // Give some credit but not too much
            let credit = BASE_SLICE_NS / 2;
            entry.vruntime = min_vrt.saturating_sub(credit);
        }
    }
    
    // Calculate initial deadline
    entry.vdeadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
}

/// Update process state after running for delta_exec nanoseconds
pub fn update_curr(entry: &mut super::types::ProcessEntry, delta_exec_ns: u64) {
    // Update vruntime
    let delta_vrt = calc_delta_vruntime(delta_exec_ns, entry.weight);
    entry.vruntime = entry.vruntime.saturating_add(delta_vrt);
    
    // Decrease remaining slice
    entry.slice_remaining_ns = entry.slice_remaining_ns.saturating_sub(delta_exec_ns);
    
    // Decrease lag (we consumed CPU time)
    entry.lag = entry.lag.saturating_sub(delta_exec_ns as i64);
    
    // Update legacy fields
    entry.total_time = entry.total_time.saturating_add(ns_to_ms(delta_exec_ns));
    entry.time_slice = ns_to_ms(entry.slice_remaining_ns);
}

/// Check if current process needs to be preempted
/// Returns true if time slice exhausted or better candidate exists
pub fn check_preempt_curr(
    curr_entry: &super::types::ProcessEntry,
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
) -> bool {
    // Always preempt if time slice exhausted
    if curr_entry.slice_remaining_ns == 0 {
        return true;
    }
    
    // Check for realtime processes
    if curr_entry.policy == SchedPolicy::Realtime {
        return false; // Realtime processes are not preempted by non-realtime
    }
    
    // Find if there's an eligible process with earlier deadline
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }
        if entry.process.pid == curr_entry.process.pid {
            continue;
        }
        
        // Realtime processes preempt normal processes
        if entry.policy == SchedPolicy::Realtime && curr_entry.policy != SchedPolicy::Realtime {
            return true;
        }
        
        // Check eligibility and deadline for EEVDF
        if is_eligible(entry) && entry.vdeadline < curr_entry.vdeadline {
            // Only preempt if difference is significant (avoid thrashing)
            let deadline_diff = curr_entry.vdeadline.saturating_sub(entry.vdeadline);
            if deadline_diff > SCHED_GRANULARITY_NS {
                return true;
            }
        }
    }
    
    false
}

/// Replenish time slice when exhausted
pub fn replenish_slice(entry: &mut super::types::ProcessEntry) {
    let slice = match entry.policy {
        SchedPolicy::Realtime => BASE_SLICE_NS * 2,     // Longer slices for realtime
        SchedPolicy::Normal => BASE_SLICE_NS,
        SchedPolicy::Batch => BASE_SLICE_NS * 4,       // Much longer for batch
        SchedPolicy::Idle => BASE_SLICE_NS,
    };
    
    entry.slice_ns = slice;
    entry.slice_remaining_ns = slice;
    entry.time_slice = ns_to_ms(slice);
    
    // Recalculate deadline with new slice
    entry.vdeadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
}

/// Periodic priority boost for all processes (EEVDF version)
/// In EEVDF, we reset lag values periodically to prevent permanent starvation
pub fn boost_all_priorities() {
    let mut table = PROCESS_TABLE.lock();
    
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.state == ProcessState::Zombie {
            continue;
        }
        
        // Reset lag to give everyone a fair chance
        entry.lag = 0;
        
        // Ensure process has a valid time slice
        if entry.slice_remaining_ns == 0 {
            replenish_slice(entry);
        }
        
        crate::kdebug!(
            "EEVDF boost: PID {} vrt={}, vdl={}, lag=0",
            entry.process.pid, entry.vruntime, entry.vdeadline
        );
    }
    
    update_min_vruntime();
}

/// Set the scheduling policy for a process
pub fn set_process_policy(pid: Pid, policy: SchedPolicy, nice: i8) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        let old_weight = entry.weight;
        entry.policy = policy;
        entry.nice = nice.clamp(-20, 19);
        entry.weight = nice_to_weight(entry.nice);
        
        // Recalculate deadline with new weight
        entry.vdeadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
        
        // Update priority for backward compatibility
        entry.priority = calculate_dynamic_priority(entry.base_priority, 0, 0, entry.nice);

        crate::kinfo!(
            "EEVDF: PID {} policy={:?}, nice={}, weight {} -> {}",
            pid, policy, nice, old_weight, entry.weight
        );
        return Ok(());
    }

    Err("Process not found")
}

/// Get process scheduling information
pub fn get_process_sched_info(pid: Pid) -> Option<(u8, u8, SchedPolicy, i8, u64, u64)> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        return Some((
            entry.priority,
            0, // quantum_level not used in EEVDF
            entry.policy,
            entry.nice,
            entry.total_time,
            entry.wait_time,
        ));
    }

    None
}

/// Adjust process priority dynamically (for syscalls like nice())
pub fn adjust_process_priority(pid: Pid, nice_delta: i8) -> Result<i8, &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        let old_nice = entry.nice;
        let old_weight = entry.weight;
        
        entry.nice = (entry.nice + nice_delta).clamp(-20, 19);
        entry.weight = nice_to_weight(entry.nice);
        entry.priority = calculate_dynamic_priority(entry.base_priority, 0, 0, entry.nice);
        
        // Recalculate deadline with new weight
        entry.vdeadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);

        crate::kdebug!(
            "EEVDF nice: PID {} nice {} -> {}, weight {} -> {}",
            pid, old_nice, entry.nice, old_weight, entry.weight
        );

        return Ok(entry.nice);
    }

    Err("Process not found")
}

/// Age process priorities (EEVDF version)
/// Increases lag for waiting processes to ensure fairness
pub fn age_process_priorities() {
    let mut table = PROCESS_TABLE.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    // Calculate total weight of all runnable processes
    let total_weight: u64 = table.iter()
        .filter_map(|slot| slot.as_ref())
        .filter(|e| e.process.state == ProcessState::Ready || e.process.state == ProcessState::Running)
        .map(|e| e.weight)
        .sum();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        let wait_delta = current_tick.saturating_sub(entry.last_scheduled);
        if wait_delta == 0 {
            continue;
        }
        
        // Increase lag for waiting (they're being starved of CPU time)
        // The longer they wait, the more eligible they become
        let lag_credit = ms_to_ns(wait_delta) as i64 * entry.weight as i64 / total_weight.max(1) as i64;
        entry.lag = entry.lag.saturating_add(lag_credit);
        
        // Update wait time for statistics
        entry.wait_time = entry.wait_time.saturating_add(wait_delta);
    }
}

/// Force reschedule by exhausting current process time slice
pub fn force_reschedule() {
    let Some(curr_pid) = *super::table::CURRENT_PID.lock() else {
        return;
    };

    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == curr_pid {
            entry.slice_remaining_ns = 0;
            entry.time_slice = 0;
            crate::kdebug!("EEVDF force reschedule for PID {}", curr_pid);
            break;
        }
    }
}

/// Get EEVDF specific info for a process (for debugging/stats)
pub fn get_eevdf_info(pid: Pid) -> Option<(u64, u64, i64, u64)> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        return Some((
            entry.vruntime,
            entry.vdeadline,
            entry.lag,
            entry.weight,
        ));
    }

    None
}
