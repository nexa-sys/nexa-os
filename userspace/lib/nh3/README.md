# NexaOS HTTP/3 Library (nh3)

A modern, nghttp3 ABI-compatible HTTP/3 library for NexaOS with QUIC backend via ntcp2.

## Features

- **Full HTTP/3 protocol support** (RFC 9114)
- **nghttp3 C ABI compatibility** for drop-in replacement
- **QUIC transport via ntcp2** (dynamic linking to libngtcp2.so)
- **QPACK header compression** (RFC 9204)
- **Server push support**
- **Priority handling** (RFC 9218)

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                        │
│  (nghttp3-compatible C API or Native Rust API)             │
├─────────────────────────────────────────────────────────────┤
│                    HTTP/3 Layer (nh3)                       │
│  - Stream management                                        │
│  - Request/Response handling                                │
│  - QPACK encoding/decoding                                 │
├─────────────────────────────────────────────────────────────┤
│                    QUIC Layer (ntcp2)                       │
│  - Connection management                                    │
│  - Flow control & congestion control                       │
│  - Loss detection & recovery                               │
├─────────────────────────────────────────────────────────────┤
│                    Crypto Layer (nssl)                      │
│  - TLS 1.3 handshake                                       │
│  - AEAD encryption                                         │
└─────────────────────────────────────────────────────────────┘
```

## HTTP/3 Stream Types

HTTP/3 uses QUIC streams with specific types:

| Stream Type | Stream ID Pattern | Purpose |
|-------------|-------------------|---------|
| Control | Client: 0x02, Server: 0x03 | Settings, GOAWAY frames |
| QPACK Encoder | Client: 0x06, Server: 0x07 | Dynamic table updates |
| QPACK Decoder | Client: 0x0A, Server: 0x0B | Acknowledgments |
| Request | Client-initiated bidi | HTTP requests/responses |
| Push | Server-initiated uni | Server push |

## Usage (Rust API)

```rust
use nh3::{Connection, Config, StreamId};

// Create configuration
let config = Config::default();

// Create a client connection
let conn = Connection::client(&config)?;

// Submit a request
let stream_id = conn.submit_request(
    &[
        (":method", "GET"),
        (":scheme", "https"),
        (":path", "/"),
        (":authority", "example.com"),
    ],
    None,
)?;

// Process data
conn.read_stream(stream_id, &mut buf)?;
```

## Usage (C API - nghttp3 compatible)

```c
#include <nghttp3/nghttp3.h>

nghttp3_conn *conn;
nghttp3_callbacks callbacks;
nghttp3_settings settings;

nghttp3_settings_default(&settings);
nghttp3_callbacks_default(&callbacks);

nghttp3_conn_client_new(&conn, &callbacks, &settings, NULL, user_data);

// Bind QUIC streams
nghttp3_conn_bind_control_stream(conn, ctrl_stream_id);
nghttp3_conn_bind_qpack_streams(conn, qenc_stream_id, qdec_stream_id);

// Submit request
nghttp3_submit_request(conn, stream_id, nva, nvlen, NULL, NULL);
```

## Dependencies

- **ntcp2** (libngtcp2.so): QUIC protocol implementation
- **nssl** (libssl.so): TLS 1.3 support (transitive via ntcp2)
- **ncryptolib** (libcrypto.so): Cryptographic operations (transitive via ntcp2)

## Building

```bash
cd userspace/nh3
cargo build --release
```

The build produces:
- `libnghttp3.so` - Dynamic shared object
- `libnghttp3.a` - Static library
- Rust rlib for Rust consumers

## License

Same as NexaOS project license.
