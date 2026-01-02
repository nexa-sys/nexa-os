//! Virtual Networking
//!
//! This module provides enterprise networking features including:
//! - Virtual switches (vSwitch) with VLANs
//! - Virtual NICs with QoS
//! - SR-IOV support
//! - Network isolation and security groups
//! - Distributed virtual switching

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Instant, Duration};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::core::{VmId, HypervisorError, HypervisorResult};

// ============================================================================
// Network Manager
// ============================================================================

/// Central network manager
pub struct NetworkManager {
    /// Virtual switches
    switches: RwLock<HashMap<VSwitchId, Arc<VirtualSwitch>>>,
    /// Virtual NICs
    vnics: RwLock<HashMap<VNicId, Arc<VirtualNic>>>,
    /// Port groups
    port_groups: RwLock<HashMap<PortGroupId, Arc<PortGroup>>>,
    /// Security groups
    security_groups: RwLock<HashMap<SecurityGroupId, Arc<SecurityGroup>>>,
    /// Network policies
    policies: RwLock<Vec<NetworkPolicy>>,
    /// Configuration
    config: RwLock<NetworkConfig>,
    /// Statistics
    stats: RwLock<NetworkStats>,
    /// ID generators
    next_switch_id: AtomicU64,
    next_vnic_id: AtomicU64,
    next_port_group_id: AtomicU64,
    next_security_group_id: AtomicU64,
}

/// Virtual switch identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VSwitchId(u64);

impl VSwitchId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Virtual NIC identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VNicId(u64);

impl VNicId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Port group identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortGroupId(u64);

impl PortGroupId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Security group identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecurityGroupId(u64);

impl SecurityGroupId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Network configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Enable promiscuous mode
    pub allow_promiscuous: bool,
    /// Enable MAC address changes
    pub allow_mac_changes: bool,
    /// Enable forged transmits
    pub allow_forged_transmits: bool,
    /// Default MTU
    pub default_mtu: u32,
    /// Enable jumbo frames
    pub jumbo_frames: bool,
    /// Default VLAN
    pub default_vlan: u16,
    /// Enable network virtualization (VXLAN/NVGRE)
    pub network_virtualization: bool,
    /// VXLAN port
    pub vxlan_port: u16,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            allow_promiscuous: false,
            allow_mac_changes: false,
            allow_forged_transmits: false,
            default_mtu: 1500,
            jumbo_frames: false,
            default_vlan: 1,
            network_virtualization: true,
            vxlan_port: 4789,
        }
    }
}

/// Network statistics
#[derive(Debug, Clone, Default)]
pub struct NetworkStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
}

impl NetworkManager {
    pub fn new() -> Self {
        Self {
            switches: RwLock::new(HashMap::new()),
            vnics: RwLock::new(HashMap::new()),
            port_groups: RwLock::new(HashMap::new()),
            security_groups: RwLock::new(HashMap::new()),
            policies: RwLock::new(Vec::new()),
            config: RwLock::new(NetworkConfig::default()),
            stats: RwLock::new(NetworkStats::default()),
            next_switch_id: AtomicU64::new(1),
            next_vnic_id: AtomicU64::new(1),
            next_port_group_id: AtomicU64::new(1),
            next_security_group_id: AtomicU64::new(1),
        }
    }
    
    /// Configure network
    pub fn configure(&self, config: NetworkConfig) {
        *self.config.write().unwrap() = config;
    }
    
    // ========== Virtual Switch Management ==========
    
    /// Create virtual switch
    pub fn create_switch(&self, name: &str, spec: VSwitchSpec) -> HypervisorResult<VSwitchId> {
        let id = VSwitchId::new(self.next_switch_id.fetch_add(1, Ordering::SeqCst));
        
        let switch = Arc::new(VirtualSwitch::new(id, name.to_string(), spec));
        self.switches.write().unwrap().insert(id, switch);
        
        Ok(id)
    }
    
    /// Delete virtual switch
    pub fn delete_switch(&self, id: VSwitchId) -> HypervisorResult<()> {
        let switch = self.get_switch(id)?;
        
        // Check if switch has connected ports
        if switch.port_count() > 0 {
            return Err(HypervisorError::NetworkError(
                "Cannot delete switch with connected ports".to_string()
            ));
        }
        
        self.switches.write().unwrap().remove(&id);
        Ok(())
    }
    
