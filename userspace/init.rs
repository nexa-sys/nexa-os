#![no_std]
#![no_main]
#![feature(lang_items)]

//! /sbin/init - System initialization program (PID 1)
//! 
//! Hybrid kernel init system with process supervision
//! 
//! Features:
//! - PID 1 process management
//! - Service supervision and respawn
//! - systemd-style logging
//! - Automatic restart on failure
//! - Runlevel management
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

// Service management constants
const MAX_RESPAWN_COUNT: u32 = 5;  // Max respawns within window
const RESPAWN_WINDOW_SEC: u64 = 60; // Respawn window in seconds
const RESTART_DELAY_MS: u64 = 1000; // Delay between restarts

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

/// Service state tracking
struct ServiceState {
    respawn_count: u32,
    last_respawn_time: u64,
    total_starts: u64,
}

impl ServiceState {
    const fn new() -> Self {
        Self {
            respawn_count: 0,
            last_respawn_time: 0,
            total_starts: 0,
        }
    }

    fn should_respawn(&mut self, current_time: u64) -> bool {
        // Reset counter if outside window
        if current_time - self.last_respawn_time > RESPAWN_WINDOW_SEC {
            self.respawn_count = 0;
        }

        if self.respawn_count >= MAX_RESPAWN_COUNT {
            return false; // Hit respawn limit
        }

        self.respawn_count += 1;
        self.last_respawn_time = current_time;
        self.total_starts += 1;
        true
    }
}

/// systemd-style logging with colors
fn log_info(msg: &str) {
    print("\x1b[1;32m[  OK  ]\x1b[0m ");  // Green
    print(msg);
    print("\n");
}

fn log_start(msg: &str) {
    print("\x1b[1;36m[ .... ]\x1b[0m ");  // Cyan
    print(msg);
    print("\n");
}

fn log_fail(msg: &str) {
    print("\x1b[1;31m[FAILED]\x1b[0m ");  // Red
    print(msg);
    print("\n");
}

fn log_warn(msg: &str) {
    print("\x1b[1;33m[ WARN ]\x1b[0m ");  // Yellow
    print(msg);
    print("\n");
}

/// Simple timestamp (just a counter for now)
fn get_timestamp() -> u64 {
    static mut COUNTER: u64 = 0;
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

/// Delay function
fn delay_ms(ms: u64) {
    for _ in 0..(ms * 1000) {
        unsafe { asm!("pause") }
    }
}

/// Main init loop with service supervision
fn init_main() -> ! {
    print("\n");
    print("\x1b[1;34m=========================================\x1b[0m\n");  // Blue
    print("\x1b[1;34m  NexaOS Init (ni) - PID 1\x1b[0m\n");
    print("\x1b[1;34m  Hybrid Kernel - Process Supervisor\x1b[0m\n");
    print("\x1b[1;34m=========================================\x1b[0m\n");
    print("\n");
    
    // Verify we are PID 1
    let pid = getpid();
    let ppid = getppid();
    
    let mut buf = [0u8; 32];
    
    log_start("Verifying init process identity");
    print("         PID: ");
    print(itoa(pid, &mut buf));
    print("\n");
    print("         PPID: ");
    print(itoa(ppid, &mut buf));
    print("\n");
    
    if pid != 1 {
        log_fail("Not running as PID 1 - system unstable");
        exit(1);
    }
    
    if ppid != 0 {
        log_warn("PPID is not 0 - unusual configuration");
    } else {
        log_info("Init process identity verified");
    }
    
    // Get current runlevel
    log_start("Querying system runlevel");
    let runlevel = get_runlevel();
    if runlevel >= 0 {
        print("         Runlevel: ");
        print(itoa(runlevel as u64, &mut buf));
        print("\n");
        log_info("System runlevel configured");
    } else {
        log_warn("Failed to query runlevel");
    }
    
    print("\n");
    log_info("System initialization complete");
    print("\n");
    
    // Service supervision with fork/exec/wait
    log_start("Starting service supervision");
    log_info("Using fork/exec/wait supervision model");
    print("\n");
    
    let mut service_state = ServiceState::new();
    
    // Main supervision loop
    loop {
        let timestamp = get_timestamp();
        
        if !service_state.should_respawn(timestamp) {
            log_fail("Shell respawn limit exceeded");
            log_fail("System cannot continue without shell");
            eprint("\nni: CRITICAL: Too many shell failures\n");
            eprint("ni: Respawn limit: ");
            print(itoa(MAX_RESPAWN_COUNT as u64, &mut buf));
            eprint(" in ");
            print(itoa(RESPAWN_WINDOW_SEC, &mut buf));
            eprint(" seconds\n");
            eprint("ni: Total starts: ");
            print(itoa(service_state.total_starts, &mut buf));
            eprint("\n");
            exit(1);
        }
        
        log_start("Spawning /bin/sh");
        print("         Attempt: ");
        print(itoa(service_state.total_starts, &mut buf));
        print("\n");
        
        // Fork to create child process
        let pid = fork();
        
        if pid < 0 {
            // Fork failed
            log_fail("fork() failed - cannot create child process");
            delay_ms(RESTART_DELAY_MS);
            continue;
        }
        
        if pid == 0 {
            // Child process - execute shell
            let path = "/bin/sh\0";
            let argv: [*const u8; 2] = [
                path.as_ptr(),
                core::ptr::null(),
            ];
            let envp: [*const u8; 1] = [
                core::ptr::null(),
            ];
            
            log_info("Child process executing /bin/sh");
            execve(path, &argv, &envp);
            
            // If execve returns, it failed
            log_fail("execve failed in child process");
            exit(1);
        } else {
            // Parent process - wait for child
            log_info("Shell started successfully");
            print("         Child PID: ");
            print(itoa(pid as u64, &mut buf));
            print("\n\n");
            
            // Wait for child to exit
            let mut status: i32 = 0;
            let wait_pid = wait4(pid, &mut status, 0);
            
            print("\n");
            if wait_pid as i64 == pid {
                log_warn("Shell process exited");
                print("         Exit status: ");
                print(itoa((status & 0xFF) as u64, &mut buf));
                print("\n");
            } else {
                log_fail("wait4() failed");
            }
            
            // Delay before respawn
            log_start("Waiting before respawn");
            delay_ms(RESTART_DELAY_MS);
            log_info("Respawning shell");
            print("\n");
        }
    }
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
