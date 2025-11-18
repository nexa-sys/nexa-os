use std::env;
use std::process;
use std::ffi::CString;
use std::mem;

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
const IFLA_ADDRESS: u16 = 1;
const IFA_ADDRESS: u16 = 1;

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

fn main() {
    println!("Starting DHCP client...");

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

    // 2. Create UDP socket
    let fd = unsafe { socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP) };
    if fd < 0 {
        println!("Failed to create UDP socket");
        return;
    }

    // 3. Enable broadcast
    let broadcast: i32 = 1;
    if unsafe { setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &broadcast as *const _ as *const std::ffi::c_void, 4) } < 0 {
        println!("Failed to enable broadcast");
        unsafe { close(fd) };
        return;
    }

    // 4. Bind to port 68
    let addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: DHCP_CLIENT_PORT.to_be(),
        sin_addr: 0, // INADDR_ANY
        sin_zero: [0; 8],
    };

    if unsafe { bind(fd, &addr as *const _ as *const std::ffi::c_void, 16) } < 0 {
        println!("Failed to bind to port 68");
        unsafe { close(fd) };
        return;
    }

    // 5. Send DHCP DISCOVER
    let xid: u32 = 0x12345678; // Random transaction ID
    let mut packet = create_dhcp_packet(DHCP_DISCOVER, &mac, xid);
    
    let dest_addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: DHCP_SERVER_PORT.to_be(),
        sin_addr: 0xFFFFFFFF, // INADDR_BROADCAST
        sin_zero: [0; 8],
    };

    println!("Sending DHCP DISCOVER...");
    if unsafe { sendto(fd, &packet as *const _ as *const std::ffi::c_void, mem::size_of::<DhcpPacket>(), 0, &dest_addr as *const _ as *const std::ffi::c_void, 16) } < 0 {
        println!("Failed to send DHCP DISCOVER");
        unsafe { close(fd) };
        return;
    }

    // 6. Receive DHCP OFFER
    println!("Waiting for DHCP OFFER...");
    let mut buf = [0u8; 1024];
    let mut src_addr = SockAddrIn {
        sin_family: 0,
        sin_port: 0,
        sin_addr: 0,
        sin_zero: [0; 8],
    };
    let mut addr_len: u32 = 16;

    let len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 1024, 0, &mut src_addr as *mut _ as *mut std::ffi::c_void, &mut addr_len) };
    if len < 0 {
        println!("Failed to receive DHCP OFFER");
        unsafe { close(fd) };
        return;
    }

    let offer_packet = unsafe { &*(buf.as_ptr() as *const DhcpPacket) };
    if offer_packet.xid != xid {
        println!("Received packet with wrong XID");
        unsafe { close(fd) };
        return;
    }

    let offered_ip = u32::from_be(offer_packet.yiaddr);
    let server_ip = u32::from_be(offer_packet.siaddr); // Usually in options, but siaddr might be set
    
    println!("Received DHCP OFFER: IP {}.{}.{}.{}", 
             (offered_ip >> 24) & 0xFF, (offered_ip >> 16) & 0xFF, (offered_ip >> 8) & 0xFF, offered_ip & 0xFF);

    // 7. Send DHCP REQUEST
    println!("Sending DHCP REQUEST...");
    packet = create_dhcp_packet(DHCP_REQUEST, &mac, xid);
    // Add Requested IP option
    add_option(&mut packet, DHCP_OPT_REQUESTED_IP, &offered_ip.to_be_bytes());
    // Add Server ID option (required for REQUEST)
    // We need to find Server ID in options of OFFER, but for simplicity let's try using siaddr or just broadcast
    // The correct way is to parse options.
    
    // Parse options to find Server ID
    let server_id = find_option(&buf[..len as usize], DHCP_OPT_SERVER_ID);
    if let Some(sid) = server_id {
        add_option(&mut packet, DHCP_OPT_SERVER_ID, sid);
    } else {
        // Fallback to siaddr if option not present (though it should be)
        add_option(&mut packet, DHCP_OPT_SERVER_ID, &offer_packet.siaddr.to_ne_bytes());
    }

    if unsafe { sendto(fd, &packet as *const _ as *const std::ffi::c_void, mem::size_of::<DhcpPacket>(), 0, &dest_addr as *const _ as *const std::ffi::c_void, 16) } < 0 {
        println!("Failed to send DHCP REQUEST");
        unsafe { close(fd) };
        return;
    }

    // 8. Receive DHCP ACK
    println!("Waiting for DHCP ACK...");
    let len = unsafe { recvfrom(fd, buf.as_mut_ptr() as *mut std::ffi::c_void, 1024, 0, &mut src_addr as *mut _ as *mut std::ffi::c_void, &mut addr_len) };
    if len < 0 {
        println!("Failed to receive DHCP ACK");
        unsafe { close(fd) };
        return;
    }

    let ack_packet = unsafe { &*(buf.as_ptr() as *const DhcpPacket) };
    if ack_packet.xid != xid {
        println!("Received packet with wrong XID");
        unsafe { close(fd) };
        return;
    }

    // Check message type option to be sure it is ACK
    // TODO: Parse options

    let assigned_ip = u32::from_be(ack_packet.yiaddr);
    println!("DHCP Success! Assigned IP: {}.{}.{}.{}", 
             (assigned_ip >> 24) & 0xFF, (assigned_ip >> 16) & 0xFF, (assigned_ip >> 8) & 0xFF, assigned_ip & 0xFF);

    // Configure interface with IP
    // Default mask 255.255.255.0 if not provided
    let subnet_mask = 0xFFFFFF00; // TODO: Parse from options
    configure_interface(if_index, assigned_ip, subnet_mask);
    
    unsafe { close(fd) };
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
    let mut offset = 16; // Skip NlMsgHdr
    while offset < len as usize {
        let hdr = unsafe { &*(buf.as_ptr().add(offset) as *const NlMsgHdr) };
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
        
        while attr_offset < offset + hdr.nlmsg_len as usize {
            let attr = unsafe { &*(buf.as_ptr().add(attr_offset) as *const RtAttr) };
            if attr.rta_type == IFLA_ADDRESS {
                let mac_ptr = unsafe { buf.as_ptr().add(attr_offset + 4) };
                let mut mac = [0u8; 6];
                unsafe { std::ptr::copy_nonoverlapping(mac_ptr, mac.as_mut_ptr(), 6) };
                return Some((if_index, mac));
            }
            attr_offset += ((attr.rta_len + 3) & !3) as usize; // Align to 4 bytes
        }

        offset += ((hdr.nlmsg_len + 3) & !3) as usize;
    }

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
    // Skip header
    let options_start = mem::size_of::<DhcpPacket>() - 308;
    if buf.len() < options_start { return None; }
    
    let mut idx = options_start;
    // Check magic cookie
    if buf[idx] != 0x63 || buf[idx+1] != 0x82 || buf[idx+2] != 0x53 || buf[idx+3] != 0x63 {
        return None;
    }
    idx += 4;

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
