//! PEM Encoding/Decoding
//!
//! Provides PEM format handling for certificates and keys.

use std::vec::Vec;
use std::string::String;
use crate::encoding::{base64_encode, base64_decode};

/// PEM header/footer markers
pub mod pem_types {
    pub const CERTIFICATE: &str = "CERTIFICATE";
    pub const X509_CRL: &str = "X509 CRL";
    pub const CERTIFICATE_REQUEST: &str = "CERTIFICATE REQUEST";
    pub const PRIVATE_KEY: &str = "PRIVATE KEY";
    pub const RSA_PRIVATE_KEY: &str = "RSA PRIVATE KEY";
    pub const RSA_PUBLIC_KEY: &str = "RSA PUBLIC KEY";
    pub const EC_PRIVATE_KEY: &str = "EC PRIVATE KEY";
    pub const PUBLIC_KEY: &str = "PUBLIC KEY";
    pub const ENCRYPTED_PRIVATE_KEY: &str = "ENCRYPTED PRIVATE KEY";
}

/// PEM block structure
#[derive(Clone)]
pub struct PemBlock {
    /// Block type (e.g., "CERTIFICATE", "PRIVATE KEY")
    pub block_type: String,
    /// Headers (name: value pairs)
    pub headers: Vec<(String, String)>,
    /// DER-encoded data
    pub data: Vec<u8>,
}

impl PemBlock {
    /// Create new PEM block
    pub fn new(block_type: &str, data: Vec<u8>) -> Self {
        Self {
            block_type: block_type.to_string(),
            headers: Vec::new(),
            data,
        }
    }

    /// Add header
    pub fn add_header(&mut self, name: &str, value: &str) {
        self.headers.push((name.to_string(), value.to_string()));
    }

    /// Encode to PEM format
    pub fn encode(&self) -> String {
        let mut result = String::new();
        
        // Begin marker
        result.push_str("-----BEGIN ");
        result.push_str(&self.block_type);
        result.push_str("-----\n");
        
        // Headers
        for (name, value) in &self.headers {
            result.push_str(name);
            result.push_str(": ");
            result.push_str(value);
            result.push('\n');
        }
        
        if !self.headers.is_empty() {
            result.push('\n'); // Blank line after headers
        }
        
        // Base64 encoded data (64 chars per line)
        let b64 = base64_encode(&self.data);
        for chunk in b64.as_bytes().chunks(64) {
            result.push_str(core::str::from_utf8(chunk).unwrap_or(""));
            result.push('\n');
        }
        
        // End marker
        result.push_str("-----END ");
        result.push_str(&self.block_type);
        result.push_str("-----\n");
        
        result
    }
}

/// Parse PEM data and extract all blocks
pub fn pem_decode(pem: &[u8]) -> Vec<PemBlock> {
    let mut blocks = Vec::new();
    let text = match std::str::from_utf8(pem) {
        Ok(s) => s,
        Err(_) => return blocks,
    };
    
    let mut pos = 0;
    while pos < text.len() {
        // Find begin marker
        let begin_prefix = "-----BEGIN ";
        if let Some(begin_idx) = text[pos..].find(begin_prefix) {
            let abs_begin = pos + begin_idx;
            let after_begin = abs_begin + begin_prefix.len();
            
            // Find end of begin line
            if let Some(dash_idx) = text[after_begin..].find("-----") {
                let block_type = &text[after_begin..after_begin + dash_idx];
                let after_begin_line = after_begin + dash_idx + 5;
                
                // Find end marker
                let end_marker = format!("-----END {}-----", block_type);
                if let Some(end_idx) = text[after_begin_line..].find(&end_marker) {
                    let content = &text[after_begin_line..after_begin_line + end_idx];
                    
                    // Parse headers and base64 data
                    let (headers, b64_data) = parse_pem_content(content);
                    
                    // Decode base64
                    let cleaned: String = b64_data.chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    
                    if let Ok(data) = base64_decode(&cleaned) {
                        let mut block = PemBlock::new(block_type, data);
                        block.headers = headers;
                        blocks.push(block);
                    }
                    
                    pos = after_begin_line + end_idx + end_marker.len();
                    continue;
                }
            }
        }
        break;
    }
    
    blocks
}

/// Parse PEM content into headers and base64 data
fn parse_pem_content(content: &str) -> (Vec<(String, String)>, &str) {
    let mut headers = Vec::new();
    let mut lines = content.lines().peekable();
    let mut data_start = 0;
    let mut in_headers = true;
    
    for line in content.lines() {
        if in_headers {
            if line.is_empty() {
                in_headers = false;
                data_start += line.len() + 1;
                continue;
            }
            
            if let Some(colon_idx) = line.find(':') {
                let name = line[..colon_idx].trim();
                let value = line[colon_idx + 1..].trim();
                headers.push((name.to_string(), value.to_string()));
                data_start += line.len() + 1;
            } else {
                // No more headers
                in_headers = false;
            }
        }
    }
    
    let data = if data_start > 0 && data_start < content.len() {
        &content[data_start..]
    } else {
        content
    };
    
    (headers, data)
}

/// Decode single PEM block
pub fn pem_decode_one(pem: &[u8], expected_type: &str) -> Option<Vec<u8>> {
    let blocks = pem_decode(pem);
    for block in blocks {
        if block.block_type == expected_type {
            return Some(block.data);
        }
    }
    None
}

/// Encode data to PEM format
pub fn pem_encode(block_type: &str, data: &[u8]) -> String {
    PemBlock::new(block_type, data.to_vec()).encode()
}

// ============================================================================
// C ABI Exports
// ============================================================================

/// PEM_read_bio - Read PEM from BIO (simplified)
#[no_mangle]
pub extern "C" fn PEM_read_bio(
    _bio: *mut core::ffi::c_void,
    _name: *mut *mut i8,
    _header: *mut *mut i8,
    _data: *mut *mut u8,
    _len: *mut i64,
) -> i32 {
    // Simplified stub
    0
}

/// PEM_write_bio - Write PEM to BIO (simplified)
#[no_mangle]
pub extern "C" fn PEM_write_bio(
    _bio: *mut core::ffi::c_void,
    _name: *const i8,
    _header: *const i8,
    _data: *const u8,
    _len: i64,
) -> i32 {
    // Simplified stub
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pem_encode_decode() {
        let data = vec![0x30, 0x82, 0x01, 0x22]; // Sample DER data
        let pem = pem_encode("CERTIFICATE", &data);
        
        assert!(pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(pem.contains("-----END CERTIFICATE-----"));
        
        let decoded = pem_decode_one(pem.as_bytes(), "CERTIFICATE");
        assert_eq!(decoded, Some(data));
    }
}
