//! TLS Record Layer
//!
//! Handles encryption/decryption and framing of TLS records.

use std::vec::Vec;
use crate::bio::Bio;
use crate::tls::ContentType;
use crate::{c_int, TLS1_2_VERSION, TLS1_3_VERSION};

/// Maximum TLS record size
pub const MAX_RECORD_SIZE: usize = 16384;

/// Maximum TLS record size with overhead
pub const MAX_RECORD_SIZE_WITH_OVERHEAD: usize = MAX_RECORD_SIZE + 256;

/// TLS Record Header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RecordHeader {
    pub content_type: u8,
    pub version_major: u8,
    pub version_minor: u8,
    pub length: u16,
}

impl RecordHeader {
    pub fn new(content_type: ContentType, version: u16, length: u16) -> Self {
        Self {
            content_type: content_type as u8,
            version_major: (version >> 8) as u8,
            version_minor: (version & 0xFF) as u8,
            length,
        }
    }

    pub fn to_bytes(&self) -> [u8; 5] {
        [
            self.content_type,
            self.version_major,
            self.version_minor,
            (self.length >> 8) as u8,
            (self.length & 0xFF) as u8,
        ]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 5 {
            return None;
        }
        Some(Self {
            content_type: data[0],
            version_major: data[1],
            version_minor: data[2],
            length: ((data[3] as u16) << 8) | (data[4] as u16),
        })
    }
}

/// Record layer state
pub struct RecordLayer {
    /// Read sequence number
    read_seq: u64,
    /// Write sequence number
    write_seq: u64,
    /// Read key
    read_key: Option<Vec<u8>>,
    /// Write key
    write_key: Option<Vec<u8>>,
    /// Read IV
    read_iv: Option<Vec<u8>>,
    /// Write IV
    write_iv: Option<Vec<u8>>,
    /// Pending data buffer
    pending: Vec<u8>,
    /// EOF flag
    eof: bool,
    /// Protocol version
    version: u16,
}

impl RecordLayer {
    pub fn new() -> Self {
        Self {
            read_seq: 0,
            write_seq: 0,
            read_key: None,
            write_key: None,
            read_iv: None,
            write_iv: None,
            pending: Vec::new(),
            eof: false,
            version: TLS1_2_VERSION,
        }
    }

    /// Set encryption keys
    pub fn set_keys(&mut self, read_key: Vec<u8>, write_key: Vec<u8>, read_iv: Vec<u8>, write_iv: Vec<u8>) {
        self.read_key = Some(read_key);
        self.write_key = Some(write_key);
        self.read_iv = Some(read_iv);
        self.write_iv = Some(write_iv);
        self.read_seq = 0;
        self.write_seq = 0;
    }
    
    /// Set protocol version
    pub fn set_version(&mut self, version: u16) {
        self.version = version;
    }

    /// Check if EOF
    pub fn is_eof(&self) -> bool {
        self.eof
    }

    /// Write handshake message
    pub fn write_handshake(&mut self, data: &[u8], wbio: *mut Bio) -> bool {
        self.write_record(ContentType::Handshake, data, wbio)
    }

    /// Write ChangeCipherSpec message
    pub fn write_ccs(&mut self, data: &[u8], wbio: *mut Bio) -> bool {
        self.write_record(ContentType::ChangeCipherSpec, data, wbio)
    }

    /// Read ChangeCipherSpec message
    pub fn read_ccs(&mut self, rbio: *mut Bio) -> bool {
        if rbio.is_null() {
            return false;
        }

        // Read header
        let mut header_buf = [0u8; 5];
        unsafe {
            let n = (*rbio).read(header_buf.as_mut_ptr(), 5);
            if n != 5 {
                return false;
            }
        }

        let header = match RecordHeader::from_bytes(&header_buf) {
            Some(h) => h,
            None => return false,
        };

        // Check content type
        if header.content_type != ContentType::ChangeCipherSpec as u8 {
            return false;
        }

        // Read record data (should be 1 byte: 0x01)
        let mut data = vec![0u8; header.length as usize];
        unsafe {
            let n = (*rbio).read(data.as_mut_ptr(), header.length as i32);
            if n != header.length as i32 {
                return false;
            }
        }

        // Verify CCS content
        data.len() == 1 && data[0] == 1
    }

