//! Process management functions
//!
//! This module contains functions for adding, removing, and managing processes
//! in the scheduler.

use alloc::alloc::{dealloc, Layout};
use core::sync::atomic::Ordering;

use crate::process::{Pid, Process, ProcessState, MAX_PROCESSES};
use crate::{kdebug, kerror, ktrace};

use super::priority::calculate_time_slice;
use super::table::{current_pid, set_current_pid, CURRENT_PID, GLOBAL_TICK, PROCESS_TABLE};
use super::types::{ProcessEntry, SchedPolicy};

/// Add a process to the scheduler with full initialization
pub fn add_process(process: Process, priority: u8) -> Result<(), &'static str> {
    add_process_with_policy(process, priority, SchedPolicy::Normal, 0)
}

/// Add a process to the scheduler with policy and nice value
pub fn add_process_with_policy(
    process: Process,
    priority: u8,
    policy: SchedPolicy,
    nice: i8,
) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    for slot in table.iter_mut() {
        if slot.is_none() {
            let quantum_level = match policy {
                SchedPolicy::Realtime => 0, // Shortest quantum, highest priority
                SchedPolicy::Normal => 4,   // Middle level
                SchedPolicy::Batch => 6,    // Longer quantum, lower priority
                SchedPolicy::Idle => 7,     // Longest quantum, lowest priority
            };

            *slot = Some(ProcessEntry {
                process,
                priority,
                base_priority: priority,
                time_slice: calculate_time_slice(quantum_level),
                total_time: 0,
                wait_time: 0,
                last_scheduled: current_tick,
                cpu_burst_count: 0,
                avg_cpu_burst: 0,
                policy,
                nice: nice.clamp(-20, 19),
                quantum_level,
                preempt_count: 0,
                voluntary_switches: 0,
                cpu_affinity: 0xFFFFFFFF, // All CPUs by default
                last_cpu: 0,
            });
            crate::kinfo!(
                "Scheduler: Added process PID {} with priority {}, policy {:?}, nice {} (CR3={:#x})",
                process.pid,
                priority,
                policy,
                nice,
                process.cr3
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

    let mut table = PROCESS_TABLE.lock();
    let mut removed_cr3 = None;
    let mut removed_kernel_stack = 0;
    let mut removed = false;

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                crate::kinfo!("Scheduler: Removed process PID {}", pid);

                // Save CR3 for cleanup after releasing the lock
                if entry.process.cr3 != 0 {
                    removed_cr3 = Some(entry.process.cr3);
                    kdebug!(
                        "[remove_process] PID {} had CR3={:#x}, will free page tables",
                        pid,
                        entry.process.cr3
                    );
                }

                if entry.process.kernel_stack != 0 {
                    removed_kernel_stack = entry.process.kernel_stack;
                }

                *slot = None;
                removed = true;
                break;
            }
        }
    }

    drop(table);

    if removed {
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

        Ok(())
    } else {
        Err("Process not found")
    }
}

/// Update process state
pub fn set_process_state(pid: Pid, state: ProcessState) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
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

    Err("Process not found")
}

/// Record the exit status for a process. This value is preserved while the
/// process sits in the zombie list so that wait4() can report it to the
/// parent.
pub fn set_process_exit_code(pid: Pid, code: i32) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.process.exit_code = code;
                return Ok(());
            }
        }
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
        if let Some(entry) = slot {
            if entry.process.pid == child_pid {
                ktrace!(
                    "[get_child_state] Found PID {}: ppid={}, parent_pid arg={}, state={:?}",
                    child_pid,
                    entry.process.ppid,
                    parent_pid,
                    entry.process.state
                );
                if entry.process.ppid == parent_pid {
                    return Some(entry.process.state);
                } else {
                    kerror!(
                        "[get_child_state] PID {} has wrong parent (ppid={}, expected={})",
                        child_pid,
                        entry.process.ppid,
                        parent_pid
                    );
                    return None;
                }
            }
        }
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
        if let Some(entry) = slot {
            if entry.process.ppid == parent_pid && entry.process.state == target_state {
                return Some(entry.process.pid);
            }
        }
    }

    None
}

/// Mark a process as a forked child (will return 0 from fork when it runs)
pub fn mark_process_as_forked_child(pid: Pid) {
    // In a real implementation, we'd set a flag on the process
    // For now, this is a placeholder - the fork return value handling
    // will be done differently (see fork implementation notes)
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                // Child process is marked as Ready, will be scheduled later
                entry.process.state = ProcessState::Ready;
                crate::kdebug!("Marked PID {} as forked child", pid);
                return;
            }
        }
    }
}

/// Update the CR3 (page table root) associated with a process. When the target
/// process is currently running, the CPU's CR3 register is switched immediately
/// so the new address space takes effect without waiting for the next context
/// switch.
pub fn update_process_cr3(pid: Pid, new_cr3: u64) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();
    let mut found = false;

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.process.cr3 = new_cr3;
                found = true;
                break;
            }
        }
    }

    drop(table);

    if !found {
        return Err("Process not found");
    }

    if current_pid() == Some(pid) {
        crate::paging::activate_address_space(new_cr3);
    }

    Ok(())
}

/// Set the state of the current process
pub fn set_current_process_state(state: ProcessState) {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();

    if let Some(curr_pid) = current {
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == curr_pid {
                    entry.process.state = state;
                    break;
                }
            }
        }
    }
}

/// Wake up a process by PID (set state to Ready)
pub fn wake_process(pid: Pid) -> bool {
    let mut table = PROCESS_TABLE.lock();
    let mut found = false;

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                if entry.process.state == ProcessState::Sleeping {
                    entry.process.state = ProcessState::Ready;
                    // Reset wait time for fairness
                    entry.wait_time = 0;
                    found = true;
                }
                break;
            }
        }
    }
    found
}
