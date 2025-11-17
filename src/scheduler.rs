/// Advanced multi-level feedback queue (MLFQ) process scheduler for hybrid kernel
use crate::process::{Pid, Process, ProcessState};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const MAX_PROCESSES: usize = 64; // Increased from 32
const NUM_PRIORITY_LEVELS: usize = 8; // 0 = highest, 7 = lowest
const BASE_TIME_SLICE_MS: u64 = 5; // Base quantum for highest priority

/// Scheduling policy for a process
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal,     // Standard priority-based scheduling
    Realtime,   // Real-time priority (higher than normal)
    Batch,      // Background batch processing (lower priority)
    Idle,       // Only runs when nothing else is ready
}

/// Process control block with advanced scheduling info
#[derive(Clone, Copy)]
pub struct ProcessEntry {
    pub process: Process,
    pub priority: u8,           // Current dynamic priority (0 = highest, 255 = lowest)
    pub base_priority: u8,      // Base static priority
    pub time_slice: u64,        // Remaining time slice in ms
    pub total_time: u64,        // Total CPU time used in ms
    pub wait_time: u64,         // Time spent waiting in ready queue
    pub last_scheduled: u64,    // Last time this process was scheduled (in ticks)
    pub cpu_burst_count: u64,   // Number of CPU bursts
    pub avg_cpu_burst: u64,     // Average CPU burst length (for I/O vs CPU bound detection)
    pub policy: SchedPolicy,    // Scheduling policy
    pub nice: i8,               // Nice value (-20 to 19, POSIX compatible)
    pub quantum_level: u8,      // Current priority level in MLFQ (0-7)
    pub preempt_count: u64,     // Number of times preempted
    pub voluntary_switches: u64, // Number of voluntary context switches
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
                is_fork_child: false,
                cr3: 0,
                tty: 0,
                memory_base: 0,
                memory_size: 0,
                user_rip: 0,
                user_rsp: 0,
                user_rflags: 0,
                exit_code: 0,
            },
            priority: 128,
            base_priority: 128,
            time_slice: 0,
            total_time: 0,
            wait_time: 0,
            last_scheduled: 0,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            policy: SchedPolicy::Normal,
            nice: 0,
            quantum_level: 4, // Start at middle level
            preempt_count: 0,
            voluntary_switches: 0,
        }
    }
}

/// Process table
static PROCESS_TABLE: Mutex<[Option<ProcessEntry>; MAX_PROCESSES]> =
    Mutex::new([None; MAX_PROCESSES]);

/// Currently running process PID
static CURRENT_PID: Mutex<Option<Pid>> = Mutex::new(None);

/// Global tick counter for scheduler timing
static GLOBAL_TICK: AtomicU64 = AtomicU64::new(0);

/// Scheduler statistics
static SCHED_STATS: Mutex<SchedulerStats> = Mutex::new(SchedulerStats::new());

/// Scheduler statistics structure
#[derive(Clone, Copy)]
pub struct SchedulerStats {
    pub total_context_switches: u64,
    pub total_preemptions: u64,
    pub total_voluntary_switches: u64,
    pub idle_time: u64,
    pub last_idle_start: u64,
}

impl SchedulerStats {
    const fn new() -> Self {
        Self {
            total_context_switches: 0,
            total_preemptions: 0,
            total_voluntary_switches: 0,
            idle_time: 0,
            last_idle_start: 0,
        }
    }
}

/// Default time slice in milliseconds
const DEFAULT_TIME_SLICE: u64 = 10;

/// Calculate time slice based on priority level (MLFQ)
/// Higher levels get longer quanta to reduce context switching overhead
#[inline]
fn calculate_time_slice(quantum_level: u8) -> u64 {
    BASE_TIME_SLICE_MS * (1 << quantum_level.min(7))
}

