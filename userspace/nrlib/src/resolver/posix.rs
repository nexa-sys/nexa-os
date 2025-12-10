/// POSIX-compatible resolver functions
///
/// Implements getaddrinfo, freeaddrinfo, and getnameinfo.
use core::mem;

use crate::socket::{parse_ipv4, SockAddr, SockAddrIn, AF_INET, SOCK_STREAM};
use crate::{c_void, free, malloc};

use super::constants::*;
use super::global::get_resolver;
use super::types::AddrInfo;
use super::utils::format_ipv4_to_buffer;

/// Musl-compatible getaddrinfo implementation
///
/// Parameters match POSIX getaddrinfo signature
#[no_mangle]
pub extern "C" fn getaddrinfo(
    node: *const u8,
    service: *const u8,
    hints: *const AddrInfo,
    res: *mut *mut AddrInfo,
) -> i32 {
    if res.is_null() {
        return EAI_FAIL;
    }

    if node.is_null() {
        return EAI_NONAME;
    }

    unsafe {
        // Read null-terminated hostname string
        let mut hostname_buf = [0u8; 256];
        let mut len = 0;
        while len < 255 {
            let byte = *node.add(len);
            if byte == 0 {
                break;
            }
            hostname_buf[len] = byte;
            len += 1;
        }
        hostname_buf[len] = 0;

        let hostname = match core::str::from_utf8(&hostname_buf[..len]) {
            Ok(s) => s,
            Err(_) => {
                return EAI_NONAME;
            }
        };

        // Parse service (port number) if provided
        let mut port: u16 = 0;
        if !service.is_null() {
            let mut service_buf = [0u8; 16];
            let mut slen = 0;
            while slen < 15 {
                let byte = *service.add(slen);
                if byte == 0 {
                    break;
                }
                service_buf[slen] = byte;
                slen += 1;
            }

            if slen > 0 {
                if let Ok(service_str) = core::str::from_utf8(&service_buf[..slen]) {
                    // Try to parse as numeric port
                    if let Ok(p) = service_str.parse::<u16>() {
                        port = p;
                    } else {
                        // TODO: Look up service name in /etc/services
                        // For now, common services:
                        port = match service_str {
                            "http" => 80,
                            "https" => 443,
                            "ftp" => 21,
                            "ssh" => 22,
                            "telnet" => 23,
                            "smtp" => 25,
                            "dns" => 53,
                            _ => return EAI_SERVICE,
                        };
                    }
                }
            }
        }

        // First, try to parse hostname as a numeric IP address
        // This avoids unnecessary DNS queries for addresses like "192.168.1.1"
        let ip = if let Some(numeric_ip) = parse_ipv4(hostname) {
            numeric_ip
        } else {
            // Get resolver and ensure it's initialized
            let resolver = match get_resolver() {
                Some(r) => r,
                None => {
                    return EAI_FAIL;
                }
            };

            // Try to resolve hostname via DNS
            match resolver.resolve(hostname) {
                Some(ip) => ip,
                None => {
                    return EAI_NONAME;
                }
            }
        };

        // Determine socket type and protocol from hints
        let (socktype, protocol) = if !hints.is_null() {
            let hints_ref = &*hints;
            let st = if hints_ref.ai_socktype != 0 {
                hints_ref.ai_socktype
            } else {
                SOCK_STREAM // Default to TCP
            };
            let proto = if hints_ref.ai_protocol != 0 {
                hints_ref.ai_protocol
            } else if st == SOCK_STREAM {
                6 // IPPROTO_TCP
            } else {
                17 // IPPROTO_UDP
            };
            (st, proto)
        } else {
            (SOCK_STREAM, 6) // Default to TCP
        };

        // Allocate AddrInfo structure manually (malloc)
        let addrinfo_ptr = malloc(mem::size_of::<AddrInfo>()) as *mut AddrInfo;
        if addrinfo_ptr.is_null() {
            return EAI_FAIL;
        }

        // Allocate SockAddrIn
        let addr_ptr = malloc(mem::size_of::<SockAddrIn>()) as *mut SockAddrIn;
        if addr_ptr.is_null() {
            free(addrinfo_ptr as *mut c_void);
            return EAI_FAIL;
        }

        // Initialize SockAddrIn with resolved IP and port
        *addr_ptr = SockAddrIn::new(ip, port);

        // Initialize AddrInfo
        let addrinfo = &mut *addrinfo_ptr;
        addrinfo.ai_flags = 0;
        addrinfo.ai_family = AF_INET;
        addrinfo.ai_socktype = socktype;
        addrinfo.ai_protocol = protocol;
        addrinfo.ai_addrlen = mem::size_of::<SockAddrIn>() as u32;
        addrinfo.ai_addr = addr_ptr;
        addrinfo.ai_canonname = core::ptr::null_mut();
        addrinfo.ai_next = core::ptr::null_mut();

        *res = addrinfo_ptr;
    }

    0
}

