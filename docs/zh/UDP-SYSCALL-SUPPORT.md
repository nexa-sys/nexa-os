# UDP 系统调用支持

## 概述

NexaOS 现在通过标准的 POSIX socket 系统调用提供了全面的 UDP（用户数据报协议）支持。该实现遵循 Linux x86_64 系统调用约定，并支持 UDP 网络通信的基本操作。

## 支持的系统调用

### 1. **SYS_SOCKET** (系统调用 #41)
为 UDP 通信创建一个新的套接字。

**函数签名：**
```c
int socket(int domain, int type, int protocol);
```

**支持的参数：**
- `domain`: `AF_INET` (2) - 仅 IPv4
- `type`: `SOCK_DGRAM` (2) - UDP 数据报
- `protocol`: `0` 或 `IPPROTO_UDP` (17)

**返回值：**
- 成功时返回文件描述符 (>= 3)
- 失败时返回 -1 (设置 errno)

**示例：**
```c
int sockfd = socket(AF_INET, SOCK_DGRAM, 0);
if (sockfd < 0) {
    perror("socket");
    return -1;
}
```

### 2. **SYS_BIND** (系统调用 #49)
将 UDP 套接字绑定到本地地址和端口。

**函数签名：**
```c
int bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
```

**支持的参数：**
- `sockfd`: 来自 socket() 的套接字文件描述符
- `addr`: 指向 sockaddr_in 结构体的指针，包含：
  - `sa_family`: `AF_INET` (2)
  - `sa_data[0:1]`: 网络字节序端口（大端）
  - `sa_data[2:5]`: IPv4 地址（4 字节）
- `addrlen`: sockaddr 结构体的大小 (>= 8)

**返回值：**
- 成功时返回 0
- 失败时返回 -1 (设置 errno)

**错误码：**
- `EBADF`: 无效的套接字 fd
- `ENOTSOCK`: fd 不是套接字
- `EINVAL`: 无效的参数或端口为 0
- `EAFNOSUPPORT`: 不支持的地址族

**示例：**
```c
struct sockaddr_in addr;
addr.sin_family = AF_INET;
addr.sin_port = htons(5000);      // 端口 5000
addr.sin_addr.s_addr = htonl(INADDR_ANY);

if (bind(sockfd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
    perror("bind");
    return -1;
}
```

### 3. **SYS_SENDTO** (系统调用 #44)
向指定目的地发送 UDP 数据报。

**函数签名：**
```c
ssize_t sendto(int sockfd, const void *buf, size_t len, int flags,
               const struct sockaddr *dest_addr, socklen_t addrlen);
```

**支持的参数：**
- `sockfd`: 套接字文件描述符（必须先绑定）
- `buf`: 指向要发送的数据的指针
- `len`: 数据长度（必须 > 0）
- `flags`: 忽略（使用 0）
- `dest_addr`: 目标 sockaddr_in（包含端口和 IP）
- `addrlen`: sockaddr 结构体的大小 (>= 8)

**返回值：**
- 成功时返回发送的字节数
- 失败时返回 -1 (设置 errno)

**错误码：**
- `EBADF`: 无效的套接字 fd
- `ENOTSOCK`: fd 不是套接字
- `EINVAL`: 无效的参数或未绑定的套接字
- `EFAULT`: 无效的缓冲区指针
- `EAGAIN`/`EWOULDBLOCK`: 会阻塞（非阻塞套接字）

**示例：**
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

### 4. **SYS_RECVFROM** (系统调用 #45)
接收 UDP 数据报并返回源地址。

**函数签名：**
```c
ssize_t recvfrom(int sockfd, void *buf, size_t len, int flags,
                 struct sockaddr *src_addr, socklen_t *addrlen);
```

**支持的参数：**
- `sockfd`: 套接字文件描述符（必须已绑定）
- `buf`: 指向接收数据的缓冲区的指针
- `len`: 要接收的最大字节数（必须 > 0）
- `flags`: 忽略（使用 0）
- `src_addr`: 可选指针以接收源地址（可为 NULL）
- `addrlen`: 可选指针（可为 NULL）

**返回值：**
- 成功时返回接收的字节数
- 失败时返回 -1 (设置 errno)

**当前状态：**
- **占位符实现**：返回 `EAGAIN`（会阻塞）
- 完整实现需要网络驱动程序集成

**示例：**
```c
struct sockaddr_in src;
socklen_t src_len = sizeof(src);
char buf[1024];

ssize_t received = recvfrom(sockfd, buf, sizeof(buf), 0,
                            (struct sockaddr *)&src, &src_len);
if (received > 0) {
    printf("从 %s:%d 接收了 %ld 字节\n", received,
           inet_ntoa(src.sin_addr), ntohs(src.sin_port));
}
```

### 5. **SYS_CONNECT** (系统调用 #42)
为 UDP 套接字设置默认目的地（可选）。

**函数签名：**
```c
int connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
```

**支持的参数：**
- `sockfd`: UDP 套接字文件描述符
- `addr`: 目标 sockaddr_in 结构体
- `addrlen`: sockaddr 结构体的大小 (>= 8)

**返回值：**
- 成功时返回 0
- 失败时返回 -1 (设置 errno)

**注意：** 对于 UDP，`connect()` 不建立连接。它仅设置未来 `send()` 调用的默认目的地，允许 UDP 套接字使用 `send()` 而不是 `sendto()`。

