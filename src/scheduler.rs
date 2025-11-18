/// Robust O(1) Priority Scheduler (seL4-inspired)
/// Implements strict priority scheduling with round-robin for equal priorities.
/// Uses a bitmap and array of queues for O(1) complexity.

use crate::process::{Pid, Process, ProcessState};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::{Mutex, MutexGuard};
use core::ops::{Deref, DerefMut};

pub const MAX_PROCESSES: usize = 64;
pub const NUM_PRIORITIES: usize = 256;
const DEFAULT_TIME_SLICE: u64 = 10;

/// Global tick counter
static GLOBAL_TICK: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal,
    Realtime,
    Batch,
    Idle,
}

#[derive(Clone, Copy)]
pub struct ProcessEntry {
    pub process: Process,
    pub priority: u8,           // 0-255, 255 is highest
    pub base_priority: u8,
    pub time_slice: u64,
    pub total_time: u64,
    pub wait_time: u64,
    pub last_scheduled: u64,
    pub policy: SchedPolicy,
    pub nice: i8,
    pub preempt_count: u64,
    pub voluntary_switches: u64,
    
    // Intrusive linked list for ready queues
    pub next: Option<usize>,
    pub prev: Option<usize>,
    
    // Stats/Legacy fields
    pub cpu_burst_count: u64,
    pub avg_cpu_burst: u64,
    pub quantum_level: u8,
}

impl ProcessEntry {
    pub fn new(process: Process, priority: u8, policy: SchedPolicy) -> Self {
        Self {
            process,
            priority,
            base_priority: priority,
            time_slice: DEFAULT_TIME_SLICE,
            total_time: 0,
            wait_time: 0,
            last_scheduled: GLOBAL_TICK.load(Ordering::Relaxed),
            policy,
            nice: 0,
            preempt_count: 0,
            voluntary_switches: 0,
            next: None,
            prev: None,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            quantum_level: 0,
        }
    }
}

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

#[derive(Clone, Copy)]
struct ReadyQueue {
    head: Option<usize>,
    tail: Option<usize>,
}

impl ReadyQueue {
    const fn new() -> Self {
        Self { head: None, tail: None }
    }
}

pub struct Scheduler {
    pub table: [Option<ProcessEntry>; MAX_PROCESSES],
    queues: [ReadyQueue; NUM_PRIORITIES],
    bitmap: [u64; 4], // 256 bits, 1 bit per priority level
    current_pid: Option<Pid>,
    pub stats: SchedulerStats,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            table: [None; MAX_PROCESSES],
            queues: [ReadyQueue::new(); NUM_PRIORITIES],
            bitmap: [0; 4],
            current_pid: None,
            stats: SchedulerStats::new(),
        }
    }

    /// Add a process index to the tail of the ready queue for its priority
    fn enqueue(&mut self, idx: usize) {
        if let Some(entry) = &mut self.table[idx] {
            let prio = entry.priority as usize;
            
            // Add to tail
            entry.next = None;
            entry.prev = self.queues[prio].tail;
            
            if let Some(tail_idx) = self.queues[prio].tail {
                if let Some(tail_entry) = &mut self.table[tail_idx] {
                    tail_entry.next = Some(idx);
                }
            } else {
                // Queue was empty
                self.queues[prio].head = Some(idx);
                // Set bit in bitmap
                self.bitmap[prio / 64] |= 1 << (prio % 64);
            }
            self.queues[prio].tail = Some(idx);
        }
    }

    /// Remove a process index from the ready queue
    fn dequeue(&mut self, prio: usize) -> Option<usize> {
        if let Some(head_idx) = self.queues[prio].head {
            let next_idx = self.table[head_idx].as_ref().unwrap().next;
            
            if let Some(next) = next_idx {
                self.table[next].as_mut().unwrap().prev = None;
            } else {
                // Queue becoming empty
                self.queues[prio].tail = None;
                // Clear bit in bitmap
                self.bitmap[prio / 64] &= !(1 << (prio % 64));
            }
            
            self.queues[prio].head = next_idx;
            
            // Clear links in removed entry
            if let Some(entry) = &mut self.table[head_idx] {
                entry.next = None;
                entry.prev = None;
            }
            
            Some(head_idx)
        } else {
            None
        }
    }

    /// Remove a specific process index from its queue (e.g. when blocking)
    fn remove_from_queue(&mut self, idx: usize) {
        // We need to know the priority to find the queue, or we can use the links if we trust them.
        // But we need to update head/tail of the queue.
        // The entry should have the priority.
        if let Some(entry) = &self.table[idx] {
            let prio = entry.priority as usize;
            let prev = entry.prev;
            let next = entry.next;
            
            if let Some(prev_idx) = prev {
                self.table[prev_idx].as_mut().unwrap().next = next;
            } else {
                // Was head
                self.queues[prio].head = next;
                if next.is_none() {
                    // Queue became empty
                    self.bitmap[prio / 64] &= !(1 << (prio % 64));
                }
            }
            
            if let Some(next_idx) = next {
                self.table[next_idx].as_mut().unwrap().prev = prev;
            } else {
                // Was tail
                self.queues[prio].tail = prev;
            }
        }
        
        // Clear links
        if let Some(entry) = &mut self.table[idx] {
            entry.next = None;
            entry.prev = None;
        }
    }

    /// Find the highest priority with ready processes
    fn get_highest_priority(&self) -> Option<usize> {
        for i in (0..4).rev() {
            if self.bitmap[i] != 0 {
                let leading = self.bitmap[i].leading_zeros();
                let bit = 63 - leading;
                return Some(i * 64 + bit as usize);
            }
        }
        None
    }
}

