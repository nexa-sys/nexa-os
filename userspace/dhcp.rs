use std::env;
use std::process;
use std::ffi::CString;
use std::mem;

// Import time functions from nrlib
extern "C" {
    fn get_uptime() -> u64;
    fn sleep(seconds: u32);
}

// Constants
const AF_INET: i32 = 2;
const AF_NETLINK: i32 = 16;
const SOCK_DGRAM: i32 = 2;
const IPPROTO_UDP: i32 = 17;
const SOL_SOCKET: i32 = 1;
const SO_BROADCAST: i32 = 6;

// DHCP Constants
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_OP_BOOTREQUEST: u8 = 1;
const DHCP_OP_BOOTREPLY: u8 = 2;
const DHCP_HTYPE_ETHER: u8 = 1;
const DHCP_HLEN_ETHER: u8 = 6;
const DHCP_MAGIC_COOKIE: u32 = 0x63825363;

// DHCP Options
const DHCP_OPT_PAD: u8 = 0;
const DHCP_OPT_SUBNET_MASK: u8 = 1;
const DHCP_OPT_ROUTER: u8 = 3;
const DHCP_OPT_DNS: u8 = 6;
const DHCP_OPT_REQUESTED_IP: u8 = 50;
const DHCP_OPT_LEASE_TIME: u8 = 51;
const DHCP_OPT_MSG_TYPE: u8 = 53;
const DHCP_OPT_SERVER_ID: u8 = 54;
const DHCP_OPT_PARAM_REQ_LIST: u8 = 55;
const DHCP_OPT_END: u8 = 255;

// DHCP Message Types
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
const DHCP_DECLINE: u8 = 4;
const DHCP_ACK: u8 = 5;
const DHCP_NAK: u8 = 6;
const DHCP_RELEASE: u8 = 7;
const DHCP_INFORM: u8 = 8;

// Netlink constants
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_NEWROUTE: u16 = 24;
const IFLA_ADDRESS: u16 = 1;
const IFA_ADDRESS: u16 = 1;
const RTA_DST: u16 = 1;
const RTA_GATEWAY: u16 = 5;
const RTA_OIF: u16 = 4;

#[repr(C)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

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
struct RtMsg {
    rtm_family: u8,
    rtm_dst_len: u8,
    rtm_src_len: u8,
    rtm_tos: u8,
    rtm_table: u8,
    rtm_protocol: u8,
    rtm_scope: u8,
    rtm_type: u8,
    rtm_flags: u32,
}

#[repr(C)]
struct RtAttr {
    rta_len: u16,
    rta_type: u16,
}

#[repr(C, packed)]
struct DhcpPacket {
    op: u8,
    htype: u8,
    hlen: u8,
    hops: u8,
    xid: u32,
    secs: u16,
    flags: u16,
    ciaddr: u32,
    yiaddr: u32,
    siaddr: u32,
    giaddr: u32,
    chaddr: [u8; 16],
    sname: [u8; 64],
    file: [u8; 128],
    magic: u32,
    options: [u8; 308],
}

extern "C" {
    fn socket(domain: i32, type_: i32, protocol: i32) -> i32;
    fn bind(sockfd: i32, addr: *const std::ffi::c_void, addrlen: u32) -> i32;
    fn sendto(sockfd: i32, buf: *const std::ffi::c_void, len: usize, flags: i32, dest_addr: *const std::ffi::c_void, addrlen: u32) -> isize;
    fn recvfrom(sockfd: i32, buf: *mut std::ffi::c_void, len: usize, flags: i32, src_addr: *mut std::ffi::c_void, addrlen: *mut u32) -> isize;
    fn setsockopt(sockfd: i32, level: i32, optname: i32, optval: *const std::ffi::c_void, optlen: u32) -> i32;
    fn close(fd: i32) -> i32;
}

struct DhcpLease {
    ip: u32,
    subnet_mask: u32,
    server_ip: u32,
    router: Option<u32>,
    dns_servers: Vec<u32>,
    lease_time: u32,
    renewal_time: u32,
    rebinding_time: u32,
    obtained_at: u64, // Timestamp when lease was obtained
}