    /// Add uplink to switch
    pub fn add_uplink(&self, switch_id: VSwitchId, uplink: UplinkSpec) -> HypervisorResult<()> {
        let switch = self.get_switch(switch_id)?;
        switch.add_uplink(uplink)?;
        Ok(())
    }
    
    // ========== Virtual NIC Management ==========
    
    /// Create virtual NIC
    pub fn create_vnic(&self, spec: VNicSpec) -> HypervisorResult<VNicId> {
        let id = VNicId::new(self.next_vnic_id.fetch_add(1, Ordering::SeqCst));
        let config = self.config.read().unwrap();
        
        let mac = spec.mac_address.unwrap_or_else(|| generate_mac_address(id.as_u64()));
        
        let vnic = Arc::new(VirtualNic::new(
            id,
            spec.name.clone(),
            mac,
            spec.switch_id,
            spec.port_group_id,
            spec.mtu.unwrap_or(config.default_mtu),
        ));
        
        // Connect to switch if specified
        if let Some(switch_id) = spec.switch_id {
            let switch = self.get_switch(switch_id)?;
            switch.connect_port(id)?;
        }
        
        self.vnics.write().unwrap().insert(id, vnic);
        
        Ok(id)
    }
    
    /// Delete virtual NIC
    pub fn delete_vnic(&self, id: VNicId) -> HypervisorResult<()> {
        let vnic = self.get_vnic(id)?;
        
        // Disconnect from switch
        if let Some(switch_id) = vnic.switch_id {
            if let Ok(switch) = self.get_switch(switch_id) {
                switch.disconnect_port(id);
            }
        }
        
        self.vnics.write().unwrap().remove(&id);
        Ok(())
    }
    
    /// Attach vNIC to VM
    pub fn attach_vnic(&self, vnic_id: VNicId, vm_id: VmId) -> HypervisorResult<()> {
        let vnic = self.get_vnic(vnic_id)?;
        
        if vnic.attached_to.read().unwrap().is_some() {
            return Err(HypervisorError::NetworkError(
                "vNIC already attached".to_string()
            ));
        }
        
        *vnic.attached_to.write().unwrap() = Some(vm_id);
        Ok(())
    }
    
    /// Detach vNIC from VM
    pub fn detach_vnic(&self, vnic_id: VNicId) -> HypervisorResult<()> {
        let vnic = self.get_vnic(vnic_id)?;
        *vnic.attached_to.write().unwrap() = None;
        Ok(())
    }
    
    // ========== Port Group Management ==========
    
    /// Create port group
    pub fn create_port_group(&self, name: &str, spec: PortGroupSpec) -> HypervisorResult<PortGroupId> {
        let id = PortGroupId::new(self.next_port_group_id.fetch_add(1, Ordering::SeqCst));
        
        let port_group = Arc::new(PortGroup::new(id, name.to_string(), spec));
        self.port_groups.write().unwrap().insert(id, port_group);
        
        Ok(id)
    }
    
    /// Delete port group
    pub fn delete_port_group(&self, id: PortGroupId) -> HypervisorResult<()> {
        self.port_groups.write().unwrap().remove(&id);
        Ok(())
    }
    
    // ========== Security Group Management ==========
    
    /// Create security group
    pub fn create_security_group(&self, name: &str) -> HypervisorResult<SecurityGroupId> {
        let id = SecurityGroupId::new(self.next_security_group_id.fetch_add(1, Ordering::SeqCst));
        
        let sg = Arc::new(SecurityGroup::new(id, name.to_string()));
        self.security_groups.write().unwrap().insert(id, sg);
        
        Ok(id)
    }
    
    /// Add rule to security group
    pub fn add_security_rule(
        &self,
        sg_id: SecurityGroupId,
        rule: SecurityRule,
    ) -> HypervisorResult<()> {
        let sg = self.get_security_group(sg_id)?;
        sg.add_rule(rule);
        Ok(())
    }
    
    /// Apply security group to vNIC
    pub fn apply_security_group(&self, vnic_id: VNicId, sg_id: SecurityGroupId) -> HypervisorResult<()> {
        let vnic = self.get_vnic(vnic_id)?;
        vnic.add_security_group(sg_id);
        Ok(())
    }
    
    // ========== Packet Processing ==========
    
