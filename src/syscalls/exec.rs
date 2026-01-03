//! Exec context management for syscalls
//!
//! This module handles the exec context that allows execve to communicate
//! with the syscall handler assembly code.
//!
//! FIXED: Now uses per-process exec context stored in Process struct instead
//! of global EXEC_CONTEXT. This prevents race conditions where:
//! 1. Process A sets exec context
//! 2. Process B overwrites it before A consumes
//! 3. A gets B's entry point (wrong!)

use crate::ktrace;
use crate::scheduler;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Legacy global exec context - DEPRECATED, kept for compatibility
/// Use set_exec_context_for_process and get_exec_context instead
pub struct ExecContext {
    pub pending: AtomicBool,
    pub entry: AtomicU64,
    pub stack: AtomicU64,
    pub user_data_sel: AtomicU64,
}

pub static EXEC_CONTEXT: ExecContext = ExecContext {
    pending: AtomicBool::new(false),
    entry: AtomicU64::new(0),
    stack: AtomicU64::new(0),
    user_data_sel: AtomicU64::new(0),
};

/// Set exec context for a specific process (NEW - race-condition-free)
/// Called by execve syscall implementation
pub fn set_exec_context_for_process(pid: u64, entry: u64, stack: u64, user_data_sel: u64) {
    scheduler::with_process_mut(pid, |proc| {
        proc.exec_pending = true;
        proc.exec_entry = entry;
        proc.exec_stack = stack;
        proc.exec_user_data_sel = user_data_sel;
        ktrace!(
            "[set_exec_context_for_process] pid={} entry={:#x} stack={:#x}",
            pid,
            entry,
            stack
        );
    });
}

/// Set exec context for current process (convenience wrapper)
/// Called by execve syscall implementation
pub fn set_exec_context(entry: u64, stack: u64, user_data_sel: u64) {
    if let Some(pid) = scheduler::current_pid() {
        set_exec_context_for_process(pid, entry, stack, user_data_sel);
    }
}

/// Get and clear exec context for current process (called from assembly)
/// Returns: AL = 1 if exec was pending, 0 otherwise
/// Outputs: entry_out, stack_out, user_data_sel_out (each 8 bytes)
#[no_mangle]
pub extern "C" fn get_exec_context(
    entry_out: *mut u64,
    stack_out: *mut u64,
    user_data_sel_out: *mut u64,
) -> bool {
    let pid = match scheduler::current_pid() {
        Some(p) => p,
        None => {
            ktrace!("[get_exec_context] no current process!");
            return false;
        }
    };

    let result =
        scheduler::with_process_mut(pid, |proc| {
            if proc.exec_pending {
                // Atomically consume the exec context
                proc.exec_pending = false;
                let entry = proc.exec_entry;
                let stack = proc.exec_stack;
                let user_data_sel = proc.exec_user_data_sel;

                unsafe {
                    *entry_out = entry;
                    *stack_out = stack;
                    if !user_data_sel_out.is_null() {
                        *user_data_sel_out = user_data_sel;
                    }
                }
                ktrace!(
                "[get_exec_context] pid={} returning entry={:#x}, stack={:#x}, user_data_sel={:#x}",
                pid, entry, stack, user_data_sel
            );
                true
            } else {
                false
            }
        });

    match result {
        Some(true) => true,
        _ => {
            ktrace!("[get_exec_context] pid={} no exec pending!", pid);
            false
        }
    }
}
