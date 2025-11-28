//! Process execution and user-mode transition
//!
//! This module contains functions for executing processes and transitioning
//! to user mode (Ring 3), including context switching and sysretq handling.

use crate::{kdebug, kinfo, ktrace};

use super::types::{Process, ProcessState};

impl Process {
    /// Set parent process ID (POSIX)
    pub fn set_ppid(&mut self, ppid: super::types::Pid) {
        self.ppid = ppid;
    }

    /// Get process ID
    pub fn pid(&self) -> super::types::Pid {
        self.pid
    }

    /// Get parent process ID
    pub fn ppid(&self) -> super::types::Pid {
        self.ppid
    }

    /// Get process state
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// Execute the process in user mode (Ring 3)
    pub fn execute(&mut self) {
        self.state = ProcessState::Running;

        ktrace!(
            "[process::execute] PID={}, entry={:#x}, stack={:#x}, has_entered_user={}, is_fork_child={}",
            self.pid, self.entry_point, self.stack_top, self.has_entered_user, self.is_fork_child
        );

        kinfo!(
            "Executing process PID={}, entry={:#x}, stack={:#x}",
            self.pid,
            self.entry_point,
            self.stack_top
        );

        // Mark as entered user mode BEFORE switching CR3
        self.has_entered_user = true;

        if self.is_fork_child {
            // Fork child: Return from syscall with RAX=0
            // Use sysret mechanism to return to userspace at syscall_return_addr
            ktrace!(
                "[process::execute] Fork child: returning to {:#x} with RAX=0",
                self.entry_point
            );

            // Set up syscall return context
            crate::interrupts::restore_user_syscall_context(
                self.entry_point, // user_rip (syscall return address)
                self.stack_top,   // user_rsp
                self.user_rflags, // user_rflags
            );

            // CRITICAL: Switch CR3 and return to userspace atomically
            // We must switch CR3 in the same assembly block that does sysretq
            // to avoid accessing kernel stack after address space switch
            unsafe {
                core::arch::asm!(
                    "cli",                 // Disable interrupts during transition
                    "mov cr3, {cr3}",      // Switch to child's address space
                    "mov rcx, {rip}",      // RCX = return RIP for sysretq
                    "mov r11, {rflags}",   // R11 = RFLAGS for sysretq
                    "mov rsp, {rsp}",      // RSP = user stack
                    "xor rax, rax",        // RAX = 0 (fork child return value)
                    "sysretq",             // Return to Ring 3
                    cr3 = in(reg) self.cr3,
                    rip = in(reg) self.entry_point,
                    rflags = in(reg) self.user_rflags,
                    rsp = in(reg) self.stack_top,
                    options(noreturn)
                );
            }
        } else {
            // Normal process (init/execve): Jump to entry point
            jump_to_usermode_with_cr3(self.entry_point, self.stack_top, self.cr3);
        }
    }

    /// Set the controlling TTY for this process
    pub fn set_tty(&mut self, tty: usize) {
        self.tty = tty;
    }

    /// Get the controlling TTY for this process
    pub fn tty(&self) -> usize {
        self.tty
    }
}

/// Jump to user mode (Ring 3) and execute code at given address with CR3 switch
/// This function never returns - execution continues in user space
#[inline(never)]
pub fn jump_to_usermode_with_cr3(entry: u64, stack: u64, cr3: u64) -> ! {
    // Use kdebug! macro for direct serial output
    kdebug!(
        "[jump_to_usermode_with_cr3] ENTRY: entry={:#x}, stack={:#x}, cr3={:#x}",
        entry,
        stack,
        cr3
    );

    kdebug!(
        "[jump_to_usermode_with_cr3] entry={:#018x} stack={:#018x} cr3={:#018x}",
        entry,
        stack,
        cr3
    );

    // Set GS data for syscall and Ring 3 switching
    crate::gdt::debug_dump_selectors("jump_to_usermode_with_cr3");
    let selectors = unsafe { crate::gdt::get_selectors() };
    let user_code_sel = selectors.user_code_selector.0;
    let user_data_sel = selectors.user_data_selector.0;

    kdebug!(
        "[jump_to_usermode_with_cr3] user_code_selector.0={:04x}, user_data_selector.0={:04x}",
        user_code_sel,
        user_data_sel
    );

    kdebug!(
        "[jump_to_usermode_with_cr3] Setting GS_DATA: entry={:#x}, stack={:#x}, user_cs={:#x}, user_ds={:#x}",
        entry,
        stack,
        user_code_sel as u64 | 3,
        user_data_sel as u64 | 3
    );

    unsafe {
        crate::interrupts::set_gs_data(
            entry,
            stack,
            user_code_sel as u64 | 3,
            user_data_sel as u64 | 3,
            user_data_sel as u64 | 3,
        );

        // Set GS base to point to per-CPU GS_DATA for both kernel and user mode
        use x86_64::registers::model_specific::Msr;
        let gs_base = crate::smp::current_gs_data_ptr() as u64;
        Msr::new(0xc0000101).write(gs_base);
    }

    kdebug!("[jump_to_usermode_with_cr3] About to switch CR3 and execute sysretq");

    unsafe {
        kdebug!("BEFORE_SYSRET_WITH_CR3");

        // CRITICAL FIX: Switch CR3 and jump to usermode atomically
        // We MUST switch CR3 in the same assembly block as sysretq
        // to avoid accessing kernel stack after address space switch
        //
        // The sequence is:
        // 1. Disable interrupts (cli)
        // 2. Switch CR3 to user address space
        // 3. Set up sysretq registers (RCX, R11, RSP)
        // 4. Execute sysretq
        //
        // No Rust code can execute between steps 2 and 4, otherwise
        // we would access kernel stack through user page tables!
        core::arch::asm!(
            "cli",                 // Mask interrupts during the transition
            "mov cr3, {cr3}",      // Switch to user address space
            "mov rcx, {entry}",    // RCX = user RIP for sysretq
            "mov rsp, {stack}",    // Set user stack (safe after CR3 switch)
            "mov r11d, 0x202",     // User RFLAGS with IF=1, reserved bit=1
            "xor rax, rax",        // Clear return value
            "sysretq",             // Return to Ring 3
            cr3 = in(reg) cr3,
            entry = in(reg) entry,
            stack = in(reg) stack,
            options(noreturn)
        );
    }
}

