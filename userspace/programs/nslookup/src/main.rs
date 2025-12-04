/// nslookup - DNS query tool for NexaOS
///
/// Usage: nslookup <hostname> [nameserver]
///        nslookup -type=<TYPE> <hostname> [nameserver]
///        nslookup -tcp <hostname> [nameserver]
///
/// Query types: A, AAAA, MX, NS, TXT, SOA, PTR, CNAME, SRV, ANY
/// Protocols: UDP (default), TCP (with -tcp flag)
///
/// This tool performs real DNS queries using std's UdpSocket/TcpStream

use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::process;
use std::net::{UdpSocket, TcpStream, SocketAddr};
use std::time::Duration;

// DNS constants
const DNS_PORT: u16 = 53;
const DNS_TIMEOUT_SECS: u64 = 5;
const MAX_DNS_PACKET_SIZE: usize = 512;

// DNS query type constants
const QTYPE_A: u16 = 1;
const QTYPE_NS: u16 = 2;
const QTYPE_CNAME: u16 = 5;
const QTYPE_SOA: u16 = 6;
const QTYPE_PTR: u16 = 12;
const QTYPE_MX: u16 = 15;
const QTYPE_TXT: u16 = 16;
const QTYPE_AAAA: u16 = 28;
const QTYPE_SRV: u16 = 33;
const QTYPE_ANY: u16 = 255;

// DNS class constants
const QCLASS_IN: u16 = 1;

#[derive(Debug, Clone, Copy)]
enum QueryType {
    A,
    NS,
    CNAME,
    SOA,
    PTR,
    MX,
    TXT,
    AAAA,
    SRV,
    ANY,
}

impl QueryType {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "a" => Some(QueryType::A),
            "ns" => Some(QueryType::NS),
            "cname" => Some(QueryType::CNAME),
            "soa" => Some(QueryType::SOA),
            "ptr" => Some(QueryType::PTR),
            "mx" => Some(QueryType::MX),
            "txt" => Some(QueryType::TXT),
            "aaaa" => Some(QueryType::AAAA),
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

