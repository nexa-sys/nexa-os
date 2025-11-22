use std::env;
use std::process;

// Constants
const AF_NETLINK: i32 = 16;
const SOCK_DGRAM: i32 = 2;

// Netlink constants
const RTM_GETLINK: u16 = 18;
const RTM_NEWLINK: u16 = 16;
const RTM_GETADDR: u16 = 22;
const RTM_NEWADDR: u16 = 20;
const IFLA_ADDRESS: u16 = 1;
const IFLA_IFNAME: u16 = 3;
const IFA_ADDRESS: u16 = 1;
const IFA_LOCAL: u16 = 2;
const IFA_LABEL: u16 = 3;

#[repr(C)]
struct SockAddrNl {
    nl_family: u16,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

#[repr(C)]
struct NlMsgHdr {
    nlmsg_len: u32,
    nlmsg_type: u16,
    nlmsg_flags: u16,
    nlmsg_seq: u32,
    nlmsg_pid: u32,
}

#[repr(C)]
struct IfInfoMsg {
    ifi_family: u8,
    __pad: u8,
    ifi_type: u16,
    ifi_index: u32,
    ifi_flags: u32,
    ifi_change: u32,
}

#[repr(C)]
struct IfAddrMsg {
    ifa_family: u8,
    ifa_prefixlen: u8,
    ifa_flags: u8,
    ifa_scope: u8,
    ifa_index: u32,
}

#[repr(C)]
struct RtAttr {
    rta_len: u16,
    rta_type: u16,
}

extern "C" {
    fn socket(domain: i32, type_: i32, protocol: i32) -> i32;
    fn bind(sockfd: i32, addr: *const std::ffi::c_void, addrlen: u32) -> i32;
    fn sendto(sockfd: i32, buf: *const std::ffi::c_void, len: usize, flags: i32, dest_addr: *const std::ffi::c_void, addrlen: u32) -> isize;
    fn recvfrom(sockfd: i32, buf: *mut std::ffi::c_void, len: usize, flags: i32, src_addr: *mut std::ffi::c_void, addrlen: *mut u32) -> isize;
    fn close(fd: i32) -> i32;
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: ip [link|addr]");
        return;
    }

    let cmd = &args[1];
    match cmd.as_str() {
        "link" => do_link(),
        "addr" => do_addr(),
        _ => println!("Unknown command: {}", cmd),
    }
}

fn do_link() {
    // Create socket
    let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM, 0) };
    if fd < 0 {
        println!("Failed to create socket");
        return;
    }

    // Bind
    let addr = SockAddrNl {
        nl_family: AF_NETLINK as u16,
        nl_pad: 0,
        nl_pid: process::id(),
        nl_groups: 0,
    };
    
    if unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 12) } < 0 {
        println!("Failed to bind socket");
        unsafe { close(fd) };
        return;
    }

    // Send RTM_GETLINK
    let req = NlMsgHdr {
        nlmsg_len: 16,
        nlmsg_type: RTM_GETLINK,
        nlmsg_flags: 1, // NLM_F_REQUEST
        nlmsg_seq: 1,
        nlmsg_pid: process::id(),
    };

    if unsafe { sendto(fd, &req as *const _ as *const std::ffi::c_void, 16, 0, std::ptr::null(), 0) } < 0 {
        println!("Failed to send request");
        unsafe { close(fd) };
        return;
    }

    // Receive response
    let mut buf = [0u8; 4096];
    let len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 4096, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
    
    if len < 0 {
        println!("Failed to receive response");
    } else {
        println!("Received {} bytes", len);
        // Parse response (TODO)
    }

    unsafe { close(fd) };
}

fn do_addr() {
    println!("Network interfaces and addresses:\n");
    
    // Create socket
    let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM, 0) };
    if fd < 0 {
        println!("Failed to create netlink socket");
        return;
    }

    // Bind
    let addr = SockAddrNl {
        nl_family: AF_NETLINK as u16,
        nl_pad: 0,
        nl_pid: process::id(),
        nl_groups: 0,
    };
    
    if unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 12) } < 0 {
        println!("Failed to bind netlink socket");
        unsafe { close(fd) };
        return;
    }

    // First, get link information (interface names and MACs)
    let req = NlMsgHdr {
        nlmsg_len: 16,
        nlmsg_type: RTM_GETLINK,
        nlmsg_flags: 1, // NLM_F_REQUEST
        nlmsg_seq: 1,
        nlmsg_pid: process::id(),
    };

    if unsafe { sendto(fd, &req as *const _ as *const std::ffi::c_void, 16, 0, std::ptr::null(), 0) } < 0 {
        println!("Failed to send RTM_GETLINK request");
        unsafe { close(fd) };
        return;
    }

    // Receive all link info messages until DONE
    let mut buf = [0u8; 4096];
    loop {
        let len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 4096, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
        
        if len < 0 {
            println!("Failed to receive link info");
            unsafe { close(fd) };
            return;
        }

        if len >= 16 {
            let hdr = unsafe { &*(buf.as_ptr() as *const NlMsgHdr) };
            if hdr.nlmsg_type == 3 { // NLMSG_DONE
                break;
            }
            // Parse this link info message
            parse_link_info(&buf[..len as usize]);
        }
    }

    // Now get address information
    let req = NlMsgHdr {
        nlmsg_len: 16,
        nlmsg_type: RTM_GETADDR,
        nlmsg_flags: 1, // NLM_F_REQUEST
        nlmsg_seq: 2,
        nlmsg_pid: process::id(),
    };

    if unsafe { sendto(fd, &req as *const _ as *const std::ffi::c_void, 16, 0, std::ptr::null(), 0) } < 0 {
        println!("Failed to send RTM_GETADDR request");
        unsafe { close(fd) };
        return;
    }

    // Receive all address info messages until DONE
    buf.fill(0);
    loop {
        let len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 4096, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
        
        if len < 0 {
            println!("Failed to receive address info (no response)");
            break;
        } else if len == 0 {
            println!("No address information available");
            break;
        } else if len >= 16 {
            let hdr = unsafe { &*(buf.as_ptr() as *const NlMsgHdr) };
            if hdr.nlmsg_type == 3 { // NLMSG_DONE
                break;
            }
            // Parse this address info message
            parse_addr_info(&buf[..len as usize]);
        }
    }

    unsafe { close(fd) };
}

