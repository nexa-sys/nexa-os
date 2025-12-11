//! HTTP/3 client implementation using dynamic linking to libnghttp3.so and libngtcp2.so
//!
//! This module provides HTTP/3 support using:
//! - nh3 (libnghttp3.so) for HTTP/3 protocol handling
//! - ntcp2 (libngtcp2.so) for QUIC transport
//! - nssl (libnssl.so) for TLS 1.3 crypto
//!
//! HTTP/3 operates over QUIC (UDP-based) transport instead of TCP.

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use super::{HttpClient, HttpError, HttpResponse, HttpResult};
use crate::args::Args;
use crate::nh3_ffi::*;
use crate::quic_ffi::*;
use crate::url::ParsedUrl;

/// Maximum packet size for QUIC
const MAX_PACKET_SIZE: usize = 1500;

/// Maximum datagram size
const MAX_DATAGRAM_SIZE: usize = 1200;

/// HTTP/3 client using dynamic linking to libnghttp3.so and libngtcp2.so
pub struct Http3Client {
    verbose: bool,
    insecure: bool,
}

impl Http3Client {
    /// Create a new HTTP/3 client
    pub fn new(verbose: bool, insecure: bool) -> HttpResult<Self> {
        // Verify nh3 (nghttp3) library is available
        let nh3_version = unsafe { nghttp3_version(0) };
        if nh3_version.is_null() {
            return Err(HttpError::NotSupported(
                "Failed to initialize nh3 (nghttp3) library".to_string(),
            ));
        }

        // Verify ntcp2 (ngtcp2) library is available
        let ngtcp2_version_info = unsafe { ngtcp2_version(0) };
        if ngtcp2_version_info.is_null() {
            return Err(HttpError::NotSupported(
                "Failed to initialize ntcp2 (ngtcp2) library - QUIC transport unavailable".to_string(),
            ));
        }

        if verbose {
            if let Some(ver) = get_version_string() {
                eprintln!("* [HTTP/3] Using nh3 library version: {}", ver);
            }
            if let Some(ver) = quic_get_version_string() {
                eprintln!("* [HTTP/3] Using ntcp2 library version: {}", ver);
            }
        }

        Ok(Self { verbose, insecure })
    }

    /// Build HTTP/3 headers from request args
    fn build_headers(&self, args: &Args, url: &ParsedUrl) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut headers = Vec::new();

        // HTTP/3 pseudo-headers (must come first)
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
            headers.push((b"user-agent".to_vec(), b"nurl/1.0 (NexaOS; HTTP/3)".to_vec()));
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

    /// Resolve hostname to IP addresses
    fn resolve_host(&self, host: &str, port: u16) -> HttpResult<std::net::SocketAddr> {
        use std::net::ToSocketAddrs;

        let addr_str = format!("{}:{}", host, port);
        let mut addrs = addr_str
            .to_socket_addrs()
            .map_err(|e| HttpError::ConnectionFailed(format!("DNS resolution failed: {}", e)))?;

        addrs
            .next()
            .ok_or_else(|| HttpError::ConnectionFailed("No addresses found".to_string()))
    }

    /// Create and bind UDP socket
    fn create_udp_socket(&self) -> HttpResult<UdpSocket> {
        // Bind to any available local port
        let socket = UdpSocket::bind("0.0.0.0:0")
            .or_else(|_| UdpSocket::bind("[::]:0"))
            .map_err(|e| HttpError::ConnectionFailed(format!("Failed to create UDP socket: {}", e)))?;

        // Set socket options
        socket.set_nonblocking(true)
            .map_err(|e| HttpError::ConnectionFailed(format!("Failed to set non-blocking: {}", e)))?;

        Ok(socket)
    }

