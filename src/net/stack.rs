use crate::logger;

use super::arp::{ArpCache, ArpOperation, ArpPacket};
use super::drivers::NetError;
use super::ethernet::{EtherType, EthernetFrame, MacAddress};
use super::ipv4::{IpProtocol, Ipv4Address, Ipv4Header};
use super::netlink::{NetlinkSubsystem, NetlinkSocket};
use super::udp::{UdpDatagram, UdpDatagramMut, UdpHeader};

pub const MAX_FRAME_SIZE: usize = 1536;
const TX_BATCH_CAPACITY: usize = 4;
const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const PROTO_ICMP: u8 = 1;
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;
const LISTEN_PORT: u16 = 8080;
const MAX_UDP_SOCKETS: usize = 16;
pub const UDP_MAX_PAYLOAD: usize = MAX_FRAME_SIZE - 14 - 20 - 8;
const UDP_RX_QUEUE_LEN: usize = 8;

pub struct TxBatch {
    buffers: [[u8; MAX_FRAME_SIZE]; TX_BATCH_CAPACITY],
    lengths: [usize; TX_BATCH_CAPACITY],
    count: usize,
}

impl TxBatch {
    pub const fn new() -> Self {
        Self {
            buffers: [[0u8; MAX_FRAME_SIZE]; TX_BATCH_CAPACITY],
            lengths: [0; TX_BATCH_CAPACITY],
            count: 0,
        }
    }

    pub fn push(&mut self, frame: &[u8]) -> Result<(), NetError> {
        if self.count >= TX_BATCH_CAPACITY {
            return Err(NetError::TxBusy);
        }
        if frame.len() > MAX_FRAME_SIZE {
            return Err(NetError::BufferTooSmall);
        }
        self.buffers[self.count][..frame.len()].copy_from_slice(frame);
        self.lengths[self.count] = frame.len();
        self.count += 1;
        Ok(())
    }

    pub fn frames(&self) -> impl Iterator<Item = &[u8]> {
        (0..self.count).map(move |idx| &self.buffers[idx][..self.lengths[idx]])
    }
}

#[derive(Clone, Copy)]
struct DeviceInfo {
    mac: [u8; 6],
    ip: [u8; 4],
    present: bool,
}

impl DeviceInfo {
    const fn empty() -> Self {
        Self {
            mac: [0; 6],
            ip: [0; 4],
            present: false,
        }
    }
}

/// UDP socket state
#[derive(Clone, Copy)]
pub struct UdpSocket {
    pub local_port: u16,
    pub remote_ip: Option<[u8; 4]>,
    pub remote_port: Option<u16>,
    pub in_use: bool,
    rx_head: usize,
    rx_tail: usize,
    rx_len: usize,
    rx_payloads: [[u8; UDP_MAX_PAYLOAD]; UDP_RX_QUEUE_LEN],
    rx_entries: [UdpRxEntry; UDP_RX_QUEUE_LEN],
}

#[derive(Clone, Copy)]
struct UdpRxEntry {
    len: usize,
    payload_len: usize,
    truncated: bool,
    src_ip: [u8; 4],
    src_port: u16,
}

