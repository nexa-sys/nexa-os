//! ls - List directory contents
//!
//! Usage:
//!   ls [-a] [-l] [path]
//!     -a  Show all files (including hidden)
//!     -l  Long format with details

use std::arch::asm;
use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process;

// NexaOS syscall numbers
const SYS_LIST_FILES: u64 = 200;
const SYS_GETERRNO: u64 = 201;
const LIST_FLAG_INCLUDE_HIDDEN: u64 = 0x1;

#[repr(C)]
struct ListDirRequest {
    path_ptr: u64,
    path_len: u64,
    flags: u64,
}

#[inline(always)]
fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x81",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            clobber_abi("sysv64")
        );
    }
    ret
}

fn syscall1(n: u64, a1: u64) -> u64 {
    syscall3(n, a1, 0, 0)
}

fn errno() -> i32 {
    syscall1(SYS_GETERRNO, 0) as i32
}

fn list_files(path: Option<&str>, include_hidden: bool) -> Result<String, i32> {
    let mut request = ListDirRequest {
        path_ptr: 0,
        path_len: 0,
        flags: if include_hidden { LIST_FLAG_INCLUDE_HIDDEN } else { 0 },
    };

    if let Some(p) = path {
        if p != "/" {
            request.path_ptr = p.as_ptr() as u64;
            request.path_len = p.len() as u64;
        }
    }

    let req_ptr = if request.path_len == 0 && request.flags == 0 {
        0
    } else {
        &request as *const ListDirRequest as u64
    };

    let mut buf = vec![0u8; 4096];
    let written = syscall3(SYS_LIST_FILES, buf.as_mut_ptr() as u64, buf.len() as u64, req_ptr);
    
    if written == u64::MAX {
        return Err(errno());
    }

    buf.truncate(written as usize);
    String::from_utf8(buf).map_err(|_| -1)
}

fn format_mode(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o040000 => 'd',
        0o100000 => '-',
        0o120000 => 'l',
        0o020000 => 'c',
        0o060000 => 'b',
        0o010000 => 'p',
        0o140000 => 's',
        _ => '?',
    };

    let mut result = String::with_capacity(10);
    result.push(file_type);
    
    for shift in [6, 3, 0] {
        let p = (mode >> shift) & 0o7;
        result.push(if (p & 0o4) != 0 { 'r' } else { '-' });
        result.push(if (p & 0o2) != 0 { 'w' } else { '-' });
        result.push(if (p & 0o1) != 0 { 'x' } else { '-' });
    }
    
    result
}

fn print_usage() {
    println!("ls - List directory contents");
    println!();
    println!("Usage: ls [OPTIONS] [PATH]");
    println!();
    println!("Options:");
    println!("  -a    Show all files (including hidden)");
    println!("  -l    Long format with details");
    println!("  -h    Show this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut show_all = false;
    let mut long_format = false;
    let mut target: Option<&str> = None;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg[1..].chars() {
                match c {
                    'a' => show_all = true,
                    'l' => long_format = true,
                    'h' => {
                        print_usage();
                        process::exit(0);
                    }
                    _ => {
                        eprintln!("ls: unknown option -{}", c);
                        process::exit(1);
                    }
                }
            }
        } else {
            target = Some(arg);
        }
    }

    let path = target.unwrap_or(".");
    let path_str = if path == "." {
        match env::current_dir() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => "/".to_string(),
        }
    } else {
        path.to_string()
    };

    match list_files(Some(&path_str), show_all) {
        Ok(list) => {
            for entry in list.lines() {
                if entry.is_empty() { continue; }
                if !show_all && entry.starts_with('.') { continue; }

                if long_format {
                    let full_path = Path::new(&path_str).join(entry);
                    if let Ok(meta) = fs::metadata(&full_path) {
                        let mode = format_mode(meta.mode());
                        println!("{} {:>4} {:>4} {:>8} {}", 
                            mode, meta.uid(), meta.gid(), meta.len(), entry);
                        continue;
                    }
                }
                println!("{}", entry);
            }
        }
        Err(e) => {
            eprintln!("ls: failed to read directory '{}' (errno {})", path, e);
            process::exit(1);
        }
    }
}
