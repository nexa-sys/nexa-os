//! RSA Public-Key Cryptography
//!
//! PKCS#1 v2.2 (RFC 8017) compliant RSA implementation.
//! Supports key sizes: 2048, 3072, 4096 bits.

use std::vec::Vec;

use crate::bigint::BigInt;
use crate::hash::{sha256, SHA256_DIGEST_SIZE};
use crate::random::random_bytes;

// ============================================================================
// Constants
// ============================================================================

/// RSA-2048 key size in bits
pub const RSA_2048_BITS: usize = 2048;
/// RSA-3072 key size in bits
pub const RSA_3072_BITS: usize = 3072;
/// RSA-4096 key size in bits
pub const RSA_4096_BITS: usize = 4096;

/// Common public exponent (65537)
pub const RSA_E: u32 = 65537;

// PKCS#1 v1.5 DigestInfo prefixes
const SHA256_DIGEST_INFO: &[u8] = &[
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86,
    0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

// ============================================================================
// RSA Key Structures
// ============================================================================

/// RSA public key
#[derive(Clone, Debug)]
pub struct RsaPublicKey {
    /// Modulus n = p * q
    pub n: BigInt,
    /// Public exponent e (typically 65537)
    pub e: BigInt,
    /// Key size in bits
    pub bits: usize,
}

/// RSA private key (CRT form)
#[derive(Clone)]
pub struct RsaPrivateKey {
    /// Public key
    pub public: RsaPublicKey,
    /// Private exponent d
    pub d: BigInt,
    /// First prime p
    pub p: BigInt,
    /// Second prime q
    pub q: BigInt,
    /// d mod (p-1)
    pub dp: BigInt,
    /// d mod (q-1)
    pub dq: BigInt,
    /// q^(-1) mod p
    pub qinv: BigInt,
}

impl RsaPublicKey {
    /// Create a public key from components
    pub fn new(n: BigInt, e: BigInt) -> Self {
        let bits = n.bit_length();
        Self { n, e, bits }
    }

    /// Get key size in bytes
    pub fn byte_size(&self) -> usize {
        (self.bits + 7) / 8
    }
}

impl RsaPrivateKey {
    /// Get the public key
    pub fn public_key(&self) -> &RsaPublicKey {
        &self.public
    }

    /// Get key size in bytes
    pub fn byte_size(&self) -> usize {
        self.public.byte_size()
    }
}

// ============================================================================
// RSA Key Generation
// ============================================================================

/// Generate a random prime of the specified bit length
fn generate_prime(bits: usize) -> BigInt {
    loop {
        let mut bytes = vec![0u8; (bits + 7) / 8];
        random_bytes(&mut bytes).unwrap();
        
        // Set top bit to ensure correct bit length
        bytes[0] |= 0x80;
        // Set bottom bit to ensure odd number
        let last_idx = bytes.len() - 1;
        bytes[last_idx] |= 0x01;
        
        let candidate = BigInt::from_bytes_be(&bytes);
        
        if is_probably_prime(&candidate, 40) {
            return candidate;
        }
    }
}

/// Miller-Rabin primality test
fn is_probably_prime(n: &BigInt, rounds: u32) -> bool {
    if n.is_even() {
        return n == &BigInt::from_u64(2);
    }
    if n < &BigInt::from_u64(3) {
        return false;
    }
    
    // Write n-1 as 2^r * d
    let n_minus_1 = n.sub(&BigInt::one());
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    
    while d.is_even() {
        d = d.div(&BigInt::from_u64(2)).0;
        r += 1;
    }
    
    // Test with random bases
    for _ in 0..rounds {
        let mut a_bytes = vec![0u8; n.byte_length().min(32)];
        random_bytes(&mut a_bytes).unwrap();
        let mut a = BigInt::from_bytes_be(&a_bytes);
        a = a.modulo(n);
        
        if a < BigInt::from_u64(2) {
            a = BigInt::from_u64(2);
        }
        
        let mut x = mod_exp(&a, &d, n);
        
        if x == BigInt::one() || x == n_minus_1 {
            continue;
        }
        
        let mut composite = true;
        for _ in 0..r - 1 {
            x = mod_exp(&x, &BigInt::from_u64(2), n);
            if x == n_minus_1 {
                composite = false;
                break;
            }
        }
        
        if composite {
            return false;
        }
    }
    
    true
}

/// Modular exponentiation: base^exp mod m
fn mod_exp(base: &BigInt, exp: &BigInt, m: &BigInt) -> BigInt {
    if m == &BigInt::one() {
        return BigInt::zero();
    }
    
    let mut result = BigInt::one();
    let mut base = base.modulo(m);
    let mut exp = exp.clone();
    
    while exp > BigInt::zero() {
        if exp.is_odd() {
            result = result.mul(&base).modulo(m);
        }
        exp = exp.div(&BigInt::from_u64(2)).0;
        base = base.mul(&base).modulo(m);
    }
    
    result
}

/// Extended Euclidean algorithm for modular inverse
fn mod_inverse(a: &BigInt, m: &BigInt) -> Option<BigInt> {
    let (gcd, x, _) = extended_gcd(a, m);
    if gcd != BigInt::one() {
        return None;
    }
    
    // Handle negative result
    if x.is_negative() {
        Some(x.add(m))
    } else {
        Some(x.modulo(m))
    }
}

/// Extended GCD
fn extended_gcd(a: &BigInt, b: &BigInt) -> (BigInt, BigInt, BigInt) {
    if b == &BigInt::zero() {
        return (a.clone(), BigInt::one(), BigInt::zero());
    }
    
    let (q, r) = a.div(b);
    let (gcd, x1, y1) = extended_gcd(b, &r);
    
    let x = y1.clone();
    let y = x1.sub(&q.mul(&y1));
    
    (gcd, x, y)
}

/// Generate an RSA key pair
pub fn generate_keypair(bits: usize) -> Result<RsaPrivateKey, &'static str> {
    if bits < 2048 {
        return Err("Key size must be at least 2048 bits");
    }
    if bits % 2 != 0 {
        return Err("Key size must be even");
    }
    
    let prime_bits = bits / 2;
    let e = BigInt::from_u64(RSA_E as u64);
    
    // Generate primes p and q
    let p = loop {
        let candidate = generate_prime(prime_bits);
        let p_minus_1 = candidate.sub(&BigInt::one());
        let gcd = extended_gcd(&p_minus_1, &e).0;
        if gcd == BigInt::one() {
            break candidate;
        }
    };
    
    let q = loop {
        let candidate = generate_prime(prime_bits);
        if candidate == p {
            continue;
        }
        let q_minus_1 = candidate.sub(&BigInt::one());
        let gcd = extended_gcd(&q_minus_1, &e).0;
        if gcd == BigInt::one() {
            break candidate;
        }
    };
    
    // Compute RSA parameters
    let n = p.mul(&q);
    let p_minus_1 = p.sub(&BigInt::one());
    let q_minus_1 = q.sub(&BigInt::one());
    let phi_n = p_minus_1.mul(&q_minus_1);
    
    // Compute private exponent d = e^(-1) mod phi(n)
    let d = mod_inverse(&e, &phi_n).ok_or("Failed to compute private exponent")?;
    
    // CRT parameters
    let dp = d.modulo(&p_minus_1);
    let dq = d.modulo(&q_minus_1);
    let qinv = mod_inverse(&q, &p).ok_or("Failed to compute qinv")?;
    
    let public = RsaPublicKey::new(n, e);
    
    Ok(RsaPrivateKey {
        public,
        d,
        p,
        q,
        dp,
        dq,
        qinv,
    })
}

