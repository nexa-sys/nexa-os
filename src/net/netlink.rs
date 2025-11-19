use alloc::collections::VecDeque;
use alloc::vec::Vec;
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
pub const RTM_GETLINK: u16 = 18; // Get link info
pub const RTM_GETADDR: u16 = 22; // Get address info
pub const RTM_NEWLINK: u16 = 16; // New link
pub const RTM_NEWADDR: u16 = 20; // New address

/// Interface info attributes
pub const IFLA_IFNAME: u16 = 3; // Interface name
pub const IFLA_MTU: u16 = 4; // MTU
pub const IFLA_OPERSTATE: u16 = 17; // Operational state
pub const IFLA_ADDRESS: u16 = 1; // MAC address

/// Address attributes
pub const IFA_ADDRESS: u16 = 1; // IP address
pub const IFA_LABEL: u16 = 3; // Interface name label

/// Netlink message header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NlMsgHdr {
    pub nlmsg_len: u32,   // Length of message including header
    pub nlmsg_type: u16,  // Message type (RTM_*, NLMSG_*)
    pub nlmsg_flags: u16, // Flags (NLM_F_*)
    pub nlmsg_seq: u32,   // Sequence number
    pub nlmsg_pid: u32,   // Sender's PID
}

/// Rtnetlink (routing netlink) message header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfInfoMsg {
    pub ifi_family: u8, // AF_UNSPEC, AF_INET, etc.
    pub __pad: u8,
    pub ifi_type: u16,   // Interface type (ARPHRD_*)
    pub ifi_index: u32,  // Interface index
    pub ifi_flags: u32,  // Interface flags (IFF_*)
    pub ifi_change: u32, // Change mask
}

/// Attribute header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtAttr {
    pub rta_len: u16,  // Length including header
    pub rta_type: u16, // Attribute type
}

/// Address message
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfAddrMsg {
    pub ifa_family: u8,    // AF_INET, etc.
    pub ifa_prefixlen: u8, // Prefix length
    pub ifa_flags: u8,     // Flags
    pub ifa_scope: u8,     // Scope
    pub ifa_index: u32,    // Interface index
}

/// Netlink socket configuration
#[derive(Clone, Copy)]
pub struct NetlinkSocket {
    pub in_use: bool,
    pub pid: u32,    // Binding PID
    pub groups: u32, // Multicast groups
}

pub const MAX_NETLINK_SOCKETS: usize = 4;
const NETLINK_RX_QUEUE_LEN: usize = 16;
const MAX_NETLINK_PAYLOAD: usize = 4096;

#[derive(Clone)]
struct NetlinkRxEntry {
    len: usize,
    data: Vec<u8>,
}

impl NetlinkSocket {
    pub const fn empty() -> Self {
        Self {
            in_use: false,
            pid: 0,
            groups: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            in_use: true,
            pid: 0,
            groups: 0,
        }
    }
}

pub struct NetlinkSubsystem {
    sockets: [NetlinkSocket; MAX_NETLINK_SOCKETS],
    rx_queues: [VecDeque<NetlinkRxEntry>; MAX_NETLINK_SOCKETS],
    next_seq: AtomicUsize,
}

impl NetlinkSubsystem {
    pub fn new() -> Self {
        let rx_queues = core::array::from_fn(|_| VecDeque::new());

        Self {
            sockets: [NetlinkSocket::empty(); MAX_NETLINK_SOCKETS],
            rx_queues,
            next_seq: AtomicUsize::new(1),
        }
    }

