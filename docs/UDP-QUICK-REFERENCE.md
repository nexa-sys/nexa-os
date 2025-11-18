# UDP Socket Quick Reference

## Creating and Using UDP Sockets

### Basic Pattern

```c
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <string.h>
#include <unistd.h>

int main() {
    // Step 1: Create socket
    int sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock < 0) {
        perror("socket");
        return 1;
    }

    // Step 2: Bind to local address
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_ANY);  // Any interface
    addr.sin_port = htons(5000);               // Port 5000

    if (bind(sock, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        perror("bind");
        close(sock);
        return 1;
    }

    // Step 3: Send data (optional - if you know destination)
    struct sockaddr_in dest;
    memset(&dest, 0, sizeof(dest));
    dest.sin_family = AF_INET;
    dest.sin_port = htons(8080);
    inet_aton("192.168.1.100", &dest.sin_addr);

    const char *msg = "Hello";
    ssize_t sent = sendto(sock, msg, 5, 0,
                          (struct sockaddr *)&dest, sizeof(dest));
    if (sent < 0) {
        perror("sendto");
    }

    // Step 4: Receive data (currently returns EAGAIN)
    char buf[1024];
    struct sockaddr_in src;
    socklen_t src_len = sizeof(src);
    ssize_t n = recvfrom(sock, buf, sizeof(buf), 0,
                         (struct sockaddr *)&src, &src_len);
    if (n > 0) {
        printf("Received %ld bytes from %s:%d\n", n,
               inet_ntoa(src.sin_addr), ntohs(src.sin_port));
    } else if (n < 0) {
        perror("recvfrom");
    }

    // Step 5: Close socket
    close(sock);
    return 0;
}
```

## Syscall Quick Reference Table

| Syscall | Number | Purpose | Return |
|---------|--------|---------|--------|
| `socket()` | 41 | Create UDP socket | fd >= 3 or -1 |
| `bind()` | 49 | Bind to local address | 0 or -1 |
| `sendto()` | 44 | Send datagram | bytes sent or -1 |
| `recvfrom()` | 45 | Receive datagram | bytes received or -1 |
| `connect()` | 42 | Set default destination | 0 or -1 |
| `close()` | 3 | Close socket | 0 or -1 |

## Common Errors and Solutions

### Error: EAFNOSUPPORT (97)
**Cause:** Using unsupported address family (not AF_INET)
```c
// ❌ Wrong
socket(AF_INET6, SOCK_DGRAM, 0);  // IPv6 not supported

// ✅ Correct
socket(AF_INET, SOCK_DGRAM, 0);   // IPv4 only
```

### Error: ENOTSOCK (88)
**Cause:** Using non-socket fd or closed socket
```c
// ❌ Wrong - using file fd
int fd = open("file.txt", O_RDONLY);
bind(fd, ...);  // ENOTSOCK

// ✅ Correct - use socket fd
int sock = socket(AF_INET, SOCK_DGRAM, 0);
bind(sock, ...);
```

### Error: EINVAL (22)
**Cause:** Invalid parameters (port 0, null address, etc.)
```c
// ❌ Wrong - port 0
addr.sin_port = htons(0);
bind(sock, (struct sockaddr *)&addr, sizeof(addr));

// ✅ Correct - valid port
addr.sin_port = htons(5000);
bind(sock, (struct sockaddr *)&addr, sizeof(addr));
```

### Error: EAGAIN (11)
**Cause:** No data available (receive would block)
```c
// Current behavior - recvfrom() always returns EAGAIN
ssize_t n = recvfrom(sock, buf, sizeof(buf), 0, NULL, NULL);
if (n < 0 && errno == EAGAIN) {
    printf("No data available yet\n");
}

// In future, can use select()/poll() to check for data
```

## Address Struct Usage

### Creating an address structure
```c
struct sockaddr_in addr;

// Clear it first
memset(&addr, 0, sizeof(addr));

// Set address family
addr.sin_family = AF_INET;

// Set port (network byte order - big endian)
addr.sin_port = htons(5000);  // Port 5000

// Set IP address
// Option 1: Any interface
addr.sin_addr.s_addr = htonl(INADDR_ANY);

// Option 2: Specific IP
inet_aton("192.168.1.100", &addr.sin_addr);

// Option 3: Loopback
addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
```

