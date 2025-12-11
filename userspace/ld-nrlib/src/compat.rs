//! musl/glibc compatibility symbols for the NexaOS dynamic linker

use crate::elf::Elf64Phdr;
use crate::helpers::{cstr_len, print_str};
use crate::state::GLOBAL_SYMTAB;
use crate::symbol::global_symbol_lookup;
use crate::syscall::exit;

// ============================================================================
// Program Name Variables
// ============================================================================

/// Global program name (musl compatibility)
#[no_mangle]
pub static mut __progname: *const u8 = b"unknown\0".as_ptr();

/// Full program name (musl compatibility)
#[no_mangle]
pub static mut __progname_full: *const u8 = b"unknown\0".as_ptr();

/// Program invocation name (glibc compatibility)
#[no_mangle]
pub static mut program_invocation_name: *mut u8 = core::ptr::null_mut();

/// Program invocation short name (glibc compatibility)
#[no_mangle]
pub static mut program_invocation_short_name: *mut u8 = core::ptr::null_mut();

// ============================================================================
// DSO Handle
// ============================================================================

/// DSO handle for atexit (musl/glibc compatibility)
/// Using usize instead of *mut u8 to avoid Sync issues
#[no_mangle]
pub static __dso_handle: usize = 0;

// ============================================================================
// libc_start_main
// ============================================================================

/// __libc_start_main - glibc/musl entry point
/// This is called by _start in glibc-compiled programs
#[no_mangle]
pub unsafe extern "C" fn __libc_start_main(
    main: extern "C" fn(i32, *const *const u8, *const *const u8) -> i32,
    argc: i32,
    argv: *const *const u8,
    init: Option<extern "C" fn()>,
    fini: Option<extern "C" fn()>,
    rtld_fini: Option<extern "C" fn()>,
    stack_end: *mut u8,
) -> i32 {
    let _ = stack_end; // unused
    let _ = rtld_fini; // unused - we handle fini ourselves

    // Set up program name
    if argc > 0 && !argv.is_null() {
        let arg0 = *argv;
        if !arg0.is_null() {
            __progname_full = arg0;
            program_invocation_name = arg0 as *mut u8;

            // Find short name (after last '/')
            let mut short_name = arg0;
            let mut p = arg0;
            while *p != 0 {
                if *p == b'/' {
                    short_name = p.add(1);
                }
                p = p.add(1);
            }
            __progname = short_name;
            program_invocation_short_name = short_name as *mut u8;
        }
    }

    // Call init function if provided
    if let Some(init_fn) = init {
        init_fn();
    }

    // Calculate envp (after argv + NULL terminator)
    let envp = argv.add(argc as usize + 1);

    // Call main
    let ret = main(argc, argv, envp);

    // Call fini function if provided
    if let Some(fini_fn) = fini {
        fini_fn();
    }

    // Exit with return value
    exit(ret)
}

/// __libc_csu_init - called by __libc_start_main to run constructors
#[no_mangle]
pub unsafe extern "C" fn __libc_csu_init() {
    // Empty - init_array is handled by the dynamic linker
}

/// __libc_csu_fini - called to run destructors
#[no_mangle]
pub unsafe extern "C" fn __libc_csu_fini() {
    // Empty - fini_array is handled by the dynamic linker
}

// ============================================================================
// atexit Functions
// ============================================================================

/// __cxa_atexit - register a destructor function
#[no_mangle]
pub unsafe extern "C" fn __cxa_atexit(
    _func: Option<extern "C" fn(*mut u8)>,
    _arg: *mut u8,
    _dso_handle: *mut u8,
) -> i32 {
    0 // Success
}

/// __cxa_finalize - run registered destructors
#[no_mangle]
pub unsafe extern "C" fn __cxa_finalize(_dso_handle: *mut u8) {
    // Empty - destructor handling is simplified for now
}

/// atexit - register an exit handler
#[no_mangle]
pub unsafe extern "C" fn atexit(_func: Option<extern "C" fn()>) -> i32 {
    0 // Success - we don't track these for now
}

// ============================================================================
// Thread-local atexit Functions
// ============================================================================

/// Maximum number of TLS destructors that can be registered
const MAX_THREAD_ATEXIT: usize = 256;

