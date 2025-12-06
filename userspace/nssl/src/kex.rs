//! Key Exchange
//!
//! Implements key exchange algorithms for TLS.

use std::vec::Vec;
use crate::tls::NamedGroup;

/// Key exchange context
pub struct KeyExchange {
    /// Named group/curve
    pub group: NamedGroup,
    /// Private key
    private_key: Vec<u8>,
    /// Public key
    public_key: Vec<u8>,
    /// Shared secret (after exchange)
    shared_secret: Option<Vec<u8>>,
}

impl KeyExchange {
    /// Create new X25519 key exchange
    pub fn new_x25519() -> Self {
        let (private, public) = generate_x25519_keypair();
        Self {
            group: NamedGroup::X25519,
            private_key: private,
            public_key: public,
            shared_secret: None,
        }
    }

    /// Create new P-256 key exchange
    pub fn new_p256() -> Self {
        let (private, public) = generate_p256_keypair();
        Self {
            group: NamedGroup::Secp256r1,
            private_key: private,
            public_key: public,
            shared_secret: None,
        }
    }

    /// Get public key
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Compute shared secret with peer's public key
    pub fn compute_shared_secret(&mut self, peer_public: &[u8]) -> Option<&[u8]> {
        let secret = match self.group {
            NamedGroup::X25519 => {
                x25519_shared_secret(&self.private_key, peer_public)
            }
            NamedGroup::Secp256r1 => {
                p256_shared_secret(&self.private_key, peer_public)
            }
            _ => return None,
        };
        
        self.shared_secret = secret;
        self.shared_secret.as_deref()
    }

    /// Get shared secret (if computed)
    pub fn shared_secret(&self) -> Option<&[u8]> {
        self.shared_secret.as_deref()
    }

    /// Get named group
    pub fn get_group(&self) -> NamedGroup {
        self.group
    }
}

/// Generate X25519 key pair
fn generate_x25519_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut private = [0u8; 32];
    let _ = ncryptolib::getrandom(&mut private, 0);
    
    // Clamp private key per RFC 7748
    private[0] &= 248;
    private[31] &= 127;
    private[31] |= 64;
    
    // Compute public key
    let public = ncryptolib::x25519::x25519_base(&private);
    
    (private.to_vec(), public.to_vec())
}

/// Generate P-256 key pair
fn generate_p256_keypair() -> (Vec<u8>, Vec<u8>) {
    // Generate P-256 key pair using ncryptolib
    let keypair = ncryptolib::p256::P256KeyPair::generate()
        .expect("Failed to generate P-256 key pair");
    let private = keypair.private_key.to_vec();
    let public = keypair.public_key_uncompressed();
    (private, public)
}

/// X25519 shared secret computation
fn x25519_shared_secret(private: &[u8], peer_public: &[u8]) -> Option<Vec<u8>> {
    if private.len() != 32 || peer_public.len() != 32 {
        return None;
    }
    
    let mut priv_arr = [0u8; 32];
    priv_arr.copy_from_slice(private);
    
    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(peer_public);
    
    Some(ncryptolib::x25519::x25519(&priv_arr, &pub_arr).to_vec())
}

/// P-256 ECDH shared secret computation
fn p256_shared_secret(private: &[u8], peer_public: &[u8]) -> Option<Vec<u8>> {
    if private.len() != 32 {
        return None;
    }
    
    // Parse peer's public key (uncompressed format: 04 || x || y)
    let peer_point = if peer_public.len() == 65 && peer_public[0] == 0x04 {
        ncryptolib::p256::P256Point::from_uncompressed(peer_public)?
    } else if peer_public.len() == 64 {
        // Raw x || y format
        let mut uncompressed = [0u8; 65];
        uncompressed[0] = 0x04;
        uncompressed[1..].copy_from_slice(peer_public);
        ncryptolib::p256::P256Point::from_uncompressed(&uncompressed)?
    } else {
        return None;
    };
    
    // Create key pair from private key
    let mut priv_arr = [0u8; 32];
    priv_arr.copy_from_slice(private);
    let keypair = ncryptolib::p256::P256KeyPair::from_private_key(&priv_arr)?;
    
    // Compute ECDH shared secret - returns x-coordinate directly
    let shared_secret = keypair.ecdh(&peer_point)?;
    
    // Return shared secret (x-coordinate)
    Some(shared_secret.to_vec())
}

