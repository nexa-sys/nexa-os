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

/// Round-robin scheduler: select next process to run with MLFQ enhancements
/// Uses multi-level feedback queue for better responsiveness and fairness
pub fn schedule() -> Option<Pid> {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);

    // Update wait times for all ready processes
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.state == ProcessState::Ready {
                let wait_delta = current_tick.saturating_sub(entry.last_scheduled);
                entry.wait_time = entry.wait_time.saturating_add(wait_delta);

                // Update dynamic priority based on wait time
                entry.priority = calculate_dynamic_priority(
                    entry.base_priority,
                    entry.wait_time,
                    entry.total_time,
                    entry.nice,
                );
            }
        }
    }

    // Find the current process index
    let mut start_idx = 0;
    if let Some(curr_pid) = current {
        for (idx, slot) in table.iter().enumerate() {
            if let Some(entry) = slot {
                if entry.process.pid == curr_pid {
                    start_idx = (idx + 1) % MAX_PROCESSES;
                    break;
                }
            }
        }
    }

    // Find next ready process using priority-based selection
    // Priority order: Realtime > Normal > Batch > Idle
    // Within same policy, select by dynamic priority and wait time
    let mut best_candidate: Option<(usize, u8, SchedPolicy, u64)> = None; // (index, priority, policy, wait_time)

    for offset in 0..MAX_PROCESSES {
        let idx = (start_idx + offset) % MAX_PROCESSES;
        if let Some(entry) = &table[idx] {
            if entry.process.state == ProcessState::Ready {
                let candidate = (idx, entry.priority, entry.policy, entry.wait_time);

                if let Some(best) = best_candidate {
                    // Compare candidates: higher policy priority wins,
                    // then lower priority value (0 is highest),
                    // then longer wait time
                    let should_replace = match (candidate.2, best.2) {
                        (SchedPolicy::Realtime, SchedPolicy::Realtime) => {
                            candidate.1 < best.1 || (candidate.1 == best.1 && candidate.3 > best.3)
                        }
                        (SchedPolicy::Realtime, _) => true,
                        (_, SchedPolicy::Realtime) => false,
                        (SchedPolicy::Normal, SchedPolicy::Normal) => {
                            candidate.1 < best.1 || (candidate.1 == best.1 && candidate.3 > best.3)
                        }
                        (SchedPolicy::Normal, _) => true,
                        (_, SchedPolicy::Normal) => false,
                        (SchedPolicy::Batch, SchedPolicy::Batch) => {
                            candidate.1 < best.1 || (candidate.1 == best.1 && candidate.3 > best.3)
                        }
                        (SchedPolicy::Batch, _) => true,
                        (_, SchedPolicy::Batch) => false,
                        (SchedPolicy::Idle, SchedPolicy::Idle) => {
                            candidate.1 < best.1 || (candidate.1 == best.1 && candidate.3 > best.3)
                        }
                    };

                    if should_replace {
                        best_candidate = Some(candidate);
                    }
                } else {
                    best_candidate = Some(candidate);
                }
            }
        }
    }

    if let Some((next_idx, _, _, _)) = best_candidate {
        let next_pid = table[next_idx].as_ref().unwrap().process.pid;

        // Update previous process state
        if let Some(curr_pid) = current {
            for slot in table.iter_mut() {
                if let Some(e) = slot {
                    if e.process.pid == curr_pid && e.process.state == ProcessState::Running {
                        e.process.state = ProcessState::Ready;
                        e.last_scheduled = current_tick;
                        break;
                    }
                }
            }
        }

        // Update next process state
        if let Some(entry) = table[next_idx].as_mut() {
            entry.time_slice = calculate_time_slice(entry.quantum_level);
            entry.process.state = ProcessState::Running;
            entry.last_scheduled = current_tick;
            entry.wait_time = 0; // Reset wait time when scheduled
            entry.cpu_burst_count += 1;
        }

        drop(table);
        *CURRENT_PID.lock() = Some(next_pid);
        return Some(next_pid);
    }

    None
}