// ============================================================================
// RSA Operations
// ============================================================================

/// RSA raw encryption (no padding)
fn rsa_encrypt_raw(m: &BigInt, key: &RsaPublicKey) -> BigInt {
    mod_exp(m, &key.e, &key.n)
}

/// RSA raw decryption using CRT
fn rsa_decrypt_raw(c: &BigInt, key: &RsaPrivateKey) -> BigInt {
    // Use CRT for faster decryption
    let m1 = mod_exp(&c.modulo(&key.p), &key.dp, &key.p);
    let m2 = mod_exp(&c.modulo(&key.q), &key.dq, &key.q);
    
    // h = qinv * (m1 - m2) mod p
    let diff = if m1 >= m2 {
        m1.sub(&m2)
    } else {
        m1.add(&key.p).sub(&m2)
    };
    let h = key.qinv.mul(&diff).modulo(&key.p);
    
    // m = m2 + h * q
    m2.add(&h.mul(&key.q))
}

// ============================================================================
// PKCS#1 v1.5 Padding
// ============================================================================

/// PKCS#1 v1.5 encryption padding
fn pkcs1_v15_pad_encrypt(message: &[u8], key_size: usize) -> Result<Vec<u8>, &'static str> {
    if message.len() > key_size - 11 {
        return Err("Message too long for key size");
    }
    
    let ps_len = key_size - message.len() - 3;
    let mut padded = Vec::with_capacity(key_size);
    
    // 0x00 || 0x02 || PS || 0x00 || M
    padded.push(0x00);
    padded.push(0x02);
    
    // Generate random non-zero padding
    let mut ps = vec![0u8; ps_len];
    for byte in ps.iter_mut() {
        loop {
            let mut b = [0u8; 1];
            random_bytes(&mut b).unwrap();
            if b[0] != 0 {
                *byte = b[0];
                break;
            }
        }
    }
    padded.extend_from_slice(&ps);
    padded.push(0x00);
    padded.extend_from_slice(message);
    
    Ok(padded)
}

