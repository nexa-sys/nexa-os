/// DNS resolver implementation compatible with musl libc
///
/// This module provides DNS query construction, parsing, and resolution
/// services compatible with POSIX getaddrinfo/getnameinfo APIs.


/// DNS header structure (12 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DnsHeader {
    pub id: u16,           // Transaction ID
    pub flags: u16,        // Flags (QR, Opcode, AA, TC, RD, RA, Z, RCODE)
    pub qdcount: u16,      // Number of questions
    pub ancount: u16,      // Number of answers
    pub nscount: u16,      // Number of authority RRs
    pub arcount: u16,      // Number of additional RRs
}

impl DnsHeader {
    pub const SIZE: usize = 12;

    /// Create a new DNS query header
    pub fn new_query(id: u16, recursion_desired: bool) -> Self {
        let mut flags = 0u16;
        if recursion_desired {
            flags |= 0x0100; // RD bit
        }
        Self {
            id: id.to_be(),
            flags: flags.to_be(),
            qdcount: 1u16.to_be(),
            ancount: 0u16.to_be(),
            nscount: 0u16.to_be(),
            arcount: 0u16.to_be(),
        }
    }

    /// Get transaction ID
    pub fn transaction_id(&self) -> u16 {
        u16::from_be(self.id)
    }

    /// Check if this is a response
    pub fn is_response(&self) -> bool {
        (u16::from_be(self.flags) & 0x8000) != 0
    }

    /// Get response code
    pub fn rcode(&self) -> u8 {
        (u16::from_be(self.flags) & 0x000F) as u8
    }

    /// Get answer count
    pub fn answer_count(&self) -> u16 {
        u16::from_be(self.ancount)
    }

    /// Get question count
    pub fn question_count(&self) -> u16 {
        u16::from_be(self.qdcount)
    }
}

/// DNS question types
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QType {
    A = 1,      // IPv4 address
    NS = 2,     // Name server
    CNAME = 5,  // Canonical name
    SOA = 6,    // Start of authority
    PTR = 12,   // Pointer record
    MX = 15,    // Mail exchange
    TXT = 16,   // Text record
    AAAA = 28,  // IPv6 address
    SRV = 33,   // Service record
    ANY = 255,  // Any record
}

impl From<u16> for QType {
    fn from(value: u16) -> Self {
        match value {
            1 => QType::A,
            2 => QType::NS,
            5 => QType::CNAME,
            6 => QType::SOA,
            12 => QType::PTR,
            15 => QType::MX,
            16 => QType::TXT,
            28 => QType::AAAA,
            33 => QType::SRV,
            255 => QType::ANY,
            _ => QType::A,
        }
    }
}

/// DNS class
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QClass {
    IN = 1,   // Internet
    CS = 2,   // CSNET (obsolete)
    CH = 3,   // CHAOS
    HS = 4,   // Hesiod
}

/// DNS query builder
pub struct DnsQuery {
    buffer: [u8; 512],
    length: usize,
}

impl DnsQuery {
    pub fn new() -> Self {
        Self {
            buffer: [0u8; 512],
            length: 0,
        }
    }

    /// Build a DNS query packet
    pub fn build(&mut self, id: u16, domain: &str, qtype: QType) -> Result<&[u8], &'static str> {
        if domain.len() > 253 {
            return Err("Domain name too long");
        }

        // DNS header
        let header = DnsHeader::new_query(id, true);
        unsafe {
            let header_bytes = core::slice::from_raw_parts(
                &header as *const DnsHeader as *const u8,
                DnsHeader::SIZE,
            );
            self.buffer[..DnsHeader::SIZE].copy_from_slice(header_bytes);
        }
        self.length = DnsHeader::SIZE;

        // Encode domain name
        self.encode_domain(domain)?;