/// Derive TLS 1.3 keys using HKDF
pub fn derive_tls13_keys(
    shared_secret: &[u8],
    transcript_hash: &[u8],
    is_handshake: bool,
) -> Option<DerivedKeys> {
    // TLS 1.3 key schedule (RFC 8446 Section 7.1)
    
    // Early secret
    let early_secret = hkdf_extract(&[0u8; 32], &[0u8; 32]);
    
    // Derive handshake secret
    let derived = hkdf_expand_label(&early_secret, b"derived", &[], 32);
    let handshake_secret = hkdf_extract(&derived, shared_secret);
    
    let label_prefix = if is_handshake { b"hs" } else { b"ap" };
    
    // Client traffic secret
    let mut client_label = Vec::new();
    client_label.extend_from_slice(b"c ");
    client_label.extend_from_slice(label_prefix);
    client_label.extend_from_slice(b" traffic");
    let client_secret = hkdf_expand_label(&handshake_secret, &client_label, transcript_hash, 32);
    
    // Server traffic secret
    let mut server_label = Vec::new();
    server_label.extend_from_slice(b"s ");
    server_label.extend_from_slice(label_prefix);
    server_label.extend_from_slice(b" traffic");
    let server_secret = hkdf_expand_label(&handshake_secret, &server_label, transcript_hash, 32);
    
    // Derive actual keys and IVs
    let client_key = hkdf_expand_label(&client_secret, b"key", &[], 32);
    let client_iv = hkdf_expand_label(&client_secret, b"iv", &[], 12);
    let server_key = hkdf_expand_label(&server_secret, b"key", &[], 32);
    let server_iv = hkdf_expand_label(&server_secret, b"iv", &[], 12);
    
    Some(DerivedKeys {
        client_key,
        client_iv,
        server_key,
        server_iv,
    })
}