    /// Create a new netlink socket
    pub fn create_socket(&mut self) -> Result<usize, NetError> {
        for (idx, socket) in self.sockets.iter_mut().enumerate() {
            if !socket.in_use {
                *socket = NetlinkSocket::new();
                crate::kinfo!("[netlink::create_socket] allocated socket index {}", idx);
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
        self.rx_queues[socket_idx].clear();
        Ok(())
    }

    /// Bind socket to PID and optional multicast groups
    pub fn bind(&mut self, socket_idx: usize, pid: u32, groups: u32) -> Result<(), NetError> {
        crate::kinfo!(
            "[netlink::bind] socket_idx={}, pid={}, groups={}",
            socket_idx,
            pid,
            groups
        );
        if socket_idx >= MAX_NETLINK_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.sockets[socket_idx].in_use {
            crate::kinfo!("[netlink::bind] socket {} not in_use", socket_idx);
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
        crate::kinfo!(
            "[netlink::bind] Socket {} bound to PID {} successfully",
            socket_idx,
            pid
        );
        Ok(())
    }

    /// Get next sequence number
    pub fn next_seq(&self) -> u32 {
        self.next_seq.fetch_add(1, Ordering::Relaxed) as u32
    }

    /// Send netlink message (queues it for the socket)
    pub fn send_message(&mut self, socket_idx: usize, message: &[u8]) -> Result<(), NetError> {
        crate::kinfo!(
            "[netlink::send_message] socket_idx={}, msg_len={}",
            socket_idx,
            message.len()
        );
        if socket_idx >= MAX_NETLINK_SOCKETS {
            crate::kinfo!("[netlink::send_message] Invalid socket index {}", socket_idx);
            return Err(NetError::InvalidSocket);
        }
        if !self.sockets[socket_idx].in_use {
            crate::kinfo!("[netlink::send_message] Socket {} not in use", socket_idx);
            return Err(NetError::InvalidSocket);
        }

        if self.rx_queues[socket_idx].len() >= NETLINK_RX_QUEUE_LEN {
            crate::kinfo!("[netlink::send_message] RX queue full (len={})", self.rx_queues[socket_idx].len());
            return Err(NetError::RxQueueFull);
        }

        // If possible, use the nlmsg_len from the header to determine message length
        let mut len = core::cmp::min(message.len(), MAX_NETLINK_PAYLOAD);
        if message.len() >= 4 {
            let hdr_len = u32::from_ne_bytes([message[0], message[1], message[2], message[3]]) as usize;
            if hdr_len == 0 || hdr_len > MAX_NETLINK_PAYLOAD {
                crate::kinfo!("[netlink::send_message] Invalid message length in header: {}", hdr_len);
                return Err(NetError::BufferTooSmall);
            }
            // Use the header length if it's less than the slice length
            len = core::cmp::min(hdr_len, len);
            // Try to extract type/seq/pid for more detailed diagnostics
            if message.len() >= 16 {
                let nlmsg_type = u16::from_ne_bytes([message[4], message[5]]);
                let nlmsg_seq = u32::from_ne_bytes([message[8], message[9], message[10], message[11]]);
                let nlmsg_pid = u32::from_ne_bytes([message[12], message[13], message[14], message[15]]);
                crate::kinfo!("[netlink::send_message] hdr_len={} type={} seq={} pid={}", hdr_len, nlmsg_type, nlmsg_seq, nlmsg_pid);
            }
        }
        let mut data = Vec::new();
        data.extend_from_slice(&message[..len]);

        self.rx_queues[socket_idx].push_back(NetlinkRxEntry { len, data });

        crate::kinfo!("[netlink::send_message] Message queued, rx_queue_len now {}", self.rx_queues[socket_idx].len());
        Ok(())
    }

    /// Receive netlink message from socket
    pub fn recv_message(
        &mut self,
        socket_idx: usize,
        buffer: &mut [u8],
    ) -> Result<usize, NetError> {
        crate::kinfo!(
            "[netlink::recv_message] socket_idx={}, buffer_len={}",
            socket_idx,
            buffer.len()
        );
        if socket_idx >= MAX_NETLINK_SOCKETS {
            crate::kinfo!("[netlink::recv_message] Invalid socket index {}", socket_idx);
            return Err(NetError::InvalidSocket);
        }
        if !self.sockets[socket_idx].in_use {
            crate::kinfo!("[netlink::recv_message] Socket {} not in use", socket_idx);
            return Err(NetError::InvalidSocket);
        }

        crate::kinfo!("[netlink::recv_message] rx_queue_len={}", self.rx_queues[socket_idx].len());

        if let Some(entry) = self.rx_queues[socket_idx].pop_front() {
            if entry.data.len() < entry.len {
                crate::kinfo!("[netlink::recv_message] WARNING: entry.data.len() < entry.len ({} < {})", entry.data.len(), entry.len);
            }
            let len = core::cmp::min(buffer.len(), entry.data.len());
            buffer[..len].copy_from_slice(&entry.data[..len]);

            crate::kinfo!("[netlink::recv_message] Returning {} bytes (entry.len={}, data_len={})", len, entry.len, entry.data.len());
            Ok(len)
        } else {
            crate::kinfo!("[netlink::recv_message] RX queue empty");
            Err(NetError::RxQueueEmpty)
        }
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

    /// Send interface info message
    pub fn send_ifinfo(
        &mut self,
        socket_idx: usize,
        seq: u32,
        dev_idx: usize,
        info: &DeviceInfo,
    ) -> Result<(), NetError> {
        let message = self.build_ifinfo_message(dev_idx, seq, info);
        // Extract the actual message length from nlmsg_len field (first 4 bytes, little-endian)
        let msg_len = u32::from_ne_bytes([message[0], message[1], message[2], message[3]]) as usize;
        let msg_len = core::cmp::min(msg_len, 256); // Safety check
        self.send_message(socket_idx, &message[..msg_len])
    }

    /// Send interface address message
    pub fn send_ifaddr(
        &mut self,
        socket_idx: usize,
        seq: u32,
        dev_idx: usize,
        info: &DeviceInfo,
    ) -> Result<(), NetError> {
        let message = self.build_ifaddr_message(dev_idx, seq, info);
        // Extract the actual message length from nlmsg_len field (first 4 bytes, little-endian)
        let msg_len = u32::from_ne_bytes([message[0], message[1], message[2], message[3]]) as usize;
        let msg_len = core::cmp::min(msg_len, 256); // Safety check
        self.send_message(socket_idx, &message[..msg_len])
    }

    /// Send DONE message
    pub fn send_done(&mut self, socket_idx: usize, seq: u32) -> Result<(), NetError> {
        let message = self.build_done_message(seq);
        self.send_message(socket_idx, &message)
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
            core::slice::from_raw_parts(
                &hdr as *const _ as *const u8,
                core::mem::size_of::<NlMsgHdr>(),
            )
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
            core::slice::from_raw_parts(
                &hdr as *const _ as *const u8,
                core::mem::size_of::<NlMsgHdr>(),
            )
        };
        if pos + core::mem::size_of::<NlMsgHdr>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<NlMsgHdr>()].copy_from_slice(hdr_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifinfo_message] buffer too small for NlMsgHdr, pos={}", pos);
            return msg;
        }
        pos += core::mem::size_of::<NlMsgHdr>();

        // Interface info message
        let ifinfo = IfInfoMsg {
            ifi_family: 0, // AF_UNSPEC
            __pad: 0,
            ifi_type: 1, // ARPHRD_ETHER
            ifi_index: (dev_idx + 1) as u32,
            ifi_flags: 0x41, // IFF_UP | IFF_RUNNING
            ifi_change: 0,
        };
        let ifinfo_bytes = unsafe {
            core::slice::from_raw_parts(
                &ifinfo as *const _ as *const u8,
                core::mem::size_of::<IfInfoMsg>(),
            )
        };
        if pos + core::mem::size_of::<IfInfoMsg>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<IfInfoMsg>()].copy_from_slice(ifinfo_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifinfo_message] buffer too small for IfInfoMsg, pos={}", pos);
            // update header length and return
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
        pos += core::mem::size_of::<IfInfoMsg>();

        // IFLA_ADDRESS attribute (MAC)
        let attr_hdr = RtAttr {
            rta_len: (core::mem::size_of::<RtAttr>() + 6) as u16,
            rta_type: IFLA_ADDRESS,
        };
        let attr_bytes = unsafe {
            core::slice::from_raw_parts(
                &attr_hdr as *const _ as *const u8,
                core::mem::size_of::<RtAttr>(),
            )
        };
        if pos + core::mem::size_of::<RtAttr>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<RtAttr>()].copy_from_slice(attr_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifinfo_message] buffer too small for RtAttr (MAC), pos={}", pos);
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
        pos += core::mem::size_of::<RtAttr>();
        if pos + 6 <= msg.len() {
            msg[pos..pos + 6].copy_from_slice(&info.mac);
            pos += 6;
        } else {
            crate::kinfo!("[netlink::build_ifinfo_message] buffer too small for MAC data, pos={}", pos);
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
        pos += 6;

        // Padding to 4-byte boundary
        // Padding to 4-byte boundary
        while pos % 4 != 0 {
            if pos < msg.len() {
                msg[pos] = 0;
                pos += 1;
            } else {
                crate::kinfo!("[netlink::build_ifinfo_message] buffer too small while padding after MAC, pos={}", pos);
                let len_bytes = (pos as u32).to_ne_bytes();
                msg[0..4].copy_from_slice(&len_bytes);
                return msg;
            }
        }

        // IFLA_IFNAME attribute
        let name = format_device_name(dev_idx);
        let mut name_len = name.len() + 1;
        let attr_hdr = RtAttr {
            rta_len: (core::mem::size_of::<RtAttr>() + name_len) as u16,
            rta_type: IFLA_IFNAME,
        };
        let attr_bytes = unsafe {
            core::slice::from_raw_parts(
                &attr_hdr as *const _ as *const u8,
                core::mem::size_of::<RtAttr>(),
            )
        };
        if pos + core::mem::size_of::<RtAttr>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<RtAttr>()].copy_from_slice(attr_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifinfo_message] buffer too small for RtAttr (IFNAME), pos={}", pos);
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
        pos += core::mem::size_of::<RtAttr>();
        // Ensure name fits - truncate if necessary
        if pos + name_len > msg.len() {
            let avail = msg.len() - pos;
            if avail == 0 {
                crate::kinfo!("[netlink::build_ifinfo_message] no room for IFNAME data, pos={}", pos);
                let len_bytes = (pos as u32).to_ne_bytes();
                msg[0..4].copy_from_slice(&len_bytes);
                return msg;
            }
            // Leave one byte for null terminator when possible
            if avail == 1 {
                // No room for name, just write null terminator
                msg[pos] = 0;
                pos += 1;
            } else {
                let write_len = avail - 1; // leave room for null
                msg[pos..pos + write_len].copy_from_slice(&name.as_bytes()[..write_len]);
                pos += write_len;
                msg[pos] = 0;
                pos += 1;
                crate::kinfo!("[netlink::build_ifinfo_message] truncated IFNAME from {} to {} bytes", name_len, write_len + 1);
            }
        } else {
            msg[pos..pos + name.len()].copy_from_slice(name.as_bytes());
            pos += name.len();
            msg[pos] = 0; // null terminate
            pos += 1;
        }

