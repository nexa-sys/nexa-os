//! SMP (Symmetric Multi-Processing) and CPU affinity functions
//!
//! This module contains functions for managing CPU affinity and load balancing
//! across multiple CPUs with NUMA awareness.

extern crate alloc;

use alloc::vec::Vec;

use crate::process::{Pid, ProcessState};

use super::table::{PROCESS_TABLE, SCHED_STATS};
use super::types::CpuMask;

/// Set CPU affinity for a process using radix tree for O(log N) lookup
pub fn set_cpu_affinity(pid: Pid, affinity_mask: CpuMask) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    entry.cpu_affinity = affinity_mask;
                    crate::kinfo!("Set CPU affinity for PID {} to {:?}", pid, affinity_mask);
                    return Ok(());
                }
            }
        }
    }

    // Fallback to linear scan
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        entry.cpu_affinity = affinity_mask;
        crate::kinfo!("Set CPU affinity for PID {} to {:?}", pid, affinity_mask);
        return Ok(());
    }

    Err("Process not found")
}

/// Get CPU affinity for a process using radix tree for O(log N) lookup
pub fn get_cpu_affinity(pid: Pid) -> Option<CpuMask> {
    let table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &table[idx] {
                if entry.process.pid == pid {
                    return Some(entry.cpu_affinity);
                }
            }
        }
    }

    // Fallback to linear scan
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == pid {
            return Some(entry.cpu_affinity);
        }
    }

    None
}

/// Collect PIDs of all ready processes
fn collect_ready_pids(
    table: &[Option<super::types::ProcessEntry>; crate::process::MAX_PROCESSES],
) -> Vec<Pid> {
    table
        .iter()
        .filter_map(|slot| slot.as_ref())
        .filter(|entry| entry.process.state == ProcessState::Ready)
        .map(|entry| entry.process.pid)
        .collect()
}

/// Migrate a process to a target CPU if allowed by affinity
fn try_migrate_process(
    table: &mut [Option<super::types::ProcessEntry>; crate::process::MAX_PROCESSES],
    pid: Pid,
    target_cpu: u16,
    stats: &mut super::types::SchedulerStats,
) {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        // Only migrate if the target CPU is in the affinity mask
        if !entry.cpu_affinity.is_set(target_cpu as usize) {
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
/// NUMA-aware: Prefers migration within the same NUMA node
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

    // NUMA-aware load balancing:
    // 1. First try to distribute within NUMA nodes
    // 2. Then balance across nodes if needed
    let numa_node_count = crate::numa::node_count();

    if numa_node_count <= 1 {
        // Non-NUMA or single node: simple round-robin
        for (idx, &pid) in ready_processes.iter().enumerate() {
            let target_cpu = (idx % cpu_count) as u16;
            try_migrate_process(&mut table, pid, target_cpu, &mut stats);
        }
    } else {
        // NUMA-aware distribution
        for &pid in ready_processes.iter() {
            let preferred_node = get_preferred_numa_node(pid, &table);
            let target_cpu = get_least_loaded_cpu_on_node(preferred_node);
            try_migrate_process(&mut table, pid, target_cpu, &mut stats);
        }
    }

    stats.load_balance_count += 1;
}

/// Get preferred NUMA node for a process based on its memory locality
fn get_preferred_numa_node(
    pid: Pid,
    table: &[Option<super::types::ProcessEntry>; crate::process::MAX_PROCESSES],
) -> u32 {
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        // Check if process has a preferred NUMA node set
        if entry.numa_preferred_node != crate::numa::NUMA_NO_NODE {
            return entry.numa_preferred_node;
        }

        // Otherwise, prefer the NUMA node of the last CPU used
        if crate::numa::is_initialized() {
            let last_cpu_apic = get_cpu_apic_id(entry.last_cpu);
            return crate::numa::cpu_to_node(last_cpu_apic);
        }

        return 0;
    }

    0
}

