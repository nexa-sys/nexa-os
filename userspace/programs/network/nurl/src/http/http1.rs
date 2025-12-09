/// HTTP/1.1 client implementation

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::request::Http1RequestBuilder;
use super::response::parse_http1_response;
use super::{HttpClient, HttpError, HttpResponse, HttpResult};
use crate::args::Args;
use crate::url::ParsedUrl;

#[cfg(any(feature = "https", feature = "https-dynamic"))]
use crate::tls::TlsConnection;

/// HTTP/1.1 client
pub struct Http1Client {
    verbose: bool,
    insecure: bool,
}

impl Http1Client {
    /// Create a new HTTP/1.1 client
    pub fn new(verbose: bool, insecure: bool) -> HttpResult<Self> {
        Ok(Self { verbose, insecure })
    }

    /// Connect to the server
    fn connect(&self, url: &ParsedUrl) -> HttpResult<TcpStream> {
        let addr = url.addr();

        if self.verbose {
            eprintln!("* Connecting to {}...", addr);
        }

        let stream = TcpStream::connect(&addr)
            .map_err(|e| HttpError::ConnectionFailed(format!("{}: {}", addr, e)))?;

        // Set timeouts
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .ok();

        if self.verbose {
            eprintln!("* Connected!");
        }

        Ok(stream)
    }

    /// Perform plain HTTP request
    fn perform_http(&self, stream: TcpStream, request: &[u8]) -> HttpResult<Vec<u8>> {
        self.send_and_receive(stream, request)
    }

    /// Perform HTTPS request
    #[cfg(any(feature = "https", feature = "https-dynamic"))]
    fn perform_https(
        &self,
        stream: TcpStream,
        hostname: &str,
        request: &[u8],
    ) -> HttpResult<Vec<u8>> {
        use std::os::unix::io::AsRawFd;

        if self.verbose {
            eprintln!("* Performing TLS handshake...");
        }

        let mut tls = TlsConnection::new(stream.as_raw_fd(), hostname, self.insecure)?;

        if self.verbose {
            if let Some(version) = tls.version() {
                eprintln!("* TLS handshake complete: {}", version);
            }
            if let Some(cipher) = tls.cipher() {
                eprintln!("* Cipher: {}", cipher);
            }
        }

        // Send request
        tls.write_all(request)
            .map_err(|e| HttpError::SendFailed(e.to_string()))?;

        if self.verbose {
            eprintln!("* Request sent, waiting for response...");
        }

        // Read response
        let mut response_data = Vec::new();
        tls.read_to_end(&mut response_data)
            .map_err(|e| HttpError::ReceiveFailed(e.to_string()))?;

        // Keep the stream alive during TLS operations
        drop(stream);

        Ok(response_data)
    }

    /// Send request and receive response over a TCP stream
    fn send_and_receive(&self, mut stream: TcpStream, request: &[u8]) -> HttpResult<Vec<u8>> {
        // Send request
        stream
            .write_all(request)
            .map_err(|e| HttpError::SendFailed(e.to_string()))?;

        if self.verbose {
            eprintln!("* Request sent, waiting for response...");
        }

        // Read response
        let mut response_data = Vec::new();
        stream
            .read_to_end(&mut response_data)
            .map_err(|e| HttpError::ReceiveFailed(e.to_string()))?;

        Ok(response_data)
    }
}

impl HttpClient for Http1Client {
    fn request(&mut self, args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse> {
        // Build request
        let request = Http1RequestBuilder::from_args(args, url).build();

        if self.verbose {
            // Print first line of request
            if let Some(first_line) = std::str::from_utf8(&request)
                .ok()
                .and_then(|s| s.lines().next())
            {
                eprintln!("> {}", first_line);
            }
        }

        // Connect
        let stream = self.connect(url)?;

        // Perform request
        let response_data = if url.is_https {
            #[cfg(any(feature = "https", feature = "https-dynamic"))]
            {
                self.perform_https(stream, &url.host, &request)?
            }
            #[cfg(not(any(feature = "https", feature = "https-dynamic")))]
            {
                drop(stream);
                return Err(HttpError::NotSupported(
                    "HTTPS not supported (compile with 'https' or 'https-dynamic' feature)".to_string(),
                ));
            }
        } else {
            self.perform_http(stream, &request)?
        };

        if response_data.is_empty() {
            return Err(HttpError::ReceiveFailed("Empty response".to_string()));
        }

        if self.verbose {
            eprintln!("* Received {} bytes", response_data.len());
        }

        // Parse response
        let response = parse_http1_response(&response_data)?;

        if self.verbose {
            eprintln!("< {}", response.status_line());
        }

        Ok(response)
    }
}
