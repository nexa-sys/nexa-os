//! date - Print or set the system date and time
//!
//! Usage:
//!   date [OPTIONS] [+FORMAT]

use std::env;
use std::fs;
use std::process;

fn print_usage() {
    println!("date - Print or set the system date and time");
    println!();
    println!("Usage: date [OPTIONS] [+FORMAT]");
    println!();
    println!("Options:");
    println!("  -u, --utc       Print in UTC/GMT timezone");
    println!("  -R, --rfc-email Print in RFC 5322 format");
    println!("  -I, --iso-8601  Print in ISO 8601 format");
    println!("  -h, --help      Show this help message");
    println!();
    println!("Format specifiers:");
    println!("  %Y  Year (4 digits)");
    println!("  %m  Month (01-12)");
    println!("  %d  Day of month (01-31)");
    println!("  %H  Hour (00-23)");
    println!("  %M  Minute (00-59)");
    println!("  %S  Second (00-59)");
    println!("  %a  Abbreviated weekday name");
    println!("  %b  Abbreviated month name");
    println!("  %Z  Timezone name");
    println!("  %%  Literal %");
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", 
                            "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

struct DateTime {
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    weekday: u32, // 0 = Sunday
}

impl DateTime {
    fn new() -> Self {
        // Try to read from RTC or /proc
        if let Some(dt) = Self::read_from_rtc() {
            return dt;
        }
        
        // Default fallback
        DateTime {
            year: 2024,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0,
        }
    }
    
    fn read_from_rtc() -> Option<Self> {
        // Try reading from /proc/driver/rtc
        let content = fs::read_to_string("/proc/driver/rtc").ok()?;
        
        let mut year = 0u32;
        let mut month = 0u32;
        let mut day = 0u32;
        let mut hour = 0u32;
        let mut minute = 0u32;
        let mut second = 0u32;
        
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let key = parts[0].trim();
                let value = parts[1..].join(":").trim().to_string();
                
                match key {
                    "rtc_time" => {
                        let time_parts: Vec<&str> = value.split(':').collect();
                        if time_parts.len() >= 3 {
                            hour = time_parts[0].trim().parse().unwrap_or(0);
                            minute = time_parts[1].trim().parse().unwrap_or(0);
                            second = time_parts[2].trim().parse().unwrap_or(0);
                        }
                    }
                    "rtc_date" => {
                        let date_parts: Vec<&str> = value.split('-').collect();
                        if date_parts.len() >= 3 {
                            year = date_parts[0].trim().parse().unwrap_or(2024);
                            month = date_parts[1].trim().parse().unwrap_or(1);
                            day = date_parts[2].trim().parse().unwrap_or(1);
                        }
                    }
                    _ => {}
                }
            }
        }
        
        if year > 0 {
            Some(DateTime {
                year,
                month,
                day,
                hour,
                minute,
                second,
                weekday: Self::calculate_weekday(year, month, day),
            })
        } else {
            None
        }
    }
    
    fn calculate_weekday(year: u32, month: u32, day: u32) -> u32 {
        // Zeller's congruence (simplified)
        let y = if month < 3 { year - 1 } else { year } as i32;
        let m = if month < 3 { month + 12 } else { month } as i32;
        let d = day as i32;
        
        let h = (d + (13 * (m + 1)) / 5 + y + y / 4 - y / 100 + y / 400) % 7;
        ((h + 6) % 7) as u32 // Convert to Sunday = 0
    }
    
    fn format(&self, fmt: &str) -> String {
        let mut result = String::new();
        let mut chars = fmt.chars().peekable();
        
        while let Some(c) = chars.next() {
            if c == '%' {
                if let Some(&spec) = chars.peek() {
                    chars.next();
                    match spec {
                        'Y' => result.push_str(&format!("{:04}", self.year)),
                        'y' => result.push_str(&format!("{:02}", self.year % 100)),
                        'm' => result.push_str(&format!("{:02}", self.month)),
                        'd' => result.push_str(&format!("{:02}", self.day)),
                        'H' => result.push_str(&format!("{:02}", self.hour)),
                        'M' => result.push_str(&format!("{:02}", self.minute)),
                        'S' => result.push_str(&format!("{:02}", self.second)),
                        'a' => result.push_str(WEEKDAYS[self.weekday as usize % 7]),
                        'A' => result.push_str(match self.weekday {
                            0 => "Sunday",
                            1 => "Monday",
                            2 => "Tuesday",
                            3 => "Wednesday",
                            4 => "Thursday",
                            5 => "Friday",
                            6 => "Saturday",
                            _ => "?",
                        }),
                        'b' | 'h' => result.push_str(MONTHS[(self.month as usize).saturating_sub(1) % 12]),
                        'B' => result.push_str(match self.month {
                            1 => "January",
                            2 => "February",
                            3 => "March",
                            4 => "April",
                            5 => "May",
                            6 => "June",
                            7 => "July",
                            8 => "August",
                            9 => "September",
                            10 => "October",
                            11 => "November",
                            12 => "December",
                            _ => "?",
                        }),
                        'Z' => result.push_str("UTC"), // Timezone
                        'z' => result.push_str("+0000"), // Timezone offset
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        '%' => result.push('%'),
                        _ => {
                            result.push('%');
                            result.push(spec);
                        }
                    }
                } else {
                    result.push('%');
                }
            } else {
                result.push(c);
            }
        }
        
        result
    }
    
    fn default_format(&self) -> String {
        // Format: "Thu Jan  1 00:00:00 UTC 2024"
        format!("{} {} {:>2} {:02}:{:02}:{:02} UTC {}",
            WEEKDAYS[self.weekday as usize % 7],
            MONTHS[(self.month as usize).saturating_sub(1) % 12],
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.year
        )
    }
    
    fn rfc_format(&self) -> String {
        // RFC 5322 format: "Thu, 01 Jan 2024 00:00:00 +0000"
        format!("{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
            WEEKDAYS[self.weekday as usize % 7],
            self.day,
            MONTHS[(self.month as usize).saturating_sub(1) % 12],
            self.year,
            self.hour,
            self.minute,
            self.second
        )
    }
    
    fn iso_format(&self) -> String {
        // ISO 8601 format: "2024-01-01T00:00:00+00:00"
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
            self.year, self.month, self.day,
            self.hour, self.minute, self.second
        )
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut rfc_format = false;
    let mut iso_format = false;
    let mut custom_format: Option<&str> = None;
    
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-u" | "--utc" | "--universal" => {
                // UTC is our default anyway
            }
            "-R" | "--rfc-email" | "--rfc-2822" => rfc_format = true,
            "-I" | "--iso-8601" => iso_format = true,
            _ if arg.starts_with('+') => {
                custom_format = Some(&arg[1..]);
            }
            _ if arg.starts_with('-') => {
                eprintln!("date: unknown option: {}", arg);
                process::exit(1);
            }
            _ => {
                // Setting date not implemented
                eprintln!("date: cannot set date: Operation not permitted");
                process::exit(1);
            }
        }
    }

    let dt = DateTime::new();
    
    let output = if let Some(fmt) = custom_format {
        dt.format(fmt)
    } else if rfc_format {
        dt.rfc_format()
    } else if iso_format {
        dt.iso_format()
    } else {
        dt.default_format()
    };
    
    println!("{}", output);
}
