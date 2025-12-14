//! PIC timer interrupt assembly wrapper.
//!
//! We need to preserve *all* user-mode GPRs across preemptive scheduling.
//! The Rust `extern "x86-interrupt"` ABI doesn't expose GPRs to Rust code,
//! so we use an assembly wrapper that:
//! - Saves full GPRs on the kernel stack
//! - Detects Ring 3 vs Ring 0 via the interrupt frame CS
//! - `swapgs` only for Ring 3 entries
//! - Populates GS_DATA slots (`GS_SLOT_*`) so the scheduler can save the user
//!   context just like the syscall fast paths
//! - Calls `timer_interrupt_handler_inner` (Rust)
//! - Restores GPRs and returns via `iretq`

use core::arch::global_asm;

// NOTE: We intentionally save GPRs on the stack. If the scheduler switches
// away, the interrupted process will later resume in kernel mode and unwind
// back through this wrapper, which will then restore registers and `iretq`.
global_asm!(
    r#"
    .intel_syntax noprefix
    .global timer_interrupt_handler_asm
    timer_interrupt_handler_asm:
        // Save all GPRs (15 regs) so we can freely clobber scratch regs.
        push r15
        push r14
        push r13
        push r12
        push r11
        push r10
        push r9
        push r8
        push rbp
        push rbx
        push rdx
        push rcx
        push rsi
        push rdi
        push rax

        // Interrupt frame is above the pushed regs.
        // regs_bytes = 15 * 8 = 120
        // frame layout on Ring 3 entry:
        //   [frame+0]  RIP
        //   [frame+8]  CS
        //   [frame+16] RFLAGS
        //   [frame+24] RSP
        //   [frame+32] SS
        // On Ring 0 entry, only RIP/CS/RFLAGS are pushed.

        // If Ring 3, swapgs to kernel GS_DATA.
        test byte ptr [rsp + 120 + 8], 3
        jz 1f
        swapgs

        // Populate GS_DATA slots needed by the scheduler and (later) full restore.
        // Save user RIP/RSP/RFLAGS/CS/SS from the interrupt frame.
        mov rax, [rsp + 120 + 0]
        mov gs:[56], rax     // GS_SLOT_SAVED_RCX (7)  -> user RIP
        mov rax, [rsp + 120 + 16]
        mov gs:[64], rax     // GS_SLOT_SAVED_RFLAGS (8)
        mov rax, [rsp + 120 + 24]
        mov gs:[0], rax      // GS_SLOT_USER_RSP (0)
        mov rax, [rsp + 120 + 8]
        mov gs:[240], rax    // GS_SLOT_SAVED_USER_CS (30)
        mov rax, [rsp + 120 + 32]
        mov gs:[248], rax    // GS_SLOT_SAVED_USER_SS (31)

        // Save full user GPR snapshot to GS_DATA.
        mov gs:[80],  rax    // placeholder (overwrite below with real RAX)
        mov gs:[88],  rdi
        mov gs:[96],  rsi
        mov gs:[104], rdx
        mov gs:[112], rbx
        mov gs:[120], rbp
        mov gs:[128], r8
        mov gs:[136], r9
        mov gs:[144], r10
        mov gs:[152], r12
        mov gs:[200], r13
        mov gs:[208], r14
        mov gs:[216], r15
        mov gs:[224], rcx    // GS_SLOT_SAVED_GPR_RCX (28)
        mov gs:[232], r11    // GS_SLOT_SAVED_GPR_R11 (29)
        mov rax, [rsp + 0]   // saved RAX is at top of our push-save area
        mov gs:[80], rax     // GS_SLOT_SAVED_RAX (10)
    1:

        // Call Rust handler (System V ABI).
        call timer_interrupt_handler_inner

        // If we came from Ring 3, swapgs back before iretq.
        test byte ptr [rsp + 120 + 8], 3
        jz 2f
        swapgs
    2:

        // Restore GPRs and return.
        pop rax
        pop rdi
        pop rsi
        pop rcx
        pop rdx
        pop rbx
        pop rbp
        pop r8
        pop r9
        pop r10
        pop r11
        pop r12
        pop r13
        pop r14
        pop r15
        iretq
    "#
);

extern "C" {
    fn timer_interrupt_handler_inner();
}