/// Jump to user mode (Ring 3) and execute code at given address
/// This function never returns - execution continues in user space
pub fn jump_to_usermode(entry: u64, stack: u64) -> ! {
    // Use kdebug! macro for direct serial output
    kdebug!(
        "[jump_to_usermode] ENTRY: entry={:#x}, stack={:#x}",
        entry,
        stack
    );

    kdebug!(
        "[jump_to_usermode] entry={:#018x} stack={:#018x}",
        entry,
        stack
    );

    // Set GS data for syscall and Ring 3 switching
    crate::gdt::debug_dump_selectors("jump_to_usermode");
    let selectors = unsafe { crate::gdt::get_selectors() };
    let user_code_sel = selectors.user_code_selector.0;
    let user_data_sel = selectors.user_data_selector.0;

    kdebug!(
        "[jump_to_usermode] user_code_selector.0={:04x}, user_data_selector.0={:04x}",
        user_code_sel,
        user_data_sel
    );

    kdebug!(
        "[jump_to_usermode] Setting GS_DATA: entry={:#x}, stack={:#x}, user_cs={:#x}, user_ds={:#x}",
        entry,
        stack,
        user_code_sel as u64 | 3,
        user_data_sel as u64 | 3
    );

    unsafe {
        crate::interrupts::set_gs_data(
            entry,
            stack,
            user_code_sel as u64 | 3,
            user_data_sel as u64 | 3,
            user_data_sel as u64 | 3,
        );

        // Set GS base to point to per-CPU GS_DATA for both kernel and user mode
        use x86_64::registers::model_specific::Msr;
        let gs_base = crate::smp::current_gs_data_ptr() as u64;
        Msr::new(0xc0000101).write(gs_base);
    }

    kdebug!("[jump_to_usermode] About to execute sysretq");

    unsafe {
        kdebug!("BEFORE_SYSRET");

        // CRITICAL FIX for exit syscall GP fault:
        // Don't manually set segment registers before sysretq!
        // sysretq automatically sets CS/SS from STAR MSR, and setting
        // DS/ES/FS/GS to user segments in kernel mode can cause GP faults.
        // Let the user program set DS/ES/FS after entering Ring 3.
        //
        // Ensure R11 (user RFLAGS) is programmed with the canonical 0x202
        // value explicitly to avoid allocator reuse that can leak stale bits
        // from prior syscalls and trigger a #GP during sysretq.
        //
        // Additionally, disable interrupts just before switching RSP to the
        // user stack so we never take an interrupt while still running in
        // kernel mode with a user-mode stack pointer. Otherwise the interrupt
        // handler would observe a bogus kernel stack and eventually crash with
        // an unpredictable #GP.
        core::arch::asm!(
            "cli",                 // Mask interrupts during the stack swap
            "mov rcx, {entry}",    // RCX = user RIP for sysretq
            "mov rsp, {stack}",    // Set user stack (now safe from interrupts)
            "mov r11d, 0x202",     // User RFLAGS with IF=1, reserved bit=1
            "xor rax, rax",        // Clear return value
            "sysretq",             // Return to Ring 3
            entry = in(reg) entry,
            stack = in(reg) stack,
            options(noreturn)
        );
    }
}

/// User process entry point and stack for Ring 3 switching
static mut USER_ENTRY: u64 = 0;
static mut USER_STACK: u64 = 0;

/// Get the stored user entry point
pub unsafe fn get_user_entry() -> u64 {
    use core::ptr::addr_of;
    core::ptr::read(addr_of!(USER_ENTRY))
}

/// Get the stored user stack
pub unsafe fn get_user_stack() -> u64 {
    use core::ptr::addr_of;
    core::ptr::read(addr_of!(USER_STACK))
}