/// PKCS#1 v1.5 encryption unpadding
fn pkcs1_v15_unpad_encrypt(padded: &[u8]) -> Result<Vec<u8>, &'static str> {
    if padded.len() < 11 || padded[0] != 0x00 || padded[1] != 0x02 {
        return Err("Invalid PKCS#1 v1.5 padding");
    }
    
    // Find the 0x00 separator
    let mut sep_idx = None;
    for i in 2..padded.len() {
        if padded[i] == 0x00 {
            sep_idx = Some(i);
            break;
        }
    }
    
    match sep_idx {
        Some(idx) if idx >= 10 => Ok(padded[idx + 1..].to_vec()),
        _ => Err("Invalid PKCS#1 v1.5 padding"),
    }
}

/// PKCS#1 v1.5 signature padding (EMSA-PKCS1-v1_5)
fn pkcs1_v15_pad_sign(hash: &[u8], key_size: usize) -> Result<Vec<u8>, &'static str> {
    let t = [SHA256_DIGEST_INFO, hash].concat();
    let t_len = t.len();
    
    if key_size < t_len + 11 {
        return Err("Key too small for signature");
    }
    
    let ps_len = key_size - t_len - 3;
    let mut em = Vec::with_capacity(key_size);
    
    // 0x00 || 0x01 || PS || 0x00 || T
    em.push(0x00);
    em.push(0x01);
    em.extend(vec![0xff; ps_len]);
    em.push(0x00);
    em.extend_from_slice(&t);
    
    Ok(em)
}

/// PKCS#1 v1.5 signature verification unpadding
fn pkcs1_v15_unpad_verify(em: &[u8]) -> Result<Vec<u8>, &'static str> {
    if em.len() < 11 || em[0] != 0x00 || em[1] != 0x01 {
        return Err("Invalid signature padding");
    }
    
    // Find 0x00 separator after PS of 0xff bytes
    let mut sep_idx = None;
    for i in 2..em.len() {
        if em[i] == 0x00 {
            sep_idx = Some(i);
            break;
        }
        if em[i] != 0xff {
            return Err("Invalid PS in signature");
        }
    }
    
    match sep_idx {
        Some(idx) if idx >= 10 => {
            let t = &em[idx + 1..];
            // Extract hash from DigestInfo
            if t.len() >= SHA256_DIGEST_INFO.len() + SHA256_DIGEST_SIZE {
                let di = &t[..SHA256_DIGEST_INFO.len()];
                if di == SHA256_DIGEST_INFO {
                    return Ok(t[SHA256_DIGEST_INFO.len()..].to_vec());
                }
            }
            Err("Invalid DigestInfo")
        }
        _ => Err("Invalid signature padding"),
    }
}