/// Get the APIC ID for a given CPU index
fn get_cpu_apic_id(cpu_index: u16) -> u32 {
    let cpus = crate::acpi::cpus();
    if (cpu_index as usize) < cpus.len() {
        cpus[cpu_index as usize].apic_id as u32
    } else {
        0
    }
}

/// Get the least loaded CPU on a specific NUMA node
fn get_least_loaded_cpu_on_node(node: u32) -> u16 {
    let cpu_count = crate::smp::cpu_count();
    if cpu_count == 0 {
        return 0;
    }

    // Collect CPUs on this node
    let mut best_cpu = 0u16;
    let mut min_load = u64::MAX;
    let mut found_on_node = false;

    for cpu_idx in 0..cpu_count {
        let apic_id = get_cpu_apic_id(cpu_idx as u16);
        let cpu_node = crate::numa::cpu_to_node(apic_id);

        if cpu_node == node {
            found_on_node = true;
            // Get load for this CPU (simple count of processes assigned)
            let load = count_processes_on_cpu(cpu_idx as u16);
            if load < min_load {
                min_load = load;
                best_cpu = cpu_idx as u16;
            }
        }
    }

    // If no CPU found on node, fall back to any least loaded CPU
    if !found_on_node {
        for cpu_idx in 0..cpu_count {
            let load = count_processes_on_cpu(cpu_idx as u16);
            if load < min_load {
                min_load = load;
                best_cpu = cpu_idx as u16;
            }
        }
    }

    best_cpu
}

/// Count processes currently assigned to a CPU
fn count_processes_on_cpu(cpu: u16) -> u64 {
    let table = PROCESS_TABLE.lock();
    let mut count = 0u64;

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.state == ProcessState::Ready
            || entry.process.state == ProcessState::Running
        {
            if entry.last_cpu == cpu {
                count += 1;
            }
        }
    }

    count
}

/// Get the recommended CPU for running a process (based on affinity, NUMA, and load)
pub fn get_preferred_cpu(pid: Pid) -> u16 {
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

        // 1. Prefer the last CPU used (cache affinity) if allowed
        if entry.cpu_affinity.is_set(entry.last_cpu as usize) {
            return entry.last_cpu;
        }

        // 2. Try to find a CPU on the preferred NUMA node
        if entry.numa_preferred_node != crate::numa::NUMA_NO_NODE {
            for cpu in entry.cpu_affinity.iter_set() {
                if cpu >= cpu_count {
                    break;
                }
                let apic_id = get_cpu_apic_id(cpu as u16);
                if crate::numa::cpu_to_node(apic_id) == entry.numa_preferred_node {
                    return cpu as u16;
                }
            }
        }

        // 3. Find first available CPU in affinity mask
        if let Some(first_cpu) = entry.cpu_affinity.first_set() {
            if first_cpu < cpu_count {
                return first_cpu as u16;
            }
        }

        return 0;
    }

    0
}

/// Set the preferred NUMA node for a process
pub fn set_numa_preferred_node(pid: Pid, node: u32) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        // Validate node ID
        if node != crate::numa::NUMA_NO_NODE && node >= crate::numa::node_count() {
            return Err("Invalid NUMA node");
        }

        entry.numa_preferred_node = node;
        crate::kinfo!("Set NUMA preferred node for PID {} to {}", pid, node);
        return Ok(());
    }

    Err("Process not found")
}

/// Get the preferred NUMA node for a process
pub fn get_numa_preferred_node(pid: Pid) -> Option<u32> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == pid {
            return Some(entry.numa_preferred_node);
        }
    }

    None
}

/// Set NUMA memory policy for a process
pub fn set_numa_policy(pid: Pid, policy: crate::numa::NumaPolicy) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != pid {
            continue;
        }

        entry.numa_policy = policy;
        crate::kdebug!("Set NUMA policy for PID {} to {:?}", pid, policy);
        return Ok(());
    }

    Err("Process not found")
}