/// TLS destructor entry
struct ThreadAtexitEntry {
    dtor: unsafe extern "C" fn(*mut core::ffi::c_void),
    obj: *mut core::ffi::c_void,
}

/// Global list of TLS destructors
static mut THREAD_ATEXIT_ENTRIES: [Option<ThreadAtexitEntry>; MAX_THREAD_ATEXIT] =
    [const { None }; MAX_THREAD_ATEXIT];
static mut THREAD_ATEXIT_COUNT: usize = 0;

/// __cxa_thread_atexit_impl - Register a destructor for a thread-local object
///
/// This is called by Rust's thread_local! macro to register TLS destructors.
#[no_mangle]
pub unsafe extern "C" fn __cxa_thread_atexit_impl(
    dtor: unsafe extern "C" fn(*mut core::ffi::c_void),
    obj: *mut core::ffi::c_void,
    _dso_handle: *mut core::ffi::c_void,
) -> i32 {
    if THREAD_ATEXIT_COUNT >= MAX_THREAD_ATEXIT {
        return -1; // No space left
    }

    THREAD_ATEXIT_ENTRIES[THREAD_ATEXIT_COUNT] = Some(ThreadAtexitEntry { dtor, obj });
    THREAD_ATEXIT_COUNT += 1;
    0 // Success
}

/// Run all registered TLS destructors (called on thread/program exit)
#[no_mangle]
pub unsafe extern "C" fn __cxa_thread_atexit_run() {
    while THREAD_ATEXIT_COUNT > 0 {
        THREAD_ATEXIT_COUNT -= 1;
        if let Some(entry) = THREAD_ATEXIT_ENTRIES[THREAD_ATEXIT_COUNT].take() {
            (entry.dtor)(entry.obj);
        }
    }
}

// ============================================================================
// Stack Protection
// ============================================================================

/// __stack_chk_guard - stack canary value
#[no_mangle]
pub static __stack_chk_guard: u64 = 0x00000aff0a0d0000;

/// __stack_chk_fail - called when stack smashing is detected
#[no_mangle]
pub unsafe extern "C" fn __stack_chk_fail() -> ! {
    print_str("[ld-nrlib] *** stack smashing detected ***\n");
    exit(127)
}

// ============================================================================
// Abort and Exit
// ============================================================================

/// abort - abnormal program termination
#[no_mangle]
pub unsafe extern "C" fn abort() -> ! {
    print_str("[ld-nrlib] abort() called\n");
    exit(134) // 128 + SIGABRT (6)
}

/// _exit - immediate program termination
#[no_mangle]
pub unsafe extern "C" fn _exit(status: i32) -> ! {
    exit(status)
}

/// _Exit - immediate program termination (C99)
#[no_mangle]
pub unsafe extern "C" fn _Exit(status: i32) -> ! {
    exit(status)
}

// ============================================================================
// Environment Variables
// ============================================================================

/// environ - environment pointer (musl/glibc compatibility)
#[no_mangle]
pub static mut environ: *mut *mut u8 = core::ptr::null_mut();

/// __environ - environment pointer (glibc compatibility alias)
#[no_mangle]
pub static mut __environ: *mut *mut u8 = core::ptr::null_mut();

/// _environ - environment pointer (alternative alias)
#[no_mangle]
pub static mut _environ: *mut *mut u8 = core::ptr::null_mut();

// ============================================================================
// Threading
// ============================================================================

/// __libc_single_threaded - indicate single-threaded mode
#[no_mangle]
pub static __libc_single_threaded: u8 = 1;

// ============================================================================
// errno
// ============================================================================

static mut ERRNO_VAL: i32 = 0;

#[no_mangle]
pub unsafe extern "C" fn __errno_location() -> *mut i32 {
    &mut ERRNO_VAL as *mut i32
}

/// ___errno - alternative errno accessor
#[no_mangle]
pub unsafe extern "C" fn ___errno() -> *mut i32 {
    &mut ERRNO_VAL as *mut i32
}

// ============================================================================
// dl_iterate_phdr
// ============================================================================