// ============================================================================
// OAEP Padding (RFC 8017)
// ============================================================================

/// MGF1 mask generation function
fn mgf1_sha256(seed: &[u8], len: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(len);
    let mut counter = 0u32;
    
    while output.len() < len {
        let mut input = seed.to_vec();
        input.extend_from_slice(&counter.to_be_bytes());
        output.extend_from_slice(&sha256(&input));
        counter += 1;
    }
    
    output.truncate(len);
    output
}

/// RSA-OAEP encryption (SHA-256)
pub fn rsa_oaep_encrypt(
    message: &[u8],
    key: &RsaPublicKey,
    label: Option<&[u8]>,
) -> Result<Vec<u8>, &'static str> {
    let k = key.byte_size();
    let h_len = SHA256_DIGEST_SIZE;
    let max_msg_len = k - 2 * h_len - 2;
    
    if message.len() > max_msg_len {
        return Err("Message too long");
    }
    
    let label_hash = sha256(label.unwrap_or(&[]));
    
    // DB = lHash || PS || 0x01 || M
    let ps_len = k - message.len() - 2 * h_len - 2;
    let mut db = Vec::with_capacity(k - h_len - 1);
    db.extend_from_slice(&label_hash);
    db.extend(vec![0u8; ps_len]);
    db.push(0x01);
    db.extend_from_slice(message);
    
    // Generate random seed
    let mut seed = vec![0u8; h_len];
    random_bytes(&mut seed).unwrap();
    
    // dbMask = MGF1(seed, k - hLen - 1)
    let db_mask = mgf1_sha256(&seed, k - h_len - 1);
    
    // maskedDB = DB XOR dbMask
    let mut masked_db: Vec<u8> = db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    
    // seedMask = MGF1(maskedDB, hLen)
    let seed_mask = mgf1_sha256(&masked_db, h_len);
    
    // maskedSeed = seed XOR seedMask
    let masked_seed: Vec<u8> = seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
    
    // EM = 0x00 || maskedSeed || maskedDB
    let mut em = Vec::with_capacity(k);
    em.push(0x00);
    em.extend_from_slice(&masked_seed);
    em.append(&mut masked_db);
    
    // Encrypt
    let m = BigInt::from_bytes_be(&em);
    let c = rsa_encrypt_raw(&m, key);
    
    Ok(c.to_bytes_be_padded(k))
}

/// RSA-OAEP decryption (SHA-256)
pub fn rsa_oaep_decrypt(
    ciphertext: &[u8],
    key: &RsaPrivateKey,
    label: Option<&[u8]>,
) -> Result<Vec<u8>, &'static str> {
    let k = key.byte_size();
    let h_len = SHA256_DIGEST_SIZE;
    
    if ciphertext.len() != k || k < 2 * h_len + 2 {
        return Err("Decryption error");
    }
    
    // Decrypt
    let c = BigInt::from_bytes_be(ciphertext);
    let em_int = rsa_decrypt_raw(&c, key);
    let em = em_int.to_bytes_be_padded(k);
    
    // Check leading byte
    if em[0] != 0x00 {
        return Err("Decryption error");
    }
    
    let masked_seed = &em[1..1 + h_len];
    let masked_db = &em[1 + h_len..];
    
    // Unmask seed
    let seed_mask = mgf1_sha256(masked_db, h_len);
    let seed: Vec<u8> = masked_seed.iter().zip(seed_mask.iter()).map(|(a, b)| a ^ b).collect();
    
    // Unmask DB
    let db_mask = mgf1_sha256(&seed, k - h_len - 1);
    let db: Vec<u8> = masked_db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    
    // Verify label hash
    let label_hash = sha256(label.unwrap_or(&[]));
    if db[..h_len] != label_hash[..] {
        return Err("Decryption error");
    }
    
    // Find 0x01 separator
    let mut sep_idx = None;
    for i in h_len..db.len() {
        if db[i] == 0x01 {
            sep_idx = Some(i);
            break;
        }
        if db[i] != 0x00 {
            return Err("Decryption error");
        }
    }
    
    match sep_idx {
        Some(idx) => Ok(db[idx + 1..].to_vec()),
        None => Err("Decryption error"),
    }
}

