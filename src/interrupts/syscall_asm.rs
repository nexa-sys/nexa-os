//! Syscall Assembly Entry Points
//!
//! This module contains the assembly entry points for system calls,
//! both the int 0x81 interrupt gate and the fast syscall/sysret path.

use core::arch::asm;
use core::arch::global_asm;

use crate::{kdebug, ktrace};

// Assembly entry point for int 0x81 syscall interrupt
global_asm!(
    ".global syscall_interrupt_handler",
    "syscall_interrupt_handler:",
    // On int gate from Ring 3, CPU pushed: SS, RSP, RFLAGS, CS, RIP (in that order from high to low addresses)
    // Current stack layout (from top, rsp+0 to rsp+32):
    //   [rsp+0]  = RIP
    //   [rsp+8]  = CS
    //   [rsp+16] = RFLAGS
    //   [rsp+24] = user RSP
    //   [rsp+32] = SS
    "swapgs", // Swap to kernel GS
    // CRITICAL: Save additional syscall argument registers for syscalls with >3 args
    // This must be done FIRST before any register is modified!
    "mov gs:[32], r10", // GS[4]  = arg4 (r10)
    "mov gs:[40], r8",  // GS[5]  = arg5 (r8)
    "mov gs:[48], r9",  // GS[6]  = arg6 (r9)
    // CRITICAL: Save callee-saved registers to GS_DATA BEFORE they are modified
    // These are needed for fork() to properly restore child's registers
    // GS_SLOT_SAVED_RBX = 14, offset = 14 * 8 = 112 (but 112/120/128 are used for CS/SS snapshot)
    // Use new slots: 176 for RBX, 184 for RBP, 192 for R12, 200 for R13, 208 for R14, 216 for R15
    "mov gs:[176], rbx", // Save callee-saved RBX
    "mov gs:[184], rbp", // Save callee-saved RBP
    "mov gs:[192], r12", // Save callee-saved R12
    "mov gs:[200], r13", // Save callee-saved R13
    "mov gs:[208], r14", // Save callee-saved R14
    "mov gs:[216], r15", // Save callee-saved R15
    // Record the incoming CS/SS pair for diagnostics
    "mov r10, [rsp + 8]",
    "mov gs:[120], r10", // gs slot 15 = entry CS snapshot
    "mov r10, [rsp + 32]",
    "mov gs:[128], r10", // gs slot 16 = entry SS snapshot
    // CRITICAL: Save user RSP for fork() and context switching
    "mov r10, [rsp + 24]", // r10 = user RSP
    "mov gs:[0], r10",     // gs slot 0 = user RSP (GS_SLOT_USER_RSP)
    // CRITICAL: Save user RIP for context switching
    "mov r10, [rsp + 0]", // r10 = user RIP
    "mov gs:[56], r10", // gs slot 7 = saved RIP (GS_SLOT_SAVED_RCX, used for syscall return address)
    // CRITICAL: Save user RFLAGS for context switching
    "mov r10, [rsp + 16]", // r10 = user RFLAGS
    "mov gs:[64], r10",    // gs slot 8 = saved RFLAGS (GS_SLOT_SAVED_RFLAGS)
    // Guard against nested entries while processes share a single kernel stack
    "mov r9, gs:[160]",
    "test r9, r9",
    "jz .Lkernel_stack_guard_set_int",
    "mov rdi, 0",
    "call kernel_stack_guard_reentry_fail",
    ".Lkernel_stack_guard_set_int:",
    "mov gs:[168], rsp",
    "mov r9, 1",
    "mov gs:[160], r9",
    // Now push general-purpose registers (we will NOT touch the interrupt frame on stack)
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Align stack to 16 bytes before calling into Rust (SysV ABI requires
    // %rsp % 16 == 8 at the call site so the callee observes 16-byte alignment).
    "sub rsp, 8",
    // Prepare arguments for syscall_dispatch(nr=rax, arg1=rdi, arg2=rsi, arg3=rdx, syscall_return_addr=user_rip)
    // System V x86_64 ABI: rdi, rsi, rdx, rcx, r8
    // user_rip is already in GS_DATA slot 7, load it to r8
    "mov r8, gs:[56]", // r8 = syscall_return_addr (from GS_SLOT_SAVED_RCX)
    "mov rcx, rdx",    // rcx = arg3
    "mov rdx, rsi",    // rdx = arg2
    "mov rsi, rdi",    // rsi = arg1
    "mov rdi, rax",    // rdi = nr
    "call syscall_dispatch",
    // Check if execve returned (magic value 0x4558454300000000)
    "movabs rbx, 0x4558454300000000",
    "cmp rax, rbx",
    "jne .Lnormal_int_return", // Not exec, normal return
    // Exec return: modify interrupt frame to jump to new program
    // Call get_exec_context to get new entry/stack
    "push rax",           // Save magic value (will be overwritten)
    "sub rsp, 16",        // Space for entry (rsp+8) and stack (rsp)
    "lea rdi, [rsp + 8]", // entry_out pointer (1st param)
    "mov rsi, rsp",       // stack_out pointer (2nd param)
    "xor rdx, rdx",       // user_data_sel_out = NULL (3rd param) - we don't need it for iretq
    "call get_exec_context",
    "test al, al",      // Check if exec was pending
    "jz .Lexec_failed", // Not exec, restore and normal return
    // Load new entry and stack
    "mov r14, [rsp + 8]", // New entry -> r14
    "mov r15, [rsp]",     // New stack -> r15
    "add rsp, 16",        // Clean up
    "add rsp, 8",         // Remove saved rax
    // Now we're back at the alignment padding (sub rsp,8)
    "add rsp, 8", // Remove alignment
    // Pop all general-purpose registers (we don't need to restore them for exec)
    "add rsp, 80", // Skip 10 registers (r15, r14, r13, r12, rbp, rbx, rdi, rsi, rdx, rcx)
    // Now RSP points to the interrupt frame on stack
    // Interrupt frame layout (from rsp):
    //   [rsp+0]  = RIP  ← modify this to new entry
    //   [rsp+8]  = CS
    //   [rsp+16] = RFLAGS
    //   [rsp+24] = RSP  ← modify this to new stack
    //   [rsp+32] = SS
    "mov [rsp], r14",      // Set new entry as return RIP
    "mov [rsp + 24], r15", // Set new stack as user RSP
    "xor r9, r9",
    "mov gs:[160], r9",
    "mov gs:[168], r9",
    "xor rax, rax", // Clear return value for exec
    "swapgs",       // Swap back to user GS
    "iretq",        // Jump to new program
    ".Lexec_failed:",
    "add rsp, 16", // Clean up get_exec_context params
    "pop rax",     // Restore rax
    ".Lnormal_int_return:",
    // Return value already in rax
    "add rsp, 8",
    // Restore general-purpose registers (reverse order)
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    // At this point, stack pointer is back to where the interrupt frame starts
    // The interrupt frame (RIP, CS, RFLAGS, RSP, SS) is still intact on the stack

    // CRITICAL: Restore syscall argument registers from GS_DATA
    // These were saved at syscall entry and must be restored before iretq
    // because they may have been overwritten by kernel code or other processes
    "push rax", // Save syscall return value
    "mov rax, gs:[32]",
    "mov r10, rax", // Restore r10 (arg4) from GS[4]
    "mov rax, gs:[40]",
    "mov r8, rax", // Restore r8 (arg5) from GS[5]
    "mov rax, gs:[48]",
    "mov r9, rax", // Restore r9 (arg6) from GS[6]
    "pop rax",     // Restore syscall return value
    // Snapshot the user-mode frame before we hand control back, so faults can
    // report the exact values that iretq attempted to restore.
    "xor r11, r11",
    "mov gs:[160], r11",
    "mov gs:[168], r11",
    "mov r11, rax", // Save syscall return value temporarily
    "mov rax, [rsp]",
    "mov gs:[80], rax", // gs slot 10 = user RIP
    "mov rax, [rsp + 8]",
    "mov gs:[88], rax", // gs slot 11 = user CS
    "mov rax, [rsp + 16]",
    "mov gs:[96], rax", // gs slot 12 = user RFLAGS
    "mov rax, [rsp + 24]",
    "mov gs:[104], rax", // gs slot 13 = user RSP
    "mov rax, [rsp + 32]",
    "mov gs:[112], rax", // gs slot 14 = user SS
    "mov rax, r11",      // Restore syscall return value
    "swapgs",            // Swap back to user GS
    "iretq"
);

// Ring 3 switch handler - interrupt 0x80
global_asm!(
    ".global ring3_switch_handler",
    "ring3_switch_handler:",
    "swapgs", // Swap to kernel GS
    // Stack layout from int 0x80: [ss, rsp, rflags, cs, rip] + pushed values [entry, stack, rflags, cs, ss]
    // We need to set up sysret parameters
    "mov rax, gs:[160]",
    "test rax, rax",
    "jz .Lkernel_stack_guard_set_ring3",
    "mov rdi, 0",
    "call kernel_stack_guard_reentry_fail",
    ".Lkernel_stack_guard_set_ring3:",
    "mov gs:[168], rsp",
    "mov rax, 1",
    "mov gs:[160], rax",
    "mov rcx, [rsp + 8]",  // entry point (rip for sysret)
    "mov r11, [rsp + 16]", // rflags
    "mov rsp, [rsp]",      // stack pointer
    "mov gs:[136], rcx",   // gs slot 17 = sysret target RIP
    "mov gs:[144], r11",   // gs slot 18 = sysret target RFLAGS
    "mov gs:[152], rsp",   // gs slot 19 = sysret target RSP
    "xor rdx, rdx",
    "mov gs:[160], rdx",
    "mov gs:[168], rdx",
    "swapgs", // Swap back to user GS (saved in KernelGSBase)
    // Set user data segments
    "mov ax, 0x23",
    "mov ds, ax",
    "mov es, ax",
    "mov fs, ax",
    "mov gs, ax",
    // Return to user mode
    "sysretq"
);

// Syscall fast path via SYSCALL instruction
#[unsafe(naked)]
pub extern "C" fn syscall_instruction_handler() {
    core::arch::naked_asm!(
        // On SYSCALL entry the CPU stores the user return RIP in RCX and the
        // user RFLAGS in R11. Capture that state alongside the user stack so
        // the kernel can restore it exactly before executing SYSRET.
        "swapgs",           // Swap to kernel GS
        "mov gs:[0], rsp",  // GS[0]  = user RSP snapshot
        "mov gs:[72], rsp", // GS[9]  = debug copy of user RSP
        "mov rsp, gs:[8]",  // RSP    = kernel stack top
        "mov gs:[56], rcx", // GS[7]  = user return RIP (RCX)
        "mov gs:[64], r11", // GS[8]  = user RFLAGS (R11)
        // Save additional syscall argument registers for syscalls with >3 args
        "mov gs:[32], r10", // GS[4]  = arg4 (r10)
        "mov gs:[40], r8",  // GS[5]  = arg5 (r8)
        "mov gs:[48], r9",  // GS[6]  = arg6 (r9)
        "mov rcx, gs:[160]",
        "test rcx, rcx",
        "jz .Lkernel_stack_guard_set_syscall",
        "mov rdi, 1",
        "call kernel_stack_guard_reentry_fail",
        ".Lkernel_stack_guard_set_syscall:",
        "mov gs:[168], rsp",
        "mov rcx, 1",
        "mov gs:[160], rcx",
        // Preserve callee-saved registers that Rust expects us to maintain.
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",
        // See note in int 0x81 handler: ensure 16-byte stack alignment before call.
        "sub rsp, 8",
        // Arrange SysV ABI arguments for syscall_dispatch(nr, arg1, arg2, arg3, syscall_return_addr).
        // Get syscall return address from gs:[56] (user RCX saved earlier)
        "mov r8, gs:[56]", // r8 = syscall return address (5th param)
        "mov rcx, rdx",    // rcx = arg3 (4th param)
        "mov rdx, rsi",    // rdx = arg2 (3rd param)
        "mov rsi, rdi",    // rsi = arg1 (2nd param)
        "mov rdi, rax",    // rdi = syscall number (1st param)
        "call syscall_dispatch",
        // Check if execve returned (magic value 0x4558454300000000)
        "movabs rbx, 0x4558454300000000",
        "cmp rax, rbx",
        "jne 2f", // Not exec, normal return
        // Exec return: get new entry/stack from ExecContext
        "sub rsp, 16",        // Space for entry (rsp+8) and stack (rsp)
        "lea rdi, [rsp + 8]", // entry_out pointer (1st param)
        "mov rsi, rsp",       // stack_out pointer (2nd param)
        "call get_exec_context",
        "test al, al", // Check if exec was pending
        "jz 1f",       // Not exec, restore stack and normal return
        // Load new entry and stack
        "mov rcx, [rsp + 8]", // New entry -> rcx (for sysretq)
        "mov rsp, [rsp]",     // New stack -> rsp
        "xor rdx, rdx",
        "mov gs:[160], rdx",
        "mov gs:[168], rdx",
        "mov r11, 0x202", // User rflags (IF=1, reserved=1)
        "xor rax, rax",   // Clear return value for exec
        "swapgs",         // Swap back to user GS
        "sysretq",        // Jump to new program
        "1:",             // exec failed, restore stack
        "add rsp, 16",
        "2:", // Normal return path
        "add rsp, 8",
        // Restore the callee-saved register set before we leave the kernel stack.
        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        // Clear kernel stack guard flags
        "xor rcx, rcx",
        "mov gs:[160], rcx",
        "mov gs:[168], rcx",
        // Restore user execution context for SYSRETQ.
        // IMPORTANT: Restore these BEFORE loading RCX/R11/RSP for sysretq
        "mov r10, gs:[32]", // Restore r10 (arg4)
        "mov r8, gs:[40]",  // Restore r8 (arg5)
        "mov r9, gs:[48]",  // Restore r9 (arg6)
        // Now load registers for sysretq
        "mov rcx, gs:[56]", // rcx = user RIP (will be loaded into RIP by sysretq)
        "mov r11, gs:[64]", // r11 = user RFLAGS (will be loaded into RFLAGS by sysretq)
        "mov rsp, gs:[0]",  // rsp = user RSP
        "swapgs",           // Swap back to user GS
        "sysretq",
    );
}

extern "C" {
    pub fn syscall_interrupt_handler();
    pub fn ring3_switch_handler();
    // Import get_exec_context from syscall module for exec support
    #[allow(dead_code)]
    fn get_exec_context(
        entry_out: *mut u64,
        stack_out: *mut u64,
        user_data_sel_out: *mut u64,
    ) -> bool;
}

/// Inner handler for syscall_instruction_handler (called from naked assembly)
/// This is for debugging purposes - the actual syscall dispatch is done via syscall_dispatch
#[unsafe(no_mangle)]
extern "C" fn syscall_instruction_handler_inner() {
    // This function is called from naked assembly
    // Registers: rax=syscall_num, rdi=arg1, rsi=arg2, rdx=arg3
    let syscall_num: u64;
    let arg1: u64;
    let arg2: u64;
    let arg3: u64;

    unsafe {
        asm!(
            "mov {}, rax",
            "mov {}, rdi",
            "mov {}, rsi",
            "mov {}, rdx",
            out(reg) syscall_num,
            out(reg) arg1,
            out(reg) arg2,
            out(reg) arg3,
        );
    }

    kdebug!(
        "SYSCALL_INSTRUCTION_HANDLER: syscall={} arg1={:#x} arg2={:#x} arg3={:#x}",
        syscall_num,
        arg1,
        arg2,
        arg3
    );

    if syscall_num == 1 {
        // write
        let fd = arg1;
        let buf_ptr = arg2 as *const u8;
        let count = arg3 as usize;

        kdebug!(
            "SYSCALL: write fd={} buf={:#x} count={}",
            fd,
            buf_ptr as u64,
            count
        );

        // For simplicity, assume fd=1 and print to VGA and serial
        for i in 0..count {
            let byte = unsafe { *buf_ptr.add(i) };
            kdebug!("SYSCALL: writing byte {}", byte as char);
            write_char_to_vga(byte);
            write_char_to_serial(byte);
        }

        // Return count
        unsafe {
            asm!("mov rax, {}", in(reg) count as u64);
        }
    } else {
        kdebug!("SYSCALL: unknown syscall {}", syscall_num);
        unsafe {
            asm!("mov rax, {}", in(reg) (-1i64 as u64));
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn write_char_to_vga(c: u8) {
    use core::fmt::Write;
    crate::vga_buffer::with_writer(|writer| {
        let _ = write!(writer, "{}", c as char);
    });
}

#[unsafe(no_mangle)]
extern "C" fn write_char_to_serial(c: u8) {
    ktrace!("{}", c as char);
}

// Debug function for Ring 3 switch
#[unsafe(no_mangle)]
extern "C" fn ring3_debug_print() {
    // This function is called from assembly with registers set
    // rsi = entry, rdi = stack, rdx = cs, rcx = ss, r8 = ds
    let entry: u64;
    let stack: u64;
    let cs: u64;
    let ss: u64;
    let ds: u64;

    unsafe {
        asm!(
            "mov {}, rsi",
            "mov {}, rdi",
            "mov {}, rdx",
            "mov {}, rcx",
            "mov {}, r8",
            out(reg) entry,
            out(reg) stack,
            out(reg) cs,
            out(reg) ss,
            out(reg) ds,
        );
    }

    crate::kinfo!(
        "RING3_SWITCH: entry={:#x}, stack={:#x}, cs={:#x}, ss={:#x}, ds={:#x}",
        entry,
        stack,
        cs,
        ss,
        ds
    );
}

// Debug function for Ring 3 switch GS check
#[unsafe(no_mangle)]
extern "C" fn ring3_debug_print2() {
    // This function is called from assembly with registers set
    // rax = gs:[0], rbx = gs:[8], rcx = gs:[40]
    let gs0: u64;
    let gs8: u64;
    let gs40: u64;

    unsafe {
        asm!(
            "mov {}, rax",
            "mov {}, rbx",
            "mov {}, rcx",
            out(reg) gs0,
            out(reg) gs8,
            out(reg) gs40,
        );
    }

    kdebug!(
        "GS check: gs:[0]={:#x}, gs:[8]={:#x}, gs:[40]={:#x}",
        gs0,
        gs8,
        gs40
    );
}
