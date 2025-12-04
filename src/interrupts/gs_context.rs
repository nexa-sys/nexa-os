//! GS Data Context Management
//!
//! This module manages the GS segment data block used for fast syscall/sysret
//! transitions between user and kernel mode. The GS_DATA structure stores
//! user-mode context (RSP, RIP, RFLAGS) and kernel stack pointers.

use x86_64::instructions::port::Port;

// Offsets (in u64 slots) within the GS_DATA scratchpad.
// Keeping these as explicit constants prevents accidental drift between the
// assembly fast paths and the Rust helpers that maintain user/kernel context.
pub const GS_SLOT_USER_RSP: usize = 0;
pub const GS_SLOT_KERNEL_RSP: usize = 1;
pub const GS_SLOT_USER_ENTRY: usize = 2;
pub const GS_SLOT_USER_STACK: usize = 3;
pub const GS_SLOT_USER_CS: usize = 4;
pub const GS_SLOT_USER_SS: usize = 5;
pub const GS_SLOT_USER_DS: usize = 6;
pub const GS_SLOT_SAVED_RCX: usize = 7;
pub const GS_SLOT_SAVED_RFLAGS: usize = 8;
pub const GS_SLOT_USER_RSP_DEBUG: usize = 9;
pub const GS_SLOT_SAVED_RAX: usize = 10; // For fork child return value
// Slots 11-19: User registers saved for fork() - allows child process to inherit parent's register state
// (These are used by syscall instruction path in syscalls/mod.rs)
pub const GS_SLOT_SAVED_RDI: usize = 11;
pub const GS_SLOT_SAVED_RSI: usize = 12;
pub const GS_SLOT_SAVED_RDX: usize = 13;
pub const GS_SLOT_SAVED_RBX: usize = 14;
pub const GS_SLOT_SAVED_RBP: usize = 15;
pub const GS_SLOT_SAVED_R8: usize = 16;
pub const GS_SLOT_SAVED_R9: usize = 17;
pub const GS_SLOT_SAVED_R10: usize = 18;
pub const GS_SLOT_SAVED_R12: usize = 19;
// More slots if needed
pub const GS_SLOT_KERNEL_STACK_GUARD: usize = 20;
pub const GS_SLOT_KERNEL_STACK_SNAPSHOT: usize = 21;
// Slots 22-27: Callee-saved registers from int 0x81 path (syscall_interrupt_handler)
// These preserve the ORIGINAL register values before syscall wrapper modified them
// offset 176 = slot 22, 184 = slot 23, etc.
pub const GS_SLOT_INT81_RBX: usize = 22;  // offset 176
pub const GS_SLOT_INT81_RBP: usize = 23;  // offset 184
pub const GS_SLOT_INT81_R12: usize = 24;  // offset 192
pub const GS_SLOT_INT81_R13: usize = 25;  // offset 200
pub const GS_SLOT_INT81_R14: usize = 26;  // offset 208
pub const GS_SLOT_INT81_R15: usize = 27;  // offset 216

pub const GUARD_SOURCE_INT_GATE: u64 = 0;
pub const GUARD_SOURCE_SYSCALL: u64 = 1;

/// Write a u64 value as hexadecimal to a serial port (unsafe, low-level)
pub unsafe fn write_hex_u64(port: &mut Port<u8>, value: u64) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as usize;
        port.write(HEX[nibble]);
    }
}

/// Encode a u64 value as hexadecimal into a buffer
pub fn encode_hex_u64(value: u64, buf: &mut [u8; 16]) {
    for (idx, byte) in buf.iter_mut().enumerate() {
        let shift = (15 - idx) * 4;
        let nibble = ((value >> shift) & 0xF) as u8;
        *byte = match nibble {
            0..=9 => b'0' + nibble,
            _ => b'A' + (nibble - 10),
        };
    }
}

