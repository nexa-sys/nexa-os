/// DNS response parser
///
/// Handles parsing of DNS response packets including A records,
/// CNAME records, and name compression.

/// Result of parsing a DNS response
#[derive(Debug, Clone, Copy)]
pub enum DnsParseOutcome {
    /// Found an A record with IPv4 address
    Address([u8; 4]),
    /// Found a CNAME record, length stored in buffer
    Cname(usize),
    /// No answer records found
    NoAnswer,
}

/// Parse a DNS response packet
///
/// # Arguments
/// * `data` - Raw DNS response bytes
/// * `expected_id` - Transaction ID to verify
/// * `cname_buf` - Buffer to store CNAME if found
/// * `scratch_buf` - Scratch buffer for additional section parsing
///
/// # Returns
/// * `Ok(DnsParseOutcome)` - Parse result
/// * `Err(&'static str)` - Error description
pub fn parse_dns_response(
    data: &[u8],
    expected_id: u16,
    cname_buf: &mut [u8],
    scratch_buf: &mut [u8],
) -> Result<DnsParseOutcome, &'static str> {
    // Minimum DNS response: 12 byte header + at least minimal question
    if data.len() < 12 {
        return Err("dns response too short");
    }
    
    // Maximum reasonable DNS response size (RFC 6891 recommends 4096 for EDNS)
    // But we only support 512 byte UDP responses without EDNS
    if data.len() > 512 {
        return Err("dns response too large");
    }

    // Validate transaction ID matches our query
    let transaction_id = u16::from_be_bytes([data[0], data[1]]);
    if transaction_id != expected_id {
        return Err("dns transaction id mismatch");
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    
    // QR bit (bit 15) must be 1 for response
    if flags & 0x8000 == 0 {
        return Err("dns packet is not a response");
    }
    
    // TC bit (bit 9) indicates truncation - response may be incomplete
    // For UDP we cannot recover from this, but we can try to parse what we have
    let truncated = flags & 0x0200 != 0;
    
    // RCODE (bits 0-3) should be 0 for success
    let rcode = flags & 0x000F;
    match rcode {
        0 => {} // NOERROR - success
        1 => return Err("dns format error"),
        2 => return Err("dns server failure"),
        3 => return Err("dns name does not exist"), // NXDOMAIN
        4 => return Err("dns not implemented"),
        5 => return Err("dns query refused"),
        _ => return Err("dns server returned error"),
    }

    let question_count = u16::from_be_bytes([data[4], data[5]]) as usize;
    let answer_count = u16::from_be_bytes([data[6], data[7]]) as usize;
    let authority_count = u16::from_be_bytes([data[8], data[9]]) as usize;
    let additional_count = u16::from_be_bytes([data[10], data[11]]) as usize;

    // Sanity check: prevent excessive iteration
    if question_count > 64 || answer_count > 256 || authority_count > 256 || additional_count > 256 {
        return Err("dns response has too many records");
    }
    
    // If truncated and no answers, we can't proceed
    if truncated && answer_count == 0 {
        return Err("dns response truncated with no answers");
    }

    let mut offset = 12;
    
    // Skip question section
    for _ in 0..question_count {
        skip_name(data, &mut offset)?;
        if offset + 4 > data.len() {
            return Err("dns question overflow");
        }
        offset += 4;
    }

    let mut last_cname_len: Option<usize> = None;

    for _ in 0..answer_count {
        skip_name(data, &mut offset)?;
        if offset + 10 > data.len() {
            return Err("dns answer header overflow");
        }
        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlength > data.len() {
            return Err("dns answer data overflow");
        }

        // Save rdata start position for CNAME parsing
        let rdata_start = offset;

        match rtype {
            1 => {
                // A record (IPv4 address)
                if rdlength != 4 {
                    return Err("invalid A record length");
                }
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&data[offset..offset + 4]);
                return Ok(DnsParseOutcome::Address(ip));
            }
            5 => {
                // CNAME record - read the canonical name
                // Note: read_name_into updates offset, but we need to ensure
                // we advance by exactly rdlength for proper packet parsing
                let mut temp_offset = rdata_start;
                let len = read_name_into(data, &mut temp_offset, cname_buf)?;
                last_cname_len = Some(len);
                // Skip past the entire rdata section
                offset = rdata_start + rdlength;
                continue;
            }
            28 => {
                // AAAA record (IPv6) - skip for now, we only support IPv4
                offset += rdlength;
                continue;
            }
            _ => {
                // Unknown record type - skip
            }
        }
        offset += rdlength;
    }

    for _ in 0..authority_count {
        skip_resource_record(data, &mut offset)?;
    }

    if let Some(len) = last_cname_len {
        let cname = &cname_buf[..len];
        // Try to find a matching A record in additional section
        // This optimization avoids an extra DNS query for the CNAME target
        for _ in 0..additional_count {
            let name_len = match read_name_into(data, &mut offset, scratch_buf) {
                Ok(l) => l,
                Err(_) => {
                    // If we can't parse additional records, just return CNAME
                    // The caller will do a followup query
                    return Ok(DnsParseOutcome::Cname(len));
                }
            };
            if offset + 10 > data.len() {
                // Truncated additional section, return CNAME for followup
                return Ok(DnsParseOutcome::Cname(len));
            }
            let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
            offset += 10;
            if offset + rdlength > data.len() {
                // Truncated data, return CNAME for followup
                return Ok(DnsParseOutcome::Cname(len));
            }
            if rtype == 1
                && rdlength == 4
                && name_len == len
                && dns_name_equals(&scratch_buf[..name_len], cname)
            {
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&data[offset..offset + 4]);
                return Ok(DnsParseOutcome::Address(ip));
            }
            offset += rdlength;
        }
        return Ok(DnsParseOutcome::Cname(len));
    }
    
    // Skip additional section if we didn't find any answers
    // (already skipped if we had a CNAME)

    for _ in 0..additional_count {
        skip_resource_record(data, &mut offset)?;
    }

    Ok(DnsParseOutcome::NoAnswer)
}

