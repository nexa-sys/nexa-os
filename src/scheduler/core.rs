//! Core scheduling algorithms
//!
//! This module contains the main scheduling algorithms including the MLFQ
//! scheduler, timer tick handler, and context switch logic.

use core::sync::atomic::Ordering;

use crate::process::{Pid, ProcessState, MAX_PROCESSES};
use crate::{kdebug, ktrace};

use super::context::context_switch;
use super::priority::{calculate_dynamic_priority, calculate_time_slice};
use super::table::{
    current_pid, set_current_pid, CURRENT_PID, GLOBAL_TICK, PROCESS_TABLE, SCHED_STATS,
};
use super::types::{SchedPolicy, DEFAULT_TIME_SLICE, NUM_PRIORITY_LEVELS, BASE_TIME_SLICE_MS};

/// Initialize scheduler subsystem
pub fn init() {
    crate::kinfo!(
        "Advanced process scheduler initialized (MLFQ with {} priority levels, {} max processes, {}ms base quantum)",
        NUM_PRIORITY_LEVELS,
        MAX_PROCESSES,
        BASE_TIME_SLICE_MS
    );
    crate::kinfo!(
        "Scheduling policies: Realtime, Normal, Batch, Idle with dynamic priority adjustment"
    );
}

/// Compare two candidate processes for scheduling priority.
/// Returns true if `candidate` should replace `best`.
#[inline]
fn should_replace_candidate(
    candidate: (usize, u8, SchedPolicy, u64),
    best: (usize, u8, SchedPolicy, u64),
) -> bool {
    // Compare by priority within same policy class
    let same_policy_compare = |c_pri: u8, c_wait: u64, b_pri: u8, b_wait: u64| -> bool {
        c_pri < b_pri || (c_pri == b_pri && c_wait > b_wait)
    };

    match (candidate.2, best.2) {
        (SchedPolicy::Realtime, SchedPolicy::Realtime) => {
            same_policy_compare(candidate.1, candidate.3, best.1, best.3)
        }
        (SchedPolicy::Realtime, _) => true,
        (_, SchedPolicy::Realtime) => false,
        (SchedPolicy::Normal, SchedPolicy::Normal) => {
            same_policy_compare(candidate.1, candidate.3, best.1, best.3)
        }
        (SchedPolicy::Normal, _) => true,
        (_, SchedPolicy::Normal) => false,
        (SchedPolicy::Batch, SchedPolicy::Batch) => {
            same_policy_compare(candidate.1, candidate.3, best.1, best.3)
        }
        (SchedPolicy::Batch, _) => true,
        (_, SchedPolicy::Batch) => false,
        (SchedPolicy::Idle, SchedPolicy::Idle) => {
            same_policy_compare(candidate.1, candidate.3, best.1, best.3)
        }
    }
}

/// Update wait times and dynamic priorities for all ready processes
fn update_ready_process_priorities(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    current_tick: u64,
) {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        let wait_delta = current_tick.saturating_sub(entry.last_scheduled);
        entry.wait_time = entry.wait_time.saturating_add(wait_delta);
        entry.priority = calculate_dynamic_priority(
            entry.base_priority,
            entry.wait_time,
            entry.total_time,
            entry.nice,
        );
    }
}

/// Find the starting index for round-robin scheduling
fn find_start_index(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    current_pid: Option<Pid>,
) -> usize {
    let Some(curr_pid) = current_pid else {
        return 0;
    };

    for (idx, slot) in table.iter().enumerate() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == curr_pid {
            return (idx + 1) % MAX_PROCESSES;
        }
    }
    0
}

/// Find the best ready process candidate for scheduling
fn find_best_candidate(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    start_idx: usize,
) -> Option<(usize, u8, SchedPolicy, u64)> {
    let mut best_candidate: Option<(usize, u8, SchedPolicy, u64)> = None;

    for offset in 0..MAX_PROCESSES {
        let idx = (start_idx + offset) % MAX_PROCESSES;
        let Some(entry) = &table[idx] else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        let candidate = (idx, entry.priority, entry.policy, entry.wait_time);
        let should_use = best_candidate
            .map(|best| should_replace_candidate(candidate, best))
            .unwrap_or(true);

        if should_use {
            best_candidate = Some(candidate);
        }
    }

    best_candidate
}

