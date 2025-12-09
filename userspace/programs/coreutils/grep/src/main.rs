//! grep - Search for patterns in files
//!
//! Usage:
//!   grep [OPTIONS] PATTERN [FILE]...
//!
//! Options:
//!   -i    Ignore case distinctions
//!   -v    Invert match (select non-matching lines)
//!   -n    Print line numbers
//!   -c    Count matching lines only
//!   -r    Recursively search directories
//!   -l    Print only filenames with matches
//!   -h    Suppress filename prefix on output

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process;

struct GrepOptions {
    ignore_case: bool,
    invert_match: bool,
    show_line_numbers: bool,
    count_only: bool,
    recursive: bool,
    files_only: bool,
    suppress_filename: bool,
    pattern: String,
    files: Vec<String>,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            ignore_case: false,
            invert_match: false,
            show_line_numbers: false,
            count_only: false,
            recursive: false,
            files_only: false,
            suppress_filename: false,
            pattern: String::new(),
            files: Vec::new(),
        }
    }
}

fn print_usage() {
    println!("grep - Search for patterns in files");
    println!();
    println!("Usage: grep [OPTIONS] PATTERN [FILE]...");
    println!();
    println!("Options:");
    println!("  -i    Ignore case distinctions");
    println!("  -v    Invert match (select non-matching lines)");
    println!("  -n    Print line numbers");
    println!("  -c    Count matching lines only");
    println!("  -r    Recursively search directories");
    println!("  -l    Print only filenames with matches");
    println!("  -h    Suppress filename prefix on output");
    println!("  --help  Show this help message");
    println!();
    println!("If no FILE is given, read from standard input.");
}

fn line_matches(line: &str, pattern: &str, ignore_case: bool) -> bool {
    if ignore_case {
        line.to_lowercase().contains(&pattern.to_lowercase())
    } else {
        line.contains(pattern)
    }
}

fn grep_file(path: &Path, opts: &GrepOptions, show_filename: bool) -> io::Result<(u64, bool)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut match_count: u64 = 0;
    let mut found_match = false;
    let mut stdout = io::stdout();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue, // Skip lines that can't be read as UTF-8
        };

        let matches = line_matches(&line, &opts.pattern, opts.ignore_case);
        let should_print = if opts.invert_match { !matches } else { matches };

        if should_print {
            match_count += 1;
            found_match = true;

            if opts.files_only {
                // Just return true, we'll print the filename once
                return Ok((match_count, true));
            }

            if !opts.count_only {
                let mut output = String::new();

                if show_filename && !opts.suppress_filename {
                    output.push_str(&format!("{}:", path.display()));
                }

                if opts.show_line_numbers {
                    output.push_str(&format!("{}:", line_num + 1));
                }

                output.push_str(&line);
                writeln!(stdout, "{}", output)?;
            }
        }
    }

    Ok((match_count, found_match))
}

fn grep_stdin(opts: &GrepOptions) -> io::Result<u64> {
    let stdin = io::stdin();
    let reader = stdin.lock();
    let mut match_count: u64 = 0;
    let mut stdout = io::stdout();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };

        let matches = line_matches(&line, &opts.pattern, opts.ignore_case);
        let should_print = if opts.invert_match { !matches } else { matches };

        if should_print {
            match_count += 1;

            if !opts.count_only {
                if opts.show_line_numbers {
                    write!(stdout, "{}:", line_num + 1)?;
                }
                writeln!(stdout, "{}", line)?;
            }
        }
    }

    Ok(match_count)
}

fn grep_directory(path: &Path, opts: &GrepOptions, show_filename: bool) -> io::Result<(u64, bool)> {
    let mut total_count: u64 = 0;
    let mut any_match = false;

    let entries = fs::read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            if opts.recursive {
                let (count, found) = grep_directory(&entry_path, opts, show_filename)?;
                total_count += count;
                any_match |= found;
            }
        } else if entry_path.is_file() {
            match grep_file(&entry_path, opts, show_filename) {
                Ok((count, found)) => {
                    if opts.files_only && found {
                        println!("{}", entry_path.display());
                    } else if opts.count_only && count > 0 {
                        if show_filename && !opts.suppress_filename {
                            println!("{}:{}", entry_path.display(), count);
                        }
                    }
                    total_count += count;
                    any_match |= found;
                }
                Err(_) => {
                    // Skip files we can't read
                    continue;
                }
            }
        }
    }

    Ok((total_count, any_match))
}

fn parse_args() -> Result<GrepOptions, String> {
    let args: Vec<String> = env::args().collect();
    let mut opts = GrepOptions::default();
    let mut positional: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for c in arg[1..].chars() {
                match c {
                    'i' => opts.ignore_case = true,
                    'v' => opts.invert_match = true,
                    'n' => opts.show_line_numbers = true,
                    'c' => opts.count_only = true,
                    'r' | 'R' => opts.recursive = true,
                    'l' => opts.files_only = true,
                    'h' => opts.suppress_filename = true,
                    _ => return Err(format!("Unknown option: -{}", c)),
                }
            }
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("Missing PATTERN".to_string());
    }

    opts.pattern = positional.remove(0);
    opts.files = positional;

    Ok(opts)
}

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("grep: {}", e);
            eprintln!("Try 'grep --help' for more information.");
            process::exit(2);
        }
    };

    let mut exit_code = 1; // No matches found
    let show_filename = opts.files.len() > 1 || opts.recursive;

    if opts.files.is_empty() {
        // Read from stdin
        match grep_stdin(&opts) {
            Ok(count) => {
                if opts.count_only {
                    println!("{}", count);
                }
                if count > 0 {
                    exit_code = 0;
                }
            }
            Err(e) => {
                eprintln!("grep: stdin: {}", e);
                process::exit(2);
            }
        }
    } else {
        let mut total_matches: u64 = 0;

        for file_path in &opts.files {
            let path = Path::new(file_path);

            if path.is_dir() {
                if opts.recursive {
                    match grep_directory(path, &opts, show_filename) {
                        Ok((count, _)) => {
                            total_matches += count;
                        }
                        Err(e) => {
                            eprintln!("grep: {}: {}", file_path, e);
                        }
                    }
                } else {
                    eprintln!("grep: {}: Is a directory", file_path);
                }
            } else {
                match grep_file(path, &opts, show_filename) {
                    Ok((count, found)) => {
                        if opts.files_only && found {
                            println!("{}", path.display());
                        } else if opts.count_only {
                            if show_filename && !opts.suppress_filename {
                                println!("{}:{}", path.display(), count);
                            } else {
                                println!("{}", count);
                            }
                        }
                        total_matches += count;
                    }
                    Err(e) => {
                        eprintln!("grep: {}: {}", file_path, e);
                    }
                }
            }
        }

        if total_matches > 0 {
            exit_code = 0;
        }
    }

    process::exit(exit_code);
}
