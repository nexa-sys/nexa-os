/// nurl - A simple curl-like HTTP client for NexaOS
/// 
/// Usage: nurl [OPTIONS] <URL>
/// 
/// Options:
///   -X, --request METHOD   HTTP method to use (default: GET)
///   -d, --data DATA        Data to send in POST request
///   -H, --header HEADER    Add custom header
///   -i, --include          Include response headers in output
///   -v, --verbose          Verbose output
///   --help                 Show this help message

use std::env;
use std::process;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// HTTP methods
#[derive(Debug, Clone, Copy)]
enum HttpMethod {
    Get,
    Post,
    Head,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Head => "HEAD",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
        }
    }
}

/// Command-line arguments
struct Args {
    url: Option<String>,
    method: HttpMethod,
    data: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
    include_headers: bool,
    verbose: bool,
}

impl Args {
    fn new() -> Self {
        Self {
            url: None,
            method: HttpMethod::Get,
            data: None,
            headers: Vec::new(),
            include_headers: false,
            verbose: false,
        }
    }
}

fn main() {
    let args = parse_args();

    if args.url.is_none() {
        print_usage();
        process::exit(1);
    }

    let url = args.url.as_ref().unwrap();

    if args.verbose {
        eprintln!("* Requesting: {}", url);
        eprintln!("* Method: {}", args.method.as_str());
    }

    // Parse URL
    let (host, port, path) = match parse_url(url) {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("Error parsing URL: {}", err);
            process::exit(1);
        }
    };

    // Resolve hostname to IP
    let ip_str = match resolve_host(&host) {
        Ok(ip) => format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
        Err(err) => {
            eprintln!("Error resolving host '{}': {}", host, err);
            process::exit(1);
        }
    };

    if args.verbose {
        eprintln!("* Resolved {} to {}", host, ip_str);
        eprintln!("* Connecting to {}:{}...", ip_str, port);
    }

    // Connect to server
    let addr = format!("{}:{}", ip_str, port);
    let mut stream = match TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        Duration::from_secs(10),
    ) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("Error connecting to {}: {}", addr, err);
            process::exit(1);
        }
    };

    if args.verbose {
        eprintln!("* Connected!");
    }

    // Set read/write timeouts
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(30))).ok();

    // Build HTTP request
    let mut request = String::new();
    request.push_str(args.method.as_str());
    request.push(' ');
    request.push_str(&path);
    request.push_str(" HTTP/1.1\r\n");
    
    // Add Host header
    request.push_str("Host: ");
    request.push_str(&host);
    if port != 80 {
        request.push_str(&format!(":{}", port));
    }
    request.push_str("\r\n");

    // Add custom headers
    for (key, value) in &args.headers {
        request.push_str(key);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }

    // Add Content-Length if we have a body
    if let Some(ref body_data) = args.data {
        request.push_str(&format!("Content-Length: {}\r\n", body_data.len()));
    }

    // Add Connection: close
    request.push_str("Connection: close\r\n");
    
    // End of headers
    request.push_str("\r\n");

    if args.verbose {
        eprintln!("> {}", request.lines().next().unwrap());
    }

    // Send request
    if let Err(err) = stream.write_all(request.as_bytes()) {
        eprintln!("Error sending request: {}", err);
        process::exit(1);
    }

    // Send body if present
    if let Some(ref body_data) = args.data {
        if let Err(err) = stream.write_all(body_data) {
            eprintln!("Error sending body: {}", err);
            process::exit(1);
        }
    }

    if args.verbose {
        eprintln!("* Request sent, waiting for response...");
    }

    // Read response
    let mut response_data = Vec::new();
    if let Err(err) = stream.read_to_end(&mut response_data) {
        eprintln!("Error reading response: {}", err);
        process::exit(1);
    }

    if response_data.is_empty() {
        eprintln!("Error: Empty response");
        process::exit(1);
    }

    if args.verbose {
        eprintln!("* Received {} bytes", response_data.len());
    }

    // Parse and display response
    match parse_response(&response_data) {
        Ok((status_line, headers, body)) => {
            if args.verbose {
                eprintln!("< {}", status_line);
            }

            if args.include_headers {
                println!("{}", status_line);
                for (key, value) in headers {
                    println!("{}: {}", key, value);
                }
                println!();
            }

            // Print body
            if let Ok(body_str) = std::str::from_utf8(&body) {
                print!("{}", body_str);
            } else {
                // Binary data
                std::io::stdout().write_all(&body).ok();
            }
        }
        Err(err) => {
            eprintln!("Error parsing response: {}", err);
            process::exit(1);
        }
    }
}