/// Round-robin scheduler: select next process to run with MLFQ enhancements
/// Uses multi-level feedback queue for better responsiveness and fairness
pub fn schedule() -> Option<Pid> {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    update_ready_process_priorities(&mut table, current_tick);
    let start_idx = find_start_index(&table, current);
    let best_candidate = find_best_candidate(&table, start_idx);

    let Some((next_idx, _, _, _)) = best_candidate else {
        return None;
    };

    let next_pid = table[next_idx].as_ref().unwrap().process.pid;

    // Update previous process state
    if let Some(curr_pid) = current {
        update_previous_process_state(&mut table, curr_pid, current_tick);
    }

    // Update next process state
    if let Some(entry) = table[next_idx].as_mut() {
        entry.time_slice = calculate_time_slice(entry.quantum_level);
        entry.process.state = ProcessState::Running;
        entry.last_scheduled = current_tick;
        entry.wait_time = 0;
        entry.cpu_burst_count += 1;
    }

    drop(table);
    *CURRENT_PID.lock() = Some(next_pid);
    Some(next_pid)
}

/// Update state of the previous running process to Ready
fn update_previous_process_state(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Pid,
    current_tick: u64,
) {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != curr_pid || entry.process.state != ProcessState::Running {
            continue;
        }
        entry.process.state = ProcessState::Ready;
        entry.last_scheduled = current_tick;
        break;
    }
}

/// Check if a ready process should preempt the current running process
#[inline]
fn should_preempt_for(ready_policy: SchedPolicy, ready_priority: u8, 
                       current_policy: SchedPolicy, current_priority: u8) -> bool {
    match (ready_policy, current_policy) {
        (SchedPolicy::Realtime, SchedPolicy::Realtime) => ready_priority < current_priority,
        (SchedPolicy::Realtime, _) => true,
        (_, SchedPolicy::Realtime) => false,
        (SchedPolicy::Normal, SchedPolicy::Normal) => ready_priority + 10 < current_priority,
        (SchedPolicy::Normal, _) => true,
        (_, SchedPolicy::Normal) => false,
        _ => false,
    }
}

/// Check if any ready process has higher priority than the current one
fn has_higher_priority_ready_process(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    current_policy: SchedPolicy,
    current_priority: u8,
) -> bool {
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }
        if should_preempt_for(entry.policy, entry.priority, current_policy, current_priority) {
            return true;
        }
    }
    false
}

/// Handle preemption: update preempt count and potentially demote quantum level
fn handle_preemption(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Pid,
) {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != curr_pid {
            continue;
        }
        entry.preempt_count += 1;
        // MLFQ: Demote to lower priority level if preempted too much
        if entry.preempt_count > 3 && entry.quantum_level < 7 {
            entry.quantum_level += 1;
            crate::kdebug!(
                "Process {} demoted to quantum level {}",
                curr_pid,
                entry.quantum_level
            );
        }
        break;
    }
}

/// Find the entry for the currently running process
fn find_current_running_entry_mut<'a>(
    table: &'a mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Pid,
) -> Option<&'a mut super::types::ProcessEntry> {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == curr_pid && entry.process.state == ProcessState::Running {
            return Some(entry);
        }
    }
    None
}