/// Set GS data for Ring 3 switch
/// Uses the current CPU's GS_DATA structure
pub unsafe fn set_gs_data(entry: u64, stack: u64, user_cs: u64, user_ss: u64, user_ds: u64) {
    // Get kernel stack from TSS privilege stack table for current CPU
    let cpu_id = crate::smp::current_cpu_id() as usize;
    let kernel_stack = crate::gdt::get_kernel_stack_top(cpu_id);

    // Get per-CPU GS_DATA pointer
    let gs_data_ptr = crate::smp::current_gs_data_ptr();

    unsafe {
        gs_data_ptr.add(GS_SLOT_USER_RSP).write(stack);
        gs_data_ptr.add(GS_SLOT_USER_RSP_DEBUG).write(stack);
        gs_data_ptr.add(GS_SLOT_KERNEL_RSP).write(kernel_stack);
        gs_data_ptr.add(GS_SLOT_USER_ENTRY).write(entry);
        gs_data_ptr.add(GS_SLOT_USER_STACK).write(stack);
        gs_data_ptr.add(GS_SLOT_USER_CS).write(user_cs);
        gs_data_ptr.add(GS_SLOT_USER_SS).write(user_ss);
        gs_data_ptr.add(GS_SLOT_USER_DS).write(user_ds);
        gs_data_ptr.add(GS_SLOT_SAVED_RCX).write(0);
        gs_data_ptr.add(GS_SLOT_SAVED_RFLAGS).write(0);
        gs_data_ptr.add(GS_SLOT_KERNEL_STACK_GUARD).write(0);
        gs_data_ptr.add(GS_SLOT_KERNEL_STACK_SNAPSHOT).write(0);
    }
}

/// Set GS data for Ring 3 switch
///
/// This function is called by the scheduler to prepare the GS_DATA structure
/// for a return to userspace via sysretq.
///
/// # Safety
/// This function writes to the GS_DATA structure which is used by the CPU
/// for syscall/sysret operations. Incorrect values can cause kernel crashes.
pub unsafe extern "C" fn restore_user_syscall_context(rip: u64, rsp: u64, rflags: u64) {
    let gs_ptr = crate::smp::current_gs_data_ptr() as *mut u64;
    gs_ptr.add(GS_SLOT_SAVED_RCX).write(rip);
    gs_ptr.add(GS_SLOT_USER_RSP).write(rsp);
    gs_ptr.add(GS_SLOT_USER_RSP_DEBUG).write(rsp);
    gs_ptr.add(GS_SLOT_SAVED_RFLAGS).write(rflags);
    gs_ptr.add(GS_SLOT_KERNEL_STACK_GUARD).write(0);
    gs_ptr.add(GS_SLOT_KERNEL_STACK_SNAPSHOT).write(0);
}

/// Called when the kernel stack guard detects a re-entry condition
/// This is a fatal error - the system cannot recover from this state
#[cold]
#[no_mangle]
pub extern "C" fn kernel_stack_guard_reentry_fail(source: u64) -> ! {
    // Use per-CPU GS_DATA
    let base_ptr = crate::smp::current_gs_data_ptr() as *const u64;
    let guard_val = unsafe { base_ptr.add(GS_SLOT_KERNEL_STACK_GUARD).read_volatile() };
    let snapshot = unsafe { base_ptr.add(GS_SLOT_KERNEL_STACK_SNAPSHOT).read_volatile() };
    let pid = crate::scheduler::current_pid();

    let source_desc = match source {
        GUARD_SOURCE_INT_GATE => "int 0x81",
        GUARD_SOURCE_SYSCALL => "syscall fastpath",
        _ => "unknown",
    };

    let current_rsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) current_rsp, options(nostack, preserves_flags));
    }

    crate::kfatal!(
        "Kernel stack guard violation detected on {} (pid={:?}, guard={}, snapshot={:#x}, rsp={:#x})",
        source_desc,
        pid,
        guard_val,
        snapshot,
        current_rsp
    );

    loop {
        x86_64::instructions::hlt();
    }
}
