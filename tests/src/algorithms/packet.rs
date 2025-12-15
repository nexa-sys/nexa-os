//! Network packet parsing and building for testing
//!
//! Simulates network protocol handling without actual network I/O.

/// Ethernet frame header
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    pub dst_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ether_type: u16,
}

impl EthernetHeader {
    pub const SIZE: usize = 14;
    pub const ETHERTYPE_IPV4: u16 = 0x0800;
    pub const ETHERTYPE_ARP: u16 = 0x0806;
    pub const ETHERTYPE_IPV6: u16 = 0x86DD;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        
        let mut dst_mac = [0u8; 6];
        let mut src_mac = [0u8; 6];
        dst_mac.copy_from_slice(&data[0..6]);
        src_mac.copy_from_slice(&data[6..12]);
        let ether_type = u16::from_be_bytes([data[12], data[13]]);
        
        Some(Self {
            dst_mac,
            src_mac,
            ether_type,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..6].copy_from_slice(&self.dst_mac);
        buf[6..12].copy_from_slice(&self.src_mac);
        buf[12..14].copy_from_slice(&self.ether_type.to_be_bytes());
        buf
    }

    /// Check if destination is broadcast
    pub fn is_broadcast(&self) -> bool {
        self.dst_mac == [0xFF; 6]
    }

    /// Check if destination is multicast
    pub fn is_multicast(&self) -> bool {
        self.dst_mac[0] & 0x01 != 0
    }
}

/// IPv4 header
#[derive(Debug, Clone)]
pub struct Ipv4Header {
    pub version: u8,
    pub ihl: u8,
    pub dscp: u8,
    pub ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags: u8,
    pub fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub checksum: u16,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub options: Vec<u8>,
}

impl Ipv4Header {
    pub const MIN_SIZE: usize = 20;
    pub const PROTOCOL_ICMP: u8 = 1;
    pub const PROTOCOL_TCP: u8 = 6;
    pub const PROTOCOL_UDP: u8 = 17;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::MIN_SIZE {
            return None;
        }

        let version = data[0] >> 4;
        let ihl = data[0] & 0x0F;
        let header_len = (ihl as usize) * 4;
        
        if version != 4 || header_len < Self::MIN_SIZE || data.len() < header_len {
            return None;
        }

        let dscp = data[1] >> 2;
        let ecn = data[1] & 0x03;
        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags = data[6] >> 5;
        let fragment_offset = u16::from_be_bytes([data[6] & 0x1F, data[7]]);
        let ttl = data[8];
        let protocol = data[9];
        let checksum = u16::from_be_bytes([data[10], data[11]]);

        let mut src_ip = [0u8; 4];
        let mut dst_ip = [0u8; 4];
        src_ip.copy_from_slice(&data[12..16]);
        dst_ip.copy_from_slice(&data[16..20]);

        let options = if header_len > Self::MIN_SIZE {
            data[Self::MIN_SIZE..header_len].to_vec()
        } else {
            Vec::new()
        };

        Some(Self {
            version,
            ihl,
            dscp,
            ecn,
            total_length,
            identification,
            flags,
            fragment_offset,
            ttl,
            protocol,
            checksum,
            src_ip,
            dst_ip,
            options,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_len = Self::MIN_SIZE + self.options.len();
        let ihl = (header_len / 4) as u8;
        
        let mut buf = Vec::with_capacity(header_len);
        buf.push((4 << 4) | ihl);
        buf.push((self.dscp << 2) | self.ecn);
        buf.extend_from_slice(&self.total_length.to_be_bytes());
        buf.extend_from_slice(&self.identification.to_be_bytes());
        buf.push((self.flags << 5) | ((self.fragment_offset >> 8) as u8 & 0x1F));
        buf.push(self.fragment_offset as u8);
        buf.push(self.ttl);
        buf.push(self.protocol);
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.src_ip);
        buf.extend_from_slice(&self.dst_ip);
        buf.extend_from_slice(&self.options);
        
        buf
    }

    /// Calculate header length in bytes
    pub fn header_len(&self) -> usize {
        (self.ihl as usize) * 4
    }

    /// Get payload offset
    pub fn payload_offset(&self) -> usize {
        self.header_len()
    }

    /// Format IP address as string
    pub fn format_ip(ip: [u8; 4]) -> String {
        format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
    }
}

/// UDP header
#[derive(Debug, Clone, Copy)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

