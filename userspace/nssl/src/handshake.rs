//! TLS Handshake Protocol
//!
//! Implements TLS 1.2 and TLS 1.3 handshake messages.

use std::vec::Vec;
use crate::tls::{HandshakeType, ExtensionType, NamedGroup, SignatureScheme, DEFAULT_NAMED_GROUPS, DEFAULT_SIGNATURE_ALGORITHMS};
use crate::{TLS1_2_VERSION, TLS1_3_VERSION};

/// Handshake state machine
pub struct HandshakeState {
    /// Client random
    pub client_random: [u8; 32],
    /// Server random
    pub server_random: [u8; 32],
    /// Session ID
    pub session_id: Vec<u8>,
    /// Handshake hash context
    pub transcript: Vec<u8>,
    /// Handshake secret (for key derivation)
    pub handshake_secret: Option<Vec<u8>>,
    /// Master secret
    pub master_secret: Option<Vec<u8>>,
    /// Key share (for TLS 1.3)
    pub key_share_private: Option<Vec<u8>>,
    pub key_share_public: Option<Vec<u8>>,
    /// Use SHA-384 for transcript hash (for AES-256-GCM cipher suites)
    pub use_sha384: bool,
}

impl HandshakeState {
    pub fn new() -> Self {
        let mut client_random = [0u8; 32];
        let _ = ncryptolib::getrandom(&mut client_random, 0);
        
        Self {
            client_random,
            server_random: [0u8; 32],
            session_id: Vec::new(),
            transcript: Vec::new(),
            handshake_secret: None,
            master_secret: None,
            key_share_private: None,
            key_share_public: None,
            use_sha384: false,
        }
    }

    /// Build ClientHello message
    pub fn build_client_hello(
        &mut self,
        max_version: u16,
        tls13_suites: &[u16],
        tls12_suites: &[u16],
        hostname: Option<&str>,
        alpn: &[u8],
    ) -> Vec<u8> {
        let mut msg = Vec::new();
        
        // Handshake type
        msg.push(HandshakeType::ClientHello as u8);
        
        // Length placeholder (3 bytes)
        let length_pos = msg.len();
        msg.extend_from_slice(&[0, 0, 0]);
        
        // Legacy version (TLS 1.2 for compatibility)
        msg.push(0x03);
        msg.push(0x03);
        
        // Client random
        msg.extend_from_slice(&self.client_random);
        
        // Session ID
        self.session_id = vec![0u8; 32];
        let _ = ncryptolib::getrandom(&mut self.session_id, 0);
        msg.push(self.session_id.len() as u8);
        msg.extend_from_slice(&self.session_id);
        
        // Cipher suites
        let mut suites: Vec<u16> = Vec::new();
        if max_version >= TLS1_3_VERSION {
            suites.extend_from_slice(tls13_suites);
        }
        suites.extend_from_slice(tls12_suites);
        
        let suites_len = (suites.len() * 2) as u16;
        msg.push((suites_len >> 8) as u8);
        msg.push((suites_len & 0xFF) as u8);
        for suite in &suites {
            msg.push((suite >> 8) as u8);
            msg.push((suite & 0xFF) as u8);
        }
        
        // Compression methods (null only)
        msg.push(1); // Length
        msg.push(0); // Null compression
        
        // Extensions
        let extensions = self.build_client_hello_extensions(max_version, hostname, alpn);
        let ext_len = extensions.len() as u16;
        msg.push((ext_len >> 8) as u8);
        msg.push((ext_len & 0xFF) as u8);
        msg.extend_from_slice(&extensions);
        
        // Update length
        let length = (msg.len() - length_pos - 3) as u32;
        msg[length_pos] = ((length >> 16) & 0xFF) as u8;
        msg[length_pos + 1] = ((length >> 8) & 0xFF) as u8;
        msg[length_pos + 2] = (length & 0xFF) as u8;
        
        // Add to transcript
        self.transcript.extend_from_slice(&msg);
        
        msg
    }

