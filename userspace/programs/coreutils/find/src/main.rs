//! find - Search for files in a directory hierarchy
//!
//! Usage:
//!   find [PATH]... [EXPRESSION]
//!
//! Options:
//!   -name PATTERN     Match filename (supports * and ? wildcards)
//!   -type TYPE        Match file type (f=file, d=directory, l=symlink)
//!   -maxdepth N       Descend at most N directory levels
//!   -mindepth N       Ignore levels less than N
//!   -size [+-]N[ckMG] Match file size
//!   -empty            Match empty files or directories
//!   -print            Print the full file name (default)

use std::env;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Clone)]
enum FileType {
    File,
    Directory,
    Symlink,
    Any,
}

#[derive(Clone)]
enum SizeComparison {
    Exact(u64),
    GreaterThan(u64),
    LessThan(u64),
}

#[derive(Clone)]
struct FindOptions {
    paths: Vec<PathBuf>,
    name_pattern: Option<String>,
    file_type: FileType,
    max_depth: Option<usize>,
    min_depth: Option<usize>,
    size: Option<SizeComparison>,
    empty: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            name_pattern: None,
            file_type: FileType::Any,
            max_depth: None,
            min_depth: None,
            size: None,
            empty: false,
        }
    }
}

fn print_usage() {
    println!("find - Search for files in a directory hierarchy");
    println!();
    println!("Usage: find [PATH]... [EXPRESSION]");
    println!();
    println!("Expressions:");
    println!("  -name PATTERN     Match filename (supports * and ? wildcards)");
    println!("  -type TYPE        Match file type:");
    println!("                      f = regular file");
    println!("                      d = directory");
    println!("                      l = symbolic link");
    println!("  -maxdepth N       Descend at most N directory levels");
    println!("  -mindepth N       Ignore levels less than N");
    println!("  -size [+-]N[ckMG] Match file size:");
    println!("                      c = bytes");
    println!("                      k = kilobytes (default)");
    println!("                      M = megabytes");
    println!("                      G = gigabytes");
    println!("                      + = greater than, - = less than");
    println!("  -empty            Match empty files or directories");
    println!("  -print            Print the full file name (default)");
    println!("  --help            Show this help message");
    println!();
    println!("Examples:");
    println!("  find .                       List all files recursively");
    println!("  find /etc -name '*.conf'     Find config files");
    println!("  find . -type d               Find directories only");
    println!("  find . -size +1M             Find files larger than 1MB");
}

/// Simple wildcard pattern matching supporting * and ?
fn pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let name_chars: Vec<char> = name.chars().collect();

    fn match_helper(pattern: &[char], name: &[char]) -> bool {
        if pattern.is_empty() {
            return name.is_empty();
        }

        match pattern[0] {
            '*' => {
                // Try matching * with 0 characters, then 1, then 2, etc.
                for i in 0..=name.len() {
                    if match_helper(&pattern[1..], &name[i..]) {
                        return true;
                    }
                }
                false
            }
            '?' => {
                if name.is_empty() {
                    false
                } else {
                    match_helper(&pattern[1..], &name[1..])
                }
            }
            c => {
                if name.is_empty() || name[0] != c {
                    false
                } else {
                    match_helper(&pattern[1..], &name[1..])
                }
            }
        }
    }

    match_helper(&pattern_chars, &name_chars)
}

fn parse_size(s: &str) -> Result<SizeComparison, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Empty size specification".to_string());
    }

    let (comparison_fn, rest): (fn(u64) -> SizeComparison, &str) = if s.starts_with('+') {
        (SizeComparison::GreaterThan, &s[1..])
    } else if s.starts_with('-') {
        (SizeComparison::LessThan, &s[1..])
    } else {
        (SizeComparison::Exact, s)
    };

    // Extract numeric part and suffix
    let mut num_end = 0;
    for (i, c) in rest.char_indices() {
        if c.is_ascii_digit() {
            num_end = i + 1;
        } else {
            break;
        }
    }

    if num_end == 0 {
        return Err("Invalid size: no number found".to_string());
    }

    let num: u64 = rest[..num_end].parse().map_err(|_| "Invalid number")?;
    let suffix = &rest[num_end..];

    let multiplier: u64 = match suffix {
        "" | "k" => 1024,
        "c" => 1,
        "M" => 1024 * 1024,
        "G" => 1024 * 1024 * 1024,
        _ => return Err(format!("Unknown size suffix: {}", suffix)),
    };

    Ok(comparison_fn(num * multiplier))
}

