//! Resource Pool Management
//!
//! This module provides resource pool management for CPU, memory, storage, and network
//! resources. It implements features similar to VMware's resource pools and vSphere DRS.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicU32, Ordering}};
use std::path::PathBuf;

use super::core::HypervisorError;

/// Resource type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Cpu,
    Memory,
    Storage,
    Network,
    Gpu,
}

/// Resource allocation tracking
#[derive(Debug, Clone)]
pub struct ResourceAllocation {
    pub resource_type: ResourceType,
    pub allocated: u64,
    pub reserved: u64,
    pub limit: Option<u64>,
    pub shares: u32,
}

/// Base trait for resource pools
pub trait ResourcePool: Send + Sync {
    fn resource_type(&self) -> ResourceType;
    fn total(&self) -> u64;
    fn available(&self) -> u64;
    fn allocated(&self) -> u64;
    fn reserved(&self) -> u64;
    fn utilization(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        (self.allocated() as f64 / total as f64) * 100.0
    }
}

// ============================================================================
// CPU Pool
// ============================================================================

/// CPU resource pool with advanced features
pub struct CpuPool {
    /// Total physical cores
    total_cores: u32,
    /// Available (unallocated) cores
    available_cores: AtomicU32,
    /// Total allocated vCPUs
    allocated_vcpus: AtomicU64,
    /// Overcommit ratio (e.g., 4.0 means 4 vCPUs per pCPU)
    overcommit_ratio: f64,
    /// Per-core allocation tracking
    core_allocations: RwLock<Vec<CoreAllocation>>,
    /// NUMA topology
    numa_topology: RwLock<Option<NumaTopology>>,
    /// CPU features available
    cpu_features: RwLock<CpuFeatures>,
    /// Statistics
    stats: RwLock<CpuPoolStats>,
}

#[derive(Debug, Clone, Default)]
struct CoreAllocation {
    core_id: u32,
    allocated_vcpus: u32,
    pinned_vms: Vec<String>,
}

/// CPU pool statistics
#[derive(Debug, Clone, Default)]
pub struct CpuPoolStats {
    pub total_allocations: u64,
    pub total_releases: u64,
    pub peak_utilization: f64,
    pub average_utilization: f64,
}

/// NUMA topology
#[derive(Debug, Clone)]
pub struct NumaTopology {
    pub nodes: Vec<NumaNode>,
}

/// NUMA node
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: u32,
    pub cores: Vec<u32>,
    pub memory_mb: u64,
    pub distances: HashMap<u32, u32>,
}

/// CPU features
#[derive(Debug, Clone, Default)]
pub struct CpuFeatures {
    pub vmx: bool,
    pub svm: bool,
    pub avx: bool,
    pub avx2: bool,
    pub avx512: bool,
    pub aes: bool,
    pub sse4_2: bool,
}

impl CpuPool {
    pub fn new(total_cores: u32) -> Self {
        let core_allocations: Vec<_> = (0..total_cores)
            .map(|id| CoreAllocation {
                core_id: id,
                allocated_vcpus: 0,
                pinned_vms: Vec::new(),
            })
            .collect();
        
        Self {
            total_cores,
            available_cores: AtomicU32::new(total_cores),
            allocated_vcpus: AtomicU64::new(0),
            overcommit_ratio: 4.0,
            core_allocations: RwLock::new(core_allocations),
            numa_topology: RwLock::new(None),
            cpu_features: RwLock::new(CpuFeatures::default()),
            stats: RwLock::new(CpuPoolStats::default()),
        }
    }
    
    pub fn with_overcommit(mut self, ratio: f64) -> Self {
        self.overcommit_ratio = ratio;
        self
    }
    
    pub fn with_numa(mut self, topology: NumaTopology) -> Self {
        *self.numa_topology.write().unwrap() = Some(topology);
        self
    }
    