**示例：**
```c
struct sockaddr_in peer;
peer.sin_family = AF_INET;
peer.sin_port = htons(5000);
inet_aton("192.168.1.50", &peer.sin_addr);

if (connect(sockfd, (struct sockaddr *)&peer, sizeof(peer)) < 0) {
    perror("connect");
}
// 现在可以使用 send() 而不是 sendto()
```

### 6. **SYS_CLOSE** (系统调用 #3)
关闭 UDP 套接字。

**函数签名：**
```c
int close(int fd);
```

**示例：**
```c
close(sockfd);
```

## 数据结构

### sockaddr_in
```c
struct sockaddr_in {
    unsigned short sin_family;      // AF_INET
    unsigned short sin_port;        // 端口（网络字节序）
    struct in_addr sin_addr;        // IP 地址（网络字节序）
    char sin_zero[8];              // 零填充
};

struct in_addr {
    uint32_t s_addr;               // 32 位 IP 地址
};
```

### 通用 sockaddr（在内核中使用）
```c
struct sockaddr {
    unsigned short sa_family;      // 地址族（AF_INET = 2）
    unsigned char sa_data[14];     // 地址数据：
                                   // [0:1]  = 端口（网络字节序）
                                   // [2:5]  = IPv4 地址
};
```

## 错误码

实现使用标准的 POSIX errno 值：

| 错误 | 值 | 含义 |
|------|------|------|
| `EAFNOSUPPORT` | 97 | 不支持的地址族 |
| `EBADF` | 9 | 坏的文件描述符 |
| `ENOTSOCK` | 88 | 非套接字上的套接字操作 |
| `EINVAL` | 22 | 无效参数 |
| `EMFILE` | 24 | 打开的文件过多 |
| `EFAULT` | 14 | 坏地址 |
| `ENOSYS` | 38 | 函数未实现 |
| `EAGAIN`/`EWOULDBLOCK` | 11/11 | 会阻塞（无数据可用） |

## 架构

### 套接字存储
- UDP 套接字存储在全局 `FILE_HANDLES` 数组中
- 每个套接字有一个 `FileBacking::Socket` 变体，包含：
  - `domain`: 地址族（AF_INET）
  - `socket_type`: 套接字类型（SOCK_DGRAM）
  - `protocol`: 协议（IPPROTO_UDP）
  - `socket_index`: 网络栈插槽（未来实现的占位符）

### 网络栈集成
- 当前实现是网络栈操作的**占位符**
- `sendto()` 返回成功但不实际传输
- `recvfrom()` 返回 EAGAIN（无可用数据）
- 未来版本将与网络驱动程序集成（`src/net/`）

## 限制

1. **仅阻塞操作**：尚不支持非阻塞模式
2. **无接收队列**：`recvfrom()` 总是返回 EAGAIN
3. **无实际网络 I/O**：发送/接收不与网络硬件交互
4. **单个地址族**：仅支持 AF_INET (IPv4)
5. **单个套接字类型**：仅支持 SOCK_DGRAM (UDP)
6. **有限的错误检查**：某些边界情况可能未处理

## 与网络栈的集成

要完全启用 UDP 网络：

1. 在 `src/net/stack.rs` 中分配 UDP 套接字
2. 实现通过 `src/net/drivers/` 的实际数据包传输
3. 设置数据包接收队列和中断处理
4. 实现阻塞操作的超时处理

框架已准备就绪；仅待网络驱动程序集成。

## 使用示例

```c
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <unistd.h>

int main() {
    // 创建 UDP 套接字
    int sockfd = socket(AF_INET, SOCK_DGRAM, 0);
    if (sockfd < 0) {
        perror("socket");
        return 1;
    }

    // 绑定到本地端口 5000
    struct sockaddr_in local;
    local.sin_family = AF_INET;
    local.sin_port = htons(5000);
    local.sin_addr.s_addr = htonl(INADDR_ANY);

    if (bind(sockfd, (struct sockaddr *)&local, sizeof(local)) < 0) {
        perror("bind");
        close(sockfd);
        return 1;
    }

    printf("UDP 套接字监听端口 5000\n");

    // 连接到远程对等体（可选）
    struct sockaddr_in peer;
    peer.sin_family = AF_INET;
    peer.sin_port = htons(8080);
    inet_aton("192.168.1.100", &peer.sin_addr);

    if (connect(sockfd, (struct sockaddr *)&peer, sizeof(peer)) < 0) {
        perror("connect");
    } else {
        // 发送消息
        const char *msg = "Hello, UDP!";
        if (sendto(sockfd, msg, strlen(msg), 0,
                   (struct sockaddr *)&peer, sizeof(peer)) < 0) {
            perror("sendto");
        }
    }

    // 尝试接收（现在会返回 EAGAIN）
    char buf[1024];
    struct sockaddr_in src;
    socklen_t src_len = sizeof(src);
    ssize_t n = recvfrom(sockfd, buf, sizeof(buf), 0,
                         (struct sockaddr *)&src, &src_len);
    if (n > 0) {
        printf("接收了 %ld 字节\n", n);
    } else if (n < 0) {
        perror("recvfrom");
    }

    close(sockfd);
    return 0;
}
```

## 未来增强

- [ ] 与网络驱动程序集成以进行实际数据包传输
- [ ] 为 UDP 套接字实现接收队列
- [ ] 添加非阻塞套接字支持
- [ ] 添加套接字选项支持（SO_RCVBUF、SO_SNDBUF 等）
- [ ] 实现 UDP 广播支持
- [ ] 添加 IPv6 支持（AF_INET6）
- [ ] 实现 TCP 支持（SOCK_STREAM）