        // Question type and class
        let qtype_val = (qtype as u16).to_be_bytes();
        let qclass_val = (QClass::IN as u16).to_be_bytes();
        self.buffer[self.length..self.length + 2].copy_from_slice(&qtype_val);
        self.length += 2;
        self.buffer[self.length..self.length + 2].copy_from_slice(&qclass_val);
        self.length += 2;

        Ok(&self.buffer[..self.length])
    }

    /// Encode domain name in DNS format (length-prefixed labels)
    fn encode_domain(&mut self, domain: &str) -> Result<(), &'static str> {
        for label in domain.split('.') {
            if label.is_empty() {
                continue;
            }
            if label.len() > 63 {
                return Err("Label too long");
            }
            if self.length + 1 + label.len() > 512 {
                return Err("Buffer overflow");
            }

            self.buffer[self.length] = label.len() as u8;
            self.length += 1;
            self.buffer[self.length..self.length + label.len()]
                .copy_from_slice(label.as_bytes());
            self.length += label.len();
        }

        // Null terminator
        if self.length >= 512 {
            return Err("Buffer overflow");
        }
        self.buffer[self.length] = 0;
        self.length += 1;

        Ok(())
    }
}

/// DNS response parser
#[allow(dead_code)]
pub struct DnsResponse<'a> {
    data: &'a [u8],
    offset: usize,
}

#[allow(dead_code)]
impl<'a> DnsResponse<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    /// Parse DNS header
    pub fn parse_header(&mut self) -> Result<DnsHeader, &'static str> {
        if self.data.len() < DnsHeader::SIZE {
            return Err("Response too short");
        }

        let header = unsafe {
            core::ptr::read_unaligned(self.data.as_ptr() as *const DnsHeader)
        };

        self.offset = DnsHeader::SIZE;
        Ok(header)
    }

    /// Skip DNS question section
    pub fn skip_question(&mut self) -> Result<(), &'static str> {
        // Skip domain name
        self.skip_name()?;

        // Skip qtype and qclass (4 bytes)
        if self.offset + 4 > self.data.len() {
            return Err("Unexpected end of data");
        }
        self.offset += 4;

        Ok(())
    }

    /// Parse DNS answer and extract IPv4 address
    pub fn parse_a_record(&mut self) -> Result<Option<[u8; 4]>, &'static str> {
        // Skip name
        self.skip_name()?;

        // Read type, class, ttl, rdlength
        if self.offset + 10 > self.data.len() {
            return Err("Unexpected end of data");
        }

        let rtype = u16::from_be_bytes([
            self.data[self.offset],
            self.data[self.offset + 1],
        ]);
        let rdlength = u16::from_be_bytes([
            self.data[self.offset + 8],
            self.data[self.offset + 9],
        ]) as usize;

        self.offset += 10;

        if rtype != 1 {
            // Not an A record, skip rdata
            if self.offset + rdlength > self.data.len() {
                return Err("Invalid rdlength");
            }
            self.offset += rdlength;
            return Ok(None);
        }

        // Read IPv4 address
        if rdlength != 4 {
            return Err("Invalid A record length");
        }
        if self.offset + 4 > self.data.len() {
            return Err("Unexpected end of data");
        }

        let mut ip = [0u8; 4];
        ip.copy_from_slice(&self.data[self.offset..self.offset + 4]);
        self.offset += 4;

        Ok(Some(ip))
    }

    /// Skip a DNS name (with compression support)
    fn skip_name(&mut self) -> Result<(), &'static str> {
        loop {
            if self.offset >= self.data.len() {
                return Err("Unexpected end of data");
            }

            let len = self.data[self.offset];

            // Check for compression pointer
            if len & 0xC0 == 0xC0 {
                if self.offset + 1 >= self.data.len() {
                    return Err("Invalid compression pointer");
                }
                self.offset += 2;
                return Ok(());
            }

            // Regular label
            if len == 0 {
                self.offset += 1;
                return Ok(());
            }

            self.offset += 1 + len as usize;
        }
    }

    /// Read full domain name (with compression)
    pub fn read_name(&mut self, buffer: &mut [u8]) -> Result<usize, &'static str> {
        let mut buf_pos = 0;
        let mut pos = self.offset;
        let mut jumped = false;
        let mut first_label = true;

        loop {
            if pos >= self.data.len() {
                return Err("Unexpected end of data");
            }

            let len = self.data[pos];

            // Compression pointer
            if len & 0xC0 == 0xC0 {
                if pos + 1 >= self.data.len() {
                    return Err("Invalid compression pointer");
                }
                if !jumped {
                    self.offset = pos + 2;
                }
                let offset = (((len & 0x3F) as usize) << 8) | (self.data[pos + 1] as usize);
                pos = offset;
                jumped = true;
                continue;
            }

            // End of name
            if len == 0 {
                if !jumped {
                    self.offset = pos + 1;
                }
                buffer[buf_pos] = 0;
                return Ok(buf_pos);
            }

            // Label
            if !first_label {
                if buf_pos >= buffer.len() {
                    return Err("Buffer too small");
                }
                buffer[buf_pos] = b'.';
                buf_pos += 1;
            }
            first_label = false;

            let label_len = len as usize;
            if pos + 1 + label_len > self.data.len() {
                return Err("Invalid label length");
            }
            if buf_pos + label_len > buffer.len() {
                return Err("Buffer too small");
            }

            buffer[buf_pos..buf_pos + label_len]
                .copy_from_slice(&self.data[pos + 1..pos + 1 + label_len]);
            buf_pos += label_len;
            pos += 1 + label_len;
        }
    }
}