/// Derive TLS 1.3 handshake keys and return handshake secrets for later use
pub fn derive_tls13_handshake_keys(
    shared_secret: &[u8],
    transcript_hash: &[u8],
    key_len: usize,
) -> Option<(HandshakeSecrets, DerivedKeys)> {
    // TLS 1.3 key schedule (RFC 8446 Section 7.1)
    // key_len == 32 (AES-256-GCM) uses SHA-384
    // key_len == 16 (AES-128-GCM) uses SHA-256
    let use_sha384 = key_len == 32;
    let hash_len = if use_sha384 { 48 } else { 32 };
    
    eprintln!("[TLS1.3-KDF] shared_secret len={}, transcript_hash len={}, key_len={}, use_sha384={}", 
        shared_secret.len(), transcript_hash.len(), key_len, use_sha384);
    eprintln!("[TLS1.3-KDF] shared_secret={:02x?}", shared_secret);
    eprintln!("[TLS1.3-KDF] transcript_hash={:02x?}", transcript_hash);
    
    // 1. Early secret: HKDF-Extract(salt=0, IKM=0) for no PSK
    let early_secret = if use_sha384 {
        let zero_key = [0u8; 48];
        hkdf_extract_sha384(&zero_key, &zero_key)
    } else {
        let zero_key = [0u8; 32];
        hkdf_extract(&zero_key, &zero_key)
    };
    eprintln!("[TLS1.3-KDF] early_secret={:02x?}", &early_secret);
    
    // 2. Derive-Secret(early_secret, "derived", "")
    let (empty_hash, derived_early) = if use_sha384 {
        let h = ncryptolib::sha384(&[]);
        let d = hkdf_expand_label_sha384(&early_secret, b"derived", &h, hash_len);
        (h.to_vec(), d)
    } else {
        let h = ncryptolib::sha256(&[]);
        let d = hkdf_expand_label(&early_secret, b"derived", &h, hash_len);
        (h.to_vec(), d)
    };
    eprintln!("[TLS1.3-KDF] derived_early={:02x?}", &derived_early);
    
    // 3. Handshake secret: HKDF-Extract(derived_early, shared_secret)
    let handshake_secret = if use_sha384 {
        hkdf_extract_sha384(&derived_early, shared_secret)
    } else {
        hkdf_extract(&derived_early, shared_secret)
    };
    eprintln!("[TLS1.3-KDF] handshake_secret={:02x?}", &handshake_secret);
    
    // 4. Client handshake traffic secret
    let client_hs_secret = if use_sha384 {
        hkdf_expand_label_sha384(&handshake_secret, b"c hs traffic", transcript_hash, hash_len)
    } else {
        hkdf_expand_label(&handshake_secret, b"c hs traffic", transcript_hash, hash_len)
    };
    eprintln!("[TLS1.3-KDF] client_hs_secret={:02x?}", &client_hs_secret);
    
    // 5. Server handshake traffic secret
    let server_hs_secret = if use_sha384 {
        hkdf_expand_label_sha384(&handshake_secret, b"s hs traffic", transcript_hash, hash_len)
    } else {
        hkdf_expand_label(&handshake_secret, b"s hs traffic", transcript_hash, hash_len)
    };
    eprintln!("[TLS1.3-KDF] server_hs_secret={:02x?}", &server_hs_secret);
    
    // 6. Derive keys and IVs (key_len depends on cipher suite)
    let (client_key, client_iv, server_key, server_iv) = if use_sha384 {
        (
            hkdf_expand_label_sha384(&client_hs_secret, b"key", &[], key_len),
            hkdf_expand_label_sha384(&client_hs_secret, b"iv", &[], 12),
            hkdf_expand_label_sha384(&server_hs_secret, b"key", &[], key_len),
            hkdf_expand_label_sha384(&server_hs_secret, b"iv", &[], 12),
        )
    } else {
        (
            hkdf_expand_label(&client_hs_secret, b"key", &[], key_len),
            hkdf_expand_label(&client_hs_secret, b"iv", &[], 12),
            hkdf_expand_label(&server_hs_secret, b"key", &[], key_len),
            hkdf_expand_label(&server_hs_secret, b"iv", &[], 12),
        )
    };
    
    eprintln!("[TLS1.3-KDF] server_key={:02x?}", &server_key);
    eprintln!("[TLS1.3-KDF] server_iv={:02x?}", &server_iv);
    
    let hs_secrets = HandshakeSecrets {
        handshake_secret: handshake_secret.clone(),
        client_hs_traffic_secret: client_hs_secret,
        server_hs_traffic_secret: server_hs_secret,
    };
    
    Some((hs_secrets, DerivedKeys {
        client_key,
        client_iv,
        server_key,
        server_iv,
    }))
}