/// Timer tick handler: update time slices and trigger scheduling
/// Implements preemptive scheduling with dynamic priority adjustments
pub fn tick(elapsed_ms: u64) -> bool {
    GLOBAL_TICK.fetch_add(1, Ordering::Relaxed);

    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();
    let mut should_preempt = false;

    if let Some(curr_pid) = current {
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == curr_pid && entry.process.state == ProcessState::Running {
                    entry.total_time += elapsed_ms;

                    if entry.time_slice > elapsed_ms {
                        entry.time_slice -= elapsed_ms;

                        // Update average CPU burst
                        let new_burst = entry.total_time / entry.cpu_burst_count.max(1);
                        entry.avg_cpu_burst = (entry.avg_cpu_burst + new_burst) / 2;

                        // Check if we should preempt based on priority changes
                        // or if a higher priority process is waiting
                        let current_priority = entry.priority;
                        let current_policy = entry.policy;

                        drop(table);
                        table = PROCESS_TABLE.lock();

                        // Check for higher priority ready processes
                        for check_slot in table.iter() {
                            if let Some(check_entry) = check_slot {
                                if check_entry.process.state == ProcessState::Ready {
                                    let should_preempt_for_this =
                                        match (check_entry.policy, current_policy) {
                                            (SchedPolicy::Realtime, SchedPolicy::Realtime) => {
                                                check_entry.priority < current_priority
                                            }
                                            (SchedPolicy::Realtime, _) => true,
                                            (_, SchedPolicy::Realtime) => false,
                                            (SchedPolicy::Normal, SchedPolicy::Normal) => {
                                                check_entry.priority + 10 < current_priority
                                                // Significant priority difference
                                            }
                                            (SchedPolicy::Normal, _) => true,
                                            (_, SchedPolicy::Normal) => false,
                                            _ => false,
                                        };

                                    if should_preempt_for_this {
                                        should_preempt = true;
                                        break;
                                    }
                                }
                            }
                        }

                        if !should_preempt {
                            return false; // Continue running current process
                        } else {
                            // Preemption due to higher priority process
                            for slot in table.iter_mut() {
                                if let Some(entry) = slot {
                                    if entry.process.pid == curr_pid {
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
                            }
                            return true;
                        }
                    } else {
                        entry.time_slice = 0;

                        // MLFQ: Move to lower priority level after exhausting time slice
                        if entry.quantum_level < 7 {
                            entry.quantum_level += 1;
                        }

                        // Update average CPU burst
                        let new_burst = entry.total_time / entry.cpu_burst_count.max(1);
                        entry.avg_cpu_burst = (entry.avg_cpu_burst + new_burst) / 2;

                        return true; // Time slice expired, need to reschedule
                    }
                }
            }
        }
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

fn do_schedule_internal(from_interrupt: bool) {
    // Poll network stack to process any incoming packets
    crate::net::poll();

    // Update scheduler statistics
    {
        let mut stats = SCHED_STATS.lock();
        stats.total_context_switches += 1;
    }

    // Debug: Print all process states before scheduling
    {
        let table = PROCESS_TABLE.lock();
        ktrace!("[do_schedule] Process table snapshot:");
        for slot in table.iter() {
            if let Some(entry) = slot {
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

    let decision: Option<ScheduleDecision> = {
        let mut table = PROCESS_TABLE.lock();
        let mut current_lock = CURRENT_PID.lock();
        let current = *current_lock;

        let start_idx = if let Some(pid) = current {
            table
                .iter()
                .position(|entry| entry.as_ref().map_or(false, |e| e.process.pid == pid))
                .map(|i| (i + 1) % MAX_PROCESSES)
                .unwrap_or(0)
        } else {
            0
        };

        let mut next_idx = None;

        // CRITICAL FIX: If current process is Zombie, prioritize its parent process
        // This ensures wait4() promptly detects child exit and doesn't cause
        // unnecessary scheduling of other processes (like init).
        //
        // When child exits (becomes Zombie) and calls do_schedule(), we want
        // the parent (which is likely blocked in wait4) to run next so it can
        // immediately detect the zombie child and reap it.
        if let Some(curr_pid) = current {
            if let Some(curr_entry) = table
                .iter()
                .find(|e| e.as_ref().map_or(false, |p| p.process.pid == curr_pid))
            {
                if let Some(entry) = curr_entry {
                    if entry.process.state == ProcessState::Zombie {
                        let parent_pid = entry.process.ppid;
                        if parent_pid > 0 {
                            // Try to find parent and schedule it if Ready
                            if let Some(parent_idx) = table.iter().position(|e| {
                                e.as_ref().map_or(false, |p| {
                                    p.process.pid == parent_pid
                                        && p.process.state == ProcessState::Ready
                                })
                            }) {
                                kdebug!(
                                    "[do_schedule] Child PID {} is Zombie, prioritizing parent PID {}",
                                    curr_pid, parent_pid
                                );
                                next_idx = Some(parent_idx);
                            }
                        }
                    }
                }
            }
        }

        // Fall back to normal round-robin if no parent priority
        if next_idx.is_none() {
            for offset in 0..MAX_PROCESSES {
                let idx = (start_idx + offset) % MAX_PROCESSES;
                if let Some(entry) = &table[idx] {
                    if entry.process.state == ProcessState::Ready {
                        next_idx = Some(idx);
                        break;
                    }
                }
            }
        }

        if let Some(next_idx) = next_idx {
            if let Some(curr_pid) = current {
                for slot in table.iter_mut() {
                    if let Some(entry) = slot {
                        if entry.process.pid == curr_pid
                            && entry.process.state == ProcessState::Running
                        {
                            // CRITICAL: Save syscall context from GS_DATA before yielding CPU
                            // When a process calls syscall (like wait4) and yields via do_schedule(),
                            // the syscall handler saved RIP/RSP/RFLAGS in GS_DATA.
                            // We must copy these values to the process struct so they can be
                            // restored when this process is scheduled again.
                            // Skip this in interrupt context as syscall context may not be valid
                            if !from_interrupt {
                                unsafe {
                                    let gs_data_ptr =
                                        core::ptr::addr_of!(crate::initramfs::GS_DATA.0)
                                            as *const u64;
                                    let saved_rip = gs_data_ptr
                                        .add(crate::interrupts::GS_SLOT_SAVED_RCX)
                                        .read();
                                    let saved_rsp =
                                        gs_data_ptr.add(crate::interrupts::GS_SLOT_USER_RSP).read();
                                    let saved_rflags = gs_data_ptr
                                        .add(crate::interrupts::GS_SLOT_SAVED_RFLAGS)
                                        .read();

                                    ktrace!(
                                        "[do_schedule] Saving syscall context for PID {}: rip={:#x}, rsp={:#x}, rflags={:#x}",
                                        curr_pid, saved_rip, saved_rsp, saved_rflags
                                    );

                                    entry.process.user_rip = saved_rip;
                                    entry.process.user_rsp = saved_rsp;
                                    entry.process.user_rflags = saved_rflags;
                                }
                            }

                            entry.process.state = ProcessState::Ready;
                            break;
                        }
                    }
                }
            }

            // Extract all needed info from the next process entry in a separate scope
            // to avoid holding a mutable borrow on `table` while we iterate it later.
            let (
                first_run,
                next_pid,
                next_cr3,
                user_rip,
                user_rsp,
                user_rflags,
                next_context,
                kernel_stack,
                process_copy,
            ) = {
                let entry = table[next_idx].as_mut().expect("Process entry vanished");
                entry.time_slice = DEFAULT_TIME_SLICE;
                entry.process.state = ProcessState::Running;

                let first_run = !entry.process.has_entered_user;
                let next_pid = entry.process.pid;
                let next_cr3 = entry.process.cr3;
                let user_rip = entry.process.user_rip;
                let user_rsp = entry.process.user_rsp;
                let user_rflags = entry.process.user_rflags;
                let next_context = entry.process.context;
                let kernel_stack = entry.process.kernel_stack;
                let process_copy = entry.process; // Process is Copy

                (
                    first_run,
                    next_pid,
                    next_cr3,
                    user_rip,
                    user_rsp,
                    user_rflags,
                    next_context,
                    kernel_stack,
                    process_copy,
                )
            };

            *current_lock = Some(next_pid);

            if first_run {
                // CRITICAL: Don't set has_entered_user here!
                // We return a COPY of the process, and execute() will set it on the copy.
                // We need to set it in the process table AFTER execute() completes.
                // But execute() never returns, so we can't do it there.
                // The solution: set it NOW in the process table, not on the copy.
                kdebug!(
                    "[do_schedule] Creating FirstRun decision for PID {}, CR3={:#x}",
                    next_pid,
                    next_cr3
                );
                Some(ScheduleDecision::FirstRun(process_copy))
            } else {
                // Check if current process is a zombie - if so, don't save its context
                let (old_context_ptr, is_voluntary) = if let Some(curr_pid) = current {
                    let result = table.iter_mut().find_map(|slot| {
                        slot.as_mut().and_then(|candidate| {
                            if candidate.process.pid == curr_pid {
                                // Check if this is a voluntary context switch
                                let voluntary = candidate.process.state == ProcessState::Sleeping ||
                                               candidate.time_slice > 0;

                                // Update voluntary switch counter
                                if voluntary {
                                    candidate.voluntary_switches += 1;
                                }

                                // Don't save context for zombie processes
                                if candidate.process.state == ProcessState::Zombie {
                                    kdebug!(
                                        "[do_schedule] Current PID {} is Zombie, not saving context",
                                        curr_pid
                                    );
                                    Some((None, voluntary))
                                } else {
                                    Some((Some(&mut candidate.process.context as *mut _), voluntary))
                                }
                            } else {
                                None
                            }
                        })
                    });
                    result.unwrap_or((None, false))
                } else {
                    (None, false)
                };

                Some(ScheduleDecision::Switch {
                    old_context_ptr: old_context_ptr.unwrap_or(core::ptr::null_mut()),
                    next_context,
                    next_cr3,
                    user_rip,
                    user_rsp,
                    user_rflags,
                    is_voluntary,
                    kernel_stack,
                })
            }
        } else {
            None
        }
    };

    match decision {
        Some(ScheduleDecision::FirstRun(mut process)) => {
            kdebug!(
                "[do_schedule] FirstRun: PID={}, entry={:#x}, stack={:#x}, has_entered_user={}, CR3={:#x}",
                process.pid, process.entry_point, process.stack_top, process.has_entered_user, process.cr3
            );

            // CRITICAL: Validate CR3 before activating address space
            if process.cr3 == 0 {
                crate::kfatal!(
                    "PANIC: FirstRun for PID {} has CR3=0! This should never happen. \
                     Entry={:#x}, Stack={:#x}, MemBase={:#x}",
                    process.pid,
                    process.entry_point,
                    process.stack_top,
                    process.memory_base
                );
            }

            // CRITICAL FIX: Mark the process as entered in the process table BEFORE execute()
            // because execute() never returns and we have a copy of the process here.
            let pid = process.pid;
            {
                let mut table = PROCESS_TABLE.lock();
                for slot in table.iter_mut() {
                    if let Some(entry) = slot {
                        if entry.process.pid == pid {
                            entry.process.has_entered_user = true;
                            break;
                        }
                    }
                }
            }

            // CRITICAL: Do NOT call activate_address_space here!
            // process.execute() will switch CR3 atomically with entering usermode
            // to avoid accessing kernel stack after address space switch.
            process.execute();
            crate::kfatal!("process::execute returned unexpectedly");
        }
        Some(ScheduleDecision::Switch {
            old_context_ptr,
            next_context,
            next_cr3,
            user_rip,
            user_rsp,
            user_rflags,
            is_voluntary,
            kernel_stack,
        }) => unsafe {
            // Update kernel stack in GS
            if kernel_stack != 0 {
                let gs_data_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *mut u64;
                // GS_SLOT_KERNEL_RSP is 1
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
                user_rip,
                user_rsp,
                user_rflags
            );
            if user_rsp != 0 {
                crate::interrupts::restore_user_syscall_context(user_rip, user_rsp, user_rflags);
            }
            crate::paging::activate_address_space(next_cr3);
            context_switch(old_context_ptr, &next_context as *const _);
        },
        None => {
            set_current_pid(None);
            crate::kwarn!("do_schedule(): No ready process found, returning to caller");
        }
    }
}
