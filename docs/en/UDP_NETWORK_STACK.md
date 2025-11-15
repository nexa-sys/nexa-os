# UDP Network Stack Implementation

## Overview

NexaOS now has a complete UDP/IP network stack built on top of the UEFI network device infrastructure. The stack provides:

- **Ethernet II frame handling** (src/net/ethernet.rs)
- **IPv4 packet processing** (src/net/ipv4.rs)
- **ARP protocol** for MAC address resolution (src/net/arp.rs)
- **UDP datagram** transmission and reception (src/net/udp.rs)
- **Network stack coordinator** with socket management (src/net/stack.rs)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     User Space                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │ UDP App 1    │  │ UDP App 2    │  │ TCP App      │     │
│  │ socket()     │  │ socket()     │  │ connect()    │     │
│  │ bind()       │  │ bind()       │  │ send()       │     │
│  │ sendto()     │  │ recvfrom()   │  │ recv()       │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
│         │                 │                  │             │
└─────────┼─────────────────┼──────────────────┼─────────────┘
          │                 │                  │
          └─────────────────┴──────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                     Kernel Space                            │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐ │
│  │              Network Stack (src/net/stack.rs)         │ │
│  │                                                       │ │
│  │  ┌─────────────────┐  ┌─────────────────┐           │ │
│  │  │ UDP Sockets[16] │  │ TCP Endpoint    │           │ │
│  │  │ - local_port    │  │ - state         │           │ │
│  │  │ - remote_ip     │  │ - connection    │           │ │
│  │  │ - remote_port   │  │                 │           │ │
│  │  └─────────────────┘  └─────────────────┘           │ │
│  │                                                       │ │
│  │  ┌─────────────────────────────────────────────────┐ │ │
│  │  │        ARP Cache (32 entries)                   │ │ │
│  │  │        IP → MAC mapping with timestamps         │ │ │
│  │  └─────────────────────────────────────────────────┘ │ │
│  └───────────────────────────────────────────────────────┘ │
│                            │                               │
│                            ▼                               │
│  ┌───────────────────────────────────────────────────────┐ │
│  │          Protocol Handlers                            │ │
│  │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐     │ │
│  │  │  ARP   │  │ IPv4   │  │ ICMP   │  │  UDP   │     │ │
│  │  │ .rs    │  │ .rs    │  │ echo   │  │ .rs    │     │ │
│  │  └────────┘  └────────┘  └────────┘  └────────┘     │ │
│  └───────────────────────────────────────────────────────┘ │
│                            │                               │
│                            ▼                               │
│  ┌───────────────────────────────────────────────────────┐ │
│  │     Ethernet Frame Processing (ethernet.rs)           │ │
│  │     - Frame parsing/construction                      │ │
│  │     - MAC address handling                            │ │
│  │     - EtherType demultiplexing                        │ │
│  └───────────────────────────────────────────────────────┘ │
│                            │                               │
│                            ▼                               │
│  ┌───────────────────────────────────────────────────────┐ │
│  │        NIC Drivers (drivers/e1000.rs, etc.)           │ │
│  │        - TX/RX ring management                        │ │
│  │        - DMA buffer handling                          │ │
│  │        - Interrupt processing                         │ │
│  └───────────────────────────────────────────────────────┘ │
│                            │                               │
└────────────────────────────┼───────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                    Hardware (NIC)                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ Intel E1000 (or compatible)                         │   │
│  │ - MMIO registers from UEFI                          │   │
│  │ - MAC address from UEFI                             │   │
│  │ - IRQ line from UEFI                                │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Data Structures

### UDP Socket (src/net/stack.rs)

```rust
pub struct UdpSocket {
    pub local_port: u16,               // Local UDP port (bound)
    pub remote_ip: Option<[u8; 4]>,    // Connected remote IP (optional)
    pub remote_port: Option<u16>,      // Connected remote port (optional)
    pub in_use: bool,                  // Socket allocation flag
}
```

- **Total capacity**: 16 concurrent UDP sockets
- **State**: Unconnected (local_port only) or Connected (remote_ip + remote_port)
- **Binding**: Port uniqueness enforced across all sockets

### ARP Cache (src/net/arp.rs)

```rust
pub struct ArpEntry {
    pub ip: Ipv4Address,        // IPv4 address
    pub mac: MacAddress,        // Corresponding MAC address
    pub timestamp_ms: u64,      // Entry creation time
    pub valid: bool,            // Entry validity flag
}

pub struct ArpCache {
    entries: [ArpEntry; 32],    // Fixed-size cache
}
```