/// Derive TLS 1.3 application keys from handshake secret
pub fn derive_tls13_application_keys(
    handshake_secret: &[u8],
    transcript_hash: &[u8],
    key_len: usize,
) -> Option<DerivedKeys> {
    // key_len == 32 (AES-256-GCM) uses SHA-384
    // key_len == 16 (AES-128-GCM) uses SHA-256
    let use_sha384 = key_len == 32;
    let hash_len = if use_sha384 { 48 } else { 32 };
    
    // 1. Derive-Secret(handshake_secret, "derived", "")
    let derived_hs = if use_sha384 {
        let empty_hash = ncryptolib::sha384(&[]);
        hkdf_expand_label_sha384(handshake_secret, b"derived", &empty_hash, hash_len)
    } else {
        let empty_hash = ncryptolib::sha256(&[]);
        hkdf_expand_label(handshake_secret, b"derived", &empty_hash, hash_len)
    };
    
    // 2. Master secret: HKDF-Extract(derived_hs, 0)
    let master_secret = if use_sha384 {
        let zero_key = [0u8; 48];
        hkdf_extract_sha384(&derived_hs, &zero_key)
    } else {
        let zero_key = [0u8; 32];
        hkdf_extract(&derived_hs, &zero_key)
    };
    
    // 3. Client application traffic secret
    let client_app_secret = if use_sha384 {
        hkdf_expand_label_sha384(&master_secret, b"c ap traffic", transcript_hash, hash_len)
    } else {
        hkdf_expand_label(&master_secret, b"c ap traffic", transcript_hash, hash_len)
    };
    
    // 4. Server application traffic secret
    let server_app_secret = if use_sha384 {
        hkdf_expand_label_sha384(&master_secret, b"s ap traffic", transcript_hash, hash_len)
    } else {
        hkdf_expand_label(&master_secret, b"s ap traffic", transcript_hash, hash_len)
    };
    
    // 5. Derive keys and IVs (key_len depends on cipher suite)
    let (client_key, client_iv, server_key, server_iv) = if use_sha384 {
        (
            hkdf_expand_label_sha384(&client_app_secret, b"key", &[], key_len),
            hkdf_expand_label_sha384(&client_app_secret, b"iv", &[], 12),
            hkdf_expand_label_sha384(&server_app_secret, b"key", &[], key_len),
            hkdf_expand_label_sha384(&server_app_secret, b"iv", &[], 12),
        )
    } else {
        (
            hkdf_expand_label(&client_app_secret, b"key", &[], key_len),
            hkdf_expand_label(&client_app_secret, b"iv", &[], 12),
            hkdf_expand_label(&server_app_secret, b"key", &[], key_len),
            hkdf_expand_label(&server_app_secret, b"iv", &[], 12),
        )
    };
    
    Some(DerivedKeys {
        client_key,
        client_iv,
        server_key,
        server_iv,
    })
}

/// Derive TLS 1.3 traffic secret for Finished message verification
pub fn derive_tls13_traffic_secret(
    handshake_secret: &[u8],
    label: &[u8],
    transcript_hash: &[u8],
    use_sha384: bool,
) -> Vec<u8> {
    // 首先需要从 handshake_secret 计算 traffic secret
    // 这个函数直接计算指定的 traffic secret
    let hash_len = if use_sha384 { 48 } else { 32 };
    if use_sha384 {
        hkdf_expand_label_sha384(handshake_secret, label, transcript_hash, hash_len)
    } else {
        hkdf_expand_label(handshake_secret, label, transcript_hash, hash_len)
    }
}

/// Derive TLS 1.2 keys using PRF
pub fn derive_tls12_keys(
    pre_master_secret: &[u8],
    client_random: &[u8],
    server_random: &[u8],
    key_len: usize,
    iv_len: usize,
) -> Option<DerivedKeys> {
    // TLS 1.2 PRF (RFC 5246 Section 5)
    
    // master_secret = PRF(pre_master_secret, "master secret", ClientHello.random + ServerHello.random)
    let mut seed = Vec::new();
    seed.extend_from_slice(client_random);
    seed.extend_from_slice(server_random);
    let master_secret = prf_sha256(pre_master_secret, b"master secret", &seed, 48);
    
    // key_block = PRF(master_secret, "key expansion", server_random + client_random)
    let mut key_seed = Vec::new();
    key_seed.extend_from_slice(server_random);
    key_seed.extend_from_slice(client_random);
    let key_block_len = 2 * (key_len + iv_len);
    let key_block = prf_sha256(&master_secret, b"key expansion", &key_seed, key_block_len);
    
    // Split key block
    let mut offset = 0;
    let client_key = key_block[offset..offset + key_len].to_vec();
    offset += key_len;
    let server_key = key_block[offset..offset + key_len].to_vec();
    offset += key_len;
    let client_iv = key_block[offset..offset + iv_len].to_vec();
    offset += iv_len;
    let server_iv = key_block[offset..offset + iv_len].to_vec();
    
    Some(DerivedKeys {
        client_key,
        client_iv,
        server_key,
        server_iv,
    })
}