    /// Process incoming packet
    pub fn process_rx_packet(&self, vnic_id: VNicId, packet: &[u8]) -> HypervisorResult<()> {
        let vnic = self.get_vnic(vnic_id)?;
        
        // Apply security rules
        if !self.check_security_rules(&vnic, packet, Direction::Ingress)? {
            self.stats.write().unwrap().rx_dropped += 1;
            return Ok(());
        }
        
        // Enqueue packet
        vnic.rx_queue.lock().unwrap().push_back(packet.to_vec());
        
        self.stats.write().unwrap().rx_packets += 1;
        self.stats.write().unwrap().rx_bytes += packet.len() as u64;
        
        Ok(())
    }
    
    /// Process outgoing packet
    pub fn process_tx_packet(&self, vnic_id: VNicId, packet: &[u8]) -> HypervisorResult<()> {
        let vnic = self.get_vnic(vnic_id)?;
        
        // Apply security rules
        if !self.check_security_rules(&vnic, packet, Direction::Egress)? {
            self.stats.write().unwrap().tx_dropped += 1;
            return Ok(());
        }
        
        // Apply QoS
        if let Some(ref qos) = *vnic.qos.read().unwrap() {
            if !qos.allow_packet(packet.len()) {
                self.stats.write().unwrap().tx_dropped += 1;
                return Ok(());
            }
        }
        
        // Forward to switch
        if let Some(switch_id) = vnic.switch_id {
            let switch = self.get_switch(switch_id)?;
            switch.forward_packet(vnic_id, packet)?;
        }
        
        self.stats.write().unwrap().tx_packets += 1;
        self.stats.write().unwrap().tx_bytes += packet.len() as u64;
        
        Ok(())
    }
    
    fn check_security_rules(
        &self,
        vnic: &VirtualNic,
        packet: &[u8],
        direction: Direction,
    ) -> HypervisorResult<bool> {
        let sg_ids = vnic.security_groups.read().unwrap();
        
        for &sg_id in sg_ids.iter() {
            if let Ok(sg) = self.get_security_group(sg_id) {
                if !sg.check_packet(packet, direction) {
                    return Ok(false);
                }
            }
        }
        
        Ok(true)
    }
    
    /// Get statistics
    pub fn stats(&self) -> NetworkStats {
        self.stats.read().unwrap().clone()
    }
    
    fn get_switch(&self, id: VSwitchId) -> HypervisorResult<Arc<VirtualSwitch>> {
        self.switches.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::NetworkError(format!("Switch {} not found", id.0)))
    }
    
    fn get_vnic(&self, id: VNicId) -> HypervisorResult<Arc<VirtualNic>> {
        self.vnics.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::NetworkError(format!("vNIC {} not found", id.0)))
    }
    
    fn get_security_group(&self, id: SecurityGroupId) -> HypervisorResult<Arc<SecurityGroup>> {
        self.security_groups.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::NetworkError(format!("Security group {} not found", id.0)))
    }
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Virtual Switch
// ============================================================================

/// Virtual switch (vSwitch)
pub struct VirtualSwitch {
    id: VSwitchId,
    name: String,
    spec: VSwitchSpec,
    /// Connected ports (vNIC ID -> port)
    ports: RwLock<HashMap<VNicId, Port>>,
    /// Uplinks (physical NIC connections)
    uplinks: RwLock<Vec<Uplink>>,
    /// MAC address table
    mac_table: RwLock<HashMap<MacAddress, VNicId>>,
    /// VLAN database
    vlan_db: RwLock<HashMap<u16, VlanInfo>>,
    /// Statistics
    stats: RwLock<SwitchStats>,
}

/// Virtual switch specification
#[derive(Debug, Clone)]
pub struct VSwitchSpec {
    /// Switch type
    pub switch_type: SwitchType,
    /// Number of ports
    pub num_ports: u32,
    /// MTU
    pub mtu: u32,
    /// Enable spanning tree
    pub spanning_tree: bool,
    /// Enable IGMP snooping
    pub igmp_snooping: bool,
    /// Enable NetFlow/sFlow
    pub flow_export: bool,
    /// Teaming policy
    pub teaming_policy: TeamingPolicy,
}

impl Default for VSwitchSpec {
    fn default() -> Self {
        Self {
            switch_type: SwitchType::Standard,
            num_ports: 128,
            mtu: 1500,
            spanning_tree: true,
            igmp_snooping: true,
            flow_export: false,
            teaming_policy: TeamingPolicy::LoadBalance,
        }
    }
}