// ============================================================================
// RSA Encryption/Decryption (PKCS#1 v1.5)
// ============================================================================

/// RSA-PKCS#1 v1.5 encryption
pub fn rsa_encrypt(message: &[u8], key: &RsaPublicKey) -> Result<Vec<u8>, &'static str> {
    let k = key.byte_size();
    let padded = pkcs1_v15_pad_encrypt(message, k)?;
    let m = BigInt::from_bytes_be(&padded);
    let c = rsa_encrypt_raw(&m, key);
    Ok(c.to_bytes_be_padded(k))
}

/// RSA-PKCS#1 v1.5 decryption
pub fn rsa_decrypt(ciphertext: &[u8], key: &RsaPrivateKey) -> Result<Vec<u8>, &'static str> {
    let k = key.byte_size();
    if ciphertext.len() != k {
        return Err("Invalid ciphertext length");
    }
    
    let c = BigInt::from_bytes_be(ciphertext);
    let m = rsa_decrypt_raw(&c, key);
    let padded = m.to_bytes_be_padded(k);
    
    pkcs1_v15_unpad_encrypt(&padded)
}

// ============================================================================
// RSA Signatures (PKCS#1 v1.5 with SHA-256)
// ============================================================================

/// RSA-PKCS#1 v1.5 signature
pub fn rsa_sign(message: &[u8], key: &RsaPrivateKey) -> Result<Vec<u8>, &'static str> {
    let k = key.byte_size();
    let hash = sha256(message);
    let em = pkcs1_v15_pad_sign(&hash, k)?;
    let m = BigInt::from_bytes_be(&em);
    let s = rsa_decrypt_raw(&m, key); // Sign with private key
    Ok(s.to_bytes_be_padded(k))
}

/// RSA-PKCS#1 v1.5 signature verification
pub fn rsa_verify(message: &[u8], signature: &[u8], key: &RsaPublicKey) -> Result<bool, &'static str> {
    let k = key.byte_size();
    if signature.len() != k {
        return Err("Invalid signature length");
    }
    
    let s = BigInt::from_bytes_be(signature);
    let m = rsa_encrypt_raw(&s, key); // Verify with public key
    let em = m.to_bytes_be_padded(k);
    
    let extracted_hash = pkcs1_v15_unpad_verify(&em)?;
    let expected_hash = sha256(message);
    
    Ok(extracted_hash == expected_hash)
}

// ============================================================================
// C ABI Exports
// ============================================================================

use crate::{c_int, size_t};

/// RSA encrypt (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_rsa_encrypt(
    message: *const u8,
    message_len: size_t,
    n: *const u8,
    n_len: size_t,
    e: *const u8,
    e_len: size_t,
    ciphertext: *mut u8,
    ciphertext_len: *mut size_t,
) -> c_int {
    if message.is_null() || n.is_null() || e.is_null() || ciphertext.is_null() {
        return -1;
    }

    let message_slice = core::slice::from_raw_parts(message, message_len);
    let n_slice = core::slice::from_raw_parts(n, n_len);
    let e_slice = core::slice::from_raw_parts(e, e_len);

    let key = RsaPublicKey::new(
        BigInt::from_bytes_be(n_slice),
        BigInt::from_bytes_be(e_slice),
    );

    match rsa_encrypt(message_slice, &key) {
        Ok(ct) => {
            core::ptr::copy_nonoverlapping(ct.as_ptr(), ciphertext, ct.len());
            *ciphertext_len = ct.len();
            0
        }
        Err(_) => -1,
    }
}

