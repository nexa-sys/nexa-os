# ntcp2 - NexaOS QUIC Library

A modern QUIC protocol library for NexaOS with ngtcp2 ABI compatibility and tokio async backend.

## Features

- **Full QUIC Protocol Support** - Implements RFC 9000, RFC 9001, RFC 9002
- **ngtcp2 ABI Compatible** - Drop-in replacement for ngtcp2 (libngtcp2.so)
- **Tokio Async Backend** - High-performance asynchronous I/O
- **QPACK Header Compression** - Full RFC 9204 implementation
- **Connection Migration** - Seamless network path changes
- **0-RTT Early Data** - Fast connection establishment
- **Multipath QUIC** - Multiple network paths simultaneously
- **Datagram Support** - Unreliable datagrams (RFC 9221)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                        │
│  (ngtcp2-compatible C API or Native Rust API)              │
├─────────────────────────────────────────────────────────────┤
│                    Connection Layer                         │
│  - Stream management                                        │
│  - Flow control                                             │
│  - Congestion control                                       │
│  - Connection migration                                     │
├─────────────────────────────────────────────────────────────┤
│                    Packet Layer                             │
│  - Packet serialization/deserialization                    │
│  - QPACK encoding/decoding                                 │
│  - Loss detection & recovery                               │
├─────────────────────────────────────────────────────────────┤
│                    Crypto Layer                             │
│  - TLS 1.3 handshake integration (via nssl)                │
│  - AEAD encryption (AES-GCM, ChaCha20-Poly1305)            │
│  - Key derivation (HKDF)                                   │
├─────────────────────────────────────────────────────────────┤
│                    Transport Layer                          │
│  - Tokio async UDP I/O                                     │
│  - Path validation                                         │
└─────────────────────────────────────────────────────────────┘
```

## Protocol Support

### QUIC Version
- QUIC v1 (RFC 9000) - `0x00000001`
- QUIC v2 (RFC 9369) - `0x6b3343cf`

### Congestion Control
- New Reno (default)
- Cubic
- BBR (experimental)

### Loss Detection
- Packet threshold (3 packets)
- Time threshold (9/8 * max(smoothed_rtt, latest_rtt))
- PTO (Probe Timeout) based retransmission

## Usage

### Rust API (Async)

```rust
use ntcp2::async_io::{Client, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create QUIC client
    let client = Client::connect("example.com:443").await?;
    
    // Open a bidirectional stream
    let stream = client.open_stream().await?;
    
    // Send data
    stream.write(b"Hello, QUIC!").await?;
    
    // Receive response
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    
    println!("Received: {:?}", &buf[..n]);
    
    Ok(())
}
```

### Rust API (Sync/Callbacks)

```rust
use ntcp2::{Connection, ConnectionCallbacks, Config};
use std::net::UdpSocket;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = Config::new()?;
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    
    // Create callbacks
    let callbacks = ConnectionCallbacks::new();
    
    // Create a client connection
    let conn = Connection::client(&config, callbacks)?;
    
    // Get data to send
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    
    // Write handshake data
    let mut buf = vec![0u8; 1350];
    let (n, _) = conn.write_pkt(&mut buf)?;
    socket.send_to(&buf[..n], "example.com:443")?;
    
    // Receive and process
    let mut recv_buf = vec![0u8; 65535];
    let (n, peer) = socket.recv_from(&mut recv_buf)?;
    conn.read_pkt(&recv_buf[..n], peer)?;
    
    Ok(())
}
```

### C API (ngtcp2 compatible)

```c
#include <ngtcp2/ngtcp2.h>

ngtcp2_conn *conn;
ngtcp2_settings settings;
ngtcp2_transport_params params;
ngtcp2_callbacks callbacks;

// Initialize settings
ngtcp2_settings_default(&settings);

// Initialize transport parameters
ngtcp2_transport_params_default(&params);
params.initial_max_data = 10000000;
params.initial_max_stream_data_bidi_local = 1000000;

// Set callbacks
memset(&callbacks, 0, sizeof(callbacks));
callbacks.recv_crypto_data = recv_crypto_data_cb;
callbacks.encrypt = encrypt_cb;
callbacks.decrypt = decrypt_cb;
// ... more callbacks

// Create client connection
ngtcp2_conn_client_new(&conn, &dcid, &scid, &path,
                        NGTCP2_PROTO_VER_V1, &callbacks,
                        &settings, &params, NULL, user_data);

// Write packets
uint8_t buf[1350];
ngtcp2_ssize written = ngtcp2_conn_write_pkt(conn, &path, NULL,
                                              buf, sizeof(buf), ts);

// Read received packets
ngtcp2_conn_read_pkt(conn, &path, NULL, data, datalen, ts);
```

## Build

```bash
# Build the library
cargo build --release

# Run tests
cargo test

# Build with all features
cargo build --release --features "async-tokio,qpack,migration,early-data,datagram"
```

## Dependencies

- **nssl** - TLS 1.3 for QUIC handshake
- **ncryptolib** - Cryptographic primitives (AEAD, HKDF)
- **tokio** - Async runtime (optional)

## Specification Compliance

| RFC | Title | Status |
|-----|-------|--------|
| RFC 9000 | QUIC: A UDP-Based Multiplexed and Secure Transport | ✅ Full |
| RFC 9001 | Using TLS to Secure QUIC | ✅ Full |
| RFC 9002 | QUIC Loss Detection and Congestion Control | ✅ Full |
| RFC 9204 | QPACK: Field Compression for HTTP/3 | ✅ Full |
| RFC 9221 | An Unreliable Datagram Extension to QUIC | ✅ Full |
| RFC 9287 | Greasing the QUIC Bit | ✅ Full |
| RFC 9369 | QUIC Version 2 | ✅ Full |

## License

Same as NexaOS kernel license.
