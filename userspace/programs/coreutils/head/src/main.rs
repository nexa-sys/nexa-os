//! head - Output the first part of files
//!
//! Usage:
//!   head [OPTIONS] <file>...

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

const DEFAULT_LINES: usize = 10;

fn print_usage() {
    println!("head - Output the first part of files");
    println!();
    println!("Usage: head [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -n NUM   Output the first NUM lines (default: 10)");
    println!("  -c NUM   Output the first NUM bytes");
    println!("  -q       Never print headers for each file");
    println!("  -v       Always print headers for each file");
    println!("  -h       Show this help message");
}

fn head_lines(path: &str, num_lines: usize) -> io::Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut stdout = io::stdout();
    let mut count = 0;

    for line in reader.lines() {
        if count >= num_lines {
            break;
        }
        let line = line?;
        writeln!(stdout, "{}", line)?;
        count += 1;
    }

    stdout.flush()?;
    Ok(())
}

fn head_bytes(path: &str, num_bytes: usize) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut stdout = io::stdout();
    let mut buffer = vec![0u8; num_bytes.min(4096)];
    let mut remaining = num_bytes;

    while remaining > 0 {
        let to_read = remaining.min(buffer.len());
        let mut slice = &mut buffer[..to_read];
        match io::Read::read(&mut reader, &mut slice) {
            Ok(0) => break, // EOF
            Ok(n) => {
                stdout.write_all(&buffer[..n])?;
                remaining -= n;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }

    stdout.flush()?;
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut num_lines: Option<usize> = None;
    let mut num_bytes: Option<usize> = None;
    let mut quiet = false;
    let mut verbose = false;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];

        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-q" || arg == "--quiet" || arg == "--silent" {
            quiet = true;
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg == "-n" {
            i += 1;
            if i >= args.len() {
                eprintln!("head: option requires an argument -- 'n'");
                process::exit(1);
            }
            match args[i].parse::<usize>() {
                Ok(n) => num_lines = Some(n),
                Err(_) => {
                    eprintln!("head: invalid number of lines: '{}'", args[i]);
                    process::exit(1);
                }
            }
        } else if arg.starts_with("-n") {
            let num_str = &arg[2..];
            match num_str.parse::<usize>() {
                Ok(n) => num_lines = Some(n),
                Err(_) => {
                    eprintln!("head: invalid number of lines: '{}'", num_str);
                    process::exit(1);
                }
            }
        } else if arg == "-c" {
            i += 1;
            if i >= args.len() {
                eprintln!("head: option requires an argument -- 'c'");
                process::exit(1);
            }
            match args[i].parse::<usize>() {
                Ok(n) => num_bytes = Some(n),
                Err(_) => {
                    eprintln!("head: invalid number of bytes: '{}'", args[i]);
                    process::exit(1);
                }
            }
        } else if arg.starts_with("-c") {
            let num_str = &arg[2..];
            match num_str.parse::<usize>() {
                Ok(n) => num_bytes = Some(n),
                Err(_) => {
                    eprintln!("head: invalid number of bytes: '{}'", num_str);
                    process::exit(1);
                }
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Try to parse as -NUM (shorthand for -n NUM)
            if let Ok(n) = arg[1..].parse::<usize>() {
                num_lines = Some(n);
            } else {
                eprintln!("head: unknown option: {}", arg);
                process::exit(1);
            }
        } else {
            files.push(arg);
        }

        i += 1;
    }

    if files.is_empty() {
        eprintln!("head: missing file operand");
        process::exit(1);
    }

    let show_headers = if quiet {
        false
    } else if verbose {
        true
    } else {
        files.len() > 1
    };

    let mut exit_code = 0;
    let mut first = true;

    for file in &files {
        if show_headers {
            if !first {
                println!();
            }
            println!("==> {} <==", file);
        }
        first = false;

        let result = if let Some(bytes) = num_bytes {
            head_bytes(file, bytes)
        } else {
            head_lines(file, num_lines.unwrap_or(DEFAULT_LINES))
        };

        if let Err(e) = result {
            eprintln!("head: cannot open '{}' for reading: {}", file, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
