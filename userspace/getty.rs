#![no_std]
#![no_main]
#![feature(lang_items)]

//! Getty - login prompt service
//! Displays login prompt and spawns login program for authentication

use core::arch::asm;
use core::panic::PanicInfo;

// System call numbers
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;

// Standard file descriptors
const STDIN: u64 = 0;
const STDOUT: u64 = 1;
const STDERR: u64 = 2;

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
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
fn syscall2(n: u64, a1: u64, a2: u64) -> u64 {
    syscall3(n, a1, a2, 0)
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
    if buf.is_empty() {
        return 0;
    }
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

/// Exit process
fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        unsafe { asm!("hlt") }
    }
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

/// Wait for process state change
fn wait4(pid: i64, status: *mut i32, options: i32) -> i64 {
    let ret = syscall3(SYS_WAIT4, pid as u64, status as u64, options as u64);
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

fn getty_main() -> ! {
    loop {
        // Display banner
        print("\n");
        print("\x1b[1;36m╔════════════════════════════════════════╗\x1b[0m\n");
        print("\x1b[1;36m║                                        ║\x1b[0m\n");
        print("\x1b[1;36m║          \x1b[1;37mWelcome to NexaOS\x1b[1;36m               ║\x1b[0m\n");
        print("\x1b[1;36m║                                        ║\x1b[0m\n");
        print("\x1b[1;36m║    \x1b[0mHybrid Kernel Operating System\x1b[1;36m        ║\x1b[0m\n");
        print("\x1b[1;36m║                                        ║\x1b[0m\n");
        print("\x1b[1;36m╚════════════════════════════════════════╝\x1b[0m\n");
        print("\n");

        // Fork and execute login
        let pid = fork();
        
        if pid < 0 {
            print("\x1b[1;31mError: Cannot fork login process\x1b[0m\n");
            exit(1);
        }
        
        if pid == 0 {
            // Child process - exec login
            let login_path = "/bin/login\0";
            let login_path_str = unsafe {
                core::str::from_utf8_unchecked(&login_path.as_bytes()[..11])
            };
            
            let argv: [*const u8; 2] = [
                login_path.as_ptr(),
                core::ptr::null(),
            ];
            let envp: [*const u8; 1] = [
                core::ptr::null(),
            ];
            
            execve(login_path_str, &argv, &envp);
            
            // If execve fails, exit and let init restart getty
            print("\x1b[1;31mError: Failed to execute /bin/login\x1b[0m\n");
            exit(1);
        }
        
        // Parent process - wait for login to complete
        let mut status: i32 = 0;
        wait4(pid, &mut status as *mut i32, 0);
        
        // Login exited, show prompt again
        print("\n\x1b[1;33mSession ended. Please login again.\x1b[0m\n");
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("\ngetty: PANIC\n");
    exit(1);
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    getty_main()
}

#[allow(dead_code)]
fn main() {
    loop {}
}
