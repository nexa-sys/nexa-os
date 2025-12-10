//! uptime - Show how long the system has been running
//!
//! Usage:
//!   uptime [OPTIONS]

use std::env;
use std::fs;
use std::process;

fn print_usage() {
    println!("uptime - Show how long the system has been running");
    println!();
    println!("Usage: uptime [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -p, --pretty    Show uptime in pretty format");
    println!("  -s, --since     System up since (boot time)");
    println!("  -h, --help      Show this help message");
}

fn get_uptime_seconds() -> Option<f64> {
    // Read from /proc/uptime
    if let Ok(content) = fs::read_to_string("/proc/uptime") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if !parts.is_empty() {
            if let Ok(uptime) = parts[0].parse::<f64>() {
                return Some(uptime);
            }
        }
    }
    None
}

fn get_load_average() -> Option<(f64, f64, f64)> {
    // Read from /proc/loadavg
    if let Ok(content) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            let load1 = parts[0].parse::<f64>().ok()?;
            let load5 = parts[1].parse::<f64>().ok()?;
            let load15 = parts[2].parse::<f64>().ok()?;
            return Some((load1, load5, load15));
        }
    }
    None
}

fn format_uptime(seconds: f64) -> String {
    let total_seconds = seconds as u64;
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;

    if days > 0 {
        if hours > 0 {
            format!("{} day(s), {:02}:{:02}", days, hours, minutes)
        } else {
            format!("{} day(s), {} min", days, minutes)
        }
    } else if hours > 0 {
        format!("{:02}:{:02}", hours, minutes)
    } else {
        format!("{} min", minutes)
    }
}

fn format_pretty_uptime(seconds: f64) -> String {
    let total_seconds = seconds as u64;
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;

    let mut parts: Vec<String> = Vec::new();

    if days > 0 {
        parts.push(format!("{} day{}", days, if days == 1 { "" } else { "s" }));
    }
    if hours > 0 {
        parts.push(format!(
            "{} hour{}",
            hours,
            if hours == 1 { "" } else { "s" }
        ));
    }
    if minutes > 0 || parts.is_empty() {
        parts.push(format!(
            "{} minute{}",
            minutes,
            if minutes == 1 { "" } else { "s" }
        ));
    }

    format!("up {}", parts.join(", "))
}

fn get_current_time() -> String {
    // Read from /proc/driver/rtc or just return a placeholder
    // In a real system, we'd use a time syscall
    "??:??:??".to_string()
}

fn get_user_count() -> usize {
    // Count entries in /var/run/utmp or similar
    // For now, return 1 as placeholder
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut pretty = false;
    let mut since = false;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-p" | "--pretty" => pretty = true,
            "-s" | "--since" => since = true,
            _ if arg.starts_with('-') => {
                eprintln!("uptime: unknown option: {}", arg);
                process::exit(1);
            }
            _ => {
                eprintln!("uptime: extra operand: {}", arg);
                process::exit(1);
            }
        }
    }

    let uptime_secs = match get_uptime_seconds() {
        Some(s) => s,
        None => {
            eprintln!("uptime: cannot read uptime");
            process::exit(1);
        }
    };

    if since {
        // Calculate boot time (current time - uptime)
        // Without proper time syscalls, we can't show accurate boot time
        println!("boot time unavailable");
        process::exit(0);
    }

    if pretty {
        println!("{}", format_pretty_uptime(uptime_secs));
        process::exit(0);
    }

    // Standard format: time up X days, HH:MM, N users, load average: X.XX, X.XX, X.XX
    let time = get_current_time();
    let uptime_str = format_uptime(uptime_secs);
    let users = get_user_count();

    let load_str = if let Some((l1, l5, l15)) = get_load_average() {
        format!("load average: {:.2}, {:.2}, {:.2}", l1, l5, l15)
    } else {
        "load average: 0.00, 0.00, 0.00".to_string()
    };

    println!(
        " {}  up {},  {} user{},  {}",
        time,
        uptime_str,
        users,
        if users == 1 { "" } else { "s" },
        load_str
    );
}