fn main() {
    println!("Starting DHCP client daemon...");

    // 1. Get MAC address via Netlink
    let (if_index, mac) = match get_mac_address() {
        Some(m) => m,
        None => {
            println!("Failed to get MAC address");
            return;
        }
    };
    println!("Using MAC address: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} (Index: {})", 
             mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], if_index);

    // Main daemon loop
    loop {
        println!("\n=== Starting DHCP lease acquisition ===");
        
        match acquire_lease(if_index, &mac) {
            Some(lease) => {
                println!("Lease acquired successfully!");
                println!("  IP: {}.{}.{}.{}", 
                         (lease.ip >> 24) & 0xFF, (lease.ip >> 16) & 0xFF, 
                         (lease.ip >> 8) & 0xFF, lease.ip & 0xFF);
                println!("  Lease time: {} seconds", lease.lease_time);
                println!("  Renewal time: {} seconds", lease.renewal_time);
                if let Some(router) = lease.router {
                    println!("  Router: {}.{}.{}.{}", 
                             (router >> 24) & 0xFF, (router >> 16) & 0xFF,
                             (router >> 8) & 0xFF, router & 0xFF);
                }
                if !lease.dns_servers.is_empty() {
                    print!("  DNS servers:");
                    for dns in &lease.dns_servers {
                        print!(" {}.{}.{}.{}", 
                               (dns >> 24) & 0xFF, (dns >> 16) & 0xFF,
                               (dns >> 8) & 0xFF, dns & 0xFF);
                    }
                    println!();
                }
                
                // Configure interface
                configure_interface(if_index, lease.ip, lease.subnet_mask);
                
                // Configure default gateway if provided
                if let Some(router) = lease.router {
                    configure_default_route(if_index, router);
                }
                
                // Write DNS configuration
                write_resolv_conf(&lease.dns_servers);
                
                // Enter lease maintenance loop
                maintain_lease(if_index, &mac, lease);
            }
            None => {
                println!("Failed to acquire lease, retrying in 30 seconds...");
                unsafe { sleep(30) };
            }
        }
    }
}