/// Calculate dynamic priority based on wait time and CPU usage
/// Rewards I/O-bound processes and penalizes CPU-bound processes
#[inline]
fn calculate_dynamic_priority(base: u8, wait_time: u64, cpu_time: u64, nice: i8) -> u8 {
    let base = base as i32;
    let nice_offset = nice as i32; // -20 to 19
    
    // Priority boost for waiting (I/O bound processes)
    let wait_boost = (wait_time / 100).min(40) as i32;
    
    // Priority penalty for CPU usage
    let cpu_penalty = (cpu_time / 1000).min(40) as i32;
    
    let dynamic = base + nice_offset + cpu_penalty - wait_boost;
    dynamic.clamp(0, 255) as u8
}

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
                SchedPolicy::Realtime => 0,  // Shortest quantum, highest priority
                SchedPolicy::Normal => 4,    // Middle level
                SchedPolicy::Batch => 6,     // Longer quantum, lower priority
                SchedPolicy::Idle => 7,      // Longest quantum, lowest priority
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
    crate::serial::_print(format_args!("[remove_process] Removing PID {}\n", pid));

    let mut table = PROCESS_TABLE.lock();
    let mut removed_cr3 = None;
    let mut removed = false;

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                crate::kinfo!("Scheduler: Removed process PID {}", pid);

                // Save CR3 for cleanup after releasing the lock
                if entry.process.cr3 != 0 {
                    removed_cr3 = Some(entry.process.cr3);
                    crate::serial::_print(format_args!(
                        "[remove_process] PID {} had CR3={:#x}, will free page tables\n",
                        pid, entry.process.cr3
                    ));
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

        // Clean up process page tables if it had its own CR3
        if let Some(cr3) = removed_cr3 {
            crate::kdebug!("Freeing page tables for PID {} (CR3={:#x})", pid, cr3);
            crate::paging::free_process_address_space(cr3);
            crate::serial::_print(format_args!(
                "[remove_process] Freed page tables for PID {} (CR3={:#x})\n",
                pid, cr3
            ));
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
                crate::serial::_print(format_args!(
                    "[set_process_state] PID {} state: {:?} -> {:?}\n",
                    pid, entry.process.state, state
                ));
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
                                    let should_preempt_for_this = match (check_entry.policy, current_policy) {
                                        (SchedPolicy::Realtime, SchedPolicy::Realtime) => {
                                            check_entry.priority < current_priority
                                        }
                                        (SchedPolicy::Realtime, _) => true,
                                        (_, SchedPolicy::Realtime) => false,
                                        (SchedPolicy::Normal, SchedPolicy::Normal) => {
                                            check_entry.priority + 10 < current_priority // Significant priority difference
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
            if entry.process.pid == child_pid {
                crate::serial::_print(format_args!(
                    "[get_child_state] Found PID {}: ppid={}, parent_pid arg={}, state={:?}\n",
                    child_pid, entry.process.ppid, parent_pid, entry.process.state
                ));
                if entry.process.ppid == parent_pid {
                    return Some(entry.process.state);
                } else {
                    crate::serial::_print(format_args!(
                        "[get_child_state] PID {} has wrong parent (ppid={}, expected={})\n",
                        child_pid, entry.process.ppid, parent_pid
                    ));
                    return None;
                }
            }
        }
    }

    crate::serial::_print(format_args!(
        "[get_child_state] PID {} not found in process table\n",
        child_pid
    ));
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

/// List all processes for debugging with extended information
pub fn list_processes() {
    let table = PROCESS_TABLE.lock();
    crate::kinfo!("=== Process List (Extended) ===");
    crate::kinfo!(
        "{:<5} {:<5} {:<12} {:<8} {:<6} {:<5} {:<10} {:<10} {:<8} {:<10}",
        "PID", "PPID", "State", "Policy", "Nice", "QLvl", "CPU(ms)", "Wait(ms)", "Preempt", "CR3"
    );

    for slot in table.iter() {
        if let Some(entry) = slot {
            let policy_str = match entry.policy {
                SchedPolicy::Realtime => "RT",
                SchedPolicy::Normal => "Normal",
                SchedPolicy::Batch => "Batch",
                SchedPolicy::Idle => "Idle",
            };
            
            let state_str = match entry.process.state {
                ProcessState::Ready => "Ready",
                ProcessState::Running => "Running",
                ProcessState::Sleeping => "Sleeping",
                ProcessState::Zombie => "Zombie",
            };
            
            crate::kinfo!(
                "{:<5} {:<5} {:<12} {:<8} {:<6} {:<5} {:<10} {:<10} {:<8} {:#010x}",
                entry.process.pid,
                entry.process.ppid,
                state_str,
                policy_str,
                entry.nice,
                entry.quantum_level,
                entry.total_time,
                entry.wait_time,
                entry.preempt_count,
                entry.process.cr3
            );
        }
    }
    
    let stats = SCHED_STATS.lock();
    crate::kinfo!("=== Scheduler Statistics ===");
    crate::kinfo!("Total context switches: {}", stats.total_context_switches);
    crate::kinfo!("Total preemptions: {}", stats.total_preemptions);
    crate::kinfo!("Total voluntary switches: {}", stats.total_voluntary_switches);
    crate::kinfo!("Idle time: {}ms", stats.idle_time);
}

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

/// Boost priority of a process (MLFQ priority boost mechanism)
/// This is called periodically to prevent starvation
pub fn boost_all_priorities() {
    let mut table = PROCESS_TABLE.lock();
    
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.state != ProcessState::Zombie {
                // Reset to highest priority level
                entry.quantum_level = match entry.policy {
                    SchedPolicy::Realtime => 0,
                    SchedPolicy::Normal => 2,
                    SchedPolicy::Batch => 4,
                    SchedPolicy::Idle => 6,
                };
                
                // Reset priority to base
                entry.priority = entry.base_priority;
                
                // Reset counters
                entry.preempt_count = 0;
                
                crate::kdebug!(
                    "Boosted priority for PID {} to level {}",
                    entry.process.pid,
                    entry.quantum_level
                );
            }
        }
    }
}