        // Update message length
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
            core::slice::from_raw_parts(
                &hdr as *const _ as *const u8,
                core::mem::size_of::<NlMsgHdr>(),
            )
        };
        if pos + core::mem::size_of::<NlMsgHdr>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<NlMsgHdr>()].copy_from_slice(hdr_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifaddr_message] buffer too small for NlMsgHdr, pos={}", pos);
            return msg;
        }
        pos += core::mem::size_of::<NlMsgHdr>();

        // Address message
        let ifaddr = IfAddrMsg {
            ifa_family: 2,     // AF_INET
            ifa_prefixlen: 24, // /24
            ifa_flags: 0x20,   // IFA_F_PERMANENT
            ifa_scope: 0,
            ifa_index: (dev_idx + 1) as u32,
        };
        let ifaddr_bytes = unsafe {
            core::slice::from_raw_parts(
                &ifaddr as *const _ as *const u8,
                core::mem::size_of::<IfAddrMsg>(),
            )
        };
        if pos + core::mem::size_of::<IfAddrMsg>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<IfAddrMsg>()].copy_from_slice(ifaddr_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifaddr_message] buffer too small for IfAddrMsg, pos={}", pos);
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
        pos += core::mem::size_of::<IfAddrMsg>();

        // IFA_ADDRESS attribute (IP)
        let attr_hdr = RtAttr {
            rta_len: (core::mem::size_of::<RtAttr>() + 4) as u16,
            rta_type: IFA_ADDRESS,
        };
        let attr_bytes = unsafe {
            core::slice::from_raw_parts(
                &attr_hdr as *const _ as *const u8,
                core::mem::size_of::<RtAttr>(),
            )
        };
        if pos + core::mem::size_of::<RtAttr>() <= msg.len() {
            msg[pos..pos + core::mem::size_of::<RtAttr>()].copy_from_slice(attr_bytes);
        } else {
            crate::kinfo!("[netlink::build_ifaddr_message] buffer too small for RtAttr, pos={}", pos);
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
        pos += core::mem::size_of::<RtAttr>();
        if pos + 4 <= msg.len() {
            msg[pos..pos + 4].copy_from_slice(&info.ip);
            pos += 4;
        } else {
            crate::kinfo!("[netlink::build_ifaddr_message] buffer too small for IP, pos={}", pos);
            let len_bytes = (pos as u32).to_ne_bytes();
            msg[0..4].copy_from_slice(&len_bytes);
            return msg;
        }
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