fn acquire_lease(if_index: u32, mac: &[u8; 6]) -> Option<DhcpLease> {
    // Create UDP socket
    let fd = unsafe { socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP) };
    println!("[acquire_lease] socket() returned: {}", fd);
    if fd < 0 {
        println!("Failed to create UDP socket");
        return None;
    }

    // Enable broadcast
    let broadcast: i32 = 1;
    let ret = unsafe { setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &broadcast as *const _ as *const std::ffi::c_void, 4) };
    println!("[acquire_lease] setsockopt(SO_BROADCAST) returned: {}", ret);
    if ret < 0 {
        println!("Failed to enable broadcast");
        unsafe { close(fd) };
        return None;
    }

    // Bind to port 68
    let addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: DHCP_CLIENT_PORT.to_be(),
        sin_addr: 0, // INADDR_ANY
        sin_zero: [0; 8],
    };

    let ret = unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 16) };
    println!("[acquire_lease] bind(port={}) returned: {}", DHCP_CLIENT_PORT, ret);
    if ret < 0 {
        println!("Failed to bind to port 68");
        unsafe { close(fd) };
        return None;
    }

    // Generate transaction ID
    let xid: u32 = generate_xid();
    
    let dest_addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: DHCP_SERVER_PORT.to_be(),
        sin_addr: (0xFFFFFFFFu32).to_be(), // INADDR_BROADCAST in network byte order
        sin_zero: [0; 8],
    };

    // Send DHCP DISCOVER
    println!("Sending DHCP DISCOVER...");
    let packet = create_dhcp_packet(DHCP_DISCOVER, mac, xid);
    let packet_size = mem::size_of::<DhcpPacket>();
    println!("[acquire_lease] Calling sendto: fd={}, size={}, dest=255.255.255.255:{}", 
             fd, packet_size, DHCP_SERVER_PORT);
    let ret = unsafe { sendto(fd, &packet as *const _ as *const std::ffi::c_void, packet_size, 0, &dest_addr as *const _ as *const std::ffi::c_void, 16) };
    println!("[acquire_lease] sendto() returned: {}", ret);
    if ret < 0 {
        println!("Failed to send DHCP DISCOVER");
        unsafe { close(fd) };
        return None;
    }

    // Receive DHCP OFFER (with retry loop)
    println!("Waiting for DHCP OFFER...");
    let mut buf = [0u8; 1024];
    let mut src_addr = SockAddrIn {
        sin_family: 0,
        sin_port: 0,
        sin_addr: 0,
        sin_zero: [0; 8],
    };
    let mut addr_len: u32 = 16;

    // Retry loop: try up to 50 times with 100ms delay between attempts (total ~5 seconds)
    let mut len: isize = -1;
    for attempt in 0..50 {
        len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 1024, 0, &mut src_addr as *mut _ as *mut std::ffi::c_void, &mut addr_len) };
        
        if len >= 0 {
            println!("Received DHCP OFFER after {} attempts", attempt + 1);
            break;
        }
        
        // Brief delay before retry (sleep ~100ms)
        // Note: This is a busy-wait since we don't have proper sleep yet
        for _ in 0..100000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
    
    if len < 0 {
        println!("Failed to receive DHCP OFFER after retries");
        unsafe { close(fd) };
        return None;
    }

    let offer_packet = unsafe { &*(buf.as_ptr() as *const DhcpPacket) };
    if offer_packet.xid != xid {
        println!("Received packet with wrong XID");
        unsafe { close(fd) };
        return None;
    }

    let offered_ip = u32::from_be(offer_packet.yiaddr);
    println!("Received DHCP OFFER: IP {}.{}.{}.{}", 
             (offered_ip >> 24) & 0xFF, (offered_ip >> 16) & 0xFF, (offered_ip >> 8) & 0xFF, offered_ip & 0xFF);

    // Send DHCP REQUEST
    println!("Sending DHCP REQUEST...");
    let mut packet = create_dhcp_packet(DHCP_REQUEST, mac, xid);
    add_option(&mut packet, DHCP_OPT_REQUESTED_IP, &offered_ip.to_be_bytes());
    
    // Parse Server ID from OFFER
    if let Some(sid) = find_option(&buf[..len as usize], DHCP_OPT_SERVER_ID) {
        add_option(&mut packet, DHCP_OPT_SERVER_ID, sid);
    } else {
        add_option(&mut packet, DHCP_OPT_SERVER_ID, &offer_packet.siaddr.to_ne_bytes());
    }

    if unsafe { sendto(fd, &packet as *const _ as *const std::ffi::c_void, mem::size_of::<DhcpPacket>(), 0, &dest_addr as *const _ as *const std::ffi::c_void, 16) } < 0 {
        println!("Failed to send DHCP REQUEST");
        unsafe { close(fd) };
        return None;
    }

    // Receive DHCP ACK (with retry loop)
    println!("Waiting for DHCP ACK...");
    let mut len: isize = -1;
    for attempt in 0..50 {
        len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 1024, 0, &mut src_addr as *mut _ as *mut std::ffi::c_void, &mut addr_len) };
        
        if len >= 0 {
            println!("Received DHCP ACK after {} attempts", attempt + 1);
            break;
        }
        
        // Brief delay before retry
        for _ in 0..100000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
    
    if len < 0 {
        println!("Failed to receive DHCP ACK after retries");
        unsafe { close(fd) };
        return None;
    }

    let ack_packet = unsafe { &*(buf.as_ptr() as *const DhcpPacket) };
    if ack_packet.xid != xid {
        println!("Received packet with wrong XID");
        unsafe { close(fd) };
        return None;
    }

    // Debug: Print all DHCP options
    debug_print_dhcp_options(&buf[..len as usize]);

    let assigned_ip = u32::from_be(ack_packet.yiaddr);
    
    // Parse lease options
    let lease_time = find_option(&buf[..len as usize], DHCP_OPT_LEASE_TIME)
        .and_then(|b| if b.len() == 4 { Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]])) } else { None })
        .unwrap_or(3600); // Default 1 hour
    
    let subnet_mask = find_option(&buf[..len as usize], DHCP_OPT_SUBNET_MASK)
        .and_then(|b| if b.len() == 4 { Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]])) } else { None })
        .unwrap_or(0xFFFFFF00); // Default 255.255.255.0
    
    let server_ip = find_option(&buf[..len as usize], DHCP_OPT_SERVER_ID)
        .and_then(|b| if b.len() == 4 { Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]])) } else { None })
        .unwrap_or(0);
    
    // Parse router (default gateway)
    println!("[acquire_lease] Searching for DHCP_OPT_ROUTER (option 3)...");
    let router = find_option(&buf[..len as usize], DHCP_OPT_ROUTER)
        .and_then(|b| {
            println!("[acquire_lease] Found router option, len={}", b.len());
            if b.len() >= 4 {
                let ip = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
                println!("[acquire_lease] Parsed router IP: {}.{}.{}.{}", 
                    (ip >> 24) & 0xFF, (ip >> 16) & 0xFF, (ip >> 8) & 0xFF, ip & 0xFF);
                Some(ip)
            } else {
                println!("[acquire_lease] Router option too short!");
                None
            }
        });
    if router.is_none() {
        println!("[acquire_lease] WARNING: No router option found in DHCP ACK!");
    }
    
    // Parse DNS servers
    let mut dns_servers = Vec::new();
    if let Some(dns_data) = find_option(&buf[..len as usize], DHCP_OPT_DNS) {
        let mut offset = 0;
        while offset + 4 <= dns_data.len() {
            let dns_ip = u32::from_be_bytes([
                dns_data[offset], 
                dns_data[offset + 1], 
                dns_data[offset + 2], 
                dns_data[offset + 3]
            ]);
            dns_servers.push(dns_ip);
            offset += 4;
        }
    }

    unsafe { close(fd) };

    Some(DhcpLease {
        ip: assigned_ip,
        subnet_mask,
        server_ip,
        router,
        dns_servers,
        lease_time,
        renewal_time: lease_time / 2,      // T1: 50% of lease time
        rebinding_time: lease_time * 7 / 8, // T2: 87.5% of lease time
        obtained_at: unsafe { get_uptime() },
    })
}