/// Set the scheduling policy for a process
pub fn set_process_policy(pid: Pid, policy: SchedPolicy, nice: i8) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();
    
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.policy = policy;
                entry.nice = nice.clamp(-20, 19);
                
                // Adjust quantum level based on new policy
                entry.quantum_level = match policy {
                    SchedPolicy::Realtime => 0,
                    SchedPolicy::Normal => 4,
                    SchedPolicy::Batch => 6,
                    SchedPolicy::Idle => 7,
                };
                
                // Recalculate time slice
                entry.time_slice = calculate_time_slice(entry.quantum_level);
                
                crate::kinfo!(
                    "Process {} policy changed to {:?}, nice={}, quantum_level={}",
                    pid,
                    policy,
                    nice,
                    entry.quantum_level
                );
                return Ok(());
            }
        }
    }
    
    Err("Process not found")
}

/// Get scheduler statistics
pub fn get_stats() -> SchedulerStats {
    *SCHED_STATS.lock()
}

/// Get process scheduling information
pub fn get_process_sched_info(pid: Pid) -> Option<(u8, u8, SchedPolicy, i8, u64, u64)> {
    let table = PROCESS_TABLE.lock();
    
    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some((
                    entry.priority,
                    entry.quantum_level,
                    entry.policy,
                    entry.nice,
                    entry.total_time,
                    entry.wait_time,
                ));
            }
        }
    }
    
    None
}

/// Adjust process priority dynamically (for syscalls like nice())
pub fn adjust_process_priority(pid: Pid, nice_delta: i8) -> Result<i8, &'static str> {
    let mut table = PROCESS_TABLE.lock();
    
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                let old_nice = entry.nice;
                entry.nice = (entry.nice + nice_delta).clamp(-20, 19);
                
                // Recalculate priority
                entry.priority = calculate_dynamic_priority(
                    entry.base_priority,
                    entry.wait_time,
                    entry.total_time,
                    entry.nice,
                );
                
                crate::kdebug!(
                    "Process {} nice: {} -> {}, priority: {}",
                    pid,
                    old_nice,
                    entry.nice,
                    entry.priority
                );
                
                return Ok(entry.nice);
            }
        }
    }
    
    Err("Process not found")
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

