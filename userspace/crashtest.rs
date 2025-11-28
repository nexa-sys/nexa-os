//! Crash test program for testing kernel exception handlers
//! This program intentionally causes a segmentation fault to test that
//! the kernel properly handles user-mode crashes without panicking.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

// Syscall numbers
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Just exit on panic
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_EXIT,
            in("rdi") 1u64,
            options(noreturn)
        );
    }
}

fn write_str(s: &str) {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_WRITE,
            in("rdi") 1u64, // stdout
            in("rsi") s.as_ptr(),
            in("rdx") s.len(),
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
}

fn exit(code: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_EXIT,
            in("rdi") code as u64,
            options(noreturn)
        );
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str("=== Crash Test Program ===\n");
    write_str("This program will now dereference a null pointer.\n");
    write_str("The kernel should handle this gracefully and terminate\n");
    write_str("the process WITHOUT crashing the kernel.\n\n");
    write_str("Triggering null pointer dereference in 3... 2... 1...\n");
    
    // Intentionally dereference a null pointer to trigger a page fault
    unsafe {
        let null_ptr: *const u32 = core::ptr::null();
        let _ = core::ptr::read_volatile(null_ptr);
    }
    
    // If we somehow get here (we shouldn't), exit with error
    write_str("ERROR: Reached code after null pointer dereference!\n");
    exit(1);
}
