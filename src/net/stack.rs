use crate::logger;
use crate::serial;

use super::arp::{ArpCache, ArpOperation, ArpPacket};
use super::drivers::NetError;
use super::ethernet::{EtherType, EthernetFrame, MacAddress};
use super::ipv4::{IpProtocol, Ipv4Address, Ipv4Header};
use super::netlink::{NetlinkSubsystem, NetlinkSocket};
use super::tcp::TcpSocket;
use super::udp::{UdpDatagram, UdpDatagramMut, UdpHeader};
use crate::process::Pid;

pub const MAX_FRAME_SIZE: usize = 1536;
const TX_BATCH_CAPACITY: usize = 4;
const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const PROTO_ICMP: u8 = 1;
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;
const LISTEN_PORT: u16 = 8080;
const MAX_UDP_SOCKETS: usize = 16;
const MAX_TCP_SOCKETS: usize = 16;
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

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy)]
struct DeviceInfo {
    mac: [u8; 6],
    ip: [u8; 4],
    gateway: [u8; 4],
    present: bool,
}

impl DeviceInfo {
    const fn empty() -> Self {
        Self {
            mac: [0; 6],
            ip: [0; 4],
            gateway: [0, 0, 0, 0],
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

// Pending packet waiting for ARP resolution
struct PendingPacket {
    device_index: usize,
    socket_index: usize,
    dst_ip: [u8; 4],
    dst_port: u16,
    payload: [u8; UDP_MAX_PAYLOAD],
    payload_len: usize,
    timestamp_ms: u64,
}

const MAX_PENDING_PACKETS: usize = 8;

pub struct NetStack {
    devices: [DeviceInfo; super::MAX_NET_DEVICES],
    tcp: TcpEndpoint,
    tcp_sockets: [TcpSocket; MAX_TCP_SOCKETS],
    udp_sockets: [UdpSocket; MAX_UDP_SOCKETS],
    pub netlink: NetlinkSubsystem,
    arp_cache: ArpCache,
    pending_packets: [Option<PendingPacket>; MAX_PENDING_PACKETS],
}

impl NetStack {
    pub const fn new() -> Self {
        let mut devices = [DeviceInfo::empty(); super::MAX_NET_DEVICES];
        // Register default network devices
        // For QEMU virtio-net, we typically have eth0
        // Start with 0.0.0.0 to allow DHCP discovery before IP assignment
        devices[0] = DeviceInfo {
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56], // QEMU default MAC prefix
            ip: [0, 0, 0, 0], // No IP yet, will be set by DHCP
            gateway: [192, 168, 3, 2], // QEMU user-mode networking default gateway
            present: true,
        };

        let mut tcp = TcpEndpoint::new();
        tcp.device_idx = Some(0);
        tcp.local_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        tcp.local_ip = [0, 0, 0, 0]; // No IP yet, will be set by DHCP

        Self {
            devices,
            tcp,
            tcp_sockets: [
                TcpSocket::new(), TcpSocket::new(), TcpSocket::new(), TcpSocket::new(),
                TcpSocket::new(), TcpSocket::new(), TcpSocket::new(), TcpSocket::new(),
                TcpSocket::new(), TcpSocket::new(), TcpSocket::new(), TcpSocket::new(),
                TcpSocket::new(), TcpSocket::new(), TcpSocket::new(), TcpSocket::new(),
            ],
            udp_sockets: [UdpSocket::empty(); MAX_UDP_SOCKETS],
            netlink: NetlinkSubsystem::new(),
            arp_cache: ArpCache::new(),
            pending_packets: [None, None, None, None, None, None, None, None],
        }
    }

    pub fn register_device(&mut self, index: usize, mac: [u8; 6]) {
        if index >= self.devices.len() {
            return;
        }
        // Preserve existing IP if already set (e.g., by DHCP or initial config)
        // Only use default IP if device is not yet registered
        let ip = if self.devices[index].present {
            self.devices[index].ip
        } else {
            [0, 0, 0, 0] // Start with 0.0.0.0, will be set by DHCP
        };
        let gateway = if self.devices[index].present {
            self.devices[index].gateway
        } else {
            [192, 168, 3, 2] // Default QEMU gateway
        };
        self.devices[index] = DeviceInfo {
            mac,
            ip,
            gateway,
            present: true,
        };
        self.tcp.register_local(index, mac, ip);
    }

    /// Send an ARP request for the given IP address
    fn send_arp_request(&mut self, device_index: usize, target_ip: Ipv4Address, tx: &mut TxBatch) -> Result<(), NetError> {
        if device_index >= self.devices.len() {
            return Err(NetError::InvalidDevice);
        }
        if !self.devices[device_index].present {
            return Err(NetError::InvalidDevice);
        }

        let device = &self.devices[device_index];
        let device_mac = MacAddress::from(device.mac);
        let device_ip = Ipv4Address::from(device.ip);

        crate::serial::_print(format_args!("[ARP] Sending ARP request: who has {}? Tell {}\n", target_ip, device_ip));
        crate::kinfo!("[ARP] Sending ARP request: who has {}? Tell {}", target_ip, device_ip);

        let arp_request = ArpPacket::new_request(device_mac, device_ip, target_ip);

        let mut packet = [0u8; 42];
        // Ethernet header: broadcast destination
        packet[0..6].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        packet[6..12].copy_from_slice(&device.mac);
        packet[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        
        // Copy ARP packet
        unsafe {
            let arp_bytes = core::slice::from_raw_parts(
                &arp_request as *const ArpPacket as *const u8,
                ArpPacket::SIZE,
            );
            packet[14..42].copy_from_slice(arp_bytes);
        }

        tx.push(&packet)?;
        crate::kinfo!("[ARP] ARP request queued for transmission");
        Ok(())
    }

    /// Set gateway for a device
    pub fn set_gateway(&mut self, device_index: usize, gateway: [u8; 4]) {
        if device_index < self.devices.len() && self.devices[device_index].present {
            self.devices[device_index].gateway = gateway;
        }
    }

    /// Process packets waiting for ARP resolution
    fn process_pending_packets(
        &mut self,
        _device_index: usize,
        resolved_ip: Ipv4Address,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        serial::_print(format_args!("[NetStack] Processing pending packets for {}\n", resolved_ip));
        let now_ms = logger::boot_time_us() / 1_000;
        let resolved_ip_bytes = [resolved_ip.0[0], resolved_ip.0[1], resolved_ip.0[2], resolved_ip.0[3]];

        // First pass: mark packets to send (avoid double borrow)
        let mut send_flags = [false; MAX_PENDING_PACKETS];
        
        for (i, slot) in self.pending_packets.iter().enumerate() {
            if let Some(ref pkt) = slot {
                // Check if this packet is waiting for the resolved IP
                let device = &self.devices[pkt.device_index];
                let target_ip = if pkt.dst_ip[0] == device.ip[0] && pkt.dst_ip[1] == device.ip[1] && pkt.dst_ip[2] == device.ip[2] {
                    pkt.dst_ip
                } else if device.gateway != [0, 0, 0, 0] {
                    device.gateway
                } else {
                    pkt.dst_ip
                };

                if target_ip == resolved_ip_bytes {
                    serial::_print(format_args!("[NetStack] Queued packet matched for {}:{}\n", 
                        Ipv4Address::from(pkt.dst_ip), pkt.dst_port));
                    send_flags[i] = true;
                } else if now_ms.saturating_sub(pkt.timestamp_ms) >= 5000 {
                    // Mark expired packets for removal
                    send_flags[i] = false;
                }
            }
        }
        
        // Second pass: send and remove packets
        for (i, should_send) in send_flags.iter().enumerate() {
            if *should_send {
                if let Some(pkt) = self.pending_packets[i].take() {
                    serial::_print(format_args!("[NetStack] Sending queued packet to {}:{}\n", 
                        Ipv4Address::from(pkt.dst_ip), pkt.dst_port));
                    let _ = self.udp_send(
                        pkt.device_index,
                        pkt.socket_index,
                        pkt.dst_ip,
                        pkt.dst_port,
                        &pkt.payload[..pkt.payload_len],
                        tx,
                    );
                }
            }
        }
        
        Ok(())
    }

    /// Allocate a UDP socket
    /// Check if a UDP port is available without allocating it
    pub fn is_udp_port_available(&self, local_port: u16) -> bool {
        !self.udp_sockets.iter().any(|s| s.in_use && s.local_port == local_port)
    }

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

    /// Create a new TCP socket
    pub fn tcp_socket(&mut self) -> Result<usize, NetError> {
        for (idx, socket) in self.tcp_sockets.iter_mut().enumerate() {
            if !socket.in_use {
                return Ok(idx);
            }
        }
        Err(NetError::TooManyConnections)
    }

    /// Connect TCP socket to remote address
    pub fn tcp_connect(
        &mut self,
        socket_idx: usize,
        device_index: usize,
        remote_ip: [u8; 4],
        remote_port: u16,
        local_port: u16,
    ) -> Result<(), NetError> {
        serial::_print(format_args!(
            "[tcp_connect] socket_idx={}, device_index={}, remote={}:{}\n",
            socket_idx, device_index, 
            crate::net::ipv4::Ipv4Address::from(remote_ip), remote_port
        ));
        
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        if device_index >= self.devices.len() || !self.devices[device_index].present {
            return Err(NetError::InvalidDevice);
        }

        let device = &self.devices[device_index];
        let socket = &mut self.tcp_sockets[socket_idx];
        
        serial::_print(format_args!(
            "[tcp_connect] Device info: ip={}.{}.{}.{}, mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, gateway={}.{}.{}.{}\n",
            device.ip[0], device.ip[1], device.ip[2], device.ip[3],
            device.mac[0], device.mac[1], device.mac[2], device.mac[3], device.mac[4], device.mac[5],
            device.gateway[0], device.gateway[1], device.gateway[2], device.gateway[3]
        ));
        
        serial::_print(format_args!(
            "[tcp_connect] Before connect: socket state={:?}, in_use={}\n",
            socket.state, socket.in_use
        ));

        let result = socket.connect(
            Ipv4Address::from(device.ip),
            local_port,
            Ipv4Address::from(remote_ip),
            remote_port,
            MacAddress(device.mac),
            device_index,
        );
        
        serial::_print(format_args!(
            "[tcp_connect] After connect: result={:?}, socket state={:?}, in_use={}\n",
            result, socket.state, socket.in_use
        ));
        
        // Resolve gateway MAC address via ARP before sending SYN
        if result.is_err() {
            return result;
        }
        
        let gateway_ip = Ipv4Address::from(device.gateway);
        let current_ms = (logger::boot_time_us() / 1000) as u64;
        serial::_print(format_args!(
            "[tcp_connect] Resolving gateway MAC for {}\n", gateway_ip
        ));
        
        // Try to get MAC from ARP cache
        let gateway_mac = if let Some(mac) = self.arp_cache.lookup(&gateway_ip, current_ms) {
            serial::_print(format_args!(
                "[tcp_connect] Found gateway MAC in cache: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]
            ));
            mac
        } else {
            serial::_print(format_args!(
                "[tcp_connect] Gateway MAC not in cache\n"
            ));
            // For now, assume gateway is on local network and will respond to ARP
            // In real implementation, we should send ARP request and wait
            // But for testing, let's just fail gracefully
            return Err(NetError::ArpCacheMiss);
        };
        
        // Set the remote MAC to gateway MAC
        self.tcp_sockets[socket_idx].remote_mac = gateway_mac;
        
        // Send initial SYN packet immediately by calling poll
        let mut tx_batch = TxBatch::new();
        if let Err(e) = self.tcp_sockets[socket_idx].poll(&mut tx_batch) {
            serial::_print(format_args!(
                "[tcp_connect] ERROR: poll failed: {:?}\n", e
            ));
            return Err(e);
        }
        
        serial::_print(format_args!(
            "[tcp_connect] Poll successful, {} frames to send\n", 
            tx_batch.count
        ));
        
        // Send the frames
        if tx_batch.count > 0 {
            crate::net::send_frames(device_index, &tx_batch).ok();
        }
        
        // Return success - connection initiated
        Ok(())
    }

    /// Send data on TCP socket
    pub fn tcp_send(
        &mut self,
        socket_idx: usize,
        data: &[u8],
    ) -> Result<usize, NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        
        let socket = &mut self.tcp_sockets[socket_idx];
        serial::_print(format_args!(
            "[tcp_send] socket_idx={}, state={:?}, in_use={}, data_len={}\n",
            socket_idx, socket.state, socket.in_use, data.len()
        ));
        
        self.tcp_sockets[socket_idx].send(data)
    }

    /// Receive data from TCP socket
    pub fn tcp_recv(
        &mut self,
        socket_idx: usize,
        buffer: &mut [u8],
    ) -> Result<usize, NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        self.tcp_sockets[socket_idx].recv(buffer)
    }

    /// Close TCP socket
    pub fn tcp_close(&mut self, socket_idx: usize) -> Result<(), NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        self.tcp_sockets[socket_idx].close()
    }

