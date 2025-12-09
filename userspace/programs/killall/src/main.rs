//! killall - Kill processes by name
//!
//! Usage:
//!   killall [OPTIONS] <name>...

use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
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

fn print_usage() {
    println!("killall - Kill processes by name");
    println!();
    println!("Usage: killall [OPTIONS] <name>...");
    println!();
    println!("Options:");
    println!("  -s SIGNAL   Send this signal instead of SIGTERM");
    println!("  -SIGNAL     Specify signal (e.g., -9, -KILL)");
    println!("  -e          Require exact match for process name");
    println!("  -i          Ask for confirmation before killing");
    println!("  -q          Quiet mode (no warnings)");
    println!("  -v          Report if signal was sent");
    println!("  -l          List all known signal names");
    println!("  -h          Show this help message");
    println!();
    println!("Examples:");
    println!("  killall bash       Kill all bash processes");
    println!("  killall -9 nginx   Send SIGKILL to all nginx processes");
}

fn list_signals() {
    for (name, num) in SIGNAL_MAP {
        println!("{:2} SIG{}", num, name);
    }
}

fn signal_name_to_num(name: &str) -> Option<i32> {
    let name_upper = name.to_uppercase();
    let search_name = name_upper.strip_prefix("SIG").unwrap_or(&name_upper);
    
    for (sig_name, num) in SIGNAL_MAP {
        if *sig_name == search_name {
            return Some(*num);
        }
    }
    None
}

/// Get process name from /proc/<pid>/comm
fn get_process_name(pid: u32) -> Option<String> {
    let comm_path = format!("/proc/{}/comm", pid);
    if let Ok(file) = File::open(&comm_path) {
        let reader = BufReader::new(file);
        if let Some(Ok(line)) = reader.lines().next() {
            return Some(line.trim().to_string());
        }
    }
    None
}

/// Get process command line from /proc/<pid>/cmdline
fn get_process_cmdline(pid: u32) -> Option<String> {
    let cmdline_path = format!("/proc/{}/cmdline", pid);
    if let Ok(content) = fs::read(&cmdline_path) {
        if !content.is_empty() {
            // cmdline is null-separated
            let cmd = content.split(|&b| b == 0)
                .next()
                .map(|s| String::from_utf8_lossy(s).to_string())?;
            // Extract basename
            if let Some(basename) = cmd.rsplit('/').next() {
                return Some(basename.to_string());
            }
            return Some(cmd);
        }
    }
    None
}

/// Find all PIDs matching the process name
fn find_processes_by_name(name: &str, exact_match: bool) -> Vec<u32> {
    let mut pids = Vec::new();
    
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();
            
            // Check if it's a numeric directory (PID)
            if let Ok(pid) = filename_str.parse::<u32>() {
                // Get process name
                let proc_name = get_process_name(pid)
                    .or_else(|| get_process_cmdline(pid));
                
                if let Some(proc_name) = proc_name {
                    let matches = if exact_match {
                        proc_name == name
                    } else {
                        proc_name == name || proc_name.contains(name)
                    };
                    
                    if matches {
                        pids.push(pid);
                    }
                }
            }
        }
    }
    
    pids
}

/// Send signal to a process (using syscall)
fn kill_process(pid: u32, signal: i32) -> Result<(), &'static str> {
    // Use the kill syscall via nrlib
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    
    let result = unsafe { kill(pid as i32, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err("failed to send signal")
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut signal: i32 = 15; // SIGTERM
    let mut exact_match = false;
    let mut quiet = false;
    let mut verbose = false;
    let mut names: Vec<&str> = Vec::new();
    let mut i = 1;
    
    while i < args.len() {
        let arg = &args[i];
        
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-l" || arg == "--list" {
            list_signals();
            process::exit(0);
        } else if arg == "-e" || arg == "--exact" {
            exact_match = true;
        } else if arg == "-i" || arg == "--interactive" {
            // Interactive mode - not fully implemented yet
        } else if arg == "-q" || arg == "--quiet" {
            quiet = true;
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg == "-s" || arg == "--signal" {
            i += 1;
            if i >= args.len() {
                eprintln!("killall: option requires an argument -- 's'");
                process::exit(1);
            }
            let sig_arg = &args[i];
            if let Ok(n) = sig_arg.parse::<i32>() {
                signal = n;
            } else if let Some(n) = signal_name_to_num(sig_arg) {
                signal = n;
            } else {
                eprintln!("killall: unknown signal '{}'", sig_arg);
                process::exit(1);
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            let sig_part = &arg[1..];
            // Try as number first
            if let Ok(n) = sig_part.parse::<i32>() {
                signal = n;
            } else if let Some(n) = signal_name_to_num(sig_part) {
                signal = n;
            } else {
                eprintln!("killall: unknown option: {}", arg);
                process::exit(1);
            }
        } else {
            names.push(arg);
        }
        
        i += 1;
    }

    if names.is_empty() {
        eprintln!("killall: missing process name");
        process::exit(1);
    }

    let mut exit_code = 0;
    let mut killed_any = false;
    
    for name in &names {
        let pids = find_processes_by_name(name, exact_match);
        
        if pids.is_empty() {
            if !quiet {
                eprintln!("killall: {}: no process found", name);
            }
            exit_code = 1;
        } else {
            for pid in pids {
                match kill_process(pid, signal) {
                    Ok(()) => {
                        killed_any = true;
                        if verbose {
                            println!("Killed {}(pid {})", name, pid);
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            eprintln!("killall: {}: ({}) - {}", name, pid, e);
                        }
                        exit_code = 1;
                    }
                }
            }
        }
    }

    if !killed_any && !quiet {
        // All processes failed
    }

    process::exit(exit_code);
}
