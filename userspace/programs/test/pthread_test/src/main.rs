//! Direct pthread test - bypasses std::thread completely
//! Uses raw pthread_create to test the underlying threading implementation

// Global flag to verify thread ran - using raw atomic operation
static mut THREAD_RESULT: u32 = 0;

// pthread types (must match nrlib)
type pthread_t = u64;
type pthread_attr_t = *const ();
type c_void = u8;
type c_int = i32;

extern "C" {
    fn pthread_create(
        thread: *mut pthread_t,
        attr: pthread_attr_t,
        start_routine: extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
    ) -> c_int;

    fn pthread_join(thread: pthread_t, retval: *mut *mut c_void) -> c_int;
}

// Use raw syscall for output to avoid any TLS issues
fn raw_print(s: &[u8]) {
    unsafe {
        nrlib::syscall3(1, 1, s.as_ptr() as u64, s.len() as u64);
    }
}

fn raw_print_hex(val: u64) {
    let mut buf = [0u8; 16];
    let hex = b"0123456789abcdef";
    for i in 0..16 {
        buf[15 - i] = hex[(val >> (i * 4) & 0xf) as usize];
    }
    raw_print(&buf);
}

// Thread function - completely standalone, no std dependencies inside
// Use volatile write to avoid any atomic machinery
extern "C" fn thread_func(arg: *mut c_void) -> *mut c_void {
    raw_print(b"\n[pthread_test] ==== THREAD RUNNING ====\n");
    raw_print(b"[pthread_test] arg = 0x");
    raw_print_hex(arg as u64);
    raw_print(b"\n");

    // Store result using volatile write to avoid any std atomic machinery
    unsafe {
        core::ptr::write_volatile(&mut THREAD_RESULT, 0xDEADBEEF);
    }

    raw_print(b"[pthread_test] Thread completing successfully!\n");

    // Return the arg as the result
    arg
}

fn main() {
    raw_print(b"\n\n========================================\n");
    raw_print(b"    Direct pthread_create Test\n");
    raw_print(b"========================================\n\n");

    let mut thread_id: pthread_t = 0;
    let test_arg: *mut c_void = 0xCAFEBABE as *mut c_void;

    raw_print(b"[pthread_test] Creating thread with arg=0x");
    raw_print_hex(test_arg as u64);
    raw_print(b"\n");

    let ret = unsafe { pthread_create(&mut thread_id, core::ptr::null(), thread_func, test_arg) };

    raw_print(b"[pthread_test] pthread_create returned: ");
    raw_print_hex(ret as u64);
    raw_print(b"\n");

    if ret != 0 {
        raw_print(b"[pthread_test] FAILED: pthread_create error!\n");
        std::process::exit(1);
    }

    raw_print(b"[pthread_test] Thread ID: 0x");
    raw_print_hex(thread_id);
    raw_print(b"\n");

    raw_print(b"[pthread_test] Calling pthread_join...\n");

    let mut retval: *mut c_void = core::ptr::null_mut();
    let join_ret = unsafe { pthread_join(thread_id, &mut retval) };

    raw_print(b"[pthread_test] pthread_join returned: ");
    raw_print_hex(join_ret as u64);
    raw_print(b"\n");

    raw_print(b"[pthread_test] Thread retval: 0x");
    raw_print_hex(retval as u64);
    raw_print(b"\n");

    // Check the global flag using volatile read
    let result = unsafe { core::ptr::read_volatile(&THREAD_RESULT) };
    raw_print(b"[pthread_test] THREAD_RESULT: 0x");
    raw_print_hex(result as u64);
    raw_print(b"\n");

    if result == 0xDEADBEEF {
        raw_print(b"\n========================================\n");
        raw_print(b"    SUCCESS! Thread executed correctly!\n");
        raw_print(b"========================================\n\n");
    } else {
        raw_print(b"\n========================================\n");
        raw_print(b"    FAILED: Thread did not execute\n");
        raw_print(b"========================================\n\n");
        std::process::exit(2);
    }
}