fn parse_args() -> Args {
    let mut args = Args::new();
    let argv: Vec<String> = env::args().collect();
    
    let mut i = 1; // Skip program name
    while i < argv.len() {
        let arg = &argv[i];

        match arg.as_str() {
            "-X" | "--request" => {
                if i + 1 < argv.len() {
                    args.method = match argv[i + 1].to_uppercase().as_str() {
                        "GET" => HttpMethod::Get,
                        "POST" => HttpMethod::Post,
                        "HEAD" => HttpMethod::Head,
                        "PUT" => HttpMethod::Put,
                        "DELETE" => HttpMethod::Delete,
                        "PATCH" => HttpMethod::Patch,
                        _ => {
                            eprintln!("Unknown HTTP method: {}", argv[i + 1]);
                            process::exit(1);
                        }
                    };
                    i += 1;
                } else {
                    eprintln!("Missing argument for -X/--request");
                    process::exit(1);
                }
            }
            "-d" | "--data" => {
                if i + 1 < argv.len() {
                    args.data = Some(argv[i + 1].as_bytes().to_vec());
                    // Auto-set method to POST if not explicitly set
                    if matches!(args.method, HttpMethod::Get) {
                        args.method = HttpMethod::Post;
                    }
                    i += 1;
                } else {
                    eprintln!("Missing argument for -d/--data");
                    process::exit(1);
                }
            }
            "-H" | "--header" => {
                if i + 1 < argv.len() {
                    if let Some(colon_pos) = argv[i + 1].find(':') {
                        let key = argv[i + 1][..colon_pos].trim().to_string();
                        let value = argv[i + 1][colon_pos + 1..].trim().to_string();
                        args.headers.push((key, value));
                    } else {
                        eprintln!("Invalid header format: {}", argv[i + 1]);
                        process::exit(1);
                    }
                    i += 1;
                } else {
                    eprintln!("Missing argument for -H/--header");
                    process::exit(1);
                }
            }
            "-i" | "--include" => {
                args.include_headers = true;
            }
            "-v" | "--verbose" => {
                args.verbose = true;
            }
            "--help" => {
                print_usage();
                process::exit(0);
            }
            _ => {
                if arg.starts_with('-') {
                    eprintln!("Unknown option: {}", arg);
                    process::exit(1);
                } else {
                    args.url = Some(arg.clone());
                }
            }
        }

        i += 1;
    }

    args
}

fn print_usage() {
    eprintln!("nurl - A simple curl-like HTTP client for NexaOS");
    eprintln!();
    eprintln!("Usage: nurl [OPTIONS] <URL>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -X, --request METHOD   HTTP method to use (default: GET)");
    eprintln!("  -d, --data DATA        Data to send in POST request");
    eprintln!("  -H, --header HEADER    Add custom header (format: 'Key: Value')");
    eprintln!("  -i, --include          Include response headers in output");
    eprintln!("  -v, --verbose          Verbose output");
    eprintln!("  --help                 Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  nurl http://10.0.2.2:8000");
    eprintln!("  nurl -X POST -d 'hello=world' http://10.0.2.2:8000/api");
    eprintln!("  nurl -H 'Authorization: Bearer token' http://10.0.2.2:8000/");
    eprintln!("  nurl -v -i http://10.0.2.2:8000");
}

/// Parse URL into (host, port, path)
fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    // Remove http:// prefix if present
    let url = if url.starts_with("http://") {
        &url[7..]
    } else {
        url
    };

    // Find the first / to split host and path
    let (host_port, path) = if let Some(slash_pos) = url.find('/') {
        (&url[..slash_pos], url[slash_pos..].to_string())
    } else {
        (url, "/".to_string())
    };

    // Split host and port
    let (host, port) = if let Some(colon_pos) = host_port.find(':') {
        let host = host_port[..colon_pos].to_string();
        let port_str = &host_port[colon_pos + 1..];
        let port = port_str
            .parse::<u16>()
            .map_err(|_| "Invalid port number".to_string())?;
        (host, port)
    } else {
        (host_port.to_string(), 80)
    };

    Ok((host, port, path))
}

/// Resolve hostname to IP address
fn resolve_host(host: &str) -> Result<[u8; 4], String> {
    // Try to parse as IP address first
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() == 4 {
        let mut ip = [0u8; 4];
        for (i, part) in parts.iter().enumerate() {
            ip[i] = part
                .parse::<u8>()
                .map_err(|_| "Invalid IP address".to_string())?;
        }
        return Ok(ip);
    }

    // For now, we don't support DNS resolution
    // You could add DNS lookup here using nslookup or a DNS library
    Err(format!("Hostname '{}' not supported - use IP address", host))
}

/// Parse HTTP response into (status_line, headers, body)
fn parse_response(data: &[u8]) -> Result<(String, Vec<(String, String)>, Vec<u8>), String> {
    let response_str = std::str::from_utf8(data)
        .map_err(|_| "Invalid UTF-8 in response".to_string())?;

    // Find end of headers
    let header_end = response_str
        .find("\r\n\r\n")
        .ok_or("Invalid HTTP response - no header end".to_string())?;

    let headers_str = &response_str[..header_end];
    let body_start = header_end + 4;

    // Parse status line
    let mut lines = headers_str.lines();
    let status_line = lines
        .next()
        .ok_or("No status line".to_string())?
        .to_string();

    // Parse headers
    let mut headers = Vec::new();
    for line in lines {
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let value = line[colon_pos + 1..].trim().to_string();
            headers.push((key, value));
        }
    }

    // Extract body
    let body = data[body_start..].to_vec();

    Ok((status_line, headers, body))
}