- **Capacity**: 32 entries
- **TTL**: 60 seconds (entries marked stale after timeout)
- **Replacement**: LRU (Least Recently Used)
- **Update**: ARP replies and requests update cache automatically

### UDP Header (src/net/udp.rs)

```rust
#[repr(C, packed)]
pub struct UdpHeader {
    pub src_port: u16,      // Source port (big-endian)
    pub dst_port: u16,      // Destination port (big-endian)
    pub length: u16,        // Total length including header (big-endian)
    pub checksum: u16,      // Checksum with IPv4 pseudo-header (big-endian)
}
```

**Checksum Calculation:**
```
Pseudo-header:
  +--------+--------+--------+--------+
  |     Source IPv4 Address (4 bytes)|
  +--------+--------+--------+--------+
  | Destination IPv4 Address (4 bytes)|
  +--------+--------+--------+--------+
  |  Zero  |Protocol|  UDP Length     |
  +--------+--------+--------+--------+

Protocol = 17 (UDP)
UDP Length = UDP header (8 bytes) + payload length
```

## Packet Flow

### Outgoing UDP Packet (Transmit)

```
1. Application calls udp_send(socket_idx, dst_ip, dst_port, payload)
   ↓
2. NetStack validates socket and device
   ↓
3. ARP cache lookup for dst_ip → dst_mac
   ↓ (if miss, return ArpCacheMiss error)
4. Build Ethernet frame:
   - dst_mac (from ARP cache)
   - src_mac (from device info)
   - EtherType = 0x0800 (IPv4)
   ↓
5. Build IPv4 header:
   - Version = 4, IHL = 5 (20 bytes)
   - Total Length = 20 + UDP packet length
   - Protocol = 17 (UDP)
   - Source IP = device IP
   - Destination IP = dst_ip
   - Checksum = computed over IP header
   ↓
6. Build UDP datagram:
   - Source Port = socket.local_port
   - Destination Port = dst_port
   - Length = 8 + payload length
   - Checksum = computed over pseudo-header + UDP header + payload
   ↓
7. Copy payload into packet buffer
   ↓
8. Push frame to TxBatch
   ↓
9. TxBatch flushed to NIC driver
   ↓
10. NIC driver transmits via DMA
```

### Incoming UDP Packet (Receive)

```
1. NIC receives Ethernet frame via DMA
   ↓
2. NIC driver drains RX ring, copies to kernel buffer
   ↓
3. NetStack.handle_frame(device_index, frame)
   ↓
4. Parse Ethernet header, check EtherType
   ↓ (if 0x0806, handle_arp; if 0x0800, handle_ipv4)
5. Parse IPv4 header:
   - Check destination IP matches device IP
   - Extract protocol field
   ↓ (if protocol = 17, handle_udp)
6. Parse UDP header:
   - Extract src_port, dst_port, length, checksum
   - Verify checksum (if non-zero)
   ↓
7. Find matching UDP socket:
   - Match dst_port with socket.local_port
   - If connected, match src_ip/src_port with remote_ip/remote_port
   ↓
8. Queue packet in socket receive buffer (TODO: not yet implemented)
   ↓
9. Wake up waiting user process (TODO: not yet implemented)
```

### ARP Request/Reply Flow

```
Outgoing ARP Request:
1. Need MAC for IP X.X.X.X
2. Check ARP cache → miss
3. Build ARP request:
   - HW Type = 1 (Ethernet)
   - Proto Type = 0x0800 (IPv4)
   - Operation = 1 (Request)
   - Sender HW = our MAC
   - Sender Proto = our IP
   - Target HW = 00:00:00:00:00:00
   - Target Proto = X.X.X.X
4. Wrap in Ethernet frame (dst = FF:FF:FF:FF:FF:FF broadcast)
5. Transmit

Incoming ARP Reply:
1. Receive Ethernet frame (EtherType = 0x0806)
2. Parse ARP packet
3. Verify HW/Proto types, address lengths
4. Extract sender MAC + IP
5. Update ARP cache: insert(sender_ip, sender_mac, now_ms)
6. If this is a request for our IP:
   - Build ARP reply with our MAC
   - Transmit back to sender
```

## Network Stack API

### Socket Management

```rust
// Allocate a UDP socket on specified port
pub fn udp_socket(&mut self, local_port: u16) -> Result<usize, NetError>

// Close UDP socket
pub fn udp_close(&mut self, socket_idx: usize) -> Result<(), NetError>

// Send UDP datagram
pub fn udp_send(
    &mut self,
    device_index: usize,
    socket_idx: usize,
    dst_ip: [u8; 4],
    dst_port: u16,
    payload: &[u8],
    tx: &mut TxBatch,
) -> Result<(), NetError>
```

### Device Registration