fn maintain_lease(if_index: u32, mac: &[u8; 6], mut lease: DhcpLease) {
    println!("\n=== Entering lease maintenance mode ===");
    
    loop {
        let elapsed = unsafe { get_uptime() } - lease.obtained_at;
        
        // Check if we need to renew
        if elapsed >= lease.renewal_time as u64 {
            println!("Renewal time reached, attempting to renew lease...");
            if let Some(new_lease) = renew_lease(if_index, mac, &lease) {
                lease = new_lease;
                continue;
            } else {
                println!("Renewal failed, will retry...");
            }
        }
        
        // Check if lease has expired
        if elapsed >= lease.lease_time as u64 {
            println!("Lease expired! Re-acquiring...");
            return; // Exit maintenance loop to re-acquire
        }
        
        // Sleep for a while before checking again
        unsafe { sleep(10) };
    }
}

fn renew_lease(if_index: u32, mac: &[u8; 6], current_lease: &DhcpLease) -> Option<DhcpLease> {
    // Create socket for renewal
    let fd = unsafe { socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP) };
    if fd < 0 {
        return None;
    }

    // Bind to current IP
    let addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: DHCP_CLIENT_PORT.to_be(),
        sin_addr: current_lease.ip.to_be(),
        sin_zero: [0; 8],
    };

    if unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 16) } < 0 {
        unsafe { close(fd) };
        return None;
    }

    let xid = generate_xid();
    
    // Send unicast DHCP REQUEST to server
    let mut packet = create_dhcp_packet(DHCP_REQUEST, mac, xid);
    packet.ciaddr = current_lease.ip.to_be(); // Set current IP
    add_option(&mut packet, DHCP_OPT_REQUESTED_IP, &current_lease.ip.to_be_bytes());
    add_option(&mut packet, DHCP_OPT_SERVER_ID, &current_lease.server_ip.to_be_bytes());

    let dest_addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: DHCP_SERVER_PORT.to_be(),
        sin_addr: current_lease.server_ip.to_be(),
        sin_zero: [0; 8],
    };

    if unsafe { sendto(fd, &packet as *const _ as *const std::ffi::c_void, mem::size_of::<DhcpPacket>(), 0, &dest_addr as *const _ as *const std::ffi::c_void, 16) } < 0 {
        unsafe { close(fd) };
        return None;
    }

    // Receive ACK (with retry loop)
    let mut buf = [0u8; 1024];
    let mut addr_len: u32 = 16;
    let mut len: isize = -1;
    
    for attempt in 0..50 {
        len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 1024, 0, std::ptr::null_mut(), &mut addr_len) };
        
        if len >= 0 {
            println!("Received renewal ACK after {} attempts", attempt + 1);
            break;
        }
        
        // Brief delay before retry
        for _ in 0..100000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
    
    unsafe { close(fd) };

    if len < 0 {
        return None;
    }

    let ack_packet = unsafe { &*(buf.as_ptr() as *const DhcpPacket) };
    if ack_packet.xid != xid {
        return None;
    }

    // Parse renewed lease
    let lease_time = find_option(&buf[..len as usize], DHCP_OPT_LEASE_TIME)
        .and_then(|b| if b.len() == 4 { Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]])) } else { None })
        .unwrap_or(current_lease.lease_time);

    println!("Lease renewed! New lease time: {} seconds", lease_time);

    Some(DhcpLease {
        ip: current_lease.ip,
        subnet_mask: current_lease.subnet_mask,
        server_ip: current_lease.server_ip,
        router: current_lease.router,
        dns_servers: current_lease.dns_servers.clone(),
        lease_time,
        renewal_time: lease_time / 2,
        rebinding_time: lease_time * 7 / 8,
        obtained_at: unsafe { get_uptime() },
    })
}

