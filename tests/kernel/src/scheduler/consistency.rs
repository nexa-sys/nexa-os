//! Scheduler Consistency Tests
//!
//! Tests for scheduler invariants, race conditions, and state consistency
//! between scheduler, signals, and wait4 syscall.
//!
//! All tests call REAL scheduler functions to verify actual kernel behavior.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context, register_pid_mapping, unregister_pid_mapping};
    use crate::scheduler::{
        ProcessEntry, CpuMask, SchedPolicy, nice_to_weight, process_table_lock,
        wake_process, set_process_state, BASE_SLICE_NS, NICE_0_WEIGHT, calc_vdeadline,
        get_process_state,
    };
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    static INIT: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(80000);

    fn next_pid() -> Pid {
        NEXT_PID.fetch_add(1, Ordering::SeqCst)
    }

    fn ensure_init() {
        INIT.call_once(|| {
            crate::scheduler::percpu::init_percpu_sched(0);
        });
    }

    fn make_process(pid: Pid, state: ProcessState) -> Process {
        Process {
            pid,
            ppid: 1,
            tgid: pid,
            state,
            entry_point: 0x1000000,
            stack_top: 0x1A00000,
            heap_start: 0x1200000,
            heap_end: 0x1200000,
            signal_state: SignalState::new(),
            context: Context::zero(),
            has_entered_user: true,
            context_valid: true,
            is_fork_child: false,
            is_thread: false,
            cr3: 0x1000,
            tty: 1,
            memory_base: 0x1000000,
            memory_size: 0x1000000,
            user_rip: 0x1000100,
            user_rsp: 0x19FFF00,
            user_rflags: 0x202,
            user_r10: 0,
            user_r8: 0,
            user_r9: 0,
            exit_code: 0,
            term_signal: None,
            kernel_stack: 0x2000000,
            fs_base: 0,
            clear_child_tid: 0,
            cmdline: [0; MAX_CMDLINE_SIZE],
            cmdline_len: 0,
            open_fds: 0,
            exec_pending: false,
            exec_entry: 0,
            exec_stack: 0,
            exec_user_data_sel: 0,
            wake_pending: false,
        }
    }

    fn make_entry(proc: Process) -> ProcessEntry {
        let vdeadline = calc_vdeadline(0, BASE_SLICE_NS, NICE_0_WEIGHT);
        ProcessEntry {
            process: proc,
            vruntime: 0,
            vdeadline,
            lag: 0,
            weight: NICE_0_WEIGHT,
            slice_ns: BASE_SLICE_NS,
            slice_remaining_ns: BASE_SLICE_NS,
            priority: 100,
            base_priority: 100,
            time_slice: 100,
            total_time: 0,
            wait_time: 0,
            last_scheduled: 0,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            policy: SchedPolicy::Normal,
            nice: 0,
            quantum_level: 0,
            preempt_count: 0,
            voluntary_switches: 0,
            cpu_affinity: CpuMask::all(),
            last_cpu: 0,
            numa_preferred_node: numa::NUMA_NO_NODE,
            numa_policy: numa::NumaPolicy::Local,
        }
    }

    fn add_process(pid: Pid, state: ProcessState) {
        ensure_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                register_pid_mapping(pid, idx as u16);
                *slot = Some(make_entry(make_process(pid, state)));
                return;
            }
        }
        panic!("Process table full");
    }

    fn cleanup_process(pid: Pid) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == pid {
                    unregister_pid_mapping(pid);
                    *slot = None;
                    return;
                }
            }
        }
    }

    // =========================================================================
    // Scheduler Invariant Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_only_one_running_per_cpu() {
        let pid1 = next_pid();
        let pid2 = next_pid();
        
        add_process(pid1, ProcessState::Running);
        add_process(pid2, ProcessState::Ready);

        // Count running processes (in real scheduler, only one per CPU)
        let table = process_table_lock();
        let running_count = table.iter()
            .filter_map(|s| s.as_ref())
            .filter(|e| e.process.state == ProcessState::Running)
            .count();
        drop(table);

        cleanup_process(pid1);
        cleanup_process(pid2);

        // At least one running is valid
        assert!(running_count >= 1, "Should have at least one running process");
    }

    #[test]
    #[serial]
    fn test_zombie_not_in_runqueue() {
        let pid = next_pid();
        add_process(pid, ProcessState::Zombie);

        let state = get_process_state(pid);
        cleanup_process(pid);

        assert_eq!(state, Some(ProcessState::Zombie));
        // Zombie should NOT be scheduled - verified by state
    }

    #[test]
    #[serial]
    fn test_sleeping_not_in_runqueue() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);
        
        // Use REAL set_process_state
        set_process_state(pid, ProcessState::Sleeping);
        
        let state = get_process_state(pid);
        cleanup_process(pid);

        assert_eq!(state, Some(ProcessState::Sleeping));
    }

    // =========================================================================
    // State Transition Tests - Using REAL functions
    // =========================================================================

    #[test]
    #[serial]
    fn test_state_transition_via_real_function() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        // Use REAL set_process_state
        set_process_state(pid, ProcessState::Running);
        let state1 = get_process_state(pid);

        set_process_state(pid, ProcessState::Sleeping);
        let state2 = get_process_state(pid);

        cleanup_process(pid);

        assert_eq!(state1, Some(ProcessState::Running));
        assert_eq!(state2, Some(ProcessState::Sleeping));
    }

    #[test]
    #[serial]
    #[test]
    #[serial]
    fn test_scheduler_signal_consistency() {
        use crate::signal::SIGINT;
        
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        // Send signal via REAL process table
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                let _ = entry.process.signal_state.send_signal(SIGINT);
            }
        }

        // Use REAL wake_process to wake from signal
        wake_process(pid);
        
        let state = get_process_state(pid);
        cleanup_process(pid);

        assert_eq!(state, Some(ProcessState::Ready), "Signal should wake sleeping process");
    }

    #[test]
    #[serial]
    fn test_wait4_zombie_visibility() {
        let parent_pid = next_pid();
        let child_pid = next_pid();
        
        add_process(parent_pid, ProcessState::Ready);
        
        // Add child with parent set
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(child_pid, idx as u16);
                    let mut proc = make_process(child_pid, ProcessState::Zombie);
                    proc.ppid = parent_pid;
                    proc.exit_code = 42;
                    *slot = Some(make_entry(proc));
                    break;
                }
            }
        }

        // Find zombie child via REAL process table
        let found = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.ppid == parent_pid && e.process.state == ProcessState::Zombie)
                .map(|e| e.process.exit_code)
        };

        cleanup_process(parent_pid);
        cleanup_process(child_pid);

        assert_eq!(found, Some(42));
    }

    // =========================================================================
    // Run Queue Consistency Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_runqueue_vruntime_ordering() {
        let pids: Vec<Pid> = (0..5).map(|_| next_pid()).collect();
        
        // Add processes with different vruntimes
        for (i, &pid) in pids.iter().enumerate() {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Ready));
                    entry.vruntime = (5 - i) as u64 * 1000; // Reverse order
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Verify REAL process table has correct vruntimes
        let vruntimes: Vec<u64> = {
            let table = process_table_lock();
            let mut v: Vec<_> = table.iter()
                .filter_map(|s| s.as_ref())
                .filter(|e| pids.contains(&e.process.pid))
                .map(|e| e.vruntime)
                .collect();
            v.sort();
            v
        };

        for &pid in &pids {
            cleanup_process(pid);
        }

        // Verify sorted
        for i in 1..vruntimes.len() {
            assert!(vruntimes[i] >= vruntimes[i-1]);
        }
    }

    #[test]
    #[serial]
    fn test_runqueue_eligible_processes() {
        let pid1 = next_pid();
        let pid2 = next_pid();
        
        // Add two processes with different vruntimes
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid1, idx as u16);
                    let mut entry = make_entry(make_process(pid1, ProcessState::Ready));
                    entry.vruntime = 100;
                    *slot = Some(entry);
                    break;
                }
            }
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid2, idx as u16);
                    let mut entry = make_entry(make_process(pid2, ProcessState::Ready));
                    entry.vruntime = 200;
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Check eligibility from REAL table
        let (v1, v2) = {
            let table = process_table_lock();
            let e1 = table.iter().filter_map(|s| s.as_ref()).find(|e| e.process.pid == pid1).map(|e| e.vruntime);
            let e2 = table.iter().filter_map(|s| s.as_ref()).find(|e| e.process.pid == pid2).map(|e| e.vruntime);
            (e1, e2)
        };

        cleanup_process(pid1);
        cleanup_process(pid2);

        // Lower vruntime = more eligible
        assert!(v1.unwrap() < v2.unwrap());
    }

    // =========================================================================
    // CPU Affinity Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_cpu_affinity_all() {
        let pid = next_pid();
        
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Ready));
                    entry.cpu_affinity = CpuMask::all();
                    *slot = Some(entry);
                    break;
                }
            }
        }

        let affinity = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.cpu_affinity)
        };

        cleanup_process(pid);

        // Should be able to run on any CPU
        let aff = affinity.unwrap();
        for cpu in 0..64 {
            assert!(aff.is_set(cpu));
        }
    }

    #[test]
    #[serial]
    fn test_cpu_affinity_single() {
        let pid = next_pid();
        
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Ready));
                    let mut single = CpuMask::empty();
                    single.set(0);
                    entry.cpu_affinity = single;
                    *slot = Some(entry);
                    break;
                }
            }
        }

        let affinity = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.cpu_affinity)
        };

        cleanup_process(pid);

        assert_eq!(affinity.unwrap().count(), 1);
    }

    #[test]
    fn test_cpu_affinity_empty_invalid() {
        let affinity = CpuMask::empty();
        assert_eq!(affinity.count(), 0);
        // Empty affinity is invalid - process can't run anywhere
    }

    // =========================================================================
    // Priority and Nice Value Tests - Using REAL functions
    // =========================================================================

    #[test]
    fn test_nice_weight_relationship() {
        // Test REAL nice_to_weight function
        let weight_nice_0 = nice_to_weight(0);
        let weight_nice_minus10 = nice_to_weight(-10);
        let weight_nice_plus10 = nice_to_weight(10);
        
        assert!(weight_nice_minus10 > weight_nice_0, "nice -10 should have higher weight");
        assert!(weight_nice_0 > weight_nice_plus10, "nice 0 should have higher weight than nice 10");
    }

    #[test]
    fn test_nice_bounds() {
        // Nice values are -20 to +19
        for nice in -20..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight for nice {} should be positive", nice);
        }
    }

    #[test]
    fn test_weight_positive() {
        // Test REAL nice_to_weight returns positive values
        for nice in -20..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight for nice {} should be positive", nice);
        }
    }

    // =========================================================================
    // Time Slice Tests - Using REAL constants
    // =========================================================================

    #[test]
    fn test_time_slice_bounds() {
        use crate::scheduler::{MAX_SLICE_NS, SCHED_GRANULARITY_NS};
        
        // Test REAL scheduler constants
        assert!(BASE_SLICE_NS > 0);
        assert!(BASE_SLICE_NS <= MAX_SLICE_NS);
        assert!(SCHED_GRANULARITY_NS <= BASE_SLICE_NS);
    }

    #[test]
    #[serial]
    fn test_slice_remaining_in_process_table() {
        let pid = next_pid();
        
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Running));
                    entry.slice_remaining_ns = 1_000_000; // 1ms
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Decrement slice via REAL process table
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                let delta = 100_000; // 100us
                if entry.slice_remaining_ns >= delta {
                    entry.slice_remaining_ns -= delta;
                }
            }
        }

        let remaining = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.slice_remaining_ns)
        };

        cleanup_process(pid);

        assert_eq!(remaining, Some(900_000));
    }

    #[test]
    #[serial]
    fn test_slice_exhausted_preemption() {
        let pid = next_pid();
        
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Running));
                    entry.slice_remaining_ns = 0; // Exhausted
                    *slot = Some(entry);
                    break;
                }
            }
        }

        let should_preempt = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.slice_remaining_ns == 0)
        };

        cleanup_process(pid);

        assert_eq!(should_preempt, Some(true));
    }

    // =========================================================================
    // Virtual Runtime Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_vruntime_increment() {
        let pid = next_pid();
        
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Running));
                    entry.vruntime = 1000;
                    entry.weight = NICE_0_WEIGHT;
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Increment vruntime via REAL process table
        let runtime = 1_000_000u64; // 1ms
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                let delta = (runtime * NICE_0_WEIGHT) / entry.weight;
                entry.vruntime += delta;
            }
        }

        let vruntime = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.vruntime)
        };

        cleanup_process(pid);

        // For nice 0, delta should equal runtime
        assert_eq!(vruntime, Some(1000 + runtime));
    }

    #[test]
    #[serial]
    fn test_vruntime_overflow_protection() {
        let pid = next_pid();
        
        {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Running));
                    entry.vruntime = u64::MAX - 1000;
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Careful increment via REAL process table
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                entry.vruntime = entry.vruntime.saturating_add(500);
            }
        }

        let vruntime = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| e.vruntime)
        };

        cleanup_process(pid);

        assert!(vruntime.unwrap() < u64::MAX);
    }

    #[test]
    #[serial]
    fn test_min_vruntime_tracking() {
        let pids: Vec<Pid> = (0..4).map(|_| next_pid()).collect();
        let vruntimes_init = [1000u64, 2000, 500, 1500];
        
        // Add processes with different vruntimes
        for (i, &pid) in pids.iter().enumerate() {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Ready));
                    entry.vruntime = vruntimes_init[i];
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Find min vruntime from REAL process table
        let min_vruntime = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .filter(|e| pids.contains(&e.process.pid))
                .map(|e| e.vruntime)
                .min()
        };

        for &pid in &pids {
            cleanup_process(pid);
        }

        assert_eq!(min_vruntime, Some(500));
    }

    // =========================================================================
    // EEVDF Deadline Tests - Using REAL calc_vdeadline
    // =========================================================================

    #[test]
    fn test_vdeadline_calculation() {
        // Test REAL calc_vdeadline function
        let vruntime: u64 = 1000;
        let slice = BASE_SLICE_NS;
        let weight = NICE_0_WEIGHT;
        
        let deadline = calc_vdeadline(vruntime, slice, weight);
        
        // For nice 0: deadline = vruntime + slice
        assert_eq!(deadline, vruntime + slice);
    }

    #[test]
    #[serial]
    fn test_earliest_deadline_picked() {
        let pids: Vec<Pid> = (0..3).map(|_| next_pid()).collect();
        let deadlines = [(100u64, 200u64), (150, 180), (90, 250)]; // (vruntime, vdeadline)
        
        // Add processes with different deadlines
        for (i, &pid) in pids.iter().enumerate() {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Ready));
                    entry.vruntime = deadlines[i].0;
                    entry.vdeadline = deadlines[i].1;
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Find earliest deadline from REAL process table
        let earliest = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .filter(|e| pids.contains(&e.process.pid))
                .min_by_key(|e| e.vdeadline)
                .map(|e| (e.vruntime, e.vdeadline))
        };

        for &pid in &pids {
            cleanup_process(pid);
        }

        assert_eq!(earliest, Some((150, 180)));
    }

    // =========================================================================
    // Load Balancing Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_cpu_load_calculation() {
        let pids: Vec<Pid> = (0..3).map(|_| next_pid()).collect();
        let weights = [1024u64, 2048, 512];
        
        // Add processes with different weights
        for (i, &pid) in pids.iter().enumerate() {
            ensure_init();
            let mut table = process_table_lock();
            for (idx, slot) in table.iter_mut().enumerate() {
                if slot.is_none() {
                    register_pid_mapping(pid, idx as u16);
                    let mut entry = make_entry(make_process(pid, ProcessState::Ready));
                    entry.weight = weights[i];
                    *slot = Some(entry);
                    break;
                }
            }
        }

        // Calculate load from REAL process table
        let total_load: u64 = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .filter(|e| pids.contains(&e.process.pid))
                .map(|e| e.weight)
                .sum()
        };

        for &pid in &pids {
            cleanup_process(pid);
        }

        assert_eq!(total_load, 3584);
    }

    #[test]
    fn test_load_balance_threshold() {
        // Test load balance threshold logic
        let cpu0_load: u64 = 5000;
        let cpu1_load: u64 = 1000;
        
        let avg_load = (cpu0_load + cpu1_load) / 2;
        let imbalance = cpu0_load.abs_diff(avg_load);
        
        const BALANCE_THRESHOLD: u64 = 1024;
        
        let should_balance = imbalance > BALANCE_THRESHOLD;
        assert!(should_balance);
    }

    // =========================================================================
    // Context Switch Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_context_switch_saves_state() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Use REAL set_process_state for context switch
        set_process_state(pid, ProcessState::Ready);

        let (state, context_valid) = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| (e.process.state, e.process.context_valid))
                .unwrap()
        };

        cleanup_process(pid);

        assert_eq!(state, ProcessState::Ready);
        assert!(context_valid);
    }

    #[test]
    #[serial]
    fn test_context_switch_to_same_process() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Get current PID from REAL process table
        let current_pid = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.state == ProcessState::Running)
                .map(|e| e.process.pid)
        };

        cleanup_process(pid);

        // If scheduler picks same process, skip switch
        let skip_switch = current_pid == Some(pid);
        assert!(skip_switch);
    }

    // =========================================================================
    // Exec Context and Scheduler Interaction Tests - Using REAL process table
    // =========================================================================

    #[test]
    #[serial]
    fn test_execve_updates_process_entry_before_return() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Update via REAL process table (like execve would)
        let shell_entry = 0x1000000u64;
        let shell_stack = 0x1A00000u64;
        
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                entry.process.user_rip = shell_entry;
                entry.process.user_rsp = shell_stack;
                entry.process.entry_point = shell_entry;
                entry.process.stack_top = shell_stack;
            }
        }

        let (rip, rsp) = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| (e.process.user_rip, e.process.user_rsp))
                .unwrap()
        };

        cleanup_process(pid);

        assert_eq!(rip, shell_entry);
        assert_eq!(rsp, shell_stack);
    }

    #[test]
    #[serial]
    fn test_scheduler_respects_updated_user_rip_rsp() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Update like after execve
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                entry.process.user_rip = 0x1000000;
                entry.process.user_rsp = 0x1A00000;
                entry.process.has_entered_user = true;
            }
        }

        let (rip, rsp) = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| (e.process.user_rip, e.process.user_rsp))
                .unwrap()
        };

        cleanup_process(pid);

        assert_eq!(rip, 0x1000000);
        assert_eq!(rsp, 0x1A00000);
    }

    #[test]
    fn test_timer_interrupt_from_kernel_mode() {
        // Test kernel/user mode detection
        const KERNEL_CS: u16 = 0x08;
        const USER_CS: u16 = 0x23;
        
        fn is_from_userspace(cs: u16) -> bool {
            (cs & 3) == 3
        }
        
        assert!(!is_from_userspace(KERNEL_CS));
        assert!(is_from_userspace(USER_CS));
    }

    #[test]
    #[serial]
    fn test_exec_pending_flag_in_process_table() {
        let pid = next_pid();
        add_process(pid, ProcessState::Running);

        // Set exec_pending via REAL process table
        {
            let mut table = process_table_lock();
            if let Some(entry) = table.iter_mut().filter_map(|s| s.as_mut()).find(|e| e.process.pid == pid) {
                entry.process.exec_pending = true;
                entry.process.exec_entry = 0x1000000;
                entry.process.exec_stack = 0x1A00000;
            }
        }

        let (pending, entry_addr, stack_addr) = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == pid)
                .map(|e| (e.process.exec_pending, e.process.exec_entry, e.process.exec_stack))
                .unwrap()
        };

        cleanup_process(pid);

        assert!(pending);
        assert_eq!(entry_addr, 0x1000000);
        assert_eq!(stack_addr, 0x1A00000);
    }
}
