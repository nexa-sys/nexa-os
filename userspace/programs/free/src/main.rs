//! free - Display memory usage
//!
//! Usage:
//!   free [OPTIONS]
//!
//! Display information about free and used memory (physical and swap).

use std::env;
use std::fs;
use std::process;

fn print_usage() {
    eprintln!("free - Display memory usage");
    eprintln!();
    eprintln!("Usage: free [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -b, --bytes     Show output in bytes");
    eprintln!("  -k, --kibi      Show output in kibibytes (default)");
    eprintln!("  -m, --mebi      Show output in mebibytes");
    eprintln!("  -g, --gibi      Show output in gibibytes");
    eprintln!("  -h, --human     Show human readable output");
    eprintln!("  -t, --total     Show total line");
    eprintln!("  -s N, --seconds N  Repeat every N seconds");
    eprintln!("  --help          Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  free            Display memory in kibibytes");
    eprintln!("  free -m         Display memory in mebibytes");
    eprintln!("  free -h         Display with human-readable units");
}

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Bytes,
    Kibi,
    Mebi,
    Gibi,
    Human,
}

struct MemInfo {
    mem_total: u64,
    mem_free: u64,
    mem_available: u64,
    buffers: u64,
    cached: u64,
    swap_total: u64,
    swap_free: u64,
    swap_cached: u64,
}

fn parse_meminfo() -> Option<MemInfo> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    
    let mut info = MemInfo {
        mem_total: 0,
        mem_free: 0,
        mem_available: 0,
        buffers: 0,
        cached: 0,
        swap_total: 0,
        swap_free: 0,
        swap_cached: 0,
    };
    
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        
        let key = parts[0].trim_end_matches(':');
        let value: u64 = parts[1].parse().unwrap_or(0);
        
        // Values in /proc/meminfo are in kB
        let value_bytes = value * 1024;
        
        match key {
            "MemTotal" => info.mem_total = value_bytes,
            "MemFree" => info.mem_free = value_bytes,
            "MemAvailable" => info.mem_available = value_bytes,
            "Buffers" => info.buffers = value_bytes,
            "Cached" => info.cached = value_bytes,
            "SwapTotal" => info.swap_total = value_bytes,
            "SwapFree" => info.swap_free = value_bytes,
            "SwapCached" => info.swap_cached = value_bytes,
            _ => {}
        }
    }
    
    Some(info)
}

fn format_size(bytes: u64, unit: Unit) -> String {
    match unit {
        Unit::Bytes => format!("{:>12}", bytes),
        Unit::Kibi => format!("{:>12}", bytes / 1024),
        Unit::Mebi => format!("{:>12}", bytes / (1024 * 1024)),
        Unit::Gibi => format!("{:>12}", bytes / (1024 * 1024 * 1024)),
        Unit::Human => {
            if bytes >= 1024 * 1024 * 1024 {
                format!("{:>8.1}Gi", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
            } else if bytes >= 1024 * 1024 {
                format!("{:>8.1}Mi", bytes as f64 / (1024.0 * 1024.0))
            } else if bytes >= 1024 {
                format!("{:>8.1}Ki", bytes as f64 / 1024.0)
            } else {
                format!("{:>9}B", bytes)
            }
        }
    }
}

fn print_header(unit: Unit) {
    let unit_str = match unit {
        Unit::Bytes => "bytes",
        Unit::Kibi => "Ki",
        Unit::Mebi => "Mi",
        Unit::Gibi => "Gi",
        Unit::Human => "",
    };
    
    if unit == Unit::Human {
        println!(
            "{:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "total", "used", "free", "shared", "buff/cache", "available"
        );
    } else {
        println!(
            "{:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
            format!("total({})", unit_str),
            format!("used({})", unit_str),
            format!("free({})", unit_str),
            format!("shared({})", unit_str),
            format!("buff/cache({})", unit_str),
            format!("available({})", unit_str)
        );
    }
}

fn display_memory(unit: Unit, show_total: bool) {
    let info = match parse_meminfo() {
        Some(i) => i,
        None => {
            eprintln!("free: cannot read /proc/meminfo");
            return;
        }
    };
    
    // Calculate used memory
    let buff_cache = info.buffers + info.cached;
    let mem_used = info.mem_total.saturating_sub(info.mem_free).saturating_sub(buff_cache);
    let swap_used = info.swap_total.saturating_sub(info.swap_free);
    
    // Print header
    print_header(unit);
    
    // Memory line
    println!(
        "Mem:{} {} {} {:>12} {} {}",
        format_size(info.mem_total, unit),
        format_size(mem_used, unit),
        format_size(info.mem_free, unit),
        "0", // shared (not tracked)
        format_size(buff_cache, unit),
        format_size(info.mem_available, unit)
    );
    
    // Swap line
    println!(
        "Swap:{} {} {} {:>12} {:>12} {:>12}",
        format_size(info.swap_total, unit),
        format_size(swap_used, unit),
        format_size(info.swap_free, unit),
        "", "", ""
    );
    
    // Total line
    if show_total {
        let total = info.mem_total + info.swap_total;
        let used = mem_used + swap_used;
        let free = info.mem_free + info.swap_free;
        
        println!(
            "Total:{} {} {} {:>12} {:>12} {:>12}",
            format_size(total, unit),
            format_size(used, unit),
            format_size(free, unit),
            "", "", ""
        );
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut unit = Unit::Kibi;
    let mut show_total = false;
    let mut repeat_seconds: Option<u64> = None;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => {
                print_usage();
                process::exit(0);
            }
            "-b" | "--bytes" => {
                unit = Unit::Bytes;
            }
            "-k" | "--kibi" | "--kilo" => {
                unit = Unit::Kibi;
            }
            "-m" | "--mebi" | "--mega" => {
                unit = Unit::Mebi;
            }
            "-g" | "--gibi" | "--giga" => {
                unit = Unit::Gibi;
            }
            "-h" | "--human" => {
                unit = Unit::Human;
            }
            "-t" | "--total" => {
                show_total = true;
            }
            "-s" | "--seconds" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("free: --seconds requires an argument");
                    process::exit(1);
                }
                repeat_seconds = args[i].parse().ok();
            }
            arg if arg.starts_with('-') => {
                eprintln!("free: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            _ => {
                eprintln!("free: extra argument: {}", args[i]);
                print_usage();
                process::exit(1);
            }
        }
        i += 1;
    }
    
    loop {
        display_memory(unit, show_total);
        
        match repeat_seconds {
            Some(seconds) if seconds > 0 => {
                std::thread::sleep(std::time::Duration::from_secs(seconds));
                println!();
            }
            _ => break,
        }
    }
}