impl Deref for Scheduler {
    type Target = [Option<ProcessEntry>; MAX_PROCESSES];
    fn deref(&self) -> &Self::Target {
        &self.table
    }
}

impl DerefMut for Scheduler {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.table
    }
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

/// Lock the process table (returns the whole scheduler but derefs to table)
pub fn process_table_lock() -> MutexGuard<'static, Scheduler> {
    SCHEDULER.lock()
}

pub fn init() {
    crate::kinfo!("Robust O(1) Scheduler initialized (seL4-style)");
}

pub fn add_process(process: Process, priority: u8) -> Result<(), &'static str> {
    add_process_with_policy(process, priority, SchedPolicy::Normal, 0)
}

pub fn add_process_with_policy(
    process: Process,
    priority: u8,
    policy: SchedPolicy,
    nice: i8,
) -> Result<(), &'static str> {
    let mut sched = SCHEDULER.lock();
    
    // Find free slot
    let mut free_idx = None;
    for (i, slot) in sched.table.iter().enumerate() {
        if slot.is_none() {
            free_idx = Some(i);
            break;
        }
    }
    
    if let Some(idx) = free_idx {
        let entry = ProcessEntry::new(process, priority, policy);
        sched.table[idx] = Some(entry);
        
        // If ready, add to queue
        if process.state == ProcessState::Ready {
            sched.enqueue(idx);
        }
        
        crate::kinfo!("Scheduler: Added PID {} with priority {}", process.pid, priority);
        Ok(())
    } else {
        Err("Process table full")
    }
}

pub fn remove_process(pid: Pid) -> Result<(), &'static str> {
    let mut sched = SCHEDULER.lock();
    let mut found_idx = None;
    let mut removed_cr3 = None;
    
    for (i, slot) in sched.table.iter().enumerate() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                found_idx = Some(i);
                removed_cr3 = Some(entry.process.cr3);
                break;
            }
        }
    }
    
    if let Some(idx) = found_idx {
        // Remove from queue if it was in one
        // We can check if it's in a queue by checking state or just trying to remove
        // But remove_from_queue relies on next/prev which might be None if not in queue
        // However, if state is Ready, it SHOULD be in queue.
        // If Running, it is NOT in queue (in this implementation).
        // If Sleeping/Zombie, NOT in queue.
        
        let state = sched.table[idx].as_ref().unwrap().process.state;
        if state == ProcessState::Ready {
            sched.remove_from_queue(idx);
        }
        
        sched.table[idx] = None;
        
        if sched.current_pid == Some(pid) {
            sched.current_pid = None;
        }
        
        drop(sched); // Release lock before freeing memory
        
        if let Some(cr3) = removed_cr3 {
            if cr3 != 0 {
                crate::paging::free_process_address_space(cr3);
            }
        }
        
        Ok(())
    } else {
        Err("Process not found")
    }
}

