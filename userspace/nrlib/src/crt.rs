//! C Runtime (CRT) initialization for std programs
//!
//! Provides the `_start` entry point and environment plumbing so Rust `std`
//! binaries launched by the NexaOS kernel receive the expected argc/argv/envp.

use crate::{c_char, c_int, exit as sys_exit};
use core::arch::global_asm;
use core::cmp;

// Rust's generated main function (C ABI)
// For std programs, Rust generates a main function with C calling convention
extern "C" {
    fn main(argc: c_int, argv: *const *const u8) -> c_int;
}

// Raw entry point reached immediately after the kernel jumps into the binary.
// Defined in hand-written assembly so we can preserve the initial stack pointer
// before Rust emits any prologue.
// Note: We use .protected visibility to ensure symbol is exported in shared library
#[cfg(target_arch = "x86_64")]
global_asm!(
    ".section .text.startup,\"ax\",@progbits",
    ".globl _start",
    ".protected _start",
    ".type _start,@function",
    "_start:",
    "\t.cfi_startproc",
    "\tmov rbx, rsp",             // Preserve userspace stack pointer (points at argc)
    "\tand rsp, -16",             // Realign the stack prior to calling into Rust
    "\tmov rdi, rbx",             // Pass original stack pointer as first arg
    "\txor rbp, rbp",             // Clear frame pointer to aid unwinding/debug
    "\tcall __nexa_crt_start",
    "\tud2",                      // Should never return; trap if it does
    "\t.cfi_endproc",
    "\t.size _start, .-_start",
    ".globl _start_c",
    ".protected _start_c",
    ".type _start_c,@function",
    "_start_c:",
    "\tjmp _start",
    "\t.size _start_c, .-_start_c",
);

// Force the linker to export _start and _start_c by referencing them
extern "C" {
    fn _start() -> !;
    fn _start_c() -> !;
}

/// Get the address of _start (for dynamic linker to find the entry point)
#[no_mangle]
pub extern "C" fn __nexa_get_start_addr() -> usize {
    unsafe { _start as usize }
}

/// Get the address of _start_c (for compatibility)
#[no_mangle]
pub extern "C" fn __nexa_get_start_c_addr() -> usize {
    unsafe { _start_c as usize }
}

/// Decode argc/argv/envp from the preserved userspace stack and invoke `main`.
#[no_mangle]
unsafe extern "C" fn __nexa_crt_start(stack_ptr: *const usize) -> ! {
    if stack_ptr.is_null() {
        let exit_code = main(0, core::ptr::null());
        sys_exit(exit_code)
    }

    let argc_raw = unsafe { *stack_ptr } as u64;
    let argv = unsafe { stack_ptr.add(1) as *const *const u8 };
    let envp = unsafe { argv.add(argc_raw as usize + 1) };

    // Update the global environ pointers expected by libc-compatible code.
    let envp_mut = envp as *mut *mut c_char;
    unsafe {
        crate::environ = envp_mut;
        crate::__environ = envp_mut;
    }

    let argc = cmp::min(argc_raw, i32::MAX as u64) as c_int;

    // Call Rust's generated main (which calls lang_start -> user's main)
    let exit_code = main(argc, argv);

    // If main returns, exit with its return code
    sys_exit(exit_code)
}

// Provide the __rust_start_panic symbol that panic_abort needs
#[no_mangle]
pub extern "C" fn __rust_start_panic(_payload: usize) -> u32 {
    crate::abort()
}
