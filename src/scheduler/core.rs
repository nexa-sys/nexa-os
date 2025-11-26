//! Core scheduling algorithms - EEVDF Implementation
//!
//! This module implements the EEVDF (Earliest Eligible Virtual Deadline First)
//! scheduler, as used in Linux 6.6+. It provides fair CPU time distribution
//! with good latency guarantees.
//!
//! ## EEVDF Key Properties:
//! - Virtual runtime tracks weighted CPU consumption
//! - Virtual deadlines provide latency guarantees  
//! - Eligibility check ensures fairness (lag >= 0)
//! - Among eligible processes, earliest deadline wins

use core::sync::atomic::Ordering;

use crate::process::{Pid, ProcessState, MAX_PROCESSES};
use crate::{kdebug, ktrace};

use super::context::context_switch;
use super::priority::{
    calc_vdeadline, is_eligible, ms_to_ns, 
    replenish_slice, update_curr, update_min_vruntime,
};
use super::table::{
    current_pid, set_current_pid, CURRENT_PID, GLOBAL_TICK, PROCESS_TABLE, SCHED_STATS,
};
use super::types::{SchedPolicy, DEFAULT_TIME_SLICE, BASE_SLICE_NS};

/// Initialize scheduler subsystem
pub fn init() {
    crate::kinfo!(
        "EEVDF scheduler initialized ({} max processes, {}ms base slice)",
        MAX_PROCESSES,
        BASE_SLICE_NS / 1_000_000
    );
    crate::kinfo!(
        "Scheduling: Earliest Eligible Virtual Deadline First with lag-based fairness"
    );
}

/// EEVDF candidate info: (index, vdeadline, policy, is_eligible, priority)
type EevdfCandidate = (usize, u64, SchedPolicy, bool, u8);

/// Compare two candidate processes using EEVDF rules.
/// Returns true if `candidate` should replace `best`.
/// 
/// EEVDF selection rules:
/// 1. Realtime processes always beat non-realtime
/// 2. Among realtime: lower priority number wins
/// 3. Among non-realtime eligible: earliest deadline wins
/// 4. Non-eligible processes are only chosen if no eligible ones exist
#[inline]
fn should_replace_candidate(candidate: EevdfCandidate, best: EevdfCandidate) -> bool {
    let (_, c_vdl, c_policy, c_eligible, c_pri) = candidate;
    let (_, b_vdl, b_policy, b_eligible, b_pri) = best;

    // Policy class comparison
    match (c_policy, b_policy) {
        (SchedPolicy::Realtime, SchedPolicy::Realtime) => {
            // Among realtime: lower priority wins
            c_pri < b_pri
        }
        (SchedPolicy::Realtime, _) => true,  // Realtime beats all
        (_, SchedPolicy::Realtime) => false, // Non-realtime loses to realtime
        (SchedPolicy::Idle, SchedPolicy::Idle) => {
            // Idle: use deadline
            c_vdl < b_vdl
        }
        (_, SchedPolicy::Idle) => true,  // Non-idle beats idle
        (SchedPolicy::Idle, _) => false, // Idle loses to non-idle
        _ => {
            // EEVDF: eligible processes with earlier deadline win
            match (c_eligible, b_eligible) {
                (true, false) => true,   // Eligible beats non-eligible
                (false, true) => false,  // Non-eligible loses
                _ => c_vdl < b_vdl,      // Both same eligibility: earliest deadline wins
            }
        }
    }
}

/// Update EEVDF state for ready processes (lag accumulation for waiting)
fn update_ready_process_eevdf(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    current_tick: u64,
) {
    // Calculate total weight of runnable processes
    let total_weight: u64 = table.iter()
        .filter_map(|s| s.as_ref())
        .filter(|e| e.process.state == ProcessState::Ready || e.process.state == ProcessState::Running)
        .map(|e| e.weight)
        .sum();

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        let wait_delta = current_tick.saturating_sub(entry.last_scheduled);
        entry.wait_time = entry.wait_time.saturating_add(wait_delta);
        
        // Increase lag for waiting processes (they deserve more CPU)
        if wait_delta > 0 && total_weight > 0 {
            let lag_credit = (ms_to_ns(wait_delta) as i64 * entry.weight as i64) / total_weight as i64;
            entry.lag = entry.lag.saturating_add(lag_credit);
        }
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

/// Find the best ready process using EEVDF algorithm
/// Selects the eligible process with the earliest virtual deadline
/// Only considers processes that can run on the current CPU (affinity check)
fn find_best_candidate(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    _start_idx: usize,
) -> Option<EevdfCandidate> {
    let mut best_candidate: Option<EevdfCandidate> = None;
    
    // Get current CPU ID for affinity check (supports up to 1024 CPUs)
    let current_cpu = crate::smp::current_cpu_id() as usize;

    for (idx, slot) in table.iter().enumerate() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }
        
        // Check CPU affinity - skip if process cannot run on this CPU
        if !entry.cpu_affinity.is_set(current_cpu) {
            continue;
        }

        let eligible = is_eligible(entry);
        let candidate: EevdfCandidate = (
            idx,
            entry.vdeadline,
            entry.policy,
            eligible,
            entry.priority,
        );
        
        let should_use = best_candidate
            .map(|best| should_replace_candidate(candidate, best))
            .unwrap_or(true);

        if should_use {
            best_candidate = Some(candidate);
        }
    }

    best_candidate
}

