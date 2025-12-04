//! Network related syscalls
//!
//! Implements: socket, bind, connect, sendto, recvfrom, setsockopt

use super::types::*;
use crate::posix;
use crate::process::{ProcessState, USER_REGION_SIZE, USER_VIRT_BASE};
use crate::scheduler;
use crate::{kinfo, ktrace, kwarn};
use alloc::boxed::Box;
use core::{mem, slice};
use spin::Mutex;

const MAX_DNS_SERVERS: usize = 3;

#[derive(Clone, Copy)]
struct DnsConfig {
    servers: [[u8; 4]; MAX_DNS_SERVERS],
    count: usize,
}

impl DnsConfig {
    const fn new() -> Self {
        Self {
            servers: [[0; 4]; MAX_DNS_SERVERS],
            count: 0,
        }
    }

    fn update(&mut self, entries: &[[u8; 4]]) {
        let count = entries.len().min(MAX_DNS_SERVERS);
        for idx in 0..count {
            self.servers[idx] = entries[idx];
        }
        for idx in count..MAX_DNS_SERVERS {
            self.servers[idx] = [0; 4];
        }
        self.count = count;
    }
}

static DNS_CONFIG: Mutex<DnsConfig> = Mutex::new(DnsConfig::new());

/// SYS_SOCKET - Create a socket
pub fn socket(domain: i32, socket_type: i32, protocol: i32) -> u64 {
    kinfo!(
        "[SYS_SOCKET] domain={} type={} protocol={}",
        domain,
        socket_type,
        protocol
    );

    if domain != AF_INET && domain != AF_NETLINK {
        kwarn!("[SYS_SOCKET] Unsupported domain: {}", domain);
        posix::set_errno(posix::errno::EAFNOSUPPORT);
        return u64::MAX;
    }

    if socket_type != SOCK_DGRAM && socket_type != SOCK_RAW && socket_type != SOCK_STREAM {
        kwarn!("[SYS_SOCKET] Unsupported socket type: {}", socket_type);
        posix::set_errno(posix::errno::ENOSYS);
        return u64::MAX;
    }

    if protocol != 0 && protocol != IPPROTO_UDP && protocol != IPPROTO_TCP && domain != AF_NETLINK {
        kwarn!("[SYS_SOCKET] Unsupported protocol: {}", protocol);
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let actual_protocol = if protocol == 0 {
        if socket_type == SOCK_STREAM {
            IPPROTO_TCP
        } else if socket_type == SOCK_DGRAM {
            IPPROTO_UDP
        } else {
            0
        }
    } else {
        protocol
    };

    unsafe {
        for idx in 0..MAX_OPEN_FILES {
            if FILE_HANDLES[idx].is_none() {
                let mut socket_index = usize::MAX;

                if domain == AF_NETLINK {
                    if let Some(res) = crate::net::with_net_stack(|stack| stack.netlink_socket()) {
                        match res {
                            Ok(i) => socket_index = i,
                            Err(_) => {
                                posix::set_errno(posix::errno::ENOMEM);
                                return u64::MAX;
                            }
                        }
                    } else {
                        posix::set_errno(posix::errno::ENETDOWN);
                        return u64::MAX;
                    }
                } else if socket_type == SOCK_STREAM {
                    if let Some(res) = crate::net::with_net_stack(|stack| stack.tcp_socket()) {
                        match res {
                            Ok(i) => socket_index = i,
                            Err(_) => {
                                posix::set_errno(posix::errno::ENOMEM);
                                return u64::MAX;
                            }
                        }
                    } else {
                        posix::set_errno(posix::errno::ENETDOWN);
                        return u64::MAX;
                    }
                }

                let socket_handle = SocketHandle {
                    socket_index,
                    domain,
                    socket_type,
                    protocol: if domain == AF_NETLINK {
                        0
                    } else {
                        actual_protocol
                    },
                    device_index: 0,
                    broadcast_enabled: false,
                    recv_timeout_ms: 0,
                };

                let metadata = crate::posix::Metadata::empty()
                    .with_type(crate::posix::FileType::Socket)
                    .with_uid(0)
                    .with_gid(0)
                    .with_mode(0o0600);

                let handle = FileHandle {
                    backing: FileBacking::Socket(socket_handle),
                    position: 0,
                    metadata,
                };

                FILE_HANDLES[idx] = Some(handle);
                let fd = FD_BASE + idx as u64;
                super::file::mark_fd_open(fd); // Track this FD as open for the current process
                kinfo!(
                    "[SYS_SOCKET] Created {} socket at fd {}",
                    if socket_type == SOCK_STREAM {
                        "TCP"
                    } else {
                        "UDP"
                    },
                    fd
                );
                posix::set_errno(0);
                return fd;
            }
        }
    }

    kwarn!("[SYS_SOCKET] No free file descriptors");
    posix::set_errno(posix::errno::EMFILE);
    u64::MAX
}

/// SYS_BIND - Bind socket to local address
pub fn bind(sockfd: u64, addr: *const SockAddr, addrlen: u32) -> u64 {
    kinfo!("[SYS_BIND] sockfd={} addrlen={}", sockfd, addrlen);

    if addr.is_null() || addrlen < 8 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_mut() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(ref mut sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        if sock_handle.domain == AF_NETLINK {
            let addr_ref = &*addr;
            if addr_ref.sa_family != AF_NETLINK as u16 {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }

            let pid = u32::from_ne_bytes([
                addr_ref.sa_data[2],
                addr_ref.sa_data[3],
                addr_ref.sa_data[4],
                addr_ref.sa_data[5],
            ]);
            let groups = u32::from_ne_bytes([
                addr_ref.sa_data[6],
                addr_ref.sa_data[7],
                addr_ref.sa_data[8],
                addr_ref.sa_data[9],
            ]);

            kinfo!(
                "[SYS_BIND] Netlink: pid={}, groups={}, addrlen={}",
                pid,
                groups,
                addrlen
            );

            if let Some(res) = crate::net::with_net_stack(|stack| {
                stack.netlink_bind(sock_handle.socket_index, pid, groups)
            }) {
                match res {
                    Ok(_) => {
                        kinfo!("[SYS_BIND] Netlink bind successful");
                        return 0;
                    }
                    Err(_) => {
                        posix::set_errno(posix::errno::EADDRINUSE);
                        return u64::MAX;
                    }
                }
            }
            posix::set_errno(posix::errno::ENETDOWN);
            return u64::MAX;
        }

        if sock_handle.domain != AF_INET
            || sock_handle.socket_type != SOCK_DGRAM
            || sock_handle.protocol != IPPROTO_UDP
        {
            posix::set_errno(posix::errno::ENOTSUP);
            return u64::MAX;
        }

        let addr_ref = &*addr;
        if addr_ref.sa_family != AF_INET as u16 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        let port = u16::from_be_bytes([addr_ref.sa_data[0], addr_ref.sa_data[1]]);
        let ip = [
            addr_ref.sa_data[2],
            addr_ref.sa_data[3],
            addr_ref.sa_data[4],
            addr_ref.sa_data[5],
        ];

        let actual_port = if port == 0 {
            let mut dynamic_port = 0;
            for candidate in 49152..65535 {
                if crate::net::with_net_stack(|stack| stack.is_udp_port_available(candidate))
                    == Some(true)
                {
                    dynamic_port = candidate;
                    break;
                }
            }

            if dynamic_port == 0 {
                kwarn!("[SYS_BIND] No free dynamic ports available");
                posix::set_errno(posix::errno::EADDRINUSE);
                return u64::MAX;
            }
            kinfo!("[SYS_BIND] Allocated dynamic port {}", dynamic_port);
            dynamic_port
        } else {
            port
        };

        if let Some(res) = crate::net::with_net_stack(|stack| stack.udp_socket(actual_port)) {
            match res {
                Ok(socket_idx) => {
                    sock_handle.socket_index = socket_idx;
                    kinfo!(
                        "[SYS_BIND] UDP socket fd {} bound to {}.{}.{}.{}:{} (socket_idx={})",
                        sockfd,
                        ip[0],
                        ip[1],
                        ip[2],
                        ip[3],
                        actual_port,
                        socket_idx
                    );
                    posix::set_errno(0);
                    return 0;
                }
                Err(_) => {
                    kwarn!("[SYS_BIND] Failed to allocate UDP socket (port in use or no slots)");
                    posix::set_errno(posix::errno::EADDRINUSE);
                    return u64::MAX;
                }
            }
        } else {
            kwarn!("[SYS_BIND] Network stack unavailable");
            posix::set_errno(posix::errno::ENETDOWN);
            return u64::MAX;
        }
    }
}

/// Update the kernel-maintained DNS server list
pub fn set_dns_servers(ptr: *const u32, count: u32) -> u64 {
    if count as usize > MAX_DNS_SERVERS {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if count > 0 {
        if ptr.is_null() {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        // Check pointer alignment (u32 requires 4-byte alignment)
        if (ptr as usize) % core::mem::align_of::<u32>() != 0 {
            kwarn!("[set_dns_servers] Pointer {:p} not aligned for u32", ptr);
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        let byte_len = (count as usize).saturating_mul(mem::size_of::<u32>());
        if !user_buffer_in_range(ptr as u64, byte_len as u64) {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        // Safe: we've verified pointer is non-null, aligned, and in valid user range
        let input = unsafe { slice::from_raw_parts(ptr, count as usize) };
        let mut entries = [[0u8; 4]; MAX_DNS_SERVERS];
        for idx in 0..(count as usize) {
            entries[idx] = input[idx].to_be_bytes();
        }

        let mut config = DNS_CONFIG.lock();
        config.update(&entries[..count as usize]);
    } else {
        let mut config = DNS_CONFIG.lock();
        config.update(&[]);
    }

    posix::set_errno(0);
    0
}

/// Retrieve DNS servers previously published via [`set_dns_servers`]
pub fn get_dns_servers(ptr: *mut u32, capacity: u32) -> u64 {
    let capacity = capacity.min(MAX_DNS_SERVERS as u32);
    let guard = DNS_CONFIG.lock();
    let available = guard.count.min(capacity as usize);

    if available > 0 {
        if ptr.is_null() {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        // Check pointer alignment (u32 requires 4-byte alignment)
        if (ptr as usize) % core::mem::align_of::<u32>() != 0 {
            kwarn!("[get_dns_servers] Pointer {:p} not aligned for u32", ptr);
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        let byte_len = (capacity as usize).saturating_mul(mem::size_of::<u32>());
        if !user_buffer_in_range(ptr as u64, byte_len as u64) {
            posix::set_errno(posix::errno::EFAULT);
            return u64::MAX;
        }

        // Safe: we've verified pointer is non-null, aligned, and in valid user range
        let output = unsafe { slice::from_raw_parts_mut(ptr, capacity as usize) };
        for idx in 0..available {
            output[idx] = u32::from_be_bytes(guard.servers[idx]);
        }
    }

    posix::set_errno(0);
    available as u64
}

/// SYS_SENDTO - Send UDP datagram to specified address
pub fn sendto(
    sockfd: u64,
    buf: *const u8,
    len: usize,
    _flags: i32,
    dest_addr: *const SockAddr,
    addrlen: u32,
) -> u64 {
    ktrace!(
        "[SYS_SENDTO] sockfd={} len={} addrlen={}",
        sockfd,
        len,
        addrlen
    );
    kinfo!(
        "[SYS_SENDTO] sockfd={} len={} addrlen={}",
        sockfd,
        len,
        addrlen
    );

    if buf.is_null() || len == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if !user_buffer_in_range(buf as u64, len as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_ref() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        ktrace!(
            "[SYS_SENDTO] Socket domain={}, type={}, index={}",
            sock_handle.domain,
            sock_handle.socket_type,
            sock_handle.socket_index
        );

        if sock_handle.domain == AF_NETLINK {
            let data = slice::from_raw_parts(buf, len);
            ktrace!(
                "[SYS_SENDTO] Netlink sendto: socket_idx={}, data_len={}",
                sock_handle.socket_index,
                len
            );
            if let Some(res) = crate::net::with_net_stack(|stack| {
                stack.netlink_send(sock_handle.socket_index, data)
            }) {
                match res {
                    Ok(_) => {
                        ktrace!("[SYS_SENDTO] Netlink sendto successful");
                        return len as u64;
                    }
                    Err(_e) => {
                        ktrace!("[SYS_SENDTO] Netlink sendto error");
                        posix::set_errno(posix::errno::EIO);
                        return u64::MAX;
                    }
                }
            }
            ktrace!("[SYS_SENDTO] Network stack unavailable");
            posix::set_errno(posix::errno::ENETDOWN);
            return u64::MAX;
        }

        if dest_addr.is_null() || addrlen < 8 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        if sock_handle.domain != AF_INET
            || sock_handle.socket_type != SOCK_DGRAM
            || sock_handle.protocol != IPPROTO_UDP
        {
            posix::set_errno(posix::errno::ENOTSUP);
            return u64::MAX;
        }

        let addr_ref = &*dest_addr;
        if addr_ref.sa_family != AF_INET as u16 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        let port = u16::from_be_bytes([addr_ref.sa_data[0], addr_ref.sa_data[1]]);
        let ip = [
            addr_ref.sa_data[2],
            addr_ref.sa_data[3],
            addr_ref.sa_data[4],
            addr_ref.sa_data[5],
        ];

        if ip.iter().all(|&b| b == 0) {
            kwarn!("[SYS_SENDTO] Invalid destination address: 0.0.0.0");
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        if port == 0 {
            kwarn!("[SYS_SENDTO] Invalid destination port: 0");
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        let is_broadcast = ip == [255, 255, 255, 255] || (ip[3] == 255 && ip[0] != 127);

        if is_broadcast {
            ktrace!(
                "[SYS_SENDTO] Broadcast address detected: {}.{}.{}.{}",
                ip[0],
                ip[1],
                ip[2],
                ip[3]
            );
            kinfo!(
                "[SYS_SENDTO] Broadcast address detected: {}.{}.{}.{}",
                ip[0],
                ip[1],
                ip[2],
                ip[3]
            );
            if !sock_handle.broadcast_enabled {
                ktrace!("[SYS_SENDTO] ERROR: Broadcast not permitted - SO_BROADCAST not set");
                kwarn!("[SYS_SENDTO] Broadcast not permitted: SO_BROADCAST not set on socket");
                posix::set_errno(posix::errno::EACCES);
                return u64::MAX;
            }
            ktrace!("[SYS_SENDTO] SO_BROADCAST enabled, allowing broadcast");
            kinfo!("[SYS_SENDTO] SO_BROADCAST enabled, allowing broadcast transmission");
        }

        ktrace!(
            "[SYS_SENDTO] Sending {} bytes to {}.{}.{}.{}:{}",
            len,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            port
        );
        kinfo!(
            "[SYS_SENDTO] Sending {} bytes to {}.{}.{}.{}:{}",
            len,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            port
        );

        let payload = slice::from_raw_parts(buf, len);

        ktrace!(
            "[SYS_SENDTO] About to send via network stack, device={}, socket={}, broadcast={}",
            sock_handle.device_index,
            sock_handle.socket_index,
            sock_handle.broadcast_enabled
        );
        kinfo!(
            "[SYS_SENDTO] About to send via network stack, device_index={}, socket_index={}",
            sock_handle.device_index,
            sock_handle.socket_index
        );
        kinfo!(
            "[SYS_SENDTO] Socket broadcast_enabled={}",
            sock_handle.broadcast_enabled
        );

        let (udp_result, tx) = if let Some(res) = crate::net::with_net_stack(|stack| {
            ktrace!("[SYS_SENDTO] Acquired network stack lock");
            kinfo!("[SYS_SENDTO] Acquired network stack lock");
            let mut tx = Box::new(crate::net::stack::TxBatch::new());
            let result = stack.udp_send(
                sock_handle.device_index,
                sock_handle.socket_index,
                ip,
                port,
                payload,
                &mut tx,
            );
            ktrace!("[SYS_SENDTO] udp_send returned: {:?}", result);
            kinfo!("[SYS_SENDTO] udp_send returned: {:?}", result);
            (result, tx)
        }) {
            res
        } else {
            ktrace!("[SYS_SENDTO] ERROR: Network stack unavailable");
            kwarn!("[SYS_SENDTO] Network stack unavailable");
            posix::set_errno(posix::errno::ENETDOWN);
            return u64::MAX;
        };

        if !tx.is_empty() {
            if let Err(e) = crate::net::send_frames(sock_handle.device_index, &tx) {
                ktrace!("[SYS_SENDTO] ERROR: Failed to transmit frames: {:?}", e);
                kwarn!("[SYS_SENDTO] Failed to transmit frames: {:?}", e);
            } else {
                kinfo!(
                    "[SYS_SENDTO] Transmitted {} frame(s) from tx batch",
                    tx.len()
                );
            }
        }

        match udp_result {
            Ok(_) => {
                ktrace!("[SYS_SENDTO] SUCCESS: Sent {} bytes", len);
                kinfo!("[SYS_SENDTO] Successfully sent {} bytes", len);
                posix::set_errno(0);
                let result = len as u64;
                ktrace!("[SYS_SENDTO] Returning {} to userspace", result);
                result
            }
            Err(ref e) => {
                let is_arp_miss = matches!(e, crate::net::NetError::ArpCacheMiss);

                if is_arp_miss {
                    ktrace!("[SYS_SENDTO] Packet queued for ARP, returning success");
                    kinfo!("[SYS_SENDTO] Packet queued for ARP resolution");
                    posix::set_errno(0);
                    len as u64
                } else {
                    ktrace!("[SYS_SENDTO] ERROR: Failed to prepare packet: {:?}", e);
                    kwarn!("[SYS_SENDTO] Failed to prepare packet: {:?}", e);
                    posix::set_errno(posix::errno::EIO);
                    u64::MAX
                }
            }
        }
    }
}

/// SYS_RECVFROM - Receive UDP datagram and source address
pub fn recvfrom(
    sockfd: u64,
    buf: *mut u8,
    len: usize,
    _flags: i32,
    _src_addr: *mut SockAddr,
    _addrlen: *mut u32,
) -> u64 {
    kinfo!("[SYS_RECVFROM] ENTRY: sockfd={} len={}", sockfd, len);
    ktrace!("[SYS_RECVFROM] ENTRY: sockfd={} len={}", sockfd, len);

    crate::net::poll();

    if buf.is_null() || len == 0 {
        ktrace!("[SYS_RECVFROM] Invalid buf or len");
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if !user_buffer_in_range(buf as u64, len as u64) {
        ktrace!("[SYS_RECVFROM] Buffer out of range");
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_ref() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        if sock_handle.domain == AF_NETLINK {
            ktrace!(
                "[SYS_RECVFROM] Netlink receive: socket_idx={}",
                sock_handle.socket_index
            );
            let buffer = slice::from_raw_parts_mut(buf, len);
            if let Some(res) = crate::net::with_net_stack(|stack| {
                stack.netlink_receive(sock_handle.socket_index, buffer)
            }) {
                match res {
                    Ok(n) => {
                        ktrace!("[SYS_RECVFROM] Netlink received {} bytes", n);
                        return n as u64;
                    }
                    Err(e) => {
                        ktrace!("[SYS_RECVFROM] Netlink receive error: {:?}", e);
                        posix::set_errno(posix::errno::EAGAIN);
                        return u64::MAX;
                    }
                }
            }
            ktrace!("[SYS_RECVFROM] Network stack unavailable");
            posix::set_errno(posix::errno::ENETDOWN);
            return u64::MAX;
        }

        if sock_handle.domain != AF_INET
            || sock_handle.socket_type != SOCK_DGRAM
            || sock_handle.protocol != IPPROTO_UDP
        {
            kinfo!("[SYS_RECVFROM] not UDP: domain={} type={} proto={}", 
                sock_handle.domain, sock_handle.socket_type, sock_handle.protocol);
            posix::set_errno(posix::errno::ENOTSUP);
            return u64::MAX;
        }
        
        kinfo!("[SYS_RECVFROM] UDP recv loop starting");

        let timeout_ms = sock_handle.recv_timeout_ms;
        let start_tick = crate::scheduler::get_tick();

        static mut RECVFROM_LOOP_COUNT: u64 = 0;
        loop {
            unsafe {
                RECVFROM_LOOP_COUNT += 1;
                let cnt = RECVFROM_LOOP_COUNT;
                if cnt % 100 == 1 {
                    kinfo!("[SYS_RECVFROM] loop #{}", cnt);
                }
            }
            crate::net::poll();

            let buffer = slice::from_raw_parts_mut(buf, len);
            if let Some(res) = crate::net::with_net_stack(|stack| {
                stack.udp_receive(sock_handle.socket_index, buffer)
            }) {
                match res {
                    Ok(result) => {
                        ktrace!(
                            "[SYS_RECVFROM] SUCCESS: Received {} bytes from {}.{}.{}.{}:{}",
                            result.bytes_copied,
                            result.src_ip[0],
                            result.src_ip[1],
                            result.src_ip[2],
                            result.src_ip[3],
                            result.src_port
                        );

                        if !_src_addr.is_null() && !_addrlen.is_null() {
                            let src_addr = &mut *_src_addr;
                            src_addr.sa_family = AF_INET as u16;
                            src_addr.sa_data[0..2].copy_from_slice(&result.src_port.to_be_bytes());
                            src_addr.sa_data[2..6].copy_from_slice(&result.src_ip);
                            *_addrlen = 16;
                        }

                        posix::set_errno(0);
                        return result.bytes_copied as u64;
                    }
                    Err(_err) => {
                        if timeout_ms > 0 {
                            let elapsed_ms = crate::scheduler::get_tick() - start_tick;
                            if elapsed_ms >= timeout_ms {
                                ktrace!("[SYS_RECVFROM] TIMEOUT after {}ms", elapsed_ms);
                                posix::set_errno(posix::errno::EAGAIN);
                                return u64::MAX;
                            }
                        }
                        for _ in 0..1000 {
                            core::hint::spin_loop();
                        }
                    }
                }
            } else {
                ktrace!("[SYS_RECVFROM] ERROR: Network stack unavailable");
                posix::set_errno(posix::errno::ENETDOWN);
                return u64::MAX;
            }
        }
    }
}

/// SYS_CONNECT - Connect socket to remote address
pub fn connect(sockfd: u64, addr: *const SockAddr, addrlen: u32) -> u64 {
    ktrace!(
        "[SYS_CONNECT] ==== ENTRY ==== sockfd={} addrlen={}",
        sockfd,
        addrlen
    );
    kinfo!("[SYS_CONNECT] sockfd={} addrlen={}", sockfd, addrlen);

    if addr.is_null() || addrlen < 8 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_ref() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        if sock_handle.domain != AF_INET {
            posix::set_errno(posix::errno::ENOTSUP);
            return u64::MAX;
        }

        let addr_ref = &*addr;
        if addr_ref.sa_family != AF_INET as u16 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        let port = u16::from_be_bytes([addr_ref.sa_data[0], addr_ref.sa_data[1]]);
        let ip = [
            addr_ref.sa_data[2],
            addr_ref.sa_data[3],
            addr_ref.sa_data[4],
            addr_ref.sa_data[5],
        ];

        if ip.iter().all(|&b| b == 0) {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        if port == 0 {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }

        kinfo!(
            "[SYS_CONNECT] {} socket fd {} connecting to {}.{}.{}.{}:{}",
            if sock_handle.socket_type == SOCK_STREAM {
                "TCP"
            } else {
                "UDP"
            },
            sockfd,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            port
        );

        if sock_handle.socket_type == SOCK_STREAM {
            if sock_handle.socket_index == usize::MAX {
                posix::set_errno(posix::errno::EINVAL);
                return u64::MAX;
            }

            let local_port = 49152 + (sock_handle.socket_index as u16 * 123) % 16384;

            // Retry loop for ARP resolution
            let arp_start_time_us = crate::logger::boot_time_us();
            let arp_timeout_us = 5_000_000u64; // 5 second timeout for ARP
            let mut arp_retry_count = 0;
            const MAX_ARP_RETRIES: u32 = 10;

            let tcp_connect_result = loop {
                let mut tx_batch = crate::net::stack::TxBatch::new();
                let result = crate::net::with_net_stack(|stack| {
                    stack.tcp_connect(
                        sock_handle.socket_index,
                        sock_handle.device_index,
                        ip,
                        port,
                        local_port,
                        &mut tx_batch,
                    )
                });

                // Send any pending frames (including ARP requests)
                if tx_batch.len() > 0 {
                    crate::net::send_frames(sock_handle.device_index, &tx_batch).ok();
                }

                ktrace!("[SYS_CONNECT] tcp_connect returned: {:?}", result);

                match result {
                    Some(Err(crate::net::NetError::ArpCacheMiss)) => {
                        arp_retry_count += 1;
                        let elapsed_us = crate::logger::boot_time_us() - arp_start_time_us;

                        if elapsed_us > arp_timeout_us || arp_retry_count > MAX_ARP_RETRIES {
                            kwarn!(
                                "[SYS_CONNECT] ARP resolution timeout after {} retries",
                                arp_retry_count
                            );
                            break Some(Err(crate::net::NetError::ArpCacheMiss));
                        }

                        ktrace!(
                            "[SYS_CONNECT] ARP cache miss, waiting for resolution (retry {})",
                            arp_retry_count
                        );

                        // Poll network to receive ARP reply
                        for _ in 0..50 {
                            crate::net::poll();
                            // Small delay - poll a few times to give ARP time to resolve
                        }

                        // Yield to scheduler briefly
                        scheduler::set_current_process_state(ProcessState::Sleeping);
                        continue;
                    }
                    _ => break result,
                }
            };

            match tcp_connect_result {
                Some(Ok(())) => {
                    ktrace!("[SYS_CONNECT] TCP connection initiated, waiting for establishment...");
                    kinfo!("[SYS_CONNECT] TCP connection initiated, waiting for establishment...");

                    let start_time_us = crate::logger::boot_time_us();
                    let timeout_us = 30_000_000u64;

                    loop {
                        crate::net::poll();

                        let state = crate::net::with_net_stack(|stack| {
                            stack.tcp_get_state(sock_handle.socket_index)
                        });

                        match state {
                            Some(Ok(crate::net::tcp::TcpState::Established)) => {
                                kinfo!("[SYS_CONNECT] TCP connection established");
                                posix::set_errno(0);
                                return 0;
                            }
                            Some(Ok(crate::net::tcp::TcpState::Closed)) => {
                                kwarn!("[SYS_CONNECT] TCP connection failed (closed)");
                                posix::set_errno(posix::errno::ECONNREFUSED);
                                return u64::MAX;
                            }
                            Some(Ok(_)) => {
                                let elapsed_us = crate::logger::boot_time_us() - start_time_us;
                                if elapsed_us > timeout_us {
                                    kwarn!("[SYS_CONNECT] TCP connection timeout");
                                    posix::set_errno(posix::errno::ETIMEDOUT);
                                    return u64::MAX;
                                }
                                scheduler::set_current_process_state(ProcessState::Sleeping);
                                continue;
                            }
                            Some(Err(_)) | None => {
                                kwarn!("[SYS_CONNECT] TCP connection failed (error)");
                                posix::set_errno(posix::errno::ECONNREFUSED);
                                return u64::MAX;
                            }
                        }
                    }
                }
                Some(Err(crate::net::NetError::ArpCacheMiss)) => {
                    kwarn!("[SYS_CONNECT] TCP connect failed: ARP resolution failed");
                    posix::set_errno(posix::errno::ENETUNREACH);
                    u64::MAX
                }
                Some(Err(_)) => {
                    kwarn!("[SYS_CONNECT] TCP connect failed");
                    posix::set_errno(posix::errno::ECONNREFUSED);
                    u64::MAX
                }
                None => {
                    posix::set_errno(posix::errno::ENETDOWN);
                    u64::MAX
                }
            }
        } else {
            posix::set_errno(0);
            0
        }
    }
}

/// SYS_SETSOCKOPT - Set socket options
pub fn setsockopt(sockfd: u64, level: i32, optname: i32, optval: *const u8, optlen: u32) -> u64 {
    ktrace!(
        "[SYS_SETSOCKOPT] sockfd={} level={} optname={} optlen={}",
        sockfd,
        level,
        optname,
        optlen
    );
    kinfo!(
        "[SYS_SETSOCKOPT] sockfd={} level={} optname={} optlen={}",
        sockfd,
        level,
        optname,
        optlen
    );

    if optval.is_null() || optlen == 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if !user_buffer_in_range(optval as u64, optlen as u64) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let idx = if sockfd >= FD_BASE {
        (sockfd - FD_BASE) as usize
    } else {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    };

    if idx >= MAX_OPEN_FILES {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    unsafe {
        let Some(handle) = FILE_HANDLES[idx].as_mut() else {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        };

        let FileBacking::Socket(ref mut sock_handle) = handle.backing else {
            posix::set_errno(posix::errno::ENOTSOCK);
            return u64::MAX;
        };

        if level == SOL_SOCKET {
            match optname {
                SO_BROADCAST => {
                    if optlen >= 4 {
                        let value = *(optval as *const i32);
                        sock_handle.broadcast_enabled = value != 0;
                        ktrace!(
                            "[SYS_SETSOCKOPT] SO_BROADCAST set to {} for sockfd {}",
                            sock_handle.broadcast_enabled,
                            sockfd
                        );
                        kinfo!(
                            "[SYS_SETSOCKOPT] SO_BROADCAST set to {}",
                            sock_handle.broadcast_enabled
                        );
                        posix::set_errno(0);
                        return 0;
                    } else {
                        posix::set_errno(posix::errno::EINVAL);
                        return u64::MAX;
                    }
                }
                SO_REUSEADDR => {
                    kinfo!("[SYS_SETSOCKOPT] SO_REUSEADDR accepted (ignored)");
                    posix::set_errno(0);
                    return 0;
                }
                SO_RCVTIMEO | SO_SNDTIMEO => {
                    if optlen >= 16 {
                        let tv_sec = *(optval as *const i64);
                        let tv_usec = *((optval as usize + 8) as *const i64);

                        if optname == SO_RCVTIMEO {
                            let timeout_ms = (tv_sec as u64) * 1000 + (tv_usec as u64) / 1000;
                            sock_handle.recv_timeout_ms = timeout_ms;
                            kinfo!(
                                "[SYS_SETSOCKOPT] SO_RCVTIMEO set to {}ms ({}s + {}us)",
                                timeout_ms,
                                tv_sec,
                                tv_usec
                            );
                        } else {
                            kinfo!("[SYS_SETSOCKOPT] SO_SNDTIMEO accepted (ignored)");
                        }
                        posix::set_errno(0);
                        return 0;
                    } else {
                        posix::set_errno(posix::errno::EINVAL);
                        return u64::MAX;
                    }
                }
                _ => {
                    kwarn!("[SYS_SETSOCKOPT] Unsupported socket option: {}", optname);
                    posix::set_errno(posix::errno::EINVAL);
                    return u64::MAX;
                }
            }
        }

        kwarn!("[SYS_SETSOCKOPT] Unsupported level: {}", level);
        posix::set_errno(posix::errno::EINVAL);
        u64::MAX
    }
}

/// SYS_SOCKETPAIR - Create a pair of connected sockets
pub fn socketpair(domain: i32, socket_type: i32, protocol: i32, sv: *mut [i32; 2]) -> u64 {
    kinfo!(
        "[SYS_SOCKETPAIR] domain={} type={} protocol={}",
        domain,
        socket_type,
        protocol
    );

    // Validate sv pointer
    if sv.is_null() {
        kwarn!("[SYS_SOCKETPAIR] NULL sv pointer");
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    // Only AF_UNIX (AF_LOCAL) is supported for socketpair
    if domain != AF_UNIX {
        kwarn!(
            "[SYS_SOCKETPAIR] Unsupported domain: {} (only AF_UNIX=1 supported)",
            domain
        );
        posix::set_errno(posix::errno::EAFNOSUPPORT);
        return u64::MAX;
    }

    // Support SOCK_STREAM and SOCK_DGRAM for AF_UNIX
    if socket_type != SOCK_STREAM && socket_type != SOCK_DGRAM {
        kwarn!(
            "[SYS_SOCKETPAIR] Unsupported socket type: {} (only SOCK_STREAM/SOCK_DGRAM supported)",
            socket_type
        );
        posix::set_errno(posix::errno::ENOSYS);
        return u64::MAX;
    }

    // Protocol must be 0 for AF_UNIX
    if protocol != 0 {
        kwarn!(
            "[SYS_SOCKETPAIR] Unsupported protocol: {} (must be 0 for AF_UNIX)",
            protocol
        );
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    // Create the socketpair in the IPC subsystem
    let pair_id = match crate::ipc::create_socketpair() {
        Ok(id) => id,
        Err(_) => {
            kwarn!("[SYS_SOCKETPAIR] Failed to create socketpair - too many open");
            posix::set_errno(posix::errno::EMFILE);
            return u64::MAX;
        }
    };

    // Allocate two file descriptors
    let mut fd0: u64 = 0;
    let mut fd1: u64 = 0;

    unsafe {
        // Find first free slot for socket 0
        let mut idx0 = usize::MAX;
        for idx in 0..MAX_OPEN_FILES {
            if FILE_HANDLES[idx].is_none() {
                idx0 = idx;
                break;
            }
        }

        if idx0 == usize::MAX {
            // No free slot - close the socketpair and return error
            let _ = crate::ipc::close_socketpair_end(pair_id, 0);
            let _ = crate::ipc::close_socketpair_end(pair_id, 1);
            kwarn!("[SYS_SOCKETPAIR] No free file descriptors for socket 0");
            posix::set_errno(posix::errno::EMFILE);
            return u64::MAX;
        }

        // Find second free slot for socket 1
        let mut idx1 = usize::MAX;
        for idx in 0..MAX_OPEN_FILES {
            if idx != idx0 && FILE_HANDLES[idx].is_none() {
                idx1 = idx;
                break;
            }
        }

        if idx1 == usize::MAX {
            // No free slot - close the socketpair and return error
            let _ = crate::ipc::close_socketpair_end(pair_id, 0);
            let _ = crate::ipc::close_socketpair_end(pair_id, 1);
            kwarn!("[SYS_SOCKETPAIR] No free file descriptors for socket 1");
            posix::set_errno(posix::errno::EMFILE);
            return u64::MAX;
        }

        // Create file handles for both ends
        let metadata = crate::posix::Metadata::empty()
            .with_type(crate::posix::FileType::Socket)
            .with_uid(0)
            .with_gid(0)
            .with_mode(0o0600);

        let handle0 = FileHandle {
            backing: FileBacking::Socketpair(SocketpairHandle {
                pair_id,
                end: 0,
                socket_type,
            }),
            position: 0,
            metadata,
        };

        let handle1 = FileHandle {
            backing: FileBacking::Socketpair(SocketpairHandle {
                pair_id,
                end: 1,
                socket_type,
            }),
            position: 0,
            metadata,
        };

        FILE_HANDLES[idx0] = Some(handle0);
        FILE_HANDLES[idx1] = Some(handle1);

        fd0 = FD_BASE + idx0 as u64;
        fd1 = FD_BASE + idx1 as u64;

        // Track these FDs as open for the current process
        super::file::mark_fd_open(fd0);
        super::file::mark_fd_open(fd1);

        // Write file descriptors to user buffer
        (*sv)[0] = fd0 as i32;
        (*sv)[1] = fd1 as i32;
    }

    kinfo!(
        "[SYS_SOCKETPAIR] Created socketpair: sv[0]={}, sv[1]={}",
        fd0,
        fd1
    );
    posix::set_errno(0);
    0
}
