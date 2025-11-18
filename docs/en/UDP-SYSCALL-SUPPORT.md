# UDP System Call Support

## Overview

NexaOS now provides comprehensive UDP (User Datagram Protocol) support through standard POSIX socket syscalls. This implementation follows Linux x86_64 syscall conventions and supports the fundamental operations for UDP networking.

## Supported Syscalls

### 1. **SYS_SOCKET** (syscall #41)
Creates a new socket for UDP communication.

**Signature:**
```c
int socket(int domain, int type, int protocol);
```

**Supported Parameters:**
- `domain`: `AF_INET` (2) - IPv4 only
- `type`: `SOCK_DGRAM` (2) - UDP datagrams
- `protocol`: `0` or `IPPROTO_UDP` (17)

**Returns:**
- File descriptor (>= 3) on success
- -1 on error (errno set)

**Example:**
```c
int sockfd = socket(AF_INET, SOCK_DGRAM, 0);
if (sockfd < 0) {
    perror("socket");
    return -1;
}
```

### 2. **SYS_BIND** (syscall #49)
Binds a UDP socket to a local address and port.

**Signature:**
```c
int bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
```

**Supported Parameters:**
- `sockfd`: Socket file descriptor from socket()
- `addr`: Pointer to sockaddr_in structure with:
  - `sa_family`: `AF_INET` (2)
  - `sa_data[0:1]`: Port in network byte order (big-endian)
  - `sa_data[2:5]`: IPv4 address (4 bytes)
- `addrlen`: Size of sockaddr structure (>= 8)

**Returns:**
- 0 on success
- -1 on error (errno set)

**Errors:**
- `EBADF`: Invalid socket fd
- `ENOTSOCK`: fd is not a socket
- `EINVAL`: Invalid arguments or port is 0
- `EAFNOSUPPORT`: Unsupported address family

**Example:**
```c
struct sockaddr_in addr;
addr.sin_family = AF_INET;
addr.sin_port = htons(5000);      // Port 5000
addr.sin_addr.s_addr = htonl(INADDR_ANY);

if (bind(sockfd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
    perror("bind");
    return -1;
}
```

### 3. **SYS_SENDTO** (syscall #44)
Sends a UDP datagram to a specified destination.

**Signature:**
```c
ssize_t sendto(int sockfd, const void *buf, size_t len, int flags,
               const struct sockaddr *dest_addr, socklen_t addrlen);
```

**Supported Parameters:**
- `sockfd`: Socket file descriptor (must be bound first)
- `buf`: Pointer to data to send
- `len`: Length of data (must be > 0)
- `flags`: Ignored (use 0)
- `dest_addr`: Destination sockaddr_in with port and IP
- `addrlen`: Size of sockaddr structure (>= 8)

**Returns:**
- Number of bytes sent on success
- -1 on error (errno set)

**Errors:**
- `EBADF`: Invalid socket fd
- `ENOTSOCK`: fd is not a socket
- `EINVAL`: Invalid parameters or unbound socket
- `EFAULT`: Invalid buffer pointer
- `EAGAIN`/`EWOULDBLOCK`: Would block (non-blocking socket)

**Example:**
```c
struct sockaddr_in dest;
dest.sin_family = AF_INET;
dest.sin_port = htons(8080);
inet_aton("192.168.1.100", &dest.sin_addr);

const char *msg = "Hello, UDP!";
ssize_t sent = sendto(sockfd, msg, strlen(msg), 0,
                      (struct sockaddr *)&dest, sizeof(dest));
if (sent < 0) {
    perror("sendto");
}
```

### 4. **SYS_RECVFROM** (syscall #45)
Receives a UDP datagram and returns the source address.

**Signature:**
```c
ssize_t recvfrom(int sockfd, void *buf, size_t len, int flags,
                 struct sockaddr *src_addr, socklen_t *addrlen);
```

**Supported Parameters:**
- `sockfd`: Socket file descriptor (must be bound)
- `buf`: Pointer to buffer for received data
- `len`: Maximum bytes to receive (must be > 0)
- `flags`: Ignored (use 0)
- `src_addr`: Optional pointer to receive source address (can be NULL)
- `addrlen`: Optional pointer to addrlen (can be NULL)

**Returns:**
- Number of bytes received on success
- -1 on error (errno set)

**Current Status:**
- **Placeholder Implementation**: Returns `EAGAIN` (would block)
- Full implementation requires network driver integration

**Example:**
```c
struct sockaddr_in src;
socklen_t src_len = sizeof(src);
char buf[1024];

ssize_t received = recvfrom(sockfd, buf, sizeof(buf), 0,
                            (struct sockaddr *)&src, &src_len);
if (received > 0) {
    printf("Received %ld bytes from %s:%d\n", received,
           inet_ntoa(src.sin_addr), ntohs(src.sin_port));
}
```

### 5. **SYS_CONNECT** (syscall #42)
Sets the default destination for a UDP socket (optional).

**Signature:**
```c
int connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
```

**Supported Parameters:**
- `sockfd`: UDP socket file descriptor
- `addr`: Destination sockaddr_in structure
- `addrlen`: Size of sockaddr structure (>= 8)

**Returns:**
- 0 on success
- -1 on error (errno set)

**Note:** For UDP, `connect()` does NOT establish a connection. It merely sets the default destination for future `send()` calls, allowing UDP sockets to use `send()` instead of `sendto()`.