/// Musl-compatible freeaddrinfo implementation
#[no_mangle]
pub extern "C" fn freeaddrinfo(res: *mut AddrInfo) {
    if !res.is_null() {
        unsafe {
            let addrinfo = &mut *res;
            if !addrinfo.ai_addr.is_null() {
                free(addrinfo.ai_addr as *mut c_void);
            }
            if !addrinfo.ai_canonname.is_null() {
                free(addrinfo.ai_canonname as *mut c_void);
            }
            if !addrinfo.ai_next.is_null() {
                freeaddrinfo(addrinfo.ai_next);
            }
            free(res as *mut c_void);
        }
    }
}

/// Musl-compatible getnameinfo implementation (reverse DNS lookup)
#[no_mangle]
pub extern "C" fn getnameinfo(
    addr: *const SockAddr,
    addrlen: u32,
    host: *mut u8,
    hostlen: u32,
    serv: *mut u8,
    servlen: u32,
    flags: i32,
) -> i32 {
    let _ = addrlen; // Unused but part of POSIX signature

    if addr.is_null() || host.is_null() || hostlen == 0 {
        return EAI_FAIL;
    }

    unsafe {
        let sockaddr = &*addr;

        // Only support AF_INET for now
        if sockaddr.sa_family != AF_INET as u16 {
            return EAI_FAMILY;
        }

        // Extract IP address from sa_data (port at [0:2], IP at [2:6])
        let ip = [
            sockaddr.sa_data[2],
            sockaddr.sa_data[3],
            sockaddr.sa_data[4],
            sockaddr.sa_data[5],
        ];

        // Handle NI_NUMERICHOST flag
        if (flags & NI_NUMERICHOST) != 0 {
            // Convert IP to string format using helper function
            let len = format_ipv4_to_buffer(ip, host, hostlen as usize);
            if len == 0 {
                return EAI_OVERFLOW;
            }
        } else {
            // Try reverse DNS lookup
            let resolver = match get_resolver() {
                Some(r) => r,
                None => return EAI_FAIL,
            };

            if let Some(hostname) = resolver.reverse_lookup_hosts(ip) {
                let hostname_bytes = hostname.as_bytes();
                if hostname_bytes.len() + 1 > hostlen as usize {
                    return EAI_OVERFLOW;
                }
                core::ptr::copy_nonoverlapping(hostname_bytes.as_ptr(), host, hostname_bytes.len());
                *host.add(hostname_bytes.len()) = 0;
            } else {
                // NI_NAMEREQD requires a name to be found
                if (flags & NI_NAMEREQD) != 0 {
                    return EAI_NONAME;
                }
                // No reverse entry found, use numeric form
                let len = format_ipv4_to_buffer(ip, host, hostlen as usize);
                if len == 0 {
                    return EAI_OVERFLOW;
                }
            }
        }

        // Handle service (port) if requested
        if !serv.is_null() && servlen > 0 {
            // Extract port from sa_data[0:2]
            let port = u16::from_be_bytes([sockaddr.sa_data[0], sockaddr.sa_data[1]]);
            let mut port_buf = [0u8; 6];
            let mut port_pos = 0;

            let mut p = port as usize;
            let mut digits = [0u8; 5];
            let mut digit_count = 0;

            if p == 0 {
                digits[0] = b'0';
                digit_count = 1;
            } else {
                while p > 0 && digit_count < 5 {
                    digits[digit_count] = (p % 10) as u8 + b'0';
                    p /= 10;
                    digit_count += 1;
                }
            }
            // Write digits in correct order
            for j in 0..digit_count {
                port_buf[port_pos] = digits[digit_count - 1 - j];
                port_pos += 1;
            }

            if port_pos + 1 > servlen as usize {
                return EAI_OVERFLOW;
            }
            port_buf[port_pos] = 0;
            core::ptr::copy_nonoverlapping(port_buf.as_ptr(), serv, port_pos + 1);
        }
    }

    0
}
