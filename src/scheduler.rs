/// Basic round-robin process scheduler for hybrid kernel
use crate::process::{Pid, Process, ProcessState};
use spin::Mutex;

const MAX_PROCESSES: usize = 32;

/// Process control block with scheduling info
#[derive(Clone, Copy)]
pub struct ProcessEntry {
    pub process: Process,
    pub priority: u8,    // 0 = highest, 255 = lowest
    pub time_slice: u64, // Remaining time slice in ms
    pub total_time: u64, // Total CPU time used in ms
}

impl ProcessEntry {
    #[allow(dead_code)]
    const fn empty() -> Self {
        Self {
            process: Process {
                pid: 0,
                ppid: 0,
                state: ProcessState::Ready,
                entry_point: 0,
                stack_top: 0,
                heap_start: 0,
                heap_end: 0,
                signal_state: crate::signal::SignalState::new(),
                context: crate::process::Context::zero(),
                has_entered_user: false,
                cr3: 0,
                tty: 0,
                memory_base: 0,
                memory_size: 0,
                user_rip: 0,
                user_rsp: 0,
                user_rflags: 0,
            },
            priority: 128,
            time_slice: 0,
            total_time: 0,
        }
    }
}

/// Process table
static PROCESS_TABLE: Mutex<[Option<ProcessEntry>; MAX_PROCESSES]> =
    Mutex::new([None; MAX_PROCESSES]);

/// Currently running process PID
static CURRENT_PID: Mutex<Option<Pid>> = Mutex::new(None);

/// Default time slice in milliseconds
const DEFAULT_TIME_SLICE: u64 = 10;

/// Lock the process table for direct access (for syscall use)
pub fn process_table_lock() -> spin::MutexGuard<'static, [Option<ProcessEntry>; MAX_PROCESSES]> {
    PROCESS_TABLE.lock()
}

/// Update the saved user-mode return context for the currently running process.
pub fn update_current_user_context(user_rip: u64, user_rsp: u64, user_rflags: u64) {
    let current = current_pid();

    if let Some(pid) = current {
        let mut table = PROCESS_TABLE.lock();

        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.process.user_rip = user_rip;
                    entry.process.user_rsp = user_rsp;
                    entry.process.user_rflags = user_rflags;
                    break;
                }
            }
        }
    }
}

/// Add a process to the scheduler
pub fn add_process(process: Process, priority: u8) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if slot.is_none() {
            *slot = Some(ProcessEntry {
                process,
                priority,
                time_slice: DEFAULT_TIME_SLICE,
                total_time: 0,
            });
            crate::kinfo!(
                "Scheduler: Added process PID {} with priority {}",
                process.pid,
                priority
            );
            return Ok(());
        }
    }

    Err("Process table full")
}

/// Remove a process from the scheduler
pub fn remove_process(pid: Pid) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();
    let mut removed = false;

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                crate::kinfo!("Scheduler: Removed process PID {}", pid);
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
                entry.process.state = state;
                return Ok(());
            }
        }
    }

    Err("Process not found")
}

/// Get current running process PID
pub fn current_pid() -> Option<Pid> {
    *CURRENT_PID.lock()
}

/// Get the physical address of the page table currently active on the CPU.
/// When no process is running, this falls back to the kernel's page tables.
pub fn current_cr3() -> u64 {
    if let Some(pid) = current_pid() {
        if let Some(process) = get_process(pid) {
            if process.cr3 != 0 {
                return process.cr3;
            }
        }
    }

    crate::paging::kernel_pml4_phys()
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

/// Round-robin scheduler: select next process to run
pub fn schedule() -> Option<Pid> {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();

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

    // Round-robin: find next ready process
    for offset in 0..MAX_PROCESSES {
        let idx = (start_idx + offset) % MAX_PROCESSES;
        if let Some(entry) = &table[idx] {
            if entry.process.state == ProcessState::Ready {
                let next_pid = entry.process.pid;

                // Update previous process state
                if let Some(curr_pid) = current {
                    for slot in table.iter_mut() {
                        if let Some(e) = slot {
                            if e.process.pid == curr_pid && e.process.state == ProcessState::Running
                            {
                                e.process.state = ProcessState::Ready;
                            }
                        }
                    }
                }

                // Update next process state
                for slot in table.iter_mut() {
                    if let Some(e) = slot {
                        if e.process.pid == next_pid {
                            e.time_slice = DEFAULT_TIME_SLICE;
                            e.process.state = ProcessState::Running;
                            break;
                        }
                    }
                }

                drop(table);
                *CURRENT_PID.lock() = Some(next_pid);
                return Some(next_pid);
            }
        }
    }

    None
}

/// Timer tick handler: update time slices and trigger scheduling
pub fn tick(elapsed_ms: u64) -> bool {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();

    if let Some(curr_pid) = current {
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == curr_pid && entry.process.state == ProcessState::Running {
                    entry.total_time += elapsed_ms;

                    if entry.time_slice > elapsed_ms {
                        entry.time_slice -= elapsed_ms;
                        return false; // No need to reschedule
                    } else {
                        entry.time_slice = 0;
                        return true; // Time slice expired, need to reschedule
                    }
                }
            }
        }
    }

    false
}

/// Get process by PID
pub fn get_process(pid: Pid) -> Option<Process> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some(entry.process);
            }
        }
    }

    None
}

/// Query a specific child process state
/// Returns the child's state if found and is a child of parent_pid
pub fn get_child_state(parent_pid: Pid, child_pid: Pid) -> Option<ProcessState> {
    let table = PROCESS_TABLE.lock();

    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == child_pid && entry.process.ppid == parent_pid {
                return Some(entry.process.state);
            }
        }
    }

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

