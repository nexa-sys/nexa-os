//! Getty - login prompt service
//! Displays login prompt and spawns login program for authentication

use core::arch::asm;
use core::ptr;
use std::ffi::CStr;
use std::process;

// System call numbers
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;

/// Syscall wrapper
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
            clobber_abi("sysv64")
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

/// Exit process
fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    process::exit(code)
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
fn execve(path: &CStr, argv: &[*const u8], envp: &[*const u8]) -> i64 {
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

/// Wait for process state change
fn wait4(pid: i64, status: *mut i32, options: i32) -> i64 {
    let ret = syscall3(SYS_WAIT4, pid as u64, status as u64, options as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

// POSIX wait status macros
fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xff
}

fn wifexited(status: i32) -> bool {
    (status & 0x7f) == 0
}

fn wifsignaled(status: i32) -> bool {
    ((status & 0x7f) + 1) as i8 >= 2
}

fn wtermsig(status: i32) -> i32 {
    status & 0x7f
}

fn getty_main() -> ! {
    // Test output at the very beginning
    println!("GETTY_STARTING");

    loop {
        // Display banner
        println!();
        println!("\x1b[1;36m╔════════════════════════════════════════╗\x1b[0m");
        println!("\x1b[1;36m║                                        ║\x1b[0m");
        println!(
            "\x1b[1;36m║          \x1b[1;37mWelcome to NexaOS\x1b[1;36m               ║\x1b[0m"
        );
        println!("\x1b[1;36m║                                        ║\x1b[0m");
        println!("\x1b[1;36m║    \x1b[0mHybrid Kernel Operating System\x1b[1;36m        ║\x1b[0m");
        println!("\x1b[1;36m║                                        ║\x1b[0m");
        println!("\x1b[1;36m╚════════════════════════════════════════╝\x1b[0m");
        println!();

        // Fork and execute login
        let pid = fork();

        if pid < 0 {
            eprintln!("\x1b[1;31mError: Cannot fork login process\x1b[0m");
            exit(1);
        }

        if pid == 0 {
            // Child process - exec login
            let login_path = CStr::from_bytes_with_nul(b"/bin/login\0").expect("static login path");
            let argv: [*const u8; 2] = [login_path.as_ptr().cast(), ptr::null()];
            let envp: [*const u8; 1] = [ptr::null()];

            execve(&login_path, &argv, &envp);

            // If execve fails, exit and let init restart getty
            eprintln!("\x1b[1;31mError: Failed to execute /bin/login\x1b[0m");
            exit(1);
        }

        // Parent process - wait for login to complete
        let mut status: i32 = 0;
        wait4(pid, &mut status as *mut i32, 0);

        // Login exited, show prompt again
        println!();
        println!("\x1b[1;33mSession ended. Please login again.\x1b[0m");
    }
}

fn main() {
    getty_main()
}
