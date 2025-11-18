# NexaOS DNS Support Enhancements

## Overview

This document describes the complete DNS resolution implementation for NexaOS, leveraging UDP socket support to enable real DNS queries. The implementation provides a full-featured DNS resolver compatible with musl libc's `getaddrinfo`/`getnameinfo` interfaces.

## Components Modified

### 1. **nrlib DNS Module** (`userspace/nrlib/src/dns.rs`)
- Complete DNS protocol implementation with query/response parsing
- Support for A, AAAA, MX, NS, TXT, SOA, PTR, CNAME, SRV, ANY record types
- Compression pointer support in DNS names
- Efficient binary packet building and parsing

### 2. **nrlib Resolver Module** (`userspace/nrlib/src/resolver.rs`)

#### DNS Query via UDP
Added `query_dns()` method that:
- Creates a UDP socket using the socket syscall API
- Sends DNS queries to nameservers via `sendto()`
- Receives responses via `recvfrom()`
- Parses A records from responses
- Returns the first IPv4 address found

#### Resolver Implementation
New `resolve()` method that:
- Follows NSS (Name Service Switch) configuration
- First checks `/etc/hosts` for local entries
- Falls back to DNS queries to configured nameservers
- Supports multiple nameservers with retry logic
- Configurable timeout and retry attempts

#### Configuration File Parsing
- Parses `/etc/resolv.conf` for nameservers and search domains
- Parses `/etc/hosts` for local hostname mappings
- Parses `/etc/nsswitch.conf` for NSS source configuration
- Uses bounded buffers (no_std compatible)

#### Initialization
New `resolver_init()` function that:
- Loads configuration files on demand
- Returns immediately if already initialized
- Sets default nameserver (Google DNS: 8.8.8.8) if no config
- Thread-safe with atomic initialization flag

#### Musl-Compatible APIs

**getaddrinfo()**
- Standard POSIX signature matching musl ABI
- Resolves hostname to IPv4 address
- Allocates AddrInfo structure via malloc
- Returns error codes compatible with musl
- Supports flags (basic compatibility)

**getnameinfo()**
- Reverse DNS lookups from sockaddr structures
- Numeric IP conversion (NI_NUMERICHOST flag)
- Service/port number formatting
- Fallback to numeric IP if no reverse entry
- Supports localhost and other special mappings

**freeaddrinfo()**
- Properly deallocates AddrInfo chains
- Frees associated sockaddr and canonname structures
- Uses free() for heap deallocation

### 3. **nslookup Tool** (`userspace/nslookup.rs`)

Complete rewrite to use real DNS queries:

#### Features
- Uses `std::net::UdpSocket` for actual UDP communication
- Supports multiple query types: A, AAAA, MX, NS, TXT, SOA, PTR, CNAME, SRV, ANY
- Reads nameservers from `/etc/resolv.conf`
- Checks `/etc/hosts` for local entries first
- Configurable target nameserver via command line
- Query timeout handling
- Proper DNS response parsing with compression support

#### Usage Examples
```bash
# Simple A record query
nslookup example.com

# Query a specific nameserver
nslookup example.com 8.8.8.8

# Query a specific record type
nslookup -type=MX example.com

# Help
nslookup -h
```

### 4. **Socket API Enhancements** (`userspace/nrlib/src/socket.rs`)

No modifications to socket.rs itself, but the module now provides:
- UDP socket creation via `socket(AF_INET, SOCK_DGRAM, 0)`
- Sending DNS queries via `sendto()`
- Receiving responses via `recvfrom()`
- Helper functions for IPv4 parsing and formatting

### 5. **Libc Compatibility** (`userspace/nrlib/src/libc_compat.rs`)

Added two critical functions for std compatibility:

**setsockopt()**
- Stub implementation for socket option setting
- Specifically handles SO_RCVTIMEO and SO_SNDTIMEO
- Returns success to keep std::net happy
- Actual timeout handling done by kernel

**gai_strerror()**
- Returns error message strings for getaddrinfo errors
- Maps all standard EAI_* error codes
- Human-readable error reporting

## Technical Details

### Memory Management
- Uses stack-allocated buffers where possible (no_std compatible)
- Bounded buffer sizes to prevent overflow:
  - Hostname buffer: 256 bytes
  - DNS query packet: 512 bytes (standard DNS limit)
  - DNS response buffer: 512 bytes
- Heap allocation only for getaddrinfo/getnameinfo results via malloc

### Error Handling
- Returns None/error codes on failure
- Proper cleanup of resources (close file descriptors, free memory)
- Graceful fallback (Google DNS if no config found)
- Error propagation through Result types

### Synchronization
- Static global resolver with atomic initialization flag
- Safe for use in multi-threaded environments
- One-time initialization pattern prevents race conditions

### DNS Protocol Compliance
- Follows RFC 1035 (DNS)
- Supports compression pointers in names
- Proper query/response message structure
- Standard port 53 for DNS
- Default recursion desired (RD flag set)

## Configuration Files

### /etc/resolv.conf
```
# Example resolv.conf
nameserver 8.8.8.8
nameserver 1.1.1.1
search example.com
domain example.com
options timeout:5 attempts:2
```

### /etc/hosts
```
127.0.0.1       localhost
::1             localhost
192.168.1.100   myhost
```

### /etc/nsswitch.conf
```
# Default NSS configuration
hosts: files dns
```

## Testing

### Build
```bash
./scripts/build-all.sh
```

### Run
```bash
./scripts/run-qemu.sh
```

### Test nslookup
```bash
nslookup example.com
nslookup google.com 8.8.8.8
nslookup localhost
```

## Integration with std::net

The DNS support enables:
- `std::net::UdpSocket::bind()` - Works for binding sockets
- `std::net::UdpSocket::send_to()` - Sends UDP packets
- `std::net::UdpSocket::recv_from()` - Receives responses
- `std::net::SocketAddr` parsing - Converts IP:port strings
- Timeout handling via `set_read_timeout()`

This allows userspace Rust programs using `std::net` to perform actual DNS lookups instead of stubbed/mocked operations.

## Known Limitations

1. **IPv6**: Only A records (IPv4) fully supported currently
   - AAAA record parsing exists but no IPv6 socket implementation
   - Can be extended with AF_INET6 support

2. **Advanced DNS**: Some features not yet implemented
   - DNSSEC validation
   - EDNS extensions
   - TCP fallback for large responses
   - Multiple answer parsing (returns first A record only)

3. **Performance**: Basic timeout with no adaptive retry
   - Fixed 5-second timeout hardcoded
   - No exponential backoff
   - Single query attempt to each nameserver

## Future Enhancements

1. Add IPv6 support (AAAA records, AF_INET6 sockets)
2. Implement DNS caching layer
3. Add DNSSEC validation
4. Support TCP fallback for large queries
5. Add mDNS (multicast DNS) support
6. Implement negative caching
7. Add DNS query logging/debugging
8. Performance optimizations (parallel queries, pipelining)

## References

- RFC 1035: Domain Names - Implementation and Specification
- POSIX getaddrinfo/getnameinfo specifications
- musl libc DNS implementation
- Linux socket API documentation
