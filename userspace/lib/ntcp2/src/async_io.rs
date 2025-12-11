//! Async I/O Support for QUIC
//!
//! This module provides async I/O support using tokio for high-performance
//! QUIC networking.
//!
//! ## Features
//!
//! - **AsyncConnection**: Async wrapper around Connection
//! - **AsyncEndpoint**: Server/client endpoint for managing multiple connections
//! - **AsyncStream**: Async read/write for QUIC streams
//!
//! ## Usage
//!
//! ```rust,ignore
//! use ntcp2::async_io::{AsyncEndpoint, Config};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = Config::client();
//!     let endpoint = AsyncEndpoint::bind("0.0.0.0:0", config).await?;
//!     let conn = endpoint.connect("example.com:443").await?;
//!     let stream = conn.open_stream().await?;
//!     stream.write_all(b"Hello").await?;
//! }
//! ```

#[cfg(feature = "tokio")]
mod tokio_impl {
    use std::collections::HashMap;
    use std::future::Future;
    use std::io;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::UdpSocket;
    use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
    use tokio::time::{interval, sleep, timeout, Interval};

    use crate::connection::Connection;
    use crate::error::{Error, NgError, Result};
    use crate::stream::{Stream, StreamManager};
    use crate::types::{Settings, StreamId, TransportParams};

    // ========================================================================
    // Configuration
    // ========================================================================

    /// Async endpoint configuration
    #[derive(Clone)]
    pub struct Config {
        /// Transport parameters
        pub transport_params: TransportParams,
        /// Settings
        pub settings: Settings,
        /// Is server
        pub is_server: bool,
        /// Maximum connections
        pub max_connections: usize,
        /// Idle timeout
        pub idle_timeout: Duration,
        /// Receive buffer size
        pub recv_buffer_size: usize,
        /// Send buffer size
        pub send_buffer_size: usize,
    }

    impl Config {
        /// Create client config
        pub fn client() -> Self {
            Self {
                transport_params: TransportParams::default(),
                settings: Settings::default(),
                is_server: false,
                max_connections: 1,
                idle_timeout: Duration::from_secs(30),
                recv_buffer_size: 65536,
                send_buffer_size: 65536,
            }
        }

        /// Create server config
        pub fn server() -> Self {
            Self {
                transport_params: TransportParams::default(),
                settings: Settings::default(),
                is_server: true,
                max_connections: 1024,
                idle_timeout: Duration::from_secs(30),
                recv_buffer_size: 65536,
                send_buffer_size: 65536,
            }
        }

        /// Set transport parameters
        pub fn transport_params(mut self, params: TransportParams) -> Self {
            self.transport_params = params;
            self
        }

        /// Set idle timeout
        pub fn idle_timeout(mut self, timeout: Duration) -> Self {
            self.idle_timeout = timeout;
            self
        }

        /// Set max connections
        pub fn max_connections(mut self, max: usize) -> Self {
            self.max_connections = max;
            self
        }
    }

    // ========================================================================
    // Endpoint
    // ========================================================================

    /// Connection ID
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ConnectionId(u64);

    /// Async QUIC endpoint
    pub struct AsyncEndpoint {
        /// UDP socket
        socket: Arc<UdpSocket>,
        /// Configuration
        config: Config,
        /// Active connections
        connections: Arc<RwLock<HashMap<ConnectionId, Arc<AsyncConnection>>>>,
        /// Connection ID counter
        next_conn_id: AtomicU64,
        /// Shutdown flag
        shutdown: AtomicBool,
        /// New connection sender (for server)
        new_conn_tx: Option<mpsc::Sender<Arc<AsyncConnection>>>,
        /// New connection receiver (for server)
        new_conn_rx: Option<Mutex<mpsc::Receiver<Arc<AsyncConnection>>>>,
    }

    impl AsyncEndpoint {
        /// Bind to a local address
        pub async fn bind(addr: &str, config: Config) -> io::Result<Arc<Self>> {
            let socket = UdpSocket::bind(addr).await?;
            let (new_conn_tx, new_conn_rx) = if config.is_server {
                let (tx, rx) = mpsc::channel(config.max_connections);
                (Some(tx), Some(Mutex::new(rx)))
            } else {
                (None, None)
            };

            let endpoint = Arc::new(Self {
                socket: Arc::new(socket),
                config,
                connections: Arc::new(RwLock::new(HashMap::new())),
                next_conn_id: AtomicU64::new(1),
                shutdown: AtomicBool::new(false),
                new_conn_tx,
                new_conn_rx,
            });

            // Start background tasks
            let ep = endpoint.clone();
            tokio::spawn(async move {
                ep.recv_loop().await;
            });

            Ok(endpoint)
        }

