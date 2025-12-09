//! pwd - Print working directory
//!
//! Usage:
//!   pwd

use std::env;
use std::process;

fn print_usage() {
    println!("pwd - Print working directory");
    println!();
    println!("Usage: pwd [OPTIONS]");
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

    match env::current_dir() {
        Ok(path) => {
            println!("{}", path.display());
        }
        Err(e) => {
            eprintln!("pwd: {}", e);
            process::exit(1);
        }
    }
}
