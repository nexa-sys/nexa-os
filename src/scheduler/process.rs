//! Process management functions
//!
//! This module contains functions for adding, removing, and managing processes
//! in the EEVDF scheduler.

use alloc::alloc::{dealloc, Layout};
use core::sync::atomic::Ordering;

use crate::process::{Pid, Process, ProcessState, MAX_PROCESSES};
use crate::{kdebug, kerror, ktrace};

use super::priority::{calc_vdeadline, get_min_vruntime, update_min_vruntime};
use super::table::{current_pid, set_current_pid, CURRENT_PID, GLOBAL_TICK, PROCESS_TABLE};
use super::types::{nice_to_weight, ProcessEntry, SchedPolicy, BASE_SLICE_NS, DEFAULT_TIME_SLICE};

/// Add a process to the scheduler with full initialization
pub fn add_process(process: Process, priority: u8) -> Result<(), &'static str> {
    add_process_with_policy(process, priority, SchedPolicy::Normal, 0)
}

/// Add a process to the scheduler with EEVDF initialization
pub fn add_process_with_policy(
    process: Process,
    priority: u8,
    policy: SchedPolicy,
    nice: i8,
) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);
    let min_vrt = get_min_vruntime();

    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            // Register PID in the radix tree for O(log N) lookup
            let pid = process.pid;
            if !crate::process::register_pid_mapping(pid, idx as u16) {
                kerror!("Failed to register PID {} in radix tree", pid);
                // Continue anyway - linear fallback will still work
            }

            let nice_clamped = nice.clamp(-20, 19);
            let weight = nice_to_weight(nice_clamped);

            // EEVDF: Calculate initial time slice based on policy
            let slice_ns = match policy {
                SchedPolicy::Realtime => BASE_SLICE_NS * 2,
                SchedPolicy::Normal => BASE_SLICE_NS,
                SchedPolicy::Batch => BASE_SLICE_NS * 4,
                SchedPolicy::Idle => BASE_SLICE_NS,
            };

            // New processes start at min_vruntime to prevent starvation
            let initial_vruntime = min_vrt;
            let initial_deadline = calc_vdeadline(initial_vruntime, slice_ns, weight);

            *slot = Some(ProcessEntry {
                process,
                // EEVDF fields
                vruntime: initial_vruntime,
                vdeadline: initial_deadline,
                lag: 0, // New processes start with neutral lag
                weight,
                slice_ns,
                slice_remaining_ns: slice_ns,
                // Legacy fields
                priority,
                base_priority: priority,
                time_slice: DEFAULT_TIME_SLICE,
                total_time: 0,
                wait_time: 0,
                last_scheduled: current_tick,
                cpu_burst_count: 0,
                avg_cpu_burst: 0,
                policy,
                nice: nice_clamped,
                quantum_level: 0, // Not used in EEVDF
                preempt_count: 0,
                voluntary_switches: 0,
                cpu_affinity: super::types::CpuMask::all(), // All CPUs by default
                last_cpu: 0,
                // NUMA fields
                numa_preferred_node: crate::numa::NUMA_NO_NODE,
                numa_policy: crate::numa::NumaPolicy::Local,
            });

            drop(table);
            update_min_vruntime();

            crate::kinfo!(
                "EEVDF: Added PID {} (weight={}, vrt={}, vdl={}, policy={:?})",
                pid,
                weight,
                initial_vruntime,
                initial_deadline,
                policy
            );
            return Ok(());
        }
    }

    Err("Process table full")
}

