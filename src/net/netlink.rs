/// Netlink socket implementation
/// Provides interface for user-space tools to query network configuration
/// Implements a simplified netlink protocol for RTM_* messages

use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use super::drivers::NetError;
use super::stack::NetStack;

/// Netlink message types for RTM (routing) messages
pub const NLMSG_DONE: u16 = 3;
pub const NLMSG_ERROR: u16 = 2;

/// RTM message types
pub const RTM_GETLINK: u16 = 18;    // Get link info
pub const RTM_GETADDR: u16 = 22;    // Get address info
pub const RTM_NEWLINK: u16 = 16;    // New link
pub const RTM_NEWADDR: u16 = 20;    // New address

/// Interface info attributes
pub const IFLA_IFNAME: u16 = 3;    // Interface name
pub const IFLA_MTU: u16 = 4;        // MTU
pub const IFLA_OPERSTATE: u16 = 17; // Operational state
pub const IFLA_ADDRESS: u16 = 1;   // MAC address

/// Address attributes
pub const IFA_ADDRESS: u16 = 1;    // IP address
pub const IFA_LABEL: u16 = 3;      // Interface name label

/// Netlink message header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NlMsgHdr {
    pub nlmsg_len: u32,     // Length of message including header
    pub nlmsg_type: u16,    // Message type (RTM_*, NLMSG_*)
    pub nlmsg_flags: u16,   // Flags (NLM_F_*)
    pub nlmsg_seq: u32,     // Sequence number
    pub nlmsg_pid: u32,     // Sender's PID
}

/// Rtnetlink (routing netlink) message header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfInfoMsg {
    pub ifi_family: u8,     // AF_UNSPEC, AF_INET, etc.
    pub __pad: u8,
    pub ifi_type: u16,      // Interface type (ARPHRD_*)
    pub ifi_index: u32,     // Interface index
    pub ifi_flags: u32,     // Interface flags (IFF_*)
    pub ifi_change: u32,    // Change mask
}

/// Attribute header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtAttr {
    pub rta_len: u16,       // Length including header
    pub rta_type: u16,      // Attribute type
}

/// Address message
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfAddrMsg {
    pub ifa_family: u8,     // AF_INET, etc.
    pub ifa_prefixlen: u8,  // Prefix length
    pub ifa_flags: u8,      // Flags
    pub ifa_scope: u8,      // Scope
    pub ifa_index: u32,     // Interface index
}

/// Netlink socket configuration
#[derive(Clone, Copy)]
pub struct NetlinkSocket {
    pub in_use: bool,
    pub pid: u32,           // Binding PID
    pub groups: u32,        // Multicast groups
    seq: u32,               // Last sequence number
    rx_queue_head: usize,
    rx_queue_tail: usize,
    rx_queue_len: usize,
}

const MAX_NETLINK_SOCKETS: usize = 4;
const NETLINK_RX_QUEUE_LEN: usize = 16;
const MAX_NETLINK_PAYLOAD: usize = 4096;

#[derive(Clone, Copy)]
struct NetlinkRxEntry {
    len: usize,
    data: [u8; MAX_NETLINK_PAYLOAD],
}

impl NetlinkSocket {
    pub const fn empty() -> Self {
        Self {
            in_use: false,
            pid: 0,
            groups: 0,
            seq: 0,
            rx_queue_head: 0,
            rx_queue_tail: 0,
            rx_queue_len: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            in_use: true,
            pid: 0,
            groups: 0,
            seq: 0,
            rx_queue_head: 0,
            rx_queue_tail: 0,
            rx_queue_len: 0,
        }
    }
}

pub struct NetlinkSubsystem {
    sockets: [NetlinkSocket; MAX_NETLINK_SOCKETS],
    rx_queues: [[NetlinkRxEntry; NETLINK_RX_QUEUE_LEN]; MAX_NETLINK_SOCKETS],
    next_seq: AtomicUsize,
}

impl NetlinkSubsystem {
    pub const fn new() -> Self {
        Self {
            sockets: [NetlinkSocket::empty(); MAX_NETLINK_SOCKETS],
            rx_queues: [[NetlinkRxEntry {
                len: 0,
                data: [0u8; MAX_NETLINK_PAYLOAD],
            }; NETLINK_RX_QUEUE_LEN]; MAX_NETLINK_SOCKETS],
            next_seq: AtomicUsize::new(1),
        }
    }

    /// Create a new netlink socket
    pub fn create_socket(&mut self) -> Result<usize, NetError> {
        for (idx, socket) in self.sockets.iter_mut().enumerate() {
            if !socket.in_use {
                *socket = NetlinkSocket::new();
                return Ok(idx);
            }
        }
        Err(NetError::TooManyConnections)
    }

    /// Close a netlink socket
    pub fn close_socket(&mut self, socket_idx: usize) -> Result<(), NetError> {
        if socket_idx >= MAX_NETLINK_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        self.sockets[socket_idx].in_use = false;
        self.sockets[socket_idx].rx_queue_len = 0;
        Ok(())
    }