fn parse_args() -> Result<FindOptions, String> {
    let args: Vec<String> = env::args().collect();
    let mut opts = FindOptions::default();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-name" {
            i += 1;
            if i >= args.len() {
                return Err("-name requires a pattern argument".to_string());
            }
            opts.name_pattern = Some(args[i].clone());
        } else if arg == "-type" {
            i += 1;
            if i >= args.len() {
                return Err("-type requires an argument".to_string());
            }
            opts.file_type = match args[i].as_str() {
                "f" => FileType::File,
                "d" => FileType::Directory,
                "l" => FileType::Symlink,
                t => return Err(format!("Unknown file type: {}", t)),
            };
        } else if arg == "-maxdepth" {
            i += 1;
            if i >= args.len() {
                return Err("-maxdepth requires a number".to_string());
            }
            opts.max_depth = Some(args[i].parse().map_err(|_| "Invalid maxdepth value")?);
        } else if arg == "-mindepth" {
            i += 1;
            if i >= args.len() {
                return Err("-mindepth requires a number".to_string());
            }
            opts.min_depth = Some(args[i].parse().map_err(|_| "Invalid mindepth value")?);
        } else if arg == "-size" {
            i += 1;
            if i >= args.len() {
                return Err("-size requires an argument".to_string());
            }
            opts.size = Some(parse_size(&args[i])?);
        } else if arg == "-empty" {
            opts.empty = true;
        } else if arg == "-print" {
            // -print is default, just ignore
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {}", arg));
        } else {
            // It's a path
            opts.paths.push(PathBuf::from(arg));
        }

        i += 1;
    }

    // Default to current directory if no paths specified
    if opts.paths.is_empty() {
        opts.paths.push(PathBuf::from("."));
    }

    Ok(opts)
}

fn check_type(metadata: &Metadata, file_type: &FileType) -> bool {
    match file_type {
        FileType::Any => true,
        FileType::File => metadata.is_file(),
        FileType::Directory => metadata.is_dir(),
        FileType::Symlink => metadata.file_type().is_symlink(),
    }
}

fn check_size(metadata: &Metadata, size: &SizeComparison) -> bool {
    let file_size = metadata.size();
    match size {
        SizeComparison::Exact(s) => file_size == *s,
        SizeComparison::GreaterThan(s) => file_size > *s,
        SizeComparison::LessThan(s) => file_size < *s,
    }
}

fn check_empty(path: &Path, metadata: &Metadata) -> bool {
    if metadata.is_file() {
        metadata.size() == 0
    } else if metadata.is_dir() {
        match fs::read_dir(path) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => false,
        }
    } else {
        false
    }
}

fn matches_criteria(path: &Path, metadata: &Metadata, opts: &FindOptions) -> bool {
    // Check name pattern
    if let Some(ref pattern) = opts.name_pattern {
        if let Some(name) = path.file_name() {
            if !pattern_matches(pattern, &name.to_string_lossy()) {
                return false;
            }
        } else {
            return false;
        }
    }

    // Check file type
    if !check_type(metadata, &opts.file_type) {
        return false;
    }

    // Check size
    if let Some(ref size) = opts.size {
        if !check_size(metadata, size) {
            return false;
        }
    }

    // Check empty
    if opts.empty && !check_empty(path, metadata) {
        return false;
    }

    true
}

fn find_recursive(path: &Path, depth: usize, opts: &FindOptions) {
    // Check max depth
    if let Some(max) = opts.max_depth {
        if depth > max {
            return;
        }
    }

    // Get metadata
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return,
    };

    // Check if we should print this entry
    let should_print = match opts.min_depth {
        Some(min) => depth >= min,
        None => true,
    };

    if should_print && matches_criteria(path, &metadata, opts) {
        println!("{}", path.display());
    }

    // Recurse into directories
    if metadata.is_dir() {
        if let Some(max) = opts.max_depth {
            if depth >= max {
                return;
            }
        }

        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            find_recursive(&entry.path(), depth + 1, opts);
        }
    }
}

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("find: {}", e);
            eprintln!("Try 'find --help' for more information.");
            process::exit(1);
        }
    };

    for path in &opts.paths {
        if !path.exists() {
            eprintln!("find: '{}': No such file or directory", path.display());
            continue;
        }

        find_recursive(path, 0, &opts);
    }
}
