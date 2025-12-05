//! SSL Connection
//!
//! Represents a single TLS connection.

use std::vec::Vec;

use crate::context::SslContext;
use crate::cipher::SslCipher;
use crate::session::SslSession;
use crate::bio::Bio;
use crate::x509::{X509, X509Stack};
use crate::handshake::HandshakeState;
use crate::record::RecordLayer;
use crate::error::{SslError, SslResult};
use crate::{c_int, c_char, c_uchar, TLS1_2_VERSION, TLS1_3_VERSION};

/// Connection state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state
    Init,
    /// Connect state (client)
    Connect,
    /// Accept state (server)
    Accept,
    /// Handshake in progress
    Handshaking,
    /// Connection established
    Established,
    /// Shutdown in progress
    Shutdown,
    /// Connection closed
    Closed,
    /// Error state
    Error,
}

/// SSL Connection
pub struct SslConnection {
    /// Connection state
    state: ConnectionState,
    
    /// Is client mode
    is_client: bool,
    
    /// Negotiated protocol version
    version: u16,
    
    /// File descriptor
    fd: i32,
    
    /// Read BIO
    rbio: *mut Bio,
    
    /// Write BIO
    wbio: *mut Bio,
    
    /// Handshake state
    handshake: HandshakeState,
    
    /// Record layer
    record: RecordLayer,
    
    /// Current cipher
    current_cipher: Option<SslCipher>,
    
    /// Peer certificate
    peer_cert: Option<X509>,
    
    /// Peer certificate chain
    peer_chain: Vec<X509>,
    
    /// Verification result
    verify_result: i64,
    
    /// Session
    session: Option<SslSession>,
    
    /// Session was resumed
    session_reused: bool,
    
    /// SNI hostname
    hostname: Option<String>,
    
    /// Selected ALPN protocol
    alpn_selected: Vec<u8>,
    
    /// Last error code
    last_error: i32,
    
    /// Context reference (for configuration)
    min_version: u16,
    max_version: u16,
    verify_mode: i32,
    cipher_list: Vec<u16>,
    ciphersuites: Vec<u16>,
    alpn_protos: Vec<u8>,
}

use std::string::String;

impl SslConnection {
    /// Create new SSL connection from context
    pub fn new(ctx: &SslContext) -> SslResult<Self> {
        let method = ctx.get_method();
        
        Ok(Self {
            state: ConnectionState::Init,
            is_client: method.is_client_method(),
            version: 0,
            fd: -1,
            rbio: core::ptr::null_mut(),
            wbio: core::ptr::null_mut(),
            handshake: HandshakeState::new(),
            record: RecordLayer::new(),
            current_cipher: None,
            peer_cert: None,
            peer_chain: Vec::new(),
            verify_result: 0,
            session: None,
            session_reused: false,
            hostname: None,
            alpn_selected: Vec::new(),
            last_error: 0,
            min_version: ctx.get_min_version(),
            max_version: ctx.get_max_version(),
            verify_mode: ctx.get_verify_mode(),
            cipher_list: ctx.get_cipher_list().get_suites().to_vec(),
            ciphersuites: ctx.get_ciphersuites().to_vec(),
            alpn_protos: ctx.get_alpn_protos().to_vec(),
        })
    }

    /// Set file descriptor
    pub fn set_fd(&mut self, fd: i32) -> bool {
        self.fd = fd;
        
        // Create socket BIOs
        let bio = Bio::new_socket(fd, 0); // Don't close on free
        if bio.is_null() {
            return false;
        }
        
        self.rbio = bio;
        self.wbio = bio;
        true
    }

    /// Get file descriptor
    pub fn get_fd(&self) -> i32 {
        self.fd
    }

    /// Set BIOs
    pub fn set_bio(&mut self, rbio: *mut Bio, wbio: *mut Bio) {
        // Free existing BIOs if different
        if !self.rbio.is_null() && self.rbio != rbio {
            Bio::free(self.rbio);
        }
        if !self.wbio.is_null() && self.wbio != wbio && self.wbio != self.rbio {
            Bio::free(self.wbio);
        }
        
        self.rbio = rbio;
        self.wbio = wbio;
    }

    /// Set connect state (client mode)
    pub fn set_connect_state(&mut self) {
        self.state = ConnectionState::Connect;
        self.is_client = true;
    }

    /// Set accept state (server mode)
    pub fn set_accept_state(&mut self) {
        self.state = ConnectionState::Accept;
        self.is_client = false;
    }

    /// Perform client handshake
    pub fn connect(&mut self) -> c_int {
        self.set_connect_state();
        self.do_handshake()
    }

    /// Perform server handshake
    pub fn accept(&mut self) -> c_int {
        self.set_accept_state();
        self.do_handshake()
    }

