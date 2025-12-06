//! uname - Print system information
//!
//! Usage:
//!   uname [-a]

use std::env;
use std::process;

fn print_usage() {
    println!("uname - Print system information");
    println!();
    println!("Usage: uname [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -a    Print all information");
    println!("  -s    Print kernel name (default)");
    println!("  -r    Print kernel release");
    println!("  -v    Print kernel version");
    println!("  -m    Print machine architecture");
    println!("  -h    Show this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut print_all = false;
    let mut print_sysname = false;
    let mut print_release = false;
    let mut print_version = false;
    let mut print_machine = false;
    
    if args.len() == 1 {
        // Default: print kernel name
        print_sysname = true;
    }
    
    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg[1..].chars() {
                match c {
                    'a' => print_all = true,
                    's' => print_sysname = true,
                    'r' => print_release = true,
                    'v' => print_version = true,
                    'm' => print_machine = true,
                    'h' => {
                        print_usage();
                        process::exit(0);
                    }
                    '-' => {
                        if arg == "--help" {
                            print_usage();
                            process::exit(0);
                        }
                    }
                    _ => {
                        eprintln!("uname: unknown option -{}", c);
                        process::exit(1);
                    }
                }
            }
        }
    }

    if print_all {
        println!("NexaOS nexa 0.1.0 #1 x86_64 (experimental hybrid kernel)");
        return;
    }

    let mut parts: Vec<&str> = Vec::new();
    
    if print_sysname {
        parts.push("NexaOS");
    }
    if print_release {
        parts.push("0.1.0");
    }
    if print_version {
        parts.push("#1");
    }
    if print_machine {
        parts.push("x86_64");
    }

    if parts.is_empty() {
        parts.push("NexaOS");
    }

    println!("{}", parts.join(" "));
}