/// Timer tick handler: update time slices and trigger scheduling
/// Implements preemptive scheduling with dynamic priority adjustments
pub fn tick(elapsed_ms: u64) -> bool {
    GLOBAL_TICK.fetch_add(1, Ordering::Relaxed);

    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();

    let Some(curr_pid) = current else {
        return false;
    };

    // Find and update the current running process
    let Some(entry) = find_current_running_entry_mut(&mut table, curr_pid) else {
        return false;
    };

    entry.total_time += elapsed_ms;

    // Time slice exhausted - need to reschedule
    if entry.time_slice <= elapsed_ms {
        entry.time_slice = 0;
        // MLFQ: Move to lower priority level after exhausting time slice
        if entry.quantum_level < 7 {
            entry.quantum_level += 1;
        }
        // Update average CPU burst
        let new_burst = entry.total_time / entry.cpu_burst_count.max(1);
        entry.avg_cpu_burst = (entry.avg_cpu_burst + new_burst) / 2;
        return true;
    }

    // Time slice not exhausted - decrement and check for preemption
    entry.time_slice -= elapsed_ms;

    // Update average CPU burst
    let new_burst = entry.total_time / entry.cpu_burst_count.max(1);
    entry.avg_cpu_burst = (entry.avg_cpu_burst + new_burst) / 2;

    let current_priority = entry.priority;
    let current_policy = entry.policy;

    // Release and re-acquire lock to check for higher priority processes
    drop(table);
    let mut table = PROCESS_TABLE.lock();

    // Check for higher priority ready processes
    if !has_higher_priority_ready_process(&table, current_policy, current_priority) {
        return false; // Continue running current process
    }

    // Preemption due to higher priority process
    handle_preemption(&mut table, curr_pid);
    true
}

/// Perform context switch to next ready process with statistics tracking
pub fn do_schedule() {
    do_schedule_internal(false);
}

pub fn do_schedule_from_interrupt() {
    do_schedule_internal(true);
}

enum ScheduleDecision {
    FirstRun(crate::process::Process),
    Switch {
        old_context_ptr: *mut crate::process::Context,
        next_context: crate::process::Context,
        next_cr3: u64,
        user_rip: u64,
        user_rsp: u64,
        user_rflags: u64,
        is_voluntary: bool,
        kernel_stack: u64,
    },
}

/// Print process table snapshot for debugging
fn debug_print_process_table(table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES]) {
    ktrace!("[do_schedule] Process table snapshot:");
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        ktrace!(
            "  PID {}: ppid={}, state={:?}, policy={:?}, CR3={:#x}",
            entry.process.pid,
            entry.process.ppid,
            entry.process.state,
            entry.policy,
            entry.process.cr3
        );
    }
}

/// Find index of parent process to prioritize when child is zombie
fn find_zombie_parent_index(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Pid,
) -> Option<usize> {
    // Find current process entry
    let curr_entry = table.iter().find_map(|slot| {
        slot.as_ref().filter(|e| e.process.pid == curr_pid)
    })?;

    // Only proceed if current process is zombie
    if curr_entry.process.state != ProcessState::Zombie {
        return None;
    }

    let parent_pid = curr_entry.process.ppid;
    if parent_pid == 0 {
        return None;
    }

    // Find ready parent
    table.iter().position(|slot| {
        slot.as_ref().map_or(false, |e| {
            e.process.pid == parent_pid && e.process.state == ProcessState::Ready
        })
    }).map(|idx| {
        kdebug!(
            "[do_schedule] Child PID {} is Zombie, prioritizing parent PID {}",
            curr_pid, parent_pid
        );
        idx
    })
}

/// Find next ready process index using round-robin
fn find_next_ready_index(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    start_idx: usize,
) -> Option<usize> {
    for offset in 0..MAX_PROCESSES {
        let idx = (start_idx + offset) % MAX_PROCESSES;
        let Some(entry) = &table[idx] else { continue };
        if entry.process.state == ProcessState::Ready {
            return Some(idx);
        }
    }
    None
}

