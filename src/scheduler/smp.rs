//! SMP (Symmetric Multi-Processing) and CPU affinity functions
//!
//! This module contains functions for managing CPU affinity and load balancing
//! across multiple CPUs.

extern crate alloc;

use alloc::vec::Vec;

use crate::process::{Pid, ProcessState};

use super::table::{PROCESS_TABLE, SCHED_STATS};

/// Set CPU affinity for a process
pub fn set_cpu_affinity(pid: Pid, affinity_mask: u32) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        entry.cpu_affinity = affinity_mask;
        crate::kinfo!("Set CPU affinity for PID {} to {:#x}", pid, affinity_mask);
        return Ok(());
    }

    Err("Process not found")
}

/// Get CPU affinity for a process
pub fn get_cpu_affinity(pid: Pid) -> Option<u32> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == pid {
            return Some(entry.cpu_affinity);
        }
    }

    None
}

/// Collect PIDs of all ready processes
fn collect_ready_pids(table: &[Option<super::types::ProcessEntry>; crate::process::MAX_PROCESSES]) -> Vec<Pid> {
    table.iter()
        .filter_map(|slot| slot.as_ref())
        .filter(|entry| entry.process.state == ProcessState::Ready)
        .map(|entry| entry.process.pid)
        .collect()
}

/// Migrate a process to a target CPU if allowed by affinity
fn try_migrate_process(
    table: &mut [Option<super::types::ProcessEntry>; crate::process::MAX_PROCESSES],
    pid: Pid,
    target_cpu: u8,
    stats: &mut super::types::SchedulerStats,
) {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        // Only migrate if the target CPU is in the affinity mask
        if (entry.cpu_affinity & (1 << target_cpu)) == 0 {
            break;
        }

        if entry.last_cpu != target_cpu {
            entry.last_cpu = target_cpu;
            stats.migration_count += 1;
        }
        break;
    }
}

/// Perform load balancing across CPUs (called periodically)
pub fn balance_load() {
    let cpu_count = crate::smp::cpu_count();
    if cpu_count <= 1 {
        return;
    }

    let mut table = PROCESS_TABLE.lock();
    let mut stats = SCHED_STATS.lock();

    let ready_processes = collect_ready_pids(&table);
    if ready_processes.is_empty() {
        return;
    }

    // Distribute processes round-robin across CPUs
    for (idx, &pid) in ready_processes.iter().enumerate() {
        let target_cpu = (idx % cpu_count) as u8;
        try_migrate_process(&mut table, pid, target_cpu, &mut stats);
    }

    stats.load_balance_count += 1;
}

/// Get the recommended CPU for running a process (based on affinity and load)
pub fn get_preferred_cpu(pid: Pid) -> u8 {
    let cpu_count = crate::smp::cpu_count();
    if cpu_count <= 1 {
        return 0;
    }

    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        // Prefer the last CPU used (cache affinity)
        if (entry.cpu_affinity & (1 << entry.last_cpu)) != 0 {
            return entry.last_cpu;
        }

        // Find first available CPU in affinity mask
        for cpu in 0..cpu_count.min(32) {
            if (entry.cpu_affinity & (1 << cpu)) != 0 {
                return cpu as u8;
            }
        }

        return 0;
    }

    0
}
