//! tail - Output the last part of files
//!
//! Usage:
//!   tail [OPTIONS] <file>...

use std::collections::VecDeque;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::process;

const DEFAULT_LINES: usize = 10;

fn print_usage() {
    println!("tail - Output the last part of files");
    println!();
    println!("Usage: tail [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -n NUM   Output the last NUM lines (default: 10)");
    println!("  -c NUM   Output the last NUM bytes");
    println!("  -q       Never print headers for each file");
    println!("  -v       Always print headers for each file");
    println!("  -h       Show this help message");
}

fn tail_lines(path: &str, num_lines: usize) -> io::Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut stdout = io::stdout();

    // Use a ring buffer to keep track of the last N lines
    let mut line_buffer: VecDeque<String> = VecDeque::with_capacity(num_lines);

    for line in reader.lines() {
        let line = line?;
        if line_buffer.len() >= num_lines {
            line_buffer.pop_front();
        }
        line_buffer.push_back(line);
    }

    for line in line_buffer {
        writeln!(stdout, "{}", line)?;
    }

    stdout.flush()?;
    Ok(())
}

fn tail_bytes(path: &str, num_bytes: usize) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut stdout = io::stdout();

    // Get file size
    let file_size = file.seek(SeekFrom::End(0))?;

    // Calculate starting position
    let start_pos = if file_size > num_bytes as u64 {
        file_size - num_bytes as u64
    } else {
        0
    };

    // Seek to start position and read
    file.seek(SeekFrom::Start(start_pos))?;

    let mut buffer = [0u8; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                stdout.write_all(&buffer[..n])?;
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
                eprintln!("tail: option requires an argument -- 'n'");
                process::exit(1);
            }
            match args[i].parse::<usize>() {
                Ok(n) => num_lines = Some(n),
                Err(_) => {
                    eprintln!("tail: invalid number of lines: '{}'", args[i]);
                    process::exit(1);
                }
            }
        } else if arg.starts_with("-n") {
            let num_str = &arg[2..];
            match num_str.parse::<usize>() {
                Ok(n) => num_lines = Some(n),
                Err(_) => {
                    eprintln!("tail: invalid number of lines: '{}'", num_str);
                    process::exit(1);
                }
            }
        } else if arg == "-c" {
            i += 1;
            if i >= args.len() {
                eprintln!("tail: option requires an argument -- 'c'");
                process::exit(1);
            }
            match args[i].parse::<usize>() {
                Ok(n) => num_bytes = Some(n),
                Err(_) => {
                    eprintln!("tail: invalid number of bytes: '{}'", args[i]);
                    process::exit(1);
                }
            }
        } else if arg.starts_with("-c") {
            let num_str = &arg[2..];
            match num_str.parse::<usize>() {
                Ok(n) => num_bytes = Some(n),
                Err(_) => {
                    eprintln!("tail: invalid number of bytes: '{}'", num_str);
                    process::exit(1);
                }
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Try to parse as -NUM (shorthand for -n NUM)
            if let Ok(n) = arg[1..].parse::<usize>() {
                num_lines = Some(n);
            } else {
                eprintln!("tail: unknown option: {}", arg);
                process::exit(1);
            }
        } else {
            files.push(arg);
        }

        i += 1;
    }

    if files.is_empty() {
        eprintln!("tail: missing file operand");
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
            tail_bytes(file, bytes)
        } else {
            tail_lines(file, num_lines.unwrap_or(DEFAULT_LINES))
        };

        if let Err(e) = result {
            eprintln!("tail: cannot open '{}' for reading: {}", file, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