pub fn set_process_state(pid: Pid, state: ProcessState) -> Result<(), &'static str> {
    let mut sched = SCHEDULER.lock();
    let mut found_idx = None;
    
    for (i, slot) in sched.table.iter().enumerate() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                found_idx = Some(i);
                break;
            }
        }
    }
    
    if let Some(idx) = found_idx {
        let old_state = sched.table[idx].as_ref().unwrap().process.state;
        if old_state == state {
            return Ok(());
        }
        
        // Handle queue transitions
        if old_state == ProcessState::Ready {
            sched.remove_from_queue(idx);
        }
        
        sched.table[idx].as_mut().unwrap().process.state = state;
        
        if state == ProcessState::Ready {
            sched.enqueue(idx);
        }
        
        Ok(())
    } else {
        Err("Process not found")
    }
}

pub fn schedule() -> Option<Pid> {
    // This function is called to pick the next process.
    // In this O(1) implementation, we just peek at the highest priority queue.
    // But wait, schedule() in the old implementation also updated state from Running to Ready.
    // We should probably do that in do_schedule or here.
    
    // Actually, schedule() returns the PID to switch to.
    // do_schedule() calls schedule() logic internally usually.
    // The existing code had schedule() return Option<Pid> and update state.
    
    let mut sched = SCHEDULER.lock();
    let current_pid = sched.current_pid;
    
    // If current process is running, we need to decide if it stays running or yields.
    // If it yields (e.g. time slice expired), it goes to Ready.
    
    // But schedule() is usually called when we WANT to switch.
    
    // Let's look at the highest priority ready process.
    if let Some(prio) = sched.get_highest_priority() {
        if let Some(head_idx) = sched.queues[prio].head {
            let next_pid = sched.table[head_idx].as_ref().unwrap().process.pid;
            
            // If current is running and has higher or equal priority, and time slice > 0,
            // we might not want to switch unless this was called explicitly to yield.
            // But if schedule() is called, we assume a switch is requested or needed.
            
            return Some(next_pid);
        }
    }
    
    None
}

pub fn tick(elapsed_ms: u64) -> bool {
    GLOBAL_TICK.fetch_add(1, Ordering::Relaxed);
    let mut sched = SCHEDULER.lock();
    let current_pid = sched.current_pid;
    
    if let Some(pid) = current_pid {
        // Find current process
        let mut current_idx = None;
        for (i, slot) in sched.table.iter().enumerate() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    current_idx = Some(i);
                    break;
                }
            }
        }
        
        if let Some(idx) = current_idx {
            let entry = sched.table[idx].as_mut().unwrap();
            if entry.process.state == ProcessState::Running {
                entry.total_time += elapsed_ms;
                
                if entry.time_slice > elapsed_ms {
                    entry.time_slice -= elapsed_ms;
                    
                    // Check for preemption
                    let current_prio = entry.priority as usize;
                    if let Some(highest_prio) = sched.get_highest_priority() {
                        if highest_prio > current_prio {
                            return true; // Preempt!
                        }
                    }
                    return false;
                } else {
                    // Time slice expired
                    entry.time_slice = 0;
                    return true; // Reschedule
                }
            }
        }
    }
    
    false
}

// ... Helper functions ...

pub fn current_pid() -> Option<Pid> {
    SCHEDULER.lock().current_pid
}

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

pub fn set_current_pid(pid: Option<Pid>) {
    let mut sched = SCHEDULER.lock();
    sched.current_pid = pid;
    if pid.is_none() {
        crate::paging::activate_address_space(0);
    }
}

pub fn get_process(pid: Pid) -> Option<Process> {
    let sched = SCHEDULER.lock();
    for slot in sched.table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some(entry.process);
            }
        }
    }
    None
}

pub fn set_process_exit_code(pid: Pid, code: i32) -> Result<(), &'static str> {
    let mut sched = SCHEDULER.lock();
    for slot in sched.table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.process.exit_code = code;
                return Ok(());
            }
        }
    }
    Err("Process not found")
}

