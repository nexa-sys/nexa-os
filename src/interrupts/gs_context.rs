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
pub const GS_SLOT_KERNEL_STACK_GUARD: usize = 20;
pub const GS_SLOT_KERNEL_STACK_SNAPSHOT: usize = 21;

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
pub unsafe fn set_gs_data(entry: u64, stack: u64, user_cs: u64, user_ss: u64, user_ds: u64) {
    // Get kernel stack from TSS privilege stack table (BSP = CPU 0)
    let kernel_stack = crate::gdt::get_kernel_stack_top(0);

    // Get GS_DATA address without creating a reference that might corrupt nearby statics
    let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
    let gs_data_ptr = gs_data_addr as *mut u64;

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

/// Restore saved user-mode return context to the GS data block so the fast
/// syscall paths can return with the correct RIP/RSP/RFLAGS after a context
/// switch. This is called by the scheduler before resuming a process that has
/// already entered userspace at least once.
///
/// Restore user syscall context for process switch (used by scheduler)
/// Sets up GS_DATA so that when we return to userspace, registers are restored correctly
/// rax_value: the value to return in RAX register (e.g., 0 for fork child)
pub fn restore_user_syscall_context(user_rip: u64, user_rsp: u64, user_rflags: u64) {
    unsafe {
        let gs_data_addr = &raw const crate::initramfs::GS_DATA.0 as *const _ as u64;
        let gs_data_ptr = gs_data_addr as *mut u64;

        gs_data_ptr.add(GS_SLOT_USER_RSP).write(user_rsp);
        gs_data_ptr.add(GS_SLOT_USER_RSP_DEBUG).write(user_rsp);
        gs_data_ptr.add(GS_SLOT_SAVED_RCX).write(user_rip);
        gs_data_ptr.add(GS_SLOT_SAVED_RFLAGS).write(user_rflags);
        gs_data_ptr.add(GS_SLOT_KERNEL_STACK_GUARD).write(0);
        gs_data_ptr.add(GS_SLOT_KERNEL_STACK_SNAPSHOT).write(0);
        // Note: RAX is NOT set here - it comes from the process context
        // For fork children, it's set in the Context.rax field
    }
}

/// Called when the kernel stack guard detects a re-entry condition
/// This is a fatal error - the system cannot recover from this state
#[cold]
#[no_mangle]
pub extern "C" fn kernel_stack_guard_reentry_fail(source: u64) -> ! {
    let base_ptr = unsafe { core::ptr::addr_of!(crate::initramfs::GS_DATA.0) as *const u64 };
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