    /// Check if TCP socket is connected
    pub fn tcp_is_connected(&self, socket_idx: usize) -> Result<bool, NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        Ok(self.tcp_sockets[socket_idx].state == super::tcp::TcpState::Established)
    }

    /// Check if TCP socket has data available
    pub fn tcp_has_data(&self, socket_idx: usize) -> Result<bool, NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        Ok(self.tcp_sockets[socket_idx].has_data())
    }

    /// Get TCP socket state
    pub fn tcp_get_state(&self, socket_idx: usize) -> Result<super::tcp::TcpState, NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        Ok(self.tcp_sockets[socket_idx].state)
    }

    /// Register a process to wait for data on a TCP socket
    pub fn tcp_wait(&mut self, socket_idx: usize, pid: Pid) -> Result<(), NetError> {
        if socket_idx >= MAX_TCP_SOCKETS {
            return Err(NetError::InvalidSocket);
        }
        self.tcp_sockets[socket_idx].wait_queue.push(pid);
        Ok(())
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

        // Check if this is a broadcast address (global or subnet-specific)
        let is_broadcast = dst_ip == [255, 255, 255, 255] || 
                          (dst_ip[3] == 255 && device.ip != [0, 0, 0, 0]);
        
        // Determine destination MAC
        let dst_mac = if is_broadcast {
            // Use broadcast MAC address for broadcast IPs
            MacAddress([0xff, 0xff, 0xff, 0xff, 0xff, 0xff])
        } else {
            // For unicast, determine the actual target IP for ARP lookup
            // If destination is not on local subnet, use gateway
            let target_ip = if dst_ip[0] == device.ip[0] && dst_ip[1] == device.ip[1] && dst_ip[2] == device.ip[2] {
                // Same /24 subnet - direct communication
                dst_ip
            } else if device.gateway != [0, 0, 0, 0] {
                // Different subnet - route through gateway
                device.gateway
            } else {
                // No gateway configured, try direct anyway
                dst_ip
            };

            let target_ip_addr = Ipv4Address::from(target_ip);
            let now_ms = logger::boot_time_us() / 1_000;
            
            // Try to lookup in ARP cache
            match self.arp_cache.lookup(&target_ip_addr, now_ms) {
                Some(mac) => mac,
                None => {
                    // ARP cache miss - queue packet and send ARP request
                    if payload.len() <= UDP_MAX_PAYLOAD {
                        // Find free slot in pending queue
                        for slot in self.pending_packets.iter_mut() {
                            if slot.is_none() {
                                let mut pkt_payload = [0u8; UDP_MAX_PAYLOAD];
                                pkt_payload[..payload.len()].copy_from_slice(payload);
                                *slot = Some(PendingPacket {
                                    device_index,
                                    socket_index: socket_idx,
                                    dst_ip,
                                    dst_port,
                                    payload: pkt_payload,
                                    payload_len: payload.len(),
                                    timestamp_ms: now_ms,
                                });
                                serial::_print(format_args!("[NetStack] Queued packet waiting for ARP\n"));
                                break;
                            }
                        }
                    }
                    self.send_arp_request(device_index, target_ip_addr, tx)?;
                    return Err(NetError::ArpCacheMiss);
                }
            }
        };

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
        let dst_ip_addr = Ipv4Address::from(dst_ip);
        
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
        // Debug: Log received frames
        static FRAME_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let count = FRAME_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        
        if count < 10 || count % 50 == 0 {
            crate::serial::_print(format_args!(
                "[stack::handle_frame] Frame #{}, device={}, len={}\n",
                count, device_index, frame.len()
            ));
        }
        
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
        
        if count < 10 {
            crate::serial::_print(format_args!(
                "[stack::handle_frame] Ethertype={:#x}\n",
                ethertype
            ));
        }
        
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
        // Poll old TCP endpoint for backwards compatibility
        self.tcp.poll(device_index, now_ms, tx)?;
        
        // Poll all TCP sockets
        for socket in &mut self.tcp_sockets {
            if socket.in_use && socket.device_idx == Some(device_index) {
                socket.poll(tx)?;
            }
        }
        
        Ok(())
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
        // Extract values we need before any mutable operations
        let device_mac = self.devices[device_index].mac;
        let device_ip_bytes = self.devices[device_index].ip;
        
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
        
        serial::_print(format_args!(
            "[ARP] Received {} from {}: MAC={}\n",
            if opcode == 1 { "request" } else if opcode == 2 { "reply" } else { "unknown" },
            sender_ip,
            sender_mac
        ));

        // If this is an ARP reply, check for pending packets
        if opcode == 2 {
            self.process_pending_packets(device_index, sender_ip, tx)?;
        }

        if opcode != 1 {
            return Ok(());
        }

        let device_ip = Ipv4Address::from(device_ip_bytes);
        if target_ip != device_ip {
            return Ok(());
        }

        // Build ARP reply
        let device_mac = MacAddress::from(device_mac);
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
        
        // Accept packets destined to:
        // 1. Our IP address
        // 2. Broadcast address (255.255.255.255)
        // 3. Any address if our IP is 0.0.0.0 (for DHCP before we have an IP)
        // 4. Subnet broadcast (e.g., x.x.x.255 for /24 networks)
        let is_global_broadcast = dst_ip == [255, 255, 255, 255];
        let is_our_ip = dst_ip == device.ip;
        let no_ip_yet = device.ip == [0, 0, 0, 0];
        
        // Check if it's a subnet broadcast (last octet is 255 for /24 networks)
        // This handles broadcasts like 10.0.2.255, 192.168.3.255, etc.
        let is_subnet_broadcast = dst_ip[3] == 255 && 
                                  dst_ip[0] == device.ip[0] && 
                                  dst_ip[1] == device.ip[1] && 
                                  dst_ip[2] == device.ip[2];
        
        if !is_our_ip && !is_global_broadcast && !is_subnet_broadcast && !no_ip_yet {
            crate::serial::_print(format_args!(
                "[handle_ipv4] Dropping packet: dst_ip={}.{}.{}.{}, device_ip={}.{}.{}.{}\n",
                dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3],
                device.ip[0], device.ip[1], device.ip[2], device.ip[3]
            ));
            return Ok(());
        }
        
        crate::serial::_print(format_args!(
            "[handle_ipv4] Accepting packet: dst_ip={}.{}.{}.{}, proto={}, our_ip={}.{}.{}.{}\n",
            dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3], proto,
            device.ip[0], device.ip[1], device.ip[2], device.ip[3]
        ));

        match proto {
            PROTO_ICMP => self.handle_icmp(device_index, frame, ihl, total_len, tx),
            PROTO_TCP => {
                crate::serial::_print(format_args!(
                    "[handle_ipv4] TCP packet received, forwarding to TCP handlers\n"
                ));
                // First handle the old TCP endpoint (for backwards compatibility)
                self.tcp.handle_segment(device_index, frame, ihl, total_len, tx)?;
                
                // Then handle new TCP sockets
                self.handle_tcp(device_index, frame, ihl, total_len, tx)
            }
            PROTO_UDP => self.handle_udp(device_index, frame, ihl, total_len),
            _ => {
                crate::serial::_print(format_args!(
                    "[handle_ipv4] Unknown protocol: {}\n", proto
                ));
                Ok(())
            }
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

        let src_ip = &frame[26..30];
        let dst_ip = &frame[30..34];
        crate::serial::_print(format_args!(
            "[handle_udp] UDP: {}.{}.{}.{}:{} -> {}.{}.{}.{}:{}, len={}, checksum={:#x}\n",
            src_ip[0], src_ip[1], src_ip[2], src_ip[3], src_port,
            dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3], dst_port,
            length, checksum
        ));

        if length < 8 || length > udp_len {
            crate::kinfo!("net: UDP packet with invalid length ({} vs {})", length, udp_len);
            return Ok(());
        }

        // Verify checksum if present (0 means no checksum in IPv4)
        if checksum != 0 {
            let src_ip = Ipv4Address::from(&frame[26..30]);
            let dst_ip = Ipv4Address::from(&frame[30..34]);
            
            // Parse UDP header directly from frame to avoid byte order confusion
            // Frame already contains network byte order data
            if udp_offset + length <= frame.len() {
                let udp_packet_raw = &frame[udp_offset..udp_offset + length];
                
                // Parse header as-is from network data
                let header_ptr = udp_packet_raw.as_ptr() as *const UdpHeader;
                let udp_header = unsafe { &*header_ptr };
                let payload = &udp_packet_raw[UdpHeader::SIZE..];
                
                if !udp_header.verify_checksum(&src_ip, &dst_ip, payload) {
                    crate::serial::_print(format_args!(
                        "[handle_udp] Checksum mismatch on port {} from {}.{}.{}.{}:{}\n",
                        dst_port,
                        frame[26], frame[27], frame[28], frame[29], src_port
                    ));
                    return Ok(());
                }
            } else {
                return Ok(()); // Invalid length
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

    fn handle_tcp(
        &mut self,
        device_index: usize,
        frame: &[u8],
        ihl: usize,
        total_len: usize,
        tx: &mut TxBatch,
    ) -> Result<(), NetError> {
        if total_len < ihl + 20 {
            return Ok(());
        }

        let tcp_offset = 14 + ihl;
        let tcp_data = &frame[tcp_offset..14 + total_len];
        
        // Extract source IP and MAC, and parse TCP header for debugging
        let src_ip = Ipv4Address::from(&frame[26..30]);
        let src_mac = MacAddress([frame[6], frame[7], frame[8], frame[9], frame[10], frame[11]]);
        let src_port = u16::from_be_bytes([tcp_data[0], tcp_data[1]]);
        let dst_port = u16::from_be_bytes([tcp_data[2], tcp_data[3]]);
        let flags = tcp_data[13];

        serial::_print(format_args!(
            "[handle_tcp] Received TCP packet: {}:{} -> port {}, flags={:02x}, len={}\n",
            src_ip, src_port, dst_port, flags, tcp_data.len()
        ));

        // Try to find matching socket
        let mut found = false;
        for (idx, socket) in self.tcp_sockets.iter_mut().enumerate() {
            if !socket.in_use {
                serial::_print(format_args!(
                    "[handle_tcp] Socket {} not in use, skipping\n", idx
                ));
                continue;
            }
            
            serial::_print(format_args!(
                "[handle_tcp] Socket {}: in_use=true, device_idx={:?}, local_port={}, remote_ip={}, remote_port={}, state={:?}\n",
                idx, socket.device_idx, socket.local_port, socket.remote_ip, socket.remote_port, socket.state
            ));
            
            if socket.device_idx != Some(device_index) {
                serial::_print(format_args!(
                    "[handle_tcp] Socket {} device mismatch: expected {:?}, got {}\n",
                    idx, socket.device_idx, device_index
                ));
                continue;
            }

            socket.process_segment(src_ip, src_mac, tcp_data, tx)?;
            found = true;
            break; // Only process on first matching socket
        }

        if !found {
            serial::_print(format_args!(
                "[handle_tcp] No matching socket found for {}:{} -> port {}\n",
                src_ip, src_port, dst_port
            ));
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
        crate::serial::_print(format_args!("[netlink_send] socket_idx={}, data_len={}\n", socket_idx, data.len()));
        let result = self.netlink_handle_request(socket_idx, data);
        crate::serial::_print(format_args!("[netlink_send] result={:?}\n", result));
        result
    }

    /// Receive a netlink message (from kernel to user)
    pub fn netlink_receive(&mut self, socket_idx: usize, buffer: &mut [u8]) -> Result<usize, NetError> {
        self.netlink.recv_message(socket_idx, buffer)
    }

    /// Handle netlink request from userspace
    pub fn netlink_handle_request(&mut self, socket_idx: usize, data: &[u8]) -> Result<(), NetError> {
        use super::netlink::{
            NlMsgHdr, IfAddrMsg, RtAttr,
            RTM_GETLINK, RTM_GETADDR, RTM_NEWADDR,
            IFA_ADDRESS
        };

        crate::serial::_print(format_args!("[netlink_handle_request] socket_idx={}, data_len={}\n", socket_idx, data.len()));

        if data.len() < core::mem::size_of::<NlMsgHdr>() {
            crate::serial::_print(format_args!("[netlink_handle_request] Data too short for NlMsgHdr\n"));
            return Err(NetError::InvalidPacket);
        }

        let hdr = unsafe { &*(data.as_ptr() as *const NlMsgHdr) };
        crate::kinfo!("[netlink_handle_request] Message type: {}", hdr.nlmsg_type);
        
        crate::serial::_print(format_args!("[netlink_handle_request] nlmsg_type={}\n", hdr.nlmsg_type));
        
        match hdr.nlmsg_type {
            RTM_GETLINK => {
                crate::serial::_print(format_args!("[netlink_handle_request] RTM_GETLINK received\n"));
                // Send info for all devices
                for (i, dev) in self.devices.iter().enumerate() {
                    if !dev.present { continue; }
                    
                    crate::kinfo!("[netlink_handle_request] Sending ifinfo for device {}", i);
                    let info = super::netlink::DeviceInfo {
                        mac: dev.mac,
                        ip: dev.ip,
                        present: dev.present,
                    };
                    self.netlink.send_ifinfo(socket_idx, hdr.nlmsg_seq, i, &info)?;
                }
                crate::kinfo!("[netlink_handle_request] Sending DONE message");
                self.netlink.send_done(socket_idx, hdr.nlmsg_seq)?;
            }
            RTM_GETADDR => {
                crate::serial::_print(format_args!("[netlink_handle_request] RTM_GETADDR received\n"));
                let mut addr_count = 0;
                // Send address info only for devices with configured IP
                for (i, dev) in self.devices.iter().enumerate() {
                    if !dev.present { continue; }
                    
                    // Skip devices without IP addresses (0.0.0.0)
                    if dev.ip == [0, 0, 0, 0] {
                        crate::serial::_print(format_args!("[netlink_handle_request] Device {} has no IP (0.0.0.0), skipping\n", i));
                        continue;
                    }
                    
                    crate::serial::_print(format_args!("[netlink_handle_request] Sending addr for dev {}: IP={}.{}.{}.{}\n", 
                        i, dev.ip[0], dev.ip[1], dev.ip[2], dev.ip[3]));
                    
                    let info = super::netlink::DeviceInfo {
                        mac: dev.mac,
                        ip: dev.ip,
                        present: dev.present,
                    };
                    self.netlink.send_ifaddr(socket_idx, hdr.nlmsg_seq, i, &info)?;
                    addr_count += 1;
                }
                crate::serial::_print(format_args!("[netlink_handle_request] Sent {} addresses, sending DONE\n", addr_count));
                self.netlink.send_done(socket_idx, hdr.nlmsg_seq)?;
            }
            RTM_NEWADDR => {
                crate::serial::_print(format_args!("[netlink_handle_request] RTM_NEWADDR received\n"));
                // Parse IfAddrMsg
                if data.len() < core::mem::size_of::<NlMsgHdr>() + core::mem::size_of::<IfAddrMsg>() {
                    return Err(NetError::InvalidPacket);
                }
                let ifaddr = unsafe { &*(data.as_ptr().add(core::mem::size_of::<NlMsgHdr>()) as *const IfAddrMsg) };
                let dev_idx = ifaddr.ifa_index as usize;
                if dev_idx == 0 || dev_idx > self.devices.len() {
                    return Err(NetError::InvalidDevice);
                }
                let real_dev_idx = dev_idx - 1;

                // Parse attributes
                let mut pos = core::mem::size_of::<NlMsgHdr>() + core::mem::size_of::<IfAddrMsg>();
                while pos + core::mem::size_of::<RtAttr>() <= hdr.nlmsg_len as usize {
                    let attr = unsafe { &*(data.as_ptr().add(pos) as *const RtAttr) };
                    let attr_len = attr.rta_len as usize;
                    if pos + attr_len > hdr.nlmsg_len as usize {
                        break;
                    }

                    if attr.rta_type == IFA_ADDRESS {
                        if attr_len >= 4 + 4 { // Header + IPv4
                            let ip_ptr = unsafe { data.as_ptr().add(pos + 4) };
                            let mut ip = [0u8; 4];
                            unsafe { core::ptr::copy_nonoverlapping(ip_ptr, ip.as_mut_ptr(), 4) };
                            
                            // Update IP
                            if self.devices[real_dev_idx].present {
                                self.devices[real_dev_idx].ip = ip;
                                self.tcp.register_local(real_dev_idx, self.devices[real_dev_idx].mac, ip);
                                crate::serial::_print(format_args!("Netlink: Set IP for eth{} to {}.{}.{}.{}\n", 
                                    real_dev_idx, ip[0], ip[1], ip[2], ip[3]));
                            }
                        }
                    }
                    
                    pos += (attr_len + 3) & !3; // Align to 4 bytes
                }
            }
            _ => {
                crate::kinfo!("[netlink_handle_request] Unknown message type: {}", hdr.nlmsg_type);
            }
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

    let sum = checksum(&pseudo);
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