/// Resolver state
pub struct ResolverConfig {
    pub nameservers: [[u8; 4]; 3],
    pub nameserver_count: usize,
    pub search_domains: [[u8; 64]; 6],
    pub search_domain_count: usize,
    pub timeout_ms: u32,
    pub attempts: u8,
}

impl ResolverConfig {
    pub const fn new() -> Self {
        Self {
            nameservers: [[0; 4]; 3],
            nameserver_count: 0,
            search_domains: [[0; 64]; 6],
            search_domain_count: 0,
            timeout_ms: 5000,
            attempts: 2,
        }
    }

    /// Add a nameserver
    pub fn add_nameserver(&mut self, ip: [u8; 4]) -> Result<(), &'static str> {
        if self.nameserver_count >= 3 {
            return Err("Too many nameservers");
        }
        self.nameservers[self.nameserver_count] = ip;
        self.nameserver_count += 1;
        Ok(())
    }

    /// Add a search domain
    pub fn add_search_domain(&mut self, domain: &str) -> Result<(), &'static str> {
        if self.search_domain_count >= 6 {
            return Err("Too many search domains");
        }
        if domain.len() >= 64 {
            return Err("Domain too long");
        }

        let mut buf = [0u8; 64];
        buf[..domain.len()].copy_from_slice(domain.as_bytes());
        self.search_domains[self.search_domain_count] = buf;
        self.search_domain_count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_query() {
        let mut query = DnsQuery::new();
        let packet = query.build(0x1234, "example.com", QType::A).unwrap();

        // Check header
        assert!(packet.len() > DnsHeader::SIZE);
        assert_eq!(packet[0], 0x12);
        assert_eq!(packet[1], 0x34);

        // Check question count
        assert_eq!(packet[4], 0x00);
        assert_eq!(packet[5], 0x01);
    }

    #[test]
    fn test_domain_encoding() {
        let mut query = DnsQuery::new();
        query.encode_domain("example.com").unwrap();

        // Check encoded format: 7example3com0
        assert_eq!(query.buffer[0], 7); // "example"
        assert_eq!(&query.buffer[1..8], b"example");
        assert_eq!(query.buffer[8], 3); // "com"
        assert_eq!(&query.buffer[9..12], b"com");
        assert_eq!(query.buffer[12], 0); // null terminator
    }
}