/// Derived keys
#[derive(Clone)]
pub struct DerivedKeys {
    pub client_key: Vec<u8>,
    pub client_iv: Vec<u8>,
    pub server_key: Vec<u8>,
    pub server_iv: Vec<u8>,
}

/// Handshake secrets for TLS 1.3 Finished verification
pub struct HandshakeSecrets {
    pub handshake_secret: Vec<u8>,
    pub client_hs_traffic_secret: Vec<u8>,
    pub server_hs_traffic_secret: Vec<u8>,
}

/// HKDF-Extract (RFC 5869)
fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    ncryptolib::hmac_sha256(salt, ikm).to_vec()
}

/// HKDF-Extract with SHA-384
fn hkdf_extract_sha384(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    ncryptolib::hmac_sha384(salt, ikm).to_vec()
}

/// HKDF-Expand-Label (RFC 8446) - 公共函数供 Finished 消息使用
pub fn hkdf_expand_label(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    // HkdfLabel = struct {
    //   uint16 length = Length;
    //   opaque label<7..255> = "tls13 " + Label;
    //   opaque context<0..255> = Context;
    // };
    
    let mut hkdf_label = Vec::new();
    
    // Length (2 bytes)
    hkdf_label.push((length >> 8) as u8);
    hkdf_label.push((length & 0xFF) as u8);
    
    // Label with "tls13 " prefix
    let full_label_len = 6 + label.len();
    hkdf_label.push(full_label_len as u8);
    hkdf_label.extend_from_slice(b"tls13 ");
    hkdf_label.extend_from_slice(label);
    
    // Context
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    
    // HKDF-Expand
    hkdf_expand(secret, &hkdf_label, length)
}

/// HKDF-Expand (RFC 5869)
fn hkdf_expand(prk: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let hash_len = 32; // SHA-256
    let n = (length + hash_len - 1) / hash_len;
    
    let mut okm = Vec::new();
    let mut t = Vec::new();
    
    for i in 1..=n {
        let mut data = t.clone();
        data.extend_from_slice(info);
        data.push(i as u8);
        
        t = ncryptolib::hmac_sha256(prk, &data).to_vec();
        okm.extend_from_slice(&t);
    }
    
    okm.truncate(length);
    okm
}

/// HKDF-Expand with SHA-384
fn hkdf_expand_sha384(prk: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let hash_len = 48; // SHA-384
    let n = (length + hash_len - 1) / hash_len;
    
    let mut okm = Vec::new();
    let mut t = Vec::new();
    
    for i in 1..=n {
        let mut data = t.clone();
        data.extend_from_slice(info);
        data.push(i as u8);
        
        t = ncryptolib::hmac_sha384(prk, &data).to_vec();
        okm.extend_from_slice(&t);
    }
    
    okm.truncate(length);
    okm
}

/// HKDF-Expand-Label with SHA-384
pub fn hkdf_expand_label_sha384(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    let mut hkdf_label = Vec::new();
    
    // Length (2 bytes)
    hkdf_label.push((length >> 8) as u8);
    hkdf_label.push((length & 0xFF) as u8);
    
    // Label with "tls13 " prefix
    let full_label_len = 6 + label.len();
    hkdf_label.push(full_label_len as u8);
    hkdf_label.extend_from_slice(b"tls13 ");
    hkdf_label.extend_from_slice(label);
    
    // Context
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    
    // HKDF-Expand with SHA-384
    hkdf_expand_sha384(secret, &hkdf_label, length)
}

/// TLS 1.2 PRF with SHA-256 (RFC 5246 Section 5)
fn prf_sha256(secret: &[u8], label: &[u8], seed: &[u8], length: usize) -> Vec<u8> {
    // P_SHA256(secret, seed) = HMAC_SHA256(secret, A(1) + seed) +
    //                          HMAC_SHA256(secret, A(2) + seed) + ...
    // A(0) = seed
    // A(i) = HMAC_SHA256(secret, A(i-1))
    
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
