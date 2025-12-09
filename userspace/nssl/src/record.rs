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
    /// Pending application data buffer
    pending: Vec<u8>,
    /// Pending handshake data buffer (for TLS 1.3 coalesced messages)
    pending_handshake: Vec<u8>,
    /// EOF flag
    eof: bool,
    /// Protocol version
    version: u16,
    /// Last decrypted inner content type (TLS 1.3)
    last_inner_content_type: Option<u8>,
    /// Whether read encryption is enabled (after receiving CCS)
    read_encrypt_enabled: bool,
    /// Whether write encryption is enabled (after sending CCS)
    write_encrypt_enabled: bool,
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
            pending_handshake: Vec::new(),
            eof: false,
            version: TLS1_2_VERSION,
            last_inner_content_type: None,
            read_encrypt_enabled: false,
            write_encrypt_enabled: false,
        }
    }

    /// Set encryption keys
    /// 
    /// # Arguments
    /// * `enable_immediately` - If true, encryption is enabled immediately (TLS 1.3).
    ///                          If false, encryption must be enabled later via CCS (TLS 1.2).
    pub fn set_keys(&mut self, read_key: Vec<u8>, write_key: Vec<u8>, read_iv: Vec<u8>, write_iv: Vec<u8>, enable_immediately: bool) {
        self.read_key = Some(read_key);
        self.write_key = Some(write_key);
        self.read_iv = Some(read_iv);
        self.write_iv = Some(write_iv);
        self.read_seq = 0;
        self.write_seq = 0;
        if enable_immediately {
            self.read_encrypt_enabled = true;
            self.write_encrypt_enabled = true;
        }
    }
    
    /// Enable write encryption (called after sending ChangeCipherSpec)
    pub fn enable_write_encryption(&mut self) {
        self.write_encrypt_enabled = true;
        // Reset write sequence number for encrypted records
        self.write_seq = 0;
    }
    
    /// Enable read encryption (called after receiving ChangeCipherSpec)
    pub fn enable_read_encryption(&mut self) {
        self.read_encrypt_enabled = true;
        // Reset read sequence number for encrypted records
        self.read_seq = 0;
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
            // If it's an Alert (21), consume the data
            if header.content_type == ContentType::Alert as u8 && header.length >= 2 {
                let mut alert_data = vec![0u8; header.length as usize];
                unsafe {
                    (*rbio).read(alert_data.as_mut_ptr(), header.length as i32);
                }
            }
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

        // Encrypt if keys are set AND encryption is enabled
        let (record_data, length, outer_content_type) = if self.write_key.is_some() && self.write_encrypt_enabled {
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
    /// In TLS 1.3, multiple handshake messages can be coalesced into a single record.
    /// This function handles that by buffering excess data and parsing one message at a time.
    pub fn read_handshake(&mut self, rbio: *mut Bio) -> Option<Vec<u8>> {
        // First check if we have buffered handshake data from a previous coalesced record
        if !self.pending_handshake.is_empty() {
            // Try to parse a complete handshake message from the buffer
            if let Some(msg) = self.extract_handshake_message() {
                return Some(msg);
            }
        }
        
        // Read a new record
        let data = self.read_record(ContentType::Handshake, rbio)?;
        
        // Add to pending handshake buffer
        self.pending_handshake.extend_from_slice(&data);
        
        // Extract one handshake message
        self.extract_handshake_message()
    }
    
    /// Extract a single handshake message from pending_handshake buffer
    fn extract_handshake_message(&mut self) -> Option<Vec<u8>> {
        if self.pending_handshake.len() < 4 {
            return None;
        }
        
        // Handshake message format:
        // - type: 1 byte
        // - length: 3 bytes (big-endian)
        // - data: length bytes
        let msg_type = self.pending_handshake[0];
        let msg_len = ((self.pending_handshake[1] as usize) << 16) 
                    | ((self.pending_handshake[2] as usize) << 8) 
                    | (self.pending_handshake[3] as usize);
        let total_len = 4 + msg_len;
        
        if self.pending_handshake.len() < total_len {
            // Not enough data yet
            return None;
        }
        
        // Extract the complete message
        let msg: Vec<u8> = self.pending_handshake.drain(..total_len).collect();
        
        Some(msg)
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

        // Read new record - may need to loop to skip TLS 1.3 post-handshake messages
        loop {
            let data = self.read_record(ContentType::ApplicationData, rbio)?;

            // In TLS 1.3, check the inner content type
            // If it's not ApplicationData (0x17), skip this record (e.g., NewSessionTicket = 0x16)
            if self.version >= TLS1_3_VERSION {
                if let Some(inner_ct) = self.last_inner_content_type {
                    if inner_ct == ContentType::Alert as u8 {
                        // Handle alert (close_notify)
                        if data.len() >= 2 && data[0] == 1 && data[1] == 0 {
                            // close_notify
                            self.eof = true;
                            return None;
                        }
                        self.eof = true;
                        return None;
                    }
                    if inner_ct != ContentType::ApplicationData as u8 {
                        // Not application data (e.g., NewSessionTicket = 0x16), skip
                        continue;
                    }
                }
            }
            
            let copy_len = buf.len().min(data.len());
            buf[..copy_len].copy_from_slice(&data[..copy_len]);
            
            // Store excess in pending buffer
            if data.len() > copy_len {
                self.pending.extend_from_slice(&data[copy_len..]);
            }
            
            return Some(copy_len);
        }
    }

    /// Read a TLS record
    fn read_record(&mut self, expected_type: ContentType, rbio: *mut Bio) -> Option<Vec<u8>> {
        if rbio.is_null() {
            return None;
        }

        // Read header - must loop to handle partial TCP reads
        let mut header_buf = [0u8; 5];
        let mut header_read = 0usize;
        while header_read < 5 {
            unsafe {
                let n = (*rbio).read(header_buf.as_mut_ptr().add(header_read), (5 - header_read) as c_int);
                if n <= 0 {
                    if n == 0 && header_read == 0 {
                        self.eof = true;
                    }
                    return None;
                }
                header_read += n as usize;
            }
        }

        let header = RecordHeader::from_bytes(&header_buf)?;
        
        // Check content type
        // TLS 1.3: encrypted records always have ApplicationData outer type
        // Use read_encrypt_enabled instead of just checking if key exists
        let is_encrypted = self.read_key.is_some() && self.read_encrypt_enabled;
        let expected_outer_type = if is_encrypted && self.version >= TLS1_3_VERSION {
            ContentType::ApplicationData as u8
        } else {
            expected_type as u8
        };
        
        if header.content_type != expected_outer_type {
            // Handle alerts
            if header.content_type == ContentType::Alert as u8 {
                self.handle_alert(rbio, header.length);
                return None;
            }
            // TLS 1.3: Handle ChangeCipherSpec for middlebox compatibility (RFC 8446 Section 5)
            // Some servers send CCS even in TLS 1.3 for compatibility reasons
            // We should read and ignore it, then retry reading the next record
            if self.version >= TLS1_3_VERSION && header.content_type == ContentType::ChangeCipherSpec as u8 {
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
                // Continue processing
            } else {
                return None;
            }
        }

        // Validate length
        if header.length as usize > MAX_RECORD_SIZE_WITH_OVERHEAD {
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
                    return None;
                }
                total_read += n as usize;
            }
        }

        // Decrypt if keys are set AND read encryption is enabled
        let plaintext = if self.read_key.is_some() && self.read_encrypt_enabled {
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
        
        // TLS 1.2 vs TLS 1.3 have different nonce construction:
        // - TLS 1.3: 12-byte IV XOR'd with 8-byte sequence number (padded to 12 bytes)
        // - TLS 1.2 GCM: 4-byte implicit IV || 8-byte explicit nonce (random or seq_num)
        let (nonce, explicit_nonce) = if self.version >= TLS1_3_VERSION {
            // TLS 1.3: XOR IV with sequence number
            let mut nonce = iv.clone();
            let seq_bytes = self.write_seq.to_be_bytes();
            let nonce_len = nonce.len();
            for i in 0..8 {
                nonce[nonce_len - 8 + i] ^= seq_bytes[i];
            }
            (nonce, None)
        } else {
            // TLS 1.2 GCM: nonce = implicit_iv (4 bytes) || explicit_nonce (8 bytes)
            // explicit_nonce is typically the sequence number
            let seq_bytes = self.write_seq.to_be_bytes();
            let mut nonce = Vec::with_capacity(12);
            nonce.extend_from_slice(iv); // 4-byte implicit IV
            nonce.extend_from_slice(&seq_bytes); // 8-byte explicit nonce
            (nonce, Some(seq_bytes.to_vec()))
        };

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
        // TLS 1.2: AAD = seq_num (8) + content_type (1) + version (2) + length (2)
        let aad = if self.version >= TLS1_3_VERSION {
            // TLS 1.3 AAD: outer content type (0x17) + version (0x0303) + length
            // length includes inner_plaintext + 16-byte tag
            let encrypted_len = inner_plaintext.len() + 16;
            build_tls13_aad(encrypted_len as u16)
        } else {
            // TLS 1.2 GCM AAD includes sequence number
            build_aad_tls12(self.write_seq, content_type as u8, self.version, plaintext.len() as u16)
        };
        
        let plaintext_to_encrypt = if self.version >= TLS1_3_VERSION {
            &inner_plaintext
        } else {
            plaintext
        };

        // Encrypt using AES-GCM or ChaCha20-Poly1305
        // For simplicity, using AES-256-GCM
        let encrypted = if key.len() == 32 {
            // Use ncryptolib's AES-256-GCM
            let key_arr: [u8; 32] = key.as_slice().try_into().ok()?;
            let aes = crate::ncryptolib::AesGcm::new_256(&key_arr);
            let (ct, tag) = aes.encrypt(&nonce, plaintext_to_encrypt, &aad);
            
            let mut ciphertext = vec![0u8; ct.len() + 16];
            ciphertext[..ct.len()].copy_from_slice(&ct);
            ciphertext[ct.len()..].copy_from_slice(&tag);
            Some(ciphertext)
        } else if key.len() == 16 {
            // Use ncryptolib's AES-128-GCM
            let key_arr: [u8; 16] = key.as_slice().try_into().ok()?;
            let aes = crate::ncryptolib::AesGcm::new_128(&key_arr);
            let (ct, tag) = aes.encrypt(&nonce, plaintext_to_encrypt, &aad);
            
            let mut ciphertext = vec![0u8; ct.len() + 16];
            ciphertext[..ct.len()].copy_from_slice(&ct);
            ciphertext[ct.len()..].copy_from_slice(&tag);
            Some(ciphertext)
        } else {
            None
        };
        
        // For TLS 1.2, prepend the explicit nonce to the ciphertext
        if let Some(explicit) = explicit_nonce {
            if let Some(enc) = encrypted {
                let mut result = Vec::with_capacity(explicit.len() + enc.len());
                result.extend_from_slice(&explicit);
                result.extend_from_slice(&enc);
                return Some(result);
            }
            return None;
        }
        
        encrypted
    }

    /// Decrypt a record
    fn decrypt_record(&mut self, _content_type: u8, ciphertext: &[u8]) -> Option<Vec<u8>> {
        let key = self.read_key.as_ref()?;
        let iv = self.read_iv.as_ref()?;
        
        // TLS 1.2 GCM: ciphertext = explicit_nonce (8) || encrypted_data || tag (16)
        // TLS 1.3: ciphertext = encrypted_data || tag (16)
        let min_len = if self.version >= TLS1_3_VERSION { 16 } else { 8 + 16 };
        if ciphertext.len() < min_len {
            return None; // Too short
        }

        // Build nonce and extract ciphertext based on version
        let (nonce, ct, tag, plaintext_len) = if self.version >= TLS1_3_VERSION {
            // TLS 1.3: XOR IV with sequence number
            let mut nonce = iv.clone();
            let seq_bytes = self.read_seq.to_be_bytes();
            let nonce_len = nonce.len();
            for i in 0..8 {
                nonce[nonce_len - 8 + i] ^= seq_bytes[i];
            }
            
            let ct_len = ciphertext.len() - 16;
            let ct = &ciphertext[..ct_len];
            let tag = &ciphertext[ct_len..];
            (nonce, ct, tag, ct_len)
        } else {
            // TLS 1.2 GCM: extract explicit nonce from ciphertext
            // nonce = implicit_iv (4 bytes) || explicit_nonce (8 bytes from ciphertext)
            let explicit_nonce = &ciphertext[..8];
            let remaining = &ciphertext[8..];
            
            if remaining.len() < 16 {
                return None;
            }
            
            let ct_len = remaining.len() - 16;
            let ct = &remaining[..ct_len];
            let tag = &remaining[ct_len..];
            
            let mut nonce = Vec::with_capacity(12);
            nonce.extend_from_slice(iv); // 4-byte implicit IV
            nonce.extend_from_slice(explicit_nonce); // 8-byte explicit nonce
            
            (nonce, ct, tag, ct_len)
        };

        // Build AAD
        // TLS 1.3: AAD is outer record header (type=0x17, version=0x0303, length)
        // TLS 1.2: AAD is seq_num (8) + content_type (1) + version (2) + plaintext_length (2)
        let aad = if self.version >= TLS1_3_VERSION {
            build_tls13_aad(ciphertext.len() as u16)
        } else {
            // For TLS 1.2, AAD uses the plaintext length (without explicit nonce and tag)
            build_aad_tls12(self.read_seq, _content_type, self.version, plaintext_len as u16)
        };

        // Decrypt using AES-GCM
        let plaintext = if key.len() == 32 {
            let key_arr: [u8; 32] = key.as_slice().try_into().ok()?;
            let tag_arr: [u8; 16] = tag.try_into().ok()?;
            let aes = crate::ncryptolib::AesGcm::new_256(&key_arr);
            aes.decrypt(&nonce, ct, &aad, &tag_arr)?
        } else if key.len() == 16 {
            let key_arr: [u8; 16] = key.as_slice().try_into().ok()?;
            let tag_arr: [u8; 16] = tag.try_into().ok()?;
            let aes = crate::ncryptolib::AesGcm::new_128(&key_arr);
            aes.decrypt(&nonce, ct, &aad, &tag_arr)?
        } else {
            return None;
        };
        
        // TLS 1.3: remove inner content type from plaintext and return it
        if self.version >= TLS1_3_VERSION && !plaintext.is_empty() {
            // Remove trailing content type byte and any padding zeros
            let mut end = plaintext.len();
            while end > 0 && plaintext[end - 1] == 0 {
                end -= 1;
            }
            if end > 0 {
                // Last non-zero byte is the content type, remove it
                let inner_content_type = plaintext[end - 1];
                // Store inner content type for caller to check
                self.last_inner_content_type = Some(inner_content_type);
                Some(plaintext[..end - 1].to_vec())
            } else {
                None
            }
        } else {
            self.last_inner_content_type = None;
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
/// TLS 1.2 GCM AAD format: seq_num (8 bytes) || content_type (1 byte) || version (2 bytes) || length (2 bytes)
fn build_aad_tls12(seq_num: u64, content_type: u8, version: u16, length: u16) -> Vec<u8> {
    let seq_bytes = seq_num.to_be_bytes();
    vec![
        seq_bytes[0], seq_bytes[1], seq_bytes[2], seq_bytes[3],
        seq_bytes[4], seq_bytes[5], seq_bytes[6], seq_bytes[7],
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