pub fn update_current_user_context(user_rip: u64, user_rsp: u64, user_rflags: u64) {
    let mut sched = SCHEDULER.lock();
    if let Some(pid) = sched.current_pid {
        for slot in sched.table.iter_mut() {
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

pub fn get_child_state(parent_pid: Pid, child_pid: Pid) -> Option<ProcessState> {
    let sched = SCHEDULER.lock();
    for slot in sched.table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == child_pid {
                if entry.process.ppid == parent_pid {
                    return Some(entry.process.state);
                }
                return None;
            }
        }
    }
    None
}

pub fn find_child_with_state(parent_pid: Pid, target_state: ProcessState) -> Option<Pid> {
    let sched = SCHEDULER.lock();
    for slot in sched.table.iter() {
        if let Some(entry) = slot {
            if entry.process.ppid == parent_pid && entry.process.state == target_state {
                return Some(entry.process.pid);
            }
        }
    }
    None
}

pub fn list_processes() {
    let sched = SCHEDULER.lock();
    crate::kinfo!("=== Process List (O(1) Scheduler) ===");
    crate::kinfo!("PID   PPID  State       Prio  Policy");
    for slot in sched.table.iter() {
        if let Some(entry) = slot {
            let state_str = match entry.process.state {
                ProcessState::Ready => "Ready",
                ProcessState::Running => "Running",
                ProcessState::Sleeping => "Sleeping",
                ProcessState::Zombie => "Zombie",
            };
            crate::kinfo!(
                "{:<5} {:<5} {:<10} {:<5} {:?}",
                entry.process.pid,
                entry.process.ppid,
                state_str,
                entry.priority,
                entry.policy
            );
        }
    }
}

pub fn boost_all_priorities() {
    // No-op in strict priority scheduler, or could implement anti-starvation
}

pub fn set_process_policy(pid: Pid, policy: SchedPolicy, nice: i8) -> Result<(), &'static str> {
    let mut sched = SCHEDULER.lock();
    let mut found_idx = None;
    for (i, slot) in sched.table.iter().enumerate() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                found_idx = Some(i);
                break;
            }
        }
    }
    
    if let Some(idx) = found_idx {
        let _old_prio = sched.table[idx].as_ref().unwrap().priority;
        let state = sched.table[idx].as_ref().unwrap().process.state;
        
        // Remove from queue if ready
        if state == ProcessState::Ready {
            sched.remove_from_queue(idx);
        }
        
        let entry = sched.table[idx].as_mut().unwrap();
        entry.policy = policy;
        entry.nice = nice;
        
        // Map policy to priority range
        entry.priority = match policy {
            SchedPolicy::Realtime => 200,
            SchedPolicy::Normal => 100,
            SchedPolicy::Batch => 50,
            SchedPolicy::Idle => 0,
        } + (nice.clamp(-20, 19) + 20) as u8; // Simple mapping
        
        // Re-enqueue if ready
        if state == ProcessState::Ready {
            sched.enqueue(idx);
        }
        Ok(())
    } else {
        Err("Process not found")
    }
}

pub fn get_stats() -> SchedulerStats {
    SCHEDULER.lock().stats
}

pub fn get_process_sched_info(pid: Pid) -> Option<(u8, u8, SchedPolicy, i8, u64, u64)> {
    let sched = SCHEDULER.lock();
    for slot in sched.table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some((
                    entry.priority,
                    0, // quantum level unused
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

pub fn adjust_process_priority(_pid: Pid, _nice_delta: i8) -> Result<i8, &'static str> {
    // Simplified
    Ok(0)
}

pub fn get_current_pid() -> Option<Pid> {
    current_pid()
}

pub fn mark_process_as_forked_child(pid: Pid) {
    let _ = set_process_state(pid, ProcessState::Ready);
}

pub fn update_process_cr3(pid: Pid, new_cr3: u64) -> Result<(), &'static str> {
    let mut sched = SCHEDULER.lock();
    for slot in sched.table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                entry.process.cr3 = new_cr3;
                if sched.current_pid == Some(pid) {
                    crate::paging::activate_address_space(new_cr3);
                }
                return Ok(());
            }
        }
    }
    Err("Process not found")
}

pub fn detect_potential_deadlocks() -> [Option<Pid>; MAX_PROCESSES] {
    [None; MAX_PROCESSES]
}

pub fn force_reschedule() {
    let mut sched = SCHEDULER.lock();
    if let Some(pid) = sched.current_pid {
        for slot in sched.table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    entry.time_slice = 0;
                    break;
                }
            }
        }
    }
}

pub fn get_process_counts() -> (usize, usize, usize, usize) {
    let sched = SCHEDULER.lock();
    let mut counts = (0, 0, 0, 0);
    for slot in sched.table.iter() {
        if let Some(entry) = slot {
            match entry.process.state {
                ProcessState::Ready => counts.0 += 1,
                ProcessState::Running => counts.1 += 1,
                ProcessState::Sleeping => counts.2 += 1,
                ProcessState::Zombie => counts.3 += 1,
            }
        }
    }
    counts
}

pub fn get_load_average() -> (f32, f32, f32) {
    let (ready, running, _, _) = get_process_counts();
    let load = (ready + running) as f32;
    (load, load, load)
}