/// Remove a process from the scheduler
/// This also handles cleanup of process-specific resources including page tables.
pub fn remove_process(pid: Pid) -> Result<(), &'static str> {
    kdebug!("[remove_process] Removing PID {}", pid);

    let removal_result = {
        let mut table = PROCESS_TABLE.lock();

        // Try radix tree lookup first (O(log N)), fall back to linear scan
        let slot_idx = crate::process::lookup_pid(pid)
            .map(|idx| idx as usize)
            .filter(|&idx| {
                idx < table.len() && table[idx].as_ref().map_or(false, |e| e.process.pid == pid)
            })
            .or_else(|| {
                // Fallback to linear search if radix tree lookup fails or is stale
                table
                    .iter()
                    .position(|slot| slot.as_ref().map_or(false, |e| e.process.pid == pid))
            });

        let Some(idx) = slot_idx else {
            return Err("Process not found");
        };

        let entry = table[idx].as_ref().unwrap();
        crate::kinfo!("Scheduler: Removed process PID {}", pid);

        let cr3 = if entry.process.cr3 != 0 {
            kdebug!(
                "[remove_process] PID {} had CR3={:#x}, will free page tables",
                pid,
                entry.process.cr3
            );
            Some(entry.process.cr3)
        } else {
            None
        };

        let kernel_stack = entry.process.kernel_stack;
        table[idx] = None;
        (cr3, kernel_stack)
    };

    let (removed_cr3, removed_kernel_stack) = removal_result;

    if current_pid() == Some(pid) {
        set_current_pid(None);
    }

    // Clean up kernel stack
    if removed_kernel_stack != 0 {
        let layout = Layout::from_size_align(
            crate::process::KERNEL_STACK_SIZE,
            crate::process::KERNEL_STACK_ALIGN,
        )
        .unwrap();
        unsafe { dealloc(removed_kernel_stack as *mut u8, layout) };
    }

    // Clean up process page tables if it had its own CR3
    if let Some(cr3) = removed_cr3 {
        crate::kdebug!("Freeing page tables for PID {} (CR3={:#x})", pid, cr3);
        crate::paging::free_process_address_space(cr3);
        kdebug!(
            "[remove_process] Freed page tables for PID {} (CR3={:#x})",
            pid,
            cr3
        );
    }

    // Free the PID for reuse (removes from radix tree and marks as available)
    crate::process::free_pid(pid);
    kdebug!("[remove_process] Freed PID {} for reuse", pid);

    Ok(())
}

/// Update process state using radix tree for O(log N) lookup
pub fn set_process_state(pid: Pid, state: ProcessState) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    ktrace!(
                        "[set_process_state] PID {} state: {:?} -> {:?}",
                        pid,
                        entry.process.state,
                        state
                    );
                    entry.process.state = state;
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

        ktrace!(
            "[set_process_state] PID {} state: {:?} -> {:?}",
            pid,
            entry.process.state,
            state
        );
        entry.process.state = state;
        return Ok(());
    }

    Err("Process not found")
}

/// Record the exit status for a process using radix tree for O(log N) lookup.
/// This value is preserved while the process sits in the zombie list so that
/// wait4() can report it to the parent.
pub fn set_process_exit_code(pid: Pid, code: i32) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    entry.process.exit_code = code;
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

        entry.process.exit_code = code;
        return Ok(());
    }

    Err("Process not found")
}

/// Set the termination signal for a process (for signal-terminated processes)
/// This is used to properly encode the wait status for wait4()
pub fn set_process_term_signal(pid: Pid, signal: i32) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    entry.process.term_signal = Some(signal);
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

        entry.process.term_signal = Some(signal);
        return Ok(());
    }

    Err("Process not found")
}

/// Get process by PID
pub fn get_process(pid: Pid) -> Option<Process> {
    super::table::get_process_from_table(pid)
}

/// Query a specific child process state
/// Returns the child's state if found and is a child of parent_pid
pub fn get_child_state(parent_pid: Pid, child_pid: Pid) -> Option<ProcessState> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != child_pid {
            continue;
        }

        ktrace!(
            "[get_child_state] Found PID {}: ppid={}, parent_pid arg={}, state={:?}",
            child_pid,
            entry.process.ppid,
            parent_pid,
            entry.process.state
        );

        if entry.process.ppid == parent_pid {
            return Some(entry.process.state);
        }

        kerror!(
            "[get_child_state] PID {} has wrong parent (ppid={}, expected={})",
            child_pid,
            entry.process.ppid,
            parent_pid
        );
        return None;
    }

    kdebug!(
        "[get_child_state] PID {} not found in process table",
        child_pid
    );
    None
}

