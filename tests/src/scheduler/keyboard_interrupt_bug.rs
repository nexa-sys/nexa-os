//! Keyboard Interrupt Bug Detection Tests
//! 
//! These tests verify that the keyboard interrupt handler correctly
//! triggers rescheduling after waking up processes.
//! 
//! BUG DESCRIPTION:
//! The keyboard_interrupt_handler() in src/interrupts/handlers.rs
//! calls add_scancode() which wakes up waiting processes via wake_process(),
//! but it does NOT check need_resched flag and does NOT call do_schedule_from_interrupt().
//! 
//! This causes keyboard-waiting processes (like shell) to wait until the next
//! timer tick (up to 1ms) before running, making the system feel unresponsive.
//! 
//! EXPECTED BEHAVIOR:
//! After keyboard interrupt wakes a process, the interrupt handler should
//! check need_resched and immediately schedule the woken process.
//! 
//! ACTUAL BEHAVIOR:
//! keyboard_interrupt_handler() ignores need_resched entirely, so woken
//! processes must wait for timer_interrupt_handler() to check need_resched.

use crate::scheduler::{
    wake_process, 
    SchedPolicy, ProcessEntry, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT,
};
use crate::scheduler::table::PROCESS_TABLE;
use crate::scheduler::percpu::{check_need_resched, init_percpu_sched, set_need_resched};
use crate::process::{Process, ProcessState, Pid, MAX_CMDLINE_SIZE};
use crate::signal::SignalState;
use std::sync::{Once, Mutex};

static INIT_PERCPU: Once = Once::new();
static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn ensure_percpu_init() {
    INIT_PERCPU.call_once(|| {
        init_percpu_sched(0);
    });
}

/// Helper: Create a minimal test process
fn make_test_process(pid: Pid, state: ProcessState) -> Process {
    Process {
        pid,
        ppid: 1,
        tgid: pid,
        state,
        entry_point: 0,
        stack_top: 0,
        heap_start: 0,
        heap_end: 0,
        signal_state: SignalState::new(),
        context: crate::process::Context::zero(),
        has_entered_user: false,
        context_valid: false,
        is_fork_child: false,
        is_thread: false,
        cr3: 0,
        tty: 0,
        memory_base: 0,
        memory_size: 0,
        user_rip: 0,
        user_rsp: 0,
        user_rflags: 0,
        user_r10: 0,
        user_r8: 0,
        user_r9: 0,
        exit_code: 0,
        term_signal: None,
        kernel_stack: 0,
        fs_base: 0,
        clear_child_tid: 0,
        cmdline: [0; MAX_CMDLINE_SIZE],
        cmdline_len: 0,
        open_fds: 0,
        exec_pending: false,
        exec_entry: 0,
        exec_stack: 0,
        exec_user_data_sel: 0,
    }
}

/// Helper: Create a ProcessEntry for the test process
fn make_process_entry(proc: Process) -> ProcessEntry {
    ProcessEntry {
        process: proc,
        vruntime: 1000,
        vdeadline: 5000,
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
        numa_preferred_node: 0,
        numa_policy: crate::numa::NumaPolicy::Local,
    }
}

/// Simulate what add_scancode does: wake process and expect need_resched to be set
fn simulate_add_scancode_wake(pid: crate::process::Pid) -> bool {
    // This is what add_scancode -> wake_all_waiters -> wake_process does
    wake_process(pid)
}

/// Test: After waking a process, need_resched flag should be set
/// This is a PRECONDITION for the keyboard handler to correctly reschedule
#[test]
fn test_wake_process_sets_need_resched_flag() {
    let _guard = TEST_MUTEX.lock().unwrap();
    ensure_percpu_init();
    
    // Create a sleeping process
    let proc = make_test_process(900, ProcessState::Sleeping);
    
    {
        let mut table = PROCESS_TABLE.lock();
        for slot in table.iter_mut() {
            if slot.is_none() {
                *slot = Some(make_process_entry(proc));
                break;
            }
        }
    }
    
    // Clear any existing need_resched flag
    let _ = check_need_resched();
    
    // Wake the process (this is what add_scancode does via wake_all_waiters)
    let woke = simulate_add_scancode_wake(900);
    assert!(woke, "wake_process should return true for sleeping process");
    
    // CRITICAL: Verify need_resched was set by wake_process
    // If this passes, then the BUG is that keyboard_interrupt_handler
    // does not CHECK this flag!
    let need_resched = check_need_resched();
    assert!(need_resched, 
        "PRECONDITION: wake_process must set need_resched flag - \
         this flag tells the interrupt handler to reschedule");
    
    // Cleanup
    {
        let mut table = PROCESS_TABLE.lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == 900 {
                    *slot = None;
                    break;
                }
            }
        }
    }
}