    /// Write application data
    pub fn write_application_data(&mut self, data: &[u8], wbio: *mut Bio) -> bool {
        // Split into records if necessary
        let mut offset = 0;
        while offset < data.len() {
            let chunk_len = (data.len() - offset).min(MAX_RECORD_SIZE);
            let chunk = &data[offset..offset + chunk_len];
            
            if !self.write_record(ContentType::ApplicationData, chunk, wbio) {
                return false;
            }
            offset += chunk_len;
        }
        true
    }

    /// Write a TLS record
    fn write_record(&mut self, content_type: ContentType, data: &[u8], wbio: *mut Bio) -> bool {
        if wbio.is_null() {
            return false;
        }

        // Encrypt if keys are set
        let (record_data, length, outer_content_type) = if self.write_key.is_some() {
            match self.encrypt_record(content_type, data) {
                Some(encrypted) => {
                    let len = encrypted.len();
                    // TLS 1.3: outer content type is always ApplicationData
                    let outer_type = if self.version >= TLS1_3_VERSION {
                        ContentType::ApplicationData
                    } else {
                        content_type
                    };
                    (encrypted, len as u16, outer_type)
                }
                None => return false,
            }
        } else {
            (data.to_vec(), data.len() as u16, content_type)
        };

        // Build record header
        // TLS 1.3: always use TLS 1.2 version (0x0303) in record layer
        let header_version = if self.version >= TLS1_3_VERSION {
            TLS1_2_VERSION
        } else {
            self.version
        };
        let header = RecordHeader::new(outer_content_type, header_version, length);
        let header_bytes = header.to_bytes();

        // Write header
        unsafe {
            if (*wbio).write(header_bytes.as_ptr(), 5) != 5 {
                return false;
            }
            
            // Write data
            if (*wbio).write(record_data.as_ptr(), record_data.len() as c_int) != record_data.len() as c_int {
                return false;
            }
        }

        self.write_seq += 1;
        true
    }

    /// Read handshake message
    pub fn read_handshake(&mut self, rbio: *mut Bio) -> Option<Vec<u8>> {
        self.read_record(ContentType::Handshake, rbio)
    }

    /// Read application data
    pub fn read_application_data(&mut self, rbio: *mut Bio, buf: &mut [u8]) -> Option<usize> {
        // Check pending data first
        if !self.pending.is_empty() {
            let copy_len = buf.len().min(self.pending.len());
            buf[..copy_len].copy_from_slice(&self.pending[..copy_len]);
            self.pending.drain(..copy_len);
            return Some(copy_len);
        }

        // Read new record
        let data = self.read_record(ContentType::ApplicationData, rbio)?;
        
        let copy_len = buf.len().min(data.len());
        buf[..copy_len].copy_from_slice(&data[..copy_len]);
        
        // Store excess in pending buffer
        if data.len() > copy_len {
            self.pending.extend_from_slice(&data[copy_len..]);
        }
        
        Some(copy_len)
    }