    /// Allocate vCPUs
    pub fn allocate(&self, vcpus: u32) -> Result<(), HypervisorError> {
        let effective_cores = (vcpus as f64 / self.overcommit_ratio).ceil() as u32;
        let available = self.available_cores.load(Ordering::SeqCst);
        
        if effective_cores > available {
            return Err(HypervisorError::ResourceUnavailable {
                resource: "CPU".to_string(),
                requested: vcpus as u64,
                available: (available as f64 * self.overcommit_ratio) as u64,
            });
        }
        
        self.available_cores.fetch_sub(effective_cores, Ordering::SeqCst);
        self.allocated_vcpus.fetch_add(vcpus as u64, Ordering::SeqCst);
        
        let mut stats = self.stats.write().unwrap();
        stats.total_allocations += 1;
        
        Ok(())
    }
    
    /// Release vCPUs
    pub fn release(&self, vcpus: u32) {
        let effective_cores = (vcpus as f64 / self.overcommit_ratio).ceil() as u32;
        self.available_cores.fetch_add(effective_cores, Ordering::SeqCst);
        self.allocated_vcpus.fetch_sub(vcpus as u64, Ordering::SeqCst);
        
        let mut stats = self.stats.write().unwrap();
        stats.total_releases += 1;
    }
    
    /// Get available cores
    pub fn available(&self) -> u32 {
        self.available_cores.load(Ordering::SeqCst)
    }
    
    /// Get total cores
    pub fn total(&self) -> u32 {
        self.total_cores
    }
    
    /// Get allocated vCPUs
    pub fn allocated_vcpus(&self) -> u64 {
        self.allocated_vcpus.load(Ordering::SeqCst)
    }
    
    /// Pin vCPU to specific physical core(s)
    pub fn pin_vcpu(&self, vm_name: &str, vcpu_id: u32, pcpus: &[u32]) -> Result<(), HypervisorError> {
        let mut allocations = self.core_allocations.write().unwrap();
        
        for &pcpu in pcpus {
            if pcpu >= self.total_cores {
                return Err(HypervisorError::ConfigError(
                    format!("Invalid pCPU {}: only {} cores available", pcpu, self.total_cores)
                ));
            }
            
            allocations[pcpu as usize].pinned_vms.push(format!("{}:{}", vm_name, vcpu_id));
            allocations[pcpu as usize].allocated_vcpus += 1;
        }
        
        Ok(())
    }
    
    /// Unpin vCPU
    pub fn unpin_vcpu(&self, vm_name: &str, vcpu_id: u32) {
        let mut allocations = self.core_allocations.write().unwrap();
        let key = format!("{}:{}", vm_name, vcpu_id);
        
        for alloc in allocations.iter_mut() {
            if let Some(pos) = alloc.pinned_vms.iter().position(|x| x == &key) {
                alloc.pinned_vms.remove(pos);
                alloc.allocated_vcpus = alloc.allocated_vcpus.saturating_sub(1);
            }
        }
    }
    
    /// Get NUMA-aware allocation recommendation
    pub fn recommend_numa_allocation(&self, vcpus: u32, memory_mb: u64) -> Option<Vec<u32>> {
        let topology = self.numa_topology.read().unwrap();
        let topo = topology.as_ref()?;
        
        // Find node with enough resources
        for node in &topo.nodes {
            if node.cores.len() >= vcpus as usize && node.memory_mb >= memory_mb {
                return Some(node.cores[..vcpus as usize].to_vec());
            }
        }
        
        // Cross-node allocation
        let mut allocated = Vec::new();
        for node in &topo.nodes {
            let remaining = vcpus as usize - allocated.len();
            if remaining == 0 {
                break;
            }
            let take = remaining.min(node.cores.len());
            allocated.extend_from_slice(&node.cores[..take]);
        }
        
        if allocated.len() >= vcpus as usize {
            Some(allocated)
        } else {
            None
        }
    }
    
    /// Get statistics
    pub fn stats(&self) -> CpuPoolStats {
        self.stats.read().unwrap().clone()
    }
}

impl ResourcePool for CpuPool {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Cpu
    }
    
    fn total(&self) -> u64 {
        self.total_cores as u64
    }
    
    fn available(&self) -> u64 {
        self.available_cores.load(Ordering::SeqCst) as u64
    }
    
    fn allocated(&self) -> u64 {
        (self.total_cores - self.available_cores.load(Ordering::SeqCst)) as u64
    }
    
    fn reserved(&self) -> u64 {
        0 // CPU doesn't have reservation concept
    }
}

// ============================================================================
// Memory Pool
// ============================================================================