/// Switch type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchType {
    /// Standard vSwitch
    Standard,
    /// Distributed vSwitch (VMware vDS style)
    Distributed,
    /// Open vSwitch
    Ovs,
}

/// Teaming policy for uplinks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamingPolicy {
    /// Active-standby failover
    Failover,
    /// Load balance based on source port ID
    LoadBalance,
    /// Load balance based on source MAC hash
    LoadBalanceMac,
    /// Load balance based on IP hash
    LoadBalanceIp,
    /// LACP (802.3ad)
    Lacp,
}

/// Switch port
#[derive(Debug, Clone)]
pub struct Port {
    vnic_id: VNicId,
    vlan: u16,
    trunk_vlans: Vec<u16>,
    mode: PortMode,
    enabled: bool,
}

/// Port mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortMode {
    Access,
    Trunk,
}

/// Physical uplink
#[derive(Debug, Clone)]
pub struct Uplink {
    pub name: String,
    pub pnic: String,
    pub active: bool,
    pub speed: u64,
    pub duplex: Duplex,
}

/// Uplink specification
#[derive(Debug, Clone)]
pub struct UplinkSpec {
    pub name: String,
    pub pnic: String,
}

/// Link duplex
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplex {
    Half,
    Full,
}

/// VLAN information
#[derive(Debug, Clone)]
pub struct VlanInfo {
    pub id: u16,
    pub name: String,
    pub ports: Vec<VNicId>,
}

/// Switch statistics
#[derive(Debug, Clone, Default)]
pub struct SwitchStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub broadcast_packets: u64,
    pub multicast_packets: u64,
    pub unknown_unicast: u64,
}

impl VirtualSwitch {
    pub fn new(id: VSwitchId, name: String, spec: VSwitchSpec) -> Self {
        Self {
            id,
            name,
            spec,
            ports: RwLock::new(HashMap::new()),
            uplinks: RwLock::new(Vec::new()),
            mac_table: RwLock::new(HashMap::new()),
            vlan_db: RwLock::new(HashMap::new()),
            stats: RwLock::new(SwitchStats::default()),
        }
    }
    
    pub fn id(&self) -> VSwitchId { self.id }
    pub fn name(&self) -> &str { &self.name }
    
    pub fn port_count(&self) -> usize {
        self.ports.read().unwrap().len()
    }
    
    pub fn connect_port(&self, vnic_id: VNicId) -> HypervisorResult<()> {
        let mut ports = self.ports.write().unwrap();
        
        if ports.len() >= self.spec.num_ports as usize {
            return Err(HypervisorError::NetworkError(
                "Maximum ports reached".to_string()
            ));
        }
        
        ports.insert(vnic_id, Port {
            vnic_id,
            vlan: 1,
            trunk_vlans: Vec::new(),
            mode: PortMode::Access,
            enabled: true,
        });
        
        Ok(())
    }
    
    pub fn disconnect_port(&self, vnic_id: VNicId) {
        self.ports.write().unwrap().remove(&vnic_id);
        
        // Remove from MAC table
        self.mac_table.write().unwrap().retain(|_, &mut v| v != vnic_id);
    }
    
    pub fn add_uplink(&self, spec: UplinkSpec) -> HypervisorResult<()> {
        let uplink = Uplink {
            name: spec.name,
            pnic: spec.pnic,
            active: true,
            speed: 10_000_000_000, // 10Gbps default
            duplex: Duplex::Full,
        };
        
        self.uplinks.write().unwrap().push(uplink);
        Ok(())
    }
    
    pub fn configure_port_vlan(&self, vnic_id: VNicId, vlan: u16) -> HypervisorResult<()> {
        let mut ports = self.ports.write().unwrap();
        
        if let Some(port) = ports.get_mut(&vnic_id) {
            port.vlan = vlan;
            Ok(())
        } else {
            Err(HypervisorError::NetworkError("Port not found".to_string()))
        }
    }
    