/// List all processes for debugging
pub fn list_processes() {
    let table = PROCESS_TABLE.lock();
    crate::kinfo!("=== Process List ===");

    for slot in table.iter() {
        if let Some(entry) = slot {
            crate::kinfo!(
                "PID {}: {:?}, priority={}, time_slice={}ms, total_time={}ms, cr3={:#x}",
                entry.process.pid,
                entry.process.state,
                entry.priority,
                entry.time_slice,
                entry.total_time,
                entry.process.cr3
            );
        }
    }
}

/// Initialize scheduler subsystem
pub fn init() {
    crate::kinfo!(
        "Process scheduler initialized (round-robin, {} max processes, {}ms time slice)",
        MAX_PROCESSES,
        DEFAULT_TIME_SLICE
    );
}

/// Get current running process PID (alias for current_pid)
pub fn get_current_pid() -> Option<Pid> {
    current_pid()
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

/// Context switch implementation
/// Saves the old context and restores the new context
/// This is called from schedule() to switch between processes
#[unsafe(naked)]
unsafe extern "C" fn context_switch(
    _old_context: *mut crate::process::Context,
    _new_context: *const crate::process::Context,
) {
    core::arch::naked_asm!(
        // Save old context (if not null)
        "test rdi, rdi",
        "jz 2f",
        // Save all registers to old_context
        "mov [rdi + 0x00], r15",
        "mov [rdi + 0x08], r14",
        "mov [rdi + 0x10], r13",
        "mov [rdi + 0x18], r12",
        "mov [rdi + 0x20], r11",
        "mov [rdi + 0x28], r10",
        "mov [rdi + 0x30], r9",
        "mov [rdi + 0x38], r8",
        "mov [rdi + 0x40], rsi",
        "mov [rdi + 0x48], rdi",
        "mov [rdi + 0x50], rbp",
        "mov [rdi + 0x58], rdx",
        "mov [rdi + 0x60], rcx",
        "mov [rdi + 0x68], rbx",
        "mov [rdi + 0x70], rax",
        // Save rip (return address)
        "mov rax, [rsp]",
        "mov [rdi + 0x78], rax",
        // Save rsp (before return address was pushed)
        "lea rax, [rsp + 8]",
        "mov [rdi + 0x80], rax",
        // Save rflags
        "pushfq",
        "pop rax",
        "mov [rdi + 0x88], rax",
        // Restore new context
        "2:",
        "mov r15, [rsi + 0x00]",
        "mov r14, [rsi + 0x08]",
        "mov r13, [rsi + 0x10]",
        "mov r12, [rsi + 0x18]",
        "mov r11, [rsi + 0x20]",
        "mov r10, [rsi + 0x28]",
        "mov r9,  [rsi + 0x30]",
        "mov r8,  [rsi + 0x38]",
        "mov rbp, [rsi + 0x50]",
        "mov rdx, [rsi + 0x58]",
        "mov rcx, [rsi + 0x60]",
        "mov rbx, [rsi + 0x68]",
        "mov rax, [rsi + 0x70]",
        // Restore rflags
        "mov rdi, [rsi + 0x88]",
        "push rdi",
        "popfq",
        // Restore rsp
        "mov rsp, [rsi + 0x80]",
        // Push rip onto new stack for ret
        "mov rdi, [rsi + 0x78]",
        "push rdi",
        // Restore rsi and rdi last
        "mov rdi, [rsi + 0x48]",
        "mov rsi, [rsi + 0x40]",
        // Return to new context's rip
        "ret",
    )
}

/// Perform context switch to next ready process
pub fn do_schedule() {
    enum ScheduleDecision {
        FirstRun(crate::process::Process),
        Switch {
            old_context_ptr: *mut crate::process::Context,
            next_context: crate::process::Context,
            next_cr3: u64,
            user_rip: u64,
            user_rsp: u64,
            user_rflags: u64,
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
        for offset in 0..MAX_PROCESSES {
            let idx = (start_idx + offset) % MAX_PROCESSES;
            if let Some(entry) = &table[idx] {
                if entry.process.state == ProcessState::Ready {
                    next_idx = Some(idx);
                    break;
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
                            entry.process.state = ProcessState::Ready;
                            break;
                        }
                    }
                }
            }

            let entry = table[next_idx].as_mut().expect("Process entry vanished");
            entry.time_slice = DEFAULT_TIME_SLICE;
            entry.process.state = ProcessState::Running;

            let first_run = !entry.process.has_entered_user;
            let next_pid = entry.process.pid;
            let next_cr3 = entry.process.cr3;
            let user_rip = entry.process.user_rip;
            let user_rsp = entry.process.user_rsp;
            let user_rflags = entry.process.user_rflags;
            *current_lock = Some(next_pid);

            if first_run {
                entry.process.has_entered_user = true;
                Some(ScheduleDecision::FirstRun(entry.process))
            } else {
                let next_context = entry.process.context;
                let old_context_ptr = if let Some(curr_pid) = current {
                    table.iter_mut().find_map(|slot| {
                        slot.as_mut().and_then(|candidate| {
                            if candidate.process.pid == curr_pid {
                                Some(&mut candidate.process.context as *mut _)
                            } else {
                                None
                            }
                        })
                    })
                } else {
                    None
                };

                Some(ScheduleDecision::Switch {
                    old_context_ptr: old_context_ptr.unwrap_or(core::ptr::null_mut()),
                    next_context,
                    next_cr3,
                    user_rip,
                    user_rsp,
                    user_rflags,
                })
            }
        } else {
            None
        }
    };

    match decision {
        Some(ScheduleDecision::FirstRun(mut process)) => {
            crate::paging::activate_address_space(process.cr3);
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
        }) => unsafe {
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
