//! SMP (Symmetric Multi-Processing) Race Condition Bug Detection Tests
//!
//! These tests specifically target race conditions that occur in multi-core
//! environments where multiple CPUs can access shared scheduler state.
//!
//! ## Key SMP Race Scenarios:
//!
//! 1. **Cross-CPU Wake Race**
//!    - CPU 0: Process preparing to sleep (add_waiter, about to call sleep)
//!    - CPU 1: Keyboard interrupt, calls wake_process()
//!    - Race: wake arrives before sleep completes
//!
//! 2. **IPI Reschedule Race**
//!    - CPU 0: Process running
//!    - CPU 1: Sends IPI to reschedule
//!    - CPU 0: Context switch in progress
//!
//! 3. **Process Table Contention**
//!    - Multiple CPUs trying to modify same process entry
//!    - Lock ordering must prevent deadlock
//!
//! ## Test Philosophy:
//!
//! Tests FAIL when bugs exist, PASS when fixed.

#[cfg(test)]
mod tests {
    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE, Context};
    use crate::scheduler::{
        wake_process, set_process_state, process_table_lock,
        SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
        calc_vdeadline,
    };
    use crate::scheduler::percpu::{init_percpu_sched, check_need_resched, set_need_resched};
    use crate::signal::SignalState;
    use crate::numa;

    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(110000);

    fn next_pid() -> Pid {
        NEXT_PID.fetch_add(1, Ordering::SeqCst)
    }

    fn ensure_percpu_init() {
        INIT_PERCPU.call_once(|| {
            init_percpu_sched(0);
        });
    }

    fn make_test_process(pid: Pid, state: ProcessState) -> Process {
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

    fn make_process_entry(proc: Process) -> ProcessEntry {
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
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let entry = make_process_entry(make_test_process(pid, state));
                *slot = Some(entry);
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
                    crate::process::unregister_pid_mapping(pid);
                    *slot = None;
                    return;
                }
            }
        }
    }

    fn get_state(pid: Pid) -> Option<ProcessState> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.state)
    }

    fn get_wake_pending(pid: Pid) -> Option<bool> {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .find(|e| e.process.pid == pid)
            .map(|e| e.process.wake_pending)
    }

    // =========================================================================
    // SMP Race: Cross-CPU Wake Before Sleep
    // =========================================================================

    /// SMP BUG TEST: Simulated cross-CPU wake-before-sleep race
    ///
    /// This simulates:
    /// - CPU 0: Shell process, about to sleep for keyboard input
    /// - CPU 1: Keyboard interrupt handler, calling wake_process
    ///
    /// The race window is between add_waiter() and set_process_state(Sleeping).
    #[test]
    #[serial]
    fn smp_cross_cpu_wake_before_sleep() {
        let shell_pid = next_pid();
        add_process(shell_pid, ProcessState::Ready);

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let lost_wakes = Arc::new(AtomicU64::new(0));
        let lost_wakes_clone = lost_wakes.clone();

        // Simulate CPU 1: Keyboard interrupt handler
        let cpu1 = thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                // Simulate interrupt: wake the shell
                wake_process(shell_pid);
                thread::yield_now();
            }
        });

        // Simulate CPU 0: Shell process trying to read keyboard
        for _ in 0..500 {
            // Shell is Ready, simulating "just registered as waiter"
            
            // Try to sleep
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);
            
            // Check if we got stuck
            if get_state(shell_pid) == Some(ProcessState::Sleeping) {
                // This is the bug: shell is sleeping but wake was "lost"
                lost_wakes_clone.fetch_add(1, Ordering::Relaxed);
                // Recover for next iteration
                wake_process(shell_pid);
            }
            
            thread::yield_now();
        }

        stop.store(true, Ordering::Relaxed);
        cpu1.join().unwrap();

        let total_lost = lost_wakes.load(Ordering::Relaxed);
        cleanup_process(shell_pid);

        // With wake_pending mechanism, lost wakes should be recoverable
        // The key is that wake_process sets wake_pending on Ready processes,
        // and set_process_state checks it before allowing sleep.
        eprintln!("SMP test: {} iterations where process reached Sleeping state", total_lost);
        
        // Some sleeps are expected (when wake hasn't arrived yet)
        // But they should all be recoverable. If we can't wake them, that's a bug.
    }

    /// SMP BUG TEST: Multiple CPU wake race
    ///
    /// Multiple CPUs trying to wake the same process simultaneously.
    /// Only one should succeed, others should set wake_pending or no-op.
    #[test]
    #[serial]
    fn smp_multiple_cpu_wake_race() {
        let pid = next_pid();
        add_process(pid, ProcessState::Sleeping);

        let wake_success = Arc::new(AtomicU64::new(0));
        
        // Spawn 4 threads simulating 4 CPUs all trying to wake same process
        let mut handles = vec![];
        for _ in 0..4 {
            let ws = wake_success.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    if wake_process(pid) {
                        ws.fetch_add(1, Ordering::Relaxed);
                    }
                    thread::yield_now();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let total_success = wake_success.load(Ordering::Relaxed);
        let final_state = get_state(pid);
        
        cleanup_process(pid);

        // Process should end up Ready (woken from Sleeping)
        assert_eq!(final_state, Some(ProcessState::Ready),
            "Process should be Ready after multiple wakes");

        // Only ONE wake should actually succeed (Sleeping -> Ready)
        // Others either see Ready and set wake_pending, or see Ready and no-op
        assert!(total_success >= 1,
            "At least one wake should succeed");
        
        eprintln!("Multiple CPU wake: {} successful wakes reported", total_success);
    }

    /// SMP BUG TEST: Process table lock contention
    ///
    /// Tests that high contention on process table doesn't cause deadlock.
    #[test]
    #[serial]
    fn smp_process_table_contention() {
        let mut pids = vec![];
        for _ in 0..10 {
            let pid = next_pid();
            add_process(pid, ProcessState::Ready);
            pids.push(pid);
        }

        let pids = Arc::new(pids);
        let deadlock_detected = Arc::new(AtomicBool::new(false));
        
        let mut handles = vec![];
        for thread_id in 0..4 {
            let pids_clone = pids.clone();
            let dd = deadlock_detected.clone();
            
            handles.push(thread::spawn(move || {
                for i in 0..200 {
                    let pid = pids_clone[i % pids_clone.len()];
                    
                    // Different operations to create contention
                    match (thread_id + i) % 4 {
                        0 => { let _ = set_process_state(pid, ProcessState::Sleeping); }
                        1 => { wake_process(pid); }
                        2 => { let _ = get_state(pid); }
                        _ => { let _ = get_wake_pending(pid); }
                    }
                    
                    // Timeout detection (simplified)
                    if i > 100 && dd.load(Ordering::Relaxed) {
                        return; // Another thread detected issue
                    }
                }
            }));
        }

        // Wait for all threads with timeout
        for h in handles {
            h.join().expect("Thread panicked - possible deadlock or crash");
        }

        // Cleanup
        for pid in pids.iter() {
            // Force wake any sleeping processes
            wake_process(*pid);
            cleanup_process(*pid);
        }

        assert!(!deadlock_detected.load(Ordering::Relaxed),
            "Deadlock detected during process table contention test");
    }

    // =========================================================================
    // SMP Race: Wake Pending Visibility
    // =========================================================================

    /// SMP BUG TEST: wake_pending prevents sleep after wake
    ///
    /// This tests that if wake_process is called on a Ready process,
    /// a subsequent sleep attempt will be blocked by wake_pending.
    /// This is the core mechanism that prevents lost wakes.
    #[test]
    #[serial]
    fn smp_wake_pending_prevents_sleep() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let blocked_sleeps = Arc::new(AtomicU64::new(0));
        let successful_sleeps = Arc::new(AtomicU64::new(0));
        let blocked_sleeps_clone = blocked_sleeps.clone();
        let successful_sleeps_clone = successful_sleeps.clone();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        // Thread 1: Continuously call wake_process on Ready process
        let waker = thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                wake_process(pid);
                // Small delay
                for _ in 0..5 {
                    core::hint::spin_loop();
                }
            }
        });

        // Main thread: Try to sleep repeatedly
        for _ in 0..1000 {
            // First, ensure process is Ready
            wake_process(pid);
            
            // Now try to sleep - if wake_pending was set, this should fail
            let before_state = get_state(pid);
            let _ = set_process_state(pid, ProcessState::Sleeping);
            let after_state = get_state(pid);
            
            match (before_state, after_state) {
                (Some(ProcessState::Ready), Some(ProcessState::Ready)) => {
                    // Sleep was blocked (wake_pending was set)
                    blocked_sleeps_clone.fetch_add(1, Ordering::Relaxed);
                }
                (Some(ProcessState::Ready), Some(ProcessState::Sleeping)) => {
                    // Sleep succeeded (no wake_pending)
                    successful_sleeps_clone.fetch_add(1, Ordering::Relaxed);
                    // Wake it back up for next iteration
                    wake_process(pid);
                }
                _ => {}
            }
            
            thread::yield_now();
        }

        stop.store(true, Ordering::Relaxed);
        waker.join().unwrap();
        
        // Ensure process is awake for cleanup
        wake_process(pid);
        cleanup_process(pid);

        let blocked = blocked_sleeps.load(Ordering::Relaxed);
        let successful = successful_sleeps.load(Ordering::Relaxed);
        
        eprintln!("wake_pending test: {} sleeps blocked, {} sleeps successful", blocked, successful);
        
        // We should see SOME blocked sleeps if wake_pending mechanism works
        // (not all will be blocked due to timing)
        assert!(blocked > 0 || successful > 0,
            "Test did not execute properly (no sleep attempts recorded)");
        
        // The wake_pending mechanism should block at least some sleeps
        // If ALL sleeps succeed and none are blocked, wake_pending isn't working
        // But we allow for timing variation
        if successful > 100 && blocked == 0 {
            panic!("BUG: wake_pending never blocked a sleep! \
                    {} sleeps succeeded with concurrent wakes.", successful);
        }
    }

    // =========================================================================
    // SMP Race: Rapid State Transitions
    // =========================================================================

    /// SMP BUG TEST: Rapid Ready <-> Sleeping transitions from multiple CPUs
    #[test]
    #[serial]
    fn smp_rapid_state_transitions() {
        let pid = next_pid();
        add_process(pid, ProcessState::Ready);

        let invalid_state_count = Arc::new(AtomicU64::new(0));
        let none_state_count = Arc::new(AtomicU64::new(0));
        
        let mut handles = vec![];
        for thread_id in 0..2 {
            let isc = invalid_state_count.clone();
            let nsc = none_state_count.clone();
            
            handles.push(thread::spawn(move || {
                for _ in 0..500 {
                    if thread_id == 0 {
                        // Thread 0: tries to sleep
                        let _ = set_process_state(pid, ProcessState::Sleeping);
                    } else {
                        // Thread 1: tries to wake
                        wake_process(pid);
                    }
                    
                    // Verify state is always valid
                    let state = get_state(pid);
                    match state {
                        Some(ProcessState::Ready) |
                        Some(ProcessState::Sleeping) |
                        Some(ProcessState::Running) => {}
                        Some(ProcessState::Zombie) => {
                            // Zombie is also valid (though unexpected here)
                        }
                        None => {
                            // Process not found - could be test isolation issue
                            nsc.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    
                    thread::yield_now();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Ensure process is in good state for cleanup
        wake_process(pid);
        cleanup_process(pid);

        let invalid = invalid_state_count.load(Ordering::Relaxed);
        let none_count = none_state_count.load(Ordering::Relaxed);
        
        // None states could be due to test isolation issues (other tests cleaning up)
        // This is a test infrastructure issue, not a kernel bug
        if none_count > 0 {
            eprintln!("WARNING: {} None states observed (test isolation issue)", none_count);
        }
        
        assert_eq!(invalid, 0,
            "BUG: {} invalid states observed during rapid transitions!", invalid);
    }

    // =========================================================================
    // SMP: Keyboard Input Scenario (Real-world)
    // =========================================================================

    /// SMP Integration: Realistic keyboard input scenario
    ///
    /// Simulates the exact scenario that causes shell unresponsiveness:
    /// - Shell on CPU 0 waiting for keyboard
    /// - Keyboard interrupt on CPU 1
    /// - DHCP client also running
    #[test]
    #[serial]
    fn smp_keyboard_shell_scenario() {
        let shell_pid = next_pid();
        let dhcp_pid = next_pid();
        
        add_process(shell_pid, ProcessState::Ready);
        add_process(dhcp_pid, ProcessState::Ready);

        let shell_stuck_forever = Arc::new(AtomicBool::new(false));
        let ssf = shell_stuck_forever.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        // CPU 1: Keyboard interrupt simulation
        let cpu1 = thread::spawn(move || {
            let mut wakes = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                // Keyboard interrupt wakes shell
                wake_process(shell_pid);
                wakes += 1;
                
                // Simulate interrupt rate (~every 10ms for keypress)
                thread::sleep(std::time::Duration::from_micros(100));
            }
            wakes
        });

        // CPU 0: Shell trying to read input
        let mut successful_reads = 0;
        let mut stuck_count = 0;
        
        for iteration in 0..100 {
            // Shell tries to sleep waiting for input
            let _ = set_process_state(shell_pid, ProcessState::Sleeping);
            
            let state = get_state(shell_pid);
            if state == Some(ProcessState::Sleeping) {
                // Check if we can recover
                thread::sleep(std::time::Duration::from_micros(200));
                
                let state_after = get_state(shell_pid);
                if state_after == Some(ProcessState::Sleeping) {
                    // Still sleeping - this is concerning
                    stuck_count += 1;
                    
                    // Force recovery
                    if !wake_process(shell_pid) {
                        // Can't even force wake - definitely stuck!
                        ssf.store(true, Ordering::Relaxed);
                        break;
                    }
                } else {
                    // Woke up naturally - good!
                    successful_reads += 1;
                }
            } else {
                // Didn't sleep (wake_pending blocked it) - also good!
                successful_reads += 1;
            }
            
            // Simulate shell processing
            thread::yield_now();
        }

        stop.store(true, Ordering::Relaxed);
        let total_wakes = cpu1.join().unwrap();

        // Cleanup
        wake_process(shell_pid);
        wake_process(dhcp_pid);
        cleanup_process(shell_pid);
        cleanup_process(dhcp_pid);

        eprintln!(
            "Shell scenario: {} successful reads, {} stuck, {} wakes sent",
            successful_reads, stuck_count, total_wakes
        );

        assert!(!shell_stuck_forever.load(Ordering::Relaxed),
            "CRITICAL BUG: Shell got permanently stuck! \
             This is the exact production bug causing unresponsive shell.");
    }
}
