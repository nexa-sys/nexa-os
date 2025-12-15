//! Mock scheduler for testing
//!
//! Simulates process scheduling without actual hardware.

use std::collections::VecDeque;

/// Process state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Zombie,
}

/// A mock process for testing
#[derive(Clone, Debug)]
pub struct MockProcess {
    pub pid: u32,
    pub ppid: u32,
    pub state: ProcessState,
    pub priority: i32,
    pub name: String,
    pub time_slice: u32,
    pub cpu_time: u64,
}

impl MockProcess {
    pub fn new(pid: u32, name: &str) -> Self {
        Self {
            pid,
            ppid: 0,
            state: ProcessState::Ready,
            priority: 0,
            name: name.to_string(),
            time_slice: 100,
            cpu_time: 0,
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_ppid(mut self, ppid: u32) -> Self {
        self.ppid = ppid;
        self
    }
}

/// A simple round-robin scheduler
pub struct MockScheduler {
    ready_queue: VecDeque<MockProcess>,
    current: Option<MockProcess>,
    next_pid: u32,
    time_quantum: u32,
}

impl Default for MockScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockScheduler {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            current: None,
            next_pid: 1,
            time_quantum: 100,
        }
    }

    pub fn with_time_quantum(mut self, quantum: u32) -> Self {
        self.time_quantum = quantum;
        self
    }

    /// Add a process to the scheduler
    pub fn add_process(&mut self, mut process: MockProcess) {
        process.state = ProcessState::Ready;
        process.time_slice = self.time_quantum;
        self.ready_queue.push_back(process);
    }

    /// Create and add a new process
    pub fn spawn(&mut self, name: &str) -> u32 {
        let pid = self.next_pid;
        self.next_pid += 1;
        
        let process = MockProcess::new(pid, name);
        self.add_process(process);
        pid
    }

    /// Get the currently running process
    pub fn current(&self) -> Option<&MockProcess> {
        self.current.as_ref()
    }

    /// Schedule the next process
    pub fn schedule(&mut self) -> Option<u32> {
        // Put current back in queue if still runnable
        if let Some(mut current) = self.current.take() {
            if current.state == ProcessState::Running {
                current.state = ProcessState::Ready;
                current.time_slice = self.time_quantum;
                self.ready_queue.push_back(current);
            }
        }
        
        // Get next process from queue
        if let Some(mut next) = self.ready_queue.pop_front() {
            next.state = ProcessState::Running;
            let pid = next.pid;
            self.current = Some(next);
            Some(pid)
        } else {
            None
        }
    }

    /// Simulate a timer tick
    pub fn tick(&mut self) -> bool {
        if let Some(ref mut current) = self.current {
            current.cpu_time += 1;
            current.time_slice = current.time_slice.saturating_sub(1);
            
            if current.time_slice == 0 {
                return true; // Need reschedule
            }
        }
        false
    }

    /// Block the current process
    pub fn block_current(&mut self) {
        if let Some(ref mut current) = self.current {
            current.state = ProcessState::Blocked;
        }
    }

    /// Unblock a process by PID
    pub fn unblock(&mut self, _pid: u32) -> bool {
        // In a real implementation, we'd have a blocked queue
        // For simplicity, we just search and add back
        false
    }

    /// Get the number of ready processes
    pub fn ready_count(&self) -> usize {
        self.ready_queue.len()
    }

    /// Check if the scheduler is idle
    pub fn is_idle(&self) -> bool {
        self.current.is_none() && self.ready_queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_basic() {
        let mut sched = MockScheduler::new();
        
        assert!(sched.is_idle());
        
        let pid1 = sched.spawn("process1");
        let pid2 = sched.spawn("process2");
        
        assert_eq!(pid1, 1);
        assert_eq!(pid2, 2);
        assert_eq!(sched.ready_count(), 2);
    }

    #[test]
    fn test_scheduler_round_robin() {
        let mut sched = MockScheduler::new().with_time_quantum(10);
        
        sched.spawn("p1");
        sched.spawn("p2");
        sched.spawn("p3");
        
        // First schedule
        assert_eq!(sched.schedule(), Some(1));
        assert_eq!(sched.current().unwrap().name, "p1");
        
        // Exhaust time slice
        for _ in 0..10 {
            sched.tick();
        }
        
        // Should schedule next
        assert_eq!(sched.schedule(), Some(2));
        assert_eq!(sched.current().unwrap().name, "p2");
    }

    #[test]
    fn test_scheduler_tick() {
        let mut sched = MockScheduler::new().with_time_quantum(5);
        
        sched.spawn("test");
        sched.schedule();
        
        // Tick 4 times - should not need reschedule
        for _ in 0..4 {
            assert!(!sched.tick());
        }
        
        // 5th tick - time slice exhausted
        assert!(sched.tick());
    }

    #[test]
    fn test_scheduler_cpu_time() {
        let mut sched = MockScheduler::new();
        
        sched.spawn("test");
        sched.schedule();
        
        for _ in 0..50 {
            sched.tick();
        }
        
        assert_eq!(sched.current().unwrap().cpu_time, 50);
    }

    #[test]
    fn test_scheduler_block() {
        let mut sched = MockScheduler::new();
        
        sched.spawn("p1");
        sched.spawn("p2");
        
        sched.schedule();
        assert_eq!(sched.current().unwrap().pid, 1);
        
        sched.block_current();
        assert_eq!(sched.current().unwrap().state, ProcessState::Blocked);
        
        // Should not put blocked process back in queue
        sched.schedule();
        assert_eq!(sched.current().unwrap().pid, 2);
    }
}
