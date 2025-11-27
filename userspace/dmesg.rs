//! dmesg - Print kernel ring buffer log
//!
//! This tool reads the kernel ring buffer and displays kernel log messages.
//! Similar to the Linux dmesg command.
//!
//! Usage:
//!   dmesg           - Display all kernel log messages
//!   dmesg -c        - Clear the ring buffer after reading (not implemented)
//!   dmesg -n LEVEL  - Set log level (not implemented)
//!   dmesg -s SIZE   - Use SIZE for ring buffer query size
//!   dmesg -h        - Show help

use std::arch::asm;

// Syscall numbers
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_SYSLOG: u64 = 250;

// Syslog action types
const SYSLOG_ACTION_READ_ALL: i32 = 3;
const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

// Default buffer size for reading logs (64KB max)
const DEFAULT_BUFFER_SIZE: usize = 65536;

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

fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

fn write_stdout(s: &str) {
    syscall3(SYS_WRITE, 1, s.as_ptr() as u64, s.len() as u64);
}

fn write_bytes(fd: u64, buf: &[u8]) {
    syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
}

fn exit(code: i32) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {}
}

fn syslog(action: i32, buf: &mut [u8]) -> i64 {
    let ret = syscall3(
        SYS_SYSLOG,
        action as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

fn syslog_get_size() -> i64 {
    let mut dummy = [0u8; 1];
    let ret = syscall3(
        SYS_SYSLOG,
        SYSLOG_ACTION_SIZE_BUFFER as u64,
        dummy.as_mut_ptr() as u64,
        0,
    );
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

fn print_usage() {
    write_stdout("dmesg - Print kernel ring buffer log\n");
    write_stdout("\n");
    write_stdout("Usage: dmesg [OPTIONS]\n");
    write_stdout("\n");
    write_stdout("Options:\n");
    write_stdout("  -s SIZE  Buffer size for reading (default: 65536)\n");
    write_stdout("  -h       Show this help message\n");
    write_stdout("\n");
    write_stdout("Examples:\n");
    write_stdout("  dmesg          Display kernel messages\n");
    write_stdout("  dmesg -s 8192  Read up to 8KB of log\n");
}

fn parse_number(s: &[u8]) -> Option<usize> {
    let mut result: usize = 0;
    for &c in s {
        if c >= b'0' && c <= b'9' {
            result = result.checked_mul(10)?;
            result = result.checked_add((c - b'0') as usize)?;
        } else {
            return None;
        }
    }
    Some(result)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut buffer_size = DEFAULT_BUFFER_SIZE;
    let mut i = 1;

    // Parse arguments
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            print_usage();
            exit(0);
        } else if arg == "-s" {
            i += 1;
            if i >= args.len() {
                write_stdout("dmesg: -s requires a size argument\n");
                exit(1);
            }
            if let Some(size) = parse_number(args[i].as_bytes()) {
                buffer_size = size;
                if buffer_size > DEFAULT_BUFFER_SIZE {
                    buffer_size = DEFAULT_BUFFER_SIZE;
                }
                if buffer_size == 0 {
                    buffer_size = DEFAULT_BUFFER_SIZE;
                }
            } else {
                write_stdout("dmesg: invalid size\n");
                exit(1);
            }
        } else if arg.starts_with("-") {
            write_stdout("dmesg: unknown option: ");
            write_stdout(arg);
            write_stdout("\n");
            print_usage();
            exit(1);
        }
        i += 1;
    }

    // Allocate buffer
    let mut buffer = vec![0u8; buffer_size];

    // Read kernel log
    let bytes_read = syslog(SYSLOG_ACTION_READ_ALL, &mut buffer);

    if bytes_read < 0 {
        write_stdout("dmesg: failed to read kernel log\n");
        exit(1);
    }

    if bytes_read == 0 {
        // No log data available
        exit(0);
    }

    // Write log to stdout
    let data_len = bytes_read as usize;
    write_bytes(1, &buffer[..data_len]);

    // Ensure we end with a newline if there isn't one
    if data_len > 0 && buffer[data_len - 1] != b'\n' {
        write_stdout("\n");
    }

    exit(0);
}
