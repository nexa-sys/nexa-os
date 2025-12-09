//! mkdir - Make directories
//!
//! Usage:
//!   mkdir [-p] <directory>...

use std::env;
use std::fs;
use std::process;

fn print_usage() {
    println!("mkdir - Make directories");
    println!();
    println!("Usage: mkdir [OPTIONS] <directory>...");
    println!();
    println!("Options:");
    println!("  -p    Create parent directories as needed");
    println!("  -h    Show this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut parents = false;
    let mut dirs: Vec<&str> = Vec::new();
    
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-p" {
            parents = true;
        } else if arg.starts_with('-') {
            eprintln!("mkdir: unknown option {}", arg);
            process::exit(1);
        } else {
            dirs.push(arg);
        }
    }

    if dirs.is_empty() {
        eprintln!("mkdir: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    
    for dir in dirs {
        let result = if parents {
            fs::create_dir_all(dir)
        } else {
            fs::create_dir(dir)
        };
        
        if let Err(e) = result {
            eprintln!("mkdir: cannot create directory '{}': {}", dir, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
