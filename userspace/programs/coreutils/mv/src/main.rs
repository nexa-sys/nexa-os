//! mv - Move (rename) files and directories
//!
//! Usage:
//!   mv [OPTIONS] <source>... <dest>

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

fn print_usage() {
    println!("mv - Move (rename) files and directories");
    println!();
    println!("Usage: mv [OPTIONS] <source> <dest>");
    println!("       mv [OPTIONS] <source>... <directory>");
    println!();
    println!("Options:");
    println!("  -f        Force overwrite without prompting");
    println!("  -i        Prompt before overwrite");
    println!("  -v        Verbose mode");
    println!("  -n        Do not overwrite existing files");
    println!("  -h        Show this help message");
    println!();
    println!("Examples:");
    println!("  mv file1.txt file2.txt       Rename file1.txt to file2.txt");
    println!("  mv dir1/ dir2/               Rename directory");
    println!("  mv file1 file2 directory/    Move multiple files to directory");
}

fn confirm(dest: &str) -> bool {
    print!("mv: overwrite '{}'? ", dest);
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let response = input.trim().to_lowercase();
        response == "y" || response == "yes"
    } else {
        false
    }
}

fn move_item(src: &Path, dest: &Path, verbose: bool) -> io::Result<()> {
    fs::rename(src, dest)?;
    
    if verbose {
        println!("renamed '{}' -> '{}'", src.display(), dest.display());
    }
    
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 3 {
        print_usage();
        process::exit(1);
    }

    let mut force = false;
    let mut interactive = false;
    let mut verbose = false;
    let mut no_clobber = false;
    let mut paths: Vec<&str> = Vec::new();
    
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-f" || arg == "--force" {
            force = true;
            interactive = false;
        } else if arg == "-i" || arg == "--interactive" {
            if !force {
                interactive = true;
            }
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg == "-n" || arg == "--no-clobber" {
            no_clobber = true;
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Handle combined options
            for c in arg[1..].chars() {
                match c {
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
                    'n' => no_clobber = true,
                    _ => {
                        eprintln!("mv: unknown option: -{}", c);
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg);
        }
    }

    if paths.len() < 2 {
        eprintln!("mv: missing destination operand");
        process::exit(1);
    }

    let dest = paths.pop().unwrap();
    let dest_path = Path::new(dest);
    let dest_is_dir = dest_path.is_dir();
    
    // Multiple sources require destination to be a directory
    if paths.len() > 1 && !dest_is_dir {
        eprintln!("mv: target '{}' is not a directory", dest);
        process::exit(1);
    }

    let mut exit_code = 0;
    
    for src in paths {
        let src_path = Path::new(src);
        
        if !src_path.exists() {
            eprintln!("mv: cannot stat '{}': No such file or directory", src);
            exit_code = 1;
            continue;
        }
        
        let final_dest = if dest_is_dir {
            dest_path.join(src_path.file_name().unwrap_or_default())
        } else {
            dest_path.to_path_buf()
        };
        
        // Check if destination exists
        if final_dest.exists() {
            if no_clobber {
                continue;
            }
            if interactive && !confirm(&final_dest.to_string_lossy()) {
                continue;
            }
            // Remove destination first for overwrite
            if final_dest.is_dir() {
                if let Err(e) = fs::remove_dir_all(&final_dest) {
                    eprintln!("mv: cannot remove '{}': {}", final_dest.display(), e);
                    exit_code = 1;
                    continue;
                }
            }
        }
        
        if let Err(e) = move_item(src_path, &final_dest, verbose) {
            eprintln!("mv: cannot move '{}': {}", src, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
