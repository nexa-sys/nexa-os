//! Async I/O backend using Tokio
//!
//! This module provides async support for HTTP/2 sessions using the Tokio runtime.

use crate::session::{Session, SessionBuilder, SessionCallbacks, SessionType};
use crate::hpack::HeaderField;
use crate::types::{StreamId, PrioritySpec, DataProvider};
use crate::error::{Error, Result, ErrorCode};
use crate::stream::StreamState;

use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};
use std::sync::Arc;
use std::pin::Pin;
use std::task::{Context, Poll};

// ============================================================================
// Async Session Wrapper
// ============================================================================

/// Async HTTP/2 session
pub struct AsyncSession {
    inner: Arc<Session>,
}

impl AsyncSession {
    /// Create a new async client session
    pub fn client() -> Self {
        Self {
            inner: Arc::new(Session::client(SessionCallbacks::new(), core::ptr::null_mut())),
        }
    }

    /// Create a new async server session
    pub fn server() -> Self {
        Self {
            inner: Arc::new(Session::server(SessionCallbacks::new(), core::ptr::null_mut())),
        }
    }

    /// Get a reference to the inner session
    pub fn inner(&self) -> &Session {
        &self.inner
    }

    /// Submit a request asynchronously
    pub fn submit_request(
        &self,
        headers: &[HeaderField],
    ) -> Result<StreamId> {
        self.inner.submit_request(None, headers, None)
    }

    /// Submit a request with priority
    pub fn submit_request_with_priority(
        &self,
        priority: &PrioritySpec,
        headers: &[HeaderField],
    ) -> Result<StreamId> {
        self.inner.submit_request(Some(priority), headers, None)
    }

    /// Submit a response
    pub fn submit_response(
        &self,
        stream_id: StreamId,
        headers: &[HeaderField],
    ) -> Result<()> {
        self.inner.submit_response(stream_id, headers, None)
    }

    /// Submit data
    pub fn submit_data(
        &self,
        stream_id: StreamId,
        data: &[u8],
        end_stream: bool,
    ) -> Result<()> {
        self.inner.submit_data(stream_id, data, end_stream)
    }

    /// Get pending data to send
    pub fn get_send_data(&self) -> Vec<u8> {
        self.inner.mem_send()
    }

    /// Process received data
    pub fn process_recv_data(&self, data: &[u8]) -> Result<usize> {
        self.inner.mem_recv(data)
    }

    /// Check if session wants to read
    pub fn want_read(&self) -> bool {
        self.inner.want_read()
    }

    /// Check if session wants to write
    pub fn want_write(&self) -> bool {
        self.inner.want_write()
    }

    /// Get stream state
    pub fn get_stream_state(&self, stream_id: StreamId) -> Option<StreamState> {
        self.inner.get_stream(stream_id)
    }
}

// ============================================================================
// Connection
// ============================================================================

/// HTTP/2 connection over an async transport
pub struct Connection<T> {
    session: AsyncSession,
    transport: T,
    read_buf: Vec<u8>,
}