/// RSA sign (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_rsa_sign(
    message: *const u8,
    message_len: size_t,
    n: *const u8,
    n_len: size_t,
    d: *const u8,
    d_len: size_t,
    signature: *mut u8,
    signature_len: *mut size_t,
) -> c_int {
    if message.is_null() || n.is_null() || d.is_null() || signature.is_null() {
        return -1;
    }

    // Simplified: create a minimal private key structure
    let message_slice = core::slice::from_raw_parts(message, message_len);
    let hash = sha256(message_slice);
    
    let n_int = BigInt::from_bytes_be(core::slice::from_raw_parts(n, n_len));
    let d_int = BigInt::from_bytes_be(core::slice::from_raw_parts(d, d_len));
    let k = (n_int.bit_length() + 7) / 8;
    
    // Create padded message
    let em = match pkcs1_v15_pad_sign(&hash, k) {
        Ok(em) => em,
        Err(_) => return -1,
    };
    
    let m = BigInt::from_bytes_be(&em);
    let s = mod_exp(&m, &d_int, &n_int);
    let sig = s.to_bytes_be_padded(k);
    
    core::ptr::copy_nonoverlapping(sig.as_ptr(), signature, sig.len());
    *signature_len = sig.len();
    0
}

/// RSA verify (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_rsa_verify(
    message: *const u8,
    message_len: size_t,
    signature: *const u8,
    signature_len: size_t,
    n: *const u8,
    n_len: size_t,
    e: *const u8,
    e_len: size_t,
) -> c_int {
    if message.is_null() || signature.is_null() || n.is_null() || e.is_null() {
        return -1;
    }

    let message_slice = core::slice::from_raw_parts(message, message_len);
    let signature_slice = core::slice::from_raw_parts(signature, signature_len);
    let n_slice = core::slice::from_raw_parts(n, n_len);
    let e_slice = core::slice::from_raw_parts(e, e_len);

    let key = RsaPublicKey::new(
        BigInt::from_bytes_be(n_slice),
        BigInt::from_bytes_be(e_slice),
    );

    match rsa_verify(message_slice, signature_slice, &key) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(_) => -1,
    }
}

// ============================================================================
// RSA-PSS Signatures (RFC 8017)
// ============================================================================

/// RSA-PSS signature (SHA-256, salt length = hash length)
pub fn rsa_pss_sign(message: &[u8], key: &RsaPrivateKey) -> Result<Vec<u8>, &'static str> {
    rsa_pss_sign_with_salt_len(message, key, SHA256_DIGEST_SIZE)
}

