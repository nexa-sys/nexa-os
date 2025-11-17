/// nslookup - DNS query tool for NexaOS
///
/// Usage: nslookup <hostname> [nameserver]
///        nslookup -type=<TYPE> <hostname> [nameserver]
///
/// Query types: A, AAAA, MX, NS, TXT, SOA, PTR, ANY
///
/// This tool uses std's networking facilities (future: std::net::UdpSocket)
/// For now, it reads /etc configuration files and demonstrates DNS query construction.

use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::process;

#[derive(Debug, Clone, Copy)]
enum QueryType {
    A,
    AAAA,
    MX,
    NS,
    TXT,
    SOA,
    PTR,
    CNAME,
    SRV,
    ANY,
}

impl QueryType {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "a" => Some(QueryType::A),
            "aaaa" => Some(QueryType::AAAA),
            "mx" => Some(QueryType::MX),
            "ns" => Some(QueryType::NS),
            "txt" => Some(QueryType::TXT),
            "soa" => Some(QueryType::SOA),
            "ptr" => Some(QueryType::PTR),
            "cname" => Some(QueryType::CNAME),
            "srv" => Some(QueryType::SRV),
            "any" => Some(QueryType::ANY),
            _ => None,
        }
    }

    fn to_code(&self) -> u16 {
        match self {
            QueryType::A => 1,
            QueryType::NS => 2,
            QueryType::CNAME => 5,
            QueryType::SOA => 6,
            QueryType::PTR => 12,
            QueryType::MX => 15,
            QueryType::TXT => 16,
            QueryType::AAAA => 28,
            QueryType::SRV => 33,
            QueryType::ANY => 255,
        }
    }
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut index = 0;

    for part in s.split('.') {
        if index >= 4 {
            return None;
        }
        octets[index] = part.parse::<u8>().ok()?;
        index += 1;
    }

    if index != 4 {
        return None;
    }

    Some(octets)
}

fn read_resolv_conf() -> io::Result<Vec<[u8; 4]>> {
    let mut nameservers = Vec::new();
    let file = fs::File::open("/etc/resolv.conf")?;
    let reader = io::BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        if let Some("nameserver") = parts.next() {
            if let Some(ip_str) = parts.next() {
                if let Some(ip) = parse_ipv4(ip_str) {
                    nameservers.push(ip);
                }
            }
        }
    }

    Ok(nameservers)
}

fn lookup_hosts(hostname: &str) -> io::Result<Option<[u8; 4]>> {
    let file = fs::File::open("/etc/hosts")?;
    let reader = io::BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        if let Some(ip_str) = parts.next() {
            if let Some(ip) = parse_ipv4(ip_str) {
                for name in parts {
                    if name.eq_ignore_ascii_case(hostname) {
                        return Ok(Some(ip));
                    }
                }
            }
        }
    }

    Ok(None)
}

fn print_usage() {
    println!("Usage: nslookup [OPTIONS] <hostname> [nameserver]");
    println!();
    println!("Options:");
    println!("  -type=<TYPE>    Query type (A, AAAA, MX, NS, TXT, SOA, PTR, ANY)");
    println!("  -h, --help      Show this help message");
    println!();
    println!("Examples:");
    println!("  nslookup example.com");
    println!("  nslookup -type=MX example.com");
    println!("  nslookup example.com 8.8.8.8");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    println!("[nslookup-debug] raw args len={} args={:?}", args.len(), args);

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    // Parse arguments
    let mut qtype = QueryType::A;
    let mut hostname: Option<String> = None;
    let mut nameserver: Option<[u8; 4]> = None;

    for arg in args.iter().skip(1) {
        if arg.starts_with("-type=") {
            if let Some(type_str) = arg.strip_prefix("-type=") {
                if let Some(qt) = QueryType::from_str(type_str) {
                    qtype = qt;
                } else {
                    eprintln!("Error: Invalid query type '{}'", type_str);
                    process::exit(1);
                }
            }
        } else if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if hostname.is_none() {
            hostname = Some(arg.clone());
        } else if nameserver.is_none() {
            if let Some(ip) = parse_ipv4(arg) {
                nameserver = Some(ip);
            } else {
                eprintln!("Error: Invalid nameserver address '{}'", arg);
                process::exit(1);
            }
        }
    }

    let hostname = match hostname {
        Some(h) => h,
        None => {
            eprintln!("Error: No hostname specified");
            process::exit(1);
        }
    };

    // Check /etc/hosts first (for A records only)
    if matches!(qtype, QueryType::A) {
        match lookup_hosts(&hostname) {
            Ok(Some(ip)) => {
                println!("Server:\t\tlocal files");
                println!("Address:\t/etc/hosts");
                println!();
                println!("Name:\t{}", hostname);
                println!("Address: {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
                return;
            }
            Ok(None) => {
                // Not found in hosts, continue to DNS
            }
            Err(e) => {
                eprintln!("Warning: Could not read /etc/hosts: {}", e);
            }
        }
    }

    // Determine nameserver to use
    let ns = nameserver.unwrap_or_else(|| {
        match read_resolv_conf() {
            Ok(nameservers) if !nameservers.is_empty() => nameservers[0],
            _ => {
                eprintln!("Warning: No nameservers in /etc/resolv.conf, using default");
                [8, 8, 8, 8] // Google DNS
            }
        }
    });

    // Print server info
    println!("Server:\t\t{}.{}.{}.{}", ns[0], ns[1], ns[2], ns[3]);
    println!("Address:\t{}.{}.{}.{}#53", ns[0], ns[1], ns[2], ns[3]);
    println!();

    println!("Query: {} IN {:?}", hostname, qtype);
    println!();

    // TODO: Implement actual DNS query using std::net::UdpSocket
    // For now, just show what we would do
    println!("** DNS query functionality requires UDP socket support **");
    println!("** Waiting for std::net::UdpSocket implementation in NexaOS **");
    println!();
    println!("Would send DNS query:");
    println!("  - Type: {:?} (code {})", qtype, qtype.to_code());
    println!("  - Domain: {}", hostname);
    println!("  - Nameserver: {}.{}.{}.{}:53", ns[0], ns[1], ns[2], ns[3]);
    println!();
    println!("Once UDP sockets are available, this tool will:");
    println!("  1. Open a UDP socket");
    println!("  2. Send DNS query packet to nameserver");
    println!("  3. Receive and parse DNS response");
    println!("  4. Display resolved IP addresses");
}