/// dl_iterate_phdr callback info structure
#[repr(C)]
pub struct DlPhdrInfo {
    pub dlpi_addr: u64,              // Base address of object
    pub dlpi_name: *const u8,        // Null-terminated name
    pub dlpi_phdr: *const Elf64Phdr, // Pointer to program headers
    pub dlpi_phnum: u16,             // Number of program headers
    // Additional fields for newer versions
    pub dlpi_adds: u64,         // Number of loads
    pub dlpi_subs: u64,         // Number of unloads
    pub dlpi_tls_modid: usize,  // TLS module ID
    pub dlpi_tls_data: *mut u8, // TLS data address
}

/// dl_iterate_phdr - iterate over loaded shared objects
#[no_mangle]
pub unsafe extern "C" fn dl_iterate_phdr(
    callback: extern "C" fn(*mut DlPhdrInfo, usize, *mut u8) -> i32,
    data: *mut u8,
) -> i32 {
    for i in 0..GLOBAL_SYMTAB.lib_count {
        let lib = &GLOBAL_SYMTAB.libs[i];
        if !lib.valid {
            continue;
        }

        let mut info = DlPhdrInfo {
            dlpi_addr: lib.base_addr,
            dlpi_name: if i == 0 {
                b"\0".as_ptr()
            } else {
                b"lib\0".as_ptr()
            },
            dlpi_phdr: core::ptr::null(),
            dlpi_phnum: 0,
            dlpi_adds: GLOBAL_SYMTAB.lib_count as u64,
            dlpi_subs: 0,
            dlpi_tls_modid: lib.dyn_info.tls_modid as usize,
            dlpi_tls_data: core::ptr::null_mut(),
        };

        let ret = callback(&mut info, core::mem::size_of::<DlPhdrInfo>(), data);
        if ret != 0 {
            return ret;
        }
    }
    0
}

// ============================================================================
// Dynamic Linking Functions
// ============================================================================

/// dlsym - look up symbol by name (simplified)
#[no_mangle]
pub unsafe extern "C" fn dlsym(_handle: *mut u8, name: *const u8) -> *mut u8 {
    if name.is_null() {
        return core::ptr::null_mut();
    }

    let name_len = cstr_len(name);
    let name_slice = core::slice::from_raw_parts(name, name_len);

    let addr = global_symbol_lookup(name_slice);
    addr as *mut u8
}

/// dlopen - open a shared library (stub)
#[no_mangle]
pub unsafe extern "C" fn dlopen(_filename: *const u8, _flags: i32) -> *mut u8 {
    core::ptr::null_mut()
}

/// dlclose - close a shared library (stub)
#[no_mangle]
pub unsafe extern "C" fn dlclose(_handle: *mut u8) -> i32 {
    0 // Success
}

/// dlerror - get last error message (stub)
#[allow(dead_code)]
static mut DLERROR_MSG: [u8; 64] = [0; 64];

#[no_mangle]
pub unsafe extern "C" fn dlerror() -> *const u8 {
    core::ptr::null()
}

// ============================================================================
// Signal Functions
// ============================================================================

/// __libc_current_sigrtmin - get minimum real-time signal number
#[no_mangle]
pub unsafe extern "C" fn __libc_current_sigrtmin() -> i32 {
    34 // SIGRTMIN on Linux
}

/// __libc_current_sigrtmax - get maximum real-time signal number
#[no_mangle]
pub unsafe extern "C" fn __libc_current_sigrtmax() -> i32 {
    64 // SIGRTMAX on Linux
}

// ============================================================================
// Fork Handlers
// ============================================================================

/// __register_atfork - register fork handlers (stub)
#[no_mangle]
pub unsafe extern "C" fn __register_atfork(
    _prepare: Option<extern "C" fn()>,
    _parent: Option<extern "C" fn()>,
    _child: Option<extern "C" fn()>,
    _dso_handle: *mut u8,
) -> i32 {
    0 // Success
}

/// pthread_atfork - register fork handlers (stub, alias)
#[no_mangle]
pub unsafe extern "C" fn pthread_atfork(
    _prepare: Option<extern "C" fn()>,
    _parent: Option<extern "C" fn()>,
    _child: Option<extern "C" fn()>,
) -> i32 {
    0 // Success
}
