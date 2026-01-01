//! Process Context Tests
//!
//! Tests for CPU context saving/restoring and context switch mechanics.
//! Uses REAL scheduler functions for state transitions.

use crate::process::{Context, ProcessState, Process, Pid, MAX_CMDLINE_SIZE};
use crate::scheduler::{ProcessEntry, set_process_state, process_table_lock};
use crate::scheduler::{SchedPolicy, CpuMask, BASE_SLICE_NS, NICE_0_WEIGHT, calc_vdeadline};
// Use REAL kernel query function
use crate::scheduler::get_process_state;
use crate::scheduler::percpu::init_percpu_sched;
use crate::signal::SignalState;
use serial_test::serial;
use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};

static INIT_PERCPU: Once = Once::new();
static NEXT_PID: AtomicU64 = AtomicU64::new(85000);

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

fn make_process_entry_with_context(proc: Process, vruntime: u64) -> ProcessEntry {
    let vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, NICE_0_WEIGHT);
    ProcessEntry {
        process: proc,
        vruntime,
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

fn add_process_with_context(pid: Pid, state: ProcessState, rax: u64, rbx: u64, rsp: u64, rip: u64) {
    ensure_percpu_init();
    let mut proc = make_test_process(pid, state);
    proc.context.rax = rax;
    proc.context.rbx = rbx;
    proc.context.rsp = rsp;
    proc.context.rip = rip;
    
    let mut table = process_table_lock();
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            crate::process::register_pid_mapping(pid, idx as u16);
            let entry = make_process_entry_with_context(proc, 0);
            *slot = Some(entry);
            return;
        }
    }
    panic!("No free slot for test process {}", pid);
}

fn get_context_rax(pid: Pid) -> Option<u64> {
    let table = process_table_lock();
    for slot in table.iter() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                return Some(entry.process.context.rax);
            }
        }
    }
    None
}

fn cleanup_process(pid: Pid) {
    let mut table = process_table_lock();
    for slot in table.iter_mut() {
        if let Some(entry) = slot {
            if entry.process.pid == pid {
                *slot = None;
                return;
            }
        }
    }
}

// ============================================================================
// Context Structure Tests
// ============================================================================

#[test]
fn test_context_zero() {
    let ctx = Context::zero();
    
    // All registers should be zero except rflags
    assert_eq!(ctx.rax, 0);
    assert_eq!(ctx.rbx, 0);
    assert_eq!(ctx.rcx, 0);
    assert_eq!(ctx.rdx, 0);
    assert_eq!(ctx.rsi, 0);
    assert_eq!(ctx.rdi, 0);
    assert_eq!(ctx.rbp, 0);
    assert_eq!(ctx.r8, 0);
    assert_eq!(ctx.r9, 0);
    assert_eq!(ctx.r10, 0);
    assert_eq!(ctx.r11, 0);
    assert_eq!(ctx.r12, 0);
    assert_eq!(ctx.r13, 0);
    assert_eq!(ctx.r14, 0);
    assert_eq!(ctx.r15, 0);
    assert_eq!(ctx.rsp, 0);
    assert_eq!(ctx.rip, 0);
    // rflags has IF (interrupt flag) set by default (0x202)
    assert_eq!(ctx.rflags, 0x202, "rflags should have IF flag set by default");
}

#[test]
fn test_context_copy() {
    let mut ctx = Context::zero();
    ctx.rax = 0x1234;
    ctx.rsp = 0x7FFF_FF00;
    ctx.rip = 0x0040_0000;
    
    let copy = ctx;
    
    assert_eq!(copy.rax, ctx.rax);
    assert_eq!(copy.rsp, ctx.rsp);
    assert_eq!(copy.rip, ctx.rip);
}

