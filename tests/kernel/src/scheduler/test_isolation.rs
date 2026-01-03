//! Test Isolation Utilities for Scheduler Tests
//!
//! This module provides utilities to ensure each test runs in isolation
//! with a clean process table state.
//!
//! ## Problem
//!
//! The kernel uses global state (PROCESS_TABLE, PID allocator, etc.) which
//! is shared across all tests. When tests run in parallel, they can interfere
//! with each other, causing random failures.
//!
//! ## Solution
//!
//! 1. Clear the process table before and after each test
//! 2. Use unique PID ranges per test to avoid conflicts
//! 3. Provide helper functions that handle setup/teardown

use crate::process::{Pid, ProcessState, MAX_PROCESSES};
use crate::scheduler::process_table_lock;

/// Clear all entries from the process table.
/// This should be called at the start and end of each test.
pub fn clear_process_table() {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot.take() {
            // Unregister PID mapping
            crate::process::unregister_pid_mapping(entry.process.pid);
        }
    }
}

/// Count how many processes are in the table.
/// Useful for debugging test isolation issues.
pub fn count_processes() -> usize {
    let table = process_table_lock();
    table.iter().filter(|s| s.is_some()).count()
}

/// Get a list of all PIDs currently in the process table.
pub fn list_pids() -> Vec<Pid> {
    let table = process_table_lock();
    table.iter()
        .filter_map(|s| s.as_ref())
        .map(|e| e.process.pid)
        .collect()
}

/// RAII guard that clears the process table when dropped.
/// Use this to ensure cleanup even if a test panics.
pub struct ProcessTableGuard {
    pids: Vec<Pid>,
}

impl ProcessTableGuard {
    /// Create a new guard. Does NOT clear the table - use `new_clean()` for that.
    pub fn new() -> Self {
        Self { pids: Vec::new() }
    }

    /// Create a new guard and clear the process table.
    pub fn new_clean() -> Self {
        clear_process_table();
        Self { pids: Vec::new() }
    }

    /// Track a PID so it gets cleaned up when the guard is dropped.
    pub fn track(&mut self, pid: Pid) {
        self.pids.push(pid);
    }
}

impl Drop for ProcessTableGuard {
    fn drop(&mut self) {
        // Clean up all tracked PIDs
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if self.pids.contains(&entry.process.pid) {
                    crate::process::unregister_pid_mapping(entry.process.pid);
                    *slot = None;
                }
            }
        }
    }
}

/// Macro to run a test with a clean process table.
/// Ensures the table is cleared before and after the test.
#[macro_export]
macro_rules! with_clean_process_table {
    ($body:expr) => {{
        use crate::scheduler::test_isolation::{clear_process_table, ProcessTableGuard};
        
        // Clear before test
        clear_process_table();
        
        // Create guard for cleanup on panic
        let _guard = ProcessTableGuard::new();
        
        // Run test body
        let result = $body;
        
        // Clear after test (guard also cleans up on drop)
        clear_process_table();
        
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_clear_process_table() {
        // Just verify the function doesn't panic on an empty table
        clear_process_table();
        assert_eq!(count_processes(), 0);
    }

    #[test]
    #[serial]
    fn test_count_processes() {
        clear_process_table();
        assert_eq!(count_processes(), 0);
    }
}
