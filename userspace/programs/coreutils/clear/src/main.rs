//! clear - Clear the terminal screen
//!
//! Usage:
//!   clear

use std::env;
use std::io::{self, Write};
use std::process;

fn print_usage() {
    println!("clear - Clear the terminal screen");
    println!();
    println!("Usage: clear [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -h    Show this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }
    }

    // ANSI escape sequence: clear screen and move cursor to home position
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(b"\x1b[2J\x1b[H");
    let _ = handle.flush();
}
