//! rmdir - Remove empty directories
//!
//! Usage:
//!   rmdir [OPTIONS] <directory>...

use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn print_usage() {
    println!("rmdir - Remove empty directories");
    println!();
    println!("Usage: rmdir [OPTIONS] <directory>...");
    println!();
    println!("Options:");
    println!("  -p    Remove directory and its ancestors");
    println!("  -v    Verbose mode (print each directory removed)");
    println!("  -h    Show this help message");
    println!();
    println!("Note: Only empty directories can be removed.");
}

fn remove_dir_verbose(path: &str, verbose: bool) -> Result<(), String> {
    match fs::remove_dir(path) {
        Ok(()) => {
            if verbose {
                println!("rmdir: removing directory, '{}'", path);
            }
            Ok(())
        }
        Err(e) => Err(format!("{}", e)),
    }
}

fn remove_with_parents(path: &str, verbose: bool) -> Result<(), String> {
    // First remove the target directory
    remove_dir_verbose(path, verbose)?;
    
    // Then try to remove parent directories
    let mut current = Path::new(path).parent();
    while let Some(parent) = current {
        let parent_str = parent.to_string_lossy();
        if parent_str.is_empty() || parent_str == "/" || parent_str == "." {
            break;
        }
        
        // Try to remove parent, but don't fail if it's not empty
        match fs::remove_dir(parent) {
            Ok(()) => {
                if verbose {
                    println!("rmdir: removing directory, '{}'", parent_str);
                }
            }
            Err(_) => {
                // Parent not empty or permission denied, stop here
                break;
            }
        }
        current = parent.parent();
    }
    
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut parents = false;
    let mut verbose = false;
    let mut dirs: Vec<&str> = Vec::new();
    
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-p" || arg == "--parents" {
            parents = true;
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg.starts_with('-') {
            // Handle combined options like -pv
            for c in arg[1..].chars() {
                match c {
                    'p' => parents = true,
                    'v' => verbose = true,
                    _ => {
                        eprintln!("rmdir: unknown option: -{}", c);
                        process::exit(1);
                    }
                }
            }
        } else {
            dirs.push(arg);
        }
    }

    if dirs.is_empty() {
        eprintln!("rmdir: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    
    for dir in dirs {
        let result = if parents {
            remove_with_parents(dir, verbose)
        } else {
            remove_dir_verbose(dir, verbose)
        };
        
        if let Err(e) = result {
            eprintln!("rmdir: failed to remove '{}': {}", dir, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
