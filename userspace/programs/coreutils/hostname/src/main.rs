//! hostname - Show or set the system hostname
//!
//! Usage:
//!   hostname [OPTIONS] [name]

use std::env;
use std::fs;
use std::process;

const HOSTNAME_FILE: &str = "/etc/hostname";
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";

fn print_usage() {
    println!("hostname - Show or set the system hostname");
    println!();
    println!("Usage: hostname [OPTIONS] [name]");
    println!();
    println!("Options:");
    println!("  -s, --short     Print short hostname (up to first '.')");
    println!("  -f, --fqdn      Print fully qualified domain name");
    println!("  -d, --domain    Print DNS domain name");
    println!("  -i, --ip-address  Print IP address");
    println!("  -h, --help      Show this help message");
    println!();
    println!("Without arguments, displays the current hostname.");
    println!("With a name argument, sets the hostname (requires root).");
}

fn get_hostname() -> String {
    // Try reading from /proc first
    if let Ok(hostname) = fs::read_to_string(PROC_HOSTNAME) {
        return hostname.trim().to_string();
    }
    
    // Fall back to /etc/hostname
    if let Ok(hostname) = fs::read_to_string(HOSTNAME_FILE) {
        return hostname.trim().to_string();
    }
    
    // Default hostname
    "localhost".to_string()
}

fn set_hostname(name: &str) -> Result<(), String> {
    // Try to write to /proc first (requires root)
    if fs::write(PROC_HOSTNAME, format!("{}\n", name)).is_ok() {
        return Ok(());
    }
    
    // Try /etc/hostname
    if fs::write(HOSTNAME_FILE, format!("{}\n", name)).is_ok() {
        return Ok(());
    }
    
    Err("hostname: you must be root to change the hostname".to_string())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut short = false;
    let mut fqdn = false;
    let mut domain = false;
    let mut ip_address = false;
    let mut new_hostname: Option<&str> = None;
    
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-s" | "--short" => short = true,
            "-f" | "--fqdn" | "--long" => fqdn = true,
            "-d" | "--domain" => domain = true,
            "-i" | "--ip-address" => ip_address = true,
            _ if arg.starts_with('-') => {
                eprintln!("hostname: unknown option: {}", arg);
                process::exit(1);
            }
            _ => {
                new_hostname = Some(arg);
            }
        }
    }

    // Set hostname if name is provided
    if let Some(name) = new_hostname {
        if let Err(e) = set_hostname(name) {
            eprintln!("{}", e);
            process::exit(1);
        }
        process::exit(0);
    }

    // Get and display hostname
    let hostname = get_hostname();
    
    if short {
        // Print up to first '.'
        let short_name = hostname.split('.').next().unwrap_or(&hostname);
        println!("{}", short_name);
    } else if fqdn {
        // For FQDN, we'd need DNS resolution which isn't available
        // Just print the hostname as-is
        println!("{}", hostname);
    } else if domain {
        // Print domain part (after first '.')
        if let Some(idx) = hostname.find('.') {
            println!("{}", &hostname[idx + 1..]);
        } else {
            // No domain part
            println!();
        }
    } else if ip_address {
        // Would need network interface query, just print localhost for now
        println!("127.0.0.1");
    } else {
        println!("{}", hostname);
    }
}