    /// Build ClientHello extensions
    fn build_client_hello_extensions(&mut self, max_version: u16, hostname: Option<&str>, alpn: &[u8]) -> Vec<u8> {
        let mut ext = Vec::new();
        
        // SNI extension
        if let Some(name) = hostname {
            self.add_extension(&mut ext, ExtensionType::ServerName, |data| {
                let name_bytes = name.as_bytes();
                // Server name list length
                let list_len = name_bytes.len() + 3;
                data.push((list_len >> 8) as u8);
                data.push((list_len & 0xFF) as u8);
                // Name type: hostname
                data.push(0);
                // Name length
                data.push((name_bytes.len() >> 8) as u8);
                data.push((name_bytes.len() & 0xFF) as u8);
                data.extend_from_slice(name_bytes);
            });
        }
        
        // Supported versions (TLS 1.3)
        if max_version >= TLS1_3_VERSION {
            self.add_extension(&mut ext, ExtensionType::SupportedVersions, |data| {
                let versions: Vec<u16> = if max_version >= TLS1_3_VERSION {
                    vec![TLS1_3_VERSION, TLS1_2_VERSION]
                } else {
                    vec![TLS1_2_VERSION]
                };
                data.push((versions.len() * 2) as u8);
                for v in versions {
                    data.push((v >> 8) as u8);
                    data.push((v & 0xFF) as u8);
                }
            });
        }
        
        // Supported groups
        self.add_extension(&mut ext, ExtensionType::SupportedGroups, |data| {
            let groups = DEFAULT_NAMED_GROUPS;
            let len = (groups.len() * 2) as u16;
            data.push((len >> 8) as u8);
            data.push((len & 0xFF) as u8);
            for group in groups {
                let g = *group as u16;
                data.push((g >> 8) as u8);
                data.push((g & 0xFF) as u8);
            }
        });
        
        // Signature algorithms
        self.add_extension(&mut ext, ExtensionType::SignatureAlgorithms, |data| {
            let algs = DEFAULT_SIGNATURE_ALGORITHMS;
            let len = (algs.len() * 2) as u16;
            data.push((len >> 8) as u8);
            data.push((len & 0xFF) as u8);
            for alg in algs {
                let a = *alg as u16;
                data.push((a >> 8) as u8);
                data.push((a & 0xFF) as u8);
            }
        });
        
        // Key share (TLS 1.3)
        if max_version >= TLS1_3_VERSION {
            // Generate X25519 key pair
            let (private, public) = generate_x25519_keypair();
            self.key_share_private = Some(private);
            self.key_share_public = Some(public.clone());
            
            self.add_extension(&mut ext, ExtensionType::KeyShare, |data| {
                // Client key shares length
                let entry_len = 2 + 2 + public.len();
                data.push((entry_len >> 8) as u8);
                data.push((entry_len & 0xFF) as u8);
                
                // X25519 group
                data.push((NamedGroup::X25519 as u16 >> 8) as u8);
                data.push((NamedGroup::X25519 as u16 & 0xFF) as u8);
                
                // Key exchange length
                data.push((public.len() >> 8) as u8);
                data.push((public.len() & 0xFF) as u8);
                data.extend_from_slice(&public);
            });
            
            // PSK key exchange modes
            self.add_extension(&mut ext, ExtensionType::PskKeyExchangeModes, |data| {
                data.push(1); // Length
                data.push(1); // psk_dhe_ke
            });
        }
        
        // ALPN
        if !alpn.is_empty() {
            self.add_extension(&mut ext, ExtensionType::ApplicationLayerProtocolNegotiation, |data| {
                // ALPN protocols are already wire-encoded
                let len = alpn.len() as u16;
                data.push((len >> 8) as u8);
                data.push((len & 0xFF) as u8);
                data.extend_from_slice(alpn);
            });
        }
        
        ext
    }

    /// Add extension helper
    fn add_extension<F>(&self, buf: &mut Vec<u8>, ext_type: ExtensionType, build_data: F)
    where
        F: FnOnce(&mut Vec<u8>),
    {
        let ext_type_val = ext_type as u16;
        buf.push((ext_type_val >> 8) as u8);
        buf.push((ext_type_val & 0xFF) as u8);
        
        // Data length placeholder
        let len_pos = buf.len();
        buf.push(0);
        buf.push(0);
        
        // Build extension data
        let start = buf.len();
        build_data(buf);
        let data_len = (buf.len() - start) as u16;
        
        // Update length
        buf[len_pos] = (data_len >> 8) as u8;
        buf[len_pos + 1] = (data_len & 0xFF) as u8;
    }