fn generate_xid() -> u32 {
    // XID based on uptime
    unsafe { (get_uptime() & 0xFFFFFFFF) as u32 }
}

fn get_mac_address() -> Option<(u32, [u8; 6])> {
    println!("[get_mac_address] Creating netlink socket...");
    let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM, 0) };
    if fd < 0 {
        println!("[get_mac_address] socket() failed, fd={}", fd);
        return None;
    }
    println!("[get_mac_address] Socket created, fd={}", fd);

    let addr = SockAddrNl {
        nl_family: AF_NETLINK as u16,
        nl_pad: 0,
        nl_pid: process::id(),
        nl_groups: 0,
    };
    
    println!("[get_mac_address] Binding socket, pid={}", process::id());
    if unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 12) } < 0 {
        println!("[get_mac_address] bind() failed");
        unsafe { close(fd) };
        return None;
    }
    println!("[get_mac_address] Socket bound successfully");

    let req = NlMsgHdr {
        nlmsg_len: 16,
        nlmsg_type: RTM_GETLINK,
        nlmsg_flags: 1, // NLM_F_REQUEST
        nlmsg_seq: 1,
        nlmsg_pid: process::id(),
    };

    println!("[get_mac_address] Sending RTM_GETLINK request...");
    if unsafe { sendto(fd, &req as *const _ as *const std::ffi::c_void, 16, 0, std::ptr::null(), 0) } < 0 {
        println!("[get_mac_address] sendto() failed");
        unsafe { close(fd) };
        return None;
    }
    println!("[get_mac_address] Request sent successfully");

    let mut buf = [0u8; 4096];
    println!("[get_mac_address] Waiting for response...");
    let len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 4096, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
    println!("[get_mac_address] recvfrom returned: {}", len);
    unsafe { close(fd) };

    if len < 0 {
        println!("[get_mac_address] recvfrom failed, len={}", len);
        return None;
    }

    // Parse response to find IFLA_ADDRESS
    // We expect a single RTM_NEWLINK message followed by attributes
    let mut offset = 0;
    println!("[get_mac_address] Parsing response, len={}", len);
    while offset < len as usize {
        let hdr = unsafe { &*(buf.as_ptr().add(offset) as *const NlMsgHdr) };
        println!("[get_mac_address] Msg at offset {}: type={}, len={}", offset, hdr.nlmsg_type, hdr.nlmsg_len);
        
        if hdr.nlmsg_type == 3 { // NLMSG_DONE
            break;
        }
        if hdr.nlmsg_type == 2 { // NLMSG_ERROR
            return None;
        }

        // Parse IfInfoMsg
        let ifinfo = unsafe { &*(buf.as_ptr().add(offset + 16) as *const IfInfoMsg) };
        let if_index = ifinfo.ifi_index;
        let mut attr_offset = offset + 16 + 16; // NlMsgHdr + IfInfoMsg
        
        println!("[get_mac_address] Attributes start at {}", attr_offset);
        while attr_offset < offset + hdr.nlmsg_len as usize {
            let attr = unsafe { &*(buf.as_ptr().add(attr_offset) as *const RtAttr) };
            println!("[get_mac_address] Attr at {}: type={}, len={}", attr_offset, attr.rta_type, attr.rta_len);
            
            if attr.rta_type == IFLA_ADDRESS {
                let mac_ptr = unsafe { buf.as_ptr().add(attr_offset + 4) };
                let mut mac = [0u8; 6];
                unsafe { std::ptr::copy_nonoverlapping(mac_ptr, mac.as_mut_ptr(), 6) };
                return Some((if_index, mac));
            }
            let aligned_len = ((attr.rta_len + 3) & !3) as usize;
            if aligned_len == 0 { 
                println!("[get_mac_address] Zero length attribute, breaking");
                break; 
            } // Prevent infinite loop
            attr_offset += aligned_len;
        }

        let aligned_msg_len = ((hdr.nlmsg_len + 3) & !3) as usize;
        if aligned_msg_len == 0 { 
            println!("[get_mac_address] Zero length message, breaking");
            break; 
        } // Prevent infinite loop
        offset += aligned_msg_len;
    }
    println!("[get_mac_address] Parsing finished, not found");

    None
}