    /// Forward packet to destination
    pub fn forward_packet(&self, src_vnic: VNicId, packet: &[u8]) -> HypervisorResult<()> {
        // Extract destination MAC
        if packet.len() < 14 {
            return Err(HypervisorError::NetworkError("Packet too small".to_string()));
        }
        
        let dst_mac = MacAddress::from_slice(&packet[0..6]);
        let src_mac = MacAddress::from_slice(&packet[6..12]);
        
        // Learn source MAC
        self.mac_table.write().unwrap().insert(src_mac, src_vnic);
        
        // Check if broadcast/multicast
        if dst_mac.is_broadcast() {
            self.flood_packet(src_vnic, packet)?;
            self.stats.write().unwrap().broadcast_packets += 1;
        } else if dst_mac.is_multicast() {
            self.flood_packet(src_vnic, packet)?;
            self.stats.write().unwrap().multicast_packets += 1;
        } else {
            // Unicast - lookup MAC table
            let mac_table = self.mac_table.read().unwrap();
            if let Some(&dst_vnic) = mac_table.get(&dst_mac) {
                // Forward to specific port
                drop(mac_table);
                self.send_to_port(dst_vnic, packet)?;
            } else {
                // Unknown unicast - flood
                drop(mac_table);
                self.flood_packet(src_vnic, packet)?;
                self.stats.write().unwrap().unknown_unicast += 1;
            }
        }
        
        Ok(())
    }
    
    fn flood_packet(&self, src_vnic: VNicId, packet: &[u8]) -> HypervisorResult<()> {
        let ports = self.ports.read().unwrap();
        
        for (&vnic_id, port) in ports.iter() {
            if vnic_id != src_vnic && port.enabled {
                // Would send to port in real implementation
            }
        }
        
        Ok(())
    }
    
    fn send_to_port(&self, vnic_id: VNicId, _packet: &[u8]) -> HypervisorResult<()> {
        let ports = self.ports.read().unwrap();
        
        if let Some(port) = ports.get(&vnic_id) {
            if port.enabled {
                // Would send packet in real implementation
            }
        }
        
        Ok(())
    }
}

// ============================================================================
// Virtual NIC
// ============================================================================

/// Virtual network interface
pub struct VirtualNic {
    id: VNicId,
    name: String,
    mac_address: MacAddress,
    switch_id: Option<VSwitchId>,
    port_group_id: Option<PortGroupId>,
    attached_to: RwLock<Option<VmId>>,
    mtu: u32,
    enabled: AtomicBool,
    /// RX queue
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// TX queue
    tx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// QoS settings
    qos: RwLock<Option<QosPolicy>>,
    /// Security groups
    security_groups: RwLock<Vec<SecurityGroupId>>,
    /// Statistics
    stats: RwLock<VNicStats>,
}

/// Virtual NIC specification
#[derive(Debug, Clone)]
pub struct VNicSpec {
    /// NIC name
    pub name: String,
    /// MAC address (auto-generated if None)
    pub mac_address: Option<MacAddress>,
    /// Connected switch
    pub switch_id: Option<VSwitchId>,
    /// Port group
    pub port_group_id: Option<PortGroupId>,
    /// MTU
    pub mtu: Option<u32>,
    /// NIC model
    pub model: NicModel,
}

impl Default for VNicSpec {
    fn default() -> Self {
        Self {
            name: "eth0".to_string(),
            mac_address: None,
            switch_id: None,
            port_group_id: None,
            mtu: None,
            model: NicModel::VirtioNet,
        }
    }
}

/// NIC model
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicModel {
    /// Virtio network
    VirtioNet,
    /// Intel E1000
    E1000,
    /// Intel E1000E
    E1000e,
    /// VMware VMXNET3
    Vmxnet3,
    /// Realtek RTL8139
    Rtl8139,
}

/// Virtual NIC statistics
#[derive(Debug, Clone, Default)]
pub struct VNicStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
}

impl VirtualNic {
    pub fn new(
        id: VNicId,
        name: String,
        mac_address: MacAddress,
        switch_id: Option<VSwitchId>,
        port_group_id: Option<PortGroupId>,
        mtu: u32,
    ) -> Self {
        Self {
            id,
            name,
            mac_address,
            switch_id,
            port_group_id,
            attached_to: RwLock::new(None),
            mtu,
            enabled: AtomicBool::new(true),
            rx_queue: Mutex::new(VecDeque::new()),
            tx_queue: Mutex::new(VecDeque::new()),
            qos: RwLock::new(None),
            security_groups: RwLock::new(Vec::new()),
            stats: RwLock::new(VNicStats::default()),
        }
    }
    
