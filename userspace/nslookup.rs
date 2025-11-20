/// nslookup - DNS query tool for NexaOS
///
/// Usage: nslookup <hostname> [nameserver]
///        nslookup -type=<TYPE> <hostname> [nameserver]
///
/// Query types: A, AAAA, MX, NS, TXT, SOA, PTR, ANY
///
/// This tool uses std's UdpSocket for real DNS queries

use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::process;
use std::net::{UdpSocket, SocketAddr};
use std::time::Duration;

// DNS query type constants (from nrlib dns module)
const QTYPE_A: u16 = 1;
const QTYPE_AAAA: u16 = 28;
const QTYPE_MX: u16 = 15;
const QTYPE_NS: u16 = 2;
const QTYPE_TXT: u16 = 16;
const QTYPE_SOA: u16 = 6;
const QTYPE_PTR: u16 = 12;
const QTYPE_CNAME: u16 = 5;
const QTYPE_SRV: u16 = 33;
const QTYPE_ANY: u16 = 255;

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
            QueryType::A => QTYPE_A,
            QueryType::NS => QTYPE_NS,
            QueryType::CNAME => QTYPE_CNAME,
            QueryType::SOA => QTYPE_SOA,
            QueryType::PTR => QTYPE_PTR,
            QueryType::MX => QTYPE_MX,
            QueryType::TXT => QTYPE_TXT,
            QueryType::AAAA => QTYPE_AAAA,
            QueryType::SRV => QTYPE_SRV,
            QueryType::ANY => QTYPE_ANY,
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

/// Build a simple DNS query packet for A record
fn build_dns_query(hostname: &str, qtype: QueryType) -> Vec<u8> {
    let mut query = vec![
        0x12, 0x34, // Transaction ID
        0x01, 0x00, // Flags: Standard query, recursion desired
        0x00, 0x01, // Questions: 1
        0x00, 0x00, // Answer RRs: 0
        0x00, 0x00, // Authority RRs: 0
        0x00, 0x00, // Additional RRs: 0
    ];

    // Encode domain name (e.g., example.com -> 7example3com0)
    for label in hostname.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0); // Root label

    // Question type (e.g., A = 1)
    query.extend_from_slice(&qtype.to_code().to_be_bytes());
    // Question class (IN = 1)
    query.extend_from_slice(&1u16.to_be_bytes());

    query
}

/// Parse A record from DNS response
fn parse_dns_response_a(response: &[u8]) -> Option<[u8; 4]> {
    if response.len() < 12 {
        return None;
    }

    // Check if response flag is set and no error
    let flags = u16::from_be_bytes([response[2], response[3]]);
    if (flags & 0x8000) == 0 || (flags & 0x0F) != 0 {
        return None;
    }

    // Get answer count
    let answer_count = u16::from_be_bytes([response[6], response[7]]) as usize;
    if answer_count == 0 {
        return None;
    }

    // Skip questions section
    let mut offset = 12;
    let question_count = u16::from_be_bytes([response[4], response[5]]) as usize;
    
    for _ in 0..question_count {
        // Skip name (look for null terminator)
        while offset < response.len() && response[offset] != 0 {
            let len = response[offset] as usize;
            if len & 0xC0 == 0xC0 {
                // Compression pointer
                offset += 2;
                break;
            } else {
                offset += 1 + len;
            }
        }
        if offset < response.len() {
            offset += 1; // Skip null terminator
        }
        offset += 4; // Skip type and class
    }

    // Parse answer section
    for _ in 0..answer_count.min(1) {
        // Skip name
        while offset < response.len() && response[offset] != 0 {
            let len = response[offset];
            if len & 0xC0 == 0xC0 {
                offset += 2;
                break;
            } else {
                offset += 1 + len as usize;
            }
        }
        if offset < response.len() {
            offset += 1; // Skip null
        }

        if offset + 10 > response.len() {
            return None;
        }

        let rtype = u16::from_be_bytes([response[offset], response[offset + 1]]);
        let rdlength = u16::from_be_bytes([response[offset + 8], response[offset + 9]]) as usize;
        offset += 10;

        if rtype == 1 && rdlength == 4 && offset + 4 <= response.len() {
            // A record found
            let ip = [
                response[offset],
                response[offset + 1],
                response[offset + 2],
                response[offset + 3],
            ];
            return Some(ip);
        }
        offset += rdlength;
    }

    None
}

/// Query DNS server via UDP
fn query_dns(hostname: &str, nameserver: [u8; 4], qtype: QueryType) -> Option<[u8; 4]> {
    let query = build_dns_query(hostname, qtype);

    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let ns_addr = format!(
        "{}.{}.{}.{}:53",
        nameserver[0], nameserver[1], nameserver[2], nameserver[3]
    );
    let ns_socket_addr: SocketAddr = ns_addr.parse().ok()?;

    socket.send_to(&query, ns_socket_addr).ok()?;

    let mut response_buf = [0u8; 512];
    let (n, _) = socket.recv_from(&mut response_buf).ok()?;

    parse_dns_response_a(&response_buf[..n])
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
    println!("  nslookup -type=A example.com");
    println!("  nslookup example.com 8.8.8.8");
}

fn main() {
    let args: Vec<String> = env::args().collect();

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

    // Determine nameserver to use (determine early so server info is printed the same as Linux nslookup)
    let ns = nameserver.unwrap_or_else(|| {
        match read_resolv_conf() {
            Ok(nameservers) if !nameservers.is_empty() => nameservers[0],
            _ => {
                eprintln!("Warning: No nameservers in /etc/resolv.conf, using default");
                [8, 8, 8, 8] // Google DNS
            }
        }
    });

    // Print server info (always show the nameserver like Linux nslookup does)
    println!("Server:\t\t{}.{}.{}.{}", ns[0], ns[1], ns[2], ns[3]);
    println!("Address:\t{}.{}.{}.{}#53", ns[0], ns[1], ns[2], ns[3]);
    println!();

    // Check /etc/hosts first (for A records only) — if found, print using the normal server lines above
    if matches!(qtype, QueryType::A) {
        match lookup_hosts(&hostname) {
            Ok(Some(ip)) => {
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

    // (Server info already printed above)

    // Perform DNS query
    match query_dns(&hostname, ns, qtype) {
        Some(ip) => {
            println!("Name:\t{}", hostname);
            println!("Address: {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
        }
        None => {
            eprintln!("Error: No {} record found for {}", 
                match qtype {
                    QueryType::A => "A",
                    QueryType::AAAA => "AAAA",
                    QueryType::MX => "MX",
                    QueryType::NS => "NS",
                    QueryType::TXT => "TXT",
                    QueryType::SOA => "SOA",
                    QueryType::PTR => "PTR",
                    QueryType::CNAME => "CNAME",
                    QueryType::SRV => "SRV",
                    QueryType::ANY => "ANY",
                },
                hostname);
            process::exit(1);
        }
    }
}