/// Memory resource pool with advanced features
pub struct MemoryPool {
    /// Total memory in MB
    total_mb: u64,
    /// Available memory in MB
    available_mb: AtomicU64,
    /// Reserved memory in MB
    reserved_mb: AtomicU64,
    /// Overcommit ratio
    overcommit_ratio: f64,
    /// Enable KSM
    ksm_enabled: bool,
    /// KSM savings (MB)
    ksm_savings_mb: AtomicU64,
    /// Balloon reclaimed (MB)
    balloon_reclaimed_mb: AtomicU64,
    /// Statistics
    stats: RwLock<MemoryPoolStats>,
    /// Per-VM allocations
    vm_allocations: RwLock<HashMap<String, MemoryAllocation>>,
}

#[derive(Debug, Clone)]
pub struct MemoryAllocation {
    pub vm_name: String,
    pub allocated_mb: u64,
    pub reserved_mb: u64,
    pub limit_mb: Option<u64>,
    pub shares: u32,
    pub balloon_target_mb: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryPoolStats {
    pub total_allocations: u64,
    pub total_releases: u64,
    pub peak_usage_mb: u64,
    pub ksm_pages_shared: u64,
    pub balloon_deflations: u64,
    pub balloon_inflations: u64,
}

impl MemoryPool {
    pub fn new(total_mb: u64) -> Self {
        Self {
            total_mb,
            available_mb: AtomicU64::new(total_mb),
            reserved_mb: AtomicU64::new(0),
            overcommit_ratio: 1.5,
            ksm_enabled: true,
            ksm_savings_mb: AtomicU64::new(0),
            balloon_reclaimed_mb: AtomicU64::new(0),
            stats: RwLock::new(MemoryPoolStats::default()),
            vm_allocations: RwLock::new(HashMap::new()),
        }
    }
    
    pub fn with_overcommit(mut self, ratio: f64) -> Self {
        self.overcommit_ratio = ratio;
        self
    }
    
    pub fn with_ksm(mut self, enabled: bool) -> Self {
        self.ksm_enabled = enabled;
        self
    }
    
    /// Allocate memory
    pub fn allocate(&self, mb: u64) -> Result<(), HypervisorError> {
        let effective_mb = (mb as f64 / self.overcommit_ratio).ceil() as u64;
        let available = self.available_mb.load(Ordering::SeqCst);
        
        if effective_mb > available {
            // Try to reclaim via balloon
            let reclaimed = self.try_balloon_reclaim(effective_mb - available);
            if reclaimed < effective_mb - available {
                return Err(HypervisorError::ResourceUnavailable {
                    resource: "Memory".to_string(),
                    requested: mb,
                    available: (available as f64 * self.overcommit_ratio) as u64,
                });
            }
        }
        
        self.available_mb.fetch_sub(effective_mb, Ordering::SeqCst);
        
        let mut stats = self.stats.write().unwrap();
        stats.total_allocations += 1;
        let used = self.total_mb - self.available_mb.load(Ordering::SeqCst);
        if used > stats.peak_usage_mb {
            stats.peak_usage_mb = used;
        }
        
        Ok(())
    }
    
    /// Release memory
    pub fn release(&self, mb: u64) {
        let effective_mb = (mb as f64 / self.overcommit_ratio).ceil() as u64;
        self.available_mb.fetch_add(effective_mb, Ordering::SeqCst);
        
        let mut stats = self.stats.write().unwrap();
        stats.total_releases += 1;
    }
    
    /// Reserve memory (guaranteed allocation)
    pub fn reserve(&self, mb: u64) -> Result<(), HypervisorError> {
        let available = self.available_mb.load(Ordering::SeqCst);
        let reserved = self.reserved_mb.load(Ordering::SeqCst);
        
        if mb > available - reserved {
            return Err(HypervisorError::ResourceUnavailable {
                resource: "Memory (reserved)".to_string(),
                requested: mb,
                available: available - reserved,
            });
        }
        
        self.reserved_mb.fetch_add(mb, Ordering::SeqCst);
        Ok(())
    }
    
    /// Get available memory (MB)
    pub fn available(&self) -> u64 {
        self.available_mb.load(Ordering::SeqCst)
    }
    