```rust
// Register network device (called during init)
pub fn register_device(&mut self, index: usize, mac: [u8; 6])
```

### Frame Processing

```rust
// Handle incoming Ethernet frame
pub fn handle_frame(
    &mut self,
    device_index: usize,
    frame: &[u8],
    tx: &mut TxBatch,
) -> Result<(), NetError>

// Periodic polling (TCP timers, etc.)
pub fn poll_device(
    &mut self,
    device_index: usize,
    now_ms: u64,
    tx: &mut TxBatch,
) -> Result<(), NetError>
```

## Error Handling

```rust
pub enum NetError {
    UnsupportedDevice,      // Device driver not found
    DeviceMissing,          // Device index invalid
    RxExhausted,            // No more RX buffers
    TxBusy,                 // TX queue full
    InvalidDescriptor,      // Bad device descriptor from UEFI
    HardwareFault,          // NIC hardware error
    BufferTooSmall,         // Packet exceeds MAX_FRAME_SIZE
    AddressInUse,           // Port already bound
    TooManyConnections,     // Socket table full
    InvalidSocket,          // Socket index invalid or closed
    InvalidDevice,          // Device not initialized
    ArpCacheMiss,           // No MAC address in ARP cache
}
```

## Testing

### Current Support

- ✅ ICMP echo (ping) replies
- ✅ TCP echo server on port 8080
- ✅ ARP request/reply handling
- ✅ UDP packet reception and parsing
- ✅ UDP packet transmission with checksum

### Limitations (TODO)

- ⏳ UDP socket receive buffer (packets logged but not queued)
- ⏳ System calls for socket operations (socket, bind, sendto, recvfrom)
- ⏳ User-space socket API in nrlib
- ⏳ ARP request generation on cache miss
- ⏳ UDP broadcast/multicast support
- ⏳ Socket timeout and error propagation
- ⏳ IPv4 fragmentation/reassembly

## Example Usage (Future)

```rust
// User-space UDP echo server
use nrlib::{socket, bind, recvfrom, sendto, AF_INET, SOCK_DGRAM};

fn main() {
    let sock = socket(AF_INET, SOCK_DGRAM, 0).unwrap();
    bind(sock, "0.0.0.0:8080").unwrap();
    
    let mut buf = [0u8; 1024];
    loop {
        let (len, src_addr) = recvfrom(sock, &mut buf).unwrap();
        println!("Received {} bytes from {}", len, src_addr);
        sendto(sock, &buf[..len], &src_addr).unwrap();
    }
}
```

## Configuration

### Network Device Assignment

Default IP addresses (src/net/stack.rs):
```rust
fn default_ip(index: usize) -> [u8; 4] {
    let last = 15 + (index as u8);
    [10, 0, 2, last]
}
```

- Device 0: 10.0.2.15
- Device 1: 10.0.2.16
- Device 2: 10.0.2.17
- Device 3: 10.0.2.18

### Socket Limits

- **UDP sockets**: 16 (MAX_UDP_SOCKETS)
- **Network devices**: 4 (MAX_NET_DEVICES)
- **ARP cache entries**: 32 (ARP_CACHE_SIZE)
- **TX batch buffers**: 4 (TX_BATCH_CAPACITY)
- **Max frame size**: 1536 bytes (MAX_FRAME_SIZE)

## Files Modified/Created

### New Files
- `src/net/arp.rs` - ARP protocol implementation
- `src/net/udp.rs` - UDP datagram structures and checksums
- `docs/en/UDP_NETWORK_STACK.md` - This document

### Modified Files
- `src/net/mod.rs` - Exported new modules (arp, ethernet, ipv4, udp)
- `src/net/stack.rs` - Added UDP socket management, ARP cache integration
- `src/net/drivers/mod.rs` - Extended NetError enum
- `src/net/ethernet.rs` - Already existed, now used by stack
- `src/net/ipv4.rs` - Already existed, now used by stack

## Next Steps

1. **Implement UDP receive buffers**: Queue incoming packets per socket
2. **Add socket system calls**:
   - `SYS_SOCKET` (create socket)
   - `SYS_BIND` (bind to local port)
   - `SYS_SENDTO` (send datagram)
   - `SYS_RECVFROM` (receive datagram)
3. **Implement ARP request generation**: Send ARP requests on cache miss
4. **Add user-space nrlib wrappers**: socket(), bind(), sendto(), recvfrom()
5. **Create UDP test applications**: Echo server/client
6. **Add socket options**: SO_REUSEADDR, SO_BROADCAST, etc.
7. **Implement select/poll**: Multiplexing multiple sockets
8. **Add IPv4 fragmentation**: Handle packets > MTU
