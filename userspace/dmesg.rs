//! dmesg - Print kernel ring buffer log
//!
//! This tool reads the kernel ring buffer and displays kernel log messages.
//! Similar to the Linux dmesg command.
//!
//! Usage:
//!   dmesg           - Display all kernel log messages
//!   dmesg -s SIZE   - Use SIZE for ring buffer query size
//!   dmesg -h        - Show help

use std::arch::asm;
use std::env;
use std::io::{self, Write};
use std::process;

// Syscall number for syslog (NexaOS specific)
const SYS_SYSLOG: u64 = 250;

// Syslog action types (Linux compatible)
const SYSLOG_ACTION_READ_ALL: i32 = 3;

// Default buffer size for reading logs (64KB max)
const DEFAULT_BUFFER_SIZE: usize = 65536;

/// Raw syscall wrapper for syslog
fn syscall_syslog(action: i32, buf: &mut [u8]) -> i64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") SYS_SYSLOG,
            in("rdi") action as u64,
            in("rsi") buf.as_mut_ptr() as u64,
            in("rdx") buf.len() as u64,
            lateout("rax") ret,
            clobber_abi("sysv64")
        );
    }
    if ret == u64::MAX {
        -1
    } else {
        ret as i64
    }
}

fn print_usage() {
    println!("dmesg - Print kernel ring buffer log");
    println!();
    println!("Usage: dmesg [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -s SIZE  Buffer size for reading (default: 65536)");
    println!("  -h       Show this help message");
    println!();
    println!("Examples:");
    println!("  dmesg          Display kernel messages");
    println!("  dmesg -s 8192  Read up to 8KB of log");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut buffer_size = DEFAULT_BUFFER_SIZE;

    // Parse arguments
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-s" {
            i += 1;
            if i >= args.len() {
                eprintln!("dmesg: -s requires a size argument");
                process::exit(1);
            }
            match args[i].parse::<usize>() {
                Ok(size) if size > 0 => {
                    buffer_size = size.min(DEFAULT_BUFFER_SIZE);
                }
                Ok(_) => {
                    eprintln!("dmesg: size must be positive");
                    process::exit(1);
                }
                Err(_) => {
                    eprintln!("dmesg: invalid size '{}'", args[i]);
                    process::exit(1);
                }
            }
        } else if arg.starts_with('-') {
            eprintln!("dmesg: unknown option: {}", arg);
            print_usage();
            process::exit(1);
        }
        i += 1;
    }

    // Allocate buffer and read kernel log
    let mut buffer = vec![0u8; buffer_size];
    let bytes_read = syscall_syslog(SYSLOG_ACTION_READ_ALL, &mut buffer);

    if bytes_read < 0 {
        eprintln!("dmesg: failed to read kernel log");
        process::exit(1);
    }

    if bytes_read == 0 {
        // No log data available
        process::exit(0);
    }

    // Write log to stdout
    let data = &buffer[..bytes_read as usize];
    io::stdout().write_all(data).unwrap_or_else(|e| {
        eprintln!("dmesg: write error: {}", e);
        process::exit(1);
    });

    // Ensure we end with a newline
    if !data.is_empty() && data[data.len() - 1] != b'\n' {
        println!();
    }
}