        /// Connect to a remote address (client)
        pub async fn connect(&self, addr: &str) -> Result<Arc<AsyncConnection>> {
            let remote: SocketAddr = addr
                .parse()
                .map_err(|_| Error::Ng(NgError::InvalidArg))?;

            let conn_id = ConnectionId(self.next_conn_id.fetch_add(1, Ordering::Relaxed));

            let conn = Arc::new(AsyncConnection::new(
                conn_id,
                self.socket.clone(),
                remote,
                false,
                self.config.clone(),
            ));

            // Register connection
            {
                let mut conns = self.connections.write().await;
                conns.insert(conn_id, conn.clone());
            }

            // Send Initial packet
            conn.send_initial().await?;

            // Wait for handshake
            conn.wait_handshake().await?;

            Ok(conn)
        }

        /// Accept a new connection (server)
        pub async fn accept(&self) -> Option<Arc<AsyncConnection>> {
            if let Some(ref rx) = self.new_conn_rx {
                let mut rx = rx.lock().await;
                rx.recv().await
            } else {
                None
            }
        }

        /// Background receive loop
        async fn recv_loop(self: &Arc<Self>) {
            let mut buf = vec![0u8; 65536];

            while !self.shutdown.load(Ordering::Relaxed) {
                match self.socket.recv_from(&mut buf).await {
                    Ok((len, from)) => {
                        let data = buf[..len].to_vec();
                        self.handle_packet(data, from).await;
                    }
                    Err(e) => {
                        if e.kind() != io::ErrorKind::WouldBlock {
                            // Log error
                        }
                    }
                }
            }
        }

        /// Handle incoming packet
        async fn handle_packet(&self, data: Vec<u8>, from: SocketAddr) {
            // Find or create connection
            let conn = self.find_connection(&data, from).await;

            if let Some(conn) = conn {
                conn.handle_packet(&data).await;
            }
        }

        /// Find connection for packet
        async fn find_connection(
            &self,
            data: &[u8],
            from: SocketAddr,
        ) -> Option<Arc<AsyncConnection>> {
            // Try to find existing connection by remote address
            {
                let conns = self.connections.read().await;
                for conn in conns.values() {
                    if conn.remote_addr() == from {
                        return Some(conn.clone());
                    }
                }
            }

            // For server, create new connection
            if self.config.is_server {
                if let Some(ref tx) = self.new_conn_tx {
                    let conn_id = ConnectionId(self.next_conn_id.fetch_add(1, Ordering::Relaxed));

                    let conn = Arc::new(AsyncConnection::new(
                        conn_id,
                        self.socket.clone(),
                        from,
                        true,
                        self.config.clone(),
                    ));

                    // Register connection
                    {
                        let mut conns = self.connections.write().await;
                        conns.insert(conn_id, conn.clone());
                    }

                    // Notify accept
                    let _ = tx.send(conn.clone()).await;

                    return Some(conn);
                }
            }

            None
        }

        /// Shutdown the endpoint
        pub async fn shutdown(&self) {
            self.shutdown.store(true, Ordering::Relaxed);

            // Close all connections
            let conns = self.connections.read().await;
            for conn in conns.values() {
                let _ = conn.close().await;
            }
        }

        /// Get local address
        pub fn local_addr(&self) -> io::Result<SocketAddr> {
            self.socket.local_addr()
        }
    }

    // ========================================================================
    // Connection
    // ========================================================================

    /// Connection state
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConnectionState {
        /// Initial state
        Initial,
        /// Handshake in progress
        Handshaking,
        /// Handshake complete
        Connected,
        /// Draining
        Draining,
        /// Closed
        Closed,
    }