    pub fn id(&self) -> VNicId { self.id }
    pub fn name(&self) -> &str { &self.name }
    pub fn mac_address(&self) -> MacAddress { self.mac_address }
    pub fn mtu(&self) -> u32 { self.mtu }
    
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }
    
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }
    
    pub fn set_qos(&self, policy: QosPolicy) {
        *self.qos.write().unwrap() = Some(policy);
    }
    
    pub fn add_security_group(&self, sg_id: SecurityGroupId) {
        self.security_groups.write().unwrap().push(sg_id);
    }
    
    pub fn remove_security_group(&self, sg_id: SecurityGroupId) {
        self.security_groups.write().unwrap().retain(|&id| id != sg_id);
    }
    
    /// Receive packet from queue
    pub fn recv(&self) -> Option<Vec<u8>> {
        self.rx_queue.lock().unwrap().pop_front()
    }
    
    /// Send packet to TX queue
    pub fn send(&self, packet: Vec<u8>) -> HypervisorResult<()> {
        self.tx_queue.lock().unwrap().push_back(packet);
        Ok(())
    }
    
    pub fn stats(&self) -> VNicStats {
        self.stats.read().unwrap().clone()
    }
}

// ============================================================================
// MAC Address
// ============================================================================

/// MAC address
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    pub fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
    
    pub fn from_slice(slice: &[u8]) -> Self {
        let mut bytes = [0u8; 6];
        bytes.copy_from_slice(&slice[..6]);
        Self(bytes)
    }
    
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
    
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    }
    
    pub fn is_multicast(&self) -> bool {
        (self.0[0] & 0x01) != 0 && !self.is_broadcast()
    }
    
    pub fn is_local(&self) -> bool {
        (self.0[0] & 0x02) != 0
    }
}

impl std::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5])
    }
}

/// Generate a MAC address from an ID
pub fn generate_mac_address(id: u64) -> MacAddress {
    // Use locally administered, unicast address
    MacAddress([
        0x52, 0x54, 0x00, // QEMU OUI with local bit
        ((id >> 16) & 0xff) as u8,
        ((id >> 8) & 0xff) as u8,
        (id & 0xff) as u8,
    ])
}

// ============================================================================
// Port Group
// ============================================================================

/// Port group (network profile)
pub struct PortGroup {
    id: PortGroupId,
    name: String,
    spec: PortGroupSpec,
    members: RwLock<Vec<VNicId>>,
}

/// Port group specification
#[derive(Debug, Clone)]
pub struct PortGroupSpec {
    /// VLAN ID
    pub vlan: u16,
    /// VLAN trunking
    pub trunk_vlans: Vec<u16>,
    /// Security policy
    pub security: PortGroupSecurity,
    /// Traffic shaping
    pub traffic_shaping: Option<TrafficShaping>,
}

impl Default for PortGroupSpec {
    fn default() -> Self {
        Self {
            vlan: 1,
            trunk_vlans: Vec::new(),
            security: PortGroupSecurity::default(),
            traffic_shaping: None,
        }
    }
}

/// Port group security settings
#[derive(Debug, Clone)]
pub struct PortGroupSecurity {
    pub allow_promiscuous: bool,
    pub allow_mac_changes: bool,
    pub allow_forged_transmits: bool,
}

impl Default for PortGroupSecurity {
    fn default() -> Self {
        Self {
            allow_promiscuous: false,
            allow_mac_changes: false,
            allow_forged_transmits: false,
        }
    }
}

/// Traffic shaping policy
#[derive(Debug, Clone)]
pub struct TrafficShaping {
    pub enabled: bool,
    pub average_bandwidth: u64,
    pub peak_bandwidth: u64,
    pub burst_size: u64,
}

impl PortGroup {
    pub fn new(id: PortGroupId, name: String, spec: PortGroupSpec) -> Self {
        Self {
            id,
            name,
            spec,
            members: RwLock::new(Vec::new()),
        }
    }
    
    pub fn id(&self) -> PortGroupId { self.id }
    pub fn name(&self) -> &str { &self.name }
    pub fn vlan(&self) -> u16 { self.spec.vlan }
    
    pub fn add_member(&self, vnic_id: VNicId) {
        self.members.write().unwrap().push(vnic_id);
    }
    
    pub fn remove_member(&self, vnic_id: VNicId) {
        self.members.write().unwrap().retain(|&id| id != vnic_id);
    }
}

// ============================================================================
// QoS Policy
// ============================================================================

/// QoS policy for traffic management
pub struct QosPolicy {
    /// Rate limit (bytes/sec)
    rate_limit: u64,
    /// Burst size (bytes)
    burst_size: u64,
    /// Token bucket
    tokens: AtomicU64,
    /// Last update time
    last_update: RwLock<Instant>,
    /// Priority (0-7, higher = more important)
    priority: u8,
}

