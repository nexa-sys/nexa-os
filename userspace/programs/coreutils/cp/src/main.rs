//! cp - Copy files and directories
//!
//! Usage:
//!   cp [OPTIONS] <source>... <dest>

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

fn print_usage() {
    println!("cp - Copy files and directories");
    println!();
    println!("Usage: cp [OPTIONS] <source> <dest>");
    println!("       cp [OPTIONS] <source>... <directory>");
    println!();
    println!("Options:");
    println!("  -r, -R    Copy directories recursively");
    println!("  -f        Force overwrite without prompting");
    println!("  -i        Prompt before overwrite");
    println!("  -v        Verbose mode");
    println!("  -n        Do not overwrite existing files");
    println!("  -h        Show this help message");
    println!();
    println!("Examples:");
    println!("  cp file1.txt file2.txt       Copy file1.txt to file2.txt");
    println!("  cp -r dir1/ dir2/            Copy directory recursively");
    println!("  cp file1 file2 directory/    Copy multiple files to directory");
}

fn confirm(dest: &str) -> bool {
    print!("cp: overwrite '{}'? ", dest);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let response = input.trim().to_lowercase();
        response == "y" || response == "yes"
    } else {
        false
    }
}

fn copy_file(src: &Path, dest: &Path, verbose: bool) -> io::Result<()> {
    let mut src_file = File::open(src)?;
    let mut dest_file = File::create(dest)?;

    let mut buffer = [0u8; 8192];
    loop {
        match src_file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                dest_file.write_all(&buffer[..n])?;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }

    if verbose {
        println!("'{}' -> '{}'", src.display(), dest.display());
    }

    Ok(())
}

fn copy_recursive(src: &Path, dest: &Path, verbose: bool) -> io::Result<()> {
    if src.is_dir() {
        // Create destination directory if it doesn't exist
        if !dest.exists() {
            fs::create_dir_all(dest)?;
            if verbose {
                println!("created directory '{}'", dest.display());
            }
        }

        // Copy contents
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dest_path = dest.join(entry.file_name());
            copy_recursive(&src_path, &dest_path, verbose)?;
        }
    } else {
        copy_file(src, dest, verbose)?;
    }

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        print_usage();
        process::exit(1);
    }

    let mut recursive = false;
    let mut force = false;
    let mut interactive = false;
    let mut verbose = false;
    let mut no_clobber = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-r" || arg == "-R" || arg == "--recursive" {
            recursive = true;
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
                    'n' => no_clobber = true,
                    _ => {
                        eprintln!("cp: unknown option: -{}", c);
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg);
        }
    }

    if paths.len() < 2 {
        eprintln!("cp: missing destination operand");
        process::exit(1);
    }

    let dest = paths.pop().unwrap();
    let dest_path = Path::new(dest);
    let dest_is_dir = dest_path.is_dir();

    // Multiple sources require destination to be a directory
    if paths.len() > 1 && !dest_is_dir {
        eprintln!("cp: target '{}' is not a directory", dest);
        process::exit(1);
    }

    let mut exit_code = 0;

    for src in paths {
        let src_path = Path::new(src);

        if !src_path.exists() {
            eprintln!("cp: cannot stat '{}': No such file or directory", src);
            exit_code = 1;
            continue;
        }

        if src_path.is_dir() && !recursive {
            eprintln!("cp: -r not specified; omitting directory '{}'", src);
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
        }

        let result = if recursive && src_path.is_dir() {
            copy_recursive(src_path, &final_dest, verbose)
        } else {
            copy_file(src_path, &final_dest, verbose)
        };

        if let Err(e) = result {
            eprintln!("cp: cannot copy '{}': {}", src, e);
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
