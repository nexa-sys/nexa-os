//! kill - Send signals to processes
//!
//! Usage:
//!   kill [-s signal | -signal] pid...
//!   kill -l [signal]
//!
//! Sends a signal to each specified process.
//! Default signal is SIGTERM (15).

use std::env;
use std::io::{self, Write};
use std::process;

/// Signal name to number mapping
const SIGNAL_MAP: &[(&str, i32)] = &[
    ("HUP", 1),
    ("INT", 2),
    ("QUIT", 3),
    ("ILL", 4),
    ("TRAP", 5),
    ("ABRT", 6),
    ("BUS", 7),
    ("FPE", 8),
    ("KILL", 9),
    ("USR1", 10),
    ("SEGV", 11),
    ("USR2", 12),
    ("PIPE", 13),
    ("ALRM", 14),
    ("TERM", 15),
    ("CHLD", 17),
    ("CONT", 18),
    ("STOP", 19),
    ("TSTP", 20),
    ("TTIN", 21),
    ("TTOU", 22),
];

/// All signal names with numbers
const ALL_SIGNALS: &[(&str, i32)] = &[
    ("SIGHUP", 1),
    ("SIGINT", 2),
    ("SIGQUIT", 3),
    ("SIGILL", 4),
    ("SIGTRAP", 5),
    ("SIGABRT", 6),
    ("SIGBUS", 7),
    ("SIGFPE", 8),
    ("SIGKILL", 9),
    ("SIGUSR1", 10),
    ("SIGSEGV", 11),
    ("SIGUSR2", 12),
    ("SIGPIPE", 13),
    ("SIGALRM", 14),
    ("SIGTERM", 15),
    ("SIGCHLD", 17),
    ("SIGCONT", 18),
    ("SIGSTOP", 19),
    ("SIGTSTP", 20),
    ("SIGTTIN", 21),
    ("SIGTTOU", 22),
];

fn print_usage() {
    eprintln!("kill - Send signals to processes");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  kill [-s signal | -signal] pid...");
    eprintln!("  kill -l [signal]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -s signal   Specify signal name or number");
    eprintln!("  -signal     Signal number (e.g., -9 for SIGKILL)");
    eprintln!("  -l          List signal names");
    eprintln!("  -h, --help  Show this help message");
    eprintln!();
    eprintln!("Common signals:");
    eprintln!("   1 SIGHUP    Hangup");
    eprintln!("   2 SIGINT    Interrupt (Ctrl+C)");
    eprintln!("   3 SIGQUIT   Quit");
    eprintln!("   9 SIGKILL   Kill (cannot be caught)");
    eprintln!("  15 SIGTERM   Terminate (default)");
    eprintln!("  18 SIGCONT   Continue if stopped");
    eprintln!("  19 SIGSTOP   Stop (cannot be caught)");
}

fn list_signals(signal: Option<&str>) {
    match signal {
        Some(s) => {
            // Try to parse as number first
            if let Ok(num) = s.parse::<i32>() {
                // Find signal name for this number
                for (name, signum) in ALL_SIGNALS {
                    if *signum == num {
                        println!("{}", name.trim_start_matches("SIG"));
                        return;
                    }
                }
                eprintln!("kill: unknown signal: {}", num);
                process::exit(1);
            } else {
                // Try to find the signal by name
                let upper = s.to_uppercase();
                let name = upper.trim_start_matches("SIG");
                for (signame, signum) in ALL_SIGNALS {
                    if signame.trim_start_matches("SIG") == name {
                        println!("{}", signum);
                        return;
                    }
                }
                eprintln!("kill: unknown signal: {}", s);
                process::exit(1);
            }
        }
        None => {
            // List all signals
            for (i, (name, num)) in ALL_SIGNALS.iter().enumerate() {
                if i > 0 && i % 4 == 0 {
                    println!();
                }
                print!("{:2}) {:10} ", num, name);
            }
            println!();
        }
    }
}

fn parse_signal(s: &str) -> Result<i32, String> {
    // Try to parse as number
    if let Ok(num) = s.parse::<i32>() {
        if num >= 0 && num < 32 {
            return Ok(num);
        }
        return Err(format!("invalid signal number: {}", num));
    }

    // Try to parse as signal name (with or without SIG prefix)
    let upper = s.to_uppercase();
    let name = upper.trim_start_matches("SIG");
    
    for (signame, signum) in SIGNAL_MAP {
        if *signame == name {
            return Ok(*signum);
        }
    }

    Err(format!("unknown signal: {}", s))
}

fn kill_process(pid: i32, signal: i32) -> io::Result<()> {
    // Use the nrlib kill function through libc
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    
    let result = unsafe { kill(pid, signal) };
    
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut signal = 15; // Default: SIGTERM
    let mut pids: Vec<i32> = Vec::new();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-l" {
            // List signals
            let signal_arg = args.get(i + 1).map(|s| s.as_str());
            list_signals(signal_arg);
            process::exit(0);
        } else if arg == "-s" {
            // -s signal
            if i + 1 >= args.len() {
                eprintln!("kill: option requires an argument -- 's'");
                process::exit(1);
            }
            i += 1;
            match parse_signal(&args[i]) {
                Ok(sig) => signal = sig,
                Err(e) => {
                    eprintln!("kill: {}", e);
                    process::exit(1);
                }
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // -signal (e.g., -9, -KILL, -SIGKILL)
            let sig_str = &arg[1..];
            match parse_signal(sig_str) {
                Ok(sig) => signal = sig,
                Err(e) => {
                    eprintln!("kill: {}", e);
                    process::exit(1);
                }
            }
        } else {
            // Should be a PID
            match arg.parse::<i32>() {
                Ok(pid) => pids.push(pid),
                Err(_) => {
                    eprintln!("kill: invalid pid: {}", arg);
                    process::exit(1);
                }
            }
        }
        
        i += 1;
    }

    if pids.is_empty() {
        eprintln!("kill: no process ID specified");
        print_usage();
        process::exit(1);
    }

    let mut had_error = false;

    for pid in pids {
        match kill_process(pid, signal) {
            Ok(()) => {
                // Success, optionally print message
            }
            Err(e) => {
                eprintln!("kill: ({}) - {}", pid, e);
                had_error = true;
            }
        }
    }

    if had_error {
        process::exit(1);
    }
}