impl QosPolicy {
    pub fn new(rate_limit: u64, burst_size: u64, priority: u8) -> Self {
        Self {
            rate_limit,
            burst_size,
            tokens: AtomicU64::new(burst_size),
            last_update: RwLock::new(Instant::now()),
            priority,
        }
    }
    
    /// Check if packet is allowed (token bucket)
    pub fn allow_packet(&self, packet_size: usize) -> bool {
        self.refill_tokens();
        
        let needed = packet_size as u64;
        let current = self.tokens.load(Ordering::SeqCst);
        
        if current >= needed {
            self.tokens.fetch_sub(needed, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
    
    fn refill_tokens(&self) {
        let mut last_update = self.last_update.write().unwrap();
        let elapsed = last_update.elapsed();
        *last_update = Instant::now();
        
        let new_tokens = (elapsed.as_secs_f64() * self.rate_limit as f64) as u64;
        let current = self.tokens.load(Ordering::SeqCst);
        let new_total = (current + new_tokens).min(self.burst_size);
        self.tokens.store(new_total, Ordering::SeqCst);
    }
}

// ============================================================================
// Security Group
// ============================================================================

/// Security group (firewall rules)
pub struct SecurityGroup {
    id: SecurityGroupId,
    name: String,
    rules: RwLock<Vec<SecurityRule>>,
}

/// Security rule
#[derive(Debug, Clone)]
pub struct SecurityRule {
    /// Rule direction
    pub direction: Direction,
    /// Protocol
    pub protocol: Protocol,
    /// Source (CIDR or security group)
    pub source: RuleTarget,
    /// Destination (CIDR or security group)
    pub destination: RuleTarget,
    /// Port range
    pub port_range: Option<(u16, u16)>,
    /// Action
    pub action: RuleAction,
    /// Priority (lower = higher priority)
    pub priority: u32,
}

/// Rule direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ingress,
    Egress,
}

/// Protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Any,
    Tcp,
    Udp,
    Icmp,
    Icmpv6,
}

/// Rule target
#[derive(Debug, Clone)]
pub enum RuleTarget {
    Any,
    Cidr(String),
    SecurityGroup(SecurityGroupId),
}

/// Rule action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Allow,
    Deny,
}

impl SecurityGroup {
    pub fn new(id: SecurityGroupId, name: String) -> Self {
        Self {
            id,
            name,
            rules: RwLock::new(Vec::new()),
        }
    }
    
    pub fn id(&self) -> SecurityGroupId { self.id }
    pub fn name(&self) -> &str { &self.name }
    
    pub fn add_rule(&self, rule: SecurityRule) {
        let mut rules = self.rules.write().unwrap();
        rules.push(rule);
        rules.sort_by_key(|r| r.priority);
    }
    
    pub fn remove_rule(&self, priority: u32) {
        self.rules.write().unwrap().retain(|r| r.priority != priority);
    }
    
    /// Check if packet matches rules
    pub fn check_packet(&self, packet: &[u8], direction: Direction) -> bool {
        let rules = self.rules.read().unwrap();
        
        // Find first matching rule
        for rule in rules.iter() {
            if rule.direction != direction {
                continue;
            }
            
            // Simplified packet matching
            // In real implementation, would parse packet headers
            if self.packet_matches_rule(packet, rule) {
                return rule.action == RuleAction::Allow;
            }
        }
        
        // Default deny
        false
    }
    
    fn packet_matches_rule(&self, _packet: &[u8], _rule: &SecurityRule) -> bool {
        // Simplified - always matches for testing
        true
    }
}

// ============================================================================
// Network Policy
// ============================================================================

/// Network policy for advanced traffic control
#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    /// Policy name
    pub name: String,
    /// Source selector
    pub source: PolicySelector,
    /// Destination selector
    pub destination: PolicySelector,
    /// Allowed protocols
    pub protocols: Vec<Protocol>,
    /// Priority
    pub priority: u32,
}

/// Policy selector
#[derive(Debug, Clone)]
pub enum PolicySelector {
    /// Match all
    All,
    /// Match by labels
    Labels(HashMap<String, String>),
    /// Match by namespace
    Namespace(String),
    /// Match by CIDR
    Cidr(String),
}

// ============================================================================
// SR-IOV Support
// ============================================================================