/// Skip a resource record in DNS packet
fn skip_resource_record(data: &[u8], offset: &mut usize) -> Result<(), &'static str> {
    skip_name(data, offset)?;
    if *offset + 10 > data.len() {
        return Err("dns rr header overflow");
    }
    let rdlength = u16::from_be_bytes([data[*offset + 8], data[*offset + 9]]) as usize;
    *offset += 10;
    if *offset + rdlength > data.len() {
        return Err("dns rr data overflow");
    }
    *offset += rdlength;
    Ok(())
}

/// Skip a DNS name (handles compression pointers)
pub(crate) fn skip_name(data: &[u8], offset: &mut usize) -> Result<(), &'static str> {
    let mut pos = *offset;
    let mut jumped = false;
    let mut steps = 0;
    let initial_offset = *offset;

    loop {
        if pos >= data.len() {
            return Err("dns name exceeds packet");
        }
        let len = data[pos];
        
        // Check for compression pointer (top 2 bits = 11)
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err("dns name pointer overflow");
            }
            let ptr = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            // Pointer must point to earlier in the packet (forward references not allowed)
            // Also prevent self-referencing pointers
            if ptr >= data.len() || ptr >= initial_offset || ptr == pos {
                return Err("dns name pointer out of bounds");
            }
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            steps += 1;
            // Limit pointer hops to prevent infinite loops
            if steps > 128 {
                return Err("dns name pointer loop");
            }
            continue;
        }
        
        // Check for reserved label type (top 2 bits = 01 or 10)
        if len & 0xC0 != 0 {
            return Err("dns invalid label type");
        }

        if len == 0 {
            if !jumped {
                *offset = pos + 1;
            }
            return Ok(());
        }

        pos += 1;
        if pos + len as usize > data.len() {
            return Err("dns label exceeds packet");
        }
        pos += len as usize;
        if !jumped {
            *offset = pos;
        }
    }
}

/// Read a DNS name into buffer (handles compression pointers)
pub(crate) fn read_name_into(data: &[u8], offset: &mut usize, out: &mut [u8]) -> Result<usize, &'static str> {
    let mut pos = *offset;
    let mut jumped = false;
    let mut steps = 0;
    let mut buf_pos = 0;
    let mut total_len = 0usize; // Track total name length
    let initial_offset = *offset;

    loop {
        if pos >= data.len() {
            return Err("dns name exceeds packet");
        }
        let len = data[pos];
        
        // Check for compression pointer
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return Err("dns name pointer overflow");
            }
            let ptr = (((len & 0x3F) as usize) << 8) | (data[pos + 1] as usize);
            // Pointer must point to earlier in the packet (prevent forward and self references)
            if ptr >= data.len() || ptr >= initial_offset || ptr == pos {
                return Err("dns name pointer out of bounds");
            }
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            steps += 1;
            if steps > 128 {
                return Err("dns name pointer loop");
            }
            continue;
        }
        
        // Check for reserved label type
        if len & 0xC0 != 0 {
            return Err("dns invalid label type");
        }

        if len == 0 {
            if !jumped {
                *offset = pos + 1;
            }
            if buf_pos < out.len() {
                out[buf_pos] = 0;
            }
            return Ok(buf_pos);
        }
        
        // Validate total name length won't exceed DNS limit (253 chars)
        total_len += len as usize + 1; // +1 for dot separator
        if total_len > 254 {
            return Err("dns name too long");
        }

        pos += 1;
        if pos + len as usize > data.len() {
            return Err("dns label exceeds packet");
        }

        if buf_pos != 0 {
            if buf_pos >= out.len() {
                return Err("dns name too long");
            }
            out[buf_pos] = b'.';
            buf_pos += 1;
        }

        if buf_pos + len as usize > out.len() {
            return Err("dns name too long");
        }
        out[buf_pos..buf_pos + len as usize].copy_from_slice(&data[pos..pos + len as usize]);
        buf_pos += len as usize;
        pos += len as usize;

        if !jumped {
            *offset = pos;
        }
    }
}