impl UdpHeader {
    pub const SIZE: usize = 8;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dst_port: u16::from_be_bytes([data[2], data[3]]),
            length: u16::from_be_bytes([data[4], data[5]]),
            checksum: u16::from_be_bytes([data[6], data[7]]),
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        buf[4..6].copy_from_slice(&self.length.to_be_bytes());
        buf[6..8].copy_from_slice(&self.checksum.to_be_bytes());
        buf
    }

    /// Get payload length
    pub fn payload_len(&self) -> usize {
        (self.length as usize).saturating_sub(Self::SIZE)
    }
}

/// TCP header
#[derive(Debug, Clone)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8,
    pub flags: TcpFlags,
    pub window: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
    pub options: Vec<u8>,
}

/// TCP flags
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpFlags {
    pub fin: bool,
    pub syn: bool,
    pub rst: bool,
    pub psh: bool,
    pub ack: bool,
    pub urg: bool,
    pub ece: bool,
    pub cwr: bool,
}

impl TcpFlags {
    pub fn from_byte(byte: u8) -> Self {
        Self {
            fin: byte & 0x01 != 0,
            syn: byte & 0x02 != 0,
            rst: byte & 0x04 != 0,
            psh: byte & 0x08 != 0,
            ack: byte & 0x10 != 0,
            urg: byte & 0x20 != 0,
            ece: byte & 0x40 != 0,
            cwr: byte & 0x80 != 0,
        }
    }

    pub fn to_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.fin { byte |= 0x01; }
        if self.syn { byte |= 0x02; }
        if self.rst { byte |= 0x04; }
        if self.psh { byte |= 0x08; }
        if self.ack { byte |= 0x10; }
        if self.urg { byte |= 0x20; }
        if self.ece { byte |= 0x40; }
        if self.cwr { byte |= 0x80; }
        byte
    }
}

impl TcpHeader {
    pub const MIN_SIZE: usize = 20;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::MIN_SIZE {
            return None;
        }

        let data_offset = data[12] >> 4;
        let header_len = (data_offset as usize) * 4;
        
        if header_len < Self::MIN_SIZE || data.len() < header_len {
            return None;
        }

        let flags = TcpFlags::from_byte(data[13]);

        let options = if header_len > Self::MIN_SIZE {
            data[Self::MIN_SIZE..header_len].to_vec()
        } else {
            Vec::new()
        };

        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dst_port: u16::from_be_bytes([data[2], data[3]]),
            seq_num: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            ack_num: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            data_offset,
            flags,
            window: u16::from_be_bytes([data[14], data[15]]),
            checksum: u16::from_be_bytes([data[16], data[17]]),
            urgent_ptr: u16::from_be_bytes([data[18], data[19]]),
            options,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_len = Self::MIN_SIZE + self.options.len();
        let data_offset = ((header_len + 3) / 4) as u8;
        
        let mut buf = Vec::with_capacity(header_len);
        buf.extend_from_slice(&self.src_port.to_be_bytes());
        buf.extend_from_slice(&self.dst_port.to_be_bytes());
        buf.extend_from_slice(&self.seq_num.to_be_bytes());
        buf.extend_from_slice(&self.ack_num.to_be_bytes());
        buf.push(data_offset << 4);
        buf.push(self.flags.to_byte());
        buf.extend_from_slice(&self.window.to_be_bytes());
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.urgent_ptr.to_be_bytes());
        buf.extend_from_slice(&self.options);
        
        // Pad to 4-byte boundary
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        
        buf
    }

    /// Get header length in bytes
    pub fn header_len(&self) -> usize {
        (self.data_offset as usize) * 4
    }
}

/// ARP packet
#[derive(Debug, Clone, Copy)]
pub struct ArpPacket {
    pub hardware_type: u16,
    pub protocol_type: u16,
    pub hw_addr_len: u8,
    pub proto_addr_len: u8,
    pub operation: u16,
    pub sender_hw_addr: [u8; 6],
    pub sender_proto_addr: [u8; 4],
    pub target_hw_addr: [u8; 6],
    pub target_proto_addr: [u8; 4],
}

impl ArpPacket {
    pub const SIZE: usize = 28;
    pub const OP_REQUEST: u16 = 1;
    pub const OP_REPLY: u16 = 2;
    pub const HW_ETHERNET: u16 = 1;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        let mut sender_hw_addr = [0u8; 6];
        let mut sender_proto_addr = [0u8; 4];
        let mut target_hw_addr = [0u8; 6];
        let mut target_proto_addr = [0u8; 4];

