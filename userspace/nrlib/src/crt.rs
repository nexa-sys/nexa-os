//! C Runtime (CRT) initialization for std programs
//!
//! Provides the _start entry point and environment setup needed for Rust std

use crate::{c_int, exit as sys_exit};
use core::arch::asm;

// Rust's generated main function (C ABI)
// For std programs, Rust generates a main function with C calling convention
extern "C" {
    fn main(argc: c_int, argv: *const *const u8) -> c_int;
}

/// Program entry point
/// This is called by the kernel/loader when the program starts
#[no_mangle]
#[link_section = ".text.startup"]
pub unsafe extern "C" fn _start() -> ! {
    // Our kernel doesn't set up argc/argv properly yet
    // For now, pass 0/NULL
    let argc: c_int = 0;
    let argv: *const *const u8 = core::ptr::null();

    // Call Rust's generated main (which calls lang_start -> user's main)
    let exit_code = main(argc, argv);

    // If main returns, exit with its return code
    sys_exit(exit_code)
}

/// Alternate entry point name (some systems use this)
#[no_mangle]
pub unsafe extern "C" fn _start_c() -> ! {
    _start()
}

// Provide the __rust_start_panic symbol that panic_abort needs
#[no_mangle]
pub extern "C" fn __rust_start_panic(_payload: usize) -> u32 {
    crate::abort()
}