/// SR-IOV configuration
pub struct SriovConfig {
    /// Physical function
    pub pf: String,
    /// Number of VFs
    pub num_vfs: u32,
    /// VF assignments
    pub vf_assignments: RwLock<HashMap<u32, VmId>>,
}

impl SriovConfig {
    pub fn new(pf: &str, num_vfs: u32) -> Self {
        Self {
            pf: pf.to_string(),
            num_vfs,
            vf_assignments: RwLock::new(HashMap::new()),
        }
    }
    
    /// Assign VF to VM
    pub fn assign_vf(&self, vf: u32, vm_id: VmId) -> HypervisorResult<()> {
        if vf >= self.num_vfs {
            return Err(HypervisorError::NetworkError(
                format!("Invalid VF number: {}", vf)
            ));
        }
        
        let mut assignments = self.vf_assignments.write().unwrap();
        if assignments.contains_key(&vf) {
            return Err(HypervisorError::NetworkError(
                format!("VF {} already assigned", vf)
            ));
        }
        
        assignments.insert(vf, vm_id);
        Ok(())
    }
    
    /// Release VF from VM
    pub fn release_vf(&self, vf: u32) {
        self.vf_assignments.write().unwrap().remove(&vf);
    }
    
    /// Get available VFs
    pub fn available_vfs(&self) -> Vec<u32> {
        let assignments = self.vf_assignments.read().unwrap();
        (0..self.num_vfs)
            .filter(|vf| !assignments.contains_key(vf))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_network_manager() {
        let manager = NetworkManager::new();
        
        // Create switch
        let switch_id = manager.create_switch("vswitch0", VSwitchSpec::default()).unwrap();
        
        // Create vNIC
        let vnic_spec = VNicSpec {
            name: "eth0".to_string(),
            switch_id: Some(switch_id),
            ..Default::default()
        };
        
        let vnic_id = manager.create_vnic(vnic_spec).unwrap();
        
        // Attach to VM
        manager.attach_vnic(vnic_id, VmId::new(1)).unwrap();
        
        // Create security group
        let sg_id = manager.create_security_group("default").unwrap();
        
        manager.add_security_rule(sg_id, SecurityRule {
            direction: Direction::Ingress,
            protocol: Protocol::Any,
            source: RuleTarget::Any,
            destination: RuleTarget::Any,
            port_range: None,
            action: RuleAction::Allow,
            priority: 100,
        }).unwrap();
        
        manager.apply_security_group(vnic_id, sg_id).unwrap();
    }
    
    #[test]
    fn test_mac_address() {
        let mac = generate_mac_address(1);
        assert!(mac.is_local());
        assert!(!mac.is_broadcast());
        assert!(!mac.is_multicast());
        
        let broadcast = MacAddress::new([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert!(broadcast.is_broadcast());
        
        let multicast = MacAddress::new([0x01, 0x00, 0x5e, 0x00, 0x00, 0x01]);
        assert!(multicast.is_multicast());
    }
    
    #[test]
    fn test_qos_policy() {
        let qos = QosPolicy::new(1_000_000, 10_000, 5); // 1MB/s, 10KB burst
        
        // Should allow small packet
        assert!(qos.allow_packet(1000));
        
        // Multiple packets should eventually be rate limited
        let mut allowed = 0;
        for _ in 0..100 {
            if qos.allow_packet(1000) {
                allowed += 1;
            }
        }
        assert!(allowed < 100); // Some should be rate limited
    }
    
    #[test]
    fn test_virtual_switch() {
        let switch = VirtualSwitch::new(
            VSwitchId::new(1),
            "test".to_string(),
            VSwitchSpec::default(),
        );
        
        // Connect port
        switch.connect_port(VNicId::new(1)).unwrap();
        assert_eq!(switch.port_count(), 1);
        
        // Disconnect port
        switch.disconnect_port(VNicId::new(1));
        assert_eq!(switch.port_count(), 0);
    }
    
    #[test]
    fn test_sriov() {
        let sriov = SriovConfig::new("eth0", 8);
        
        // Assign VF
        sriov.assign_vf(0, VmId::new(1)).unwrap();
        assert_eq!(sriov.available_vfs().len(), 7);
        
        // Cannot assign same VF twice
        assert!(sriov.assign_vf(0, VmId::new(2)).is_err());
        
        // Release VF
        sriov.release_vf(0);
        assert_eq!(sriov.available_vfs().len(), 8);
    }
}