    /// Read a TLS record
    fn read_record(&mut self, expected_type: ContentType, rbio: *mut Bio) -> Option<Vec<u8>> {
        if rbio.is_null() {
            eprintln!("[TLS-READ] rbio is null");
            return None;
        }

        // Read header - must loop to handle partial TCP reads
        let mut header_buf = [0u8; 5];
        let mut header_read = 0usize;
        while header_read < 5 {
            unsafe {
                let n = (*rbio).read(header_buf.as_mut_ptr().add(header_read), (5 - header_read) as c_int);
                eprintln!("[TLS-READ] read header: n={}, total={}", n, header_read + n.max(0) as usize);
                if n <= 0 {
                    if n == 0 && header_read == 0 {
                        self.eof = true;
                        eprintln!("[TLS-READ] EOF");
                    }
                    return None;
                }
                header_read += n as usize;
            }
        }

        let header = RecordHeader::from_bytes(&header_buf)?;
        eprintln!("[TLS-READ] header: type={:#x}, ver={:#x}{:02x}, len={}", 
            header.content_type, header.version_major, header.version_minor, header.length);
        
        // Check content type
        // TLS 1.3: encrypted records always have ApplicationData outer type
        let is_encrypted = self.read_key.is_some();
        let expected_outer_type = if is_encrypted && self.version >= TLS1_3_VERSION {
            ContentType::ApplicationData as u8
        } else {
            expected_type as u8
        };
        
        eprintln!("[TLS-READ] is_encrypted={}, expected_outer_type={:#x}", is_encrypted, expected_outer_type);
        
        if header.content_type != expected_outer_type {
            eprintln!("[TLS-READ] content type mismatch: got {:#x}, expected {:#x}", 
                header.content_type, expected_outer_type);
            // Handle alerts
            if header.content_type == ContentType::Alert as u8 {
                eprintln!("[TLS-READ] handling alert");
                self.handle_alert(rbio, header.length);
                return None;
            }
            // TLS 1.3: Handle ChangeCipherSpec for middlebox compatibility (RFC 8446 Section 5)
            // Some servers send CCS even in TLS 1.3 for compatibility reasons
            // We should read and ignore it, then retry reading the next record
            if self.version >= TLS1_3_VERSION && header.content_type == ContentType::ChangeCipherSpec as u8 {
                eprintln!("[TLS-READ] TLS 1.3 middlebox compatibility CCS, ignoring");
                // Read and discard CCS data (should be 1 byte: 0x01)
                let mut ccs_data = vec![0u8; header.length as usize];
                unsafe {
                    let _ = (*rbio).read(ccs_data.as_mut_ptr(), header.length as c_int);
                }
                // Recursively try to read the next record (the actual encrypted data)
                return self.read_record(expected_type, rbio);
            }
            // TLS 1.3 encrypted: might be receiving ApplicationData when expecting Handshake
            // This is normal - inner type is in the decrypted content
            if is_encrypted && self.version >= TLS1_3_VERSION && header.content_type == ContentType::ApplicationData as u8 {
                eprintln!("[TLS-READ] TLS 1.3 encrypted record, continuing");
                // Continue processing
            } else {
                return None;
            }
        }

        // Validate length
        if header.length as usize > MAX_RECORD_SIZE_WITH_OVERHEAD {
            eprintln!("[TLS-READ] record too large: {}", header.length);
            return None;
        }

        // Read record data - must loop to handle partial TCP reads
        let mut data = vec![0u8; header.length as usize];
        let mut total_read = 0usize;
        while total_read < header.length as usize {
            let remaining = header.length as usize - total_read;
            unsafe {
                let n = (*rbio).read(data.as_mut_ptr().add(total_read), remaining as c_int);
                if n <= 0 {
                    eprintln!("[TLS-READ] read failed: n={}, total_read={}, remaining={}", n, total_read, remaining);
                    return None;
                }
                eprintln!("[TLS-READ] read data: n={}, total_read={}/{}", n, total_read + n as usize, header.length);
                total_read += n as usize;
            }
        }
        
        // Debug: print hash of entire data to verify we read correctly
        let mut hash = 0u32;
        for (i, &b) in data.iter().enumerate() {
            hash = hash.wrapping_add(b as u32).wrapping_mul(31);
            if i < 16 || i >= data.len() - 16 {
                // Already printed first 32 bytes, also print last 16
                if i == data.len() - 16 {
                    eprintln!("[TLS-READ] data last 16 bytes: {:02x?}", &data[data.len()-16..]);
                }
            }
        }
        eprintln!("[TLS-READ] data checksum: {:#x}, len={}", hash, data.len());

        // Decrypt if keys are set
        let plaintext = if self.read_key.is_some() {
            self.decrypt_record(header.content_type, &data)?
        } else {
            data
        };

        self.read_seq += 1;
        Some(plaintext)
    }

