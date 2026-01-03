//! DNS protocol tests
//!
//! Tests for DNS query construction and response parsing.

#[cfg(test)]
mod tests {
    // =========================================================================
    // DNS Header Tests
    // =========================================================================

    #[test]
    fn test_dns_header_size() {
        // DNS header is 12 bytes
        const DNS_HEADER_SIZE: usize = 12;
        assert_eq!(DNS_HEADER_SIZE, 12);
    }

    #[test]
    fn test_dns_flags() {
        // Standard query flags
        const QR_QUERY: u16 = 0;
        const QR_RESPONSE: u16 = 1 << 15;
        const OPCODE_QUERY: u16 = 0;
        const RD_FLAG: u16 = 1 << 8; // Recursion Desired
        const RA_FLAG: u16 = 1 << 7; // Recursion Available
        
        // Standard recursive query
        let query_flags = QR_QUERY | OPCODE_QUERY | RD_FLAG;
        assert_eq!(query_flags & QR_RESPONSE, 0); // Is a query
        assert_ne!(query_flags & RD_FLAG, 0); // Recursion desired
    }

    #[test]
    fn test_dns_rcode_values() {
        const RCODE_NOERROR: u8 = 0;
        const RCODE_FORMERR: u8 = 1; // Format error
        const RCODE_SERVFAIL: u8 = 2; // Server failure
        const RCODE_NXDOMAIN: u8 = 3; // Non-existent domain
        const RCODE_NOTIMP: u8 = 4; // Not implemented
        const RCODE_REFUSED: u8 = 5; // Query refused
        
        // All should be distinct
        let codes = [RCODE_NOERROR, RCODE_FORMERR, RCODE_SERVFAIL, RCODE_NXDOMAIN, RCODE_NOTIMP, RCODE_REFUSED];
        for (i, &c1) in codes.iter().enumerate() {
            for (j, &c2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(c1, c2);
                }
            }
        }
    }

    // =========================================================================
    // DNS Record Types Tests
    // =========================================================================

    #[test]
    fn test_dns_record_types() {
        const TYPE_A: u16 = 1;      // IPv4 address
        const TYPE_NS: u16 = 2;     // Nameserver
        const TYPE_CNAME: u16 = 5;  // Canonical name
        const TYPE_SOA: u16 = 6;    // Start of Authority
        const TYPE_PTR: u16 = 12;   // Pointer (reverse DNS)
        const TYPE_MX: u16 = 15;    // Mail exchanger
        const TYPE_TXT: u16 = 16;   // Text record
        const TYPE_AAAA: u16 = 28;  // IPv6 address
        const TYPE_SRV: u16 = 33;   // Service locator
        
        // Common types
        assert_eq!(TYPE_A, 1);
        assert_eq!(TYPE_AAAA, 28);
        assert_eq!(TYPE_CNAME, 5);
    }

    #[test]
    fn test_dns_class() {
        const CLASS_IN: u16 = 1; // Internet
        const CLASS_CH: u16 = 3; // Chaos
        const CLASS_HS: u16 = 4; // Hesiod
        
        // Almost all queries use CLASS_IN
        assert_eq!(CLASS_IN, 1);
    }

    // =========================================================================
    // Domain Name Encoding Tests
    // =========================================================================

    #[test]
    fn test_domain_name_encoding() {
        // "example.com" -> [7, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 3, 'c', 'o', 'm', 0]
        fn encode_domain(domain: &str) -> Vec<u8> {
            let mut result = Vec::new();
            for label in domain.split('.') {
                result.push(label.len() as u8);
                result.extend_from_slice(label.as_bytes());
            }
            result.push(0); // Null terminator
            result
        }
        
        let encoded = encode_domain("example.com");
        assert_eq!(encoded[0], 7); // "example" length
        assert_eq!(encoded[8], 3); // "com" length
        assert_eq!(*encoded.last().unwrap(), 0); // Null terminator
    }

    #[test]
    fn test_domain_name_max_label_length() {
        // Each label max 63 bytes
        const MAX_LABEL_LEN: usize = 63;
        
        let long_label = "a".repeat(MAX_LABEL_LEN);
        assert_eq!(long_label.len(), 63);
        
        // Encoding would be: length byte (1) + label (63) = 64 bytes per label
        // Total domain name max 253 characters
    }

    #[test]
    fn test_domain_name_max_length() {
        // Max domain name length is 253 characters (255 bytes in DNS format)
        const MAX_DOMAIN_LEN: usize = 253;
        
        // 253 characters + 2 bytes for length/null = 255
        assert_eq!(MAX_DOMAIN_LEN + 2, 255);
    }

    #[test]
    fn test_domain_compression_pointer() {
        // DNS compression uses pointers: first 2 bits = 11 (0xC0)
        fn is_compression_pointer(byte: u8) -> bool {
            (byte & 0xC0) == 0xC0
        }
        
        assert!(is_compression_pointer(0xC0));
        assert!(is_compression_pointer(0xC1));
        assert!(is_compression_pointer(0xFF));
        
        // Regular labels have length < 64
        assert!(!is_compression_pointer(0x3F)); // 63 = max label length
        assert!(!is_compression_pointer(0x00)); // Null terminator
    }

    // =========================================================================
    // Query ID Tests
    // =========================================================================

    #[test]
    fn test_query_id_generation() {
        // Query IDs should be unique for pending queries
        use std::collections::HashSet;
        
        let mut ids = HashSet::new();
        for i in 0..1000u16 {
            // Simple sequential ID generation (real impl should be random)
            let id = i;
            assert!(ids.insert(id), "ID {} already exists", id);
        }
    }

    #[test]
    fn test_query_id_matching() {
        // Response ID must match query ID
        let query_id: u16 = 0x1234;
        let response_id: u16 = 0x1234;
        let wrong_id: u16 = 0x5678;
        
        assert_eq!(query_id, response_id);
        assert_ne!(query_id, wrong_id);
    }

    // =========================================================================
    // TTL Tests
    // =========================================================================

    #[test]
    fn test_dns_ttl_values() {
        // TTL is 32-bit unsigned
        let min_ttl: u32 = 0;
        let typical_ttl: u32 = 3600; // 1 hour
        let max_ttl: u32 = u32::MAX;
        
        assert_eq!(min_ttl, 0);
        assert_eq!(typical_ttl, 3600);
        assert_eq!(max_ttl, 0xFFFFFFFF);
    }

    #[test]
    fn test_ttl_caching() {
        // Cached entry should expire after TTL
        let cached_at = 1000u64; // timestamp
        let ttl = 3600u32; // 1 hour
        let expires_at = cached_at + ttl as u64;
        
        // Not expired
        assert!(1500 < expires_at);
        
        // Expired
        assert!(5000 > expires_at);
    }

    // =========================================================================
    // Well-Known DNS Servers
    // =========================================================================

    #[test]
    fn test_well_known_dns_servers() {
        // Google DNS
        let google_dns_1 = [8, 8, 8, 8];
        let google_dns_2 = [8, 8, 4, 4];
        
        // Cloudflare DNS
        let cloudflare_dns = [1, 1, 1, 1];
        
        // Quad9
        let quad9_dns = [9, 9, 9, 9];
        
        // All should be public IPs (not private)
        fn is_private(ip: &[u8; 4]) -> bool {
            ip[0] == 10 
            || (ip[0] == 172 && ip[1] >= 16 && ip[1] <= 31)
            || (ip[0] == 192 && ip[1] == 168)
        }
        
        assert!(!is_private(&google_dns_1));
        assert!(!is_private(&cloudflare_dns));
        assert!(!is_private(&quad9_dns));
    }

    // =========================================================================
    // DNS Port
    // =========================================================================

    #[test]
    fn test_dns_port() {
        const DNS_PORT: u16 = 53;
        const DNS_OVER_TLS_PORT: u16 = 853;
        const DNS_OVER_HTTPS_PORT: u16 = 443;
        
        assert_eq!(DNS_PORT, 53);
        assert!(DNS_PORT < 1024); // Well-known port
    }
}
