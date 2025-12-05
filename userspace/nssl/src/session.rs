//! TLS Session Management
//!
//! Implements session caching and resumption for TLS 1.2 and TLS 1.3.

use std::vec::Vec;
use std::collections::HashMap;

/// TLS Session
#[derive(Clone)]
pub struct SslSession {
    /// Session ID (TLS 1.2)
    session_id: Vec<u8>,
    /// Session ticket (TLS 1.2) or PSK identity (TLS 1.3)
    ticket: Vec<u8>,
    /// Master secret (TLS 1.2) or resumption master secret (TLS 1.3)
    master_secret: Vec<u8>,
    /// Protocol version
    version: u16,
    /// Cipher suite ID
    cipher_suite: u16,
    /// Creation time (Unix timestamp)
    creation_time: u64,
    /// Expiration time (Unix timestamp)
    expiration_time: u64,
    /// Server hostname (for session binding)
    hostname: Option<String>,
    /// Maximum early data size (TLS 1.3)
    max_early_data: u32,
    /// Reference count
    ref_count: u32,
}

impl SslSession {
    /// Create new session
    pub fn new() -> Self {
        Self {
            session_id: Vec::new(),
            ticket: Vec::new(),
            master_secret: Vec::new(),
            version: 0,
            cipher_suite: 0,
            creation_time: 0,
            expiration_time: 0,
            hostname: None,
            max_early_data: 0,
            ref_count: 1,
        }
    }

    /// Create session from parameters
    pub fn from_params(
        session_id: Vec<u8>,
        master_secret: Vec<u8>,
        version: u16,
        cipher_suite: u16,
        lifetime: u64,
    ) -> Self {
        let now = get_current_time();
        Self {
            session_id,
            ticket: Vec::new(),
            master_secret,
            version,
            cipher_suite,
            creation_time: now,
            expiration_time: now + lifetime,
            hostname: None,
            max_early_data: 0,
            ref_count: 1,
        }
    }

    /// Get session ID
    pub fn get_id(&self) -> &[u8] {
        &self.session_id
    }

    /// Set session ID
    pub fn set_id(&mut self, id: &[u8]) {
        self.session_id = id.to_vec();
    }

    /// Get session ticket
    pub fn get_ticket(&self) -> &[u8] {
        &self.ticket
    }

    /// Set session ticket
    pub fn set_ticket(&mut self, ticket: &[u8]) {
        self.ticket = ticket.to_vec();
    }

    /// Get master secret
    pub fn get_master_secret(&self) -> &[u8] {
        &self.master_secret
    }

    /// Set master secret
    pub fn set_master_secret(&mut self, secret: &[u8]) {
        self.master_secret = secret.to_vec();
    }

    /// Get protocol version
    pub fn get_version(&self) -> u16 {
        self.version
    }

    /// Get cipher suite
    pub fn get_cipher_suite(&self) -> u16 {
        self.cipher_suite
    }

    /// Check if session is expired
    pub fn is_expired(&self) -> bool {
        get_current_time() > self.expiration_time
    }

    /// Check if session is valid
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && !self.master_secret.is_empty()
    }

    /// Get timeout (remaining lifetime)
    pub fn get_timeout(&self) -> u64 {
        if self.is_expired() {
            0
        } else {
            self.expiration_time - get_current_time()
        }
    }

    /// Set timeout
    pub fn set_timeout(&mut self, timeout: u64) {
        self.expiration_time = get_current_time() + timeout;
    }

    /// Get hostname
    pub fn get_hostname(&self) -> Option<&str> {
        self.hostname.as_deref()
    }

    /// Set hostname
    pub fn set_hostname(&mut self, hostname: &str) {
        self.hostname = Some(hostname.to_string());
    }

    /// Check if session matches hostname
    pub fn matches_hostname(&self, hostname: &str) -> bool {
        match &self.hostname {
            Some(h) => h == hostname,
            None => true, // No hostname restriction
        }
    }

    /// Get max early data size
    pub fn get_max_early_data(&self) -> u32 {
        self.max_early_data
    }

    /// Set max early data size
    pub fn set_max_early_data(&mut self, size: u32) {
        self.max_early_data = size;
    }

    /// Increment reference count
    pub fn up_ref(&mut self) {
        self.ref_count += 1;
    }

    /// Free session (decrement reference count)
    pub fn free(session: *mut SslSession) {
        if session.is_null() {
            return;
        }
        
        unsafe {
            (*session).ref_count -= 1;
            if (*session).ref_count == 0 {
                // Securely zero the master secret
                ncryptolib::secure_zero(&mut (*session).master_secret);
                drop(Box::from_raw(session));
            }
        }
    }
}

impl Default for SslSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Session cache
pub struct SessionCache {
    /// Sessions by ID
    by_id: HashMap<Vec<u8>, SslSession>,
    /// Sessions by hostname
    by_hostname: HashMap<String, Vec<u8>>,
    /// Maximum cache size
    max_size: usize,
    /// Session timeout (seconds)
    timeout: u64,
}

impl SessionCache {
    /// Create new session cache
    pub fn new(max_size: usize, timeout: u64) -> Self {
        Self {
            by_id: HashMap::new(),
            by_hostname: HashMap::new(),
            max_size,
            timeout,
        }
    }

