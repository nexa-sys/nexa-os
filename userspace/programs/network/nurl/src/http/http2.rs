/// HTTP/2 client implementation using nh2 library
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::{HttpClient, HttpError, HttpResponse, HttpResult};
use crate::args::Args;
use crate::url::ParsedUrl;

#[cfg(any(feature = "https", feature = "https-dynamic"))]
use crate::tls::TlsConnection;

use nh2::{HeaderField, Session, SessionCallbacks, StreamState};

/// HTTP/2 client
pub struct Http2Client {
    verbose: bool,
    insecure: bool,
}

impl Http2Client {
    /// Create a new HTTP/2 client
    pub fn new(verbose: bool, insecure: bool) -> HttpResult<Self> {
        Ok(Self { verbose, insecure })
    }

    /// Connect to the server
    fn connect(&self, url: &ParsedUrl) -> HttpResult<TcpStream> {
        let addr = url.addr();

        if self.verbose {
            eprintln!("* [HTTP/2] Connecting to {}...", addr);
        }

        let stream = TcpStream::connect(&addr)
            .map_err(|e| HttpError::ConnectionFailed(format!("{}: {}", addr, e)))?;

        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(30))).ok();

        if self.verbose {
            eprintln!("* [HTTP/2] Connected!");
        }

        Ok(stream)
    }

    /// Build HTTP/2 headers from request args
    fn build_headers(&self, args: &Args, url: &ParsedUrl) -> Vec<HeaderField> {
        let mut headers = Vec::new();

        // Pseudo-headers (must come first in HTTP/2)
        headers.push(HeaderField::new(":method", args.method.as_str()));
        headers.push(HeaderField::new(":scheme", url.scheme()));
        headers.push(HeaderField::new(":path", url.path_with_query()));
        headers.push(HeaderField::new(":authority", url.host_with_port()));

        // Add user-specified headers
        for (name, value) in &args.headers {
            if !name.starts_with(':') {
                headers.push(HeaderField::new(name.to_lowercase(), value.as_str()));
            }
        }

        // Default user-agent
        let has_user_agent = args
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
        if !has_user_agent {
            headers.push(HeaderField::new("user-agent", "nurl/1.0 (NexaOS; HTTP/2)"));
        }

        // Request compression
        if args.compressed {
            let has_accept_encoding = args
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"));
            if !has_accept_encoding {
                headers.push(HeaderField::new("accept-encoding", "gzip, deflate"));
            }
        }

        headers
    }

    /// Perform HTTP/2 exchange over a generic Read+Write stream
    fn perform_http2<S: Read + Write>(
        &self,
        stream: &mut S,
        args: &Args,
        url: &ParsedUrl,
    ) -> HttpResult<HttpResponse> {
        // Create client session
        let session = Session::client(SessionCallbacks::new(), std::ptr::null_mut());

        // Build request headers
        let headers = self.build_headers(args, url);
        let has_body = args.data.is_some();

        if self.verbose {
            eprintln!("* [HTTP/2] Submitting request...");
            for h in &headers {
                eprintln!(
                    "> {}: {}",
                    String::from_utf8_lossy(&h.name),
                    String::from_utf8_lossy(&h.value)
                );
            }
        }

        // Submit request (without data provider for GET, with for POST etc)
        let stream_id = session
            .submit_request(None, &headers, None)
            .map_err(|e| HttpError::ProtocolError(format!("Failed to submit request: {:?}", e)))?;

        // Send request body if present
        if let Some(ref data) = args.data {
            session
                .submit_data(stream_id, data, true)
                .map_err(|e| HttpError::ProtocolError(format!("Failed to send data: {:?}", e)))?;
        }

        // Send connection preface and initial frames
        let send_data = session.mem_send();
        stream
            .write_all(&send_data)
            .map_err(|e| HttpError::SendFailed(e.to_string()))?;

        if self.verbose {
            eprintln!(
                "* [HTTP/2] Sent {} bytes (preface + request)",
                send_data.len()
            );
        }

        // Read and process response
        let mut recv_buffer = vec![0u8; 16384];
        let mut response_body = Vec::new();
        let mut response_headers: Vec<(String, String)> = Vec::new();
        let mut status_code: Option<u16> = None;

        loop {
            // Read from transport
            let n = stream
                .read(&mut recv_buffer)
                .map_err(|e| HttpError::ReceiveFailed(e.to_string()))?;

            if n == 0 {
                if self.verbose {
                    eprintln!("* [HTTP/2] Connection closed by server");
                }
                break;
            }

            if self.verbose {
                eprintln!("* [HTTP/2] Received {} bytes", n);
            }

            // Feed data to session
            let _ = session.mem_recv(&recv_buffer[..n]).map_err(|e| {
                HttpError::ProtocolError(format!("Failed to process data: {:?}", e))
            })?;

            // Get stream to check for headers and data
            if let Some(stream_data) = session.get_stream_data(stream_id) {
                // Extract response headers if not already done
                if status_code.is_none() && !stream_data.response_headers.is_empty() {
                    for h in &stream_data.response_headers {
                        let name = String::from_utf8_lossy(&h.name);
                        let value = String::from_utf8_lossy(&h.value);

                        if name == ":status" {
                            status_code = value.parse().ok();
                            if self.verbose {
                                eprintln!("< :status: {}", value);
                            }
                        } else if !name.starts_with(':') {
                            if self.verbose {
                                eprintln!("< {}: {}", name, value);
                            }
                            response_headers.push((name.into_owned(), value.into_owned()));
                        }
                    }
                }

                // Get received body data
                if !stream_data.recv_buffer.is_empty() {
                    if self.verbose {
                        eprintln!(
                            "* [HTTP/2] Got {} bytes of body data",
                            stream_data.recv_buffer.len()
                        );
                    }
                    response_body.extend_from_slice(&stream_data.recv_buffer);
                }
            }

            // Check if stream is complete
            let stream_state = session.get_stream(stream_id);
            if matches!(
                stream_state,
                Some(StreamState::Closed) | Some(StreamState::HalfClosedRemote)
            ) {
                if self.verbose {
                    eprintln!("* [HTTP/2] Stream {} complete", stream_id);
                }
                break;
            }

            // Send any pending frames (WINDOW_UPDATE, etc.)
            let pending = session.mem_send();
            if !pending.is_empty() {
                stream
                    .write_all(&pending)
                    .map_err(|e| HttpError::SendFailed(e.to_string()))?;
            }
        }

        // Final data collection
        if let Some(stream_data) = session.get_stream_data(stream_id) {
            if status_code.is_none() {
                for h in &stream_data.response_headers {
                    let name = String::from_utf8_lossy(&h.name);
                    let value = String::from_utf8_lossy(&h.value);

                    if name == ":status" {
                        status_code = value.parse().ok();
                    } else if !name.starts_with(':') {
                        response_headers.push((name.into_owned(), value.into_owned()));
                    }
                }
            }
            if stream_data.recv_buffer.len() > response_body.len() {
                response_body = stream_data.recv_buffer.clone();
            }
        }

        let status = status_code
            .ok_or_else(|| HttpError::InvalidResponse("No :status header received".to_string()))?;

        Ok(HttpResponse {
            status_code: status,
            reason: http_status_reason(status).to_string(),
            headers: response_headers,
            body: response_body,
            version: "HTTP/2".to_string(),
        })
    }

    /// Perform HTTP/2 over plain TCP (h2c)
    fn perform_h2c(
        &self,
        mut stream: TcpStream,
        args: &Args,
        url: &ParsedUrl,
    ) -> HttpResult<HttpResponse> {
        if self.verbose {
            eprintln!("* [HTTP/2] Using h2c (HTTP/2 cleartext)");
        }
        self.perform_http2(&mut stream, args, url)
    }

    /// Perform HTTP/2 over TLS (h2)
    #[cfg(any(feature = "https", feature = "https-dynamic"))]
    fn perform_h2(
        &self,
        tcp_stream: TcpStream,
        hostname: &str,
        args: &Args,
        url: &ParsedUrl,
    ) -> HttpResult<HttpResponse> {
        use std::os::unix::io::AsRawFd;

        if self.verbose {
            eprintln!("* [HTTP/2] Performing TLS handshake with ALPN...");
        }

        let mut tls = TlsConnection::new_with_alpn(
            tcp_stream.as_raw_fd(),
            hostname,
            self.insecure,
            &["h2", "http/1.1"],
        )?;

        // Check negotiated protocol
        let negotiated = tls.alpn_protocol().unwrap_or("http/1.1");
        if self.verbose {
            eprintln!("* [HTTP/2] ALPN negotiated: {}", negotiated);
        }

        if negotiated != "h2" {
            return Err(HttpError::ProtocolError(
                "Server does not support HTTP/2".to_string(),
            ));
        }

        if self.verbose {
            if let Some(version) = tls.version() {
                eprintln!("* [HTTP/2] TLS version: {}", version);
            }
            if let Some(cipher) = tls.cipher() {
                eprintln!("* [HTTP/2] Cipher: {}", cipher);
            }
        }

        self.perform_http2(&mut tls, args, url)
    }
}

impl HttpClient for Http2Client {
    fn request(&mut self, args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse> {
        let stream = self.connect(url)?;

        if url.is_https {
            #[cfg(any(feature = "https", feature = "https-dynamic"))]
            {
                self.perform_h2(stream, &url.host, args, url)
            }
            #[cfg(not(any(feature = "https", feature = "https-dynamic")))]
            {
                drop(stream);
                Err(HttpError::NotSupported(
                    "HTTPS not supported (compile with 'https' or 'https-dynamic' feature)"
                        .to_string(),
                ))
            }
        } else {
            self.perform_h2c(stream, args, url)
        }
    }
}

/// Get HTTP status reason phrase
fn http_status_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}
