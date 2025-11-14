use crate::logger;

use super::drivers::NetError;

pub const MAX_FRAME_SIZE: usize = 1536;
const TX_BATCH_CAPACITY: usize = 4;
const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const PROTO_ICMP: u8 = 1;
const PROTO_TCP: u8 = 6;
const LISTEN_PORT: u16 = 8080;

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

pub struct NetStack {
    devices: [DeviceInfo; super::MAX_NET_DEVICES],
    tcp: TcpEndpoint,
}

impl NetStack {
    pub const fn new() -> Self {
        Self {
            devices: [DeviceInfo::empty(); super::MAX_NET_DEVICES],
            tcp: TcpEndpoint::new(),
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
        if opcode != 1 {
            return Ok(());
        }

        let target_ip = &frame[38..42];
        if target_ip != device.ip {
            return Ok(());
        }

        let sender_mac = &frame[6..12];
        let sender_ip = &frame[28..32];

        let mut packet = [0u8; 42];
        packet[0..6].copy_from_slice(sender_mac);
        packet[6..12].copy_from_slice(&device.mac);
        packet[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        packet[14..22].copy_from_slice(&frame[14..22]);
        packet[20..22].copy_from_slice(&2u16.to_be_bytes());
        packet[22..28].copy_from_slice(&device.mac);
        packet[28..32].copy_from_slice(&device.ip);
        packet[32..38].copy_from_slice(sender_mac);
        packet[38..42].copy_from_slice(sender_ip);

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TcpState {
    Closed,
    SynReceived,
    Established,
    Closing,
}

struct TcpEndpoint {
    state: TcpState,
    device_idx: Option<usize>,
    local_mac: [u8; 6],
    local_ip: [u8; 4],
    peer_mac: [u8; 6],
    peer_ip: [u8; 4],
    peer_port: u16,
    snd_nxt: u32,
    snd_una: u32,
    rcv_nxt: u32,
    last_activity: u64,
}

impl TcpEndpoint {
    const fn new() -> Self {
        Self {
            state: TcpState::Closed,
            device_idx: None,
            local_mac: [0; 6],
            local_ip: [0; 4],
            peer_mac: [0; 6],
            peer_ip: [0; 4],
            peer_port: 0,
            snd_nxt: 0,
            snd_una: 0,
            rcv_nxt: 0,
            last_activity: 0,
        }
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

    fn reset(&mut self) {
        self.state = TcpState::Closed;
        self.device_idx = None;
        self.peer_mac = [0; 6];
        self.peer_ip = [0; 4];
        self.peer_port = 0;
        self.snd_nxt = 0;
        self.snd_una = 0;
        self.rcv_nxt = 0;
    }
}
