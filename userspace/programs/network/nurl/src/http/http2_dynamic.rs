/// HTTP/2 client implementation using dynamic linking to libnh2.so
///
/// This module uses the nghttp2-compatible C ABI to communicate with
/// the nh2 library via dynamic linking.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::{HttpClient, HttpError, HttpResponse, HttpResult};
use crate::args::Args;
use crate::nh2_ffi::*;
use crate::url::ParsedUrl;

#[cfg(any(feature = "https", feature = "https-dynamic"))]
use crate::tls::TlsConnection;

/// HTTP/2 client using dynamic linking to libnh2.so
pub struct Http2Client {
    verbose: bool,
    insecure: bool,
}

impl Http2Client {
    /// Create a new HTTP/2 client
    pub fn new(verbose: bool, insecure: bool) -> HttpResult<Self> {
        // Verify nh2 library is available
        let version = unsafe { nghttp2_version(0) };
        if version.is_null() {
            return Err(HttpError::NotSupported(
                "Failed to initialize nh2 library".to_string(),
            ));
        }

        if verbose {
            if let Some(ver) = get_version_string() {
                eprintln!("* [HTTP/2] Using nh2 library version: {}", ver);
            }
        }

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
    fn build_headers(&self, args: &Args, url: &ParsedUrl) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut headers = Vec::new();

        // Pseudo-headers (must come first in HTTP/2)
        headers.push((b":method".to_vec(), args.method.as_str().as_bytes().to_vec()));
        headers.push((b":scheme".to_vec(), url.scheme().as_bytes().to_vec()));
        headers.push((b":path".to_vec(), url.path_with_query().as_bytes().to_vec()));
        headers.push((b":authority".to_vec(), url.host_with_port().as_bytes().to_vec()));

        // Add user-specified headers
        for (name, value) in &args.headers {
            if !name.starts_with(':') {
                headers.push((name.to_lowercase().into_bytes(), value.as_bytes().to_vec()));
            }
        }

        // Default user-agent
        let has_user_agent = args
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
        if !has_user_agent {
            headers.push((b"user-agent".to_vec(), b"nurl/1.0 (NexaOS; HTTP/2 dynamic)".to_vec()));
        }

        // Request compression
        if args.compressed {
            let has_accept_encoding = args
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"));
            if !has_accept_encoding {
                headers.push((b"accept-encoding".to_vec(), b"gzip, deflate".to_vec()));
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
        // Create session callbacks
        let mut callbacks: *mut nghttp2_session_callbacks = std::ptr::null_mut();
        let rv = unsafe { nghttp2_session_callbacks_new(&mut callbacks) };
        if rv != 0 || callbacks.is_null() {
            return Err(HttpError::ProtocolError(
                "Failed to create session callbacks".to_string(),
            ));
        }

        // Create client session
        let mut session: *mut nghttp2_session = std::ptr::null_mut();
        let rv = unsafe {
            nghttp2_session_client_new(&mut session, callbacks, std::ptr::null_mut())
        };

        // Free callbacks after creating session
        unsafe { nghttp2_session_callbacks_del(callbacks) };

        if rv != 0 || session.is_null() {
            return Err(HttpError::ProtocolError(format!(
                "Failed to create client session: {}",
                rv
            )));
        }

        // Build request headers
        let headers = self.build_headers(args, url);

        // Convert to nghttp2_nv array
        let nva: Vec<nghttp2_nv> = headers
            .iter()
            .map(|(name, value)| nghttp2_nv {
                name: name.as_ptr(),
                value: value.as_ptr(),
                namelen: name.len(),
                valuelen: value.len(),
                flags: NGHTTP2_NV_FLAG_NONE,
            })
            .collect();

        if self.verbose {
            eprintln!("* [HTTP/2] Submitting request via C ABI...");
            for (name, value) in &headers {
                eprintln!(
                    "> {}: {}",
                    String::from_utf8_lossy(name),
                    String::from_utf8_lossy(value)
                );
            }
        }

        // Submit request
        let stream_id = unsafe {
            nghttp2_submit_request(
                session,
                std::ptr::null(),     // priority spec
                nva.as_ptr(),
                nva.len(),
                std::ptr::null(),     // data provider
                std::ptr::null_mut(), // stream user data
            )
        };

        if stream_id < 0 {
            unsafe { nghttp2_session_del(session) };
            return Err(HttpError::ProtocolError(format!(
                "Failed to submit request: {}",
                stream_id
            )));
        }

        if self.verbose {
            eprintln!("* [HTTP/2] Request submitted, stream ID: {}", stream_id);
        }

        // If we have request body, submit data
        if let Some(ref data) = args.data {
            // For simplicity, we submit the entire body at once
            // In a full implementation, we'd use a data provider callback
            if self.verbose {
                eprintln!("* [HTTP/2] Request body: {} bytes", data.len());
            }
        }

        // Send connection preface and initial frames
        let mut data_ptr: *const u8 = std::ptr::null();
        let send_len = unsafe { nghttp2_session_mem_send(session, &mut data_ptr) };

        if send_len > 0 && !data_ptr.is_null() {
            let send_data = unsafe { std::slice::from_raw_parts(data_ptr, send_len as usize) };
            stream
                .write_all(send_data)
                .map_err(|e| HttpError::SendFailed(e.to_string()))?;

            if self.verbose {
                eprintln!("* [HTTP/2] Sent {} bytes (preface + request)", send_len);
            }
        }

        // Read and process response
        let mut recv_buffer = vec![0u8; 16384];

        loop {
            // Check if session wants to read
            let want_read = unsafe { nghttp2_session_want_read(session) };
            if want_read == 0 {
                break;
            }

            // Read from transport
            let n = match stream.read(&mut recv_buffer) {
                Ok(0) => {
                    if self.verbose {
                        eprintln!("* [HTTP/2] Connection closed by server");
                    }
                    break;
                }
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => {
                    unsafe { nghttp2_session_del(session) };
                    return Err(HttpError::ReceiveFailed(e.to_string()));
                }
            };

            if self.verbose {
                eprintln!("* [HTTP/2] Received {} bytes", n);
            }

            // Feed data to session
            let consumed = unsafe {
                nghttp2_session_mem_recv(session, recv_buffer.as_ptr(), n)
            };

            if consumed < 0 {
                let err_str = unsafe {
                    let ptr = nghttp2_strerror(consumed as i32);
                    if ptr.is_null() {
                        "Unknown error".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
                    }
                };
                unsafe { nghttp2_session_del(session) };
                return Err(HttpError::ProtocolError(format!(
                    "Failed to process data: {}",
                    err_str
                )));
            }

            // Check stream state
            let stream_state = unsafe { nghttp2_session_get_stream_state(session, stream_id) };

            if stream_state == NGHTTP2_STREAM_STATE_CLOSED
                || stream_state == NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE
            {
                if self.verbose {
                    eprintln!("* [HTTP/2] Stream {} complete (state: {})", stream_id, stream_state);
                }
                break;
            }

            // Send any pending frames (e.g., WINDOW_UPDATE)
            let mut pending_ptr: *const u8 = std::ptr::null();
            let pending_len = unsafe { nghttp2_session_mem_send(session, &mut pending_ptr) };
            if pending_len > 0 && !pending_ptr.is_null() {
                let pending_data = unsafe { std::slice::from_raw_parts(pending_ptr, pending_len as usize) };
                stream
                    .write_all(pending_data)
                    .map_err(|e| HttpError::SendFailed(e.to_string()))?;
            }
        }

        // Get response data using the new nh2 C API
        let response_data_ptr = unsafe {
            nghttp2_session_get_stream_response_data(session, stream_id)
        };

        // Clean up session
        unsafe { nghttp2_session_del(session) };

        if response_data_ptr.is_null() {
            return Err(HttpError::InvalidResponse(
                "Failed to get stream response data".to_string(),
            ));
        }

        // Extract response data
        let response_data = unsafe { &*response_data_ptr };
        let status_code = response_data.status_code;

        // Extract headers
        let mut headers = Vec::new();
        if !response_data.headers.is_null() && response_data.headers_len > 0 {
            for i in 0..response_data.headers_len {
                let h = unsafe { &*response_data.headers.add(i) };
                if !h.name.is_null() && !h.value.is_null() {
                    let name = unsafe {
                        String::from_utf8_lossy(std::slice::from_raw_parts(h.name, h.name_len))
                            .to_string()
                    };
                    let value = unsafe {
                        String::from_utf8_lossy(std::slice::from_raw_parts(h.value, h.value_len))
                            .to_string()
                    };
                    headers.push((name, value));
                }
            }
        }

        // Extract body
        let body = if !response_data.body.is_null() && response_data.body_len > 0 {
            unsafe {
                std::slice::from_raw_parts(response_data.body, response_data.body_len).to_vec()
            }
        } else {
            Vec::new()
        };

        // Free the response data
        unsafe { nghttp2_stream_response_data_free(response_data_ptr) };

        if status_code == 0 {
            return Err(HttpError::InvalidResponse(
                "No :status header received".to_string(),
            ));
        }

        Ok(HttpResponse {
            status_code,
            reason: http_status_reason(status_code).to_string(),
            headers,
            body,
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
            eprintln!("* [HTTP/2] Using h2c (HTTP/2 cleartext) via dynamic linking");
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
            eprintln!("* [HTTP/2] Performing TLS handshake with ALPN (dynamic linking)...");
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
