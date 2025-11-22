# nslookup DNS Query Tool - Implementation Summary

## Overview
The `nslookup` tool has been enhanced to perform real DNS queries using both UDP and TCP protocols, providing full DNS resolution capabilities on NexaOS.

## Features

### Query Types Supported
- **A** - IPv4 address records (default)
- **NS** - Name server records
- **CNAME** - Canonical name records
- **SOA** - Start of authority records
- **PTR** - Pointer records (reverse DNS)
- **MX** - Mail exchange records
- **TXT** - Text records
- **AAAA** - IPv6 address records
- **SRV** - Service records
- **ANY** - All available records

### Transport Protocols
- **UDP** (default) - Standard DNS over UDP port 53
- **TCP** - DNS over TCP port 53 (use `-tcp` flag)
  - Automatically handles TCP length prefix (2-byte header)
  - Useful for large responses or when UDP fails
  - Supports connection timeout (5 seconds)

### Resolution Sources
1. **/etc/hosts** - Checked first for A records
2. **DNS servers** - From /etc/resolv.conf or specified on command line
3. **Fallback** - Google DNS (8.8.8.8) if no nameserver configured

## Usage

### Basic Usage
```bash
# Query A record using /etc/resolv.conf nameserver
nslookup example.com

# Query using specific nameserver
nslookup example.com 8.8.8.8
```

### Query Types
```bash
# Query MX records
nslookup -type=MX example.com

# Query NS records
nslookup -type=NS example.com

# Query IPv6 address
nslookup -type=AAAA ipv6.google.com

# Query all available records
nslookup -type=ANY example.com
```

### TCP Transport
```bash
# Use TCP instead of UDP
nslookup -tcp example.com

# Combine with query type
nslookup -type=TXT -tcp example.com 8.8.8.8
```

### Help
```bash
nslookup -h
nslookup --help
```

## Implementation Details

### DNS Packet Construction
- Transaction ID: Fixed at 0x1234
- Flags: Recursion desired (RD=1)
- QCLASS: IN (Internet) = 1
- Domain name encoding: Length-prefixed labels (DNS format)
- Example: "example.com" → `\x07example\x03com\x00`

### DNS Response Parsing
- Validates response flags and RCODE
- Handles DNS name compression (pointer format 0xC0)
- Extracts various record types:
  - **A**: 4-byte IPv4 address
  - **AAAA**: 16-byte IPv6 address
  - **CNAME/NS/PTR**: Domain names (with compression support)
  - **MX**: Preference value + exchange domain
  - **TXT**: Length-prefixed text strings

### Error Handling
- DNS RCODE errors:
  - 1: Format error
  - 2: Server failure
  - 3: Name error (NXDOMAIN)
  - 4: Not implemented
  - 5: Refused
- Network timeouts (5 seconds)
- Invalid responses (malformed packets)
- Connection failures

### Socket Operations
Uses standard library networking:
- `std::net::UdpSocket` - For UDP queries
- `std::net::TcpStream` - For TCP queries
- `std::time::Duration` - For timeouts

Backed by nrlib system calls:
- `socket()` - Create socket
- `sendto()` - Send UDP datagram
- `recvfrom()` - Receive UDP datagram
- `send()` - Send TCP data
- `recv()` - Receive TCP data
- `connect()` - Connect TCP stream

## Configuration Files

### /etc/resolv.conf
```
nameserver 8.8.8.8
nameserver 8.8.4.4
```

### /etc/hosts
```
127.0.0.1   localhost
192.168.1.1 router
```

## Output Format

### Standard Output (mimics Linux nslookup)
```
Server:         8.8.8.8
Address:        8.8.8.8#53

Non-authoritative answer:
Name:   example.com
Address: 93.184.216.34
```

### Error Output
```
Server:         8.8.8.8
Address:        8.8.8.8#53

** server can't find example.invalid: Name error (NXDOMAIN) (RCODE=3)
```

## Dependencies
- **nrlib**: Provides POSIX socket API implementation
- **std**: Uses standard library for networking, I/O, and collections

## Technical Notes

### Why Both UDP and TCP?
- **UDP** (default): Fast, lightweight, most DNS queries fit in 512 bytes
- **TCP**: Required for responses > 512 bytes, reliable for critical queries
- DNS RFC 1035 specifies both must be supported for full compliance

### DNS Name Compression
DNS uses pointer compression to reduce packet size:
- When length byte has top 2 bits set (0xC0), it's a pointer
- Points to earlier occurrence of the same domain name
- Reduces redundancy in responses with multiple records

### Memory Safety
- All buffers use fixed sizes (512 bytes for UDP, dynamic for TCP)
- Bounds checking on all array accesses
- No heap allocation in DNS packet construction
- Safe parsing with offset validation

## Future Enhancements
- [ ] DNSSEC validation
- [ ] DNS-over-TLS (DoT) support
- [ ] DNS-over-HTTPS (DoH) support
- [ ] Interactive mode (like traditional nslookup)
- [ ] Batch query support
- [ ] Response caching
- [ ] IPv6 transport support
- [ ] Custom timeout and retry options

## Testing
Test DNS resolution:
```bash
# Basic A record
nslookup google.com

# Try TCP
nslookup -tcp google.com

# Query different record types
nslookup -type=MX gmail.com
nslookup -type=NS google.com
nslookup -type=TXT google.com

# Use different nameservers
nslookup google.com 1.1.1.1  # Cloudflare DNS
nslookup google.com 8.8.8.8  # Google DNS
```

## References
- RFC 1035: Domain Names - Implementation and Specification
- RFC 7766: DNS Transport over TCP - Implementation Requirements
- POSIX Socket API