/// RSA-PSS signature with custom salt length
pub fn rsa_pss_sign_with_salt_len(
    message: &[u8],
    key: &RsaPrivateKey,
    salt_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let k = key.byte_size();
    let em_bits = key.public.bits - 1;
    let em_len = (em_bits + 7) / 8;
    let h_len = SHA256_DIGEST_SIZE;
    
    if em_len < h_len + salt_len + 2 {
        return Err("Key too short for PSS padding");
    }
    
    // Hash the message
    let m_hash = sha256(message);
    
    // Generate random salt
    let mut salt = vec![0u8; salt_len];
    if salt_len > 0 {
        random_bytes(&mut salt).map_err(|_| "RNG failure")?;
    }
    
    // M' = (0x)00 00 00 00 00 00 00 00 || mHash || salt
    let mut m_prime = Vec::with_capacity(8 + h_len + salt_len);
    m_prime.extend_from_slice(&[0u8; 8]);
    m_prime.extend_from_slice(&m_hash);
    m_prime.extend_from_slice(&salt);
    
    // H = Hash(M')
    let h = sha256(&m_prime);
    
    // DB = PS || 0x01 || salt
    let ps_len = em_len - h_len - salt_len - 2;
    let mut db = Vec::with_capacity(em_len - h_len - 1);
    db.extend(vec![0u8; ps_len]);
    db.push(0x01);
    db.extend_from_slice(&salt);
    
    // dbMask = MGF1(H, emLen - hLen - 1)
    let db_mask = mgf1_sha256(&h, em_len - h_len - 1);
    
    // maskedDB = DB XOR dbMask
    let mut masked_db: Vec<u8> = db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    
    // Set leftmost bits to zero
    let zero_bits = 8 * em_len - em_bits;
    if zero_bits > 0 && !masked_db.is_empty() {
        masked_db[0] &= 0xFF >> zero_bits;
    }
    
    // EM = maskedDB || H || 0xbc
    let mut em = Vec::with_capacity(em_len);
    em.extend_from_slice(&masked_db);
    em.extend_from_slice(&h);
    em.push(0xbc);
    
    // Pad to k bytes if needed
    let mut padded_em = Vec::with_capacity(k);
    if em.len() < k {
        padded_em.extend(vec![0u8; k - em.len()]);
    }
    padded_em.extend_from_slice(&em);
    
    // Sign with private key
    let m = BigInt::from_bytes_be(&padded_em);
    let s = rsa_decrypt_raw(&m, key);
    
    Ok(s.to_bytes_be_padded(k))
}

/// RSA-PSS signature verification
pub fn rsa_pss_verify(
    message: &[u8],
    signature: &[u8],
    key: &RsaPublicKey,
) -> Result<bool, &'static str> {
    rsa_pss_verify_with_salt_len(message, signature, key, SHA256_DIGEST_SIZE)
}

/// RSA-PSS signature verification with custom salt length
pub fn rsa_pss_verify_with_salt_len(
    message: &[u8],
    signature: &[u8],
    key: &RsaPublicKey,
    salt_len: usize,
) -> Result<bool, &'static str> {
    let k = key.byte_size();
    let em_bits = key.bits - 1;
    let em_len = (em_bits + 7) / 8;
    let h_len = SHA256_DIGEST_SIZE;
    
    if signature.len() != k {
        return Err("Invalid signature length");
    }
    
    if em_len < h_len + salt_len + 2 {
        return Ok(false);
    }
    
    // Verify with public key
    let s = BigInt::from_bytes_be(signature);
    let m = rsa_encrypt_raw(&s, key);
    let em_full = m.to_bytes_be_padded(k);
    
    // Extract EM (may be padded with leading zeros)
    let em = if em_full.len() > em_len {
        &em_full[em_full.len() - em_len..]
    } else {
        &em_full
    };
    
    // Check trailer byte
    if em.is_empty() || em[em.len() - 1] != 0xbc {
        return Ok(false);
    }
    
    let masked_db = &em[..em_len - h_len - 1];
    let h = &em[em_len - h_len - 1..em_len - 1];
    
    // Check leftmost bits
    let zero_bits = 8 * em_len - em_bits;
    if zero_bits > 0 && !masked_db.is_empty() {
        let mask = 0xFF << (8 - zero_bits);
        if (masked_db[0] & mask) != 0 {
            return Ok(false);
        }
    }
    
    // Unmask DB
    let db_mask = mgf1_sha256(h, em_len - h_len - 1);
    let mut db: Vec<u8> = masked_db.iter().zip(db_mask.iter()).map(|(a, b)| a ^ b).collect();
    
    // Clear leftmost bits
    if zero_bits > 0 && !db.is_empty() {
        db[0] &= 0xFF >> zero_bits;
    }
    
    // Verify PS and separator
    let ps_len = em_len - h_len - salt_len - 2;
    for i in 0..ps_len {
        if db[i] != 0x00 {
            return Ok(false);
        }
    }
    if db[ps_len] != 0x01 {
        return Ok(false);
    }
    
    // Extract salt
    let salt = &db[ps_len + 1..];
    
    // Hash the message
    let m_hash = sha256(message);
    
    // M' = (0x)00 00 00 00 00 00 00 00 || mHash || salt
    let mut m_prime = Vec::with_capacity(8 + h_len + salt_len);
    m_prime.extend_from_slice(&[0u8; 8]);
    m_prime.extend_from_slice(&m_hash);
    m_prime.extend_from_slice(salt);
    
    // H' = Hash(M')
    let h_prime = sha256(&m_prime);
    
    // Compare H and H'
    Ok(h == &h_prime[..])
}

