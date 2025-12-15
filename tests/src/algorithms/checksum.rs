//! Checksum algorithms
//!
//! Used for network protocols (IP, TCP, UDP checksums)

/// Calculate the Internet checksum (RFC 1071)
/// Used for IP, ICMP, TCP, UDP headers
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;

    // Sum up 16-bit words
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }

    // Handle odd byte
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }

    // Fold 32-bit sum to 16 bits
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // One's complement
    !(sum as u16)
}

/// Verify an Internet checksum (result should be 0xFFFF if valid)
pub fn verify_internet_checksum(data: &[u8]) -> bool {
    let mut sum: u32 = 0;
    let mut i = 0;

    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }

    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    sum as u16 == 0xFFFF
}

/// Calculate a pseudo-header checksum for TCP/UDP
pub fn pseudo_header_checksum(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    protocol: u8,
    length: u16,
) -> u32 {
    let mut sum: u32 = 0;
    
    // Source IP
    sum = sum.wrapping_add(u16::from_be_bytes([src_ip[0], src_ip[1]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([src_ip[2], src_ip[3]]) as u32);
    
    // Destination IP
    sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[0], dst_ip[1]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[2], dst_ip[3]]) as u32);
    
    // Protocol (zero-padded to 16 bits)
    sum = sum.wrapping_add(protocol as u32);
    
    // Length
    sum = sum.wrapping_add(length as u32);
    
    sum
}

/// Finalize a partial checksum
pub fn finalize_checksum(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internet_checksum_simple() {
        // Test with a simple known value
        let data = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let checksum = internet_checksum(&data);
        
        // Verify by adding checksum back
        let mut verify_data = data.to_vec();
        verify_data.extend_from_slice(&checksum.to_be_bytes());
        assert!(verify_internet_checksum(&verify_data));
    }

    #[test]
    fn test_internet_checksum_odd_length() {
        let data = [0x45, 0x00, 0x00, 0x73, 0x00];
        let checksum = internet_checksum(&data);
        
        // Should handle odd-length data correctly
        assert_ne!(checksum, 0);
    }

    #[test]
    fn test_internet_checksum_empty() {
        let data: [u8; 0] = [];
        let checksum = internet_checksum(&data);
        assert_eq!(checksum, 0xFFFF);
    }

    #[test]
    fn test_internet_checksum_all_zeros() {
        let data = [0u8; 20];
        let checksum = internet_checksum(&data);
        assert_eq!(checksum, 0xFFFF);
    }

    #[test]
    fn test_pseudo_header_checksum() {
        let src = [192, 168, 1, 1];
        let dst = [192, 168, 1, 2];
        let protocol = 17; // UDP
        let length = 20;
        
        let sum = pseudo_header_checksum(src, dst, protocol, length);
        assert!(sum > 0);
    }

    #[test]
    fn test_ip_header_checksum() {
        // Example IPv4 header (20 bytes) with checksum field zeroed
        let mut header: [u8; 20] = [
            0x45, 0x00, 0x00, 0x73, // Version, IHL, TOS, Total Length
            0x00, 0x00, 0x40, 0x00, // ID, Flags, Fragment Offset
            0x40, 0x11, 0x00, 0x00, // TTL, Protocol (UDP=17), Checksum (zeroed)
            0xc0, 0xa8, 0x00, 0x01, // Source IP: 192.168.0.1
            0xc0, 0xa8, 0x00, 0xc7, // Dest IP: 192.168.0.199
        ];
        
        let checksum = internet_checksum(&header);
        header[10] = (checksum >> 8) as u8;
        header[11] = (checksum & 0xFF) as u8;
        
        // Verify the complete header
        assert!(verify_internet_checksum(&header));
    }
}