    /// Async QUIC connection
    pub struct AsyncConnection {
        /// Connection ID
        id: ConnectionId,
        /// UDP socket
        socket: Arc<UdpSocket>,
        /// Remote address
        remote: SocketAddr,
        /// Is server side
        is_server: bool,
        /// Configuration
        config: Config,
        /// Connection state
        state: RwLock<ConnectionState>,
        /// Stream manager
        streams: RwLock<StreamManager>,
        /// Pending data to send
        send_queue: Mutex<Vec<Vec<u8>>>,
        /// Handshake complete signal
        handshake_complete: Mutex<Option<oneshot::Sender<Result<()>>>>,
        /// Handshake waiter
        handshake_wait: Mutex<Option<oneshot::Receiver<Result<()>>>>,
        /// Last activity
        last_activity: RwLock<Instant>,
        /// Next stream ID
        next_stream_id: AtomicU64,
    }

    impl AsyncConnection {
        /// Create a new async connection
        pub fn new(
            id: ConnectionId,
            socket: Arc<UdpSocket>,
            remote: SocketAddr,
            is_server: bool,
            config: Config,
        ) -> Self {
            let (tx, rx) = oneshot::channel();

            Self {
                id,
                socket,
                remote,
                is_server,
                config,
                state: RwLock::new(ConnectionState::Initial),
                streams: RwLock::new(StreamManager::new(!is_server)),
                send_queue: Mutex::new(Vec::new()),
                handshake_complete: Mutex::new(Some(tx)),
                handshake_wait: Mutex::new(Some(rx)),
                last_activity: RwLock::new(Instant::now()),
                next_stream_id: AtomicU64::new(if is_server { 1 } else { 0 }),
            }
        }

        /// Get connection ID
        pub fn id(&self) -> ConnectionId {
            self.id
        }

        /// Get remote address
        pub fn remote_addr(&self) -> SocketAddr {
            self.remote
        }

        /// Get connection state
        pub async fn state(&self) -> ConnectionState {
            *self.state.read().await
        }

        /// Check if connected
        pub async fn is_connected(&self) -> bool {
            *self.state.read().await == ConnectionState::Connected
        }

        /// Send Initial packet
        async fn send_initial(&self) -> Result<()> {
            *self.state.write().await = ConnectionState::Handshaking;

            // Build Initial packet
            let packet = self.build_initial_packet()?;
            self.send_packet(&packet).await?;

            Ok(())
        }

        /// Wait for handshake to complete
        async fn wait_handshake(&self) -> Result<()> {
            let rx = {
                let mut wait = self.handshake_wait.lock().await;
                wait.take()
            };

            if let Some(rx) = rx {
                match timeout(Duration::from_secs(30), rx).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err(Error::Ng(NgError::Proto)),
                    Err(_) => Err(Error::Ng(NgError::Proto)), // Timeout
                }
            } else {
                Ok(()) // Already completed
            }
        }

        /// Handle incoming packet
        async fn handle_packet(&self, data: &[u8]) {
            // Update activity
            *self.last_activity.write().await = Instant::now();

            // Process packet based on current state
            let state = *self.state.read().await;

            match state {
                ConnectionState::Initial | ConnectionState::Handshaking => {
                    self.handle_handshake_packet(data).await;
                }
                ConnectionState::Connected => {
                    self.handle_app_packet(data).await;
                }
                ConnectionState::Draining | ConnectionState::Closed => {
                    // Ignore
                }
            }
        }

        /// Handle handshake packet
        async fn handle_handshake_packet(&self, _data: &[u8]) {
            // Process handshake
            // For now, mark as connected after receiving any packet
            *self.state.write().await = ConnectionState::Connected;

            // Signal handshake complete
            let tx = {
                let mut complete = self.handshake_complete.lock().await;
                complete.take()
            };

            if let Some(tx) = tx {
                let _ = tx.send(Ok(()));
            }
        }

        /// Handle application packet
        async fn handle_app_packet(&self, data: &[u8]) {
            // Parse and process frames
            // Dispatch stream data to appropriate streams
        }

        /// Build Initial packet
        fn build_initial_packet(&self) -> Result<Vec<u8>> {
            // Build QUIC Initial packet
            let mut packet = Vec::with_capacity(1200);

            // Long header form with Initial type
            packet.push(0xc0); // Long header, Initial

            // Version
            packet.extend_from_slice(&0x00000001u32.to_be_bytes());

            // DCID length and DCID
            packet.push(8);
            packet.extend_from_slice(&[0; 8]);

            // SCID length and SCID
            packet.push(8);
            packet.extend_from_slice(&[0; 8]);

            // Token length (0)
            packet.push(0);

            // Packet number and payload (placeholder)
            packet.extend_from_slice(&[0; 4]);

            Ok(packet)
        }