    /// Perform handshake
    pub fn do_handshake(&mut self) -> c_int {
        if self.state == ConnectionState::Init {
            // Auto-detect based on method
            if self.is_client {
                self.state = ConnectionState::Connect;
            } else {
                self.state = ConnectionState::Accept;
            }
        }

        self.state = ConnectionState::Handshaking;
        
        // Perform TLS handshake
        let result = if self.is_client {
            self.do_client_handshake()
        } else {
            self.do_server_handshake()
        };
        
        if result == 1 {
            self.state = ConnectionState::Established;
        } else if result < 0 {
            self.state = ConnectionState::Error;
        }
        
        result
    }

    /// Client handshake implementation
    fn do_client_handshake(&mut self) -> c_int {
        // Send ClientHello
        if !self.send_client_hello() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // Receive ServerHello
        if !self.receive_server_hello() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // Complete handshake based on version
        if self.version >= TLS1_3_VERSION {
            self.do_tls13_client_handshake()
        } else {
            self.do_tls12_client_handshake()
        }
    }

    /// Server handshake implementation
    fn do_server_handshake(&mut self) -> c_int {
        // Receive ClientHello
        if !self.receive_client_hello() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // Send ServerHello
        if !self.send_server_hello() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // Complete handshake based on version
        if self.version >= TLS1_3_VERSION {
            self.do_tls13_server_handshake()
        } else {
            self.do_tls12_server_handshake()
        }
    }

    /// Send ClientHello
    fn send_client_hello(&mut self) -> bool {
        let hello = self.handshake.build_client_hello(
            self.max_version,
            &self.ciphersuites,
            &self.cipher_list,
            self.hostname.as_deref(),
            &self.alpn_protos,
        );
        
        self.record.write_handshake(&hello, self.wbio)
    }

    /// Receive ServerHello
    fn receive_server_hello(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        if let Some((version, cipher, _extensions)) = self.handshake.parse_server_hello(&data) {
            self.version = version;
            // Find cipher from ID
            self.current_cipher = SslCipher::from_id(cipher);
            true
        } else {
            false
        }
    }

    /// Receive ClientHello (server side)
    fn receive_client_hello(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        self.handshake.parse_client_hello(&data).is_some()
    }

    /// Send ServerHello (server side)
    fn send_server_hello(&mut self) -> bool {
        // Select cipher and version
        self.version = self.select_version();
        let cipher = self.select_cipher();
        
        if let Some(c) = cipher {
            let cipher_id = c.id;
            self.current_cipher = Some(c);
            let hello = self.handshake.build_server_hello(
                self.version,
                cipher_id,
                &self.alpn_protos,
            );
            self.record.write_handshake(&hello, self.wbio)
        } else {
            false
        }
    }

    /// TLS 1.3 client handshake
    fn do_tls13_client_handshake(&mut self) -> c_int {
        // TLS 1.3 handshake:
        // 1. Receive EncryptedExtensions
        // 2. Receive Certificate (optional)
        // 3. Receive CertificateVerify (optional)
        // 4. Receive Finished
        // 5. Send Finished
        
        // Simplified: assume successful handshake
        // Real implementation would handle all messages
        
        self.derive_keys_tls13();
        1
    }

    /// TLS 1.3 server handshake
    fn do_tls13_server_handshake(&mut self) -> c_int {
        // TLS 1.3 server handshake
        self.derive_keys_tls13();
        1
    }

    /// TLS 1.2 client handshake
    fn do_tls12_client_handshake(&mut self) -> c_int {
        // TLS 1.2 handshake:
        // 1. Receive Certificate
        // 2. Receive ServerKeyExchange
        // 3. Receive ServerHelloDone
        // 4. Send ClientKeyExchange
        // 5. Send ChangeCipherSpec
        // 6. Send Finished
        // 7. Receive ChangeCipherSpec
        // 8. Receive Finished
        
        self.derive_keys_tls12();
        1
    }

    /// TLS 1.2 server handshake
    fn do_tls12_server_handshake(&mut self) -> c_int {
        self.derive_keys_tls12();
        1
    }

    /// Derive TLS 1.3 keys
    fn derive_keys_tls13(&mut self) {
        // Use HKDF to derive keys from handshake secret
        // This would use ncryptolib::kdf::hkdf
    }

    /// Derive TLS 1.2 keys
    fn derive_keys_tls12(&mut self) {
        // Use PRF to derive keys from pre-master secret
    }

    /// Select protocol version
    fn select_version(&self) -> u16 {
        // Prefer highest common version
        self.max_version.min(TLS1_3_VERSION)
    }