impl UdpRxEntry {
    const fn empty() -> Self {
        Self {
            len: 0,
            payload_len: 0,
            truncated: false,
            src_ip: [0; 4],
            src_port: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UdpReceiveResult {
    pub bytes_copied: usize,
    pub payload_len: usize,
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub truncated: bool,
}

impl UdpSocket {
    pub const fn empty() -> Self {
        Self {
            local_port: 0,
            remote_ip: None,
            remote_port: None,
            in_use: false,
            rx_head: 0,
            rx_tail: 0,
            rx_len: 0,
            rx_payloads: [[0u8; UDP_MAX_PAYLOAD]; UDP_RX_QUEUE_LEN],
            rx_entries: [UdpRxEntry::empty(); UDP_RX_QUEUE_LEN],
        }
    }

    pub fn new(local_port: u16) -> Self {
        let mut socket = Self::empty();
        socket.local_port = local_port;
        socket.in_use = true;
        socket
    }

    pub fn connect(&mut self, remote_ip: [u8; 4], remote_port: u16) {
        self.remote_ip = Some(remote_ip);
        self.remote_port = Some(remote_port);
    }

    pub fn disconnect(&mut self) {
        self.remote_ip = None;
        self.remote_port = None;
    }

    pub fn reset(&mut self) {
        *self = Self::empty();
    }

    pub fn enqueue_packet(
        &mut self,
        src_ip: [u8; 4],
        src_port: u16,
        payload: &[u8],
    ) -> Result<(), super::drivers::NetError> {
        if self.rx_len >= UDP_RX_QUEUE_LEN {
            return Err(super::drivers::NetError::RxQueueFull);
        }

        let slot = self.rx_tail;
        let copy_len = core::cmp::min(payload.len(), UDP_MAX_PAYLOAD);
        self.rx_payloads[slot][..copy_len].copy_from_slice(&payload[..copy_len]);
        self.rx_entries[slot] = UdpRxEntry {
            len: copy_len,
            payload_len: payload.len(),
            truncated: payload.len() > copy_len,
            src_ip,
            src_port,
        };
        self.rx_tail = (self.rx_tail + 1) % UDP_RX_QUEUE_LEN;
        self.rx_len += 1;
        Ok(())
    }

    pub fn dequeue_packet(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<UdpReceiveResult, super::drivers::NetError> {
        if self.rx_len == 0 {
            return Err(super::drivers::NetError::RxQueueEmpty);
        }

        let slot = self.rx_head;
        let entry = self.rx_entries[slot];
        let to_copy = core::cmp::min(buffer.len(), entry.len);
        if to_copy > 0 {
            buffer[..to_copy].copy_from_slice(&self.rx_payloads[slot][..to_copy]);
        }

        self.rx_head = (self.rx_head + 1) % UDP_RX_QUEUE_LEN;
        self.rx_len -= 1;

        Ok(UdpReceiveResult {
            bytes_copied: to_copy,
            payload_len: entry.payload_len,
            src_ip: entry.src_ip,
            src_port: entry.src_port,
            truncated: entry.truncated || entry.len > buffer.len(),
        })
    }
}

pub struct NetStack {
    devices: [DeviceInfo; super::MAX_NET_DEVICES],
    tcp: TcpEndpoint,
    udp_sockets: [UdpSocket; MAX_UDP_SOCKETS],
    pub netlink: NetlinkSubsystem,
    arp_cache: ArpCache,
}

impl NetStack {
    pub const fn new() -> Self {
        Self {
            devices: [DeviceInfo::empty(); super::MAX_NET_DEVICES],
            tcp: TcpEndpoint::new(),
            udp_sockets: [UdpSocket::empty(); MAX_UDP_SOCKETS],
            netlink: NetlinkSubsystem::new(),
            arp_cache: ArpCache::new(),
        }
    }

    pub fn register_device(&mut self, index: usize, mac: [u8; 6]) {
        if index >= self.devices.len() {
            return;
        }
        let ip = default_ip(index);
        self.devices[index] = DeviceInfo {
            mac,
            ip,
            present: true,
        };
        self.tcp.register_local(index, mac, ip);
    }

    /// Allocate a UDP socket
    pub fn udp_socket(&mut self, local_port: u16) -> Result<usize, NetError> {
        // Check if port already in use
        for socket in &self.udp_sockets {
            if socket.in_use && socket.local_port == local_port {
                return Err(NetError::AddressInUse);
            }
        }

        // Find free slot
        for (idx, socket) in self.udp_sockets.iter_mut().enumerate() {
            if !socket.in_use {
                *socket = UdpSocket::new(local_port);
                return Ok(idx);
            }
        }

        Err(NetError::TooManyConnections)
    }

    /// Close UDP socket
    pub fn udp_close(&mut self, socket_idx: usize) -> Result<(), NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        self.udp_sockets[socket_idx].in_use = false;
        Ok(())
    }

    /// Connect UDP socket to remote address
    pub fn udp_connect(
        &mut self,
        socket_idx: usize,
        remote_ip: [u8; 4],
        remote_port: u16,
    ) -> Result<(), NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.udp_sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }
        self.udp_sockets[socket_idx].connect(remote_ip, remote_port);
        Ok(())
    }

    /// Disconnect UDP socket from remote address
    pub fn udp_disconnect(&mut self, socket_idx: usize) -> Result<(), NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.udp_sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }
        self.udp_sockets[socket_idx].disconnect();
        Ok(())
    }

    /// Receive UDP packet from socket
    pub fn udp_receive(
        &mut self,
        socket_idx: usize,
        buffer: &mut [u8],
    ) -> Result<UdpReceiveResult, NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        self.udp_sockets[socket_idx].dequeue_packet(buffer)
    }

    /// Get socket information
    pub fn udp_get_socket_info(&self, socket_idx: usize) -> Result<(u16, Option<u16>), NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.udp_sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }
        let socket = &self.udp_sockets[socket_idx];
        Ok((socket.local_port, socket.remote_port))
    }

    /// Check if socket has pending data
    pub fn udp_has_pending_data(&self, socket_idx: usize) -> Result<bool, NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.udp_sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }
        Ok(self.udp_sockets[socket_idx].rx_len > 0)
    }

    /// Send UDP datagram
    pub fn udp_send(
        &mut self,
        device_index: usize,
        socket_idx: usize,
        dst_ip: [u8; 4],
        dst_port: u16,
        payload: &[u8],
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        if socket_idx >= MAX_UDP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if !self.udp_sockets[socket_idx].in_use {
            return Err(NetError::InvalidSocket);
        }
        if device_index >= self.devices.len() {
            return Err(NetError::InvalidDevice);
        }
        if !self.devices[device_index].present {
            return Err(NetError::InvalidDevice);
        }

        let socket = &self.udp_sockets[socket_idx];
        let device = &self.devices[device_index];

        // Lookup destination MAC in ARP cache
        let dst_ip_addr = Ipv4Address::from(dst_ip);
        let now_ms = logger::boot_time_us() / 1_000;
        let dst_mac = self.arp_cache.lookup(&dst_ip_addr, now_ms)
            .ok_or(NetError::ArpCacheMiss)?;

        // Build UDP datagram
        let udp_len = 8 + payload.len();
        let ip_total_len = 20 + udp_len;
        let frame_len = 14 + ip_total_len;

        if frame_len > MAX_FRAME_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let mut packet = [0u8; MAX_FRAME_SIZE];

        // Ethernet header
        packet[0..6].copy_from_slice(&dst_mac.0);
        packet[6..12].copy_from_slice(&device.mac);
        packet[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

        // IPv4 header
        packet[14] = 0x45; // Version 4, IHL 5
        packet[15] = 0;    // DSCP/ECN
        packet[16..18].copy_from_slice(&(ip_total_len as u16).to_be_bytes());
        packet[18..20].copy_from_slice(&[0, 0]); // Identification
        packet[20..22].copy_from_slice(&[0x40, 0]); // Flags, Fragment offset
        packet[22] = 64;   // TTL
        packet[23] = PROTO_UDP;
        packet[24..26].copy_from_slice(&[0, 0]); // Checksum (will be filled)
        packet[26..30].copy_from_slice(&device.ip);
        packet[30..34].copy_from_slice(&dst_ip);
        let ip_checksum = checksum(&packet[14..34]);
        packet[24..26].copy_from_slice(&ip_checksum.to_be_bytes());

        // UDP header + payload
        let udp_offset = 34;
        packet[udp_offset..udp_offset + 2].copy_from_slice(&socket.local_port.to_be_bytes());
        packet[udp_offset + 2..udp_offset + 4].copy_from_slice(&dst_port.to_be_bytes());
        packet[udp_offset + 4..udp_offset + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
        packet[udp_offset + 6..udp_offset + 8].copy_from_slice(&[0, 0]); // Checksum
        packet[udp_offset + 8..udp_offset + 8 + payload.len()].copy_from_slice(payload);

        // Calculate UDP checksum with pseudo-header
        let src_ip_addr = Ipv4Address::from(device.ip);
        
        // Cast the UDP header part of the packet to UdpHeader
        let header_ptr = packet[udp_offset..].as_mut_ptr() as *mut UdpHeader;
        let header = unsafe { &mut *header_ptr };
        
        // The payload is after the header
        let payload = &packet[udp_offset + 8..udp_offset + udp_len];
        
        header.calculate_checksum(&src_ip_addr, &dst_ip_addr, payload);
        // Checksum is now set in the packet buffer because header points to it.

        tx.push(&packet[..frame_len])?;
        Ok(())
    }

    pub fn handle_frame(
        &mut self,
        device_index: usize,
        frame: &[u8],
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        if device_index >= self.devices.len() {
            return Ok(());
        }
        if !self.devices[device_index].present {
            return Ok(());
        }
        if frame.len() < 14 {
            return Ok(());
        }

        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        match ethertype {
            ETHERTYPE_ARP => self.handle_arp(device_index, frame, tx),
            ETHERTYPE_IPV4 => self.handle_ipv4(device_index, frame, tx),
            _ => Ok(()),
        }
    }

    pub fn poll_device(
        &mut self,
        device_index: usize,
        now_ms: u64,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        self.tcp.poll(device_index, now_ms, tx)
    }

    fn handle_arp(
        &mut self,
        device_index: usize,
        frame: &[u8],
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        if frame.len() < 42 {
            return Ok(());
        }
        let device = &self.devices[device_index];
        let hw_type = u16::from_be_bytes([frame[14], frame[15]]);
        let proto_type = u16::from_be_bytes([frame[16], frame[17]]);
        let hlen = frame[18] as usize;
        let plen = frame[19] as usize;
        let opcode = u16::from_be_bytes([frame[20], frame[21]]);

        if hw_type != 1 || proto_type != ETHERTYPE_IPV4 || hlen != 6 || plen != 4 {
            return Ok(());
        }

        let sender_mac = MacAddress::from(&frame[22..28]);
        let sender_ip = Ipv4Address::from(&frame[28..32]);
        let target_ip = Ipv4Address::from(&frame[38..42]);

        // Update ARP cache with sender info
        let now_ms = logger::boot_time_us() / 1_000;
        self.arp_cache.insert(sender_ip, sender_mac, now_ms);

        if opcode != 1 {
            return Ok(());
        }

        let device_ip = Ipv4Address::from(device.ip);
        if target_ip != device_ip {
            return Ok(());
        }

        // Build ARP reply
        let device_mac = MacAddress::from(device.mac);
        let arp_reply = ArpPacket::new_reply(device_mac, device_ip, sender_mac, sender_ip);

        let mut packet = [0u8; 42];
        packet[0..6].copy_from_slice(&sender_mac.0);
        packet[6..12].copy_from_slice(&device_mac.0);
        packet[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        
        // Copy ARP packet
        unsafe {
            let arp_bytes = core::slice::from_raw_parts(
                &arp_reply as *const ArpPacket as *const u8,
                ArpPacket::SIZE,
            );
            packet[14..42].copy_from_slice(arp_bytes);
        }

        tx.push(&packet)?;
        Ok(())
    }

    fn handle_ipv4(
        &mut self,
        device_index: usize,
        frame: &[u8],
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        let device = &self.devices[device_index];
        if frame.len() < 34 {
            return Ok(());
        }
        let ihl = (frame[14] & 0x0F) as usize * 4;
        if ihl < 20 || frame.len() < 14 + ihl {
            return Ok(());
        }

        let total_len = u16::from_be_bytes([frame[16], frame[17]]) as usize;
        if total_len < ihl || 14 + total_len > frame.len() {
            return Ok(());
        }

        let proto = frame[23];
        let dst_ip = &frame[30..34];
        if dst_ip != device.ip {
            return Ok(());
        }

        match proto {
            PROTO_ICMP => self.handle_icmp(device_index, frame, ihl, total_len, tx),
            PROTO_TCP => self
                .tcp
                .handle_segment(device_index, frame, ihl, total_len, tx),
            PROTO_UDP => self.handle_udp(device_index, frame, ihl, total_len),
            _ => Ok(()),
        }
    }

    fn handle_icmp(
        &mut self,
        device_index: usize,
        frame: &[u8],
        ihl: usize,
        total_len: usize,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        let device = &self.devices[device_index];
        if total_len < ihl + 8 {
            return Ok(());
        }
        let icmp_offset = 14 + ihl;
        let icmp_len = total_len - ihl;
        let icmp_type = frame[icmp_offset];
        if icmp_type != 8 {
            return Ok(());
        }

        let mut packet = [0u8; MAX_FRAME_SIZE];
        let frame_len = 14 + total_len;
        packet[..frame_len].copy_from_slice(&frame[..frame_len]);
        packet[0..6].copy_from_slice(&frame[6..12]);
        packet[6..12].copy_from_slice(&device.mac);
        packet[26..30].copy_from_slice(&device.ip);
        packet[30..34].copy_from_slice(&frame[26..30]);
        packet[icmp_offset] = 0; // Echo Reply
        packet[icmp_offset + 1] = 0;
        packet[icmp_offset + 2..icmp_offset + 4].copy_from_slice(&[0, 0]);
        let icmp_checksum = checksum(&packet[icmp_offset..icmp_offset + icmp_len]);
        packet[icmp_offset + 2..icmp_offset + 4].copy_from_slice(&icmp_checksum.to_be_bytes());
        packet[14 + 10..14 + 12].copy_from_slice(&[0, 0]);
        let ip_checksum = checksum(&packet[14..14 + ihl]);
        packet[14 + 10..14 + 12].copy_from_slice(&ip_checksum.to_be_bytes());

        tx.push(&packet[..frame_len])?;
        Ok(())
    }

    fn handle_udp(
        &mut self,
        device_index: usize,
        frame: &[u8],
        ihl: usize,
        total_len: usize,
    ) -> Result<(), NetError> {
        if total_len < ihl + 8 {
            return Ok(());
        }

        let udp_offset = 14 + ihl;
        let udp_len = total_len - ihl;
        
        if udp_len < 8 {
            return Ok(());
        }

        // Parse UDP header
        let src_port = u16::from_be_bytes([frame[udp_offset], frame[udp_offset + 1]]);
        let dst_port = u16::from_be_bytes([frame[udp_offset + 2], frame[udp_offset + 3]]);
        let length = u16::from_be_bytes([frame[udp_offset + 4], frame[udp_offset + 5]]) as usize;
        let checksum = u16::from_be_bytes([frame[udp_offset + 6], frame[udp_offset + 7]]);

        if length < 8 || length > udp_len {
            crate::kinfo!("net: UDP packet with invalid length ({} vs {})", length, udp_len);
            return Ok(());
        }

        // Verify checksum if present (0 means no checksum in IPv4)
        if checksum != 0 {
            let src_ip = Ipv4Address::from(&frame[26..30]);
            let dst_ip = Ipv4Address::from(&frame[30..34]);
            
            // Create temporary UDP header for validation
            let mut temp_header = UdpHeader::new(src_port, dst_port, length - 8);
            let payload = &frame[udp_offset + 8..udp_offset + length];
            temp_header.checksum = checksum;
            
            if !temp_header.verify_checksum(&src_ip, &dst_ip, payload) {
                crate::kwarn!(
                    "net: UDP checksum mismatch on port {} from {}.{}.{}.{}:{}",
                    dst_port,
                    frame[26], frame[27], frame[28], frame[29], src_port
                );
                return Ok(());
            }
        }

        // Find matching socket
        let mut socket_idx = None;
        for (idx, socket) in self.udp_sockets.iter().enumerate() {
            if !socket.in_use {
                continue;
            }
            if socket.local_port != dst_port {
                continue;
            }

            // Check if socket is connected to specific remote
            if let Some(remote_ip) = socket.remote_ip {
                if remote_ip != &frame[26..30] {
                    continue;
                }
            }
            if let Some(remote_port) = socket.remote_port {
                if remote_port != src_port {
                    continue;
                }
            }

            socket_idx = Some(idx);
            break;
        }

        if let Some(idx) = socket_idx {
            let payload = &frame[udp_offset + 8..udp_offset + length];
            let src_ip_bytes = [frame[26], frame[27], frame[28], frame[29]];
            
            // Attempt to enqueue packet
            match self.udp_sockets[idx].enqueue_packet(src_ip_bytes, src_port, payload) {
                Ok(()) => {
                    crate::kinfo!(
                        "net: UDP datagram received on port {}, from {}.{}.{}.{}:{} ({} bytes)",
                        dst_port,
                        frame[26], frame[27], frame[28], frame[29],
                        src_port,
                        payload.len()
                    );
                }
                Err(e) => {
                    crate::kwarn!(
                        "net: Failed to queue UDP packet on port {} ({:?})",
                        dst_port,
                        e
                    );
                }
            }
        } else {
            crate::kinfo!(
                "net: UDP packet to port {} from {}.{}.{}.{}:{} (no matching socket)",
                dst_port,
                frame[26], frame[27], frame[28], frame[29],
                src_port
            );
        }

        Ok(())
    }

    /// Create a netlink socket
    pub fn netlink_socket(&mut self) -> Result<usize, NetError> {
        self.netlink.create_socket()
    }

    /// Close a netlink socket
    pub fn netlink_close(&mut self, socket_idx: usize) -> Result<(), NetError> {
        self.netlink.close_socket(socket_idx)
    }

    /// Bind a netlink socket
    pub fn netlink_bind(&mut self, socket_idx: usize, pid: u32, groups: u32) -> Result<(), NetError> {
        self.netlink.bind(socket_idx, pid, groups)
    }

    /// Send a netlink message (from user to kernel)
    /// This processes the request and queues the response
    pub fn netlink_send(&mut self, socket_idx: usize, data: &[u8]) -> Result<(), NetError> {
        self.netlink_handle_request(socket_idx, data)
    }

    /// Receive a netlink message (from kernel to user)
    pub fn netlink_receive(&mut self, socket_idx: usize, buffer: &mut [u8]) -> Result<usize, NetError> {
        self.netlink.recv_message(socket_idx, buffer)
    }

    /// Handle netlink request from userspace
    pub fn netlink_handle_request(&mut self, socket_idx: usize, data: &[u8]) -> Result<(), NetError> {
        use super::netlink::{
            NlMsgHdr, IfInfoMsg, IfAddrMsg, 
            NLMSG_DONE, RTM_GETLINK, RTM_GETADDR, RTM_NEWLINK, RTM_NEWADDR,
            IFLA_IFNAME, IFLA_MTU, IFLA_OPERSTATE, IFLA_ADDRESS,
            IFA_ADDRESS, IFA_LABEL
        };

        if data.len() < core::mem::size_of::<NlMsgHdr>() {
            return Err(NetError::InvalidPacket);
        }

        let hdr = unsafe { &*(data.as_ptr() as *const NlMsgHdr) };
        
        match hdr.nlmsg_type {
            RTM_GETLINK => {
                // Send info for all devices
                for (i, dev) in self.devices.iter().enumerate() {
                    if !dev.present { continue; }
                    
                    // Construct response... 
                    // For now, just a placeholder implementation
                    // In a real implementation, we would construct the full netlink message
                    // with attributes for each interface.
                }
                // Send DONE message
            }
            RTM_GETADDR => {
                // Send address info
            }
            _ => {}
        }
        
        Ok(())
    }

    /// Get device information for netlink queries
    pub fn get_device_info(&self, index: usize) -> Option<super::netlink::DeviceInfo> {
        if index >= self.devices.len() {
            return None;
        }
        let device = &self.devices[index];
        if !device.present {
            return None;
        }
        Some(super::netlink::DeviceInfo {
            mac: device.mac,
            ip: device.ip,
            present: device.present,
        })
    }
}

fn default_ip(index: usize) -> [u8; 4] {
    let last = 15 + (index as u8);
    [10, 0, 2, last]
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for chunk in chunks.by_ref() {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if let Some(&byte) = chunks.remainder().get(0) {
        sum += (byte as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn checksum_with_initial(data: &[u8], initial: u32) -> u16 {
    let mut sum = initial;
    let mut chunks = data.chunks_exact(2);
    for chunk in chunks.by_ref() {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if let Some(&byte) = chunks.remainder().get(0) {
        sum += (byte as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn tcp_checksum(src_ip: &[u8], dst_ip: &[u8], segment: &[u8]) -> u16 {
    let mut pseudo = [0u8; 12];
    pseudo[0..4].copy_from_slice(src_ip);
    pseudo[4..8].copy_from_slice(dst_ip);
    pseudo[8] = 0;
    pseudo[9] = PROTO_TCP;
    pseudo[10..12].copy_from_slice(&(segment.len() as u16).to_be_bytes());

    let mut sum = checksum(&pseudo);
    checksum_with_initial(segment, sum as u32)
}

#[derive(Clone, Copy, PartialEq)]
enum TcpState {
    Closed,
    Listen,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
}

struct TcpEndpoint {
    state: TcpState,
    device_idx: Option<usize>,
    local_mac: [u8; 6],
    local_ip: [u8; 4],
    peer_mac: [u8; 6],
    peer_ip: [u8; 4],
    peer_port: u16,
    seq: u32,
    ack: u32,
    snd_una: u32,
    snd_nxt: u32,
    rcv_nxt: u32,
    last_activity: u64,
}

impl TcpEndpoint {
    const fn new() -> Self {
        Self {
            state: TcpState::Listen,
            device_idx: None,
            local_mac: [0; 6],
            local_ip: [0; 4],
            peer_mac: [0; 6],
            peer_ip: [0; 4],
            peer_port: 0,
            seq: 0,
            ack: 0,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            last_activity: 0,
        }
    }

    fn reset(&mut self) {
        self.state = TcpState::Listen;
        self.peer_mac = [0; 6];
        self.peer_ip = [0; 4];
        self.peer_port = 0;
        self.seq = 0;
        self.ack = 0;
        self.snd_una = 0;
        self.snd_nxt = 0;
        self.rcv_nxt = 0;
    }

    fn register_local(&mut self, device_idx: usize, mac: [u8; 6], ip: [u8; 4]) {
        if self.device_idx.is_none() {
            self.device_idx = Some(device_idx);
        }
        self.local_mac = mac;
        self.local_ip = ip;
    }

    fn poll(&mut self, device_idx: usize, now_ms: u64, _tx: &mut TxBatch) -> Result<(), NetError> {
        if Some(device_idx) != self.device_idx {
            return Ok(());
        }
        if matches!(self.state, TcpState::Closing)
            && now_ms.saturating_sub(self.last_activity) > 5_000
        {
            self.reset();
        }
        Ok(())
    }

    fn handle_segment(
        &mut self,
        device_idx: usize,
        frame: &[u8],
        ihl: usize,
        total_len: usize,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        if total_len < ihl + 20 {
            return Ok(());
        }

        let tcp_offset = 14 + ihl;
        let header = &frame[tcp_offset..tcp_offset + 20];
        let src_port = u16::from_be_bytes([header[0], header[1]]);
        let dst_port = u16::from_be_bytes([header[2], header[3]]);
        if dst_port != LISTEN_PORT {
            return Ok(());
        }

        let seq = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        let ack = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
        let data_offset = ((header[12] >> 4) * 4) as usize;
        if total_len < ihl + data_offset {
            return Ok(());
        }

        let flags = header[13];
        let payload = &frame[tcp_offset + data_offset..14 + total_len];
        self.last_activity = logger::boot_time_us() / 1_000;

        if self.state == TcpState::Closed {
            if flags & 0x02 != 0 {
                self.accept_syn(device_idx, frame, src_port, seq, tx)?;
            }
            return Ok(());
        }

        if Some(device_idx) != self.device_idx {
            return Ok(());
        }
        if src_port != self.peer_port {
            return Ok(());
        }

        if flags & 0x04 != 0 {
            self.reset();
            return Ok(());
        }

        if flags & 0x10 != 0 {
            self.snd_una = ack;
            if self.state == TcpState::SynReceived && ack == self.snd_nxt {
                self.state = TcpState::Established;
            } else if self.state == TcpState::Closing && ack == self.snd_nxt {
                self.reset();
                return Ok(());
            }
        }

        if !payload.is_empty() {
            if seq == self.rcv_nxt {
                self.rcv_nxt = self.rcv_nxt.wrapping_add(payload.len() as u32);
                self.send_packet(payload, 0x18, tx)?;
            } else {
                self.send_packet(&[], 0x10, tx)?;
            }
        } else if flags & 0x10 != 0 {
            self.send_packet(&[], 0x10, tx)?;
        }

        let fin_seq = seq.wrapping_add(payload.len() as u32);
        if flags & 0x01 != 0 && fin_seq == self.rcv_nxt {
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            self.send_packet(&[], 0x10, tx)?;
            self.send_packet(&[], 0x11, tx)?;
            self.state = TcpState::Closing;
        }

        Ok(())
    }

    fn accept_syn(
        &mut self,
        device_idx: usize,
        frame: &[u8],
        src_port: u16,
        seq: u32,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        self.state = TcpState::SynReceived;
        self.device_idx = Some(device_idx);
        self.peer_mac.copy_from_slice(&frame[6..12]);
        self.peer_ip.copy_from_slice(&frame[26..30]);
        self.peer_port = src_port;
        self.rcv_nxt = seq.wrapping_add(1);
        self.snd_nxt = self.generate_iss();
        self.snd_una = self.snd_nxt;
        self.send_packet(&[], 0x12, tx)?;
        Ok(())
    }

    fn send_packet(&mut self, payload: &[u8], flags: u8, tx: &mut TxBatch) -> Result<(), NetError> {
        if self.device_idx.is_none() {
            return Ok(());
        }

        let ip_header_len = 20;
        let tcp_header_len = 20;
        let tcp_offset = 14 + ip_header_len;
        let total_len = 14 + ip_header_len + tcp_header_len + payload.len();
        if total_len > MAX_FRAME_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let mut packet = [0u8; MAX_FRAME_SIZE];
        packet[0..6].copy_from_slice(&self.peer_mac);
        packet[6..12].copy_from_slice(&self.local_mac);
        packet[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

        packet[14] = 0x45;
        packet[15] = 0;
        packet[16..18].copy_from_slice(
            &((ip_header_len + tcp_header_len + payload.len()) as u16).to_be_bytes(),
        );
        packet[18..20].copy_from_slice(&[0, 0]);
        packet[20..22].copy_from_slice(&[0x40, 0]);
        packet[22] = 64;
        packet[23] = PROTO_TCP;
        packet[24..26].copy_from_slice(&[0, 0]);
        packet[26..30].copy_from_slice(&self.local_ip);
        packet[30..34].copy_from_slice(&self.peer_ip);
        let ip_checksum = checksum(&packet[14..34]);
        packet[24..26].copy_from_slice(&ip_checksum.to_be_bytes());

        packet[tcp_offset..tcp_offset + 2].copy_from_slice(&LISTEN_PORT.to_be_bytes());
        packet[tcp_offset + 2..tcp_offset + 4].copy_from_slice(&self.peer_port.to_be_bytes());
        packet[tcp_offset + 4..tcp_offset + 8].copy_from_slice(&self.snd_nxt.to_be_bytes());
        packet[tcp_offset + 8..tcp_offset + 12].copy_from_slice(&self.rcv_nxt.to_be_bytes());
        packet[tcp_offset + 12] = (tcp_header_len as u8 / 4) << 4;
        packet[tcp_offset + 13] = flags;
        packet[tcp_offset + 14..tcp_offset + 16].copy_from_slice(&0x4000u16.to_be_bytes());
        packet[tcp_offset + 16..tcp_offset + 18].copy_from_slice(&[0, 0]);
        packet[tcp_offset + 18..tcp_offset + 20].copy_from_slice(&[0, 0]);
        packet[tcp_offset + 20..tcp_offset + 20 + payload.len()].copy_from_slice(payload);

        let checksum = tcp_checksum(
            &self.local_ip,
            &self.peer_ip,
            &packet[tcp_offset..tcp_offset + tcp_header_len + payload.len()],
        );
        packet[tcp_offset + 16..tcp_offset + 18].copy_from_slice(&checksum.to_be_bytes());

        tx.push(&packet[..total_len])?;

        let mut advance = payload.len() as u32;
        if flags & 0x01 != 0 || flags & 0x02 != 0 {
            advance = advance.wrapping_add(1);
        }
        if advance != 0 {
            self.snd_nxt = self.snd_nxt.wrapping_add(advance);
        }
        Ok(())
    }

    fn generate_iss(&self) -> u32 {
        (logger::boot_time_us() as u32) ^ 0x1357_9BDF
    }
}