impl<T> Connection<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Create a new client connection
    pub fn client(transport: T) -> Self {
        Self {
            session: AsyncSession::client(),
            transport,
            read_buf: vec![0u8; 65536],
        }
    }

    /// Create a new server connection
    pub fn server(transport: T) -> Self {
        Self {
            session: AsyncSession::server(),
            transport,
            read_buf: vec![0u8; 65536],
        }
    }

    /// Get a reference to the session
    pub fn session(&self) -> &AsyncSession {
        &self.session
    }

    /// Perform the HTTP/2 handshake
    pub async fn handshake(&mut self) -> Result<()> {
        // Send initial data (preface + settings for client)
        self.flush().await?;

        // Receive server settings
        self.recv().await?;

        Ok(())
    }

    /// Send pending data
    pub async fn flush(&mut self) -> Result<()> {
        let data = self.session.get_send_data();
        if !data.is_empty() {
            self.transport.write_all(&data).await
                .map_err(|_| Error::Internal("write error"))?;
        }
        Ok(())
    }

    /// Receive and process data
    pub async fn recv(&mut self) -> Result<usize> {
        let n = self.transport.read(&mut self.read_buf).await
            .map_err(|_| Error::Internal("read error"))?;

        if n == 0 {
            return Err(Error::ConnectionClosed);
        }

        self.session.process_recv_data(&self.read_buf[..n])
    }

    /// Run the connection event loop
    pub async fn run(&mut self) -> Result<()> {
        loop {
            // Send any pending data
            if self.session.want_write() {
                self.flush().await?;
            }

            // Receive data if needed
            if self.session.want_read() {
                match self.recv().await {
                    Ok(_) => {}
                    Err(Error::ConnectionClosed) => break,
                    Err(e) => return Err(e),
                }
            }

            // If neither read nor write is needed, we're done
            if !self.session.want_read() && !self.session.want_write() {
                break;
            }
        }

        Ok(())
    }

    /// Submit a request and get the stream ID
    pub fn submit_request(&self, headers: &[HeaderField]) -> Result<StreamId> {
        self.session.submit_request(headers)
    }

    /// Submit a response
    pub fn submit_response(&self, stream_id: StreamId, headers: &[HeaderField]) -> Result<()> {
        self.session.submit_response(stream_id, headers)
    }

    /// Submit data on a stream
    pub fn submit_data(&self, stream_id: StreamId, data: &[u8], end_stream: bool) -> Result<()> {
        self.session.submit_data(stream_id, data, end_stream)
    }

    /// Get the underlying transport
    pub fn into_transport(self) -> T {
        self.transport
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// HTTP/2 request
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub scheme: String,
    pub authority: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl Request {
    /// Create a new GET request
    pub fn get(uri: &str) -> Self {
        Self {
            method: "GET".to_string(),
            scheme: "https".to_string(),
            authority: String::new(),
            path: uri.to_string(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Create a new POST request
    pub fn post(uri: &str) -> Self {
        Self {
            method: "POST".to_string(),
            scheme: "https".to_string(),
            authority: String::new(),
            path: uri.to_string(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Set the authority (host)
    pub fn authority(mut self, authority: &str) -> Self {
        self.authority = authority.to_string();
        self
    }

    /// Add a header
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Set the body
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Convert to header fields
    pub fn to_header_fields(&self) -> Vec<HeaderField> {
        let mut fields = vec![
            HeaderField::new(b":method".to_vec(), self.method.as_bytes().to_vec()),
            HeaderField::new(b":scheme".to_vec(), self.scheme.as_bytes().to_vec()),
            HeaderField::new(b":authority".to_vec(), self.authority.as_bytes().to_vec()),
            HeaderField::new(b":path".to_vec(), self.path.as_bytes().to_vec()),
        ];

        for (name, value) in &self.headers {
            fields.push(HeaderField::new(
                name.to_lowercase().as_bytes().to_vec(),
                value.as_bytes().to_vec(),
            ));
        }

        fields
    }
}

/// HTTP/2 response
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// Create a new response
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Add a header
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Set the body
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Convert to header fields
    pub fn to_header_fields(&self) -> Vec<HeaderField> {
        let mut fields = vec![
            HeaderField::new(b":status".to_vec(), self.status.to_string().as_bytes().to_vec()),
        ];

        for (name, value) in &self.headers {
            fields.push(HeaderField::new(
                name.to_lowercase().as_bytes().to_vec(),
                value.as_bytes().to_vec(),
            ));
        }

        fields
    }
}

// ============================================================================
// Client
// ============================================================================

/// High-level HTTP/2 client
pub struct Client<T> {
    connection: Connection<T>,
}

impl<T> Client<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Create a new client
    pub async fn new(transport: T) -> Result<Self> {
        let mut connection = Connection::client(transport);
        connection.handshake().await?;
        Ok(Self { connection })
    }

    /// Send a request
    pub async fn send(&mut self, request: Request) -> Result<Response> {
        let headers = request.to_header_fields();
        let stream_id = self.connection.submit_request(&headers)?;

        // Send body if present
        if let Some(body) = request.body {
            self.connection.submit_data(stream_id, &body, true)?;
        }

        // Flush and receive response
        self.connection.flush().await?;
        
        // Wait for response
        loop {
            self.connection.recv().await?;
            
            if let Some(state) = self.connection.session().get_stream_state(stream_id) {
                if state == StreamState::Closed || state == StreamState::HalfClosedRemote {
                    break;
                }
            }
        }

        // TODO: Extract response from stream
        Ok(Response::new(200))
    }

    /// Close the client
    pub async fn close(mut self) -> Result<()> {
        self.connection.session.inner.terminate(ErrorCode::NoError)?;
        self.connection.flush().await?;
        Ok(())
    }
}

// ============================================================================
// Server
// ============================================================================

/// Handler trait for HTTP/2 server
#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, request: Request) -> Response;
}

/// Simple handler function wrapper
pub struct FnHandler<F>(pub F);

#[async_trait::async_trait]
impl<F, Fut> Handler for FnHandler<F>
where
    F: Fn(Request) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Response> + Send,
{
    async fn handle(&self, request: Request) -> Response {
        (self.0)(request).await
    }
}

/// High-level HTTP/2 server connection
pub struct ServerConnection<T, H> {
    connection: Connection<T>,
    handler: Arc<H>,
}

impl<T, H> ServerConnection<T, H>
where
    T: AsyncRead + AsyncWrite + Unpin,
    H: Handler,
{
    /// Create a new server connection
    pub async fn new(transport: T, handler: Arc<H>) -> Result<Self> {
        let mut connection = Connection::server(transport);
        connection.handshake().await?;
        Ok(Self { connection, handler })
    }

    /// Run the server connection
    pub async fn run(&mut self) -> Result<()> {
        self.connection.run().await
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder() {
        let req = Request::get("/api/v1/users")
            .authority("example.com")
            .header("accept", "application/json");

        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/api/v1/users");
        assert_eq!(req.authority, "example.com");
    }

    #[test]
    fn test_response_builder() {
        let resp = Response::new(200)
            .header("content-type", "application/json")
            .body(b"{}".to_vec());

        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"{}");
    }
}