    /// Encrypt a record
    fn encrypt_record(&self, content_type: ContentType, plaintext: &[u8]) -> Option<Vec<u8>> {
        let key = self.write_key.as_ref()?;
        let iv = self.write_iv.as_ref()?;
        
        // Build nonce from IV and sequence number
        let mut nonce = iv.clone();
        let seq_bytes = self.write_seq.to_be_bytes();
        let nonce_len = nonce.len();
        for i in 0..8 {
            nonce[nonce_len - 8 + i] ^= seq_bytes[i];
        }

        // TLS 1.3: append inner content type to plaintext
        let inner_plaintext = if self.version >= TLS1_3_VERSION {
            let mut inner = plaintext.to_vec();
            inner.push(content_type as u8);
            inner
        } else {
            plaintext.to_vec()
        };

        // Build AAD (additional authenticated data)
        // TLS 1.3: AAD = record header with ApplicationData type and encrypted length
        // TLS 1.2: AAD = seq_num (8) + record header
        let aad = if self.version >= TLS1_3_VERSION {
            // TLS 1.3 AAD: outer content type (0x17) + version (0x0303) + length
            // length includes inner_plaintext + 16-byte tag
            let encrypted_len = inner_plaintext.len() + 16;
            build_tls13_aad(encrypted_len as u16)
        } else {
            build_aad(content_type as u8, self.version, plaintext.len() as u16)
        };
        
        let plaintext_to_encrypt = if self.version >= TLS1_3_VERSION {
            &inner_plaintext
        } else {
            plaintext
        };

        // Encrypt using AES-GCM or ChaCha20-Poly1305
        // For simplicity, using AES-256-GCM
        if key.len() == 32 {
            let mut ciphertext = vec![0u8; plaintext_to_encrypt.len() + 16]; // +16 for tag
            
            // Use ncryptolib's AES-256-GCM
            let key_arr: [u8; 32] = key.as_slice().try_into().ok()?;
            let aes = ncryptolib::AesGcm::new_256(&key_arr);
            let (ct, tag) = aes.encrypt(&nonce, plaintext_to_encrypt, &aad);
            
            ciphertext[..ct.len()].copy_from_slice(&ct);
            ciphertext[ct.len()..].copy_from_slice(&tag);
            
            Some(ciphertext)
        } else if key.len() == 16 {
            let mut ciphertext = vec![0u8; plaintext_to_encrypt.len() + 16];
            
            // Use ncryptolib's AES-128-GCM
            let key_arr: [u8; 16] = key.as_slice().try_into().ok()?;
            let aes = ncryptolib::AesGcm::new_128(&key_arr);
            let (ct, tag) = aes.encrypt(&nonce, plaintext_to_encrypt, &aad);
            
            ciphertext[..ct.len()].copy_from_slice(&ct);
            ciphertext[ct.len()..].copy_from_slice(&tag);
            
            Some(ciphertext)
        } else {
            None
        }
    }