fn configure_interface(if_index: u32, ip: u32, mask: u32) {
    println!("Configuring interface {} with IP...", if_index);
    
    let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM, 0) };
    if fd < 0 {
        println!("Failed to create netlink socket");
        return;
    }

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

    // Calculate prefix length from mask
    let prefix_len = mask.count_ones() as u8;

    let seq = 2; // Arbitrary sequence number
    let mut packet = [0u8; 256];
    let mut pos = 0;

    // Netlink Header
    let hdr = NlMsgHdr {
        nlmsg_len: 0, // Fill later
        nlmsg_type: RTM_NEWADDR,
        nlmsg_flags: 1 | 4, // NLM_F_REQUEST | NLM_F_CREATE
        nlmsg_seq: seq,
        nlmsg_pid: process::id(),
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&hdr as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<NlMsgHdr>());
    }
    pos += mem::size_of::<NlMsgHdr>();

    // IfAddrMsg
    let ifaddr = IfAddrMsg {
        ifa_family: AF_INET as u8,
        ifa_prefixlen: prefix_len,
        ifa_flags: 0,
        ifa_scope: 0,
        ifa_index: if_index,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&ifaddr as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<IfAddrMsg>());
    }
    pos += mem::size_of::<IfAddrMsg>();

    // IFA_ADDRESS Attribute
    let attr_hdr = RtAttr {
        rta_len: (mem::size_of::<RtAttr>() + 4) as u16,
        rta_type: IFA_ADDRESS,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&attr_hdr as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<RtAttr>());
    }
    pos += mem::size_of::<RtAttr>();
    
    let ip_bytes = ip.to_be_bytes();
    unsafe {
        std::ptr::copy_nonoverlapping(ip_bytes.as_ptr(), packet.as_mut_ptr().add(pos), 4);
    }
    pos += 4;
    
    // Align to 4 bytes
    while pos % 4 != 0 { packet[pos] = 0; pos += 1; }

    // Update length
    let len = pos as u32;
    unsafe {
        std::ptr::copy_nonoverlapping(&len as *const _ as *const u8, packet.as_mut_ptr(), 4);
    }

    if unsafe { sendto(fd, packet.as_ptr() as *const std::ffi::c_void, pos, 0, std::ptr::null(), 0) } < 0 {
        println!("Failed to send RTM_NEWADDR");
    } else {
        println!("Sent RTM_NEWADDR to configure IP");
    }

    unsafe { close(fd) };
}