fn parse_link_info(data: &[u8]) {
    let mut offset = 0;
    
    while offset + 16 <= data.len() {
        let hdr = unsafe { &*(data.as_ptr().add(offset) as *const NlMsgHdr) };
        
        if hdr.nlmsg_type == 3 { // NLMSG_DONE
            break;
        }
        if hdr.nlmsg_type == 2 { // NLMSG_ERROR
            println!("Netlink error in link info");
            break;
        }
        
        if hdr.nlmsg_type == RTM_NEWLINK {
            let ifinfo_offset = offset + 16;
            if ifinfo_offset + 16 <= data.len() {
                let ifinfo = unsafe { &*(data.as_ptr().add(ifinfo_offset) as *const IfInfoMsg) };
                
                print!("{}: ", ifinfo.ifi_index);
                
                // Parse attributes
                let mut attr_offset = ifinfo_offset + 16;
                let mut if_name = String::from("unknown");
                let mut mac_addr = String::new();
                
                while attr_offset + 4 <= offset + hdr.nlmsg_len as usize {
                    let attr = unsafe { &*(data.as_ptr().add(attr_offset) as *const RtAttr) };
                    if attr.rta_len < 4 { break; }
                    
                    let data_offset = attr_offset + 4;
                    let data_len = (attr.rta_len as usize).saturating_sub(4);
                    
                    if data_offset + data_len <= data.len() {
                        if attr.rta_type == IFLA_IFNAME {
                            let name_bytes = &data[data_offset..data_offset + data_len];
                            if let Some(end) = name_bytes.iter().position(|&b| b == 0) {
                                if let Ok(name) = std::str::from_utf8(&name_bytes[..end]) {
                                    if_name = name.to_string();
                                }
                            }
                        } else if attr.rta_type == IFLA_ADDRESS && data_len >= 6 {
                            let mac = &data[data_offset..data_offset + 6];
                            mac_addr = format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
                        }
                    }
                    
                    let aligned_len = ((attr.rta_len + 3) & !3) as usize;
                    if aligned_len == 0 { break; }
                    attr_offset += aligned_len;
                }
                
                print!("{}: ", if_name);
                if !mac_addr.is_empty() {
                    print!("<BROADCAST,MULTICAST> link/ether {}", mac_addr);
                }
                println!();
            }
        }
        
        let aligned_msg_len = ((hdr.nlmsg_len + 3) & !3) as usize;
        if aligned_msg_len == 0 || offset + aligned_msg_len > data.len() { break; }
        offset += aligned_msg_len;
    }
}

fn parse_addr_info(data: &[u8]) {
    if data.is_empty() {
        println!("    (No address data received)");
        return;
    }
    
    let mut offset = 0;
    let mut addr_count = 0;
    
    while offset + 16 <= data.len() {
        let hdr = unsafe { &*(data.as_ptr().add(offset) as *const NlMsgHdr) };
        
        if hdr.nlmsg_type == 3 { // NLMSG_DONE
            break;
        }
        if hdr.nlmsg_type == 2 { // NLMSG_ERROR
            println!("    Netlink error in address info");
            break;
        }
        
        if hdr.nlmsg_type == RTM_NEWADDR {
            let ifaddr_offset = offset + 16;
            if ifaddr_offset + 8 <= data.len() {
                let ifaddr = unsafe { &*(data.as_ptr().add(ifaddr_offset) as *const IfAddrMsg) };
                
                // Parse attributes
                let mut attr_offset = ifaddr_offset + 8;
                let mut ip_addr = String::new();
                
                while attr_offset + 4 <= offset + hdr.nlmsg_len as usize {
                    let attr = unsafe { &*(data.as_ptr().add(attr_offset) as *const RtAttr) };
                    if attr.rta_len < 4 { break; }
                    
                    let data_offset = attr_offset + 4;
                    let data_len = (attr.rta_len as usize).saturating_sub(4);
                    
                    if data_offset + data_len <= data.len() && (attr.rta_type == IFA_ADDRESS || attr.rta_type == IFA_LOCAL) {
                        if data_len >= 4 {
                            let ip = &data[data_offset..data_offset + 4];
                            ip_addr = format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
                        }
                    }
                    
                    let aligned_len = ((attr.rta_len + 3) & !3) as usize;
                    if aligned_len == 0 { break; }
                    attr_offset += aligned_len;
                }
                
                if !ip_addr.is_empty() {
                    println!("    inet {}/{} scope global", ip_addr, ifaddr.ifa_prefixlen);
                    addr_count += 1;
                }
            }
        }
        
        let aligned_msg_len = ((hdr.nlmsg_len + 3) & !3) as usize;
        if aligned_msg_len == 0 || offset + aligned_msg_len > data.len() { break; }
        offset += aligned_msg_len;
    }
    
    if addr_count == 0 {
        println!("    (No addresses configured)");
    }
}