### Using with syscalls
```c
// Cast to generic sockaddr
bind(sock, (struct sockaddr *)&addr, sizeof(addr));
connect(sock, (struct sockaddr *)&addr, sizeof(addr));
sendto(sock, buf, len, 0, (struct sockaddr *)&addr, sizeof(addr));
```

## Common Patterns

### Server: Listen and Send Response
```c
// Bind to local port
struct sockaddr_in addr;
memset(&addr, 0, sizeof(addr));
addr.sin_family = AF_INET;
addr.sin_port = htons(5000);
addr.sin_addr.s_addr = htonl(INADDR_ANY);
bind(sock, (struct sockaddr *)&addr, sizeof(addr));

// Receive (currently returns EAGAIN)
char buf[1024];
struct sockaddr_in client;
socklen_t client_len = sizeof(client);
ssize_t n = recvfrom(sock, buf, sizeof(buf), 0,
                     (struct sockaddr *)&client, &client_len);

// Send response
if (n > 0) {
    const char *reply = "ACK";
    sendto(sock, reply, 3, 0,
           (struct sockaddr *)&client, client_len);
}
```

### Client: Send and Receive
```c
// Create socket (no need to bind for client)
int sock = socket(AF_INET, SOCK_DGRAM, 0);

// Set destination
struct sockaddr_in dest;
memset(&dest, 0, sizeof(dest));
dest.sin_family = AF_INET;
dest.sin_port = htons(5000);
inet_aton("192.168.1.100", &dest.sin_addr);

// Send
const char *msg = "Hello Server";
sendto(sock, msg, strlen(msg), 0,
       (struct sockaddr *)&dest, sizeof(dest));

// Or use connect() for default destination
connect(sock, (struct sockaddr *)&dest, sizeof(dest));
// Then can use send() instead of sendto()
send(sock, msg, strlen(msg), 0);

// Try to receive response
char buf[1024];
ssize_t n = recvfrom(sock, buf, sizeof(buf), 0, NULL, NULL);
```

## Helper Macros

```c
// Convert network byte order to host
#define ntohs(x) ((((x) & 0xff) << 8) | ((x) >> 8))
#define ntohl(x) (((x) & 0xff000000) >> 24 | \
                  ((x) & 0xff0000) >> 8 | \
                  ((x) & 0xff00) << 8 | \
                  ((x) & 0xff) << 24)

// Convert host byte order to network
#define htons(x) ntohs(x)
#define htonl(x) ntohl(x)
```

## Testing

### Compile
```bash
gcc -o udp_test udp_test.c
```

### Run
```bash
./scripts/run-qemu.sh
# In QEMU
./udp_test
```

### Check with kernel logs
```
[SYS_SOCKET] domain=2 type=2 protocol=0
[SYS_SOCKET] Created UDP socket at fd 3
[SYS_BIND] sockfd=3 addrlen=16
[SYS_BIND] UDP socket fd 3 bound to 0.0.0.0:5000
[SYS_SENDTO] sockfd=3 len=5 addrlen=16
[SYS_SENDTO] Sending 5 bytes to 192.168.1.100:8080
[SYS_RECVFROM] sockfd=3 len=1024
[SYS_RECVFROM] No data available (would block)
```

## Current Limitations

1. ❌ `recvfrom()` always returns EAGAIN (no data)
2. ❌ No actual packet transmission
3. ❌ No receive queue
4. ❌ No non-blocking support
5. ❌ No socket options
6. ❌ No broadcast support
7. ❌ IPv6 not supported (AF_INET6)

These will be implemented in future versions with network driver integration.

## References

- `man 2 socket` - POSIX socket syscall
- `man 2 bind` - Bind syscall
- `man 2 sendto` - Send datagram syscall
- `man 2 recvfrom` - Receive datagram syscall
- `man 2 connect` - Connect syscall
