//! stat - Display file status
//!
//! Usage:
//!   stat <file>...

use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process;

fn print_usage() {
    println!("stat - Display file status");
    println!();
    println!("Usage: stat [OPTIONS] <file>...");
    println!();
    println!("Options:");
    println!("  -h    Show this help message");
}

fn format_mode(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o040000 => "directory",
        0o100000 => "regular file",
        0o120000 => "symbolic link",
        0o020000 => "character device",
        0o060000 => "block device",
        0o010000 => "FIFO",
        0o140000 => "socket",
        _ => "unknown",
    };
    file_type.to_string()
}

fn stat_file(path: &str) {
    match fs::metadata(path) {
        Ok(meta) => {
            println!("  File: {}", path);
            println!("  Size: {} bytes", meta.len());
            println!("  Type: {}", format_mode(meta.mode()));
            println!("  Mode: 0o{:o}", meta.mode() & 0o7777);
            println!("Device: {}", meta.dev());
            println!(" Inode: {}", meta.ino());
            println!(" Links: {}", meta.nlink());
            println!("   Uid: {}", meta.uid());
            println!("   Gid: {}", meta.gid());
            println!("Blocks: {}", meta.blocks());
        }
        Err(e) => {
            eprintln!("stat: cannot stat '{}': {}", path, e);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut files: Vec<&str> = Vec::new();

    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }
        files.push(arg);
    }

    if files.is_empty() {
        eprintln!("stat: missing file name");
        process::exit(1);
    }

    for (i, file) in files.iter().enumerate() {
        if i > 0 {
            println!();
        }
        stat_file(file);
    }
}