/// EEVDF Scheduler: select the eligible process with earliest virtual deadline
/// Provides fair CPU time distribution with latency guarantees
pub fn schedule() -> Option<Pid> {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    update_ready_process_eevdf(&mut table, current_tick);
    let start_idx = find_start_index(&table, current);
    let best_candidate = find_best_candidate(&table, start_idx);

    let Some((next_idx, _, _, _, _)) = best_candidate else {
        return None;
    };

    let next_pid = table[next_idx].as_ref().unwrap().process.pid;

    // Update previous process state
    if let Some(curr_pid) = current {
        update_previous_process_state(&mut table, curr_pid, current_tick);
    }

    // Update next process state (EEVDF)
    if let Some(entry) = table[next_idx].as_mut() {
        // Replenish slice if exhausted
        if entry.slice_remaining_ns == 0 {
            replenish_slice(entry);
        }
        
        entry.process.state = ProcessState::Running;
        entry.last_scheduled = current_tick;
        entry.wait_time = 0;
        entry.cpu_burst_count += 1;
        
        // Reset lag when scheduled (consumed their fair share of waiting)
        entry.lag = 0;
        
        // Recalculate deadline based on current vruntime
        entry.vdeadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
    }

    drop(table);
    *CURRENT_PID.lock() = Some(next_pid);
    
    // Update global min_vruntime
    update_min_vruntime();
    
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

/// Check if a ready process should preempt the current running process (EEVDF)
#[inline]
fn should_preempt_for_eevdf(
    ready_entry: &super::types::ProcessEntry,
    curr_entry: &super::types::ProcessEntry,
) -> bool {
    // Realtime always preempts non-realtime
    if ready_entry.policy == SchedPolicy::Realtime && curr_entry.policy != SchedPolicy::Realtime {
        return true;
    }
    if curr_entry.policy == SchedPolicy::Realtime {
        return false; // Can't preempt realtime unless also realtime
    }
    
    // Non-idle beats idle
    if ready_entry.policy != SchedPolicy::Idle && curr_entry.policy == SchedPolicy::Idle {
        return true;
    }
    
    // EEVDF: eligible process with significantly earlier deadline preempts
    if is_eligible(ready_entry) {
        let deadline_diff = curr_entry.vdeadline.saturating_sub(ready_entry.vdeadline);
        // Only preempt if deadline difference is significant (avoid thrashing)
        return deadline_diff > super::types::SCHED_GRANULARITY_NS;
    }
    
    false
}

/// Check if any ready process should preempt the current one (EEVDF)
fn should_preempt_current(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_entry: &super::types::ProcessEntry,
) -> bool {
    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }
        if should_preempt_for_eevdf(entry, curr_entry) {
            return true;
        }
    }
    false
}

/// Handle preemption in EEVDF: just increment counter
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
        crate::kdebug!(
            "EEVDF: PID {} preempted (vrt={}, vdl={}, lag={})",
            curr_pid, entry.vruntime, entry.vdeadline, entry.lag
        );
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

/// Timer tick handler: EEVDF scheduler tick
/// Updates vruntime and checks for preemption based on virtual deadlines
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

    // Convert elapsed_ms to nanoseconds for EEVDF calculations
    let elapsed_ns = ms_to_ns(elapsed_ms);
    
    // Update EEVDF state
    update_curr(entry, elapsed_ns);

    // Update legacy total_time (already done in update_curr via ns_to_ms)
    // Update average CPU burst
    let new_burst = entry.total_time / entry.cpu_burst_count.max(1);
    entry.avg_cpu_burst = (entry.avg_cpu_burst + new_burst) / 2;

    // Time slice exhausted - need to reschedule
    if entry.slice_remaining_ns == 0 {
        crate::kdebug!(
            "EEVDF: PID {} slice exhausted (vrt={}, vdl={})",
            curr_pid, entry.vruntime, entry.vdeadline
        );
        return true;
    }

    // Save current entry info for preemption check
    let curr_entry_copy = *entry;
    
    // Check for preemption by eligible process with earlier deadline
    if should_preempt_current(&table, &curr_entry_copy) {
        handle_preemption(&mut table, curr_pid);
        return true;
    }

    false
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
/// Only considers processes that can run on the current CPU (affinity check)
fn find_next_ready_index(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    start_idx: usize,
) -> Option<usize> {
    // Get current CPU ID for affinity check (supports up to 1024 CPUs)
    let current_cpu = crate::smp::current_cpu_id() as usize;

    for offset in 0..MAX_PROCESSES {
        let idx = (start_idx + offset) % MAX_PROCESSES;
        let Some(entry) = &table[idx] else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }
        // Check CPU affinity - skip if process cannot run on this CPU
        if !entry.cpu_affinity.is_set(current_cpu) {
            continue;
        }
        return Some(idx);
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
    
    // Update last_cpu to record which CPU is running this process
    entry.last_cpu = crate::smp::current_cpu_id() as u16;

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
