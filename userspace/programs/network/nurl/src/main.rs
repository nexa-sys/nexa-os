/// nurl - A curl-like HTTP/HTTPS client for NexaOS
///
/// Supports HTTP/1.1 with plans for HTTP/2 and HTTP/3.
///
/// Usage: nurl [OPTIONS] <URL>
///
/// Options:
///   -X, --request METHOD   HTTP method to use (default: GET)
///   -d, --data DATA        Data to send in POST request
///   -H, --header HEADER    Add custom header
///   -o, --output FILE      Write output to file
///   -i, --include          Include response headers in output
///   -v, --verbose          Verbose output
///   -k, --insecure         Allow insecure SSL connections
///   --http1.1              Force HTTP/1.1
///   --http2                Use HTTP/2 (when available)
///   --http3                Use HTTP/3 (when available)
///   --help                 Show this help message

mod args;
mod http;
mod url;

// TLS module selection based on features:
// - https: static linking with nssl crate
// - https-dynamic: dynamic linking with libnssl.so via FFI

#[cfg(feature = "https")]
mod tls;

#[cfg(feature = "https-dynamic")]
mod nssl_ffi;

#[cfg(feature = "https-dynamic")]
#[path = "tls_dynamic.rs"]
mod tls;

// Fallback when no TLS support
#[cfg(not(any(feature = "https", feature = "https-dynamic")))]
mod tls {
    use crate::http::HttpError;
    
    pub struct TlsConnection;
    
    impl TlsConnection {
        pub fn new(_fd: i32, _hostname: &str, _insecure: bool) -> Result<Self, HttpError> {
            Err(HttpError::NotSupported(
                "HTTPS not supported (compile with 'https' or 'https-dynamic' feature)".to_string(),
            ))
        }
        
        pub fn new_with_alpn(
            _fd: i32, _hostname: &str, _insecure: bool, _alpn: &[&str]
        ) -> Result<Self, HttpError> {
            Err(HttpError::NotSupported(
                "HTTPS not supported (compile with 'https' or 'https-dynamic' feature)".to_string(),
            ))
        }
    }
}

use std::fs::File;
use std::io::Write;
use std::process;

use args::{parse_args, print_usage};
use http::perform_request;
use url::parse_url;

fn main() {
    let args = parse_args();

    if args.url.is_none() {
        print_usage();
        process::exit(1);
    }

    let url_str = args.url.as_ref().unwrap();

    if args.verbose {
        eprintln!("* Requesting: {}", url_str);
        eprintln!("* Method: {}", args.method.as_str());
    }

    // Parse URL
    let url = match parse_url(url_str) {
        Ok(u) => u,
        Err(err) => {
            eprintln!("Error parsing URL: {}", err);
            process::exit(1);
        }
    };

    if args.verbose {
        eprintln!("* Protocol: {}", url.scheme());
        eprintln!("* Host: {}", url.host);
        eprintln!("* Port: {}", url.port);
    }

    // Perform request
    let response = match perform_request(&args, &url) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Error: {}", err);
            process::exit(1);
        }
    };

    // Output response
    let output: Box<dyn Write> = if let Some(ref path) = args.output_file {
        match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("Error creating output file '{}': {}", path, e);
                process::exit(1);
            }
        }
    } else {
        Box::new(std::io::stdout())
    };

    if let Err(e) = write_output(output, &args, &response) {
        eprintln!("Error writing output: {}", e);
        process::exit(1);
    }
}

/// Write response to output
fn write_output(
    mut output: Box<dyn Write>,
    args: &args::Args,
    response: &http::HttpResponse,
) -> std::io::Result<()> {
    // Print headers if requested
    if args.include_headers {
        writeln!(output, "{}", response.status_line())?;
        for (key, value) in &response.headers {
            writeln!(output, "{}: {}", key, value)?;
        }
        writeln!(output)?;
    }

    // Print body
    output.write_all(&response.body)?;

    Ok(())
}
