//! id - Print user and group IDs
//!
//! Usage:
//!   id [OPTIONS] [USER]

use std::env;
use std::fs;
use std::process;

fn print_usage() {
    println!("id - Print user and group IDs");
    println!();
    println!("Usage: id [OPTIONS] [USER]");
    println!();
    println!("Options:");
    println!("  -u, --user     Print only the effective user ID");
    println!("  -g, --group    Print only the effective group ID");
    println!("  -G, --groups   Print all group IDs");
    println!("  -n, --name     Print name instead of number (with -ugG)");
    println!("  -r, --real     Print real ID instead of effective ID");
    println!("  -h, --help     Show this help message");
    println!();
    println!("Without options, prints complete identity information.");
}

// Syscall wrappers
fn getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

fn geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

fn getgid() -> u32 {
    extern "C" {
        fn getgid() -> u32;
    }
    unsafe { getgid() }
}

fn getegid() -> u32 {
    extern "C" {
        fn getegid() -> u32;
    }
    unsafe { getegid() }
}

/// Get username from UID by reading /etc/passwd
fn get_username(uid: u32) -> Option<String> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;

    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 {
            if let Ok(user_uid) = fields[2].parse::<u32>() {
                if user_uid == uid {
                    return Some(fields[0].to_string());
                }
            }
        }
    }
    None
}

/// Get group name from GID by reading /etc/group
fn get_groupname(gid: u32) -> Option<String> {
    let group = fs::read_to_string("/etc/group").ok()?;

    for line in group.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 {
            if let Ok(group_gid) = fields[2].parse::<u32>() {
                if group_gid == gid {
                    return Some(fields[0].to_string());
                }
            }
        }
    }
    None
}

/// Get all groups for a user
fn get_user_groups(username: &str, primary_gid: u32) -> Vec<(u32, String)> {
    let mut groups = Vec::new();

    // Add primary group
    if let Some(name) = get_groupname(primary_gid) {
        groups.push((primary_gid, name));
    } else {
        groups.push((primary_gid, primary_gid.to_string()));
    }

    // Check supplementary groups in /etc/group
    if let Ok(group_file) = fs::read_to_string("/etc/group") {
        for line in group_file.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 4 {
                let group_name = fields[0];
                let gid: u32 = fields[2].parse().unwrap_or(0);
                let members: Vec<&str> = fields[3].split(',').collect();

                if gid != primary_gid && members.contains(&username) {
                    groups.push((gid, group_name.to_string()));
                }
            }
        }
    }

    groups
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut show_user = false;
    let mut show_group = false;
    let mut show_groups = false;
    let mut show_name = false;
    let mut show_real = false;
    let mut target_user: Option<&str> = None;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-u" | "--user" => show_user = true,
            "-g" | "--group" => show_group = true,
            "-G" | "--groups" => show_groups = true,
            "-n" | "--name" => show_name = true,
            "-r" | "--real" => show_real = true,
            _ if arg.starts_with('-') => {
                // Handle combined options
                for c in arg[1..].chars() {
                    match c {
                        'u' => show_user = true,
                        'g' => show_group = true,
                        'G' => show_groups = true,
                        'n' => show_name = true,
                        'r' => show_real = true,
                        _ => {
                            eprintln!("id: unknown option: -{}", c);
                            process::exit(1);
                        }
                    }
                }
            }
            _ => {
                if target_user.is_none() {
                    target_user = Some(arg);
                } else {
                    eprintln!("id: extra operand '{}'", arg);
                    process::exit(1);
                }
            }
        }
    }

    let uid = if show_real { getuid() } else { geteuid() };
    let gid = if show_real { getgid() } else { getegid() };

    let username = get_username(uid).unwrap_or_else(|| uid.to_string());
    let groupname = get_groupname(gid).unwrap_or_else(|| gid.to_string());

    if show_user {
        if show_name {
            println!("{}", username);
        } else {
            println!("{}", uid);
        }
    } else if show_group {
        if show_name {
            println!("{}", groupname);
        } else {
            println!("{}", gid);
        }
    } else if show_groups {
        let groups = get_user_groups(&username, gid);
        let output: Vec<String> = if show_name {
            groups.iter().map(|(_, name)| name.clone()).collect()
        } else {
            groups.iter().map(|(id, _)| id.to_string()).collect()
        };
        println!("{}", output.join(" "));
    } else {
        // Default: print all
        let groups = get_user_groups(&username, gid);
        let group_str: Vec<String> = groups
            .iter()
            .map(|(id, name)| format!("{}({})", id, name))
            .collect();

        println!(
            "uid={}({}) gid={}({}) groups={}",
            uid,
            username,
            gid,
            groupname,
            group_str.join(",")
        );
    }
}
