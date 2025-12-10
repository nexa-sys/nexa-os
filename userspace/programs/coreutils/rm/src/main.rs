//! rm - Remove files or directories
//!
//! Usage:
//!   rm [OPTIONS] <file>...

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

fn print_usage() {
    println!("rm - Remove files or directories");
    println!();
    println!("Usage: rm [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -r, -R    Remove directories and their contents recursively");
    println!("  -f        Force removal, ignore nonexistent files");
    println!("  -i        Prompt before every removal");
    println!("  -v        Verbose mode, explain what is being done");
    println!("  -d        Remove empty directories");
    println!("  -h        Show this help message");
    println!();
    println!("Examples:");
    println!("  rm file.txt         Remove a single file");
    println!("  rm -rf directory/   Remove directory and all contents");
}

fn confirm(path: &str) -> bool {
    print!("rm: remove '{}'? ", path);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let response = input.trim().to_lowercase();
        response == "y" || response == "yes"
    } else {
        false
    }
}

fn remove_recursive(path: &Path, force: bool, interactive: bool, verbose: bool) -> io::Result<()> {
    if !path.exists() {
        if force {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No such file or directory",
        ));
    }

    if interactive && !confirm(&path.to_string_lossy()) {
        return Ok(());
    }

    if path.is_dir() {
        // Remove contents first
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            remove_recursive(&entry.path(), force, interactive, verbose)?;
        }
        // Then remove the directory
        fs::remove_dir(path)?;
        if verbose {
            println!("removed directory '{}'", path.display());
        }
    } else {
        fs::remove_file(path)?;
        if verbose {
            println!("removed '{}'", path.display());
        }
    }

    Ok(())
}

fn remove_file(path: &Path, force: bool, interactive: bool, verbose: bool) -> io::Result<()> {
    if !path.exists() {
        if force {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No such file or directory",
        ));
    }

    if path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Is a directory (use -r to remove)",
        ));
    }

    if interactive && !confirm(&path.to_string_lossy()) {
        return Ok(());
    }

    fs::remove_file(path)?;
    if verbose {
        println!("removed '{}'", path.display());
    }

    Ok(())
}

fn remove_empty_dir(path: &Path, force: bool, interactive: bool, verbose: bool) -> io::Result<()> {
    if !path.exists() {
        if force {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No such file or directory",
        ));
    }

    if !path.is_dir() {
        // For -d flag with a file, just remove the file
        return remove_file(path, force, interactive, verbose);
    }

    if interactive && !confirm(&path.to_string_lossy()) {
        return Ok(());
    }

    fs::remove_dir(path)?;
    if verbose {
        println!("removed directory '{}'", path.display());
    }

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut recursive = false;
    let mut force = false;
    let mut interactive = false;
    let mut verbose = false;
    let mut empty_dirs = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-r" || arg == "-R" || arg == "--recursive" {
            recursive = true;
        } else if arg == "-f" || arg == "--force" {
            force = true;
            interactive = false; // -f overrides -i
        } else if arg == "-i" || arg == "--interactive" {
            if !force {
                interactive = true;
            }
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg == "-d" || arg == "--dir" {
            empty_dirs = true;
        } else if arg == "-rf" || arg == "-fr" {
            recursive = true;
            force = true;
            interactive = false;
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Handle combined options like -rfv
            for c in arg[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    'f' => {
                        force = true;
                        interactive = false;
                    }
                    'i' => {
                        if !force {
                            interactive = true;
                        }
                    }
                    'v' => verbose = true,
                    'd' => empty_dirs = true,
                    _ => {
                        eprintln!("rm: unknown option: -{}", c);
                        process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg);
        }
    }

    if files.is_empty() {
        if !force {
            eprintln!("rm: missing operand");
            process::exit(1);
        }
        process::exit(0);
    }

    let mut exit_code = 0;

    for file in files {
        let path = Path::new(file);
        let result = if recursive {
            remove_recursive(path, force, interactive, verbose)
        } else if empty_dirs {
            remove_empty_dir(path, force, interactive, verbose)
        } else {
            remove_file(path, force, interactive, verbose)
        };

        if let Err(e) = result {
            if !force {
                eprintln!("rm: cannot remove '{}': {}", file, e);
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}
