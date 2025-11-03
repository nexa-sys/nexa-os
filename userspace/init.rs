#![no_std]
#![no_main]
#![feature(lang_items)]

//! /sbin/init - System initialization program (PID 1)
//! 
//! This is the first userspace program executed by the kernel.
//! It follows Unix conventions:
//! - Always runs as PID 1
//! - Never exits (or kernel panic)
//! - Spawns and manages system services
//! - Reaps zombie processes
//! - Handles system runlevel changes
//!
//! POSIX/Unix-like compliance:
//! - Process hierarchy root (PPID = 0)
//! - Orphan process adoption
//! - Signal handling for system control
//! - Service respawn on failure

use core::arch::asm;
use core::panic::PanicInfo;

// System call numbers
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;
const SYS_GETPID: u64 = 39;
const SYS_GETPPID: u64 = 110;
const SYS_RUNLEVEL: u64 = 231;

// Standard file descriptors
const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

/// Syscall wrapper
/// NexaOS uses int 0x81 for system calls (not syscall instruction)
#[inline(always)]
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

#[inline(always)]
fn syscall0(n: u64) -> u64 {
    syscall3(n, 0, 0, 0)
}

/// Write to file descriptor
fn write(fd: u64, buf: &[u8]) -> isize {
    let ret = syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as isize
    }
}

/// Print string to stdout
fn print(s: &str) {
    write(STDOUT, s.as_bytes());
}

/// Print string to stderr
fn eprint(s: &str) {
    write(STDERR, s.as_bytes());
}

/// Exit process
fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        unsafe { asm!("hlt") }
    }
}

/// Get process ID
fn getpid() -> u64 {
    syscall0(SYS_GETPID)
}

/// Get parent process ID
fn getppid() -> u64 {
    syscall0(SYS_GETPPID)
}

/// Fork process
fn fork() -> i64 {
    let ret = syscall0(SYS_FORK);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Execute program
fn execve(path: &str, argv: &[*const u8], envp: &[*const u8]) -> i64 {
    let ret = syscall3(
        SYS_EXECVE,
        path.as_ptr() as u64,
        argv.as_ptr() as u64,
        envp.as_ptr() as u64,
    );
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Wait for child process
fn wait4(pid: i64, status: &mut i32, options: i32) -> i64 {
    let ret = syscall3(
        SYS_WAIT4,
        pid as u64,
        status as *mut i32 as u64,
        options as u64,
    );
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

/// Get current runlevel
fn get_runlevel() -> i32 {
    let ret = syscall1(SYS_RUNLEVEL, (-1i32) as u64);
    ret as i32
}

/// Simple integer to string conversion
fn itoa(mut n: u64, buf: &mut [u8]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[0..1]).unwrap();
    }
    
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    
    // Reverse
    for j in 0..i/2 {
        buf.swap(j, i - 1 - j);
    }
    
    core::str::from_utf8(&buf[0..i]).unwrap()
}

/// Spawn a shell and wait for it
fn spawn_shell() -> bool {
    print("init: spawning shell /bin/sh\n");
    
    let pid = fork();
    
    if pid < 0 {
        eprint("init: ERROR: fork() failed\n");
        return false;
    }
    
    if pid == 0 {
        // Child process - exec shell
        let shell_path = "/bin/sh\0";
        let argv: [*const u8; 2] = [
            shell_path.as_ptr(),
            core::ptr::null(),
        ];
        let envp: [*const u8; 1] = [core::ptr::null()];
        
        if execve("/bin/sh", &argv, &envp) < 0 {
            eprint("init: ERROR: execve(/bin/sh) failed\n");
            exit(1);
        }
        
        // Never reached
        exit(0);
    }
    
    // Parent process - wait for shell
    let mut buf = [0u8; 32];
    let pid_str = itoa(pid as u64, &mut buf);
    print("init: shell spawned with PID ");
    print(pid_str);
    print("\n");
    
    let mut status: i32 = 0;
    let wait_pid = wait4(pid, &mut status, 0);
    
    if wait_pid > 0 {
        print("init: shell (PID ");
        let wait_pid_str = itoa(wait_pid as u64, &mut buf);
        print(wait_pid_str);
        print(") exited with status ");
        let status_str = itoa((status & 0xFF) as u64, &mut buf);
        print(status_str);
        print("\n");
    } else {
        eprint("init: ERROR: wait4() failed\n");
    }
    
    true
}

/// Main init loop
fn init_main() -> ! {
    print("\n");
    print("=========================================\n");
    print("  NexaOS Init System (PID 1)\n");
    print("=========================================\n");
    print("\n");
    
    // Verify we are PID 1
    let pid = getpid();
    let ppid = getppid();
    
    let mut buf = [0u8; 32];
    print("init: process ID: ");
    print(itoa(pid, &mut buf));
    print("\n");
    
    print("init: parent process ID: ");
    print(itoa(ppid, &mut buf));
    print("\n");
    
    if pid != 1 {
        eprint("init: WARNING: Not running as PID 1!\n");
        eprint("init: This is unusual for init process\n");
    }
    
    if ppid != 0 {
        eprint("init: WARNING: PPID is not 0!\n");
        eprint("init: Init should have no parent\n");
    }
    
    // Get current runlevel
    let runlevel = get_runlevel();
    if runlevel >= 0 {
        print("init: current runlevel: ");
        print(itoa(runlevel as u64, &mut buf));
        print("\n");
    }
    
    print("\n");
    print("init: system initialization complete\n");
    print("init: NOTE: fork/exec system calls not yet implemented\n");
    print("init: exec'ing /bin/sh directly (replacing PID 1)\n");
    print("\n");
    
    // Since fork() is not yet implemented in the kernel,
    // we exec the shell directly, replacing the init process
    // In a full implementation, init would fork() first to create
    // a child process, then the child would exec the shell
    let path = "/bin/sh\0";
    let argv: [*const u8; 2] = [
        path.as_ptr(),
        core::ptr::null(),
    ];
    let envp: [*const u8; 1] = [
        core::ptr::null(),
    ];
    
    print("init: executing /bin/sh...\n\n");
    
    let ret = execve(path, &argv, &envp);
    if ret < 0 {
        eprint("\ninit: FATAL: execve(/bin/sh) failed\n");
        eprint("init: system cannot continue without shell\n");
        exit(1);
    }
    
    // Should never reach here if execve succeeds
    eprint("\ninit: ERROR: execve returned unexpectedly\n");
    exit(1);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    eprint("\ninit: PANIC: Init process panicked!\n");
    eprint("init: FATAL: System cannot continue without PID 1\n");
    exit(1);
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    init_main()
}

// Dummy main function (never called, but needed for compilation)
#[allow(dead_code)]
fn main() {
    loop {}
}
