//! Process table and global scheduler state
//!
//! This module contains the global process table and related state management.

use crate::process::{Pid, Process, ProcessState, MAX_PROCESSES};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::types::{ProcessEntry, SchedulerStats};

/// Process table - stores all process entries
pub static PROCESS_TABLE: Mutex<[Option<ProcessEntry>; MAX_PROCESSES]> =
    Mutex::new([None; MAX_PROCESSES]);

/// Currently running process PID
pub static CURRENT_PID: Mutex<Option<Pid>> = Mutex::new(None);

/// Global tick counter for scheduler timing
pub static GLOBAL_TICK: AtomicU64 = AtomicU64::new(0);

/// Scheduler statistics
pub static SCHED_STATS: Mutex<SchedulerStats> = Mutex::new(SchedulerStats::new());

/// Lock the process table for direct access (for syscall use)
pub fn process_table_lock() -> spin::MutexGuard<'static, [Option<ProcessEntry>; MAX_PROCESSES]> {
    PROCESS_TABLE.lock()
}

/// Get current running process PID
pub fn current_pid() -> Option<Pid> {
    *CURRENT_PID.lock()
}

/// Set current running process
pub fn set_current_pid(pid: Option<Pid>) {
    {
        let mut current = CURRENT_PID.lock();
        *current = pid;
    }

    if pid.is_none() {
        // Ensure we always execute kernel code on the kernel address space when
        // no user process is active.
        crate::paging::activate_address_space(0);
    }
}

/// Get the physical address of the page table currently active on the CPU.
/// When no process is running, this falls back to the kernel's page tables.
pub fn current_cr3() -> u64 {
    use super::process::get_process;

    if let Some(pid) = current_pid() {
        if let Some(process) = get_process(pid) {
            if process.cr3 != 0 {
                return process.cr3;
            }
        }
    }

    crate::paging::kernel_pml4_phys()
}

/// Get current global tick count (in milliseconds)
pub fn get_tick() -> u64 {
    GLOBAL_TICK.load(Ordering::Relaxed)
}

/// Get current running process PID (alias for current_pid)
pub fn get_current_pid() -> Option<Pid> {
    current_pid()
}

/// Update the saved user-mode return context for the currently running process.
pub fn update_current_user_context(user_rip: u64, user_rsp: u64, user_rflags: u64) {
    let Some(pid) = current_pid() else {
        return;
    };

    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == pid {
            entry.process.user_rip = user_rip;
            entry.process.user_rsp = user_rsp;
            entry.process.user_rflags = user_rflags;
            break;
        }
    }
}

/// Get process by PID using radix tree for O(log N) lookup
pub fn get_process_from_table(pid: Pid) -> Option<Process> {
    let table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &table[idx] {
                if entry.process.pid == pid {
                    return Some(entry.process);
                }
            }
        }
    }

    // Fallback to linear scan if radix tree lookup fails
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == pid {
            return Some(entry.process);
        }
    }

    None
}