/// Save syscall context from GS_DATA to process entry
unsafe fn save_syscall_context_to_entry(entry: &mut super::types::ProcessEntry, curr_pid: Pid) {
    let gs_data_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64;
    let saved_rip = gs_data_ptr.add(crate::interrupts::GS_SLOT_SAVED_RCX).read();
    let saved_rsp = gs_data_ptr.add(crate::interrupts::GS_SLOT_USER_RSP).read();
    let saved_rflags = gs_data_ptr.add(crate::interrupts::GS_SLOT_SAVED_RFLAGS).read();

    ktrace!(
        "[do_schedule] Saving syscall context for PID {}: rip={:#x}, rsp={:#x}, rflags={:#x}",
        curr_pid, saved_rip, saved_rsp, saved_rflags
    );

    entry.process.user_rip = saved_rip;
    entry.process.user_rsp = saved_rsp;
    entry.process.user_rflags = saved_rflags;
}

/// Transition current running process to Ready state
fn transition_current_to_ready(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Pid,
    from_interrupt: bool,
) {
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid != curr_pid || entry.process.state != ProcessState::Running {
            continue;
        }

        // Save syscall context if not from interrupt
        if !from_interrupt {
            unsafe { save_syscall_context_to_entry(entry, curr_pid) };
        }

        entry.process.state = ProcessState::Ready;
        break;
    }
}

/// Extract process info for context switch
fn extract_next_process_info(
    entry: &mut super::types::ProcessEntry,
) -> (bool, Pid, u64, u64, u64, u64, crate::process::Context, u64, crate::process::Process) {
    entry.time_slice = DEFAULT_TIME_SLICE;
    entry.process.state = ProcessState::Running;

    (
        !entry.process.has_entered_user,
        entry.process.pid,
        entry.process.cr3,
        entry.process.user_rip,
        entry.process.user_rsp,
        entry.process.user_rflags,
        entry.process.context,
        entry.process.kernel_stack,
        entry.process, // Process is Copy
    )
}

/// Get old context pointer and voluntary flag for current process
fn get_old_context_info(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Option<Pid>,
) -> (Option<*mut crate::process::Context>, bool) {
    let Some(pid) = curr_pid else {
        return (None, false);
    };

    for slot in table.iter_mut() {
        let Some(candidate) = slot else { continue };
        if candidate.process.pid != pid {
            continue;
        }

        let voluntary = candidate.process.state == ProcessState::Sleeping || candidate.time_slice > 0;
        if voluntary {
            candidate.voluntary_switches += 1;
        }

        // Don't save context for zombie processes
        if candidate.process.state == ProcessState::Zombie {
            kdebug!("[do_schedule] Current PID {} is Zombie, not saving context", pid);
            return (None, voluntary);
        }

        return (Some(&mut candidate.process.context as *mut _), voluntary);
    }

    (None, false)
}

/// Mark process as entered user mode in the process table
/// Uses radix tree for O(log N) lookup
fn mark_process_entered_user(pid: Pid) {
    let mut table = PROCESS_TABLE.lock();

    // Try radix tree lookup first (O(log N))
    if let Some(idx) = crate::process::lookup_pid(pid) {
        let idx = idx as usize;
        if idx < table.len() {
            if let Some(entry) = &mut table[idx] {
                if entry.process.pid == pid {
                    entry.process.has_entered_user = true;
                    return;
                }
            }
        }
    }

    // Fallback to linear scan
    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.pid == pid {
            entry.process.has_entered_user = true;
            break;
        }
    }
}

/// Execute first-run process
fn execute_first_run(mut process: crate::process::Process) {
    kdebug!(
        "[do_schedule] FirstRun: PID={}, entry={:#x}, stack={:#x}, has_entered_user={}, CR3={:#x}",
        process.pid, process.entry_point, process.stack_top, process.has_entered_user, process.cr3
    );

    if process.cr3 == 0 {
        crate::kfatal!(
            "PANIC: FirstRun for PID {} has CR3=0! Entry={:#x}, Stack={:#x}, MemBase={:#x}",
            process.pid, process.entry_point, process.stack_top, process.memory_base
        );
    }

    mark_process_entered_user(process.pid);
    process.execute();
    crate::kfatal!("process::execute returned unexpectedly");
}

