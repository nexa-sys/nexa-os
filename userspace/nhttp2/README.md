# nhttp2 - NexaOS HTTP/2 Library

A modern HTTP/2 library for NexaOS with nghttp2 ABI compatibility and tokio async backend.

## Features

- **Full HTTP/2 Protocol Support** - Implements RFC 7540 and RFC 9113
- **nghttp2 ABI Compatible** - Drop-in replacement for nghttp2 (libnghttp2.so.14)
- **Tokio Async Backend** - High-performance asynchronous I/O
- **HPACK Header Compression** - Full RFC 7541 implementation with Huffman coding
- **Flow Control** - Automatic and manual flow control management
- **Stream Priority** - RFC 7540 priority tree implementation
- **Server Push** - Support for HTTP/2 server push

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                        │
│  (nghttp2-compatible C API or Native Rust API)             │
├─────────────────────────────────────────────────────────────┤
│                    Session Layer                            │
│  - Stream management                                        │
│  - Flow control                                             │
│  - Priority handling                                        │
├─────────────────────────────────────────────────────────────┤
│                    Frame Layer                              │
│  - Frame serialization/deserialization                     │
│  - HPACK encoding/decoding                                 │
├─────────────────────────────────────────────────────────────┤
│                    Transport Layer                          │
│  - Tokio async I/O                                         │
│  - TLS integration (via nssl)                              │
└─────────────────────────────────────────────────────────────┘
```

## Usage

### Rust API (Async)

```rust
use nhttp2::async_io::{Client, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create HTTP/2 client
    let client = Client::new();
    
    // Create a request
    let request = Request::get("https://example.com/")
        .header("user-agent", "nhttp2/1.0")
        .build()?;
    
    // Send request and get response
    let response = client.send(request).await?;
    
    println!("Status: {}", response.status());
    println!("Body: {:?}", response.body());
    
    Ok(())
}
```

### Rust API (Sync/Callbacks)

```rust
use nhttp2::{Session, SessionBuilder, SessionCallbacks, HeaderField};
use std::ptr::null_mut;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create callbacks
    let callbacks = SessionCallbacks::new();
    
    // Create a client session
    let session = Session::client(callbacks, null_mut());
    
    // Submit a request
    let headers = vec![
        HeaderField::new(":method", "GET"),
        HeaderField::new(":scheme", "https"),
        HeaderField::new(":path", "/"),
        HeaderField::new(":authority", "example.com"),
    ];
    
    let stream_id = session.submit_request(None, &headers, None)?;
    
    // Send and receive data
    let data_to_send = session.mem_send();
    // ... send data over network ...
    
    // ... receive data from network ...
    let received_bytes = session.mem_recv(&received_data)?;
    
    Ok(())
}
```

### C API (nghttp2 compatible)

```c
#include <nghttp2/nghttp2.h>

int main() {
    nghttp2_session *session;
    nghttp2_session_callbacks *callbacks;
    
    nghttp2_session_callbacks_new(&callbacks);
    nghttp2_session_callbacks_set_send_callback(callbacks, send_callback);
    nghttp2_session_callbacks_set_recv_callback(callbacks, recv_callback);
    nghttp2_session_callbacks_set_on_frame_recv_callback(callbacks, on_frame_recv);
    nghttp2_session_callbacks_set_on_data_chunk_recv_callback(callbacks, on_data_recv);
    
    nghttp2_session_client_new(&session, callbacks, user_data);
    
    // Submit request
    nghttp2_nv nva[] = {
        MAKE_NV(":method", "GET"),
        MAKE_NV(":scheme", "https"),
        MAKE_NV(":path", "/"),
        MAKE_NV(":authority", "example.com"),
    };
    
    nghttp2_submit_request(session, NULL, nva, sizeof(nva)/sizeof(nva[0]), NULL, NULL);
    
    // Event loop
    while (nghttp2_session_want_read(session) || nghttp2_session_want_write(session)) {
        nghttp2_session_send(session);
        nghttp2_session_recv(session);
    }
    
    nghttp2_session_del(session);
    nghttp2_session_callbacks_del(callbacks);
    
    return 0;
}
```

## Building

```bash
# Build all userspace libraries including nhttp2
./scripts/build.sh userspace

# Build only nhttp2
cd userspace/nhttp2
cargo build --release
```

## ABI Compatibility

The library provides nghttp2-compatible symbols with the following SONAME:
- `libnghttp2.so.14`

This allows nhttp2 to be used as a drop-in replacement for nghttp2 in existing applications.

## Frame Types Supported

| Frame Type | ID | Description |
|------------|----|-----------------------|
| DATA | 0x0 | Payload data |
| HEADERS | 0x1 | Header fields |
| PRIORITY | 0x2 | Stream priority (deprecated) |
| RST_STREAM | 0x3 | Stream termination |
| SETTINGS | 0x4 | Connection settings |
| PUSH_PROMISE | 0x5 | Server push |
| PING | 0x6 | Connection liveness |
| GOAWAY | 0x7 | Graceful shutdown |
| WINDOW_UPDATE | 0x8 | Flow control |
| CONTINUATION | 0x9 | Header continuation |

## Error Codes

| Error | Value | Description |
|-------|-------|-------------|
| NO_ERROR | 0x0 | Graceful shutdown |
| PROTOCOL_ERROR | 0x1 | Protocol error |
| INTERNAL_ERROR | 0x2 | Internal error |
| FLOW_CONTROL_ERROR | 0x3 | Flow control exceeded |
| SETTINGS_TIMEOUT | 0x4 | Settings timeout |
| STREAM_CLOSED | 0x5 | Stream is closed |
| FRAME_SIZE_ERROR | 0x6 | Invalid frame size |
| REFUSED_STREAM | 0x7 | Stream refused |
| CANCEL | 0x8 | Stream cancelled |
| COMPRESSION_ERROR | 0x9 | HPACK error |
| CONNECT_ERROR | 0xa | Connect error |
| ENHANCE_YOUR_CALM | 0xb | Rate limiting |
| INADEQUATE_SECURITY | 0xc | TLS requirements |
| HTTP_1_1_REQUIRED | 0xd | HTTP/1.1 required |

## License

Same as NexaOS kernel license.
