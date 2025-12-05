//! SSL Context
//!
//! SSL_CTX holds configuration shared across multiple SSL connections.

use std::vec::Vec;
use std::string::String;

use crate::ssl::SslMethod;
use crate::connection::SslConnection;
use crate::cipher::CipherList;
use crate::x509::{X509, X509Store, X509StoreCtx};
use crate::error::{SslError, SslResult};
use crate::{c_int, c_ulong, TLS1_2_VERSION, TLS1_3_VERSION};

/// Verification callback type
pub type VerifyCallback = Option<extern "C" fn(c_int, *mut X509StoreCtx) -> c_int>;

/// SSL Context - holds shared configuration
pub struct SslContext {
    /// SSL method (defines protocol version and mode)
    method: SslMethod,
    
    /// Options flags
    options: u64,
    
    /// Mode flags
    mode: u64,
    
    /// Minimum protocol version
    min_version: u16,
    
    /// Maximum protocol version
    max_version: u16,
    
    /// TLS 1.2 cipher list
    cipher_list: CipherList,
    
    /// TLS 1.3 ciphersuites
    ciphersuites: Vec<u16>,
    
    /// Verification mode
    verify_mode: i32,
    
    /// Verification depth
    verify_depth: usize,
    
    /// Verification callback
    verify_callback: VerifyCallback,
    
    /// Certificate chain
    cert_chain: Vec<X509>,
    
    /// Private key (DER encoded)
    private_key: Option<Vec<u8>>,
    
    /// CA certificate store
    cert_store: X509Store,
    
    /// ALPN protocols
    alpn_protos: Vec<u8>,
    
    /// Session timeout (seconds)
    session_timeout: u64,
    
    /// Session cache mode
    session_cache_mode: i32,
}

impl SslContext {
    /// Create a new SSL context with the given method
    pub fn new(method: &SslMethod) -> SslResult<Self> {
        let mut ctx = Self {
            method: SslMethod {
                method_type: method.method_type,
                min_version: method.min_version,
                max_version: method.max_version,
                is_client: method.is_client,
                is_server: method.is_server,
            },
            options: crate::ssl_options::SSL_OP_ALL,
            mode: 0,
            min_version: method.min_version.max(TLS1_2_VERSION),
            max_version: method.max_version.min(TLS1_3_VERSION),
            cipher_list: CipherList::default(),
            ciphersuites: default_tls13_ciphersuites(),
            verify_mode: crate::ssl_verify::SSL_VERIFY_NONE,
            verify_depth: 100,
            verify_callback: None,
            cert_chain: Vec::new(),
            private_key: None,
            cert_store: X509Store::new(),
            alpn_protos: Vec::new(),
            session_timeout: 300,
            session_cache_mode: 0,
        };
        
        // Set default cipher list
        ctx.cipher_list = CipherList::default_secure();
        
        Ok(ctx)
    }

    /// Set options
    pub fn set_options(&mut self, options: u64) -> u64 {
        self.options |= options;
        // Always enforce minimum security
        self.options |= crate::ssl_options::SSL_OP_NO_SSLv2;
        self.options |= crate::ssl_options::SSL_OP_NO_SSLv3;
        self.options |= crate::ssl_options::SSL_OP_NO_TLSv1;
        self.options |= crate::ssl_options::SSL_OP_NO_TLSv1_1;
        self.options
    }

    /// Get options
    pub fn get_options(&self) -> u64 {
        self.options
    }

    /// Clear options
    pub fn clear_options(&mut self, options: u64) -> u64 {
        // Don't allow clearing security-critical options
        let safe_mask = !(crate::ssl_options::SSL_OP_NO_SSLv2 |
                         crate::ssl_options::SSL_OP_NO_SSLv3 |
                         crate::ssl_options::SSL_OP_NO_TLSv1 |
                         crate::ssl_options::SSL_OP_NO_TLSv1_1);
        self.options &= !(options & safe_mask);
        self.options
    }

    /// Set minimum protocol version
    pub fn set_min_proto_version(&mut self, version: u16) -> bool {
        // Enforce minimum TLS 1.2
        if version < TLS1_2_VERSION {
            self.min_version = TLS1_2_VERSION;
        } else if version > TLS1_3_VERSION {
            return false;
        } else {
            self.min_version = version;
        }
        true
    }

    /// Set maximum protocol version
    pub fn set_max_proto_version(&mut self, version: u16) -> bool {
        if version < self.min_version || version > TLS1_3_VERSION {
            return false;
        }
        self.max_version = version;
        true
    }

    /// Set TLS 1.2 cipher list
    pub fn set_cipher_list(&mut self, cipher_str: &str) -> bool {
        match CipherList::from_string(cipher_str) {
            Some(list) => {
                self.cipher_list = list;
                true
            }
            None => false,
        }
    }

    /// Set TLS 1.3 ciphersuites
    pub fn set_ciphersuites(&mut self, cipher_str: &str) -> bool {
        let suites = parse_tls13_ciphersuites(cipher_str);
        if suites.is_empty() {
            return false;
        }
        self.ciphersuites = suites;
        true
    }

    /// Set verification mode
    pub fn set_verify(&mut self, mode: i32, callback: VerifyCallback) {
        self.verify_mode = mode;
        self.verify_callback = callback;
    }

    /// Set verification depth
    pub fn set_verify_depth(&mut self, depth: usize) {
        self.verify_depth = depth;
    }

    /// Load certificate from file
    pub fn use_certificate_file(&mut self, path: &str, file_type: i32) -> bool {
        match X509::load_from_file(path, file_type) {
            Some(cert) => {
                self.cert_chain.clear();
                self.cert_chain.push(cert);
                true
            }
            None => false,
        }
    }