pub fn age_process_priorities() {}

#[unsafe(naked)]
unsafe extern "C" fn context_switch(
    _old_context: *mut crate::process::Context,
    _new_context: *const crate::process::Context,
) {
    core::arch::naked_asm!(
        "test rdi, rdi",
        "jz 2f",
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
        "mov rax, [rsp]",
        "mov [rdi + 0x78], rax",
        "lea rax, [rsp + 8]",
        "mov [rdi + 0x80], rax",
        "pushfq",
        "pop rax",
        "mov [rdi + 0x88], rax",
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
        "mov rdi, [rsi + 0x88]",
        "push rdi",
        "popfq",
        "mov rsp, [rsi + 0x80]",
        "mov rdi, [rsi + 0x78]",
        "push rdi",
        "mov rdi, [rsi + 0x48]",
        "mov rsi, [rsi + 0x40]",
        "ret",
    )
}

pub fn do_schedule() {
    // This is the main scheduling loop logic
    // 1. Check if current process needs to be saved
    // 2. Pick next process
    // 3. Context switch
    
    let mut sched = SCHEDULER.lock();
    sched.stats.total_context_switches += 1;
    
    let current_pid = sched.current_pid;
    let mut current_idx = None;
    
    // Handle current process
    if let Some(pid) = current_pid {
        for (i, slot) in sched.table.iter_mut().enumerate() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    current_idx = Some(i);
                    
                    if entry.process.state == ProcessState::Running {
                        // Save syscall context if needed (from GS_DATA)
                        unsafe {
                            let gs_data_ptr = core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64;
                            entry.process.user_rip = gs_data_ptr.add(crate::interrupts::GS_SLOT_SAVED_RCX).read();
                            entry.process.user_rsp = gs_data_ptr.add(crate::interrupts::GS_SLOT_USER_RSP).read();
                            entry.process.user_rflags = gs_data_ptr.add(crate::interrupts::GS_SLOT_SAVED_RFLAGS).read();
                        }
                        
                        entry.process.state = ProcessState::Ready;
                        // Re-enqueue current process since it's still ready
                        // We need to drop the mutable borrow of entry to call enqueue
                        // So we do it after loop
                    }
                    break;
                }
            }
        }
    }
    
    if let Some(idx) = current_idx {
        if sched.table[idx].as_ref().unwrap().process.state == ProcessState::Ready {
            sched.enqueue(idx);
        }
    }
    
    // Pick next process
    let next_idx = if let Some(prio) = sched.get_highest_priority() {
        sched.dequeue(prio)
    } else {
        None
    };
    
    if let Some(idx) = next_idx {
        let entry = sched.table[idx].as_mut().unwrap();
        entry.process.state = ProcessState::Running;
        entry.time_slice = DEFAULT_TIME_SLICE;
        entry.last_scheduled = GLOBAL_TICK.load(Ordering::Relaxed);
        
        let next_pid = entry.process.pid;
        let next_cr3 = entry.process.cr3;
        let next_context = entry.process.context;
        let first_run = !entry.process.has_entered_user;
        let user_rip = entry.process.user_rip;
        let user_rsp = entry.process.user_rsp;
        let user_rflags = entry.process.user_rflags;
        
        sched.current_pid = Some(next_pid);
        
        // Prepare for switch
        let old_context_ptr = if let Some(curr_idx) = current_idx {
             if let Some(entry) = sched.table[curr_idx].as_mut() {
                 if entry.process.state != ProcessState::Zombie {
                     Some(&mut entry.process.context as *mut _)
                 } else {
                     None
                 }
             } else { None }
        } else { None };
        
        drop(sched); // Release lock
        
        if first_run {
            // Handle first run
            // We need to set has_entered_user = true
            {
                let mut sched = SCHEDULER.lock();
                if let Some(entry) = sched.table[idx].as_mut() {
                    entry.process.has_entered_user = true;
                    // We need a copy of process to call execute
                    let mut p = entry.process;
                    drop(sched);
                    p.execute();
                }
            }
        } else {
            unsafe {
                if user_rsp != 0 {
                    crate::interrupts::restore_user_syscall_context(user_rip, user_rsp, user_rflags);
                }
                crate::paging::activate_address_space(next_cr3);
                context_switch(old_context_ptr.unwrap_or(core::ptr::null_mut()), &next_context as *const _);
            }
        }
    } else {
        // No process to run
        sched.current_pid = None;
        drop(sched);
        // Idle loop or return
    }
}