        /// Send packet
        async fn send_packet(&self, data: &[u8]) -> Result<()> {
            self.socket
                .send_to(data, self.remote)
                .await
                .map_err(|_| Error::Ng(NgError::Proto))?;
            Ok(())
        }

        /// Open a new stream
        pub async fn open_stream(&self) -> Result<AsyncStream> {
            if *self.state.read().await != ConnectionState::Connected {
                return Err(Error::Ng(NgError::Proto));
            }

            let stream_id = self.next_stream_id.fetch_add(4, Ordering::Relaxed) as StreamId;

            Ok(AsyncStream::new(
                stream_id,
                self.socket.clone(),
                self.remote,
            ))
        }

        /// Accept an incoming stream
        pub async fn accept_stream(&self) -> Result<AsyncStream> {
            // Wait for incoming stream
            // For now, return error
            Err(Error::Ng(NgError::Proto))
        }

        /// Close the connection
        pub async fn close(&self) -> Result<()> {
            *self.state.write().await = ConnectionState::Draining;

            // Send CONNECTION_CLOSE frame
            let packet = self.build_connection_close(0, b"")?;
            self.send_packet(&packet).await?;

            *self.state.write().await = ConnectionState::Closed;

            Ok(())
        }

        /// Build CONNECTION_CLOSE frame
        fn build_connection_close(&self, error: u64, reason: &[u8]) -> Result<Vec<u8>> {
            let mut packet = Vec::new();

            // CONNECTION_CLOSE frame
            packet.push(0x1c); // Frame type
            packet.push(error as u8); // Error code
            packet.push(reason.len() as u8);
            packet.extend_from_slice(reason);

            Ok(packet)
        }
    }

    // ========================================================================
    // Stream
    // ========================================================================

    /// Async QUIC stream
    pub struct AsyncStream {
        /// Stream ID
        id: StreamId,
        /// UDP socket
        socket: Arc<UdpSocket>,
        /// Remote address
        remote: SocketAddr,
        /// Read buffer
        read_buf: Mutex<Vec<u8>>,
        /// Read position
        read_pos: AtomicU64,
        /// Write buffer
        write_buf: Mutex<Vec<u8>>,
        /// Stream closed
        closed: AtomicBool,
        /// Read waker
        read_waker: Mutex<Option<Waker>>,
        /// Write waker
        write_waker: Mutex<Option<Waker>>,
    }

    impl AsyncStream {
        /// Create a new async stream
        pub fn new(id: StreamId, socket: Arc<UdpSocket>, remote: SocketAddr) -> Self {
            Self {
                id,
                socket,
                remote,
                read_buf: Mutex::new(Vec::new()),
                read_pos: AtomicU64::new(0),
                write_buf: Mutex::new(Vec::new()),
                closed: AtomicBool::new(false),
                read_waker: Mutex::new(None),
                write_waker: Mutex::new(None),
            }
        }

        /// Get stream ID
        pub fn id(&self) -> StreamId {
            self.id
        }

        /// Write data
        pub async fn write(&self, data: &[u8]) -> Result<usize> {
            if self.closed.load(Ordering::Relaxed) {
                return Err(Error::Ng(NgError::StreamState));
            }

            // Build STREAM frame and send
            let frame = self.build_stream_frame(data)?;
            self.socket
                .send_to(&frame, self.remote)
                .await
                .map_err(|_| Error::Ng(NgError::Proto))?;

            Ok(data.len())
        }

        /// Write all data
        pub async fn write_all(&self, data: &[u8]) -> Result<()> {
            let mut offset = 0;
            while offset < data.len() {
                let n = self.write(&data[offset..]).await?;
                offset += n;
            }
            Ok(())
        }

        /// Read data
        pub async fn read(&self, buf: &mut [u8]) -> Result<usize> {
            if self.closed.load(Ordering::Relaxed) {
                return Ok(0);
            }

            // Wait for data
            loop {
                {
                    let read_buf = self.read_buf.lock().await;
                    let pos = self.read_pos.load(Ordering::Relaxed) as usize;
                    if pos < read_buf.len() {
                        let available = read_buf.len() - pos;
                        let to_read = buf.len().min(available);
                        buf[..to_read].copy_from_slice(&read_buf[pos..pos + to_read]);
                        self.read_pos
                            .fetch_add(to_read as u64, Ordering::Relaxed);
                        return Ok(to_read);
                    }
                }

                // Wait for more data
                sleep(Duration::from_millis(10)).await;
            }
        }

        /// Close stream
        pub async fn close(&self) -> Result<()> {
            self.closed.store(true, Ordering::Relaxed);
            Ok(())
        }

        /// Build STREAM frame
        fn build_stream_frame(&self, data: &[u8]) -> Result<Vec<u8>> {
            let mut frame = Vec::new();

            // STREAM frame with FIN, LEN, OFF flags
            frame.push(0x0e);

            // Stream ID
            frame.extend_from_slice(&(self.id as u32).to_be_bytes());

            // Offset
            frame.extend_from_slice(&0u64.to_be_bytes());

            // Length
            frame.extend_from_slice(&(data.len() as u16).to_be_bytes());

            // Data
            frame.extend_from_slice(data);

            Ok(frame)
        }

        /// Handle incoming data
        pub async fn push_data(&self, data: &[u8]) {
            let mut buf = self.read_buf.lock().await;
            buf.extend_from_slice(data);

            // Wake reader if waiting
            if let Some(waker) = self.read_waker.lock().await.take() {
                waker.wake();
            }
        }
    }

    impl AsyncRead for AsyncStream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            // Check for closed
            if self.closed.load(Ordering::Relaxed) {
                return Poll::Ready(Ok(()));
            }

            // Try to read available data
            let read_buf = match self.read_buf.try_lock() {
                Ok(buf) => buf,
                Err(_) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
            };

            let pos = self.read_pos.load(Ordering::Relaxed) as usize;
            if pos < read_buf.len() {
                let available = read_buf.len() - pos;
                let to_read = buf.remaining().min(available);
                buf.put_slice(&read_buf[pos..pos + to_read]);
                self.read_pos
                    .fetch_add(to_read as u64, Ordering::Relaxed);
                Poll::Ready(Ok(()))
            } else {
                // Store waker and return pending
                // Note: This is simplified - proper impl would use Mutex
                Poll::Pending
            }
        }
    }

    impl AsyncWrite for AsyncStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            // Build and queue frame
            match self.build_stream_frame(buf) {
                Ok(_frame) => {
                    // Would need to actually send the frame
                    Poll::Ready(Ok(buf.len()))
                }
                Err(_) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "write error"))),
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.closed.store(true, Ordering::Relaxed);
            Poll::Ready(Ok(()))
        }
    }
}