fn configure_default_route(if_index: u32, gateway: u32) {
    println!("Configuring default route via {}.{}.{}.{}", 
             (gateway >> 24) & 0xFF, (gateway >> 16) & 0xFF,
             (gateway >> 8) & 0xFF, gateway & 0xFF);
    
    let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM, 0) };
    if fd < 0 {
        println!("Failed to create netlink socket for route");
        return;
    }

    let addr = SockAddrNl {
        nl_family: AF_NETLINK as u16,
        nl_pad: 0,
        nl_pid: process::id(),
        nl_groups: 0,
    };
    
    if unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 12) } < 0 {
        println!("Failed to bind netlink socket for route");
        unsafe { close(fd) };
        return;
    }

    let seq = 3; // Arbitrary sequence number
    let mut packet = [0u8; 256];
    let mut pos = 0;

    // Netlink Header
    let hdr = NlMsgHdr {
        nlmsg_len: 0, // Fill later
        nlmsg_type: RTM_NEWROUTE,
        nlmsg_flags: 1 | 4 | 0x100, // NLM_F_REQUEST | NLM_F_CREATE | NLM_F_REPLACE
        nlmsg_seq: seq,
        nlmsg_pid: process::id(),
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&hdr as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<NlMsgHdr>());
    }
    pos += mem::size_of::<NlMsgHdr>();

    // RtMsg - for default route (0.0.0.0/0)
    let rtmsg = RtMsg {
        rtm_family: AF_INET as u8,
        rtm_dst_len: 0,    // /0 means default route
        rtm_src_len: 0,
        rtm_tos: 0,
        rtm_table: 254,    // RT_TABLE_MAIN
        rtm_protocol: 2,   // RTPROT_KERNEL
        rtm_scope: 0,      // RT_SCOPE_UNIVERSE
        rtm_type: 1,       // RTN_UNICAST
        rtm_flags: 0,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&rtmsg as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<RtMsg>());
    }
    pos += mem::size_of::<RtMsg>();

    // RTA_GATEWAY Attribute
    let attr_gw = RtAttr {
        rta_len: (mem::size_of::<RtAttr>() + 4) as u16,
        rta_type: RTA_GATEWAY,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&attr_gw as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<RtAttr>());
    }
    pos += mem::size_of::<RtAttr>();
    
    let gw_bytes = gateway.to_be_bytes();
    unsafe {
        std::ptr::copy_nonoverlapping(gw_bytes.as_ptr(), packet.as_mut_ptr().add(pos), 4);
    }
    pos += 4;
    
    // Align to 4 bytes
    while pos % 4 != 0 { packet[pos] = 0; pos += 1; }

    // RTA_OIF Attribute (Output Interface)
    let attr_oif = RtAttr {
        rta_len: (mem::size_of::<RtAttr>() + 4) as u16,
        rta_type: RTA_OIF,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&attr_oif as *const _ as *const u8, packet.as_mut_ptr().add(pos), mem::size_of::<RtAttr>());
    }
    pos += mem::size_of::<RtAttr>();
    
    unsafe {
        std::ptr::copy_nonoverlapping(&if_index as *const _ as *const u8, packet.as_mut_ptr().add(pos), 4);
    }
    pos += 4;
    
    // Align to 4 bytes
    while pos % 4 != 0 { packet[pos] = 0; pos += 1; }

    // Update length
    let len = pos as u32;
    unsafe {
        std::ptr::copy_nonoverlapping(&len as *const _ as *const u8, packet.as_mut_ptr(), 4);
    }

    if unsafe { sendto(fd, packet.as_ptr() as *const std::ffi::c_void, pos, 0, std::ptr::null(), 0) } < 0 {
        println!("Failed to send RTM_NEWROUTE");
    } else {
        println!("Successfully configured default route");
    }

    unsafe { close(fd) };
}

fn write_resolv_conf(dns_servers: &[u32]) {
    use std::io::Write;
    
    if dns_servers.is_empty() {
        println!("No DNS servers to write to /etc/resolv.conf");
        return;
    }
    
    let mut file = match std::fs::File::create("/etc/resolv.conf") {
        Ok(f) => f,
        Err(e) => {
            println!("Failed to create /etc/resolv.conf: {}", e);
            return;
        }
    };
    
    // Write header
    if let Err(e) = writeln!(file, "# Generated by DHCP client") {
        println!("Failed to write to /etc/resolv.conf: {}", e);
        return;
    }
    
    // Write nameserver entries
    for dns_ip in dns_servers {
        let dns_str = format!("nameserver {}.{}.{}.{}\n",
                             (dns_ip >> 24) & 0xFF,
                             (dns_ip >> 16) & 0xFF,
                             (dns_ip >> 8) & 0xFF,
                             dns_ip & 0xFF);
        if let Err(e) = file.write_all(dns_str.as_bytes()) {
            println!("Failed to write DNS server to /etc/resolv.conf: {}", e);
            return;
        }
    }
    
    println!("Updated /etc/resolv.conf with {} DNS server(s)", dns_servers.len());
}

fn create_dhcp_packet(msg_type: u8, mac: &[u8; 6], xid: u32) -> DhcpPacket {
    let mut packet = DhcpPacket {
        op: DHCP_OP_BOOTREQUEST,
        htype: DHCP_HTYPE_ETHER,
        hlen: DHCP_HLEN_ETHER,
        hops: 0,
        xid: xid,
        secs: 0,
        flags: 0, // Unicast (or 0x8000 for broadcast)
        ciaddr: 0,
        yiaddr: 0,
        siaddr: 0,
        giaddr: 0,
        chaddr: [0; 16],
        sname: [0; 64],
        file: [0; 128],
        magic: DHCP_MAGIC_COOKIE.to_be(),
        options: [0; 308],
    };

    packet.chaddr[0..6].copy_from_slice(mac);

    // Add Magic Cookie (already set in struct init but let's be explicit about options start)
    // Actually magic is a field.
    
    // Add Message Type Option
    let mut opt_idx = 0;
    packet.options[opt_idx] = DHCP_OPT_MSG_TYPE;
    packet.options[opt_idx+1] = 1;
    packet.options[opt_idx+2] = msg_type;
    opt_idx += 3;

    // Add Parameter Request List
    packet.options[opt_idx] = DHCP_OPT_PARAM_REQ_LIST;
    packet.options[opt_idx+1] = 3;
    packet.options[opt_idx+2] = DHCP_OPT_SUBNET_MASK;
    packet.options[opt_idx+3] = DHCP_OPT_ROUTER;
    packet.options[opt_idx+4] = DHCP_OPT_DNS;
    opt_idx += 5;

    packet.options[opt_idx] = DHCP_OPT_END;

    packet
}