    /// Load certificate chain from file
    pub fn use_certificate_chain_file(&mut self, path: &str) -> bool {
        match X509::load_chain_from_file(path) {
            Some(chain) => {
                self.cert_chain = chain;
                true
            }
            None => false,
        }
    }

    /// Load private key from file
    pub fn use_private_key_file(&mut self, path: &str, file_type: i32) -> bool {
        match load_private_key_file(path, file_type) {
            Some(key) => {
                self.private_key = Some(key);
                true
            }
            None => false,
        }
    }

    /// Check that private key matches certificate
    pub fn check_private_key(&self) -> bool {
        // Simplified check - in real implementation would verify key matches cert
        self.private_key.is_some() && !self.cert_chain.is_empty()
    }

    /// Load CA certificates
    pub fn load_verify_locations(&mut self, ca_file: Option<&str>, ca_path: Option<&str>) -> bool {
        let mut loaded = false;
        
        if let Some(file) = ca_file {
            if self.cert_store.load_file(file) {
                loaded = true;
            }
        }
        
        if let Some(path) = ca_path {
            if self.cert_store.load_path(path) {
                loaded = true;
            }
        }
        
        loaded
    }

    /// Set default CA certificate paths
    pub fn set_default_verify_paths(&mut self) -> bool {
        // Standard CA paths (files only, as directory traversal may not be supported)
        let paths = [
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/ssl/ca-bundle.pem",
            "/etc/ssl/cert.pem",
            "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        ];
        
        for path in &paths {
            if self.cert_store.load_file(path) {
                return true;
            }
        }
        
        // Note: Directory loading (/etc/ssl/certs) requires opendir/readdir
        // which may not be available on all targets
        false
    }

    /// Set ALPN protocols
    pub fn set_alpn_protos(&mut self, protos: &[u8]) -> bool {
        self.alpn_protos = protos.to_vec();
        true
    }

    /// Create a new SSL connection from this context
    pub fn new_ssl(&self) -> SslResult<SslConnection> {
        SslConnection::new(self)
    }

    /// Get the method
    pub fn get_method(&self) -> &SslMethod {
        &self.method
    }

    /// Get minimum version
    pub fn get_min_version(&self) -> u16 {
        self.min_version
    }

    /// Get maximum version
    pub fn get_max_version(&self) -> u16 {
        self.max_version
    }

    /// Get cipher list
    pub fn get_cipher_list(&self) -> &CipherList {
        &self.cipher_list
    }

    /// Get TLS 1.3 ciphersuites
    pub fn get_ciphersuites(&self) -> &[u16] {
        &self.ciphersuites
    }

    /// Get verify mode
    pub fn get_verify_mode(&self) -> i32 {
        self.verify_mode
    }

    /// Get verify depth
    pub fn get_verify_depth(&self) -> usize {
        self.verify_depth
    }

    /// Get verify callback
    pub fn get_verify_callback(&self) -> VerifyCallback {
        self.verify_callback
    }

    /// Get certificate chain
    pub fn get_cert_chain(&self) -> &[X509] {
        &self.cert_chain
    }

    /// Get private key
    pub fn get_private_key(&self) -> Option<&[u8]> {
        self.private_key.as_deref()
    }

    /// Get certificate store
    pub fn get_cert_store(&self) -> &X509Store {
        &self.cert_store
    }

    /// Get ALPN protocols
    pub fn get_alpn_protos(&self) -> &[u8] {
        &self.alpn_protos
    }
}

/// Default TLS 1.3 ciphersuites
fn default_tls13_ciphersuites() -> Vec<u16> {
    vec![
        0x1302, // TLS_AES_256_GCM_SHA384
        0x1301, // TLS_AES_128_GCM_SHA256
        0x1303, // TLS_CHACHA20_POLY1305_SHA256
    ]
}

/// Parse TLS 1.3 ciphersuite string
fn parse_tls13_ciphersuites(s: &str) -> Vec<u16> {
    let mut suites = Vec::new();
    
    for name in s.split(':') {
        match name.trim() {
            "TLS_AES_256_GCM_SHA384" => suites.push(0x1302),
            "TLS_AES_128_GCM_SHA256" => suites.push(0x1301),
            "TLS_CHACHA20_POLY1305_SHA256" => suites.push(0x1303),
            _ => {}
        }
    }
    
    suites
}

/// Load private key from file
fn load_private_key_file(path: &str, file_type: i32) -> Option<Vec<u8>> {
    use std::fs;
    use std::io::Read;
    
    let mut file = fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    
    if file_type == crate::SSL_FILETYPE_PEM {
        // Parse PEM format
        parse_pem_private_key(&data)
    } else {
        // DER format - use as-is
        Some(data)
    }
}

/// Parse PEM encoded private key
fn parse_pem_private_key(data: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(data).ok()?;
    
    // Find private key block
    let start_markers = [
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
    ];
    
    let end_markers = [
        "-----END PRIVATE KEY-----",
        "-----END RSA PRIVATE KEY-----",
        "-----END EC PRIVATE KEY-----",
    ];
    
    for (start, end) in start_markers.iter().zip(end_markers.iter()) {
        if let Some(start_idx) = text.find(start) {
            if let Some(end_idx) = text.find(end) {
                let base64_start = start_idx + start.len();
                let base64_data = &text[base64_start..end_idx];
                let cleaned: String = base64_data.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                return ncryptolib::base64_decode(&cleaned).ok();
            }
        }
    }
    
    None
}