    /// Parse ServerHello
    pub fn parse_server_hello(&mut self, data: &[u8]) -> Option<(u16, u16, Vec<(u16, Vec<u8>)>)> {
        if data.len() < 4 {
            return None;
        }
        
        // Check handshake type
        if data[0] != HandshakeType::ServerHello as u8 {
            return None;
        }
        
        // Parse length
        let length = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        if data.len() < 4 + length {
            return None;
        }
        
        let msg = &data[4..4 + length];
        
        // Version
        let legacy_version = ((msg[0] as u16) << 8) | (msg[1] as u16);
        
        // Server random
        self.server_random.copy_from_slice(&msg[2..34]);
        
        // Session ID
        let session_id_len = msg[34] as usize;
        let pos = 35 + session_id_len;
        
        // Cipher suite
        let cipher_suite = ((msg[pos] as u16) << 8) | (msg[pos + 1] as u16);
        let pos = pos + 2;
        
        // Compression method (should be 0)
        let _compression = msg[pos];
        let pos = pos + 1;
        
        // Parse extensions
        let mut extensions = Vec::new();
        let mut version = legacy_version;
        
        if pos < msg.len() {
            let ext_len = ((msg[pos] as usize) << 8) | (msg[pos + 1] as usize);
            let mut ext_pos = pos + 2;
            let ext_end = ext_pos + ext_len;
            
            while ext_pos + 4 <= ext_end {
                let ext_type = ((msg[ext_pos] as u16) << 8) | (msg[ext_pos + 1] as u16);
                let ext_data_len = ((msg[ext_pos + 2] as usize) << 8) | (msg[ext_pos + 3] as usize);
                ext_pos += 4;
                
                if ext_pos + ext_data_len > ext_end {
                    break;
                }
                
                let ext_data = msg[ext_pos..ext_pos + ext_data_len].to_vec();
                
                // Check for supported_versions extension
                if ext_type == ExtensionType::SupportedVersions as u16 && ext_data.len() >= 2 {
                    version = ((ext_data[0] as u16) << 8) | (ext_data[1] as u16);
                }
                
                extensions.push((ext_type, ext_data));
                ext_pos += ext_data_len;
            }
        }
        
        // Add to transcript
        self.transcript.extend_from_slice(data);
        
        Some((version, cipher_suite, extensions))
    }

    /// Parse ClientHello (server side)
    pub fn parse_client_hello(&mut self, data: &[u8]) -> Option<ClientHelloInfo> {
        if data.len() < 4 {
            return None;
        }
        
        if data[0] != HandshakeType::ClientHello as u8 {
            return None;
        }
        
        let length = ((data[1] as usize) << 16) | ((data[2] as usize) << 8) | (data[3] as usize);
        if data.len() < 4 + length {
            return None;
        }
        
        let msg = &data[4..4 + length];
        
        // Legacy version
        let legacy_version = ((msg[0] as u16) << 8) | (msg[1] as u16);
        
        // Client random
        let mut client_random = [0u8; 32];
        client_random.copy_from_slice(&msg[2..34]);
        
        // Session ID
        let session_id_len = msg[34] as usize;
        let session_id = msg[35..35 + session_id_len].to_vec();
        let pos = 35 + session_id_len;
        
        // Cipher suites
        let suites_len = ((msg[pos] as usize) << 8) | (msg[pos + 1] as usize);
        let mut cipher_suites = Vec::new();
        let mut suite_pos = pos + 2;
        for _ in 0..suites_len / 2 {
            let suite = ((msg[suite_pos] as u16) << 8) | (msg[suite_pos + 1] as u16);
            cipher_suites.push(suite);
            suite_pos += 2;
        }
        let pos = suite_pos;
        
        // Compression methods
        let comp_len = msg[pos] as usize;
        let pos = pos + 1 + comp_len;
        
        // Extensions
        let mut extensions = Vec::new();
        let mut supported_version = legacy_version;
        let mut sni = None;
        let mut alpn_protos = Vec::new();
        
        if pos < msg.len() {
            let ext_len = ((msg[pos] as usize) << 8) | (msg[pos + 1] as usize);
            let mut ext_pos = pos + 2;
            let ext_end = ext_pos + ext_len;
            
            while ext_pos + 4 <= ext_end && ext_pos + 4 <= msg.len() {
                let ext_type = ((msg[ext_pos] as u16) << 8) | (msg[ext_pos + 1] as u16);
                let ext_data_len = ((msg[ext_pos + 2] as usize) << 8) | (msg[ext_pos + 3] as usize);
                ext_pos += 4;
                
                if ext_pos + ext_data_len > ext_end || ext_pos + ext_data_len > msg.len() {
                    break;
                }
                
                let ext_data = &msg[ext_pos..ext_pos + ext_data_len];
                
                // Parse supported_versions
                if ext_type == ExtensionType::SupportedVersions as u16 {
                    if ext_data.len() >= 3 {
                        // First byte is list length
                        let versions_len = ext_data[0] as usize;
                        for i in (1..1 + versions_len).step_by(2) {
                            if i + 1 < ext_data.len() {
                                let v = ((ext_data[i] as u16) << 8) | (ext_data[i + 1] as u16);
                                if v > supported_version && v <= TLS1_3_VERSION {
                                    supported_version = v;
                                }
                            }
                        }
                    }
                }
                
                // Parse SNI
                if ext_type == ExtensionType::ServerName as u16 && ext_data.len() >= 5 {
                    let name_len = ((ext_data[3] as usize) << 8) | (ext_data[4] as usize);
                    if 5 + name_len <= ext_data.len() {
                        sni = std::str::from_utf8(&ext_data[5..5 + name_len]).ok().map(|s| s.to_string());
                    }
                }
                
                // Parse ALPN
                if ext_type == ExtensionType::ApplicationLayerProtocolNegotiation as u16 {
                    alpn_protos = ext_data.to_vec();
                }
                
                extensions.push((ext_type, ext_data.to_vec()));
                ext_pos += ext_data_len;
            }
        }
        
        // Store client random
        self.client_random = client_random;
        
        // Add to transcript
        self.transcript.extend_from_slice(data);
        
        Some(ClientHelloInfo {
            legacy_version,
            supported_version,
            client_random,
            session_id,
            cipher_suites,
            sni,
            alpn_protos,
            extensions,
        })
    }

