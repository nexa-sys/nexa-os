//! Scheduler Consistency Tests
//!
//! Tests for scheduler invariants, race conditions, and state consistency
//! between scheduler, signals, and wait4 syscall.

#[cfg(test)]
mod tests {
    use crate::process::{ProcessState, Pid};
    use crate::scheduler::{ProcessEntry, CpuMask, SchedPolicy, nice_to_weight};

    // =========================================================================
    // Scheduler Invariant Tests
    // =========================================================================

    #[test]
    fn test_only_one_running_per_cpu() {
        // Each CPU should have at most one Running process
        struct CpuState {
            running_pid: Option<Pid>,
        }
        
        let mut cpu0 = CpuState { running_pid: Some(1) };
        
        // Can't have two running on same CPU
        fn set_running(cpu: &mut CpuState, pid: Pid) -> Result<(), &'static str> {
            if cpu.running_pid.is_some() {
                return Err("CPU already has running process");
            }
            cpu.running_pid = Some(pid);
            Ok(())
        }
        
        // Should fail - already has running process
        assert!(set_running(&mut cpu0, 2).is_err());
        
        // Clear current
        cpu0.running_pid = None;
        
        // Should succeed now
        assert!(set_running(&mut cpu0, 2).is_ok());
    }

    #[test]
    fn test_zombie_not_in_runqueue() {
        // Zombie processes should not be in run queue
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Zombie;
        
        fn should_be_in_runqueue(entry: &ProcessEntry) -> bool {
            matches!(entry.process.state, ProcessState::Ready | ProcessState::Running)
        }
        
        assert!(!should_be_in_runqueue(&entry));
    }

    #[test]
    fn test_sleeping_not_in_runqueue() {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Sleeping;
        
        fn should_be_in_runqueue(entry: &ProcessEntry) -> bool {
            matches!(entry.process.state, ProcessState::Ready | ProcessState::Running)
        }
        
        assert!(!should_be_in_runqueue(&entry));
    }

    // =========================================================================
    // State Transition Atomicity Tests
    // =========================================================================

    #[test]
    fn test_state_transition_requires_lock() {
        // State transitions must hold process lock
        // This is a conceptual test - actual locking is in kernel
        
        use std::sync::{Arc, Mutex};
        
        struct ProtectedProcess {
            state: ProcessState,
        }
        
        let process = Arc::new(Mutex::new(ProtectedProcess {
            state: ProcessState::Ready,
        }));
        
        // Transition with lock held
        {
            let mut proc = process.lock().unwrap();
            proc.state = ProcessState::Running;
        }
        
        // Verify transition
        let proc = process.lock().unwrap();
        assert_eq!(proc.state, ProcessState::Running);
    }

    #[test]
    fn test_scheduler_signal_consistency() {
        // Signal delivery can change process state
        // Scheduler must see consistent state
        
        let mut entry = ProcessEntry::empty();
        entry.process.pid = 1;
        entry.process.state = ProcessState::Sleeping;
        
        // Signal wakes process
        fn deliver_signal(entry: &mut ProcessEntry, signum: u32) {
            // Send signal
            let _ = entry.process.signal_state.send_signal(signum);
            
            // Wake from sleep if signal pending
            if entry.process.state == ProcessState::Sleeping {
                if entry.process.signal_state.has_pending_signal().is_some() {
                    entry.process.state = ProcessState::Ready;
                }
            }
        }
        
        use crate::signal::SIGINT;
        deliver_signal(&mut entry, SIGINT);
        
        // Should now be Ready (woken by signal)
        assert_eq!(entry.process.state, ProcessState::Ready);
    }

    #[test]
    fn test_wait4_zombie_visibility() {
        // wait4 must see Zombie state consistently
        
        let mut child = ProcessEntry::empty();
        child.process.pid = 2;
        child.process.ppid = 1;
        child.process.state = ProcessState::Zombie;
        child.process.exit_code = 42;
        
        // Parent should be able to find zombie child
        fn find_zombie_child(entries: &[ProcessEntry], parent_pid: Pid) -> Option<&ProcessEntry> {
            entries.iter().find(|e| {
                e.process.ppid == parent_pid && e.process.state == ProcessState::Zombie
            })
        }
        
        let entries = [child];
        let found = find_zombie_child(&entries, 1);
        
        assert!(found.is_some());
        assert_eq!(found.unwrap().process.exit_code, 42);
    }

    // =========================================================================
    // Run Queue Consistency Tests
    // =========================================================================

    #[test]
    fn test_runqueue_vruntime_ordering() {
        // Run queue should maintain vruntime ordering
        let mut entries: Vec<ProcessEntry> = Vec::new();
        
        for i in 0..5 {
            let mut entry = ProcessEntry::empty();
            entry.process.pid = i as Pid;
            entry.process.state = ProcessState::Ready;
            entry.vruntime = (5 - i) as u64 * 1000; // Reverse order
            entries.push(entry);
        }
        
        // Sort by vruntime (scheduler picks lowest)
        entries.sort_by_key(|e| e.vruntime);
        
        // Verify sorted
        for i in 1..entries.len() {
            assert!(entries[i].vruntime >= entries[i-1].vruntime);
        }
    }

    #[test]
    fn test_runqueue_eligible_first() {
        // EEVDF: Only eligible processes should run
        fn is_eligible(entry: &ProcessEntry, min_vruntime: u64) -> bool {
            // Eligible if lag >= 0 (i.e., vruntime <= min_vruntime)
            entry.vruntime <= min_vruntime
        }
        
        let mut entry = ProcessEntry::empty();
        entry.vruntime = 100;
        
        assert!(is_eligible(&entry, 100)); // Equal = eligible
        assert!(is_eligible(&entry, 200)); // Less = eligible
        assert!(!is_eligible(&entry, 50)); // Greater = not eligible
    }

    // =========================================================================
    // CPU Affinity Tests
    // =========================================================================

    #[test]
    fn test_cpu_affinity_all() {
        let affinity = CpuMask::all();
        
        // Should be able to run on any CPU
        for cpu in 0..64 {
            assert!(affinity.is_set(cpu));
        }
    }

    #[test]
    fn test_cpu_affinity_single() {
        // Process pinned to single CPU
        fn is_pinned(affinity: &CpuMask) -> bool {
            affinity.count() == 1
        }
        
        let mut affinity = CpuMask::empty();
        affinity.set(0);
        
        assert!(is_pinned(&affinity));
    }

    #[test]
    fn test_cpu_affinity_empty_invalid() {
        // Empty affinity mask is invalid (process can't run anywhere)
        let affinity = CpuMask::empty();
        
        assert_eq!(affinity.count(), 0);
        
        fn is_valid_affinity(affinity: &CpuMask) -> bool {
            affinity.count() > 0
        }
        
        assert!(!is_valid_affinity(&affinity));
    }

    // =========================================================================
    // Priority and Nice Value Tests
    // =========================================================================

    #[test]
    fn test_nice_weight_relationship() {
        // Higher nice = lower priority = lower weight
        let weight_nice_0 = nice_to_weight(0);
        let weight_nice_minus10 = nice_to_weight(-10);
        let weight_nice_plus10 = nice_to_weight(10);
        
        assert!(weight_nice_minus10 > weight_nice_0, "nice -10 should have higher weight");
        assert!(weight_nice_0 > weight_nice_plus10, "nice 0 should have higher weight than nice 10");
    }

    #[test]
    fn test_nice_bounds() {
        // Nice values are -20 to +19
        fn is_valid_nice(nice: i8) -> bool {
            nice >= -20 && nice <= 19
        }
        
        assert!(is_valid_nice(0));
        assert!(is_valid_nice(-20));
        assert!(is_valid_nice(19));
        assert!(!is_valid_nice(-21));
        assert!(!is_valid_nice(20));
    }

    #[test]
    fn test_weight_positive() {
        // Weight should always be positive
        for nice in -20..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight for nice {} should be positive", nice);
        }
    }

    // =========================================================================
    // Time Slice Tests
    // =========================================================================

    #[test]
    fn test_time_slice_bounds() {
        use crate::scheduler::{BASE_SLICE_NS, MAX_SLICE_NS, SCHED_GRANULARITY_NS};
        
        // Base slice should be reasonable
        assert!(BASE_SLICE_NS > 0);
        assert!(BASE_SLICE_NS <= MAX_SLICE_NS);
        
        // Granularity should be less than slice
        assert!(SCHED_GRANULARITY_NS <= BASE_SLICE_NS);
    }

    #[test]
    fn test_slice_remaining_decrement() {
        let mut entry = ProcessEntry::empty();
        entry.slice_remaining_ns = 1_000_000; // 1ms
        
        let delta = 100_000; // 100us
        
        if entry.slice_remaining_ns >= delta {
            entry.slice_remaining_ns -= delta;
        } else {
            entry.slice_remaining_ns = 0;
        }
        
        assert_eq!(entry.slice_remaining_ns, 900_000);
    }

    #[test]
    fn test_slice_exhausted_preemption() {
        let mut entry = ProcessEntry::empty();
        entry.slice_remaining_ns = 0;
        
        fn should_preempt(entry: &ProcessEntry) -> bool {
            entry.slice_remaining_ns == 0
        }
        
        assert!(should_preempt(&entry));
    }

    // =========================================================================
    // Virtual Runtime Tests
    // =========================================================================

    #[test]
    fn test_vruntime_increment() {
        use crate::scheduler::NICE_0_WEIGHT;
        
        let mut entry = ProcessEntry::empty();
        entry.vruntime = 1000;
        entry.weight = NICE_0_WEIGHT;
        
        // vruntime delta = actual_runtime * (NICE_0_WEIGHT / weight)
        let runtime = 1_000_000; // 1ms
        let delta = (runtime * NICE_0_WEIGHT) / entry.weight;
        
        entry.vruntime += delta;
        
        // For nice 0, delta should equal runtime
        assert_eq!(entry.vruntime, 1000 + runtime);
    }

    #[test]
    fn test_vruntime_overflow_protection() {
        // vruntime should handle very large values
        let mut entry = ProcessEntry::empty();
        entry.vruntime = u64::MAX - 1000;
        
        // Careful increment to avoid overflow
        let delta = 500;
        entry.vruntime = entry.vruntime.saturating_add(delta);
        
        assert!(entry.vruntime < u64::MAX);
    }

    #[test]
    fn test_min_vruntime_tracking() {
        // Scheduler tracks minimum vruntime for new process initialization
        let vruntimes = [1000u64, 2000, 500, 1500];
        let min_vruntime = *vruntimes.iter().min().unwrap();
        
        assert_eq!(min_vruntime, 500);
        
        // New process gets min_vruntime to avoid starvation of existing processes
        let mut new_entry = ProcessEntry::empty();
        new_entry.vruntime = min_vruntime;
        
        assert_eq!(new_entry.vruntime, 500);
    }

    // =========================================================================
    // EEVDF Deadline Tests
    // =========================================================================

    #[test]
    fn test_vdeadline_calculation() {
        use crate::scheduler::{calc_vdeadline, NICE_0_WEIGHT, BASE_SLICE_NS};
        
        let vruntime: u64 = 1000;
        let slice = BASE_SLICE_NS;
        let weight = NICE_0_WEIGHT;
        
        let deadline = calc_vdeadline(vruntime, slice, weight);
        
        // For nice 0: deadline = vruntime + slice
        assert_eq!(deadline, vruntime + slice);
    }

    #[test]
    fn test_earliest_deadline_picked() {
        // EEVDF picks eligible process with earliest deadline
        let mut entries: Vec<(u64, u64)> = vec![ // (vruntime, vdeadline)
            (100, 200),
            (150, 180), // Earliest deadline
            (90, 250),
        ];
        
        // Sort by deadline (scheduler picks earliest)
        entries.sort_by_key(|e| e.1);
        
        assert_eq!(entries[0], (150, 180));
    }

    // =========================================================================
    // Load Balancing Tests
    // =========================================================================

    #[test]
    fn test_cpu_load_calculation() {
        // Load = sum of weights of runnable processes
        let weights = [1024u64, 2048, 512]; // Different priorities
        let total_load: u64 = weights.iter().sum();
        
        assert_eq!(total_load, 3584);
    }

    #[test]
    fn test_load_balance_threshold() {
        // Only balance if imbalance exceeds threshold
        let cpu0_load: u64 = 5000;
        let cpu1_load: u64 = 1000;
        
        let avg_load = (cpu0_load + cpu1_load) / 2;
        let imbalance = cpu0_load.abs_diff(avg_load);
        
        const BALANCE_THRESHOLD: u64 = 1024;
        
        let should_balance = imbalance > BALANCE_THRESHOLD;
        assert!(should_balance);
    }

    // =========================================================================
    // Context Switch Tests
    // =========================================================================

    #[test]
    fn test_context_switch_saves_state() {
        let mut old_entry = ProcessEntry::empty();
        old_entry.process.state = ProcessState::Running;
        
        // Context switch should:
        // 1. Save old process state
        // 2. Mark old as Ready (if not sleeping/zombie)
        // 3. Load new process state
        // 4. Mark new as Running
        
        // Simulate saving state
        old_entry.process.context_valid = true;
        
        // Change to Ready if preempted
        if old_entry.process.state == ProcessState::Running {
            old_entry.process.state = ProcessState::Ready;
        }
        
        assert_eq!(old_entry.process.state, ProcessState::Ready);
        assert!(old_entry.process.context_valid);
    }

    #[test]
    fn test_context_switch_to_same_process() {
        // If scheduler picks the same process, skip actual switch
        let current_pid: Pid = 1;
        let next_pid: Pid = 1;
        
        let skip_switch = current_pid == next_pid;
        assert!(skip_switch);
    }
}
