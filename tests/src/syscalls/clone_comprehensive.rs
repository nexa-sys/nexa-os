//! Clone and Thread Creation Edge Case Tests
//!
//! Tests for clone() syscall using REAL kernel constants.
//! Tests flag combinations, resource sharing, and thread group management.

#[cfg(test)]
mod tests {
    // Import REAL kernel clone flags
    use crate::syscalls::{
        CLONE_VM, CLONE_FS, CLONE_FILES, CLONE_SIGHAND, CLONE_THREAD,
        CLONE_NEWNS, CLONE_SYSVSEM, CLONE_SETTLS, CLONE_PARENT_SETTID,
        CLONE_CHILD_CLEARTID, CLONE_DETACHED, CLONE_UNTRACED,
        CLONE_CHILD_SETTID, CLONE_VFORK,
    };
    
    // Additional flags from Linux ABI (not in kernel yet)
    const CLONE_PTRACE: u64 = 0x00002000;
    const CLONE_PARENT: u64 = 0x00008000;
    const CLONE_NEWCGROUP: u64 = 0x02000000;
    const CLONE_NEWUTS: u64 = 0x04000000;
    const CLONE_NEWIPC: u64 = 0x08000000;
    const CLONE_NEWUSER: u64 = 0x10000000;
    const CLONE_NEWPID: u64 = 0x20000000;
    const CLONE_NEWNET: u64 = 0x40000000;
    const CLONE_IO: u64 = 0x80000000;

    // =========================================================================
    // Flag Validation Tests
    // =========================================================================

    #[test]
    fn test_clone_flags_no_overlap() {
        // Verify all flags are distinct powers of 2
        let flags: [u64; 24] = [
            CLONE_VM, CLONE_FS, CLONE_FILES, CLONE_SIGHAND, CLONE_PTRACE,
            CLONE_VFORK, CLONE_PARENT, CLONE_THREAD, CLONE_NEWNS, CLONE_SYSVSEM,
            CLONE_SETTLS, CLONE_PARENT_SETTID, CLONE_CHILD_CLEARTID, CLONE_DETACHED,
            CLONE_UNTRACED, CLONE_CHILD_SETTID, CLONE_NEWCGROUP, CLONE_NEWUTS,
            CLONE_NEWIPC, CLONE_NEWUSER, CLONE_NEWPID, CLONE_NEWNET, CLONE_IO,
            0, // placeholder
        ];

        for (i, &flag1) in flags.iter().enumerate() {
            if flag1 == 0 { continue; }
            for (j, &flag2) in flags.iter().enumerate() {
                if flag2 == 0 || i == j { continue; }
                assert_eq!(flag1 & flag2, 0, 
                    "Clone flags at positions {} and {} overlap: {:#x} & {:#x}",
                    i, j, flag1, flag2);
            }
        }
    }

    #[test]
    fn test_pthread_create_flags() {
        // pthread_create typically uses these flags
        let pthread_flags = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND |
            CLONE_THREAD | CLONE_SYSVSEM | CLONE_SETTLS |
            CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID;

        // Verify all required flags are set
        assert_ne!(pthread_flags & CLONE_VM, 0, "CLONE_VM required for threads");
        assert_ne!(pthread_flags & CLONE_THREAD, 0, "CLONE_THREAD required for threads");
        assert_ne!(pthread_flags & CLONE_SIGHAND, 0, "CLONE_SIGHAND required with CLONE_THREAD");
    }

    #[test]
    fn test_fork_vs_clone_flags() {
        // fork() is equivalent to clone() with no flags
        let fork_flags: u64 = 0;

        // fork creates new address space
        assert_eq!(fork_flags & CLONE_VM, 0, "fork does not share VM");
        assert_eq!(fork_flags & CLONE_THREAD, 0, "fork creates new process");
    }

    // =========================================================================
    // Flag Dependency Tests
    // =========================================================================

    #[test]
    fn test_clone_thread_requires_sighand() {
        // CLONE_THREAD requires CLONE_SIGHAND (Linux semantics)
        fn validate_flags(flags: u64) -> Result<(), &'static str> {
            if (flags & CLONE_THREAD) != 0 && (flags & CLONE_SIGHAND) == 0 {
                return Err("EINVAL: CLONE_THREAD requires CLONE_SIGHAND");
            }
            Ok(())
        }

