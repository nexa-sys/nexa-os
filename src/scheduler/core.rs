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
    calc_vdeadline, is_eligible, ms_to_ns, replenish_slice, update_curr, update_min_vruntime,
};
use super::table::{
    current_pid, set_current_pid, CURRENT_PID, GLOBAL_TICK, PROCESS_TABLE, SCHED_STATS,
};
use super::types::{SchedPolicy, BASE_SLICE_NS, DEFAULT_TIME_SLICE};

/// Initialize scheduler subsystem
pub fn init() {
    crate::kinfo!(
        "EEVDF scheduler initialized ({} max processes, {}ms base slice)",
        MAX_PROCESSES,
        BASE_SLICE_NS / 1_000_000
    );
    crate::kinfo!("Scheduling: Earliest Eligible Virtual Deadline First with lag-based fairness");
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
                (true, false) => true,  // Eligible beats non-eligible
                (false, true) => false, // Non-eligible loses
                _ => c_vdl < b_vdl,     // Both same eligibility: earliest deadline wins
            }
        }
    }
}

/// Update EEVDF state for ready processes (lag accumulation for waiting)
/// Performance optimized with early exits and bounded lag values
fn update_ready_process_eevdf(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    current_tick: u64,
) {
    // Calculate total weight of runnable processes (cached for efficiency)
    let mut total_weight: u64 = 0;
    let mut ready_count = 0u32;

    for slot in table.iter() {
        let Some(entry) = slot else { continue };
        match entry.process.state {
            ProcessState::Ready | ProcessState::Running => {
                total_weight += entry.weight;
                ready_count += 1;
            }
            _ => continue,
        }
    }

    // Fast path: no ready processes or single process (no competition)
    if ready_count <= 1 || total_weight == 0 {
        return;
    }

    // Maximum lag credit to prevent unbounded growth (100ms)
    const MAX_LAG: i64 = 100_000_000;

    for slot in table.iter_mut() {
        let Some(entry) = slot else { continue };
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        let wait_delta = current_tick.saturating_sub(entry.last_scheduled);

        // Fast path: no wait time, no update needed
        if wait_delta == 0 {
            continue;
        }

        entry.wait_time = entry.wait_time.saturating_add(wait_delta);

        // Increase lag for waiting processes (they deserve more CPU)
        // Use saturating arithmetic and cap at MAX_LAG
        let lag_credit = (ms_to_ns(wait_delta) as i64 * entry.weight as i64) / total_weight as i64;
        let new_lag = entry.lag.saturating_add(lag_credit).min(MAX_LAG);
        entry.lag = new_lag;
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
///
/// Performance optimizations:
/// - Early exit for realtime processes (highest priority)
/// - Skip zombie/sleeping processes without full comparison
/// - Cache eligibility result to avoid recomputation
fn find_best_candidate(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    _start_idx: usize,
) -> Option<EevdfCandidate> {
    let mut best_candidate: Option<EevdfCandidate> = None;
    let mut found_realtime = false;

    // Get current CPU ID for affinity check (supports up to 1024 CPUs)
    let current_cpu = crate::smp::current_cpu_id() as usize;

    for (idx, slot) in table.iter().enumerate() {
        let Some(entry) = slot else { continue };

        // Fast path: skip non-ready processes
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        // Check CPU affinity - skip if process cannot run on this CPU
        if !entry.cpu_affinity.is_set(current_cpu) {
            continue;
        }

        // Fast path for realtime: if we found one, only compare with other realtime
        if found_realtime && entry.policy != SchedPolicy::Realtime {
            continue;
        }

        let eligible = is_eligible(entry);
        let candidate: EevdfCandidate =
            (idx, entry.vdeadline, entry.policy, eligible, entry.priority);

        // Handle realtime early exit
        if entry.policy == SchedPolicy::Realtime {
            if !found_realtime {
                // First realtime process found, it automatically wins over any non-RT
                best_candidate = Some(candidate);
                found_realtime = true;
            } else {
                // Compare with other realtime (lower priority value = higher priority)
                if let Some((_, _, _, _, best_pri)) = best_candidate {
                    if entry.priority < best_pri {
                        best_candidate = Some(candidate);
                    }
                }
            }
            continue;
        }

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
///
/// Performance optimizations:
/// - Early exits for common cases (realtime, idle)
/// - Threshold-based preemption to avoid thrashing
/// - Consider both eligibility and deadline difference
#[inline]
fn should_preempt_for_eevdf(
    ready_entry: &super::types::ProcessEntry,
    curr_entry: &super::types::ProcessEntry,
) -> bool {
    // Fast path 1: Realtime always preempts non-realtime
    match (ready_entry.policy, curr_entry.policy) {
        (SchedPolicy::Realtime, SchedPolicy::Realtime) => {
            // Among realtime: lower priority number wins
            return ready_entry.priority < curr_entry.priority;
        }
        (SchedPolicy::Realtime, _) => return true,
        (_, SchedPolicy::Realtime) => return false,
        _ => {}
    }

    // Fast path 2: Non-idle beats idle
    if curr_entry.policy == SchedPolicy::Idle && ready_entry.policy != SchedPolicy::Idle {
        return true;
    }

    // Fast path 3: Idle processes don't preempt normal ones
    if ready_entry.policy == SchedPolicy::Idle {
        return false;
    }

    // EEVDF: eligible process with significantly earlier deadline preempts
    if !is_eligible(ready_entry) {
        return false;
    }

    // Calculate deadline difference
    let deadline_diff = curr_entry.vdeadline.saturating_sub(ready_entry.vdeadline);

    // Preemption threshold: only preempt if significant improvement
    // This prevents excessive context switches
    // Use larger threshold for batch processes (they prefer longer runs)
    let threshold = match curr_entry.policy {
        SchedPolicy::Batch => super::types::SCHED_GRANULARITY_NS * 2,
        _ => super::types::SCHED_GRANULARITY_NS,
    };

    deadline_diff > threshold
}

/// Check if any ready process should preempt the current one (EEVDF)
/// Optimized with early exits for common cases
fn should_preempt_current(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_entry: &super::types::ProcessEntry,
) -> bool {
    // Fast path: Realtime processes are only preempted by higher priority realtime
    let is_curr_realtime = curr_entry.policy == SchedPolicy::Realtime;
    let current_cpu = crate::smp::current_cpu_id() as usize;

    for slot in table.iter() {
        let Some(entry) = slot else { continue };

        // Fast skip: not ready
        if entry.process.state != ProcessState::Ready {
            continue;
        }

        // Skip processes that can't run on this CPU
        if !entry.cpu_affinity.is_set(current_cpu) {
            continue;
        }

        // Fast path: skip non-RT if current is RT
        if is_curr_realtime && entry.policy != SchedPolicy::Realtime {
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
            curr_pid,
            entry.vruntime,
            entry.vdeadline,
            entry.lag
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
            curr_pid,
            entry.vruntime,
            entry.vdeadline
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
    FirstRun {
        process: crate::process::Process,
        old_context_ptr: *mut crate::process::Context,
        next_cr3: u64,
        kernel_stack: u64,
        fs_base: u64,
    },
    Switch {
        old_context_ptr: *mut crate::process::Context,
        next_context: crate::process::Context,
        next_cr3: u64,
        user_rip: u64,
        user_rsp: u64,
        user_rflags: u64,
        user_r10: u64,
        user_r8: u64,
        user_r9: u64,
        is_voluntary: bool,
        kernel_stack: u64,
        fs_base: u64,
    },
}

/// Find index of parent process to prioritize when child is zombie
fn find_zombie_parent_index(
    table: &[Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Pid,
) -> Option<usize> {
    // Find current process entry
    let curr_entry = table
        .iter()
        .find_map(|slot| slot.as_ref().filter(|e| e.process.pid == curr_pid))?;

    // Only proceed if current process is zombie
    if curr_entry.process.state != ProcessState::Zombie {
        return None;
    }

    let parent_pid = curr_entry.process.ppid;
    if parent_pid == 0 {
        return None;
    }

    // Find ready parent
    table
        .iter()
        .position(|slot| {
            slot.as_ref().map_or(false, |e| {
                e.process.pid == parent_pid && e.process.state == ProcessState::Ready
            })
        })
        .map(|idx| {
            kdebug!(
                "[do_schedule] Child PID {} is Zombie, prioritizing parent PID {}",
                curr_pid,
                parent_pid
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
/// This saves the user-mode registers that were stored in GS_DATA when the process
/// entered the kernel via syscall. This includes rip, rsp, rflags, and the syscall
/// argument registers r10, r8, r9 which are restored on syscall return.
unsafe fn save_syscall_context_to_entry(entry: &mut super::types::ProcessEntry, curr_pid: Pid) {
    let gs_data_ptr = crate::smp::current_gs_data_ptr() as *const u64;
    let saved_rip = gs_data_ptr.add(crate::interrupts::GS_SLOT_SAVED_RCX).read();
    let saved_rsp = gs_data_ptr.add(crate::interrupts::GS_SLOT_USER_RSP).read();
    let saved_rflags = gs_data_ptr
        .add(crate::interrupts::GS_SLOT_SAVED_RFLAGS)
        .read();
    // CRITICAL: Also save syscall argument registers r10, r8, r9
    // These are stored in GS_DATA slots 4, 5, 6 (offsets 32, 40, 48) by syscall entry
    // and are restored on syscall return. Without saving these, context switches
    // during syscalls (e.g., wait4 -> do_schedule) would corrupt these registers.
    let saved_r10 = gs_data_ptr.add(4).read(); // GS[4] = r10 (syscall arg4)
    let saved_r8 = gs_data_ptr.add(5).read();  // GS[5] = r8 (syscall arg5)
    let saved_r9 = gs_data_ptr.add(6).read();  // GS[6] = r9 (syscall arg6)

    ktrace!(
        "[do_schedule] Saving syscall context for PID {}: rip={:#x}, rsp={:#x}, rflags={:#x}",
        curr_pid,
        saved_rip,
        saved_rsp,
        saved_rflags
    );

    entry.process.user_rip = saved_rip;
    entry.process.user_rsp = saved_rsp;
    entry.process.user_rflags = saved_rflags;
    entry.process.user_r10 = saved_r10;
    entry.process.user_r8 = saved_r8;
    entry.process.user_r9 = saved_r9;
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

        // Save syscall context from GS_DATA to process entry.
        // For timer interrupts, the handler has already saved the user-mode context
        // to GS_DATA, so we can read it the same way as for syscalls.
        // Only save if process has entered user mode (has valid context to save).
        if entry.process.has_entered_user {
            unsafe { save_syscall_context_to_entry(entry, curr_pid) };
        }

        entry.process.state = ProcessState::Ready;
        break;
    }
}

/// Extract process info for context switch
fn extract_next_process_info(
    entry: &mut super::types::ProcessEntry,
) -> (
    bool,
    Pid,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    crate::process::Context,
    u64,
    u64,
    crate::process::Process,
) {
    entry.time_slice = DEFAULT_TIME_SLICE;
    entry.process.state = ProcessState::Running;

    // Update last_cpu to record which CPU is running this process
    entry.last_cpu = crate::smp::current_cpu_id() as u16;

    // first_run = true if context was never saved by context_switch
    // This means we need to use FirstRun path (trampoline) instead of Switch path
    (
        !entry.process.context_valid,
        entry.process.pid,
        entry.process.cr3,
        entry.process.user_rip,
        entry.process.user_rsp,
        entry.process.user_rflags,
        entry.process.user_r10,
        entry.process.user_r8,
        entry.process.user_r9,
        entry.process.context,
        entry.process.kernel_stack,
        entry.process.fs_base,
        entry.process, // Process is Copy
    )
}

/// Get old context pointer and voluntary flag for current process
fn get_old_context_info(
    table: &mut [Option<super::types::ProcessEntry>; MAX_PROCESSES],
    curr_pid: Option<Pid>,
) -> (Option<*mut crate::process::Context>, bool) {
    let Some(pid) = curr_pid else {
        // crate::serial_println!("[OLD_CTX] curr_pid is None");
        return (None, false);
    };

    for slot in table.iter_mut() {
        let Some(candidate) = slot else { continue };
        if candidate.process.pid != pid {
            continue;
        }

        let voluntary =
            candidate.process.state == ProcessState::Sleeping || candidate.time_slice > 0;
        if voluntary {
            candidate.voluntary_switches += 1;
        }

        // Don't save context for zombie processes
        if candidate.process.state == ProcessState::Zombie {
            // crate::serial_println!("[OLD_CTX] PID {} is Zombie, not saving", pid);
            return (None, voluntary);
        }

        // Don't save context for processes that haven't entered userspace yet
        // Their context structure contains invalid data
        if !candidate.process.has_entered_user {
            // crate::serial_println!("[OLD_CTX] PID {} has_entered_user=false, not saving", pid);
            return (None, voluntary);
        }

        // Save current FS base from MSR to Process struct
        // This is critical for TLS preservation across context switches
        let current_fs_base = unsafe {
            use x86_64::registers::model_specific::Msr;
            Msr::new(crate::safety::x86::MSR_IA32_FS_BASE).read()
        };
        if current_fs_base != 0 {
            candidate.process.fs_base = current_fs_base;
        }

        // Mark context as valid since we're about to save to it via context_switch
        candidate.process.context_valid = true;

        return (Some(&mut candidate.process.context as *mut _), voluntary);
    }

    // crate::serial_println!("[OLD_CTX] PID {} not found in table!", pid);
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

/// Execute first-run process (unused, kept for reference)
#[allow(dead_code)]
fn execute_first_run(mut process: crate::process::Process) {
    crate::serial_println!(
        "FIRST_RUN: PID={} entry={:#x} stack={:#x} cr3={:#x}",
        process.pid,
        process.entry_point,
        process.stack_top,
        process.cr3
    );

    if process.cr3 == 0 {
        crate::kfatal!(
            "PANIC: FirstRun for PID {} has CR3=0! Entry={:#x}, Stack={:#x}, MemBase={:#x}",
            process.pid,
            process.entry_point,
            process.stack_top,
            process.memory_base
        );
    }

    mark_process_entered_user(process.pid);
    crate::serial_println!("FIRST_RUN: About to call process.execute()");
    process.execute();
    crate::kfatal!("process::execute returned unexpectedly");
}

/// Global storage for first-run process info (used by trampoline)
/// This is safe because we only use it with interrupts disabled during context switch
static mut FIRST_RUN_PROCESS: Option<crate::process::Process> = None;
/// Global storage for the new process's CR3 (used by trampoline to switch address space)
static mut FIRST_RUN_CR3: u64 = 0;

/// Trampoline function called after context switch to first-run process
/// This function never returns - it jumps to userspace via process.execute()
#[no_mangle]
extern "C" fn first_run_trampoline() -> ! {
    // Switch to the new process's address space first
    let cr3 = unsafe { FIRST_RUN_CR3 };
    if cr3 != 0 {
        crate::paging::activate_address_space(cr3);
    }
    
    // Get the process from global storage
    let mut process = unsafe {
        FIRST_RUN_PROCESS.take().expect("first_run_trampoline called without process")
    };

    if process.cr3 == 0 {
        crate::kpanic!(
            "FirstRun PID {} has CR3=0",
            process.pid
        );
    }

    mark_process_entered_user(process.pid);
    process.execute();
    // process.execute() never returns, but compiler doesn't know
    unreachable!("process.execute() returned");
}

/// Execute first-run process via context_switch
/// This properly saves the old process context before switching
unsafe fn execute_first_run_via_context_switch(
    process: crate::process::Process,
    old_context_ptr: *mut crate::process::Context,
    next_cr3: u64,
    kernel_stack: u64,
    fs_base: u64,
) {
    // crate::serial_println!("[FRVCS] PID={} kstack={:#x}", process.pid, kernel_stack);
    
    // Store process and CR3 in globals for the trampoline to pick up
    FIRST_RUN_PROCESS = Some(process);
    FIRST_RUN_CR3 = next_cr3;

    // Update kernel stack in GS (per-CPU GS_DATA)
    if kernel_stack != 0 {
        let gs_data_ptr = crate::smp::current_gs_data_ptr();
        let new_kstack_top = kernel_stack + crate::process::KERNEL_STACK_SIZE as u64;
        gs_data_ptr
            .add(crate::interrupts::GS_SLOT_KERNEL_RSP)
            .write(new_kstack_top);
        
        // CRITICAL: Also update TSS RSP0 for int 0x81 syscalls
        let cpu_id = crate::smp::current_cpu_id() as usize;
        crate::arch::gdt::update_tss_rsp0(cpu_id, new_kstack_top);
    }

    // Restore FS base for TLS if set
    if fs_base != 0 {
        use x86_64::registers::model_specific::Msr;
        Msr::new(crate::safety::x86::MSR_IA32_FS_BASE).write(fs_base);
    }

    // Create a context for the new process that will call first_run_trampoline
    let mut new_context = crate::process::Context::zero();
    new_context.rip = first_run_trampoline as usize as u64;
    // Stack: -16 for 16-byte alignment (RSP % 16 == 8 after push rip; ret)
    new_context.rsp = kernel_stack + crate::process::KERNEL_STACK_SIZE as u64 - 16;
    new_context.rflags = 0x202; // IF=1

    // Context switch: saves old process context and jumps to trampoline
    // Print old_context_ptr address before switch
    // DEBUG: if !old_context_ptr.is_null() {
    //     crate::serial_println!("[FRV] old_ctx_ptr={:#x}, pre rip={:#x}",
    //         old_context_ptr as u64, (*old_context_ptr).rip);
    // }
    context_switch(old_context_ptr, &new_context as *const _);
    
    // Reached when this process is restored
    // Print debug: what RIP was restored?
    let _restored_rip: u64;
    core::arch::asm!("lea {}, [rip]", out(reg) _restored_rip, options(nomem, nostack));
    // crate::serial_println!("[FRV] RESTORED: rip~{:#x}", restored_rip);
    
    // Restore our CR3
    if let Some(pid) = *CURRENT_PID.lock() {
        let table = PROCESS_TABLE.lock();
        for slot in table.iter() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    // crate::serial_println!("[FRV] Restoring CR3 for PID {}: {:#x}", pid, entry.process.cr3);
                    crate::paging::activate_address_space(entry.process.cr3);
                    break;
                }
            }
        }
    }
    // crate::serial_println!("[FRV] About to return from execute_first_run_via_context_switch");
}

/// Trampoline for Switch path - returns to userspace via sysretq
#[unsafe(naked)]
unsafe extern "C" fn switch_return_trampoline() {
    core::arch::naked_asm!(
        // r12 = user_rip
        // r13 = user_rsp
        // r14 = user_rflags
        // r15 = cr3
        // rbx = user_r10
        // rbp = user_r8
        // [rsp] = user_r9 (on stack)
        
        // Pop user_r9 from stack into a temporary location (use rax, will be clobbered anyway)
        "pop rax",  // rax = user_r9
        
        // Ensure GS base is correct (using callee-saved registers to preserve context)
        // Save r8/r9 values before the call since they may be clobbered
        "push rax",  // save user_r9
        "push rbx",  // save user_r10
        "push rbp",  // save user_r8
        "call {ensure_kernel_gs_base}",
        "pop rbp",   // restore user_r8
        "pop rbx",   // restore user_r10
        "pop rax",   // restore user_r9
        
        // Activate address space (r15 = cr3, preserved across call)
        "push rax",
        "push rbx",
        "push rbp",
        "mov rdi, r15", // cr3
        "call {activate_address_space}",
        "pop rbp",
        "pop rbx",
        "pop rax",
        
        // Restore user syscall context with full register set
        // rdi = rip, rsi = rsp, rdx = rflags, rcx = r10, r8 = r8, r9 = r9
        "mov rdi, r12",  // rip
        "mov rsi, r13",  // rsp
        "mov rdx, r14",  // rflags
        "mov rcx, rbx",  // r10 (from rbx)
        "mov r8, rbp",   // r8 (from rbp)
        "mov r9, rax",   // r9 (from rax)
        "call {restore_user_syscall_context_full}",
        
        // sysretq to return to userspace
        "cli",
        // Clear kernel stack guard flags before returning
        "xor rax, rax",
        "mov gs:[160], rax",  // GS_SLOT_KERNEL_STACK_GUARD * 8
        "mov gs:[168], rax",  // GS_SLOT_KERNEL_STACK_SNAPSHOT * 8
        // Load sysretq parameters from GS_DATA
        "mov rcx, gs:[0x38]",  // GS_SLOT_SAVED_RCX = 7, * 8 = 0x38 -> user RIP
        "mov r11, gs:[0x40]",  // GS_SLOT_SAVED_RFLAGS = 8, * 8 = 0x40 -> user RFLAGS
        "mov rsp, gs:[0x00]",  // GS_SLOT_USER_RSP = 0, * 8 = 0x00 -> user RSP
        "sysretq",
        
        ensure_kernel_gs_base = sym crate::smp::ensure_kernel_gs_base,
        activate_address_space = sym crate::mm::paging::activate_address_space,
        restore_user_syscall_context_full = sym crate::interrupts::restore_user_syscall_context_full,
    );
}

/// Execute context switch to next process
/// 
/// There are two cases:
/// 1. Process was switched out while in kernel (context_valid=true, has valid saved context)
///    -> Restore the saved kernel context directly, let it continue where it left off
/// 2. Process needs to return to userspace (preempted from userspace or first syscall return)
///    -> Use trampoline to sysretq back to userspace
unsafe fn execute_context_switch(
    old_context_ptr: *mut crate::process::Context,
    next_context: &crate::process::Context,
    next_cr3: u64,
    user_rip: u64,
    user_rsp: u64,
    user_rflags: u64,
    user_r10: u64,
    user_r8: u64,
    user_r9: u64,
    _is_voluntary: bool,
    kernel_stack: u64,
    fs_base: u64,
) {
    // CRITICAL: Disable interrupts during context switch setup.
    // Timer interrupts could otherwise re-enter the scheduler and corrupt state.
    x86_64::instructions::interrupts::disable();
    
    // CRITICAL: Ensure GS base is correct FIRST before any GS_DATA operations.
    crate::smp::ensure_kernel_gs_base();
    
    // Update kernel stack in GS (per-CPU GS_DATA)
    if kernel_stack != 0 {
        let gs_data_ptr = crate::smp::current_gs_data_ptr();
        let write_addr = gs_data_ptr.add(crate::interrupts::GS_SLOT_KERNEL_RSP);
        let new_kstack_top = kernel_stack + crate::process::KERNEL_STACK_SIZE as u64;
        write_addr.write(new_kstack_top);
        
        // CRITICAL: Also update TSS RSP0 so that int 0x81 syscalls and
        // hardware interrupts from Ring 3 use this process's kernel stack.
        // Without this, all processes would share the CPU's static kernel stack,
        // causing context corruption when processes are preempted.
        let cpu_id = crate::smp::current_cpu_id() as usize;
        crate::arch::gdt::update_tss_rsp0(cpu_id, new_kstack_top);
    }
    
    // Restore FS base for TLS if set
    if fs_base != 0 {
        use x86_64::registers::model_specific::Msr;
        Msr::new(crate::safety::x86::MSR_IA32_FS_BASE).write(fs_base);
    }

    // Check if next_context has a valid kernel RIP (was switched out while in kernel)
    // If the saved context's RIP is in kernel space, restore it directly.
    // This happens when a process called do_schedule() voluntarily from kernel code.
    let next_rip = next_context.rip;
    let is_kernel_context = next_rip != 0 && next_rip < 0x400000; // Kernel is below 4MB
    
    if is_kernel_context {
        // Process was in kernel (e.g., in wait4 loop calling do_schedule)
        // Restore its kernel context directly - it will continue from context_switch return
        // First activate the process's address space
        crate::mm::paging::activate_address_space(next_cr3);
        
        // Restore user syscall context to GS_DATA so when the process eventually
        // returns to userspace via sysretq, it has the correct values
        // Use the full version to also restore r10, r8, r9 syscall argument registers
        crate::interrupts::restore_user_syscall_context_full(
            user_rip, user_rsp, user_rflags, user_r10, user_r8, user_r9
        );
        
        // Direct context switch to saved kernel context
        context_switch(old_context_ptr, next_context as *const _);
        
        // Reached when this process is restored - restore our CR3
        if let Some(pid) = *CURRENT_PID.lock() {
            let table = PROCESS_TABLE.lock();
            for slot in table.iter() {
                if let Some(entry) = slot {
                    if entry.process.pid == pid {
                        crate::mm::paging::activate_address_space(entry.process.cr3);
                        break;
                    }
                }
            }
        }
    } else {
        // Process needs to return to userspace via trampoline
        // Create a context that jumps to our return trampoline, using kernel stack
        let mut trampoline_context = crate::process::Context::zero();
        trampoline_context.rip = switch_return_trampoline as usize as u64;
        // Stack: -16 for 16-byte alignment
        trampoline_context.rsp = kernel_stack + crate::process::KERNEL_STACK_SIZE as u64 - 16;
        trampoline_context.rflags = 0x202; // IF=1
        
        // Pass user context via callee-saved registers to the trampoline
        // This avoids using global variables which are not SMP-safe and race-prone
        trampoline_context.r12 = user_rip;
        trampoline_context.r13 = user_rsp;
        trampoline_context.r14 = user_rflags;
        trampoline_context.r15 = next_cr3;
        // Pass syscall argument registers via additional callee-saved registers
        trampoline_context.rbx = user_r10;
        trampoline_context.rbp = user_r8;
        // We need another register for user_r9 - use the unused portion of rflags slot
        // Actually, let's store it on stack since we're using kernel stack
        // Simpler approach: push to stack before context switch, pop in trampoline
        // Even simpler: Use the stack directly since trampoline runs on kernel stack
        // Store user_r9 at a known stack offset
        let stack_top = kernel_stack + crate::process::KERNEL_STACK_SIZE as u64 - 16;
        let user_r9_slot = (stack_top - 8) as *mut u64;
        core::ptr::write_volatile(user_r9_slot, user_r9);
        trampoline_context.rsp = stack_top - 8; // Adjust stack to include user_r9
        
        context_switch(old_context_ptr, &trampoline_context as *const _);
        
        // Reached when this process is restored - restore our CR3
        if let Some(pid) = *CURRENT_PID.lock() {
            let table = PROCESS_TABLE.lock();
            for slot in table.iter() {
                if let Some(entry) = slot {
                    if entry.process.pid == pid {
                        crate::mm::paging::activate_address_space(entry.process.cr3);
                        break;
                    }
                }
            }
        }
    }
}

fn do_schedule_internal(from_interrupt: bool) {
    // TEMPORARILY DISABLED: crate::net::poll() may cause recursive scheduler issue
    // crate::net::poll();

    {
        let mut stats = SCHED_STATS.lock();
        stats.total_context_switches += 1;
    }

    let decision = compute_schedule_decision(from_interrupt);

    match decision {
        Some(ScheduleDecision::FirstRun { process, old_context_ptr, next_cr3, kernel_stack, fs_base }) => unsafe {
            // crate::serial_println!("[SCHED] Decision: FirstRun PID={}", process.pid);
            execute_first_run_via_context_switch(process, old_context_ptr, next_cr3, kernel_stack, fs_base);
        }
        Some(ScheduleDecision::Switch {
            old_context_ptr,
            next_context,
            next_cr3,
            user_rip,
            user_rsp,
            user_rflags,
            user_r10,
            user_r8,
            user_r9,
            is_voluntary,
            kernel_stack,
            fs_base,
        }) => unsafe {
            // crate::serial_println!("[SCHED] Decision: Switch to rip={:#x}", next_context.rip);
            execute_context_switch(
                old_context_ptr,
                &next_context,
                next_cr3,
                user_rip,
                user_rsp,
                user_rflags,
                user_r10,
                user_r8,
                user_r9,
                is_voluntary,
                kernel_stack,
                fs_base,
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
            table
                .iter()
                .position(|e| e.as_ref().map_or(false, |p| p.process.pid == pid))
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
    let _cpu_id = crate::smp::current_cpu_id();
    // DEBUG: crate::serial_println!("[SCHED_SEL] CPU{} Selected PID {} state={:?} ctx_valid={}", 
    //     cpu_id, entry.process.pid, entry.process.state, entry.process.context_valid);
    let (
        first_run,
        next_pid,
        next_cr3,
        user_rip,
        user_rsp,
        user_rflags,
        user_r10,
        user_r8,
        user_r9,
        next_context,
        kernel_stack,
        fs_base,
        process_copy,
    ) = extract_next_process_info(entry);

    *current_lock = Some(next_pid);

    // Always get old context info to save current process state
    let (old_context_opt, is_voluntary) = get_old_context_info(&mut table, current);
    let old_context_ptr = old_context_opt.unwrap_or(core::ptr::null_mut());

    if first_run {
        kdebug!(
            "[do_schedule] Creating FirstRun decision for PID {}, CR3={:#x}",
            next_pid,
            next_cr3
        );
        return Some(ScheduleDecision::FirstRun {
            process: process_copy,
            old_context_ptr,
            next_cr3,
            kernel_stack,
            fs_base,
        });
    }

    // DEBUG: crate::serial_println!(
    //     "[SWITCH_DBG] curr={:?} -> next={} ctx.rip={:#x} ctx.rsp={:#x}",
    //     current, next_pid, next_context.rip, next_context.rsp
    // );

    Some(ScheduleDecision::Switch {
        old_context_ptr,
        next_context,
        next_cr3,
        user_rip,
        user_rsp,
        user_rflags,
        user_r10,
        user_r8,
        user_r9,
        is_voluntary,
        kernel_stack,
        fs_base,
    })
}

