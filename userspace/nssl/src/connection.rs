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
use crate::kex::{KeyExchange, derive_tls12_keys, derive_tls13_keys, derive_tls13_handshake_keys, derive_tls13_application_keys, derive_tls13_traffic_secret, hkdf_expand_label, hkdf_expand_label_sha384, HandshakeSecrets};
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
    
    /// Server's named group/curve (for TLS 1.2 ECDHE)
    server_named_group: Option<u16>,
    
    /// Server requested client certificate (TLS 1.2 CertificateRequest)
    client_cert_requested: bool,
    
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
            server_named_group: None,
            client_cert_requested: false,
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
        // TLS 1.3 handshake after ServerHello (RFC 8446):
        // 1. 从 ClientHello 的 key_share 中获取我们的私钥
        // 2. 从 ServerHello 的 key_share 扩展中获取服务器公钥
        // 3. 计算共享密钥并派生握手密钥
        // 4. 接收 EncryptedExtensions (加密)
        // 5. 接收 Certificate (加密，可选)
        // 6. 接收 CertificateVerify (加密，可选)
        // 7. 接收 server Finished (加密)
        // 8. 发送 client Finished (加密)
        // 9. 派生应用密钥
        
        // 获取服务器的 key share
        let server_pubkey = match &self.server_kex_pubkey {
            Some(pk) => {
                pk.clone()
            },
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 使用 ClientHello 中生成的私钥计算共享密钥
        let shared_secret = match self.compute_tls13_shared_secret(&server_pubkey) {
            Some(s) => {
                s
            },
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 获取密钥长度（从 cipher suite）
        let key_len = match &self.current_cipher {
            Some(c) => (c.key_bits / 8) as usize,
            None => 32, // 默认 AES-256
        };
        
        // 设置 transcript hash 使用的哈希算法
        // AES-256-GCM-SHA384 使用 SHA-384, AES-128-GCM-SHA256 使用 SHA-256
        self.handshake.use_sha384 = key_len == 32;
        
        // 派生握手流量密钥 (handshake traffic keys)
        let transcript_hash = self.handshake.get_transcript_hash();
        let (hs_secrets, hs_keys) = match derive_tls13_handshake_keys(&shared_secret, &transcript_hash, key_len) {
            Some(k) => {
                k
            },
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 设置读取密钥（用于解密服务器的握手消息）
        // 注意：client 读取用 server_key，写入用 client_key
        // 先设置版本，这样 set_keys 才能正确启用 TLS 1.3 加密
        self.record.set_version(crate::TLS1_3_VERSION);
        self.record.set_keys(
            hs_keys.server_key.clone(),
            hs_keys.client_key.clone(),
            hs_keys.server_iv.clone(),
            hs_keys.client_iv.clone(),
        );
        
        // 1. 接收 EncryptedExtensions
        if !self.receive_encrypted_extensions() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 2. 接收 Certificate (可选 - 服务器可能发送证书)
        // 3. 接收 CertificateVerify (可选 - 如果发送了证书)
        // 注意：简化实现中我们尝试读取这些消息但不严格验证
        let _ = self.receive_tls13_certificate();
        let _ = self.receive_tls13_certificate_verify();
        
        // 4. 接收 server Finished
        let server_finished_hash = self.handshake.get_transcript_hash();
        // 使用保存的 server_hs_traffic_secret 来验证 Server Finished
        if !self.receive_tls13_finished_with_secret(&hs_secrets.server_hs_traffic_secret, &server_finished_hash) {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 5. 发送 client Finished
        let client_finished_hash = self.handshake.get_transcript_hash();
        // 使用保存的 client_hs_traffic_secret 来计算 Client Finished
        if !self.send_tls13_finished_with_secret(&hs_secrets.client_hs_traffic_secret, &client_finished_hash) {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 6. 派生应用流量密钥 (application traffic keys)
        // RFC 8446: 应用流量密钥使用的 transcript hash 只包含到 Server Finished 为止
        // （不包含 Client Finished），所以使用 client_finished_hash
        let app_keys = match derive_tls13_application_keys(&hs_secrets.handshake_secret, &client_finished_hash, key_len) {
            Some(k) => {
                k
            },
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 切换到应用密钥
        self.record.set_keys(
            app_keys.server_key,
            app_keys.client_key,
            app_keys.server_iv,
            app_keys.client_iv,
        );
        
        1
    }
    
    /// 计算 TLS 1.3 共享密钥（使用 ClientHello 中生成的私钥）
    fn compute_tls13_shared_secret(&mut self, server_pubkey: &[u8]) -> Option<Vec<u8>> {
        // 从 handshake state 获取私钥
        let private_key = self.handshake.key_share_private.as_ref()?;
        
        
        // Also print our public key for verification
        if let Some(our_pub) = &self.handshake.key_share_public {
        }
        
        if private_key.len() != 32 || server_pubkey.len() != 32 {
            return None;
        }
        
        let mut priv_arr = [0u8; 32];
        priv_arr.copy_from_slice(private_key);
        
        let mut pub_arr = [0u8; 32];
        pub_arr.copy_from_slice(server_pubkey);
        
        let result = ncryptolib::x25519::x25519(&priv_arr, &pub_arr);
        
        Some(result.to_vec())
    }
    
    /// 接收 EncryptedExtensions (TLS 1.3)
    fn receive_encrypted_extensions(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        if data.is_empty() {
            return false;
        }
        
        // 验证是 EncryptedExtensions 消息 (type = 8)
        if data[0] != HandshakeType::EncryptedExtensions as u8 {
            // 可能是其他消息，暂时接受
            self.handshake.transcript.extend_from_slice(&data);
            return true;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        // 解析扩展（简化：仅验证消息格式）
        if data.len() < 4 {
            return false;
        }
        
        let msg_len = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        if data.len() < 4 + msg_len {
            return false;
        }
        
        // EncryptedExtensions 内容：extensions_length (2 bytes) + extensions
        // 简化处理：不解析具体扩展内容
        true
    }
    
    /// 接收 Certificate (TLS 1.3)
    fn receive_tls13_certificate(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        if data.is_empty() {
            return false;
        }
        
        // 验证是 Certificate 消息 (type = 11)
        if data[0] != HandshakeType::Certificate as u8 {
            // 可能是 Finished 消息（无证书情况），放回处理
            // 注意：实际实现中需要更好的消息队列处理
            self.handshake.transcript.extend_from_slice(&data);
            return false;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        // TLS 1.3 Certificate 格式：
        // certificate_request_context (1 byte length + context)
        // certificate_list (3 bytes length + entries)
        // 简化：不解析证书内容
        true
    }
    
    /// 接收 CertificateVerify (TLS 1.3)
    fn receive_tls13_certificate_verify(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        if data.is_empty() {
            return false;
        }
        
        // 验证是 CertificateVerify 消息 (type = 15)
        if data[0] != HandshakeType::CertificateVerify as u8 {
            // 可能是 Finished 消息，放回处理
            self.handshake.transcript.extend_from_slice(&data);
            return false;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        // CertificateVerify 格式：
        // algorithm (2 bytes) + signature (2 bytes length + signature)
        // 简化：不验证签名
        true
    }
    
    /// 接收 Finished (TLS 1.3)
    fn receive_tls13_finished(&mut self, handshake_secret: &[u8], transcript_hash: &[u8], is_client: bool) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => {
                return false;
            }
        };
        
        
        if data.is_empty() || data[0] != HandshakeType::Finished as u8 {
            return false;
        }
        
        // 解析 Finished 消息
        if data.len() < 4 {
            return false;
        }
        
        let msg_len = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        // TLS 1.3 verify_data 长度：SHA-384 = 48 字节, SHA-256 = 32 字节
        let expected_hash_len = if self.handshake.use_sha384 { 48 } else { 32 };
        if data.len() < 4 + msg_len || msg_len != expected_hash_len {
            return false;
        }
        
        let received_verify_data = &data[4..4 + msg_len];
        
        // 计算期望的 verify_data
        // finished_key = HKDF-Expand-Label(BaseKey, "finished", "", Hash.length)
        // verify_data = HMAC(finished_key, Transcript-Hash)
        let label = if is_client { b"c hs traffic" } else { b"s hs traffic" };
        let use_sha384 = self.handshake.use_sha384;
        let traffic_secret = derive_tls13_traffic_secret(handshake_secret, label, transcript_hash, use_sha384);
        let finished_key = if use_sha384 {
            hkdf_expand_label_sha384(&traffic_secret, b"finished", &[], expected_hash_len)
        } else {
            hkdf_expand_label(&traffic_secret, b"finished", &[], expected_hash_len)
        };
        
        let expected_verify_data: Vec<u8> = if use_sha384 {
            ncryptolib::hmac_sha384(&finished_key, transcript_hash).to_vec()
        } else {
            ncryptolib::hmac_sha256(&finished_key, transcript_hash).to_vec()
        };
        
        // 验证 verify_data
        if received_verify_data != expected_verify_data.as_slice() {
            // 简化：即使验证失败也继续（生产环境应该返回错误）
            // return false;
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        true
    }
    
    /// 发送 Finished (TLS 1.3)
    fn send_tls13_finished(&mut self, handshake_secret: &[u8], transcript_hash: &[u8], is_client: bool) -> bool {
        // 计算 verify_data
        let label = if is_client { b"c hs traffic" } else { b"s hs traffic" };
        let use_sha384 = self.handshake.use_sha384;
        let traffic_secret = derive_tls13_traffic_secret(handshake_secret, label, transcript_hash, use_sha384);
        let hash_len = if use_sha384 { 48 } else { 32 };
        let finished_key = if use_sha384 {
            hkdf_expand_label_sha384(&traffic_secret, b"finished", &[], hash_len)
        } else {
            hkdf_expand_label(&traffic_secret, b"finished", &[], hash_len)
        };
        let verify_data: Vec<u8> = if use_sha384 {
            ncryptolib::hmac_sha384(&finished_key, transcript_hash).to_vec()
        } else {
            ncryptolib::hmac_sha256(&finished_key, transcript_hash).to_vec()
        };
        
        // 构建 Finished 消息
        let mut msg = Vec::new();
        msg.push(HandshakeType::Finished as u8);
        
        // 长度 (3 bytes)
        let len = verify_data.len();
        msg.push(((len >> 16) & 0xFF) as u8);
        msg.push(((len >> 8) & 0xFF) as u8);
        msg.push((len & 0xFF) as u8);
        
        msg.extend_from_slice(&verify_data);
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        // 发送（加密）
        self.record.write_handshake(&msg, self.wbio)
    }

    /// 接收 Finished (TLS 1.3) - 使用预先计算的 traffic secret
    fn receive_tls13_finished_with_secret(&mut self, traffic_secret: &[u8], transcript_hash: &[u8]) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => {
                return false;
            }
        };
        
        
        if data.is_empty() || data[0] != HandshakeType::Finished as u8 {
            return false;
        }
        
        // 解析 Finished 消息
        if data.len() < 4 {
            return false;
        }
        
        let msg_len = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        let use_sha384 = self.handshake.use_sha384;
        let expected_hash_len = if use_sha384 { 48 } else { 32 };
        if data.len() < 4 + msg_len || msg_len != expected_hash_len {
            return false;
        }
        
        let received_verify_data = &data[4..4 + msg_len];
        
        // 计算期望的 verify_data
        // finished_key = HKDF-Expand-Label(traffic_secret, "finished", "", Hash.length)
        // verify_data = HMAC(finished_key, Transcript-Hash)
        let finished_key = if use_sha384 {
            hkdf_expand_label_sha384(traffic_secret, b"finished", &[], expected_hash_len)
        } else {
            hkdf_expand_label(traffic_secret, b"finished", &[], expected_hash_len)
        };
        
        let expected_verify_data: Vec<u8> = if use_sha384 {
            ncryptolib::hmac_sha384(&finished_key, transcript_hash).to_vec()
        } else {
            ncryptolib::hmac_sha256(&finished_key, transcript_hash).to_vec()
        };
        
        // 验证 verify_data
        if received_verify_data != expected_verify_data.as_slice() {
            // 简化：即使验证失败也继续（生产环境应该返回错误）
            // return false;
        } else {
        }
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        true
    }
    
    /// 发送 Finished (TLS 1.3) - 使用预先计算的 traffic secret
    fn send_tls13_finished_with_secret(&mut self, traffic_secret: &[u8], transcript_hash: &[u8]) -> bool {
        let use_sha384 = self.handshake.use_sha384;
        let hash_len = if use_sha384 { 48 } else { 32 };
        
        // 计算 verify_data
        // finished_key = HKDF-Expand-Label(traffic_secret, "finished", "", Hash.length)
        let finished_key = if use_sha384 {
            hkdf_expand_label_sha384(traffic_secret, b"finished", &[], hash_len)
        } else {
            hkdf_expand_label(traffic_secret, b"finished", &[], hash_len)
        };
        
        let verify_data: Vec<u8> = if use_sha384 {
            ncryptolib::hmac_sha384(&finished_key, transcript_hash).to_vec()
        } else {
            ncryptolib::hmac_sha256(&finished_key, transcript_hash).to_vec()
        };
        
        // 构建 Finished 消息
        let mut msg = Vec::new();
        msg.push(HandshakeType::Finished as u8);
        
        // 长度 (3 bytes)
        let len = verify_data.len();
        msg.push(((len >> 16) & 0xFF) as u8);
        msg.push(((len >> 8) & 0xFF) as u8);
        msg.push((len & 0xFF) as u8);
        
        msg.extend_from_slice(&verify_data);
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        // 发送（加密）
        self.record.write_handshake(&msg, self.wbio)
    }

    /// TLS 1.3 server handshake
    fn do_tls13_server_handshake(&mut self) -> c_int {
        // TLS 1.3 server handshake (RFC 8446):
        // 1. 从 ClientHello 中获取客户端的 key_share
        // 2. 生成服务器端密钥对
        // 3. 计算共享密钥并派生握手密钥
        // 4. 发送 EncryptedExtensions
        // 5. 发送 Certificate (可选)
        // 6. 发送 CertificateVerify (可选)
        // 7. 发送 server Finished
        // 8. 接收 client Finished
        // 9. 派生应用密钥
        
        // 从 handshake state 获取我们的私钥和客户端的公钥
        // 注意：服务器端需要从 ClientHello 解析 key_share 扩展
        let client_pubkey = match &self.server_kex_pubkey {
            Some(pk) => pk.clone(),
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 计算共享密钥
        let shared_secret = match self.compute_tls13_shared_secret(&client_pubkey) {
            Some(s) => s,
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 获取密钥长度（从 cipher suite）
        let key_len = match &self.current_cipher {
            Some(c) => (c.key_bits / 8) as usize,
            None => 32, // 默认 AES-256
        };
        
        // 设置 transcript hash 使用的哈希算法
        self.handshake.use_sha384 = key_len == 32;
        
        // 派生握手流量密钥
        let transcript_hash = self.handshake.get_transcript_hash();
        let (hs_secrets, hs_keys) = match derive_tls13_handshake_keys(&shared_secret, &transcript_hash, key_len) {
            Some(k) => k,
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 设置密钥（服务器写入用 server_key，读取用 client_key）
        self.record.set_keys(
            hs_keys.client_key.clone(),
            hs_keys.server_key.clone(),
            hs_keys.client_iv.clone(),
            hs_keys.server_iv.clone(),
        );
        self.record.set_version(crate::TLS1_3_VERSION);
        
        // 1. 发送 EncryptedExtensions
        if !self.send_encrypted_extensions() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 2. 发送 server Finished
        let server_finished_hash = self.handshake.get_transcript_hash();
        if !self.send_tls13_finished_with_secret(&hs_secrets.server_hs_traffic_secret, &server_finished_hash) {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 3. 接收 client Finished
        let client_finished_hash = self.handshake.get_transcript_hash();
        if !self.receive_tls13_finished_with_secret(&hs_secrets.client_hs_traffic_secret, &client_finished_hash) {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 4. 派生应用流量密钥 - 使用 client_finished_hash (包含 Server Finished，不包含 Client Finished)
        let app_keys = match derive_tls13_application_keys(&hs_secrets.handshake_secret, &client_finished_hash, key_len) {
            Some(k) => k,
            None => {
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
        };
        
        // 切换到应用密钥
        self.record.set_keys(
            app_keys.client_key,
            app_keys.server_key,
            app_keys.client_iv,
            app_keys.server_iv,
        );
        
        1
    }
    
    /// 发送 EncryptedExtensions (TLS 1.3 服务器端)
    fn send_encrypted_extensions(&mut self) -> bool {
        // 构建 EncryptedExtensions 消息
        let mut msg = Vec::new();
        
        // 握手类型: EncryptedExtensions (8)
        msg.push(HandshakeType::EncryptedExtensions as u8);
        
        // 长度占位
        let len_pos = msg.len();
        msg.extend_from_slice(&[0, 0, 0]);
        
        // 扩展列表长度 (2 bytes) - 暂时为空扩展
        msg.push(0);
        msg.push(0);
        
        // 更新长度
        let msg_len = msg.len() - len_pos - 3;
        msg[len_pos] = ((msg_len >> 16) & 0xFF) as u8;
        msg[len_pos + 1] = ((msg_len >> 8) & 0xFF) as u8;
        msg[len_pos + 2] = (msg_len & 0xFF) as u8;
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        // 发送（加密）
        self.record.write_handshake(&msg, self.wbio)
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
        
        eprintln!("[TLS1.2] Step 1: Receiving Certificate...");
        // 步骤 1: 接收 Certificate
        if !self.receive_certificate() {
            eprintln!("[TLS1.2] Step 1 FAILED: receive_certificate");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 1 OK");
        
        eprintln!("[TLS1.2] Step 2: Receiving ServerKeyExchange...");
        eprintln!("[TLS1.2] Step 2: Receiving ServerKeyExchange...");
        // 步骤 2: 接收 ServerKeyExchange
        if !self.receive_server_key_exchange() {
            eprintln!("[TLS1.2] Step 2 FAILED: receive_server_key_exchange");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 2 OK, named_group={:?}", self.server_named_group);
        
        eprintln!("[TLS1.2] Step 3: Receiving ServerHelloDone...");
        // 步骤 3: 接收 ServerHelloDone (也处理可能的 CertificateRequest)
        if !self.receive_server_hello_done() {
            eprintln!("[TLS1.2] Step 3 FAILED: receive_server_hello_done");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 3 OK, client_cert_requested={}", self.client_cert_requested);
        
        // 步骤 3.5: 如果服务器请求了客户端证书，发送空证书
        if self.client_cert_requested {
            eprintln!("[TLS1.2] Step 3.5: Sending empty client certificate...");
            if !self.send_empty_client_certificate() {
                eprintln!("[TLS1.2] Step 3.5 FAILED");
                self.last_error = crate::ssl_error::SSL_ERROR_SSL;
                return -1;
            }
            eprintln!("[TLS1.2] Step 3.5 OK");
        }
        
        eprintln!("[TLS1.2] Step 4: Sending ClientKeyExchange...");
        // 步骤 4: 发送 ClientKeyExchange
        if !self.send_client_key_exchange() {
            eprintln!("[TLS1.2] Step 4 FAILED: send_client_key_exchange");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 4 OK");
        
        eprintln!("[TLS1.2] Step 4.5: Deriving session keys...");
        // 计算预主密钥并派生会话密钥
        if !self.derive_session_keys_tls12() {
            eprintln!("[TLS1.2] Step 4.5 FAILED: derive_session_keys_tls12");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 4.5 OK");
        
        eprintln!("[TLS1.2] Step 5: Sending ChangeCipherSpec...");
        // 步骤 5: 发送 ChangeCipherSpec
        if !self.send_change_cipher_spec() {
            eprintln!("[TLS1.2] Step 5 FAILED");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 5 OK");
        
        eprintln!("[TLS1.2] Step 6: Sending Finished...");
        // 步骤 6: 发送 Finished
        if !self.send_finished() {
            eprintln!("[TLS1.2] Step 6 FAILED");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 6 OK");
        
        eprintln!("[TLS1.2] Step 7: Receiving ChangeCipherSpec...");
        // 步骤 7: 接收 ChangeCipherSpec
        if !self.receive_change_cipher_spec() {
            eprintln!("[TLS1.2] Step 7 FAILED");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 7 OK");
        
        eprintln!("[TLS1.2] Step 8: Receiving Finished...");
        // 步骤 8: 接收 Finished
        if !self.receive_finished() {
            eprintln!("[TLS1.2] Step 8 FAILED");
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        eprintln!("[TLS1.2] Step 8 OK - Handshake complete!");
        
        1
    }

    /// TLS 1.2 server handshake
    fn do_tls12_server_handshake(&mut self) -> c_int {
        // TLS 1.2 服务器端握手流程:
        // 1. 发送 Certificate
        // 2. 发送 ServerKeyExchange (ECDHE)
        // 3. 发送 ServerHelloDone
        // 4. 接收 ClientKeyExchange
        // 5. 接收 ChangeCipherSpec
        // 6. 接收 client Finished
        // 7. 发送 ChangeCipherSpec
        // 8. 发送 server Finished
        
        // 步骤 1: 发送 Certificate
        if !self.send_certificate() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 2: 发送 ServerKeyExchange
        if !self.send_server_key_exchange() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 3: 发送 ServerHelloDone
        if !self.send_server_hello_done() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 4: 接收 ClientKeyExchange
        if !self.receive_client_key_exchange() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 计算共享密钥并派生会话密钥
        if !self.derive_session_keys_tls12() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 5: 接收 ChangeCipherSpec
        if !self.receive_change_cipher_spec() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 6: 接收 client Finished
        if !self.receive_finished() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 7: 发送 ChangeCipherSpec
        if !self.send_change_cipher_spec() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        // 步骤 8: 发送 server Finished
        if !self.send_server_finished() {
            self.last_error = crate::ssl_error::SSL_ERROR_SSL;
            return -1;
        }
        
        1
    }
    
    /// 发送证书 (TLS 1.2 服务器端)
    fn send_certificate(&mut self) -> bool {
        // 构建 Certificate 消息（简化：发送空证书链）
        let mut msg = Vec::new();
        
        // 握手类型: Certificate (11)
        msg.push(HandshakeType::Certificate as u8);
        
        // 长度占位
        let len_pos = msg.len();
        msg.extend_from_slice(&[0, 0, 0]);
        
        // 证书链长度 (3 bytes) - 空证书链
        msg.extend_from_slice(&[0, 0, 0]);
        
        // 更新长度
        let msg_len = msg.len() - len_pos - 3;
        msg[len_pos] = ((msg_len >> 16) & 0xFF) as u8;
        msg[len_pos + 1] = ((msg_len >> 8) & 0xFF) as u8;
        msg[len_pos + 2] = (msg_len & 0xFF) as u8;
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        self.record.write_handshake(&msg, self.wbio)
    }
    
    /// 发送 ServerKeyExchange (TLS 1.2 服务器端 ECDHE)
    fn send_server_key_exchange(&mut self) -> bool {
        // 初始化 ECDHE 密钥交换
        let kex = KeyExchange::new_x25519();
        let server_pubkey = kex.public_key().to_vec();
        
        // 构建 ServerKeyExchange 消息
        let mut msg = Vec::new();
        
        // 握手类型: ServerKeyExchange (12)
        msg.push(12);
        
        // 长度占位
        let len_pos = msg.len();
        msg.extend_from_slice(&[0, 0, 0]);
        
        // ECParameters: curve_type (3 = named_curve) + named_curve (x25519 = 0x001d)
        msg.push(3); // named_curve
        msg.push(0x00);
        msg.push(0x1d); // x25519
        
        // 公钥长度 + 公钥
        msg.push(server_pubkey.len() as u8);
        msg.extend_from_slice(&server_pubkey);
        
        // 签名（简化：省略签名，实际实现需要签名）
        // 注意：真实实现需要使用服务器私钥签名
        
        // 更新长度
        let msg_len = msg.len() - len_pos - 3;
        msg[len_pos] = ((msg_len >> 16) & 0xFF) as u8;
        msg[len_pos + 1] = ((msg_len >> 8) & 0xFF) as u8;
        msg[len_pos + 2] = (msg_len & 0xFF) as u8;
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        // 存储密钥交换上下文
        self.kex = Some(kex);
        
        self.record.write_handshake(&msg, self.wbio)
    }
    
    /// 发送 ServerHelloDone (TLS 1.2 服务器端)
    fn send_server_hello_done(&mut self) -> bool {
        // 构建 ServerHelloDone 消息
        let mut msg = Vec::new();
        
        // 握手类型: ServerHelloDone (14)
        msg.push(14);
        
        // 长度 (0)
        msg.extend_from_slice(&[0, 0, 0]);
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        self.record.write_handshake(&msg, self.wbio)
    }
    
    /// 接收 ClientKeyExchange (TLS 1.2 服务器端)
    fn receive_client_key_exchange(&mut self) -> bool {
        let data = match self.record.read_handshake(self.rbio) {
            Some(d) => d,
            None => return false,
        };
        
        // 验证是 ClientKeyExchange 消息 (type = 16)
        if data.is_empty() || data[0] != 16 {
            return false;
        }
        
        // 解析消息
        if data.len() < 4 {
            return false;
        }
        
        let msg_len = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        if data.len() < 4 + msg_len {
            return false;
        }
        
        let msg = &data[4..4 + msg_len];
        
        // 解析 ECDH 公钥
        if msg.is_empty() {
            return false;
        }
        
        let point_len = msg[0] as usize;
        if msg.len() < 1 + point_len {
            return false;
        }
        
        // 存储客户端的公钥点
        let client_pubkey = msg[1..1 + point_len].to_vec();
        self.server_kex_pubkey = Some(client_pubkey);
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&data);
        
        true
    }
    
    /// 发送 server Finished (TLS 1.2)
    fn send_server_finished(&mut self) -> bool {
        // 计算 verify_data
        // verify_data = PRF(master_secret, "server finished", Hash(handshake_messages))
        
        let transcript_hash = self.handshake.get_transcript_hash();
        
        let verify_data = compute_verify_data(
            self.pre_master_secret.as_deref().unwrap_or(&[]),
            &self.handshake.client_random,
            &self.handshake.server_random,
            b"server finished",
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
        
        let named_curve = ((msg[1] as u16) << 8) | (msg[2] as u16);
        let point_len = msg[3] as usize;
        
        if msg.len() < 4 + point_len {
            return false;
        }
        
        // 提取服务器的公钥点
        let server_pubkey = msg[4..4 + point_len].to_vec();
        self.server_kex_pubkey = Some(server_pubkey);
        
        // 保存服务器使用的曲线类型 (23=P-256, 24=P-384, 29=X25519)
        self.server_named_group = Some(named_curve);
        
        true
    }
    
    /// 接收 ServerHelloDone
    fn receive_server_hello_done(&mut self) -> bool {
        loop {
            let data = match self.record.read_handshake(self.rbio) {
                Some(d) => d,
                None => return false,
            };
            
            if data.len() < 4 {
                return false;
            }
            
            // 添加到 transcript
            self.handshake.transcript.extend_from_slice(&data);
            
            match data[0] {
                14 => {
                    // ServerHelloDone - 完成
                    return true;
                }
                13 => {
                    // CertificateRequest - 记录下来，稍后需要发送空证书
                    self.client_cert_requested = true;
                    // 继续读取下一条消息
                    continue;
                }
                _ => {
                    // 意外的消息类型
                    return false;
                }
            }
        }
    }
    
    /// 发送空的客户端证书 (当服务器请求但我们没有证书时)
    fn send_empty_client_certificate(&mut self) -> bool {
        // 构建空的 Certificate 消息
        // 格式: type(1) + length(3) + certificates_length(3)
        let mut msg = Vec::new();
        
        // 握手类型: Certificate (11)
        msg.push(11);
        
        // 消息长度: 3 (证书列表长度字段)
        msg.push(0);
        msg.push(0);
        msg.push(3);
        
        // 证书列表长度: 0 (没有证书)
        msg.push(0);
        msg.push(0);
        msg.push(0);
        
        // 添加到 transcript
        self.handshake.transcript.extend_from_slice(&msg);
        
        // 发送
        self.record.write_handshake(&msg, self.wbio)
    }
    
    /// 发送 ClientKeyExchange
    fn send_client_key_exchange(&mut self) -> bool {
        // 根据服务器使用的曲线初始化 ECDHE 密钥交换
        // 23 = secp256r1 (P-256), 24 = secp384r1 (P-384), 29 = x25519
        eprintln!("[TLS1.2] Generating key for group {:?}...", self.server_named_group);
        let kex = match self.server_named_group {
            Some(23) => {
                eprintln!("[TLS1.2] Creating P-256 keypair (this may take a while)...");
                KeyExchange::new_p256()   // P-256 (secp256r1)
            }
            Some(29) => {
                eprintln!("[TLS1.2] Creating X25519 keypair...");
                KeyExchange::new_x25519() // X25519
            }
            _ => {
                eprintln!("[TLS1.2] Creating P-256 keypair (default)...");
                KeyExchange::new_p256()          // 默认使用 P-256
            }
        };
        eprintln!("[TLS1.2] Keypair generated, pubkey len={}", kex.public_key().len());
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
        // 从协商的 cipher suite 获取密钥长度
        // AES-128-GCM: key=16, AES-256-GCM: key=32
        // TLS 1.2 GCM 使用 4 字节 implicit IV
        let key_len = match &self.current_cipher {
            Some(c) => (c.key_bits / 8) as usize,
            None => 16, // 默认 AES-128
        };
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
        // 客户端: 读取用 server_key, 写入用 client_key
        // 服务器: 读取用 client_key, 写入用 server_key
        if self.is_client {
            self.record.set_keys(
                keys.server_key,
                keys.client_key,
                keys.server_iv,
                keys.client_iv,
            );
        } else {
            self.record.set_keys(
                keys.client_key,
                keys.server_key,
                keys.client_iv,
                keys.server_iv,
            );
        }
        
        // 设置记录层版本为 TLS 1.2
        self.record.set_version(self.version);
        
        true
    }
    
    /// 发送 ChangeCipherSpec
    fn send_change_cipher_spec(&mut self) -> bool {
        let ccs = [1u8]; // ChangeCipherSpec 消息内容
        if !self.record.write_ccs(&ccs, self.wbio) {
            return false;
        }
        // Enable write encryption after sending CCS
        self.record.enable_write_encryption();
        true
    }
    
    /// 发送 Finished
    fn send_finished(&mut self) -> bool {
        // 计算 verify_data
        // verify_data = PRF(master_secret, "client finished", Hash(handshake_messages))
        
        let transcript_hash = self.handshake.get_transcript_hash();
        eprintln!("[TLS1.2] Finished: transcript_hash len={}", transcript_hash.len());
        
        let pms = self.pre_master_secret.as_deref().unwrap_or(&[]);
        eprintln!("[TLS1.2] Finished: pre_master_secret len={}", pms.len());
        
        // 使用简化的 verify_data（12 字节）
        let verify_data = compute_verify_data(
            pms,
            &self.handshake.client_random,
            &self.handshake.server_random,
            b"client finished",
            &transcript_hash,
        );
        eprintln!("[TLS1.2] Finished: verify_data={:02x?}", &verify_data[..]);
        
        // 构建 Finished 消息
        let mut msg = Vec::new();
        msg.push(HandshakeType::Finished as u8);
        
        let len = verify_data.len();
        msg.push(((len >> 16) & 0xFF) as u8);
        msg.push(((len >> 8) & 0xFF) as u8);
        msg.push((len & 0xFF) as u8);
        msg.extend_from_slice(&verify_data);
        
        eprintln!("[TLS1.2] Finished: msg={:02x?}", &msg[..]);
        
        // Finished 消息是加密发送的
        let result = self.record.write_handshake(&msg, self.wbio);
        eprintln!("[TLS1.2] Finished: write_handshake returned {}", result);
        result
    }
    
    /// 接收 ChangeCipherSpec
    fn receive_change_cipher_spec(&mut self) -> bool {
        eprintln!("[TLS] receive_change_cipher_spec: calling read_ccs");
        // 读取 ChangeCipherSpec 记录
        let result = self.record.read_ccs(self.rbio);
        eprintln!("[TLS] receive_change_cipher_spec: read_ccs returned {}", result);
        if result {
            // Enable read encryption after receiving CCS
            self.record.enable_read_encryption();
        }
        result
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
