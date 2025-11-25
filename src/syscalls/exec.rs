//! Exec context management for syscalls
//!
//! This module handles the exec context that allows execve to communicate
//! with the syscall handler assembly code.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::ktrace;

/// Exec context - stores entry/stack/segments for exec syscall
/// Protected by atomics with release/acquire ordering for safety
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

/// Set exec context for syscall handler to pick up
/// Called by execve syscall implementation
pub fn set_exec_context(entry: u64, stack: u64, user_data_sel: u64) {
    // Store entry first
    EXEC_CONTEXT.entry.store(entry, Ordering::SeqCst);
    // Store stack second
    EXEC_CONTEXT.stack.store(stack, Ordering::SeqCst);
    // Store user data segment selector for syscall fast path to restore
    EXEC_CONTEXT.user_data_sel.store(user_data_sel, Ordering::SeqCst);
    // Finally, signal that exec context is ready
    // SeqCst ensures all prior stores are visible before this store
    EXEC_CONTEXT.pending.store(true, Ordering::SeqCst);
}

/// Get and clear exec context (called from assembly)
/// Returns: AL = 1 if exec was pending, 0 otherwise
/// Outputs: entry_out, stack_out, user_data_sel_out (each 8 bytes)
#[no_mangle]
pub extern "C" fn get_exec_context(
    entry_out: *mut u64,
    stack_out: *mut u64,
    user_data_sel_out: *mut u64,
) -> bool {
    if EXEC_CONTEXT
        .pending
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let entry = EXEC_CONTEXT.entry.load(Ordering::SeqCst);
        let stack = EXEC_CONTEXT.stack.load(Ordering::SeqCst);
        let user_data_sel = EXEC_CONTEXT.user_data_sel.load(Ordering::SeqCst);
        unsafe {
            *entry_out = entry;
            *stack_out = stack;
            if !user_data_sel_out.is_null() {
                *user_data_sel_out = user_data_sel;
            }
        }
        ktrace!(
            "[get_exec_context] returning entry={:#x}, stack={:#x}, user_data_sel={:#x}",
            entry,
            stack,
            user_data_sel
        );
        true
    } else {
        ktrace!("[get_exec_context] no exec pending!");
        false
    }
}