/// RSA-PSS sign (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_rsa_pss_sign(
    message: *const u8,
    message_len: size_t,
    n: *const u8,
    n_len: size_t,
    d: *const u8,
    d_len: size_t,
    signature: *mut u8,
    signature_len: *mut size_t,
) -> c_int {
    if message.is_null() || n.is_null() || d.is_null() || signature.is_null() {
        return -1;
    }

    let message_slice = core::slice::from_raw_parts(message, message_len);
    let n_int = BigInt::from_bytes_be(core::slice::from_raw_parts(n, n_len));
    let d_int = BigInt::from_bytes_be(core::slice::from_raw_parts(d, d_len));
    
    // Create a minimal private key (simplified, without CRT)
    let bits = n_int.bit_length();
    let e = BigInt::from_u64(RSA_E as u64);
    let public = RsaPublicKey { n: n_int.clone(), e, bits };
    
    // Create a simplified private key
    let priv_key = RsaPrivateKey {
        public,
        d: d_int,
        p: BigInt::zero(),
        q: BigInt::zero(),
        dp: BigInt::zero(),
        dq: BigInt::zero(),
        qinv: BigInt::zero(),
    };

    match rsa_pss_sign(message_slice, &priv_key) {
        Ok(sig) => {
            core::ptr::copy_nonoverlapping(sig.as_ptr(), signature, sig.len());
            *signature_len = sig.len();
            0
        }
        Err(_) => -1,
    }
}

/// RSA-PSS verify (C ABI)
#[no_mangle]
pub unsafe extern "C" fn ncrypto_rsa_pss_verify(
    message: *const u8,
    message_len: size_t,
    signature: *const u8,
    signature_len: size_t,
    n: *const u8,
    n_len: size_t,
    e: *const u8,
    e_len: size_t,
) -> c_int {
    if message.is_null() || signature.is_null() || n.is_null() || e.is_null() {
        return -1;
    }

    let message_slice = core::slice::from_raw_parts(message, message_len);
    let signature_slice = core::slice::from_raw_parts(signature, signature_len);
    let n_slice = core::slice::from_raw_parts(n, n_len);
    let e_slice = core::slice::from_raw_parts(e, e_len);

    let key = RsaPublicKey::new(
        BigInt::from_bytes_be(n_slice),
        BigInt::from_bytes_be(e_slice),
    );

    match rsa_pss_verify(message_slice, signature_slice, &key) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mod_exp() {
        let base = BigInt::from_u64(2);
        let exp = BigInt::from_u64(10);
        let m = BigInt::from_u64(1000);
        let result = mod_exp(&base, &exp, &m);
        assert_eq!(result, BigInt::from_u64(24)); // 2^10 = 1024 mod 1000 = 24
    }

    #[test]
    fn test_mod_inverse() {
        let a = BigInt::from_u64(3);
        let m = BigInt::from_u64(11);
        let inv = mod_inverse(&a, &m).unwrap();
        // 3 * 4 = 12 ≡ 1 (mod 11)
        assert_eq!(a.mul(&inv).modulo(&m), BigInt::one());
    }

    #[test]
    fn test_mgf1() {
        let seed = b"test seed";
        let mask = mgf1_sha256(seed, 32);
        assert_eq!(mask.len(), 32);
    }
}