    /// Add session to cache
    pub fn add(&mut self, session: SslSession) {
        // Remove expired sessions if cache is full
        if self.by_id.len() >= self.max_size {
            self.cleanup();
        }
        
        // Still full? Remove oldest
        if self.by_id.len() >= self.max_size {
            if let Some(oldest_id) = self.find_oldest() {
                self.remove(&oldest_id);
            }
        }
        
        let id = session.session_id.clone();
        
        // Add to hostname index
        if let Some(ref hostname) = session.hostname {
            self.by_hostname.insert(hostname.clone(), id.clone());
        }
        
        self.by_id.insert(id, session);
    }

    /// Get session by ID
    pub fn get(&self, id: &[u8]) -> Option<&SslSession> {
        self.by_id.get(id).filter(|s| s.is_valid())
    }

    /// Get session by hostname
    pub fn get_by_hostname(&self, hostname: &str) -> Option<&SslSession> {
        let id = self.by_hostname.get(hostname)?;
        self.get(id)
    }

    /// Remove session by ID
    pub fn remove(&mut self, id: &[u8]) -> Option<SslSession> {
        if let Some(session) = self.by_id.remove(id) {
            if let Some(ref hostname) = session.hostname {
                self.by_hostname.remove(hostname);
            }
            Some(session)
        } else {
            None
        }
    }

    /// Remove expired sessions
    pub fn cleanup(&mut self) {
        let expired: Vec<Vec<u8>> = self.by_id.iter()
            .filter(|(_, s)| s.is_expired())
            .map(|(id, _)| id.clone())
            .collect();
        
        for id in expired {
            self.remove(&id);
        }
    }

    /// Find oldest session ID
    fn find_oldest(&self) -> Option<Vec<u8>> {
        self.by_id.iter()
            .min_by_key(|(_, s)| s.creation_time)
            .map(|(id, _)| id.clone())
    }

    /// Get cache size
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Clear cache
    pub fn clear(&mut self) {
        self.by_id.clear();
        self.by_hostname.clear();
    }
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new(1024, 7200) // 1024 sessions, 2 hour timeout
    }
}

/// TLS 1.3 New Session Ticket
#[derive(Clone)]
pub struct NewSessionTicket {
    /// Ticket lifetime (seconds)
    pub lifetime: u32,
    /// Ticket age add (for obfuscation)
    pub age_add: u32,
    /// Ticket nonce
    pub nonce: Vec<u8>,
    /// Ticket value
    pub ticket: Vec<u8>,
    /// Extensions
    pub extensions: Vec<(u16, Vec<u8>)>,
}

impl NewSessionTicket {
    /// Parse from wire format
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }
        
        let lifetime = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let age_add = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        
        let nonce_len = data[8] as usize;
        if data.len() < 9 + nonce_len + 2 {
            return None;
        }
        
        let nonce = data[9..9 + nonce_len].to_vec();
        let pos = 9 + nonce_len;
        
        let ticket_len = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
        let pos = pos + 2;
        if data.len() < pos + ticket_len {
            return None;
        }
        
        let ticket = data[pos..pos + ticket_len].to_vec();
        
        Some(Self {
            lifetime,
            age_add,
            nonce,
            ticket,
            extensions: Vec::new(),
        })
    }

    /// Create session from ticket
    pub fn to_session(&self, resumption_master_secret: &[u8], cipher_suite: u16) -> SslSession {
        // Derive PSK from resumption master secret and nonce
        let psk = derive_psk(resumption_master_secret, &self.nonce);
        
        SslSession::from_params(
            Vec::new(), // No session ID for TLS 1.3
            psk,
            crate::TLS1_3_VERSION,
            cipher_suite,
            self.lifetime as u64,
        )
    }
}

/// Derive PSK from resumption master secret (TLS 1.3)
fn derive_psk(resumption_master_secret: &[u8], nonce: &[u8]) -> Vec<u8> {
    // HKDF-Expand-Label(resumption_master_secret, "resumption", nonce, Hash.length)
    hkdf_expand_label(resumption_master_secret, b"resumption", nonce, 32)
}

/// HKDF-Expand-Label (simplified)
fn hkdf_expand_label(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    let mut hkdf_label = Vec::new();
    
    hkdf_label.push((length >> 8) as u8);
    hkdf_label.push((length & 0xFF) as u8);
    
    let full_label_len = 6 + label.len();
    hkdf_label.push(full_label_len as u8);
    hkdf_label.extend_from_slice(b"tls13 ");
    hkdf_label.extend_from_slice(label);
    
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    
    // HKDF-Expand
    let mut result = Vec::new();
    let mut t = Vec::new();
    let mut counter = 1u8;
    
    while result.len() < length {
        let mut data = t.clone();
        data.extend_from_slice(&hkdf_label);
        data.push(counter);
        
        t = ncryptolib::hmac_sha256(secret, &data).to_vec();
        result.extend_from_slice(&t);
        counter += 1;
    }
    
    result.truncate(length);
    result
}

/// Get current Unix timestamp
fn get_current_time() -> u64 {
    // Would use syscall in real implementation
    1700000000
}