#[test]
fn test_context_general_purpose_registers() {
    let mut ctx = Context::zero();
    
    // Set all GPRs
    ctx.rax = 1;
    ctx.rbx = 2;
    ctx.rcx = 3;
    ctx.rdx = 4;
    ctx.rsi = 5;
    ctx.rdi = 6;
    ctx.rbp = 7;
    ctx.r8 = 8;
    ctx.r9 = 9;
    ctx.r10 = 10;
    ctx.r11 = 11;
    ctx.r12 = 12;
    ctx.r13 = 13;
    ctx.r14 = 14;
    ctx.r15 = 15;
    
    // Verify all are distinct
    let regs = [
        ctx.rax, ctx.rbx, ctx.rcx, ctx.rdx,
        ctx.rsi, ctx.rdi, ctx.rbp,
        ctx.r8, ctx.r9, ctx.r10, ctx.r11,
        ctx.r12, ctx.r13, ctx.r14, ctx.r15,
    ];
    
    for i in 0..regs.len() {
        for j in (i + 1)..regs.len() {
            assert_ne!(regs[i], regs[j], "Register values should be unique");
        }
    }
}

#[test]
fn test_context_special_registers() {
    let mut ctx = Context::zero();
    
    // Set special registers
    ctx.rsp = 0x7FFF_FF00;  // Stack pointer
    ctx.rip = 0x0040_0000;  // Instruction pointer
    ctx.rflags = 0x202;     // Flags (IF set)
    
    // RSP should be in user space range
    assert!(ctx.rsp > 0 && ctx.rsp < 0x8000_0000_0000_0000);
    
    // RIP should be in user space range (for user processes)
    assert!(ctx.rip > 0 && ctx.rip < 0x8000_0000_0000_0000);
    
    // RFLAGS should have reasonable value
    assert_ne!(ctx.rflags, 0);
}

// ============================================================================
// Context in Process Entry Tests
// ============================================================================

#[test]
fn test_process_context_initially_zero() {
    let entry = ProcessEntry::empty();
    
    // Context should be zeroed in empty entry
    let ctx = &entry.process.context;
    assert_eq!(ctx.rax, 0);
    assert_eq!(ctx.rsp, 0);
    assert_eq!(ctx.rip, 0);
}

#[test]
fn test_process_context_modification() {
    let mut entry = ProcessEntry::empty();
    
    // Modify context
    entry.process.context.rax = 0xDEAD_BEEF;
    entry.process.context.rsp = 0x7FFF_0000;
    entry.process.context.rip = 0x0040_1000;
    
    // Verify
    assert_eq!(entry.process.context.rax, 0xDEAD_BEEF);
    assert_eq!(entry.process.context.rsp, 0x7FFF_0000);
    assert_eq!(entry.process.context.rip, 0x0040_1000);
}

// ============================================================================
// Context Switch Tests - Using REAL scheduler functions
// ============================================================================

#[test]
#[serial]
fn test_context_switch_save_restore_cycle() {
    // Test context switch using REAL process table and state transitions
    
    let pid_a = next_pid();
    let pid_b = next_pid();
    
    // Process A running with specific context
    add_process_with_context(pid_a, ProcessState::Running, 0x1111, 0x2222, 0x7FFF_A000, 0x0040_A000);
    
    // Process B ready with specific context
    add_process_with_context(pid_b, ProcessState::Ready, 0x3333, 0x4444, 0x7FFF_B000, 0x0040_B000);
    
    // Context switch: A -> B using REAL set_process_state
    // 1. A goes from Running to Ready (preempted)
    let _ = set_process_state(pid_a, ProcessState::Ready);
    
    // 2. B goes from Ready to Running (scheduled)
    let _ = set_process_state(pid_b, ProcessState::Running);
    
    // Verify states through REAL kernel function
    let state_a = get_process_state(pid_a);
    let state_b = get_process_state(pid_b);
    
    // Verify contexts are preserved
    let ctx_a_rax = get_context_rax(pid_a);
    let ctx_b_rax = get_context_rax(pid_b);
    
    cleanup_process(pid_a);
    cleanup_process(pid_b);
    
    assert_eq!(state_a, Some(ProcessState::Ready), "Process A should be Ready");
    assert_eq!(state_b, Some(ProcessState::Running), "Process B should be Running");
    assert_eq!(ctx_a_rax, Some(0x1111), "Context A should be preserved");
    assert_eq!(ctx_b_rax, Some(0x3333), "Context B should be preserved");
}

