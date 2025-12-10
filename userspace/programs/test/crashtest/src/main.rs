//! Crash test program for testing kernel exception handlers
//! This program intentionally causes a segmentation fault to test that
//! the kernel properly handles user-mode crashes without panicking.

fn main() {
    println!("=== Crash Test Program ===");
    println!("This program will now dereference a null pointer.");
    println!("The kernel should handle this gracefully and terminate");
    println!("the process WITHOUT crashing the kernel.");
    println!();
    println!("Triggering null pointer dereference in 3... 2... 1...");

    // Intentionally dereference a null pointer to trigger a page fault
    unsafe {
        let null_ptr: *const u32 = core::ptr::null();
        let _ = core::ptr::read_volatile(null_ptr);
    }

    // If we somehow get here (we shouldn't), exit with error
    println!("ERROR: Reached code after null pointer dereference!");
    std::process::exit(1);
}
