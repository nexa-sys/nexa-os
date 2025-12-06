//! cat - Concatenate and print files
//!
//! Usage:
//!   cat <file>...

use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::process;

fn print_usage() {
    println!("cat - Concatenate and print files");
    println!();
    println!("Usage: cat [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -h    Show this help message");
}

fn cat_file(path: &str) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; 4096];
    let mut stdout = io::stdout();
    let mut last_byte_was_newline = true;
    
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                stdout.write_all(&buffer[..n])?;
                stdout.flush()?;
                last_byte_was_newline = buffer[n - 1] == b'\n';
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    
    if !last_byte_was_newline {
        println!();
    }
    
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut files: Vec<&str> = Vec::new();
    
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }
        files.push(arg);
    }

    if files.is_empty() {
        eprintln!("cat: missing file name");
        process::exit(1);
    }

    let mut exit_code = 0;
    
    for file in files {
        if let Err(e) = cat_file(file) {
            eprintln!("cat: {}: {}", file, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