/// Find a child process by parent PID and state
/// Returns first matching child PID if found
pub fn find_child_with_state(parent_pid: Pid, target_state: ProcessState) -> Option<Pid> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.ppid == parent_pid && entry.process.state == target_state {
            return Some(entry.process.pid);
        }
    }

    None
}

/// Mark a process as a forked child (will return 0 from fork when it runs)
/// Uses radix tree for O(log N) lookup
pub fn mark_process_as_forked_child(pid: Pid) {
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    entry.process.state = ProcessState::Ready;
                    crate::kdebug!("Marked PID {} as forked child", pid);
                    return;
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

        entry.process.state = ProcessState::Ready;
        crate::kdebug!("Marked PID {} as forked child", pid);
        return;
    }
}

/// Update the CR3 (page table root) associated with a process using radix tree
/// for O(log N) lookup. When the target process is currently running, the CPU's
/// CR3 register is switched immediately so the new address space takes effect
/// without waiting for the next context switch.
pub fn update_process_cr3(pid: Pid, new_cr3: u64) -> Result<(), &'static str> {
    {
        let mut table = PROCESS_TABLE.lock();

        // Try radix tree lookup first (O(log N))
        let found = if let Some(idx) = crate::process::lookup_pid(pid) {
            let idx = idx as usize;
            if idx < table.len() {
                if let Some(entry) = &mut table[idx] {
                    if entry.process.pid == pid {
                        entry.process.cr3 = new_cr3;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // Fallback to linear scan if radix tree lookup failed
        if !found {
            let entry = table
                .iter_mut()
                .find_map(|slot| slot.as_mut().filter(|e| e.process.pid == pid));

            let Some(entry) = entry else {
                return Err("Process not found");
            };

            entry.process.cr3 = new_cr3;
        }
    }

    if current_pid() == Some(pid) {
        crate::paging::activate_address_space(new_cr3);
    }

    Ok(())
}

/// Set the state of the current process using radix tree for O(log N) lookup
pub fn set_current_process_state(state: ProcessState) {
    let Some(curr_pid) = *CURRENT_PID.lock() else {
        return;
    };

    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(curr_pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == curr_pid {
                    entry.process.state = state;
                    return;
                }
            }
        }
    }

    // Fallback to linear scan
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == curr_pid {
            entry.process.state = state;
            break;
        }
    }
}

/// Wake up a process by PID (EEVDF: adjust vruntime for waking process)
pub fn wake_process(pid: Pid) -> bool {
    let min_vrt = get_min_vruntime();
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    if entry.process.state == ProcessState::Sleeping {
                        entry.process.state = ProcessState::Ready;
                        entry.wait_time = 0;

                        // EEVDF: Adjust vruntime for waking process
                        // Give some credit but not too much to prevent unfair advantage
                        if entry.vruntime < min_vrt {
                            let credit = super::types::BASE_SLICE_NS / 2;
                            entry.vruntime = min_vrt.saturating_sub(credit);
                        }
                        // Recalculate deadline
                        entry.vdeadline =
                            calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
                        entry.lag = 0; // Reset lag on wake

                        return true;
                    }
                    return false;
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

        if entry.process.state == ProcessState::Sleeping {
            entry.process.state = ProcessState::Ready;
            entry.wait_time = 0;

            // EEVDF: Adjust vruntime for waking process
            if entry.vruntime < min_vrt {
                let credit = super::types::BASE_SLICE_NS / 2;
                entry.vruntime = min_vrt.saturating_sub(credit);
            }
            entry.vdeadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
            entry.lag = 0;

            return true;
        }
        return false;
    }
    false
}