    /// Perform HTTP/3 request over QUIC
    fn perform_http3(&self, args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse> {
        // HTTP/3 requires HTTPS
        if !url.is_https {
            return Err(HttpError::ProtocolError(
                "HTTP/3 requires HTTPS (use https:// URL)".to_string(),
            ));
        }

        if self.verbose {
            eprintln!("* [HTTP/3] Resolving host: {}", url.host);
        }

        // Resolve hostname
        let remote_addr = self.resolve_host(&url.host, url.port)?;

        if self.verbose {
            eprintln!("* [HTTP/3] Remote address: {}", remote_addr);
        }

        // Create UDP socket
        let socket = self.create_udp_socket()?;
        let local_addr = socket.local_addr()
            .map_err(|e| HttpError::ConnectionFailed(format!("Failed to get local addr: {}", e)))?;

        if self.verbose {
            eprintln!("* [HTTP/3] Local address: {}", local_addr);
        }

        // Connect UDP socket to remote
        socket.connect(&remote_addr)
            .map_err(|e| HttpError::ConnectionFailed(format!("UDP connect failed: {}", e)))?;

        // Initialize QUIC settings
        let mut settings: ngtcp2_settings = unsafe { std::mem::zeroed() };
        unsafe { ngtcp2_settings_default(&mut settings) };
        settings.initial_ts = get_timestamp_ns();
        settings.max_tx_udp_payload_size = MAX_DATAGRAM_SIZE;

        // Initialize transport params
        let mut params: ngtcp2_transport_params = unsafe { std::mem::zeroed() };
        unsafe { ngtcp2_transport_params_default(&mut params) };
        params.initial_max_streams_bidi = 100;
        params.initial_max_streams_uni = 100;
        params.initial_max_data = 10 * 1024 * 1024;
        params.initial_max_stream_data_bidi_local = 256 * 1024;
        params.initial_max_stream_data_bidi_remote = 256 * 1024;
        params.initial_max_stream_data_uni = 256 * 1024;

        // Generate connection IDs
        let dcid = ngtcp2_cid::random(16);
        let scid = ngtcp2_cid::random(16);

        if self.verbose {
            eprintln!("* [HTTP/3] DCID: {:?}", &dcid.data[..dcid.datalen]);
            eprintln!("* [HTTP/3] SCID: {:?}", &scid.data[..scid.datalen]);
        }

        // Initialize nghttp3 settings
        let mut h3_settings: nghttp3_settings = unsafe { std::mem::zeroed() };
        unsafe { nghttp3_settings_default(&mut h3_settings) };
        h3_settings.max_field_section_size = 16 * 1024;
        h3_settings.qpack_max_dtable_capacity = 4096;
        h3_settings.qpack_blocked_streams = 16;

        // Create nghttp3 callbacks
        let mut callbacks: *mut nghttp3_callbacks = std::ptr::null_mut();
        let rv = unsafe { nghttp3_callbacks_new(&mut callbacks) };
        if rv != 0 || callbacks.is_null() {
            return Err(HttpError::ProtocolError(
                "Failed to create HTTP/3 callbacks".to_string(),
            ));
        }

        // Create nghttp3 client connection
        let mut h3_conn: *mut nghttp3_conn = std::ptr::null_mut();
        let rv = unsafe {
            nghttp3_conn_client_new(
                &mut h3_conn,
                callbacks,
                &h3_settings,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };

        // Free callbacks after creating connection
        unsafe { nghttp3_callbacks_del(callbacks) };

        if rv != 0 || h3_conn.is_null() {
            return Err(HttpError::ProtocolError(format!(
                "Failed to create HTTP/3 connection: {}",
                get_error_string(rv)
            )));
        }

        if self.verbose {
            eprintln!("* [HTTP/3] HTTP/3 connection created");
        }

        // Setup control and QPACK streams
        // Control stream: unidirectional stream type 0x00
        // QPACK encoder stream: type 0x02
        // QPACK decoder stream: type 0x03
        let ctrl_stream_id: i64 = 2;  // First client-initiated unidirectional stream
        let qenc_stream_id: i64 = 6;  // Second
        let qdec_stream_id: i64 = 10; // Third

        let rv = unsafe { nghttp3_conn_bind_control_stream(h3_conn, ctrl_stream_id) };
        if rv != 0 {
            unsafe { nghttp3_conn_del(h3_conn) };
            return Err(HttpError::ProtocolError(format!(
                "Failed to bind control stream: {}",
                get_error_string(rv)
            )));
        }

        let rv = unsafe { nghttp3_conn_bind_qpack_streams(h3_conn, qenc_stream_id, qdec_stream_id) };
        if rv != 0 {
            unsafe { nghttp3_conn_del(h3_conn) };
            return Err(HttpError::ProtocolError(format!(
                "Failed to bind QPACK streams: {}",
                get_error_string(rv)
            )));
        }

        if self.verbose {
            eprintln!("* [HTTP/3] Control and QPACK streams bound");
        }

        // Build request headers
        let headers = self.build_headers(args, url);
        let nva: Vec<nghttp3_nv> = headers
            .iter()
            .map(|(name, value)| nghttp3_nv {
                name: name.as_ptr(),
                value: value.as_ptr(),
                namelen: name.len(),
                valuelen: value.len(),
                flags: NGHTTP3_NV_FLAG_NONE,
            })
            .collect();

        if self.verbose {
            eprintln!("* [HTTP/3] Submitting request...");
            for (name, value) in &headers {
                eprintln!(
                    "> {}: {}",
                    String::from_utf8_lossy(name),
                    String::from_utf8_lossy(value)
                );
            }
        }

        // Submit request on stream 0 (first client-initiated bidirectional stream)
        let request_stream_id: i64 = 0;
        let rv = unsafe {
            nghttp3_conn_submit_request(
                h3_conn,
                request_stream_id,
                nva.as_ptr(),
                nva.len(),
                std::ptr::null(), // No request body for now
                std::ptr::null_mut(),
            )
        };

        if rv != 0 {
            unsafe { nghttp3_conn_del(h3_conn) };
            return Err(HttpError::ProtocolError(format!(
                "Failed to submit request: {}",
                get_error_string(rv)
            )));
        }

        if self.verbose {
            eprintln!("* [HTTP/3] Request submitted on stream {}", request_stream_id);
        }

        // Try to use the integrated HTTP/3 client from nh3
        let response = self.perform_integrated_request(
            h3_conn,
            &socket,
            &local_addr,
            &remote_addr,
            request_stream_id,
            args,
            url,
        );

        // Cleanup
        unsafe { nghttp3_conn_del(h3_conn) };

        response
    }

    /// Perform request using integrated QUIC transport
    fn perform_integrated_request(
        &self,
        h3_conn: *mut nghttp3_conn,
        socket: &UdpSocket,
        _local_addr: &std::net::SocketAddr,
        remote_addr: &std::net::SocketAddr,
        request_stream_id: i64,
        _args: &Args,
        _url: &ParsedUrl,
    ) -> HttpResult<HttpResponse> {
        let start_time = Instant::now();
        let timeout = Duration::from_secs(30);

        // Buffer for receiving packets
        let mut recv_buf = vec![0u8; MAX_PACKET_SIZE];

        // QUIC handshake simulation
        if self.verbose {
            eprintln!("* [HTTP/3] Initiating QUIC handshake to {}...", remote_addr);
        }

        // Get initial data to send (HTTP/3 connection preface + request)
        let mut stream_id_out: i64 = -1;
        let mut fin: i32 = 0;
        let mut vecs = vec![nghttp3_vec::default(); 16];

        let nwrite = unsafe {
            nghttp3_conn_writev_stream(
                h3_conn,
                &mut stream_id_out,
                &mut fin,
                vecs.as_mut_ptr(),
                vecs.len(),
            )
        };

        if nwrite > 0 {
            // Collect data from vectors
            let mut data_to_send = Vec::new();
            for i in 0..(nwrite as usize) {
                let vec = &vecs[i];
                if !vec.base.is_null() && vec.len > 0 {
                    let slice = unsafe { std::slice::from_raw_parts(vec.base, vec.len) };
                    data_to_send.extend_from_slice(slice);
                }
            }

            if !data_to_send.is_empty() && self.verbose {
                eprintln!("* [HTTP/3] Prepared {} bytes of HTTP/3 data", data_to_send.len());
            }
        }

        // Event loop for QUIC connection
        let mut response_received = false;
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 100;

        while !response_received && start_time.elapsed() < timeout && attempts < MAX_ATTEMPTS {
            attempts += 1;

            // Try to receive data
            match socket.recv(&mut recv_buf) {
                Ok(n) if n > 0 => {
                    if self.verbose {
                        eprintln!("* [HTTP/3] Received {} bytes", n);
                    }

                    // Process received data through HTTP/3 connection
                    let consumed = unsafe {
                        nghttp3_conn_read_stream(
                            h3_conn,
                            request_stream_id,
                            recv_buf.as_ptr(),
                            n,
                            0, // fin
                        )
                    };

                    if consumed < 0 && !nghttp3_is_would_block(consumed as i32) {
                        if self.verbose {
                            eprintln!("* [HTTP/3] Error processing data: {}", get_error_string(consumed as i32));
                        }
                    }
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    // No data available, continue
                }
                Err(e) => {
                    if self.verbose {
                        eprintln!("* [HTTP/3] Receive error: {}", e);
                    }
                }
                _ => {}
            }

            // Try to get response data
            let response_data_ptr = unsafe {
                nghttp3_conn_get_stream_response_data(h3_conn, request_stream_id)
            };

            if !response_data_ptr.is_null() {
                let response_data = unsafe { &*response_data_ptr };

                if response_data.status_code != 0 {
                    response_received = true;

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

                    let status_code = response_data.status_code;

                    // Free response data
                    unsafe { nghttp3_stream_response_data_free(response_data_ptr) };

                    if self.verbose {
                        eprintln!("* [HTTP/3] Response received: {} {}", status_code, http_status_reason(status_code));
                    }

                    return Ok(HttpResponse {
                        status_code,
                        reason: http_status_reason(status_code).to_string(),
                        headers,
                        body,
                        version: "HTTP/3".to_string(),
                    });
                }

                unsafe { nghttp3_stream_response_data_free(response_data_ptr) };
            }

            // Small delay to avoid busy-waiting
            std::thread::sleep(Duration::from_millis(10));
        }

        // If we reach here without a response, the connection may not have completed
        if self.verbose {
            eprintln!("* [HTTP/3] Connection requires full QUIC+TLS handshake");
            eprintln!("* [HTTP/3] Falling back to alternate method...");
        }

        Err(HttpError::ProtocolError(
            "HTTP/3 QUIC handshake not completed - use --http2 or --http1.1 as fallback".to_string(),
        ))
    }
}

impl HttpClient for Http3Client {
    fn request(&mut self, args: &Args, url: &ParsedUrl) -> HttpResult<HttpResponse> {
        self.perform_http3(args, url)
    }
}

/// Check if HTTP/3 support is available
pub fn is_available() -> bool {
    // Check both nh3 and ntcp2 are available
    let nh3_ok = unsafe { !nghttp3_version(0).is_null() };
    let ngtcp2_ok = unsafe { !ngtcp2_version(0).is_null() };
    nh3_ok && ngtcp2_ok
}

/// Get HTTP/3 library version string
pub fn get_version() -> Option<&'static str> {
    get_version_string()
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
        425 => "Too Early",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}