        sender_hw_addr.copy_from_slice(&data[8..14]);
        sender_proto_addr.copy_from_slice(&data[14..18]);
        target_hw_addr.copy_from_slice(&data[18..24]);
        target_proto_addr.copy_from_slice(&data[24..28]);

        Some(Self {
            hardware_type: u16::from_be_bytes([data[0], data[1]]),
            protocol_type: u16::from_be_bytes([data[2], data[3]]),
            hw_addr_len: data[4],
            proto_addr_len: data[5],
            operation: u16::from_be_bytes([data[6], data[7]]),
            sender_hw_addr,
            sender_proto_addr,
            target_hw_addr,
            target_proto_addr,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..2].copy_from_slice(&self.hardware_type.to_be_bytes());
        buf[2..4].copy_from_slice(&self.protocol_type.to_be_bytes());
        buf[4] = self.hw_addr_len;
        buf[5] = self.proto_addr_len;
        buf[6..8].copy_from_slice(&self.operation.to_be_bytes());
        buf[8..14].copy_from_slice(&self.sender_hw_addr);
        buf[14..18].copy_from_slice(&self.sender_proto_addr);
        buf[18..24].copy_from_slice(&self.target_hw_addr);
        buf[24..28].copy_from_slice(&self.target_proto_addr);
        buf
    }

    /// Create an ARP request
    pub fn new_request(
        sender_mac: [u8; 6],
        sender_ip: [u8; 4],
        target_ip: [u8; 4],
    ) -> Self {
        Self {
            hardware_type: Self::HW_ETHERNET,
            protocol_type: EthernetHeader::ETHERTYPE_IPV4,
            hw_addr_len: 6,
            proto_addr_len: 4,
            operation: Self::OP_REQUEST,
            sender_hw_addr: sender_mac,
            sender_proto_addr: sender_ip,
            target_hw_addr: [0; 6],
            target_proto_addr: target_ip,
        }
    }

    /// Create an ARP reply
    pub fn new_reply(
        sender_mac: [u8; 6],
        sender_ip: [u8; 4],
        target_mac: [u8; 6],
        target_ip: [u8; 4],
    ) -> Self {
        Self {
            hardware_type: Self::HW_ETHERNET,
            protocol_type: EthernetHeader::ETHERTYPE_IPV4,
            hw_addr_len: 6,
            proto_addr_len: 4,
            operation: Self::OP_REPLY,
            sender_hw_addr: sender_mac,
            sender_proto_addr: sender_ip,
            target_hw_addr: target_mac,
            target_proto_addr: target_ip,
        }
    }

    /// Check if this is a request
    pub fn is_request(&self) -> bool {
        self.operation == Self::OP_REQUEST
    }

    /// Check if this is a reply
    pub fn is_reply(&self) -> bool {
        self.operation == Self::OP_REPLY
    }
}

/// ICMP header
#[derive(Debug, Clone, Copy)]
pub struct IcmpHeader {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub data: u32,
}

impl IcmpHeader {
    pub const SIZE: usize = 8;
    pub const TYPE_ECHO_REPLY: u8 = 0;
    pub const TYPE_ECHO_REQUEST: u8 = 8;
    pub const TYPE_DEST_UNREACHABLE: u8 = 3;
    pub const TYPE_TIME_EXCEEDED: u8 = 11;

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        Some(Self {
            icmp_type: data[0],
            code: data[1],
            checksum: u16::from_be_bytes([data[2], data[3]]),
            data: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0] = self.icmp_type;
        buf[1] = self.code;
        buf[2..4].copy_from_slice(&self.checksum.to_be_bytes());
        buf[4..8].copy_from_slice(&self.data.to_be_bytes());
        buf
    }

    /// Get identifier (for echo request/reply)
    pub fn identifier(&self) -> u16 {
        (self.data >> 16) as u16
    }

    /// Get sequence number (for echo request/reply)
    pub fn sequence(&self) -> u16 {
        self.data as u16
    }

