//! sleep - Delay for a specified amount of time
//!
//! Usage:
//!   sleep NUMBER[SUFFIX]...

use std::env;
use std::process;
use std::thread;
use std::time::Duration;

fn print_usage() {
    println!("sleep - Delay for a specified amount of time");
    println!();
    println!("Usage: sleep NUMBER[SUFFIX]...");
    println!();
    println!("Pause for NUMBER seconds. SUFFIX may be:");
    println!("  s    seconds (default)");
    println!("  m    minutes");
    println!("  h    hours");
    println!("  d    days");
    println!();
    println!("NUMBER may be decimal (e.g., 0.5 for half a second).");
    println!("Multiple arguments are added together.");
}

fn parse_duration(arg: &str) -> Option<Duration> {
    let (num_str, suffix) = if arg.ends_with(|c: char| c.is_alphabetic()) {
        let suffix = arg.chars().last().unwrap();
        (&arg[..arg.len() - 1], suffix)
    } else {
        (arg, 's')
    };

    let num: f64 = num_str.parse().ok()?;
    if num < 0.0 {
        return None;
    }

    let multiplier = match suffix {
        's' => 1.0,
        'm' => 60.0,
        'h' => 3600.0,
        'd' => 86400.0,
        _ => return None,
    };

    let secs = num * multiplier;
    Some(Duration::from_secs_f64(secs))
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut total_duration = Duration::ZERO;

    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }

        match parse_duration(arg) {
            Some(duration) => {
                total_duration += duration;
            }
            None => {
                eprintln!("sleep: invalid time interval '{}'", arg);
                process::exit(1);
            }
        }
    }

    if total_duration.is_zero() {
        process::exit(0);
    }

    thread::sleep(total_duration);
}