**Example:**
```c
struct sockaddr_in peer;
peer.sin_family = AF_INET;
peer.sin_port = htons(5000);
inet_aton("192.168.1.50", &peer.sin_addr);

if (connect(sockfd, (struct sockaddr *)&peer, sizeof(peer)) < 0) {
    perror("connect");
}
// Now can use send() instead of sendto()
```

### 6. **SYS_CLOSE** (syscall #3)
Closes a UDP socket.

**Signature:**
```c
int close(int fd);
```

**Example:**
```c
close(sockfd);
```

## Data Structures

### sockaddr_in
```c
struct sockaddr_in {
    unsigned short sin_family;      // AF_INET
    unsigned short sin_port;        // Port (network byte order)
    struct in_addr sin_addr;        // IP address (network byte order)
    char sin_zero[8];              // Zero padding
};

struct in_addr {
    uint32_t s_addr;               // 32-bit IP address
};
```

### Generic sockaddr (as used in kernel)
```c
struct sockaddr {
    unsigned short sa_family;      // Address family (AF_INET = 2)
    unsigned char sa_data[14];     // Address data:
                                   // [0:1]  = port (network byte order)
                                   // [2:5]  = IPv4 address
};
```

## Error Codes

The implementation uses standard POSIX errno values:

| Error | Value | Meaning |
|-------|-------|---------|
| `EAFNOSUPPORT` | 97 | Unsupported address family |
| `EBADF` | 9 | Bad file descriptor |
| `ENOTSOCK` | 88 | Socket operation on non-socket |
| `EINVAL` | 22 | Invalid argument |
| `EMFILE` | 24 | Too many open files |
| `EFAULT` | 14 | Bad address |
| `ENOSYS` | 38 | Function not implemented |
| `EAGAIN`/`EWOULDBLOCK` | 11/11 | Would block (no data available) |

## Architecture

### Socket Storage
- UDP sockets are stored in the global `FILE_HANDLES` array
- Each socket has a `FileBacking::Socket` variant containing:
  - `domain`: Address family (AF_INET)
  - `socket_type`: Socket type (SOCK_DGRAM)
  - `protocol`: Protocol (IPPROTO_UDP)
  - `socket_index`: Network stack slot (placeholder for future implementation)

### Network Stack Integration
- Current implementation is a **placeholder** for network stack operations
- `sendto()` returns success but doesn't actually transmit
- `recvfrom()` returns EAGAIN (no data available)
- Future versions will integrate with the network driver (`src/net/`)

## Limitations

1. **Blocking Operations Only**: No non-blocking mode support yet
2. **No Receive Queue**: `recvfrom()` always returns EAGAIN
3. **No Actual Network I/O**: Send/receive don't interact with network hardware
4. **Single Address Family**: Only AF_INET (IPv4) supported
5. **Single Socket Type**: Only SOCK_DGRAM (UDP) supported
6. **Limited Error Checking**: Some edge cases may not be handled

## Integration with Network Stack

To fully enable UDP networking:

1. Allocate UDP sockets in `src/net/stack.rs`
2. Implement actual packet transmission via `src/net/drivers/`
3. Set up packet reception queue and interrupt handling
4. Implement timeout handling for blocking operations

The framework is ready; only the network driver integration is pending.

## Usage Example

```c
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <unistd.h>

int main() {
    // Create UDP socket
    int sockfd = socket(AF_INET, SOCK_DGRAM, 0);
    if (sockfd < 0) {
        perror("socket");
        return 1;
    }

    // Bind to local port 5000
    struct sockaddr_in local;
    local.sin_family = AF_INET;
    local.sin_port = htons(5000);
    local.sin_addr.s_addr = htonl(INADDR_ANY);

    if (bind(sockfd, (struct sockaddr *)&local, sizeof(local)) < 0) {
        perror("bind");
        close(sockfd);
        return 1;
    }

    printf("UDP socket listening on port 5000\n");

    // Connect to remote peer (optional)
    struct sockaddr_in peer;
    peer.sin_family = AF_INET;
    peer.sin_port = htons(8080);
    inet_aton("192.168.1.100", &peer.sin_addr);

    if (connect(sockfd, (struct sockaddr *)&peer, sizeof(peer)) < 0) {
        perror("connect");
    } else {
        // Send message
        const char *msg = "Hello, UDP!";
        if (sendto(sockfd, msg, strlen(msg), 0,
                   (struct sockaddr *)&peer, sizeof(peer)) < 0) {
            perror("sendto");
        }
    }

    // Try to receive (will return EAGAIN for now)
    char buf[1024];
    struct sockaddr_in src;
    socklen_t src_len = sizeof(src);
    ssize_t n = recvfrom(sockfd, buf, sizeof(buf), 0,
                         (struct sockaddr *)&src, &src_len);
    if (n > 0) {
        printf("Received %ld bytes\n", n);
    } else if (n < 0) {
        perror("recvfrom");
    }

    close(sockfd);
    return 0;
}
```

## Future Enhancements

- [ ] Integrate with network driver for actual packet transmission
- [ ] Implement receive queue for UDP sockets
- [ ] Add support for non-blocking sockets
- [ ] Add support for socket options (SO_RCVBUF, SO_SNDBUF, etc.)
- [ ] Implement UDP broadcast support
- [ ] Add IPv6 support (AF_INET6)
- [ ] Implement TCP support (SOCK_STREAM)
