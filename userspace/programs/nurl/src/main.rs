/// nurl - A simple curl-like HTTP/HTTPS client for NexaOS
/// 
/// Usage: nurl [OPTIONS] <URL>
/// 
/// Options:
///   -X, --request METHOD   HTTP method to use (default: GET)
///   -d, --data DATA        Data to send in POST request
///   -H, --header HEADER    Add custom header
///   -i, --include          Include response headers in output
///   -v, --verbose          Verbose output
///   -k, --insecure         Allow insecure SSL connections (skip certificate verification)
///   --help                 Show this help message

use std::env;
use std::process;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use std::os::unix::io::AsRawFd;

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
    insecure: bool,
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
            insecure: false,
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
    let (is_https, host, port, path) = match parse_url(url) {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("Error parsing URL: {}", err);
            process::exit(1);
        }
    };

    if args.verbose {
        eprintln!("* Protocol: {}", if is_https { "HTTPS" } else { "HTTP" });
        eprintln!("* Connecting to {}:{}...", host, port);
    }

    // Check HTTPS support
    #[cfg(not(feature = "https"))]
    if is_https {
        eprintln!("Error: HTTPS not supported (nurl compiled without 'https' feature)");
        process::exit(1);
    }

    // Connect to server (TcpStream::connect accepts &str and will use getaddrinfo for DNS)
    let addr = format!("{}:{}", host, port);
    let stream = match TcpStream::connect(&addr) {
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
    let request = build_http_request(&args, &host, port, &path, is_https);

    if args.verbose {
        eprintln!("> {}", request.lines().next().unwrap());
    }

    // Perform request (HTTP or HTTPS)
    let response_data = if is_https {
        #[cfg(feature = "https")]
        {
            perform_https_request(stream, &host, &request, args.data.as_deref(), args.verbose, args.insecure)
        }
        #[cfg(not(feature = "https"))]
        {
            unreachable!()
        }
    } else {
        perform_http_request(stream, &request, args.data.as_deref(), args.verbose)
    };

    let response_data = match response_data {
        Ok(data) => data,
        Err(err) => {
            eprintln!("Error: {}", err);
            process::exit(1);
        }
    };

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

/// Build HTTP request string
fn build_http_request(args: &Args, host: &str, port: u16, path: &str, is_https: bool) -> String {
    let mut request = String::new();
    request.push_str(args.method.as_str());
    request.push(' ');
    request.push_str(path);
    request.push_str(" HTTP/1.1\r\n");
    
    // Add Host header
    request.push_str("Host: ");
    request.push_str(host);
    let default_port = if is_https { 443 } else { 80 };
    if port != default_port {
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

    request
}

/// Perform HTTP request
fn perform_http_request(
    mut stream: TcpStream,
    request: &str,
    body: Option<&[u8]>,
    verbose: bool,
) -> Result<Vec<u8>, String> {
    // Send request
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("Error sending request: {}", e))?;

    // Send body if present
    if let Some(body_data) = body {
        stream.write_all(body_data)
            .map_err(|e| format!("Error sending body: {}", e))?;
    }

    if verbose {
        eprintln!("* Request sent, waiting for response...");
    }

    // Read response
    let mut response_data = Vec::new();
    stream.read_to_end(&mut response_data)
        .map_err(|e| format!("Error reading response: {}", e))?;

    Ok(response_data)
}

/// Perform HTTPS request using nssl
#[cfg(feature = "https")]
fn perform_https_request(
    stream: TcpStream,
    hostname: &str,
    request: &str,
    body: Option<&[u8]>,
    verbose: bool,
    insecure: bool,
) -> Result<Vec<u8>, String> {
    use std::ffi::CString;
    
    // Initialize SSL
    unsafe {
        nssl::SSL_library_init();
    }

    // Create SSL context
    let method = unsafe { nssl::TLS_client_method() };
    if method.is_null() {
        return Err("Failed to get TLS client method".to_string());
    }

    let ctx = unsafe { nssl::SSL_CTX_new(method) };
    if ctx.is_null() {
        return Err("Failed to create SSL context".to_string());
    }

    // Set verification mode
    unsafe {
        if insecure {
            nssl::SSL_CTX_set_verify(ctx, nssl::ssl_verify::SSL_VERIFY_NONE, None);
            if verbose {
                eprintln!("* SSL certificate verification disabled");
            }
        } else {
            nssl::SSL_CTX_set_verify(ctx, nssl::ssl_verify::SSL_VERIFY_PEER, None);
            nssl::SSL_CTX_set_default_verify_paths(ctx);
        }
    }

    // Create SSL connection
    let ssl = unsafe { nssl::SSL_new(ctx) };
    if ssl.is_null() {
        unsafe { nssl::SSL_CTX_free(ctx); }
        return Err("Failed to create SSL connection".to_string());
    }

    // Set hostname for SNI
    let hostname_cstr = CString::new(hostname).map_err(|_| "Invalid hostname")?;
    unsafe {
        nssl::SSL_set_tlsext_host_name(ssl, hostname_cstr.as_ptr() as *const i8);
    }

    // Set file descriptor
    let fd = stream.as_raw_fd();
    let result = unsafe { nssl::SSL_set_fd(ssl, fd) };
    if result != 1 {
        unsafe {
            nssl::SSL_free(ssl);
            nssl::SSL_CTX_free(ctx);
        }
        return Err("Failed to set SSL file descriptor".to_string());
    }

    if verbose {
        eprintln!("* Performing TLS handshake...");
    }

    // Perform TLS handshake
    let result = unsafe { nssl::SSL_connect(ssl) };
    if result != 1 {
        let err = unsafe { nssl::SSL_get_error(ssl, result) };
        unsafe {
            nssl::SSL_free(ssl);
            nssl::SSL_CTX_free(ctx);
        }
        return Err(format!("TLS handshake failed (error: {})", err));
    }

    if verbose {
        // Get negotiated version
        let version_ptr = unsafe { nssl::SSL_get_version(ssl) };
        if !version_ptr.is_null() {
            let version = unsafe { std::ffi::CStr::from_ptr(version_ptr) };
            eprintln!("* TLS handshake complete: {}", version.to_string_lossy());
        }
        
        // Get cipher
        let cipher = unsafe { nssl::SSL_get_current_cipher(ssl) };
        if !cipher.is_null() {
            let cipher_name = unsafe { nssl::SSL_CIPHER_get_name(cipher) };
            if !cipher_name.is_null() {
                let name = unsafe { std::ffi::CStr::from_ptr(cipher_name) };
                eprintln!("* Cipher: {}", name.to_string_lossy());
            }
        }
    }

    // Send request
    let request_bytes = request.as_bytes();
    let written = unsafe { nssl::SSL_write(ssl, request_bytes.as_ptr(), request_bytes.len() as i32) };
    if written < 0 {
        unsafe {
            nssl::SSL_shutdown(ssl);
            nssl::SSL_free(ssl);
            nssl::SSL_CTX_free(ctx);
        }
        return Err("Failed to send request over TLS".to_string());
    }

    // Send body if present
    if let Some(body_data) = body {
        let written = unsafe { nssl::SSL_write(ssl, body_data.as_ptr(), body_data.len() as i32) };
        if written < 0 {
            unsafe {
                nssl::SSL_shutdown(ssl);
                nssl::SSL_free(ssl);
                nssl::SSL_CTX_free(ctx);
            }
            return Err("Failed to send body over TLS".to_string());
        }
    }

    if verbose {
        eprintln!("* Request sent, waiting for response...");
    }

    // Read response
    let mut response_data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = unsafe { nssl::SSL_read(ssl, buf.as_mut_ptr(), buf.len() as i32) };
        eprintln!("[NURL] SSL_read returned n={}", n);
        if n > 0 {
            eprintln!("[NURL] Read {} bytes, first 64: {:02x?}", n, &buf[..64.min(n as usize)]);
            response_data.extend_from_slice(&buf[..n as usize]);
        } else if n == 0 {
            // Connection closed
            eprintln!("[NURL] Connection closed (n=0)");
            break;
        } else {
            let err = unsafe { nssl::SSL_get_error(ssl, n) };
            eprintln!("[NURL] SSL_read error: err={}", err);
            if err == nssl::ssl_error::SSL_ERROR_ZERO_RETURN {
                // Clean shutdown
                eprintln!("[NURL] Clean shutdown");
                break;
            } else if err == nssl::ssl_error::SSL_ERROR_WANT_READ {
                // Would block, try again
                eprintln!("[NURL] WANT_READ, retrying");
                continue;
            } else {
                // Error
                eprintln!("[NURL] Breaking on error");
                break;
            }
        }
    }

    eprintln!("[NURL] Total response_data len={}", response_data.len());
    if !response_data.is_empty() {
        eprintln!("[NURL] First 200 bytes: {:02x?}", &response_data[..200.min(response_data.len())]);
    }

    // Shutdown SSL connection
    unsafe {
        nssl::SSL_shutdown(ssl);
        nssl::SSL_free(ssl);
        nssl::SSL_CTX_free(ctx);
    }

    // Keep the TcpStream alive until we're done
    drop(stream);

    Ok(response_data)
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
            "-k" | "--insecure" => {
                args.insecure = true;
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
    eprintln!("nurl - A simple curl-like HTTP/HTTPS client for NexaOS");
    eprintln!();
    eprintln!("Usage: nurl [OPTIONS] <URL>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -X, --request METHOD   HTTP method to use (default: GET)");
    eprintln!("  -d, --data DATA        Data to send in POST request");
    eprintln!("  -H, --header HEADER    Add custom header (format: 'Key: Value')");
    eprintln!("  -i, --include          Include response headers in output");
    eprintln!("  -v, --verbose          Verbose output");
    eprintln!("  -k, --insecure         Allow insecure SSL connections");
    eprintln!("  --help                 Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  nurl http://10.0.2.2:8000");
    eprintln!("  nurl https://example.com");
    eprintln!("  nurl -k https://self-signed.example.com");
    eprintln!("  nurl -X POST -d 'hello=world' http://10.0.2.2:8000/api");
    eprintln!("  nurl -H 'Authorization: Bearer token' http://10.0.2.2:8000/");
    eprintln!("  nurl -v -i https://example.com");
}

/// Parse URL into (is_https, host, port, path)
fn parse_url(url: &str) -> Result<(bool, String, u16, String), String> {
    // Check for https:// or http:// prefix
    let (is_https, url_rest) = if url.starts_with("https://") {
        (true, &url[8..])
    } else if url.starts_with("http://") {
        (false, &url[7..])
    } else {
        // Default to HTTP if no scheme specified
        (false, url)
    };

    // Default port based on protocol
    let default_port = if is_https { 443 } else { 80 };

    // Find the first / to split host and path
    let (host_port, path) = if let Some(slash_pos) = url_rest.find('/') {
        (&url_rest[..slash_pos], url_rest[slash_pos..].to_string())
    } else {
        (url_rest, "/".to_string())
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
        (host_port.to_string(), default_port)
    };

    Ok((is_https, host, port, path))
}

/// Parse HTTP response into (status_line, headers, body)
fn parse_response(data: &[u8]) -> Result<(String, Vec<(String, String)>, Vec<u8>), String> {
    // Find end of headers by searching for \r\n\r\n in raw bytes
    // This allows the body to contain arbitrary binary data
    let header_end = find_header_end(data)
        .ok_or("Invalid HTTP response - no header end".to_string())?;

    // Only the headers need to be valid UTF-8, body can be binary
    let headers_bytes = &data[..header_end];
    let headers_str = std::str::from_utf8(headers_bytes)
        .map_err(|_| "Invalid UTF-8 in response headers".to_string())?;

    let body_start = header_end + 4; // Skip \r\n\r\n

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

    // Extract body (can be binary data)
    let body = if body_start < data.len() {
        data[body_start..].to_vec()
    } else {
        Vec::new()
    };

    Ok((status_line, headers, body))
}

/// Find the position of \r\n\r\n in the byte slice (end of HTTP headers)
fn find_header_end(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    for i in 0..data.len() - 3 {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n' {
            return Some(i);
        }
    }
    None
}
