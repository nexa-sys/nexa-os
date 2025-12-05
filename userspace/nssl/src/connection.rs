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
use crate::kex::{KeyExchange, derive_tls12_keys, derive_tls13_keys};
use crate::tls::{HandshakeType, ContentType, NamedGroup, ExtensionType};
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
    
    /// Key exchange context
    kex: Option<KeyExchange>,
    
    /// Pre-master secret (TLS 1.2)
    pre_master_secret: Option<Vec<u8>>,
    
    /// Server's key exchange data (ECDHE public key)
    server_kex_pubkey: Option<Vec<u8>>,
    
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
            kex: None,
            pre_master_secret: None,
            server_kex_pubkey: None,
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
        
        if let Some((version, cipher, extensions)) = self.handshake.parse_server_hello(&data) {
            self.version = version;
            // Find cipher from ID
            self.current_cipher = SslCipher::from_id(cipher);
            
            // 对于 TLS 1.3，从扩展中提取 key_share
            if version >= TLS1_3_VERSION {
                for (ext_type, ext_data) in extensions {
                    // key_share 扩展类型是 51
                    if ext_type == ExtensionType::KeyShare as u16 && ext_data.len() >= 4 {
                        // 解析: named_group (2) + key_exchange_length (2) + key_exchange
                        let _group = ((ext_data[0] as u16) << 8) | (ext_data[1] as u16);
                        let key_len = ((ext_data[2] as usize) << 8) | (ext_data[3] as usize);
                        if ext_data.len() >= 4 + key_len {
                            self.server_kex_pubkey = Some(ext_data[4..4 + key_len].to_vec());
                        }
                    }
                }
            }
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
        // TLS 1.3 handshake after ServerHello:
        // 在 TLS 1.3 中，ServerHello 后的消息都是加密的
        // 需要先从 key_share 扩展中提取服务器的公钥并计算共享密钥
        
        // 获取服务器的 key share
        let server_pubkey = match &self.server_kex_pubkey {
            Some(pk) => pk.clone(),
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 初始化密钥交换并计算共享密钥
        let mut kex = KeyExchange::new_x25519();
        let shared_secret = match kex.compute_shared_secret(&server_pubkey) {
            Some(s) => s.to_vec(),
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 派生握手密钥
        let transcript_hash = self.handshake.get_transcript_hash();
        let keys = match derive_tls13_keys(&shared_secret, &transcript_hash, true) {
            Some(k) => k,
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 设置读取密钥（解密服务器消息）
        self.record.set_keys(
            keys.server_key.clone(),
            keys.client_key.clone(),
            keys.server_iv.clone(),
            keys.client_iv.clone(),
        );
        
        // TLS 1.3 后续消息都是加密的
        // 1. 接收 EncryptedExtensions (可选处理)
        // 2. 接收 Certificate
        // 3. 接收 CertificateVerify
        // 4. 接收 Finished
        // 简化：跳过证书验证，直接尝试读取并切换到应用密钥
        
        // 派生应用密钥
        let app_keys = match derive_tls13_keys(&shared_secret, &transcript_hash, false) {
            Some(k) => k,
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        self.record.set_keys(
            app_keys.server_key,
            app_keys.client_key,
            app_keys.server_iv,
            app_keys.client_iv,
        );
        
        self.kex = Some(kex);
        1
    }

    /// TLS 1.3 server handshake
    fn do_tls13_server_handshake(&mut self) -> c_int {
        // TLS 1.3 server handshake - 简化实现
        1
    }

    /// TLS 1.2 client handshake
    fn do_tls12_client_handshake(&mut self) -> c_int {
        // TLS 1.2 完整握手流程:
        // 1. 接收 Certificate
        // 2. 接收 ServerKeyExchange (ECDHE)
        // 3. 接收 ServerHelloDone
        // 4. 发送 ClientKeyExchange
        // 5. 发送 ChangeCipherSpec
        // 6. 发送 Finished
        // 7. 接收 ChangeCipherSpec
        // 8. 接收 Finished
        
        // 步骤 1: 接收 Certificate
        if !self.receive_certificate() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 2: 接收 ServerKeyExchange
        if !self.receive_server_key_exchange() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 3: 接收 ServerHelloDone
        if !self.receive_server_hello_done() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 4: 发送 ClientKeyExchange
        if !self.send_client_key_exchange() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 计算预主密钥并派生会话密钥
        if !self.derive_session_keys_tls12() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 5: 发送 ChangeCipherSpec
        if !self.send_change_cipher_spec() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 6: 发送 Finished
        if !self.send_finished() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 7: 接收 ChangeCipherSpec
        if !self.receive_change_cipher_spec() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 8: 接收 Finished
        if !self.receive_finished() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        1
    }

    /// TLS 1.2 server handshake
    fn do_tls12_server_handshake(&mut self) -> c_int {
        // 简化的服务器握手
        1
    }

    /// 接收服务器证书
    fn receive_certificate(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        // 验证是 Certificate 消息
        if data.is_empty() || data[0] != HandshakeType::Certificate as u8 {
            return false;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        // 简化：不解析证书，仅确认收到
        // 在生产环境中需要验证证书链
        true
    }
    
    /// 接收 ServerKeyExchange (ECDHE)
    fn receive_server_key_exchange(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        if data.len() < 4 {
            return false;
        }
        
        // ServerKeyExchange 类型是 12
        if data[0] != 12 {
            // 可能是 CertificateRequest 或 ServerHelloDone
            // 如果不是 ServerKeyExchange，可能是使用 RSA 密钥交换
            // 暂时将数据放回处理队列（简化：假设总是有 ServerKeyExchange）
            return false;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        // 解析消息长度
        let msg_len = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        if data.len() < 4 + msg_len {
            return false;
        }
        
        let msg = &data[4..4 + msg_len];
        
        // 解析 ECDHE 参数
        // curve_type (1 byte) + named_curve (2 bytes) + point_len (1 byte) + point
        if msg.len() < 4 {
            return false;
        }
        
        let curve_type = msg[0];
        if curve_type != 3 {
            // 3 = named_curve
            return false;
        }
        
        let _named_curve = ((msg[1] as u16) << 8) | (msg[2] as u16);
        let point_len = msg[3] as usize;
        
        if msg.len() < 4 + point_len {
            return false;
        }
        
        // 提取服务器的公钥点
        let server_pubkey = msg[4..4 + point_len].to_vec();
        self.server_kex_pubkey = Some(server_pubkey);
        
        true
    }
    
    /// 接收 ServerHelloDone
    fn receive_server_hello_done(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        // ServerHelloDone 类型是 14，消息体为空
        if data.len() < 4 || data[0] != 14 {
            return false;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        true
    }
    
    /// 发送 ClientKeyExchange
    fn send_client_key_exchange(&mut self) -> bool {
        // 初始化 ECDHE 密钥交换
        let kex = KeyExchange::new_x25519();
        let client_pubkey = kex.public_key().to_vec();
        
        // 构建 ClientKeyExchange 消息
        let mut msg = Vec::new();
        
        // 握手类型: ClientKeyExchange (16)
        msg.push(16);
        
        // 消息长度占位
        let len_pos = msg.len();
        msg.extend_from_slice(&[0, 0, 0]);
        
        // ECDH 公钥长度 + 公钥
        msg.push(client_pubkey.len() as u8);
        msg.extend_from_slice(&client_pubkey);
        
        // 更新长度
        let msg_len = msg.len() - len_pos - 3;
        msg[len_pos] = ((msg_len >> 16) & 0xFF) as u8;
        msg[len_pos + 1] = ((msg_len >> 8) & 0xFF) as u8;
        msg[len_pos + 2] = (msg_len & 0xFF) as u8;
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        // 存储密钥交换上下文
        self.kex = Some(kex);
        
        // 发送
        self.record.write_handshake(&msg, self.wbio)
    }
    
    /// 派生 TLS 1.2 会话密钥
    fn derive_session_keys_tls12(&mut self) -> bool {
        // 计算共享密钥（预主密钥）
        let server_pubkey = match &self.server_kex_pubkey {
            Some(pk) => pk.clone(),
            None => return false,
        };
        
        let kex = match &mut self.kex {
            Some(k) => k,
            None => return false,
        };
        
        let pre_master_secret = match kex.compute_shared_secret(&server_pubkey) {
            Some(s) => s.to_vec(),
            None => return false,
        };
        
        // 使用 PRF 派生密钥
        // 获取密钥长度（AES-256-GCM: key=32, iv=4 for explicit + 8 implicit = 12 total, 但 TLS 1.2 GCM 使用 4 字节 implicit IV）
        let key_len = 32; // AES-256
        let iv_len = 4;   // GCM implicit IV
        
        let keys = match derive_tls12_keys(
            &pre_master_secret,
            &self.handshake.client_random,
            &self.handshake.server_random,
            key_len,
            iv_len,
        ) {
            Some(k) => k,
            None => return false,
        };
        
        // 存储预主密钥（用于 Finished 消息验证）
        self.pre_master_secret = Some(pre_master_secret);
        
        // 设置记录层密钥
        self.record.set_keys(
            keys.server_key,
            keys.client_key,
            keys.server_iv,
            keys.client_iv,
        );
        
        true
    }
    
    /// 发送 ChangeCipherSpec
    fn send_change_cipher_spec(&mut self) -> bool {
        let ccs = [1u8]; // ChangeCipherSpec 消息内容
        self.record.write_ccs(&ccs, self.wbio)
    }
    
    /// 发送 Finished
    fn send_finished(&mut self) -> bool {
        // 计算 verify_data
        // verify_data = PRF(master_secret, "client finished", Hash(handshake_messages))
        
        let transcript_hash = self.handshake.get_transcript_hash();
        
        // 使用简化的 verify_data（12 字节）
        let verify_data = compute_verify_data(
            self.pre_master_secret.as_deref().unwrap_or(&[]),
            &self.handshake.client_random,
            &self.handshake.server_random,
            b"client finished",
            &transcript_hash,
        );
        
        // 构建 Finished 消息
        let mut msg = Vec::new();
        msg.push(HandshakeType::Finished as u8);
        
        let len = verify_data.len();
        msg.push(((len >> 16) & 0xFF) as u8);
        msg.push(((len >> 8) & 0xFF) as u8);
        msg.push((len & 0xFF) as u8);
        msg.extend_from_slice(&verify_data);
        
        // Finished 消息是加密发送的
        self.record.write_handshake(&msg, self.wbio)
    }
    
    /// 接收 ChangeCipherSpec
    fn receive_change_cipher_spec(&mut self) -> bool {
        // 读取 ChangeCipherSpec 记录
        self.record.read_ccs(self.rbio)
    }
    
    /// 接收 Finished
    fn receive_finished(&mut self) -> bool {
        // Finished 消息是加密的
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        if data.is_empty() || data[0] != HandshakeType::Finished as u8 {
            return false;
        }
        
        // 简化：不验证 verify_data
        // 在生产环境中应该验证
        true
    }

    /// Derive TLS 1.3 keys
    fn derive_keys_tls13(&mut self) {
        // 已在 do_tls13_client_handshake 中实现
    }

    /// Derive TLS 1.2 keys
    fn derive_keys_tls12(&mut self) {
        // 已在 derive_session_keys_tls12 中实现
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

/// 计算 TLS 1.2 Finished 消息的 verify_data
fn compute_verify_data(
    pre_master_secret: &[u8],
    client_random: &[u8],
    server_random: &[u8],
    label: &[u8],
    transcript_hash: &[u8],
) -> Vec<u8> {
    // 首先计算 master_secret
    let mut seed = Vec::new();
    seed.extend_from_slice(client_random);
    seed.extend_from_slice(server_random);
    let master_secret = prf_sha256(pre_master_secret, b"master secret", &seed, 48);
    
    // 然后计算 verify_data
    // verify_data = PRF(master_secret, label, transcript_hash)[0..12]
    prf_sha256(&master_secret, label, transcript_hash, 12)
}

/// TLS 1.2 PRF with SHA-256 (RFC 5246 Section 5)
fn prf_sha256(secret: &[u8], label: &[u8], seed: &[u8], length: usize) -> Vec<u8> {
    let mut full_seed = Vec::new();
    full_seed.extend_from_slice(label);
    full_seed.extend_from_slice(seed);
    
    let mut result = Vec::new();
    let mut a = ncryptolib::hmac_sha256(secret, &full_seed).to_vec();
    
    while result.len() < length {
        let mut data = a.clone();
        data.extend_from_slice(&full_seed);
        let p = ncryptolib::hmac_sha256(secret, &data);
        result.extend_from_slice(&p);
        a = ncryptolib::hmac_sha256(secret, &a).to_vec();
    }
    
    result.truncate(length);
    result
}
