use std::env;
use std::process;

// Constants
const AF_NETLINK: i32 = 16;
const SOCK_DGRAM: i32 = 2;

// Netlink constants
const RTM_GETLINK: u16 = 18;

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
    println!("ip addr not fully implemented");
}