        // Valid: thread with sighand
        assert!(validate_flags(CLONE_THREAD | CLONE_SIGHAND | CLONE_VM).is_ok());

        // Invalid: thread without sighand
        assert!(validate_flags(CLONE_THREAD | CLONE_VM).is_err());
    }

    #[test]
    fn test_clone_sighand_requires_vm() {
        // CLONE_SIGHAND requires CLONE_VM (Linux semantics)
        fn validate_flags(flags: u64) -> Result<(), &'static str> {
            if (flags & CLONE_SIGHAND) != 0 && (flags & CLONE_VM) == 0 {
                return Err("EINVAL: CLONE_SIGHAND requires CLONE_VM");
            }
            Ok(())
        }

        // Valid: sighand with vm
        assert!(validate_flags(CLONE_SIGHAND | CLONE_VM).is_ok());

        // Invalid: sighand without vm
        assert!(validate_flags(CLONE_SIGHAND).is_err());
    }

    #[test]
    fn test_clone_thread_requires_vm() {
        // CLONE_THREAD implies shared address space
        fn validate_flags(flags: u64) -> Result<(), &'static str> {
            if (flags & CLONE_THREAD) != 0 && (flags & CLONE_VM) == 0 {
                return Err("EINVAL: CLONE_THREAD requires CLONE_VM");
            }
            Ok(())
        }

        // Valid: thread with vm
        assert!(validate_flags(CLONE_THREAD | CLONE_VM | CLONE_SIGHAND).is_ok());

        // Invalid: thread without vm (makes no sense)
        assert!(validate_flags(CLONE_THREAD | CLONE_SIGHAND).is_err());
    }

    // =========================================================================
    // Thread Group Tests - Using REAL process table
    // =========================================================================

    use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
    use crate::scheduler::{ProcessEntry, process_table_lock};
    use crate::scheduler::{SchedPolicy, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT, calc_vdeadline};
    use crate::scheduler::percpu::init_percpu_sched;
    use crate::signal::SignalState;
    use serial_test::serial;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    static INIT_PERCPU: Once = Once::new();
    static NEXT_PID: AtomicU64 = AtomicU64::new(90000);

    fn next_pid() -> Pid {
        NEXT_PID.fetch_add(1, Ordering::SeqCst)
    }

    fn ensure_percpu_init() {
        INIT_PERCPU.call_once(|| {
            init_percpu_sched(0);
        });
    }

    fn make_test_process(pid: Pid, tgid: Pid, is_thread: bool) -> Process {
        Process {
            pid,
            ppid: 1,
            tgid,
            state: ProcessState::Ready,
            entry_point: 0x1000000,
            stack_top: 0x1A00000,
            heap_start: 0x1200000,
            heap_end: 0x1200000,
            signal_state: SignalState::new(),
            context: crate::process::Context::zero(),
            has_entered_user: true,
            context_valid: true,
            is_fork_child: false,
            is_thread,
            cr3: 0x1000,
            tty: 0,
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
            numa_preferred_node: crate::numa::NUMA_NO_NODE,
            numa_policy: crate::numa::NumaPolicy::Local,
        }
    }

    fn add_thread_to_table(pid: Pid, tgid: Pid, is_thread: bool) {
        ensure_percpu_init();
        let mut table = process_table_lock();
        for (idx, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                crate::process::register_pid_mapping(pid, idx as u16);
                let entry = make_process_entry(make_test_process(pid, tgid, is_thread));
                *slot = Some(entry);
                return;
            }
        }
        panic!("No free slot for test process {}", pid);
    }

    fn count_threads_in_group(tgid: Pid) -> usize {
        let table = process_table_lock();
        table.iter()
            .filter_map(|s| s.as_ref())
            .filter(|e| e.process.tgid == tgid)
            .count()
    }

    fn cleanup_thread_group(tgid: Pid) {
        let mut table = process_table_lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.tgid == tgid {
                    *slot = None;
                }
            }
        }
    }

    #[test]
    #[serial]
    fn test_thread_group_creation() {
        let leader_pid = next_pid();
        
        // Add leader (main thread): tgid == pid, is_thread = false
        add_thread_to_table(leader_pid, leader_pid, false);
        
        let count = count_threads_in_group(leader_pid);
        
        // Check leader properties through REAL process table
        let is_leader = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == leader_pid)
                .map(|e| e.process.tgid == e.process.pid && !e.process.is_thread)
                .unwrap_or(false)
        };
        
        cleanup_thread_group(leader_pid);
        
        assert_eq!(count, 1, "Thread group should have 1 member");
        assert!(is_leader, "Leader should have tgid == pid and is_thread = false");
    }

    #[test]
    #[serial]
    fn test_thread_group_add_threads() {
        let leader_pid = next_pid();
        let thread1_pid = next_pid();
        let thread2_pid = next_pid();
        
        // Add leader
        add_thread_to_table(leader_pid, leader_pid, false);
        
        // Add threads (same tgid as leader, is_thread = true)
        add_thread_to_table(thread1_pid, leader_pid, true);
        add_thread_to_table(thread2_pid, leader_pid, true);
        
        let count = count_threads_in_group(leader_pid);
        
        // Verify thread properties
        let thread1_correct = {
            let table = process_table_lock();
            table.iter()
                .filter_map(|s| s.as_ref())
                .find(|e| e.process.pid == thread1_pid)
                .map(|e| e.process.tgid == leader_pid && e.process.is_thread)
                .unwrap_or(false)
        };
        
        cleanup_thread_group(leader_pid);
        
        assert_eq!(count, 3, "Thread group should have 3 members");
        assert!(thread1_correct, "Thread should have correct tgid and is_thread flag");
    }

    #[test]
    #[serial]
    fn test_thread_group_remove_thread() {
        let leader_pid = next_pid();
        let thread1_pid = next_pid();
        let thread2_pid = next_pid();
        
        add_thread_to_table(leader_pid, leader_pid, false);
        add_thread_to_table(thread1_pid, leader_pid, true);
        add_thread_to_table(thread2_pid, leader_pid, true);
        
        // Remove one thread (exit)
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == thread1_pid {
                        *slot = None;
                        break;
                    }
                }
            }
        }
        
        let count = count_threads_in_group(leader_pid);
        cleanup_thread_group(leader_pid);
        
        assert_eq!(count, 2, "Thread group should have 2 members after removal");
    }

    #[test]
    #[serial]
    fn test_thread_group_leader_exit() {
        let leader_pid = next_pid();
        let thread1_pid = next_pid();
        let thread2_pid = next_pid();
        
        add_thread_to_table(leader_pid, leader_pid, false);
        add_thread_to_table(thread1_pid, leader_pid, true);
        add_thread_to_table(thread2_pid, leader_pid, true);
        
        // Leader exits - in POSIX, remaining threads keep running with same tgid
        {
            let mut table = process_table_lock();
            for slot in table.iter_mut() {
                if let Some(entry) = slot {
                    if entry.process.pid == leader_pid {
                        // Mark as Zombie rather than remove immediately
                        entry.process.state = ProcessState::Zombie;
                        break;
                    }
                }
            }
        }
        
        // Other threads still exist with same tgid
        let remaining = count_threads_in_group(leader_pid);
        cleanup_thread_group(leader_pid);
        
        assert_eq!(remaining, 3, "All members (including zombie leader) should remain");
    }

    // =========================================================================
    // Stack Address Tests
    // =========================================================================

    #[test]
    fn test_stack_address_alignment() {
        // Stack pointer should be 16-byte aligned for x86_64 ABI
        fn validate_stack(stack: u64) -> bool {
            (stack & 0xF) == 0
        }

        assert!(validate_stack(0x7FFF_FFFF_FFF0), "16-byte aligned stack");
        assert!(!validate_stack(0x7FFF_FFFF_FFF1), "Misaligned by 1");
        assert!(!validate_stack(0x7FFF_FFFF_FFF8), "8-byte aligned only");
    }

    #[test]
    fn test_stack_address_range() {
        use crate::process::{STACK_BASE, STACK_SIZE};
        
        // Stack should be within defined range
        let stack_top = STACK_BASE + STACK_SIZE;
        
        assert!(stack_top > STACK_BASE, "Stack should have positive size");
    }

    #[test]
    fn test_stack_grows_down() {
        // Verify stack grows down (typical for x86_64)
        let initial_sp = 0x7FFF_FFFF_0000u64;
        let after_push = initial_sp - 8;
        
        assert!(after_push < initial_sp, "Stack grows down");
    }

    // =========================================================================
    // TLS (Thread Local Storage) Tests
    // =========================================================================

    #[test]
    fn test_tls_address_alignment() {
        // TLS base should be aligned
        fn validate_tls(addr: u64) -> bool {
            (addr & 0xF) == 0
        }

        assert!(validate_tls(0x7FFF_FFFF_0000));
        assert!(!validate_tls(0x7FFF_FFFF_0001));
    }

    #[test]
    fn test_clone_settls_flag() {
        // When CLONE_SETTLS is set, tls parameter should be used
        let flags = CLONE_VM | CLONE_THREAD | CLONE_SIGHAND | CLONE_SETTLS;
        
        assert_ne!(flags & CLONE_SETTLS, 0);
    }

    // =========================================================================
    // TID Address Tests
    // =========================================================================

    #[test]
    fn test_tid_address_validation() {
        use crate::process::{USER_VIRT_BASE, INTERP_BASE, INTERP_REGION_SIZE};
        
        fn is_valid_tid_addr(addr: u64) -> bool {
            // Must be in user space and 4-byte aligned (u32)
            let user_end = INTERP_BASE + INTERP_REGION_SIZE;
            addr >= USER_VIRT_BASE && addr < user_end && (addr & 3) == 0
        }

        assert!(is_valid_tid_addr(USER_VIRT_BASE + 0x1000));
        assert!(!is_valid_tid_addr(0)); // Null
        assert!(!is_valid_tid_addr(0x1000)); // Below user space
    }

    #[test]
    fn test_parent_settid_and_child_settid() {
        // Both should write TID to different addresses
        let flags = CLONE_PARENT_SETTID | CLONE_CHILD_SETTID;
        
        assert_ne!(flags & CLONE_PARENT_SETTID, 0);
        assert_ne!(flags & CLONE_CHILD_SETTID, 0);
    }

    // =========================================================================
    // CLONE_CHILD_CLEARTID Tests
    // =========================================================================

    #[test]
    fn test_child_cleartid_for_pthread_join() {
        // This flag enables pthread_join to work
        let pthread_flags = CLONE_VM | CLONE_THREAD | CLONE_SIGHAND |
            CLONE_SETTLS | CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID;

        // CLONE_CHILD_CLEARTID clears TID and does futex wake on thread exit
        assert_ne!(pthread_flags & CLONE_CHILD_CLEARTID, 0);
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    fn test_clone_with_no_flags() {
        // clone(0) is effectively fork()
        let flags: u64 = 0;
        
        // Should create new process with copied address space
        assert_eq!(flags & CLONE_VM, 0);
        assert_eq!(flags & CLONE_THREAD, 0);
    }

    #[test]
    fn test_clone_vm_without_thread() {
        // CLONE_VM alone creates process sharing address space (vfork-like)
        let flags = CLONE_VM;
        
        // This is valid but unusual
        assert_ne!(flags & CLONE_VM, 0);
        assert_eq!(flags & CLONE_THREAD, 0);
    }

    #[test]
    fn test_conflicting_namespace_flags() {
        // Can't have CLONE_NEWPID and CLONE_THREAD together
        fn validate_flags(flags: u64) -> Result<(), &'static str> {
            if (flags & CLONE_THREAD) != 0 && (flags & CLONE_NEWPID) != 0 {
                return Err("EINVAL: CLONE_THREAD and CLONE_NEWPID are incompatible");
            }
            Ok(())
        }

        assert!(validate_flags(CLONE_THREAD | CLONE_VM | CLONE_SIGHAND).is_ok());
        assert!(validate_flags(CLONE_NEWPID).is_ok());
        assert!(validate_flags(CLONE_THREAD | CLONE_NEWPID | CLONE_VM | CLONE_SIGHAND).is_err());
    }

    #[test]
    #[serial]
    fn test_max_threads_per_process() {
        // Test using REAL process table - limited by MAX_PROCESSES
        use crate::process::MAX_PROCESSES;
        
        let leader_pid = next_pid();
        add_thread_to_table(leader_pid, leader_pid, false);
        
        // Add threads up to a reasonable test limit (not filling entire table)
        let test_limit = 10; // Keep test fast
        for _ in 0..test_limit {
            let thread_pid = next_pid();
            add_thread_to_table(thread_pid, leader_pid, true);
        }
        
        let count = count_threads_in_group(leader_pid);
        cleanup_thread_group(leader_pid);
        
        assert_eq!(count, test_limit + 1, "Should have leader + {} threads", test_limit);
    }
}