    /// Create echo request
    pub fn new_echo_request(identifier: u16, sequence: u16) -> Self {
        Self {
            icmp_type: Self::TYPE_ECHO_REQUEST,
            code: 0,
            checksum: 0,
            data: ((identifier as u32) << 16) | (sequence as u32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethernet_header_parse() {
        let data = [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // dst MAC (broadcast)
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // src MAC
            0x08, 0x00, // EtherType (IPv4)
        ];

        let header = EthernetHeader::from_bytes(&data).unwrap();
        assert!(header.is_broadcast());
        assert_eq!(header.ether_type, EthernetHeader::ETHERTYPE_IPV4);
        assert_eq!(header.src_mac, [0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    }

    #[test]
    fn test_ethernet_header_roundtrip() {
        let header = EthernetHeader {
            dst_mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            src_mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            ether_type: EthernetHeader::ETHERTYPE_ARP,
        };

        let bytes = header.to_bytes();
        let parsed = EthernetHeader::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.dst_mac, header.dst_mac);
        assert_eq!(parsed.src_mac, header.src_mac);
        assert_eq!(parsed.ether_type, header.ether_type);
    }

    #[test]
    fn test_ipv4_header_parse() {
        let data = [
            0x45, 0x00, // Version/IHL, DSCP/ECN
            0x00, 0x3c, // Total length
            0x1c, 0x46, // Identification
            0x40, 0x00, // Flags, Fragment offset
            0x40, 0x06, // TTL, Protocol (TCP)
            0xb1, 0xe6, // Checksum
            0xc0, 0xa8, 0x00, 0x01, // Source IP
            0xc0, 0xa8, 0x00, 0x02, // Dest IP
        ];

        let header = Ipv4Header::from_bytes(&data).unwrap();
        assert_eq!(header.version, 4);
        assert_eq!(header.ihl, 5);
        assert_eq!(header.ttl, 64);
        assert_eq!(header.protocol, Ipv4Header::PROTOCOL_TCP);
        assert_eq!(header.src_ip, [192, 168, 0, 1]);
        assert_eq!(header.dst_ip, [192, 168, 0, 2]);
    }

    #[test]
    fn test_ipv4_header_roundtrip() {
        let header = Ipv4Header {
            version: 4,
            ihl: 5,
            dscp: 0,
            ecn: 0,
            total_length: 60,
            identification: 0x1234,
            flags: 0x02,
            fragment_offset: 0,
            ttl: 64,
            protocol: Ipv4Header::PROTOCOL_UDP,
            checksum: 0,
            src_ip: [10, 0, 0, 1],
            dst_ip: [10, 0, 0, 2],
            options: Vec::new(),
        };

        let bytes = header.to_bytes();
        let parsed = Ipv4Header::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.version, 4);
        assert_eq!(parsed.protocol, Ipv4Header::PROTOCOL_UDP);
        assert_eq!(parsed.src_ip, header.src_ip);
        assert_eq!(parsed.dst_ip, header.dst_ip);
    }

    #[test]
    fn test_ipv4_format_ip() {
        assert_eq!(Ipv4Header::format_ip([192, 168, 1, 1]), "192.168.1.1");
        assert_eq!(Ipv4Header::format_ip([10, 0, 0, 1]), "10.0.0.1");
        assert_eq!(Ipv4Header::format_ip([255, 255, 255, 255]), "255.255.255.255");
    }

    #[test]
    fn test_udp_header_parse() {
        let data = [
            0x00, 0x50, // Source port (80)
            0x01, 0xbb, // Dest port (443)
            0x00, 0x10, // Length
            0x00, 0x00, // Checksum
        ];

        let header = UdpHeader::from_bytes(&data).unwrap();
        assert_eq!(header.src_port, 80);
        assert_eq!(header.dst_port, 443);
        assert_eq!(header.length, 16);
        assert_eq!(header.payload_len(), 8);
    }

    #[test]
    fn test_udp_header_roundtrip() {
        let header = UdpHeader {
            src_port: 12345,
            dst_port: 53,
            length: 20,
            checksum: 0xABCD,
        };

        let bytes = header.to_bytes();
        let parsed = UdpHeader::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.src_port, header.src_port);
        assert_eq!(parsed.dst_port, header.dst_port);
        assert_eq!(parsed.length, header.length);
        assert_eq!(parsed.checksum, header.checksum);
    }

    #[test]
    fn test_tcp_flags() {
        let flags = TcpFlags {
            syn: true,
            ack: true,
            ..Default::default()
        };
        
        let byte = flags.to_byte();
        assert_eq!(byte, 0x12); // SYN | ACK
        
        let parsed = TcpFlags::from_byte(byte);
        assert!(parsed.syn);
        assert!(parsed.ack);
        assert!(!parsed.fin);
        assert!(!parsed.rst);
    }

    #[test]
    fn test_tcp_header_parse() {
        let data = [
            0x00, 0x50, // Source port (80)
            0x01, 0xbb, // Dest port (443)
            0x00, 0x00, 0x00, 0x01, // Sequence number
            0x00, 0x00, 0x00, 0x00, // Ack number
            0x50, 0x02, // Data offset, flags (SYN)
            0xff, 0xff, // Window
            0x00, 0x00, // Checksum
            0x00, 0x00, // Urgent pointer
        ];

        let header = TcpHeader::from_bytes(&data).unwrap();
        assert_eq!(header.src_port, 80);
        assert_eq!(header.dst_port, 443);
        assert_eq!(header.seq_num, 1);
        assert!(header.flags.syn);
        assert!(!header.flags.ack);
    }

    #[test]
    fn test_arp_request() {
        let sender_mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let sender_ip = [192, 168, 1, 100];
        let target_ip = [192, 168, 1, 1];

        let arp = ArpPacket::new_request(sender_mac, sender_ip, target_ip);
        
        assert!(arp.is_request());
        assert!(!arp.is_reply());
        assert_eq!(arp.sender_hw_addr, sender_mac);
        assert_eq!(arp.sender_proto_addr, sender_ip);
        assert_eq!(arp.target_proto_addr, target_ip);
        assert_eq!(arp.target_hw_addr, [0; 6]); // Unknown in request
    }

    #[test]
    fn test_arp_reply() {
        let sender_mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let sender_ip = [192, 168, 1, 1];
        let target_mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let target_ip = [192, 168, 1, 100];

        let arp = ArpPacket::new_reply(sender_mac, sender_ip, target_mac, target_ip);
        
        assert!(!arp.is_request());
        assert!(arp.is_reply());
    }

    #[test]
    fn test_arp_roundtrip() {
        let arp = ArpPacket::new_request(
            [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            [192, 168, 1, 100],
            [192, 168, 1, 1],
        );

        let bytes = arp.to_bytes();
        let parsed = ArpPacket::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.operation, arp.operation);
        assert_eq!(parsed.sender_hw_addr, arp.sender_hw_addr);
        assert_eq!(parsed.sender_proto_addr, arp.sender_proto_addr);
        assert_eq!(parsed.target_proto_addr, arp.target_proto_addr);
    }

    #[test]
    fn test_icmp_echo_request() {
        let icmp = IcmpHeader::new_echo_request(0x1234, 1);
        
        assert_eq!(icmp.icmp_type, IcmpHeader::TYPE_ECHO_REQUEST);
        assert_eq!(icmp.identifier(), 0x1234);
        assert_eq!(icmp.sequence(), 1);
    }

    #[test]
    fn test_icmp_roundtrip() {
        let icmp = IcmpHeader::new_echo_request(0xABCD, 42);
        
        let bytes = icmp.to_bytes();
        let parsed = IcmpHeader::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.icmp_type, IcmpHeader::TYPE_ECHO_REQUEST);
        assert_eq!(parsed.identifier(), 0xABCD);
        assert_eq!(parsed.sequence(), 42);
    }

    #[test]
    fn test_multicast_detection() {
        let unicast = EthernetHeader {
            dst_mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            src_mac: [0; 6],
            ether_type: 0,
        };
        assert!(!unicast.is_multicast());

        let multicast = EthernetHeader {
            dst_mac: [0x01, 0x00, 0x5e, 0x00, 0x00, 0x01],
            src_mac: [0; 6],
            ether_type: 0,
        };
        assert!(multicast.is_multicast());
    }

    #[test]
    fn test_header_too_short() {
        assert!(EthernetHeader::from_bytes(&[0; 10]).is_none());
        assert!(Ipv4Header::from_bytes(&[0; 10]).is_none());
        assert!(UdpHeader::from_bytes(&[0; 4]).is_none());
        assert!(TcpHeader::from_bytes(&[0; 10]).is_none());
        assert!(ArpPacket::from_bytes(&[0; 20]).is_none());
        assert!(IcmpHeader::from_bytes(&[0; 4]).is_none());
    }

    #[test]
    fn test_ipv4_invalid_version() {
        let data = [
            0x65, 0x00, // Version 6, IHL 5 (invalid)
            0x00, 0x3c,
            0x00, 0x00, 0x00, 0x00,
            0x40, 0x06, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        assert!(Ipv4Header::from_bytes(&data).is_none());
    }
}