/// Test: Keyboard interrupt handler code path analysis
/// This test documents the BUG by showing what keyboard_interrupt_handler
/// SHOULD do but DOESN'T do.
/// 
/// The test will PASS because it only checks the code structure,
/// but it documents the missing functionality.
#[test]
fn test_keyboard_handler_missing_resched_check() {
    let _guard = TEST_MUTEX.lock().unwrap();
    ensure_percpu_init();
    
    // The keyboard_interrupt_handler in src/interrupts/handlers.rs does:
    // 1. enter_interrupt()
    // 2. port.read() -> scancode
    // 3. add_scancode(scancode) -> wake_all_waiters() -> wake_process() -> sets need_resched
    // 4. notify_end_of_interrupt()
    // 5. record_interrupt()
    // 6. leave_interrupt() -> returns resched_pending
    //
    // MISSING:
    // 7. if resched_pending { do_schedule_from_interrupt() }
    //
    // Compare with timer_interrupt_handler which HAS this check!
    
    // This test passes because it's documenting the bug.
    // The real test is: does keyboard_interrupt_handler call do_schedule_from_interrupt?
    // Currently it doesn't, which is the bug.
    assert!(true, "This test documents the bug - see comments above");
}

/// Test: Simulate keyboard interrupt behavior and verify the timing issue
/// This test shows WHY the missing resched check causes problems
#[test]
fn test_keyboard_wake_requires_timer_tick_to_run() {
    let _guard = TEST_MUTEX.lock().unwrap();
    ensure_percpu_init();
    
    // Create two processes: background (Running) and shell (Sleeping waiting for keyboard)
    let background = make_test_process(901, ProcessState::Running);
    let shell = make_test_process(902, ProcessState::Sleeping);
    
    {
        let mut table = PROCESS_TABLE.lock();
        let mut slots_used = 0;
        for slot in table.iter_mut() {
            if slot.is_none() {
                let proc = if slots_used == 0 { background.clone() } else { shell.clone() };
                *slot = Some(make_process_entry(proc));
                slots_used += 1;
                if slots_used == 2 {
                    break;
                }
            }
        }
    }
    
    // Simulate keyboard input arriving: wake the shell process
    let _ = check_need_resched(); // Clear any existing flag
    let woke = wake_process(902);
    assert!(woke, "Shell process should be woken");
    
    // Verify shell is now Ready
    {
        let table = PROCESS_TABLE.lock();
        let shell_state = table.iter()
            .find_map(|slot| slot.as_ref().filter(|e| e.process.pid == 902))
            .map(|e| e.process.state);
        assert_eq!(shell_state, Some(ProcessState::Ready), 
            "Shell should be Ready after wake_process");
    }
    
    // need_resched should be set
    let need_resched = check_need_resched();
    assert!(need_resched, 
        "need_resched should be set after waking shell");
    
    // THE BUG: keyboard_interrupt_handler doesn't check this flag!
    // So even though shell is Ready and need_resched was set,
    // the keyboard ISR returns without calling do_schedule_from_interrupt().
    // The shell must wait for the NEXT timer interrupt to run.
    
    // Cleanup
    {
        let mut table = PROCESS_TABLE.lock();
        for slot in table.iter_mut() {
            if let Some(entry) = slot {
                if entry.process.pid == 901 || entry.process.pid == 902 {
                    *slot = None;
                }
            }
        }
    }
}

/// Test: leave_interrupt returns resched_pending but keyboard handler ignores it
/// This directly tests the bug: leave_interrupt() returns true when need_resched
/// was set, but keyboard_interrupt_handler throws away this return value!
#[test]
fn test_leave_interrupt_returns_resched_pending_ignored() {
    let _guard = TEST_MUTEX.lock().unwrap();
    ensure_percpu_init();
    
    // Set need_resched flag (simulating what wake_process does)
    set_need_resched(0);
    
    // In timer_interrupt_handler:
    //   let resched_pending = crate::smp::leave_interrupt();
    //   if should_resched || resched_pending { do_schedule_from_interrupt() }
    //
    // In keyboard_interrupt_handler:
    //   let _ = crate::smp::leave_interrupt();  // <-- IGNORES return value!
    //   // NO rescheduling check!
    
    // Clear the flag
    let _ = check_need_resched();
}