    /// Get total memory (MB)
    pub fn total(&self) -> u64 {
        self.total_mb
    }
    
    /// Try to reclaim memory via balloon driver
    fn try_balloon_reclaim(&self, needed_mb: u64) -> u64 {
        // In real implementation, this would communicate with balloon drivers
        // For now, simulate some reclaim
        let reclaimed = needed_mb.min(100); // Max 100MB per reclaim attempt
        self.balloon_reclaimed_mb.fetch_add(reclaimed, Ordering::SeqCst);
        
        let mut stats = self.stats.write().unwrap();
        stats.balloon_inflations += 1;
        
        reclaimed
    }
    
    /// Set balloon target for a VM
    pub fn set_balloon_target(&self, vm_name: &str, target_mb: u64) {
        let mut allocations = self.vm_allocations.write().unwrap();
        if let Some(alloc) = allocations.get_mut(vm_name) {
            alloc.balloon_target_mb = Some(target_mb);
        }
    }
    
    /// Report KSM savings
    pub fn report_ksm_savings(&self, pages_shared: u64) {
        let savings_mb = (pages_shared * 4096) / (1024 * 1024);
        self.ksm_savings_mb.store(savings_mb, Ordering::SeqCst);
        self.stats.write().unwrap().ksm_pages_shared = pages_shared;
    }
    
    /// Get effective available (including KSM savings)
    pub fn effective_available(&self) -> u64 {
        self.available_mb.load(Ordering::SeqCst) + self.ksm_savings_mb.load(Ordering::SeqCst)
    }
    
    /// Get statistics
    pub fn stats(&self) -> MemoryPoolStats {
        self.stats.read().unwrap().clone()
    }
}

impl ResourcePool for MemoryPool {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Memory
    }
    
    fn total(&self) -> u64 {
        self.total_mb
    }
    
    fn available(&self) -> u64 {
        self.available_mb.load(Ordering::SeqCst)
    }
    
    fn allocated(&self) -> u64 {
        self.total_mb - self.available_mb.load(Ordering::SeqCst)
    }
    
    fn reserved(&self) -> u64 {
        self.reserved_mb.load(Ordering::SeqCst)
    }
}

// ============================================================================
// Storage Pool
// ============================================================================

/// Storage resource pool
pub struct StoragePool {
    /// Pool name
    name: String,
    /// Pool path
    path: PathBuf,
    /// Total capacity in bytes
    total_bytes: u64,
    /// Available space in bytes
    available_bytes: AtomicU64,
    /// Storage type
    storage_type: StorageType,
    /// Thin provisioning enabled
    thin_provisioning: bool,
    /// Deduplication enabled
    dedup_enabled: bool,
    /// Statistics
    stats: RwLock<StoragePoolStats>,
    /// Disk images
    images: RwLock<HashMap<String, DiskImage>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    Local,
    Nfs,
    Iscsi,
    Ceph,
    Gluster,
    Vmfs,
}

#[derive(Debug, Clone)]
pub struct DiskImage {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub actual_bytes: u64, // Actual disk usage (for thin)
    pub format: DiskImageFormat,
    pub backing_file: Option<String>,
    pub snapshots: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskImageFormat {
    Raw,
    Qcow2,
    Vmdk,
    Vdi,
}

#[derive(Debug, Clone, Default)]
pub struct StoragePoolStats {
    pub images_created: u64,
    pub images_deleted: u64,
    pub snapshots_created: u64,
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub dedup_savings_bytes: u64,
}

impl StoragePool {
    pub fn new(name: &str, path: &str, capacity_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: PathBuf::from(path),
            total_bytes: capacity_bytes,
            available_bytes: AtomicU64::new(capacity_bytes),
            storage_type: StorageType::Local,
            thin_provisioning: true,
            dedup_enabled: false,
            stats: RwLock::new(StoragePoolStats::default()),
            images: RwLock::new(HashMap::new()),
        }
    }
    
    pub fn with_type(mut self, storage_type: StorageType) -> Self {
        self.storage_type = storage_type;
        self
    }
    
    pub fn with_thin_provisioning(mut self, enabled: bool) -> Self {
        self.thin_provisioning = enabled;
        self
    }
    
