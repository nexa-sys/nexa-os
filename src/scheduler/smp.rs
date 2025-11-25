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
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.cpu_affinity = affinity_mask;
                crate::kinfo!("Set CPU affinity for PID {} to {:#x}", pid, affinity_mask);
                return Ok(());
            }
        }
    }

    Err("Process not found")
}

/// Get CPU affinity for a process
pub fn get_cpu_affinity(pid: Pid) -> Option<u32> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some(entry.cpu_affinity);
            }
        }
    }

    None
}

/// Perform load balancing across CPUs (called periodically)
/// This is a simple implementation that can be enhanced later
pub fn balance_load() {
    let cpu_count = crate::smp::cpu_count();
    if cpu_count <= 1 {
        return; // No need to balance on single-CPU systems
    }

    // Simple load balancing: distribute ready processes across CPUs
    let mut table = PROCESS_TABLE.lock();
    let mut stats = SCHED_STATS.lock();

    let mut ready_processes: Vec<Pid> = Vec::new();

    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.state == ProcessState::Ready {
                ready_processes.push(entry.process.pid);
            }
        }
    }

    if ready_processes.is_empty() {
        return;
    }

    // Distribute processes round-robin across CPUs
    for (idx, &pid) in ready_processes.iter().enumerate() {
        let target_cpu = (idx % cpu_count) as u8;

        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    // Only migrate if the target CPU is in the affinity mask
                    if (entry.cpu_affinity & (1 << target_cpu)) != 0 {
                        if entry.last_cpu != target_cpu {
                            entry.last_cpu = target_cpu;
                            stats.migration_count += 1;
                        }
                    }
                    break;
                }
            }
        }
    }

    stats.load_balance_count += 1;
}

/// Get the recommended CPU for running a process (based on affinity and load)
pub fn get_preferred_cpu(pid: Pid) -> u8 {
    let table = PROCESS_TABLE.lock();
    let cpu_count = crate::smp::cpu_count();

    if cpu_count <= 1 {
        return 0;
    }

    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                // Check affinity and prefer the last CPU used (cache affinity)
                let last_cpu = entry.last_cpu;
                if (entry.cpu_affinity & (1 << last_cpu)) != 0 {
                    return last_cpu;
                }

                // Find first available CPU in affinity mask
                for cpu in 0..cpu_count.min(32) {
                    if (entry.cpu_affinity & (1 << cpu)) != 0 {
                        return cpu as u8;
                    }
                }

                return 0; // Fallback
            }
        }
    }

    0
}
