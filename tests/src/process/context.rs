//! Process Context Tests
//!
//! Tests for CPU context saving/restoring and context switch mechanics.

use crate::process::{Context, ProcessState};
use crate::scheduler::ProcessEntry;

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
// Context Switch Simulation Tests
// ============================================================================

#[test]
fn test_context_switch_save_restore_cycle() {
    // Simulate a context switch
    
    // Process A running
    let mut proc_a = ProcessEntry::empty();
    proc_a.process.pid = 1;
    proc_a.process.state = ProcessState::Running;
    proc_a.process.context.rax = 0x1111;
    proc_a.process.context.rbx = 0x2222;
    proc_a.process.context.rsp = 0x7FFF_A000;
    proc_a.process.context.rip = 0x0040_A000;
    
    // Process B ready
    let mut proc_b = ProcessEntry::empty();
    proc_b.process.pid = 2;
    proc_b.process.state = ProcessState::Ready;
    proc_b.process.context.rax = 0x3333;
    proc_b.process.context.rbx = 0x4444;
    proc_b.process.context.rsp = 0x7FFF_B000;
    proc_b.process.context.rip = 0x0040_B000;
    
    // Context switch: A -> B
    
    // 1. Save A's context (simulated - registers would be saved to context)
    proc_a.process.state = ProcessState::Ready;
    proc_a.process.context_valid = true;
    
    // 2. Restore B's context (simulated - context would be loaded to registers)
    proc_b.process.state = ProcessState::Running;
    
    // Verify states
    assert_eq!(proc_a.process.state, ProcessState::Ready);
    assert!(proc_a.process.context_valid);
    assert_eq!(proc_b.process.state, ProcessState::Running);
    
    // Verify contexts are preserved
    assert_eq!(proc_a.process.context.rax, 0x1111);
    assert_eq!(proc_b.process.context.rax, 0x3333);
}

#[test]
fn test_context_preserved_through_multiple_switches() {
    let mut proc = ProcessEntry::empty();
    proc.process.pid = 1;
    
    // Set initial context
    proc.process.context.rax = 0xAAAA;
    proc.process.context.rbx = 0xBBBB;
    proc.process.context.r12 = 0xCCCC;
    proc.process.context.r13 = 0xDDDD;
    proc.process.context.rsp = 0x7FFF_0000;
    proc.process.context.rip = 0x0040_0000;
    
    // Simulate multiple context switches
    for _ in 0..100 {
        // Ready -> Running
        proc.process.state = ProcessState::Running;
        
        // Running -> Ready (preempted)
        proc.process.state = ProcessState::Ready;
        proc.process.context_valid = true;
    }
    
    // Context should be unchanged
    assert_eq!(proc.process.context.rax, 0xAAAA);
    assert_eq!(proc.process.context.rbx, 0xBBBB);
    assert_eq!(proc.process.context.r12, 0xCCCC);
    assert_eq!(proc.process.context.r13, 0xDDDD);
    assert_eq!(proc.process.context.rsp, 0x7FFF_0000);
    assert_eq!(proc.process.context.rip, 0x0040_0000);
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
