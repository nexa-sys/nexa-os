//! wc - Word, line, character, and byte count
//!
//! Usage:
//!   wc [OPTIONS] <file>...

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::process;

fn print_usage() {
    println!("wc - Word, line, character, and byte count");
    println!();
    println!("Usage: wc [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -l        Print line count");
    println!("  -w        Print word count");
    println!("  -c        Print byte count");
    println!("  -m        Print character count");
    println!("  -L        Print maximum line length");
    println!("  -h        Show this help message");
    println!();
    println!("If no options specified, prints lines, words, and bytes.");
}

#[derive(Default)]
struct Counts {
    lines: usize,
    words: usize,
    bytes: usize,
    chars: usize,
    max_line_length: usize,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.lines += other.lines;
        self.words += other.words;
        self.bytes += other.bytes;
        self.chars += other.chars;
        if other.max_line_length > self.max_line_length {
            self.max_line_length = other.max_line_length;
        }
    }
}

fn count_file(path: &str) -> io::Result<Counts> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut counts = Counts::default();
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(bytes_read) => {
                counts.bytes += bytes_read;
                counts.chars += line.chars().count();

                // Count line (if ends with newline or is the last line)
                if line.ends_with('\n') {
                    counts.lines += 1;
                    // Calculate line length without newline
                    let line_len = line.trim_end_matches('\n').len();
                    if line_len > counts.max_line_length {
                        counts.max_line_length = line_len;
                    }
                } else {
                    // Last line without newline
                    if !line.is_empty() {
                        counts.lines += 1;
                        if line.len() > counts.max_line_length {
                            counts.max_line_length = line.len();
                        }
                    }
                }

                // Count words (sequences of non-whitespace)
                counts.words += line.split_whitespace().count();
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }

    Ok(counts)
}

fn print_counts(
    counts: &Counts,
    name: &str,
    show_lines: bool,
    show_words: bool,
    show_bytes: bool,
    show_chars: bool,
    show_max_length: bool,
) {
    let mut parts: Vec<String> = Vec::new();

    if show_lines {
        parts.push(format!("{:>7}", counts.lines));
    }
    if show_words {
        parts.push(format!("{:>7}", counts.words));
    }
    if show_chars {
        parts.push(format!("{:>7}", counts.chars));
    }
    if show_bytes {
        parts.push(format!("{:>7}", counts.bytes));
    }
    if show_max_length {
        parts.push(format!("{:>7}", counts.max_line_length));
    }

    if name.is_empty() {
        println!("{}", parts.join(""));
    } else {
        println!("{} {}", parts.join(""), name);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut show_lines = false;
    let mut show_words = false;
    let mut show_bytes = false;
    let mut show_chars = false;
    let mut show_max_length = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'l' => show_lines = true,
                    'w' => show_words = true,
                    'c' => show_bytes = true,
                    'm' => show_chars = true,
                    'L' => show_max_length = true,
                    '-' => {
                        // Handle long options
                        match arg.as_str() {
                            "--lines" => show_lines = true,
                            "--words" => show_words = true,
                            "--bytes" => show_bytes = true,
                            "--chars" => show_chars = true,
                            "--max-line-length" => show_max_length = true,
                            _ => {
                                eprintln!("wc: unknown option: {}", arg);
                                process::exit(1);
                            }
                        }
                        break;
                    }
                    _ => {
                        eprintln!("wc: unknown option: -{}", c);
                        process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg);
        }
    }

    // Default: show lines, words, bytes
    if !show_lines && !show_words && !show_bytes && !show_chars && !show_max_length {
        show_lines = true;
        show_words = true;
        show_bytes = true;
    }

    if files.is_empty() {
        eprintln!("wc: missing file operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    let mut total = Counts::default();
    let show_total = files.len() > 1;

    for file in &files {
        match count_file(file) {
            Ok(counts) => {
                print_counts(
                    &counts,
                    file,
                    show_lines,
                    show_words,
                    show_bytes,
                    show_chars,
                    show_max_length,
                );
                total.add(&counts);
            }
            Err(e) => {
                eprintln!("wc: {}: {}", file, e);
                exit_code = 1;
            }
        }
    }

    if show_total {
        print_counts(
            &total,
            "total",
            show_lines,
            show_words,
            show_bytes,
            show_chars,
            show_max_length,
        );
    }

    process::exit(exit_code);
}
