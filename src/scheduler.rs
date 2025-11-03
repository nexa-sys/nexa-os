/// Basic round-robin process scheduler for hybrid kernel
use crate::process::{Process, ProcessState, Pid};
use spin::Mutex;

const MAX_PROCESSES: usize = 32;

/// Process control block with scheduling info
#[derive(Clone, Copy)]
pub struct ProcessEntry {
    pub process: Process,
    pub priority: u8,        // 0 = highest, 255 = lowest
    pub time_slice: u64,     // Remaining time slice in ms
    pub total_time: u64,     // Total CPU time used in ms
}

impl ProcessEntry {
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
            },
            priority: 128,
            time_slice: 0,
            total_time: 0,
        }
    }
}

/// Process table
static PROCESS_TABLE: Mutex<[Option<ProcessEntry>; MAX_PROCESSES]> = Mutex::new([None; MAX_PROCESSES]);

/// Currently running process PID
static CURRENT_PID: Mutex<Option<Pid>> = Mutex::new(None);

/// Default time slice in milliseconds
const DEFAULT_TIME_SLICE: u64 = 10;

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
            crate::kinfo!("Scheduler: Added process PID {} with priority {}", process.pid, priority);
            return Ok(());
        }
    }

    Err("Process table full")
}

/// Remove a process from the scheduler
pub fn remove_process(pid: Pid) -> Result<(), &'static str> {
    let mut table = PROCESS_TABLE.lock();

    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                crate::kinfo!("Scheduler: Removed process PID {}", pid);
                *slot = None;
                return Ok(());
            }
        }
    }

    Err("Process not found")
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

/// Set current running process
pub fn set_current_pid(pid: Option<Pid>) {
    *CURRENT_PID.lock() = pid;
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
                            if e.process.pid == curr_pid && e.process.state == ProcessState::Running {
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

/// List all processes for debugging
pub fn list_processes() {
    let table = PROCESS_TABLE.lock();
    crate::kinfo!("=== Process List ===");

    for slot in table.iter() {
        if let Some(entry) = slot {
            crate::kinfo!(
                "PID {}: {:?}, priority={}, time_slice={}ms, total_time={}ms",
                entry.process.pid,
                entry.process.state,
                entry.priority,
                entry.time_slice,
                entry.total_time
            );
        }
    }
}

/// Initialize scheduler subsystem
pub fn init() {
    crate::kinfo!("Process scheduler initialized (round-robin, {} max processes, {}ms time slice)",
        MAX_PROCESSES, DEFAULT_TIME_SLICE);
}