    /// Build ServerHello message
    pub fn build_server_hello(&mut self, version: u16, cipher_suite: u16, alpn: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();
        
        // Generate server random
        let _ = ncryptolib::getrandom(&mut self.server_random, 0);
        
        // Handshake type
        msg.push(HandshakeType::ServerHello as u8);
        
        // Length placeholder
        let length_pos = msg.len();
        msg.extend_from_slice(&[0, 0, 0]);
        
        // Legacy version (0x0303 for TLS 1.2 compatibility)
        msg.push(0x03);
        msg.push(0x03);
        
        // Server random
        msg.extend_from_slice(&self.server_random);
        
        // Session ID (echo client's)
        msg.push(self.session_id.len() as u8);
        msg.extend_from_slice(&self.session_id);
        
        // Cipher suite
        msg.push((cipher_suite >> 8) as u8);
        msg.push((cipher_suite & 0xFF) as u8);
        
        // Compression method (null)
        msg.push(0);
        
        // Extensions
        let extensions = self.build_server_hello_extensions(version, alpn);
        let ext_len = extensions.len() as u16;
        msg.push((ext_len >> 8) as u8);
        msg.push((ext_len & 0xFF) as u8);
        msg.extend_from_slice(&extensions);
        
        // Update length
        let length = (msg.len() - length_pos - 3) as u32;
        msg[length_pos] = ((length >> 16) & 0xFF) as u8;
        msg[length_pos + 1] = ((length >> 8) & 0xFF) as u8;
        msg[length_pos + 2] = (length & 0xFF) as u8;
        
        // Add to transcript
        self.transcript.extend_from_slice(&msg);
        
        msg
    }

    /// Build ServerHello extensions
    fn build_server_hello_extensions(&mut self, version: u16, alpn: &[u8]) -> Vec<u8> {
        let mut ext = Vec::new();
        
        // Supported versions (TLS 1.3)
        if version >= TLS1_3_VERSION {
            self.add_extension(&mut ext, ExtensionType::SupportedVersions, |data| {
                data.push((version >> 8) as u8);
                data.push((version & 0xFF) as u8);
            });
            
            // Key share
            if let Some(ref public) = self.key_share_public {
                self.add_extension(&mut ext, ExtensionType::KeyShare, |data| {
                    // X25519 group
                    data.push((NamedGroup::X25519 as u16 >> 8) as u8);
                    data.push((NamedGroup::X25519 as u16 & 0xFF) as u8);
                    // Key length
                    data.push((public.len() >> 8) as u8);
                    data.push((public.len() & 0xFF) as u8);
                    data.extend_from_slice(public);
                });
            }
        }
        
        // ALPN
        if !alpn.is_empty() {
            self.add_extension(&mut ext, ExtensionType::ApplicationLayerProtocolNegotiation, |data| {
                let len = alpn.len() as u16;
                data.push((len >> 8) as u8);
                data.push((len & 0xFF) as u8);
                data.extend_from_slice(alpn);
            });
        }
        
        ext
    }

    /// Get transcript hash
    pub fn get_transcript_hash(&self) -> Vec<u8> {
        if self.use_sha384 {
            ncryptolib::sha384(&self.transcript).to_vec()
        } else {
            ncryptolib::sha256(&self.transcript).to_vec()
        }
    }
}

/// Parsed ClientHello information
pub struct ClientHelloInfo {
    pub legacy_version: u16,
    pub supported_version: u16,
    pub client_random: [u8; 32],
    pub session_id: Vec<u8>,
    pub cipher_suites: Vec<u16>,
    pub sni: Option<String>,
    pub alpn_protos: Vec<u8>,
    pub extensions: Vec<(u16, Vec<u8>)>,
}

/// Generate X25519 key pair
fn generate_x25519_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut private = [0u8; 32];
    let _ = ncryptolib::getrandom(&mut private, 0);
    
    // Clamp private key per RFC 7748
    private[0] &= 248;
    private[31] &= 127;
    private[31] |= 64;
    
    // Generate public key
    let public = ncryptolib::x25519::x25519_base(&private);
    
    (private.to_vec(), public.to_vec())
}

impl Default for HandshakeState {
    fn default() -> Self {
        Self::new()
    }
}