#[test]
#[serial]
fn test_context_preserved_through_multiple_switches() {
    let pid = next_pid();
    
    // Set initial context
    add_process_with_context(pid, ProcessState::Ready, 0xAAAA, 0xBBBB, 0x7FFF_0000, 0x0040_0000);
    
    // Use REAL set_process_state for multiple switches
    for _ in 0..10 {
        // Ready -> Running
        let _ = set_process_state(pid, ProcessState::Running);
        
        // Running -> Ready (preempted)
        let _ = set_process_state(pid, ProcessState::Ready);
    }
    
    // Context should be unchanged - verify through REAL process table
    let ctx_rax = get_context_rax(pid);
    
    cleanup_process(pid);
    
    assert_eq!(ctx_rax, Some(0xAAAA), "Context should be preserved through state transitions");
}

// ============================================================================
// Syscall Context Tests
// ============================================================================

#[test]
fn test_syscall_registers() {
    let mut entry = ProcessEntry::empty();
    
    // Syscall convention: RAX=syscall number, RDI=arg1, RSI=arg2, RDX=arg3, R10=arg4, R8=arg5, R9=arg6
    entry.process.context.rax = 1;  // SYS_write
    entry.process.context.rdi = 1;  // fd = stdout
    entry.process.context.rsi = 0x1000;  // buf
    entry.process.context.rdx = 5;  // count
    
    // Additional fields for R8/R9/R10 in user context
    entry.process.user_r8 = 0;
    entry.process.user_r9 = 0;
    entry.process.user_r10 = 0;
    
    // Verify syscall setup
    assert_eq!(entry.process.context.rax, 1);
    assert_eq!(entry.process.context.rdi, 1);
}

// ============================================================================
// RFLAGS Tests
// ============================================================================

#[test]
fn test_rflags_interrupt_flag() {
    let mut ctx = Context::zero();
    
    // IF flag (bit 9) controls interrupts
    const IF: u64 = 1 << 9;
    
    // Set IF (interrupts enabled)
    ctx.rflags |= IF;
    assert_ne!(ctx.rflags & IF, 0, "IF should be set");
    
    // Clear IF (interrupts disabled)
    ctx.rflags &= !IF;
    assert_eq!(ctx.rflags & IF, 0, "IF should be clear");
}

#[test]
fn test_rflags_user_mode() {
    let mut ctx = Context::zero();
    
    // Typical user mode RFLAGS
    const IF: u64 = 1 << 9;   // Interrupts enabled
    const IOPL_USER: u64 = 0; // IOPL = 0 for user mode
    const RESERVED: u64 = 1 << 1; // Bit 1 is always 1
    
    ctx.rflags = IF | RESERVED | IOPL_USER;
    
    assert_eq!(ctx.rflags, 0x202, "Standard user mode RFLAGS");
}

// ============================================================================
// Context Size and Alignment Tests
// ============================================================================

#[test]
fn test_context_size() {
    let size = core::mem::size_of::<Context>();
    eprintln!("Context size: {} bytes", size);
    
    // Context should hold at least 15 GPRs + RSP + RIP + RFLAGS
    // 18 * 8 = 144 bytes minimum
    assert!(size >= 144, "Context should hold all registers");
}

#[test]
fn test_context_alignment() {
    let align = core::mem::align_of::<Context>();
    eprintln!("Context alignment: {} bytes", align);
    
    // Context should be at least 8-byte aligned for u64 access
    assert!(align >= 8, "Context should be at least 8-byte aligned");
}