    /// Select cipher suite
    fn select_cipher(&self) -> Option<SslCipher> {
        // Return first matching cipher
        if self.version >= TLS1_3_VERSION {
            self.ciphersuites.first().and_then(|&id| SslCipher::from_id(id))
        } else {
            self.cipher_list.first().and_then(|&id| SslCipher::from_id(id))
        }
    }

    /// Read decrypted data
    pub fn read(&mut self, buf: &mut [u8]) -> c_int {
        if self.state != ConnectionState::Established {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        match self.record.read_application_data(self.rbio, buf) {
            Some(n) => n as c_int,
            None => {
                if self.record.is_eof() {
                    self.last_error = crate::ssl_error::SSL_ERROR_ZERO_RETURN;
                    0
                } else {
                    self.last_error = crate::ssl_error::SSL_ERROR_WANT_READ;
                    -1
                }
            }
        }
    }

    /// Write data (will be encrypted)
    pub fn write(&mut self, buf: &[u8]) -> c_int {
        if self.state != ConnectionState::Established {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        if self.record.write_application_data(buf, self.wbio) {
            buf.len() as c_int
        } else {
            self.last_error = crate::ssl_error::SSL_ERROR_WANT_WRITE;
            -1
        }
    }

    /// Shutdown connection
    pub fn shutdown(&mut self) -> c_int {
        if self.state == ConnectionState::Closed {
            return 1;
        }
        
        // Send close_notify alert
        self.record.send_alert(0, 0, self.wbio); // close_notify
        self.state = ConnectionState::Shutdown;
        
        // Receive close_notify
        // For bidirectional shutdown, we'd wait for peer's close_notify
        self.state = ConnectionState::Closed;
        1
    }

    /// Get last error
    pub fn get_error(&self, ret: c_int) -> c_int {
        if ret > 0 {
            crate::ssl_error::SSL_ERROR_NONE
        } else if ret == 0 {
            crate::ssl_error::SSL_ERROR_ZERO_RETURN
        } else {
            self.last_error
        }
    }

    /// Get negotiated version
    pub fn version(&self) -> u16 {
        self.version
    }

    /// Get version string
    pub fn get_version_string(&self) -> *const c_char {
        match self.version {
            TLS1_3_VERSION => b"TLSv1.3\0".as_ptr() as *const c_char,
            TLS1_2_VERSION => b"TLSv1.2\0".as_ptr() as *const c_char,
            _ => b"unknown\0".as_ptr() as *const c_char,
        }
    }

    /// Get current cipher
    pub fn get_current_cipher(&self) -> *const SslCipher {
        match &self.current_cipher {
            Some(cipher) => cipher as *const _,
            None => core::ptr::null(),
        }
    }

    /// Set SNI hostname
    pub fn set_hostname(&mut self, name: &str) -> bool {
        self.hostname = Some(name.to_string());
        true
    }

    /// Get peer certificate
    pub fn get_peer_certificate(&self) -> *mut X509 {
        // Return a copy of peer cert
        match &self.peer_cert {
            Some(cert) => Box::into_raw(Box::new(cert.clone())),
            None => core::ptr::null_mut(),
        }
    }

    /// Get peer certificate chain
    pub fn get_peer_cert_chain(&self) -> *mut X509Stack {
        if self.peer_chain.is_empty() {
            return core::ptr::null_mut();
        }
        
        let mut stack = X509Stack::new();
        for cert in &self.peer_chain {
            stack.push(cert.clone());
        }
        Box::into_raw(Box::new(stack))
    }

    /// Get verification result
    pub fn get_verify_result(&self) -> i64 {
        self.verify_result
    }

    /// Get selected ALPN protocol
    pub fn get_alpn_selected(&self) -> (*const c_uchar, usize) {
        if self.alpn_selected.is_empty() {
            (core::ptr::null(), 0)
        } else {
            (self.alpn_selected.as_ptr(), self.alpn_selected.len())
        }
    }

    /// Check if session was resumed
    pub fn session_reused(&self) -> bool {
        self.session_reused
    }

    /// Get session
    pub fn get_session(&self) -> *mut SslSession {
        match &self.session {
            Some(sess) => Box::into_raw(Box::new(sess.clone())),
            None => core::ptr::null_mut(),
        }
    }

    /// Set session for resumption
    pub fn set_session(&mut self, session: *mut SslSession) -> bool {
        if session.is_null() {
            self.session = None;
        } else {
            unsafe {
                self.session = Some((*session).clone());
            }
        }
        true
    }
}

impl Drop for SslConnection {
    fn drop(&mut self) {
        // Free BIOs
        if !self.rbio.is_null() {
            Bio::free(self.rbio);
        }
        if !self.wbio.is_null() && self.wbio != self.rbio {
            Bio::free(self.wbio);
        }
    }
}