fn add_option(packet: &mut DhcpPacket, code: u8, data: &[u8]) {
    // Find end option
    let mut idx = 0;
    while idx < packet.options.len() && packet.options[idx] != DHCP_OPT_END {
        idx += packet.options[idx+1] as usize + 2;
    }

    if idx + data.len() + 3 < packet.options.len() {
        packet.options[idx] = code;
        packet.options[idx+1] = data.len() as u8;
        packet.options[idx+2..idx+2+data.len()].copy_from_slice(data);
        packet.options[idx+2+data.len()] = DHCP_OPT_END;
    }
}

fn find_option(buf: &[u8], code: u8) -> Option<&[u8]> {
    // Magic cookie is the u32 field before options array
    let magic_offset = mem::size_of::<DhcpPacket>() - 308 - 4;
    if buf.len() < magic_offset + 4 { return None; }
    
    // Check magic cookie
    if buf[magic_offset] != 0x63 || buf[magic_offset+1] != 0x82 || buf[magic_offset+2] != 0x53 || buf[magic_offset+3] != 0x63 {
        return None;
    }
    
    let mut idx = magic_offset + 4;  // Start after magic cookie

    while idx < buf.len() {
        let opt = buf[idx];
        if opt == DHCP_OPT_END { break; }
        if opt == DHCP_OPT_PAD { idx += 1; continue; }
        
        if idx + 1 >= buf.len() { break; }
        let len = buf[idx+1] as usize;
        
        if idx + 2 + len > buf.len() { break; }
        
        if opt == code {
            return Some(&buf[idx+2..idx+2+len]);
        }
        
        idx += 2 + len;
    }
    None
}

fn debug_print_dhcp_options(buf: &[u8]) {
    // DHCP packet structure: magic cookie is a separate u32 field BEFORE options array
    // So options start at: size_of::<DhcpPacket>() - 308
    // But the magic cookie is the u32 field just before options
    let magic_offset = mem::size_of::<DhcpPacket>() - 308 - 4;  // magic is before options
    if buf.len() < magic_offset + 4 { 
        println!("[debug_dhcp_options] Buffer too short");
        return;
    }
    
    println!("[debug_dhcp_options] Buffer len={}, magic_offset={}", buf.len(), magic_offset);
    println!("[debug_dhcp_options] Magic cookie bytes: [{:02x} {:02x} {:02x} {:02x}]", 
        buf[magic_offset], buf[magic_offset+1], buf[magic_offset+2], buf[magic_offset+3]);
    
    // Check magic cookie
    if buf[magic_offset] != 0x63 || buf[magic_offset+1] != 0x82 || buf[magic_offset+2] != 0x53 || buf[magic_offset+3] != 0x63 {
        println!("[debug_dhcp_options] Invalid magic cookie (expected 63 82 53 63)");
        return;
    }
    
    let mut idx = magic_offset + 4;  // Start after magic cookie
    
    println!("[debug_dhcp_options] DHCP options in packet:");
    while idx < buf.len() {
        let opt = buf[idx];
        if opt == DHCP_OPT_END { 
            println!("  Option 255: END");
            break; 
        }
        if opt == DHCP_OPT_PAD { 
            idx += 1; 
            continue; 
        }
        
        if idx + 1 >= buf.len() { break; }
        let len = buf[idx+1] as usize;
        
        if idx + 2 + len > buf.len() { break; }
        
        print!("  Option {}: len={}", opt, len);
        if len <= 16 {
            print!(", data=[");
            for i in 0..len {
                print!("{:02x}", buf[idx+2+i]);
                if i < len-1 { print!(" "); }
            }
            println!("]");
        } else {
            println!(" (too long to display)");
        }
        
        idx += 2 + len;
    }
}