/// Perform context switch to next ready process with statistics tracking
pub fn do_schedule() {
    // Update scheduler statistics
    {
        let mut stats = SCHED_STATS.lock();
        stats.total_context_switches += 1;
    }

    // Debug: Print all process states before scheduling
    {
        let table = PROCESS_TABLE.lock();
        crate::serial::_print(format_args!("[do_schedule] Process table snapshot:\n"));
        for slot in table.iter() {
            if let Some(entry) = slot {
                crate::serial::_print(format_args!(
                    "  PID {}: ppid={}, state={:?}, policy={:?}, CR3={:#x}\n",
                    entry.process.pid, entry.process.ppid, entry.process.state, entry.policy, entry.process.cr3
                ));
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
                            // CRITICAL: Save syscall context from GS_DATA before yielding CPU
                            // When a process calls syscall (like wait4) and yields via do_schedule(),
                            // the syscall handler saved RIP/RSP/RFLAGS in GS_DATA.
                            // We must copy these values to the process struct so they can be
                            // restored when this process is scheduled again.
                            unsafe {
                                let gs_data_ptr =
                                    core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64;
                                let saved_rip =
                                    gs_data_ptr.add(crate::interrupts::GS_SLOT_SAVED_RCX).read();
                                let saved_rsp =
                                    gs_data_ptr.add(crate::interrupts::GS_SLOT_USER_RSP).read();
                                let saved_rflags = gs_data_ptr
                                    .add(crate::interrupts::GS_SLOT_SAVED_RFLAGS)
                                    .read();

                                crate::serial::_print(format_args!(
                                    "[do_schedule] Saving syscall context for PID {}: rip={:#x}, rsp={:#x}, rflags={:#x}\n",
                                    curr_pid, saved_rip, saved_rsp, saved_rflags
                                ));

                                entry.process.user_rip = saved_rip;
                                entry.process.user_rsp = saved_rsp;
                                entry.process.user_rflags = saved_rflags;
                            }

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
                // CRITICAL: Don't set has_entered_user here!
                // We return a COPY of the process, and execute() will set it on the copy.
                // We need to set it in the process table AFTER execute() completes.
                // But execute() never returns, so we can't do it there.
                // The solution: set it NOW in the process table, not on the copy.
                crate::serial::_print(format_args!(
                    "[do_schedule] Creating FirstRun decision for PID {}, CR3={:#x}\n",
                    entry.process.pid, entry.process.cr3
                ));
                Some(ScheduleDecision::FirstRun(entry.process))
            } else {
                let next_context = entry.process.context;

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
                                    crate::serial::_print(format_args!(
                                        "[do_schedule] Current PID {} is Zombie, not saving context\n",
                                        curr_pid
                                    ));
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
                })
            }
        } else {
            None
        }
    };

    match decision {
        Some(ScheduleDecision::FirstRun(mut process)) => {
            crate::serial::_print(format_args!(
                "[do_schedule] FirstRun: PID={}, entry={:#x}, stack={:#x}, has_entered_user={}, CR3={:#x}\n",
                process.pid, process.entry_point, process.stack_top, process.has_entered_user, process.cr3
            ));

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
            is_voluntary,
        }) => unsafe {
            // Update statistics
            {
                let mut stats = SCHED_STATS.lock();
                if is_voluntary {
                    stats.total_voluntary_switches += 1;
                } else {
                    stats.total_preemptions += 1;
                }
            }
            
            crate::serial::_print(format_args!(
                "[do_schedule] Switch ({}): user_rip={:#x}, user_rsp={:#x}, user_rflags={:#x}\n",
                if is_voluntary { "voluntary" } else { "preempt" },
                user_rip, user_rsp, user_rflags
            ));
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

/// Detect potential deadlocks by analyzing process wait states
/// Returns list of PIDs that might be in a deadlock
pub fn detect_potential_deadlocks() -> [Option<Pid>; MAX_PROCESSES] {
    let table = PROCESS_TABLE.lock();
    let mut potential_deadlocks = [None; MAX_PROCESSES];
    let mut count = 0;
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);
    
    // Threshold: if a process has been waiting for more than 10 seconds (10000 ticks)
    const DEADLOCK_THRESHOLD_TICKS: u64 = 10000;
    
    for slot in table.iter() {
        if let Some(entry) = slot {
            // Check for processes stuck in Sleeping state for too long
            if entry.process.state == ProcessState::Sleeping {
                let wait_ticks = current_tick.saturating_sub(entry.last_scheduled);
                if wait_ticks > DEADLOCK_THRESHOLD_TICKS {
                    crate::kwarn!(
                        "Potential deadlock: PID {} sleeping for {} ticks (>{})",
                        entry.process.pid,
                        wait_ticks,
                        DEADLOCK_THRESHOLD_TICKS
                    );
                    if count < MAX_PROCESSES {
                        potential_deadlocks[count] = Some(entry.process.pid);
                        count += 1;
                    }
                }
            }
            
            // Check for excessive wait time in Ready state (starvation)
            if entry.process.state == ProcessState::Ready && entry.wait_time > DEADLOCK_THRESHOLD_TICKS {
                crate::kwarn!(
                    "Potential starvation: PID {} waiting in Ready state for {} ms",
                    entry.process.pid,
                    entry.wait_time
                );
                if count < MAX_PROCESSES {
                    potential_deadlocks[count] = Some(entry.process.pid);
                    count += 1;
                }
            }
        }
    }
    
    potential_deadlocks
}

