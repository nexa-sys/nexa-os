//! echo - Display a line of text
//!
//! Usage:
//!   echo [text...]

use std::env;
use std::io::{self, Write};
use std::process;

fn print_usage() {
    println!("echo - Display a line of text");
    println!();
    println!("Usage: echo [OPTIONS] [text...]");
    println!();
    println!("Options:");
    println!("  -n    Do not output trailing newline");
    println!("  -e    Enable interpretation of backslash escapes");
    println!("  --help Show this help message");
}

fn process_escapes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('0') => result.push('\0'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut newline = true;
    let mut interpret_escapes = false;
    let mut text_args: Vec<&str> = Vec::new();

    for arg in args.iter().skip(1) {
        if arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg.starts_with('-') && text_args.is_empty() {
            for c in arg[1..].chars() {
                match c {
                    'n' => newline = false,
                    'e' => interpret_escapes = true,
                    _ => {
                        // Unknown option, treat as text
                        text_args.push(arg);
                        break;
                    }
                }
            }
        } else {
            text_args.push(arg);
        }
    }

    let text = text_args.join(" ");
    let output = if interpret_escapes {
        process_escapes(&text)
    } else {
        text
    };

    let stdout = io::stdout();
    let mut handle = stdout.lock();

    let _ = handle.write_all(output.as_bytes());
    if newline {
        let _ = handle.write_all(b"\n");
    }
    let _ = handle.flush();
}
