//! tee - Read from stdin and write to stdout and files
//!
//! Usage:
//!   tee [OPTIONS] <file>...

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process;

fn print_usage() {
    println!("tee - Read from stdin and write to stdout and files");
    println!();
    println!("Usage: tee [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -a, --append    Append to files instead of overwriting");
    println!("  -i, --ignore-interrupts  Ignore interrupt signals");
    println!("  -h, --help      Show this help message");
    println!();
    println!("Copy standard input to each FILE, and also to standard output.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut append = false;
    let mut files: Vec<&str> = Vec::new();
    
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-a" | "--append" => append = true,
            "-i" | "--ignore-interrupts" => {
                // Signal handling not fully implemented
            }
            _ if arg.starts_with('-') => {
                // Handle combined options
                for c in arg[1..].chars() {
                    match c {
                        'a' => append = true,
                        'i' => {}
                        _ => {
                            eprintln!("tee: unknown option: -{}", c);
                            process::exit(1);
                        }
                    }
                }
            }
            _ => {
                files.push(arg);
            }
        }
    }

    // Open output files
    let mut output_files: Vec<File> = Vec::new();
    let mut exit_code = 0;
    
    for file_path in &files {
        let result = if append {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_path)
        } else {
            File::create(file_path)
        };
        
        match result {
            Ok(file) => output_files.push(file),
            Err(e) => {
                eprintln!("tee: {}: {}", file_path, e);
                exit_code = 1;
            }
        }
    }

    // Read from stdin and write to all outputs
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut buffer = [0u8; 4096];
    
    loop {
        match stdin.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                // Write to stdout
                if let Err(e) = stdout.write_all(&buffer[..n]) {
                    eprintln!("tee: stdout: {}", e);
                    exit_code = 1;
                }
                
                // Write to all files
                for (file, path) in output_files.iter_mut().zip(files.iter()) {
                    if let Err(e) = file.write_all(&buffer[..n]) {
                        eprintln!("tee: {}: {}", path, e);
                        exit_code = 1;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("tee: stdin: {}", e);
                exit_code = 1;
                break;
            }
        }
    }
    
    // Flush stdout
    let _ = stdout.flush();

    process::exit(exit_code);
}
