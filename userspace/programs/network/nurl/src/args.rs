/// Command-line argument parsing for nurl

use std::env;
use std::process;

/// HTTP methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Head,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Head => "HEAD",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "HEAD" => Some(HttpMethod::Head),
            "PUT" => Some(HttpMethod::Put),
            "DELETE" => Some(HttpMethod::Delete),
            "PATCH" => Some(HttpMethod::Patch),
            _ => None,
        }
    }
}

impl Default for HttpMethod {
    fn default() -> Self {
        HttpMethod::Get
    }
}

/// HTTP protocol version preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpVersion {
    /// HTTP/1.1 only
    #[default]
    Http1,
    /// HTTP/2 (with HTTP/1.1 fallback)
    Http2,
    /// HTTP/3 (with HTTP/2 and HTTP/1.1 fallback)
    Http3,
}

/// Parsed command-line arguments
#[derive(Default)]
pub struct Args {
    pub url: Option<String>,
    pub method: HttpMethod,
    pub data: Option<Vec<u8>>,
    pub headers: Vec<(String, String)>,
    pub include_headers: bool,
    pub verbose: bool,
    pub insecure: bool,
    pub http_version: HttpVersion,
    pub output_file: Option<String>,
    /// Request compressed response (gzip, deflate)
    pub compressed: bool,
}

impl Args {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Parse command-line arguments
pub fn parse_args() -> Args {
    let mut args = Args::new();
    let argv: Vec<String> = env::args().collect();

    let mut i = 1; // Skip program name
    while i < argv.len() {
        let arg = &argv[i];

        match arg.as_str() {
            "-X" | "--request" => {
                if i + 1 < argv.len() {
                    match HttpMethod::from_str(&argv[i + 1]) {
                        Some(method) => args.method = method,
                        None => {
                            eprintln!("Unknown HTTP method: {}", argv[i + 1]);
                            process::exit(1);
                        }
                    }
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
                    if args.method == HttpMethod::Get {
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
            "-o" | "--output" => {
                if i + 1 < argv.len() {
                    args.output_file = Some(argv[i + 1].clone());
                    i += 1;
                } else {
                    eprintln!("Missing argument for -o/--output");
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
            "--http1.1" => {
                args.http_version = HttpVersion::Http1;
            }
            "--http2" => {
                args.http_version = HttpVersion::Http2;
            }
            "--http3" => {
                args.http_version = HttpVersion::Http3;
            }
            "--compressed" => {
                args.compressed = true;
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

/// Print usage information
pub fn print_usage() {
    eprintln!("nurl - A curl-like HTTP/HTTPS client for NexaOS");
    eprintln!();
    eprintln!("Usage: nurl [OPTIONS] <URL>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -X, --request METHOD   HTTP method to use (default: GET)");
    eprintln!("  -d, --data DATA        Data to send in POST request");
    eprintln!("  -H, --header HEADER    Add custom header (format: 'Key: Value')");
    eprintln!("  -o, --output FILE      Write output to file instead of stdout");
    eprintln!("  -i, --include          Include response headers in output");
    eprintln!("  -v, --verbose          Verbose output");
    eprintln!("  -k, --insecure         Allow insecure SSL connections");
    eprintln!("  --http1.1              Force HTTP/1.1");
    eprintln!("  --http2                Use HTTP/2 (when available)");
    eprintln!("  --http3                Use HTTP/3 (when available)");
    eprintln!("  --compressed           Request compressed response (gzip, deflate)");
    eprintln!("  --help                 Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  nurl http://10.0.2.2:8000");
    eprintln!("  nurl https://example.com");
    eprintln!("  nurl -k https://self-signed.example.com");
    eprintln!("  nurl -X POST -d 'hello=world' http://10.0.2.2:8000/api");
    eprintln!("  nurl -H 'Authorization: Bearer token' http://10.0.2.2:8000/");
    eprintln!("  nurl -v -i https://example.com");
    eprintln!("  nurl --http2 https://example.com");
}