    /// Create a new disk image
    pub fn create_image(
        &self,
        name: &str,
        size_bytes: u64,
        format: DiskImageFormat,
    ) -> Result<DiskImage, HypervisorError> {
        let actual_size = if self.thin_provisioning {
            0 // Thin: starts at 0
        } else {
            size_bytes // Thick: pre-allocated
        };
        
        let available = self.available_bytes.load(Ordering::SeqCst);
        if actual_size > available {
            return Err(HypervisorError::StorageError(
                format!("Not enough space: need {} bytes, have {} bytes", actual_size, available)
            ));
        }
        
        let image = DiskImage {
            name: name.to_string(),
            path: self.path.join(name),
            size_bytes,
            actual_bytes: actual_size,
            format,
            backing_file: None,
            snapshots: Vec::new(),
        };
        
        if actual_size > 0 {
            self.available_bytes.fetch_sub(actual_size, Ordering::SeqCst);
        }
        
        self.images.write().unwrap().insert(name.to_string(), image.clone());
        self.stats.write().unwrap().images_created += 1;
        
        Ok(image)
    }
    
    /// Create a linked clone (uses backing file)
    pub fn create_linked_clone(
        &self,
        name: &str,
        base_image: &str,
    ) -> Result<DiskImage, HypervisorError> {
        let images = self.images.read().unwrap();
        let base = images.get(base_image)
            .ok_or_else(|| HypervisorError::StorageError(
                format!("Base image '{}' not found", base_image)
            ))?;
        
        let clone = DiskImage {
            name: name.to_string(),
            path: self.path.join(name),
            size_bytes: base.size_bytes,
            actual_bytes: 0, // Linked clone starts empty
            format: DiskImageFormat::Qcow2,
            backing_file: Some(base_image.to_string()),
            snapshots: Vec::new(),
        };
        
        drop(images);
        self.images.write().unwrap().insert(name.to_string(), clone.clone());
        self.stats.write().unwrap().images_created += 1;
        
        Ok(clone)
    }
    
    /// Delete a disk image
    pub fn delete_image(&self, name: &str) -> Result<(), HypervisorError> {
        let mut images = self.images.write().unwrap();
        let image = images.remove(name)
            .ok_or_else(|| HypervisorError::StorageError(
                format!("Image '{}' not found", name)
            ))?;
        
        self.available_bytes.fetch_add(image.actual_bytes, Ordering::SeqCst);
        self.stats.write().unwrap().images_deleted += 1;
        
        Ok(())
    }
    
    /// Create a snapshot of a disk image
    pub fn create_snapshot(&self, image_name: &str, snapshot_name: &str) -> Result<(), HypervisorError> {
        let mut images = self.images.write().unwrap();
        let image = images.get_mut(image_name)
            .ok_or_else(|| HypervisorError::StorageError(
                format!("Image '{}' not found", image_name)
            ))?;
        
        image.snapshots.push(snapshot_name.to_string());
        self.stats.write().unwrap().snapshots_created += 1;
        
        Ok(())
    }
    
    /// Get pool utilization
    pub fn utilization(&self) -> f64 {
        let used = self.total_bytes - self.available_bytes.load(Ordering::SeqCst);
        (used as f64 / self.total_bytes as f64) * 100.0
    }
    
    /// Get statistics
    pub fn stats(&self) -> StoragePoolStats {
        self.stats.read().unwrap().clone()
    }
    
    /// List all images
    pub fn list_images(&self) -> Vec<DiskImage> {
        self.images.read().unwrap().values().cloned().collect()
    }
}

impl ResourcePool for StoragePool {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Storage
    }
    
    fn total(&self) -> u64 {
        self.total_bytes
    }
    
    fn available(&self) -> u64 {
        self.available_bytes.load(Ordering::SeqCst)
    }
    
    fn allocated(&self) -> u64 {
        self.total_bytes - self.available_bytes.load(Ordering::SeqCst)
    }
    
    fn reserved(&self) -> u64 {
        0
    }
}

// ============================================================================
// Network Pool
// ============================================================================