/// Force reschedule by setting current process time slice to 0
/// Used for explicit yield or priority inversion handling
pub fn force_reschedule() {
    let mut table = PROCESS_TABLE.lock();
    let current = *CURRENT_PID.lock();
    
    if let Some(curr_pid) = current {
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == curr_pid {
                    entry.time_slice = 0;
                    crate::kdebug!("Force reschedule for PID {}", curr_pid);
                    break;
                }
            }
        }
    }
}

/// Get total number of processes in each state
pub fn get_process_counts() -> (usize, usize, usize, usize) {
    let table = PROCESS_TABLE.lock();
    let mut ready = 0;
    let mut running = 0;
    let mut sleeping = 0;
    let mut zombie = 0;
    
    for slot in table.iter() {
        if let Some(entry) = slot {
            match entry.process.state {
                ProcessState::Ready => ready += 1,
                ProcessState::Running => running += 1,
                ProcessState::Sleeping => sleeping += 1,
                ProcessState::Zombie => zombie += 1,
            }
        }
    }
    
    (ready, running, sleeping, zombie)
}

/// Calculate system load average (simplified)
/// Returns (1-min load, 5-min load, 15-min load) - currently just returns ready+running count
pub fn get_load_average() -> (f32, f32, f32) {
    let (ready, running, _, _) = get_process_counts();
    let load = (ready + running) as f32;
    
    // In a real implementation, these would be exponentially-weighted moving averages
    // For now, return the same value for all three
    (load, load, load)
}

/// Age all processes' wait times to prevent starvation
/// Called periodically by the scheduler (e.g., every 100ms)
pub fn age_process_priorities() {
    let mut table = PROCESS_TABLE.lock();
    let current_tick = GLOBAL_TICK.load(Ordering::Relaxed);
    
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.state == ProcessState::Ready {
                let wait_delta = current_tick.saturating_sub(entry.last_scheduled);
                
                // Age: reduce priority number (increase priority) for long-waiting processes
                if wait_delta > 100 && entry.priority > 0 {
                    entry.priority = entry.priority.saturating_sub(1);
                    
                    // Also promote to higher quantum level for fairness
                    if entry.quantum_level > 0 && wait_delta > 500 {
                        entry.quantum_level -= 1;
                        crate::kdebug!(
                            "Aged process {}: promoted to quantum level {}",
                            entry.process.pid,
                            entry.quantum_level
                        );
                    }
                }
            }
        }
    }
}