/// Execute context switch to next process
unsafe fn execute_context_switch(
    old_context_ptr: *mut crate::process::Context,
    next_context: &crate::process::Context,
    next_cr3: u64,
    user_rip: u64,
    user_rsp: u64,
    user_rflags: u64,
    is_voluntary: bool,
    kernel_stack: u64,
) {
    // Update kernel stack in GS
    if kernel_stack != 0 {
        let gs_data_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *mut u64;
        gs_data_ptr
            .add(crate::interrupts::GS_SLOT_KERNEL_RSP)
            .write(kernel_stack + crate::process::KERNEL_STACK_SIZE as u64);
    }

    // Update statistics
    {
        let mut stats = SCHED_STATS.lock();
        if is_voluntary {
            stats.total_voluntary_switches += 1;
        } else {
            stats.total_preemptions += 1;
        }
    }

    ktrace!(
        "[do_schedule] Switch ({}): user_rip={:#x}, user_rsp={:#x}, user_rflags={:#x}",
        if is_voluntary { "voluntary" } else { "preempt" },
        user_rip, user_rsp, user_rflags
    );

    if user_rsp != 0 {
        crate::interrupts::restore_user_syscall_context(user_rip, user_rsp, user_rflags);
    }
    crate::paging::activate_address_space(next_cr3);
    context_switch(old_context_ptr, next_context as *const _);
}

fn do_schedule_internal(from_interrupt: bool) {
    crate::net::poll();

    {
        let mut stats = SCHED_STATS.lock();
        stats.total_context_switches += 1;
    }

    {
        let table = PROCESS_TABLE.lock();
        debug_print_process_table(&table);
    }

    let decision = compute_schedule_decision(from_interrupt);

    match decision {
        Some(ScheduleDecision::FirstRun(process)) => execute_first_run(process),
        Some(ScheduleDecision::Switch {
            old_context_ptr, next_context, next_cr3,
            user_rip, user_rsp, user_rflags, is_voluntary, kernel_stack,
        }) => unsafe {
            execute_context_switch(
                old_context_ptr, &next_context, next_cr3,
                user_rip, user_rsp, user_rflags, is_voluntary, kernel_stack,
            );
        },
        None => {
            set_current_pid(None);
            crate::kwarn!("do_schedule(): No ready process found, returning to caller");
        }
    }
}

/// Compute the scheduling decision: which process to run next
fn compute_schedule_decision(from_interrupt: bool) -> Option<ScheduleDecision> {
    let mut table = PROCESS_TABLE.lock();
    let mut current_lock = CURRENT_PID.lock();
    let current = *current_lock;

    let start_idx = current
        .and_then(|pid| {
            table.iter().position(|e| e.as_ref().map_or(false, |p| p.process.pid == pid))
        })
        .map(|i| (i + 1) % MAX_PROCESSES)
        .unwrap_or(0);

    // Try to prioritize parent of zombie child first
    let next_idx = current
        .and_then(|pid| find_zombie_parent_index(&table, pid))
        .or_else(|| find_next_ready_index(&table, start_idx))?;

    // Transition current process to Ready
    if let Some(curr_pid) = current {
        transition_current_to_ready(&mut table, curr_pid, from_interrupt);
    }

    let entry = table[next_idx].as_mut().expect("Process entry vanished");
    let (first_run, next_pid, next_cr3, user_rip, user_rsp, user_rflags, next_context, kernel_stack, process_copy) =
        extract_next_process_info(entry);

    *current_lock = Some(next_pid);

    if first_run {
        kdebug!("[do_schedule] Creating FirstRun decision for PID {}, CR3={:#x}", next_pid, next_cr3);
        return Some(ScheduleDecision::FirstRun(process_copy));
    }

    let (old_context_opt, is_voluntary) = get_old_context_info(&mut table, current);

    Some(ScheduleDecision::Switch {
        old_context_ptr: old_context_opt.unwrap_or(core::ptr::null_mut()),
        next_context,
        next_cr3,
        user_rip,
        user_rsp,
        user_rflags,
        is_voluntary,
        kernel_stack,
    })
}