/// Compare two DNS names case-insensitively
pub fn dns_name_equals(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if to_ascii_lower(a[i]) != to_ascii_lower(b[i]) {
            return false;
        }
    }
    true
}

/// Convert ASCII character to lowercase
#[inline]
fn to_ascii_lower(byte: u8) -> u8 {
    if byte >= b'A' && byte <= b'Z' {
        byte + 32
    } else {
        byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_HOSTNAME: usize = 256;

    #[test]
    fn test_parse_dns_response_a_record() {
        let response: [u8; 45] = [
            0x30, 0x30, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, // header
            0x05, b'b', b'a', b'i', b'd', b'u', 0x03, b'c', b'o', b'm', 0x00, // question name
            0x00, 0x01, 0x00, 0x01, // qtype, qclass
            0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04,
            0xC6, 0x12, 0x00, 0x65, // answer A record 198.18.0.101
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        match parse_dns_response(&response, 0x3030, &mut cname, &mut scratch).unwrap() {
            DnsParseOutcome::Address(ip) => assert_eq!(ip, [0xC6, 0x12, 0x00, 0x65]),
            other => panic!("unexpected parse result: {:?}", other),
        }
    }

    #[test]
    fn test_parse_dns_response_wrong_id() {
        let response: [u8; 45] = [
            0x30, 0x30, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x05, b'b', b'a', b'i', b'd', b'u', 0x03, b'c', b'o', b'm', 0x00,
            0x00, 0x01, 0x00, 0x01,
            0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04,
            0xC6, 0x12, 0x00, 0x65,
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        let result = parse_dns_response(&response, 0x1234, &mut cname, &mut scratch);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dns_response_nxdomain() {
        // NXDOMAIN response (RCODE = 3)
        let response: [u8; 25] = [
            0x12, 0x34, 0x81, 0x83, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00,
            0x00, 0x01, 0x00, 0x01,
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        let result = parse_dns_response(&response, 0x1234, &mut cname, &mut scratch);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dns_response_too_short() {
        let response: [u8; 8] = [0x12, 0x34, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01];
        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        let result = parse_dns_response(&response, 0x1234, &mut cname, &mut scratch);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dns_response_cname_with_glue() {
        let response: [u8; 87] = [
            0x11, 0x11, 0x85, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // header
            0x05, b'a', b'l', b'i', b'a', b's', 0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00, // question
            0x00, 0x01, 0x00, 0x01,
            0xC0, 0x0C, 0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11, // answer header
            0x06, b't', b'a', b'r', b'g', b'e', b't', 0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00, // canonical name
            0x06, b't', b'a', b'r', b'g', b'e', b't', 0x04, b't', b'e', b's', b't', 0x03, b'c', b'o', b'm', 0x00, // additional name
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
            0x01, 0x02, 0x03, 0x04, // glue A record
        ];

        let mut cname = [0u8; MAX_HOSTNAME];
        let mut scratch = [0u8; MAX_HOSTNAME];
        match parse_dns_response(&response, 0x1111, &mut cname, &mut scratch).unwrap() {
            DnsParseOutcome::Address(ip) => assert_eq!(ip, [1, 2, 3, 4]),
            other => panic!("unexpected parse result: {:?}", other),
        }
    }

    #[test]
    fn test_dns_name_equals() {
        assert!(dns_name_equals(b"example.com", b"example.com"));
        assert!(dns_name_equals(b"EXAMPLE.COM", b"example.com"));
        assert!(dns_name_equals(b"Example.Com", b"EXAMPLE.COM"));
        assert!(!dns_name_equals(b"example.com", b"example.org"));
        assert!(!dns_name_equals(b"example.com", b"example.co"));
    }
}