    /// Bind socket to PID and optional multicast groups
    pub fn bind(&mut self, socket_idx: usize, pid: u32, groups: u32) -> Result<(), NetError> {
        if socket_idx >= MAX_NETLINK_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }

        // Check if PID already bound (if non-zero)
        if pid != 0 {
            for socket in &self.sockets {
                if socket.in_use && socket.pid == pid && socket.pid != 0 {
                    return Err(NetError::AddressInUse);
                }
            }
        }

        self.sockets[socket_idx].pid = pid;
        self.sockets[socket_idx].groups = groups;
        Ok(())
    }

    /// Get next sequence number
    pub fn next_seq(&self) -> u32 {
        self.next_seq.fetch_add(1, Ordering::Relaxed) as u32
    }

    /// Send netlink message (queues it for the socket)
    pub fn send_message(&mut self, socket_idx: usize, message: &[u8]) -> Result<(), NetError> {
        if socket_idx >= MAX_NETLINK_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }

        let socket = &mut self.sockets[socket_idx];
        if socket.rx_queue_len >= NETLINK_RX_QUEUE_LEN {
            return Err(NetError::RxQueueFull);
        }

        let len = core::cmp::min(message.len(), MAX_NETLINK_PAYLOAD);
        let slot = socket.rx_queue_tail;
        self.rx_queues[socket_idx][slot].data[..len].copy_from_slice(&message[..len]);
        self.rx_queues[socket_idx][slot].len = len;

        socket.rx_queue_tail = (socket.rx_queue_tail + 1) % NETLINK_RX_QUEUE_LEN;
        socket.rx_queue_len += 1;
        Ok(())
    }

    /// Receive netlink message from socket
    pub fn recv_message(
        &mut self,
        socket_idx: usize,
        buffer: &mut [u8],
    ) -> Result<usize, NetError> {
        if socket_idx >= MAX_NETLINK_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }

        let socket = &mut self.sockets[socket_idx];
        if socket.rx_queue_len == 0 {
            return Err(NetError::RxQueueEmpty);
        }

        let slot = socket.rx_queue_head;
        let len = core::cmp::min(buffer.len(), self.rx_queues[socket_idx][slot].len);
        buffer[..len].copy_from_slice(&self.rx_queues[socket_idx][slot].data[..len]);

        socket.rx_queue_head = (socket.rx_queue_head + 1) % NETLINK_RX_QUEUE_LEN;
        socket.rx_queue_len -= 1;

        Ok(len)
    }

    /// Handle RTM_GETLINK request - return interface information
    pub fn handle_getlink(
        &mut self,
        socket_idx: usize,
        seq: u32,
        stack: &NetStack,
    ) -> Result<(), NetError> {
        for dev_idx in 0..super::MAX_NET_DEVICES {
            if let Some(info) = stack.get_device_info(dev_idx) {
                let message = self.build_ifinfo_message(dev_idx, seq, &info);
                self.send_message(socket_idx, &message)?;
            }
        }

        // Send NLMSG_DONE
        let done = self.build_done_message(seq);
        self.send_message(socket_idx, &done)?;
        Ok(())
    }

    /// Handle RTM_GETADDR request - return address information
    pub fn handle_getaddr(
        &mut self,
        socket_idx: usize,
        seq: u32,
        stack: &NetStack,
    ) -> Result<(), NetError> {
        for dev_idx in 0..super::MAX_NET_DEVICES {
            if let Some(info) = stack.get_device_info(dev_idx) {
                let message = self.build_ifaddr_message(dev_idx, seq, &info);
                self.send_message(socket_idx, &message)?;
            }
        }

        // Send NLMSG_DONE
        let done = self.build_done_message(seq);
        self.send_message(socket_idx, &done)?;
        Ok(())
    }

    fn build_done_message(&self, seq: u32) -> [u8; 16] {
        let hdr = NlMsgHdr {
            nlmsg_len: 16,
            nlmsg_type: NLMSG_DONE,
            nlmsg_flags: 0,
            nlmsg_seq: seq,
            nlmsg_pid: 0,
        };

        let mut msg = [0u8; 16];
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const _ as *const u8, core::mem::size_of::<NlMsgHdr>())
        };
        msg[..core::mem::size_of::<NlMsgHdr>()].copy_from_slice(hdr_bytes);
        msg
    }

    fn build_ifinfo_message(&self, dev_idx: usize, seq: u32, info: &DeviceInfo) -> [u8; 256] {
        let mut msg = [0u8; 256];
        let mut pos = 0;

        // Netlink header
        let hdr = NlMsgHdr {
            nlmsg_len: 0, // Will be filled later
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: 0,
            nlmsg_seq: seq,
            nlmsg_pid: 0,
        };
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const _ as *const u8, core::mem::size_of::<NlMsgHdr>())
        };
        msg[pos..pos + core::mem::size_of::<NlMsgHdr>()].copy_from_slice(hdr_bytes);
        pos += core::mem::size_of::<NlMsgHdr>();

        // Interface info message
        let ifinfo = IfInfoMsg {
            ifi_family: 0, // AF_UNSPEC
            __pad: 0,
            ifi_type: 1,   // ARPHRD_ETHER
            ifi_index: (dev_idx + 1) as u32,
            ifi_flags: 0x41, // IFF_UP | IFF_RUNNING
            ifi_change: 0,
        };
        let ifinfo_bytes = unsafe {
            core::slice::from_raw_parts(&ifinfo as *const _ as *const u8, core::mem::size_of::<IfInfoMsg>())
        };
        msg[pos..pos + core::mem::size_of::<IfInfoMsg>()].copy_from_slice(ifinfo_bytes);
        pos += core::mem::size_of::<IfInfoMsg>();

        // IFLA_ADDRESS attribute (MAC)
        let attr_hdr = RtAttr {
            rta_len: (core::mem::size_of::<RtAttr>() + 6) as u16,
            rta_type: IFLA_ADDRESS,
        };
        let attr_bytes = unsafe {
            core::slice::from_raw_parts(&attr_hdr as *const _ as *const u8, core::mem::size_of::<RtAttr>())
        };
        msg[pos..pos + core::mem::size_of::<RtAttr>()].copy_from_slice(attr_bytes);
        pos += core::mem::size_of::<RtAttr>();
        msg[pos..pos + 6].copy_from_slice(&info.mac);
        pos += 6;

        // Padding to 4-byte boundary
        while pos % 4 != 0 {
            msg[pos] = 0;
            pos += 1;
        }

        // IFLA_IFNAME attribute
        let name = format_device_name(dev_idx);
        let name_len = name.len() + 1;
        let attr_hdr = RtAttr {
            rta_len: (core::mem::size_of::<RtAttr>() + name_len) as u16,
            rta_type: IFLA_IFNAME,
        };
        let attr_bytes = unsafe {
            core::slice::from_raw_parts(&attr_hdr as *const _ as *const u8, core::mem::size_of::<RtAttr>())
        };
        msg[pos..pos + core::mem::size_of::<RtAttr>()].copy_from_slice(attr_bytes);
        pos += core::mem::size_of::<RtAttr>();
        msg[pos..pos + name.len()].copy_from_slice(name.as_bytes());
        pos += name.len();
        msg[pos] = 0; // null terminate
        pos += 1;

        // Update message length
        let len_bytes = (pos as u32).to_ne_bytes();
        msg[0..4].copy_from_slice(&len_bytes);

        msg
    }

    fn build_ifaddr_message(&self, dev_idx: usize, seq: u32, info: &DeviceInfo) -> [u8; 256] {
        let mut msg = [0u8; 256];
        let mut pos = 0;

        // Netlink header
        let hdr = NlMsgHdr {
            nlmsg_len: 0, // Will be filled later
            nlmsg_type: RTM_NEWADDR,
            nlmsg_flags: 0,
            nlmsg_seq: seq,
            nlmsg_pid: 0,
        };
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const _ as *const u8, core::mem::size_of::<NlMsgHdr>())
        };
        msg[pos..pos + core::mem::size_of::<NlMsgHdr>()].copy_from_slice(hdr_bytes);
        pos += core::mem::size_of::<NlMsgHdr>();

        // Address message
        let ifaddr = IfAddrMsg {
            ifa_family: 2,   // AF_INET
            ifa_prefixlen: 24, // /24
            ifa_flags: 0x20, // IFA_F_PERMANENT
            ifa_scope: 0,
            ifa_index: (dev_idx + 1) as u32,
        };
        let ifaddr_bytes = unsafe {
            core::slice::from_raw_parts(&ifaddr as *const _ as *const u8, core::mem::size_of::<IfAddrMsg>())
        };
        msg[pos..pos + core::mem::size_of::<IfAddrMsg>()].copy_from_slice(ifaddr_bytes);
        pos += core::mem::size_of::<IfAddrMsg>();

        // IFA_ADDRESS attribute (IP)
        let attr_hdr = RtAttr {
            rta_len: (core::mem::size_of::<RtAttr>() + 4) as u16,
            rta_type: IFA_ADDRESS,
        };
        let attr_bytes = unsafe {
            core::slice::from_raw_parts(&attr_hdr as *const _ as *const u8, core::mem::size_of::<RtAttr>())
        };
        msg[pos..pos + core::mem::size_of::<RtAttr>()].copy_from_slice(attr_bytes);
        pos += core::mem::size_of::<RtAttr>();
        msg[pos..pos + 4].copy_from_slice(&info.ip);
        pos += 4;

        // Update message length
        let len_bytes = (pos as u32).to_ne_bytes();
        msg[0..4].copy_from_slice(&len_bytes);

        msg
    }
}

#[derive(Clone, Copy)]
pub struct DeviceInfo {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub present: bool,
}

fn format_device_name(index: usize) -> &'static str {
    match index {
        0 => "eth0",
        1 => "eth1",
        2 => "eth2",
        3 => "eth3",
        _ => "eth?",
    }
}

lazy_static::lazy_static! {
    pub static ref NETLINK: Mutex<NetlinkSubsystem> = Mutex::new(NetlinkSubsystem::new());
}
