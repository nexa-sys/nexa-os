//! Context switch implementation
//!
//! This module contains the low-level context switch assembly code
//! and related utilities.

/// Context switch implementation
/// Saves the old context and restores the new context
/// This is called from schedule() to switch between processes
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(
    _old_context: *mut crate::process::Context,
    _new_context: *const crate::process::Context,
) {
    core::arch::naked_asm!(
        // Save old context (if not null)
        "test rdi, rdi",
        "jz 2f",
        // Save all registers to old_context
        "mov [rdi + 0x00], r15",
        "mov [rdi + 0x08], r14",
        "mov [rdi + 0x10], r13",
        "mov [rdi + 0x18], r12",
        "mov [rdi + 0x20], r11",
        "mov [rdi + 0x28], r10",
        "mov [rdi + 0x30], r9",
        "mov [rdi + 0x38], r8",
        "mov [rdi + 0x40], rsi",
        "mov [rdi + 0x48], rdi",
        "mov [rdi + 0x50], rbp",
        "mov [rdi + 0x58], rdx",
        "mov [rdi + 0x60], rcx",
        "mov [rdi + 0x68], rbx",
        "mov [rdi + 0x70], rax",
        // Save rip (return address)
        "mov rax, [rsp]",
        "mov [rdi + 0x78], rax",
        // Save rsp (before return address was pushed)
        "lea rax, [rsp + 8]",
        "mov [rdi + 0x80], rax",
        // Save rflags
        "pushfq",
        "pop rax",
        "mov [rdi + 0x88], rax",
        // Restore new context
        "2:",
        "mov r15, [rsi + 0x00]",
        "mov r14, [rsi + 0x08]",
        "mov r13, [rsi + 0x10]",
        "mov r12, [rsi + 0x18]",
        "mov r11, [rsi + 0x20]",
        "mov r10, [rsi + 0x28]",
        "mov r9,  [rsi + 0x30]",
        "mov r8,  [rsi + 0x38]",
        "mov rbp, [rsi + 0x50]",
        "mov rdx, [rsi + 0x58]",
        "mov rcx, [rsi + 0x60]",
        "mov rbx, [rsi + 0x68]",
        "mov rax, [rsi + 0x70]",
        // Restore rflags
        "mov rdi, [rsi + 0x88]",
        "push rdi",
        "popfq",
        // Restore rsp
        "mov rsp, [rsi + 0x80]",
        // Push rip onto new stack for ret
        "mov rdi, [rsi + 0x78]",
        "push rdi",
        // Restore rsi and rdi last
        "mov rdi, [rsi + 0x48]",
        "mov rsi, [rsi + 0x40]",
        // Return to new context's rip
        "ret",
    )
}