/// Network resource pool
pub struct NetworkPool {
    /// Pool name
    name: String,
    /// Network type
    net_type: NetworkPoolType,
    /// Bridge name (if bridge mode)
    bridge: Option<String>,
    /// VLAN range
    vlan_range: Option<(u16, u16)>,
    /// Available VLANs
    available_vlans: RwLock<Vec<u16>>,
    /// MAC address pool
    mac_pool: RwLock<MacPool>,
    /// IP address pool
    ip_pool: RwLock<Option<IpPool>>,
    /// Statistics
    stats: RwLock<NetworkPoolStats>,
    /// Virtual switches
    vswitches: RwLock<HashMap<String, VirtualSwitchConfig>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPoolType {
    Bridge,
    Nat,
    Internal,
    HostOnly,
    Overlay,
}

#[derive(Debug, Clone)]
struct MacPool {
    prefix: [u8; 3],
    next_suffix: u32,
    allocated: Vec<[u8; 6]>,
}

impl Default for MacPool {
    fn default() -> Self {
        Self {
            prefix: [0x52, 0x54, 0x00], // QEMU-style MAC prefix
            next_suffix: 0,
            allocated: Vec::new(),
        }
    }
}

impl MacPool {
    fn allocate(&mut self) -> [u8; 6] {
        let suffix = self.next_suffix;
        self.next_suffix += 1;
        
        let mac = [
            self.prefix[0],
            self.prefix[1],
            self.prefix[2],
            ((suffix >> 16) & 0xFF) as u8,
            ((suffix >> 8) & 0xFF) as u8,
            (suffix & 0xFF) as u8,
        ];
        
        self.allocated.push(mac);
        mac
    }
    
    fn release(&mut self, mac: [u8; 6]) {
        if let Some(pos) = self.allocated.iter().position(|&m| m == mac) {
            self.allocated.remove(pos);
        }
    }
}

#[derive(Debug, Clone)]
struct IpPool {
    network: [u8; 4],
    mask: u8,
    gateway: [u8; 4],
    range_start: [u8; 4],
    range_end: [u8; 4],
    allocated: Vec<[u8; 4]>,
}

#[derive(Debug, Clone)]
pub struct VirtualSwitchConfig {
    pub name: String,
    pub mtu: u16,
    pub vlan_mode: VlanMode,
    pub ports: Vec<VswitchPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VlanMode {
    Access(u16),
    Trunk(Vec<u16>),
    Native,
}

impl Default for VlanMode {
    fn default() -> Self {
        Self::Native
    }
}

#[derive(Debug, Clone)]
pub struct VswitchPort {
    pub name: String,
    pub vm: Option<String>,
    pub vlan: Option<u16>,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkPoolStats {
    pub mac_allocated: u64,
    pub ip_allocated: u64,
    pub vlans_in_use: u64,
}

impl NetworkPool {
    pub fn new(name: &str, net_type: NetworkPoolType) -> Self {
        Self {
            name: name.to_string(),
            net_type,
            bridge: None,
            vlan_range: None,
            available_vlans: RwLock::new(Vec::new()),
            mac_pool: RwLock::new(MacPool::default()),
            ip_pool: RwLock::new(None),
            stats: RwLock::new(NetworkPoolStats::default()),
            vswitches: RwLock::new(HashMap::new()),
        }
    }
    
    pub fn with_bridge(name: &str, bridge: &str) -> Self {
        let mut pool = Self::new(name, NetworkPoolType::Bridge);
        pool.bridge = Some(bridge.to_string());
        pool
    }
    
    pub fn with_vlan_range(mut self, start: u16, end: u16) -> Self {
        self.vlan_range = Some((start, end));
        *self.available_vlans.write().unwrap() = (start..=end).collect();
        self
    }
    
    /// Allocate a MAC address
    pub fn allocate_mac(&self) -> [u8; 6] {
        let mac = self.mac_pool.write().unwrap().allocate();
        self.stats.write().unwrap().mac_allocated += 1;
        mac
    }
    
    /// Release a MAC address
    pub fn release_mac(&self, mac: [u8; 6]) {
        self.mac_pool.write().unwrap().release(mac);
    }
    
    /// Format MAC address as string
    pub fn format_mac(mac: &[u8; 6]) -> String {
        format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5])
    }
    
    /// Allocate a VLAN
    pub fn allocate_vlan(&self) -> Option<u16> {
        let mut vlans = self.available_vlans.write().unwrap();
        let vlan = vlans.pop()?;
        self.stats.write().unwrap().vlans_in_use += 1;
        Some(vlan)
    }
    