    fn name(&self) -> &'static str {
        match self {
            QueryType::A => "A",
            QueryType::NS => "NS",
            QueryType::CNAME => "CNAME",
            QueryType::SOA => "SOA",
            QueryType::PTR => "PTR",
            QueryType::MX => "MX",
            QueryType::TXT => "TXT",
            QueryType::AAAA => "AAAA",
            QueryType::SRV => "SRV",
            QueryType::ANY => "ANY",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Protocol {
    Udp,
    Tcp,
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

/// Build DNS query packet
/// Transaction ID is fixed at 0x1234, with recursion desired
fn build_dns_query(hostname: &str, qtype: QueryType) -> Vec<u8> {
    let mut query = vec![
        0x12, 0x34, // Transaction ID
        0x01, 0x00, // Flags: Standard query, recursion desired (RD=1)
        0x00, 0x01, // Questions: 1
        0x00, 0x00, // Answer RRs: 0
        0x00, 0x00, // Authority RRs: 0
        0x00, 0x00, // Additional RRs: 0
    ];

    // Encode domain name in DNS format (length-prefixed labels)
    // Example: "example.com" -> 7example3com0
    for label in hostname.split('.') {
        if label.is_empty() {
            continue;
        }
        if label.len() > 63 {
            eprintln!("Warning: DNS label too long (max 63): {}", label);
            continue;
        }
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0); // Root label (null terminator)

    // Question type (QTYPE)
    query.extend_from_slice(&qtype.to_code().to_be_bytes());
    // Question class (IN = 1)
    query.extend_from_slice(&QCLASS_IN.to_be_bytes());

    query
}

/// Parse domain name from DNS packet (handles compression)
fn parse_dns_name(data: &[u8], offset: &mut usize) -> Option<String> {
    let mut name = String::new();
    let mut jumped = false;
    let mut jump_offset = 0;
    let mut current_offset = *offset;
    let mut first = true;

    loop {
        if current_offset >= data.len() {
            return None;
        }

        let len = data[current_offset];

        // Check for compression pointer (top 2 bits set)
        if len & 0xC0 == 0xC0 {
            if current_offset + 1 >= data.len() {
                return None;
            }
            let pointer = u16::from_be_bytes([len & 0x3F, data[current_offset + 1]]) as usize;
            if !jumped {
                jump_offset = current_offset + 2;
                jumped = true;
            }
            current_offset = pointer;
            continue;
        }

        // Null terminator - end of name
        if len == 0 {
            current_offset += 1;
            break;
        }

        // Read label
        if !first {
            name.push('.');
        }
        first = false;

        current_offset += 1;
        if current_offset + len as usize > data.len() {
            return None;
        }

        if let Ok(label) = std::str::from_utf8(&data[current_offset..current_offset + len as usize]) {
            name.push_str(label);
        } else {
            return None;
        }

        current_offset += len as usize;
    }

    // Update offset to after the name (considering jumps)
    if jumped {
        *offset = jump_offset;
    } else {
        *offset = current_offset;
    }

    Some(name)
}

/// Parse A record (IPv4 address) from DNS response
fn parse_a_record(data: &[u8], offset: usize) -> Option<[u8; 4]> {
    if offset + 4 > data.len() {
        return None;
    }
    Some([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

/// Parse AAAA record (IPv6 address) from DNS response
fn parse_aaaa_record(data: &[u8], offset: usize) -> Option<[u8; 16]> {
    if offset + 16 > data.len() {
        return None;
    }
    let mut addr = [0u8; 16];
    addr.copy_from_slice(&data[offset..offset + 16]);
    Some(addr)
}

/// Parse DNS response and extract records
fn parse_dns_response(response: &[u8], qtype: QueryType) -> Result<Vec<String>, String> {
    if response.len() < 12 {
        return Err("Response too short (< 12 bytes)".to_string());
    }

    // Parse DNS header
    let flags = u16::from_be_bytes([response[2], response[3]]);
    let is_response = (flags & 0x8000) != 0;
    let rcode = flags & 0x000F;

    if !is_response {
        return Err("Not a response packet".to_string());
    }

    if rcode != 0 {
        let error_msg = match rcode {
            1 => "Format error",
            2 => "Server failure",
            3 => "Name error (NXDOMAIN)",
            4 => "Not implemented",
            5 => "Refused",
            _ => "Unknown error",
        };
        return Err(format!("DNS error: {} (RCODE={})", error_msg, rcode));
    }

    let question_count = u16::from_be_bytes([response[4], response[5]]) as usize;
    let answer_count = u16::from_be_bytes([response[6], response[7]]) as usize;

    if answer_count == 0 {
        return Err("No answers in response".to_string());
    }

    // Skip question section
    let mut offset = 12;
    for _ in 0..question_count {
        // Skip QNAME
        while offset < response.len() && response[offset] != 0 {
            let len = response[offset];
            if len & 0xC0 == 0xC0 {
                // Compression pointer
                offset += 2;
                break;
            } else {
                offset += 1 + len as usize;
            }
        }
        if offset < response.len() && response[offset] == 0 {
            offset += 1; // Skip null terminator
        }
        offset += 4; // Skip QTYPE and QCLASS
    }

    // Parse answer section
    let mut results = Vec::new();
    for _ in 0..answer_count {
        if offset >= response.len() {
            break;
        }

        // Parse NAME (can be compressed)
        let mut name_offset = offset;
        let name = parse_dns_name(response, &mut name_offset);
        offset = name_offset;

        if offset + 10 > response.len() {
            break;
        }

        let rtype = u16::from_be_bytes([response[offset], response[offset + 1]]);
        let _rclass = u16::from_be_bytes([response[offset + 2], response[offset + 3]]);
        let _ttl = u32::from_be_bytes([
            response[offset + 4],
            response[offset + 5],
            response[offset + 6],
            response[offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([response[offset + 8], response[offset + 9]]) as usize;
        offset += 10;

        if offset + rdlength > response.len() {
            break;
        }

        // Parse based on record type
        match rtype {
            QTYPE_A if matches!(qtype, QueryType::A | QueryType::ANY) => {
                if let Some(ip) = parse_a_record(response, offset) {
                    results.push(format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]));
                }
            }
            QTYPE_AAAA if matches!(qtype, QueryType::AAAA | QueryType::ANY) => {
                if let Some(ip) = parse_aaaa_record(response, offset) {
                    results.push(format!(
                        "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
                        ip[0], ip[1], ip[2], ip[3], ip[4], ip[5], ip[6], ip[7],
                        ip[8], ip[9], ip[10], ip[11], ip[12], ip[13], ip[14], ip[15]
                    ));
                }
            }
            QTYPE_CNAME | QTYPE_NS | QTYPE_PTR if matches!(qtype, QueryType::CNAME | QueryType::NS | QueryType::PTR | QueryType::ANY) => {
                let mut data_offset = offset;
                if let Some(domain) = parse_dns_name(response, &mut data_offset) {
                    let type_name = match rtype {
                        QTYPE_CNAME => "CNAME",
                        QTYPE_NS => "NS",
                        QTYPE_PTR => "PTR",
                        _ => "Unknown",
                    };
                    if let Some(ref n) = name {
                        results.push(format!("{} -> {} ({})", n, domain, type_name));
                    } else {
                        results.push(format!("{} ({})", domain, type_name));
                    }
                }
            }
            QTYPE_MX if matches!(qtype, QueryType::MX | QueryType::ANY) => {
                if rdlength >= 2 {
                    let preference = u16::from_be_bytes([response[offset], response[offset + 1]]);
                    let mut mx_offset = offset + 2;
                    if let Some(exchange) = parse_dns_name(response, &mut mx_offset) {
                        results.push(format!("MX preference={}, exchange={}", preference, exchange));
                    }
                }
            }
            QTYPE_TXT if matches!(qtype, QueryType::TXT | QueryType::ANY) => {
                let mut txt_offset = offset;
                let mut txt_parts = Vec::new();
                while txt_offset < offset + rdlength {
                    let txt_len = response[txt_offset] as usize;
                    txt_offset += 1;
                    if txt_offset + txt_len <= offset + rdlength {
                        if let Ok(txt) = std::str::from_utf8(&response[txt_offset..txt_offset + txt_len]) {
                            txt_parts.push(txt.to_string());
                        }
                        txt_offset += txt_len;
                    } else {
                        break;
                    }
                }
                if !txt_parts.is_empty() {
                    results.push(format!("TXT: \"{}\"", txt_parts.join("")));
                }
            }
            _ => {
                // Unknown or unsupported record type, skip it
            }
        }

        offset += rdlength;
    }

    if results.is_empty() {
        Err(format!("No {} records found", qtype.name()))
    } else {
        Ok(results)
    }
}

/// Query DNS server via UDP
fn query_dns_udp(hostname: &str, nameserver: [u8; 4], qtype: QueryType) -> Result<Vec<String>, String> {
    let query = build_dns_query(hostname, qtype);
    
    eprintln!("[DEBUG] About to call UdpSocket::bind...");
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to create UDP socket: {}", e))?;
    eprintln!("[DEBUG] UdpSocket::bind succeeded!");
    
    socket.set_read_timeout(Some(Duration::from_secs(DNS_TIMEOUT_SECS)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;

    let ns_addr = format!(
        "{}.{}.{}.{}:{}",
        nameserver[0], nameserver[1], nameserver[2], nameserver[3], DNS_PORT
    );
    let ns_socket_addr: SocketAddr = ns_addr.parse()
        .map_err(|e| format!("Invalid nameserver address: {}", e))?;

    // Send query - kernel handles ARP resolution automatically
    socket.send_to(&query, ns_socket_addr)
        .map_err(|e| format!("Failed to send query: {}", e))?;

    let mut response_buf = [0u8; MAX_DNS_PACKET_SIZE];
    let (n, _) = socket.recv_from(&mut response_buf)
        .map_err(|e| format!("Failed to receive response: {}", e))?;

    parse_dns_response(&response_buf[..n], qtype)
}

/// Query DNS server via TCP (for large responses or when UDP fails)
fn query_dns_tcp(hostname: &str, nameserver: [u8; 4], qtype: QueryType) -> Result<Vec<String>, String> {
    let query = build_dns_query(hostname, qtype);

    let ns_addr = format!(
        "{}.{}.{}.{}:{}",
        nameserver[0], nameserver[1], nameserver[2], nameserver[3], DNS_PORT
    );

    let mut stream = TcpStream::connect_timeout(
        &ns_addr.parse().map_err(|e| format!("Invalid nameserver address: {}", e))?,
        Duration::from_secs(DNS_TIMEOUT_SECS)
    ).map_err(|e| format!("Failed to connect via TCP: {}", e))?;

    stream.set_read_timeout(Some(Duration::from_secs(DNS_TIMEOUT_SECS)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;

    // DNS over TCP uses 2-byte length prefix
    let query_len = query.len() as u16;
    stream.write_all(&query_len.to_be_bytes())
        .map_err(|e| format!("Failed to send length: {}", e))?;
    stream.write_all(&query)
        .map_err(|e| format!("Failed to send query: {}", e))?;

    // Read 2-byte length prefix
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf)
        .map_err(|e| format!("Failed to read response length: {}", e))?;
    let response_len = u16::from_be_bytes(len_buf) as usize;

    if response_len > 65535 {
        return Err("Response too large".to_string());
    }

    // Read DNS response
    let mut response_buf = vec![0u8; response_len];
    stream.read_exact(&mut response_buf)
        .map_err(|e| format!("Failed to read response: {}", e))?;

    parse_dns_response(&response_buf, qtype)
}

fn print_usage() {
    println!("Usage: nslookup [OPTIONS] <hostname> [nameserver]");
    println!();
    println!("Options:");
    println!("  -type=<TYPE>    Query type (A, NS, CNAME, SOA, PTR, MX, TXT, AAAA, SRV, ANY)");
    println!("  -tcp            Use TCP instead of UDP");
    println!("  -h, --help      Show this help message");
    println!();
    println!("Examples:");
    println!("  nslookup example.com");
    println!("  nslookup -type=MX example.com");
    println!("  nslookup -tcp example.com 8.8.8.8");
    println!("  nslookup -type=AAAA ipv6.google.com");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    // Parse arguments
    let mut qtype = QueryType::A;
    let mut protocol = Protocol::Udp;
    let mut hostname: Option<String> = None;
    let mut nameserver: Option<[u8; 4]> = None;

    for arg in args.iter().skip(1) {
        if arg.starts_with("-type=") {
            if let Some(type_str) = arg.strip_prefix("-type=") {
                if let Some(qt) = QueryType::from_str(type_str) {
                    qtype = qt;
                } else {
                    eprintln!("Error: Invalid query type '{}'. Valid types: A, NS, CNAME, SOA, PTR, MX, TXT, AAAA, SRV, ANY", type_str);
                    process::exit(1);
                }
            }
        } else if arg == "-tcp" {
            protocol = Protocol::Tcp;
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

    // Determine nameserver to use
    let ns = nameserver.unwrap_or_else(|| {
        match read_resolv_conf() {
            Ok(nameservers) if !nameservers.is_empty() => nameservers[0],
            _ => {
                eprintln!("Warning: No nameservers in /etc/resolv.conf, using Google DNS (8.8.8.8)");
                [8, 8, 8, 8]
            }
        }
    });

    // Print server info (mimics standard nslookup output)
    println!("Server:\t\t{}.{}.{}.{}", ns[0], ns[1], ns[2], ns[3]);
    println!("Address:\t{}.{}.{}.{}#{}", ns[0], ns[1], ns[2], ns[3], DNS_PORT);
    println!();

    // Check /etc/hosts first (for A records only)
    if matches!(qtype, QueryType::A) {
        match lookup_hosts(&hostname) {
            Ok(Some(ip)) => {
                println!("Non-authoritative answer:");
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

    // Perform DNS query
    let result = match protocol {
        Protocol::Udp => query_dns_udp(&hostname, ns, qtype),
        Protocol::Tcp => query_dns_tcp(&hostname, ns, qtype),
    };

    match result {
        Ok(records) => {
            println!("Non-authoritative answer:");
            if matches!(qtype, QueryType::A | QueryType::AAAA) && !records.is_empty() {
                println!("Name:\t{}", hostname);
                for record in records {
                    println!("Address: {}", record);
                }
            } else {
                for record in records {
                    println!("{}", record);
                }
            }
        }
        Err(e) => {
            eprintln!("** server can't find {}: {}", hostname, e);
            process::exit(1);
        }
    }
}