    /// Decrypt a record
    fn decrypt_record(&self, _content_type: u8, ciphertext: &[u8]) -> Option<Vec<u8>> {
        let key = self.read_key.as_ref()?;
        let iv = self.read_iv.as_ref()?;
        
        eprintln!("[TLS-DECRYPT] content_type={:#x}, ciphertext_len={}, read_seq={}", 
            _content_type, ciphertext.len(), self.read_seq);
        
        if ciphertext.len() < 16 {
            eprintln!("[TLS-DECRYPT] ciphertext too short for tag");
            return None; // Too short for tag
        }

        // Build nonce
        let mut nonce = iv.clone();
        let seq_bytes = self.read_seq.to_be_bytes();
        let nonce_len = nonce.len();
        for i in 0..8 {
            nonce[nonce_len - 8 + i] ^= seq_bytes[i];
        }
        
        eprintln!("[TLS-DECRYPT] nonce={:02x?}", &nonce);

        // Split ciphertext and tag
        let ct_len = ciphertext.len() - 16;
        let ct = &ciphertext[..ct_len];
        let tag = &ciphertext[ct_len..];
        
        eprintln!("[TLS-DECRYPT] ct_len={}, tag={:02x?}", ct_len, tag);
        eprintln!("[TLS-DECRYPT] ct first 32 bytes={:02x?}", &ct[..32.min(ct.len())]);

        // Build AAD
        // TLS 1.3: AAD is outer record header (type=0x17, version=0x0303, length)
        // TLS 1.2: AAD is content type + version + length
        let aad = if self.version >= TLS1_3_VERSION {
            build_tls13_aad(ciphertext.len() as u16)
        } else {
            build_aad(_content_type, self.version, ct_len as u16)
        };
        
        eprintln!("[TLS-DECRYPT] aad={:02x?}", &aad);

        // Decrypt using AES-GCM
        let plaintext = if key.len() == 32 {
            let key_arr: [u8; 32] = key.as_slice().try_into().ok()?;
            let tag_arr: [u8; 16] = tag.try_into().ok()?;
            let aes = ncryptolib::AesGcm::new_256(&key_arr);
            match aes.decrypt(&nonce, ct, &aad, &tag_arr) {
                Some(p) => p,
                None => {
                    eprintln!("[TLS-DECRYPT] AES-256-GCM decryption failed (tag mismatch)");
                    return None;
                }
            }
        } else if key.len() == 16 {
            let key_arr: [u8; 16] = key.as_slice().try_into().ok()?;
            let tag_arr: [u8; 16] = tag.try_into().ok()?;
            let aes = ncryptolib::AesGcm::new_128(&key_arr);
            match aes.decrypt(&nonce, ct, &aad, &tag_arr) {
                Some(p) => p,
                None => {
                    eprintln!("[TLS-DECRYPT] AES-128-GCM decryption failed (tag mismatch)");
                    return None;
                }
            }
        } else {
            eprintln!("[TLS-DECRYPT] unsupported key length: {}", key.len());
            return None;
        };
        
        eprintln!("[TLS-DECRYPT] decryption OK, plaintext_len={}", plaintext.len());
        
        // TLS 1.3: remove inner content type from plaintext
        if self.version >= TLS1_3_VERSION && !plaintext.is_empty() {
            // Remove trailing content type byte and any padding zeros
            let mut end = plaintext.len();
            while end > 0 && plaintext[end - 1] == 0 {
                end -= 1;
            }
            if end > 0 {
                // Last non-zero byte is the content type, remove it
                Some(plaintext[..end - 1].to_vec())
            } else {
                None
            }
        } else {
            Some(plaintext)
        }
    }

    /// Handle alert record
    fn handle_alert(&mut self, rbio: *mut Bio, length: u16) {
        if rbio.is_null() || length < 2 {
            return;
        }

        let mut alert = [0u8; 2];
        unsafe {
            let _ = (*rbio).read(alert.as_mut_ptr(), 2);
        }

        let level = alert[0];
        let desc = alert[1];

        // close_notify
        if level == 1 && desc == 0 {
            self.eof = true;
        }
    }

    /// Send alert
    pub fn send_alert(&mut self, level: u8, description: u8, wbio: *mut Bio) -> bool {
        let alert = [level, description];
        self.write_record(ContentType::Alert, &alert, wbio)
    }
}

/// Build Additional Authenticated Data for AEAD (TLS 1.2)
fn build_aad(content_type: u8, version: u16, length: u16) -> Vec<u8> {
    vec![
        content_type,
        (version >> 8) as u8,
        (version & 0xFF) as u8,
        (length >> 8) as u8,
        (length & 0xFF) as u8,
    ]
}

/// Build Additional Authenticated Data for TLS 1.3 AEAD
/// TLS 1.3 AAD format: 0x17 (ApplicationData) || 0x03 0x03 (TLS 1.2) || length (2 bytes)
fn build_tls13_aad(encrypted_length: u16) -> Vec<u8> {
    vec![
        0x17, // ApplicationData
        0x03, 0x03, // TLS 1.2 legacy version
        (encrypted_length >> 8) as u8,
        (encrypted_length & 0xFF) as u8,
    ]
}

impl Default for RecordLayer {
    fn default() -> Self {
        Self::new()
    }
}