    /// Release a VLAN
    pub fn release_vlan(&self, vlan: u16) {
        self.available_vlans.write().unwrap().push(vlan);
        let mut stats = self.stats.write().unwrap();
        if stats.vlans_in_use > 0 {
            stats.vlans_in_use -= 1;
        }
    }
    
    /// Create a virtual switch
    pub fn create_vswitch(&self, name: &str, mtu: u16) -> Result<(), HypervisorError> {
        let config = VirtualSwitchConfig {
            name: name.to_string(),
            mtu,
            vlan_mode: VlanMode::default(),
            ports: Vec::new(),
        };
        
        self.vswitches.write().unwrap().insert(name.to_string(), config);
        Ok(())
    }
    
    /// Add port to virtual switch
    pub fn add_vswitch_port(&self, vswitch: &str, port_name: &str) -> Result<(), HypervisorError> {
        let mut vswitches = self.vswitches.write().unwrap();
        let vs = vswitches.get_mut(vswitch)
            .ok_or_else(|| HypervisorError::NetworkError(
                format!("Virtual switch '{}' not found", vswitch)
            ))?;
        
        vs.ports.push(VswitchPort {
            name: port_name.to_string(),
            vm: None,
            vlan: None,
        });
        
        Ok(())
    }
    
    /// Get statistics
    pub fn stats(&self) -> NetworkPoolStats {
        self.stats.read().unwrap().clone()
    }
}

impl ResourcePool for NetworkPool {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Network
    }
    
    fn total(&self) -> u64 {
        // For network, total is VLAN count if VLAN mode
        self.vlan_range.map_or(0, |(s, e)| (e - s + 1) as u64)
    }
    
    fn available(&self) -> u64 {
        self.available_vlans.read().unwrap().len() as u64
    }
    
    fn allocated(&self) -> u64 {
        self.stats.read().unwrap().vlans_in_use
    }
    
    fn reserved(&self) -> u64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cpu_pool_allocation() {
        let pool = CpuPool::new(16);
        
        assert_eq!(pool.total(), 16);
        assert_eq!(pool.available(), 16);
        
        pool.allocate(4).unwrap();
        // With 4:1 overcommit, 4 vCPUs = 1 effective core
        assert_eq!(pool.available(), 15);
        
        pool.release(4);
        assert_eq!(pool.available(), 16);
    }
    
    #[test]
    fn test_memory_pool_allocation() {
        let pool = MemoryPool::new(32 * 1024); // 32GB
        
        assert_eq!(pool.total(), 32 * 1024);
        
        pool.allocate(4096).unwrap(); // 4GB
        // With 1.5:1 overcommit, 4GB = ~2.7GB effective
        assert!(pool.available() < 32 * 1024);
        
        pool.release(4096);
        assert_eq!(pool.available(), 32 * 1024);
    }
    
    #[test]
    fn test_storage_pool_image_creation() {
        let pool = StoragePool::new("default", "/var/lib/nexahv/images", 1024 * 1024 * 1024 * 100);
        
        let image = pool.create_image("test.qcow2", 10 * 1024 * 1024 * 1024, DiskImageFormat::Qcow2).unwrap();
        assert_eq!(image.name, "test.qcow2");
        assert_eq!(image.actual_bytes, 0); // Thin provisioned
        
        pool.delete_image("test.qcow2").unwrap();
    }
    
    #[test]
    fn test_network_pool_mac_allocation() {
        let pool = NetworkPool::new("default", NetworkPoolType::Bridge);
        
        let mac1 = pool.allocate_mac();
        let mac2 = pool.allocate_mac();
        
        assert_ne!(mac1, mac2);
        
        let mac_str = NetworkPool::format_mac(&mac1);
        assert!(mac_str.starts_with("52:54:00:"));
    }
    
    #[test]
    fn test_network_pool_vlan() {
        let pool = NetworkPool::new("vlan", NetworkPoolType::Bridge)
            .with_vlan_range(100, 200);
        
        let vlan1 = pool.allocate_vlan().unwrap();
        assert!(vlan1 >= 100 && vlan1 <= 200);
        
        pool.release_vlan(vlan1);
    }
}