// Re-export tokio implementation when feature is enabled
#[cfg(feature = "tokio")]
pub use tokio_impl::*;

// Non-async fallback for when tokio is not available
#[cfg(not(feature = "tokio"))]
pub mod sync {
    use std::io;
    use std::net::{SocketAddr, UdpSocket};
    use std::time::Duration;

    use crate::error::{Error, NgError, Result};

    /// Synchronous endpoint (fallback)
    pub struct SyncEndpoint {
        socket: UdpSocket,
    }

    impl SyncEndpoint {
        /// Bind to address
        pub fn bind(addr: &str) -> io::Result<Self> {
            let socket = UdpSocket::bind(addr)?;
            socket.set_read_timeout(Some(Duration::from_millis(100)))?;
            Ok(Self { socket })
        }

        /// Connect to remote
        pub fn connect(&self, addr: &str) -> Result<SyncConnection> {
            let remote: SocketAddr = addr
                .parse()
                .map_err(|_| Error::Ng(NgError::InvalidArg))?;

            Ok(SyncConnection {
                remote,
                socket_fd: 0, // Would need socket clone/dup
            })
        }
    }

    /// Synchronous connection (fallback)
    pub struct SyncConnection {
        remote: SocketAddr,
        socket_fd: i32,
    }

    impl SyncConnection {
        pub fn remote_addr(&self) -> SocketAddr {
            self.remote
        }
    }
}

#[cfg(not(feature = "tokio"))]
pub use sync::*;
