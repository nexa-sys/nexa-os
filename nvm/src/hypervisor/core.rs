//! Core Hypervisor Types and Implementation
//!
//! This module provides the core hypervisor implementation with VM lifecycle management,
//! resource allocation, and enterprise features.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, Ordering}};
use std::time::{Instant, Duration};
use std::path::PathBuf;

use super::{HypervisorFeatures, HypervisorStats};
use super::resources::{ResourcePool, CpuPool, MemoryPool, StoragePool, NetworkPool};
use super::memory::MemoryManager;
use super::scheduler::VmScheduler;
use super::security::SecurityPolicy;

/// Unique VM identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VmId(u64);

impl VmId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
    
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for VmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "vm-{:016x}", self.0)
    }
}

/// VM status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmStatus {
    /// VM is created but not yet configured
    Created,
    /// VM is configured and ready to start
    Configured,
    /// VM is starting up
    Starting,
    /// VM is running
    Running,
    /// VM is paused
    Paused,
    /// VM is saving state (hibernating)
    Saving,
    /// VM is in saved state
    Saved,
    /// VM is restoring from saved state
    Restoring,
    /// VM is stopping
    Stopping,
    /// VM is stopped
    Stopped,
    /// VM is being migrated
    Migrating,
    /// VM is in error state
    Error,
    /// VM is being destroyed
    Destroying,
}

impl Default for VmStatus {
    fn default() -> Self {
        Self::Created
    }
}

impl std::fmt::Display for VmStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Configured => write!(f, "configured"),
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Paused => write!(f, "paused"),
            Self::Saving => write!(f, "saving"),
            Self::Saved => write!(f, "saved"),
            Self::Restoring => write!(f, "restoring"),
            Self::Stopping => write!(f, "stopping"),
            Self::Stopped => write!(f, "stopped"),
            Self::Migrating => write!(f, "migrating"),
            Self::Error => write!(f, "error"),
            Self::Destroying => write!(f, "destroying"),
        }
    }
}

/// VM specification for creation
#[derive(Debug, Clone)]
pub struct VmSpec {
    /// VM name
    pub name: String,
    /// VM description
    pub description: Option<String>,
    /// Number of vCPUs
    pub vcpus: u32,
    /// Memory size in MB
    pub memory_mb: u64,
    /// Maximum memory size in MB (for hot-plug)
    pub max_memory_mb: Option<u64>,
    /// Disk specifications
    pub disks: Vec<DiskSpec>,
    /// Network specifications
    pub networks: Vec<NetworkSpec>,
    /// Boot order
    pub boot_order: Vec<BootDevice>,
    /// BIOS or UEFI
    pub firmware: FirmwareType,
    /// Enable nested virtualization
    pub nested_virt: bool,
    /// CPU model
    pub cpu_model: CpuModel,
    /// Machine type (q35, i440fx, etc.)
    pub machine_type: MachineType,
    /// NUMA configuration
    pub numa: Option<NumaSpec>,
    /// CPU pinning
    pub cpu_pinning: Option<CpuPinning>,
    /// Security configuration
    pub security: Option<VmSecuritySpec>,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl Default for VmSpec {
    fn default() -> Self {
        Self {
            name: String::from("unnamed-vm"),
            description: None,
            vcpus: 1,
            memory_mb: 1024,
            max_memory_mb: None,
            disks: Vec::new(),
            networks: Vec::new(),
            boot_order: vec![BootDevice::Disk, BootDevice::Network],
            firmware: FirmwareType::Bios,
            nested_virt: false,
            cpu_model: CpuModel::Host,
            machine_type: MachineType::Q35,
            numa: None,
            cpu_pinning: None,
            security: None,
            metadata: HashMap::new(),
        }
    }
}

impl VmSpec {
    pub fn builder() -> VmSpecBuilder {
        VmSpecBuilder::new()
    }
}

/// VM specification builder
pub struct VmSpecBuilder {
    spec: VmSpec,
}

impl VmSpecBuilder {
    pub fn new() -> Self {
        Self {
            spec: VmSpec::default(),
        }
    }
    
    pub fn name(mut self, name: &str) -> Self {
        self.spec.name = name.to_string();
        self
    }
    
    pub fn description(mut self, desc: &str) -> Self {
        self.spec.description = Some(desc.to_string());
        self
    }
    
    pub fn vcpus(mut self, count: u32) -> Self {
        self.spec.vcpus = count;
        self
    }
    
    pub fn memory_mb(mut self, mb: u64) -> Self {
        self.spec.memory_mb = mb;
        self
    }
    
    pub fn max_memory_mb(mut self, mb: u64) -> Self {
        self.spec.max_memory_mb = Some(mb);
        self
    }
    
    pub fn disk(mut self, disk: DiskSpec) -> Self {
        self.spec.disks.push(disk);
        self
    }
    
    pub fn network(mut self, network: NetworkSpec) -> Self {
        self.spec.networks.push(network);
        self
    }
    
    pub fn boot_order(mut self, order: Vec<BootDevice>) -> Self {
        self.spec.boot_order = order;
        self
    }
    
    pub fn firmware(mut self, fw: FirmwareType) -> Self {
        self.spec.firmware = fw;
        self
    }
    
    pub fn nested_virt(mut self, enable: bool) -> Self {
        self.spec.nested_virt = enable;
        self
    }
    
    pub fn cpu_model(mut self, model: CpuModel) -> Self {
        self.spec.cpu_model = model;
        self
    }
    
    pub fn machine_type(mut self, mt: MachineType) -> Self {
        self.spec.machine_type = mt;
        self
    }
    
    pub fn numa(mut self, config: NumaSpec) -> Self {
        self.spec.numa = Some(config);
        self
    }
    
    pub fn cpu_pinning(mut self, pinning: CpuPinning) -> Self {
        self.spec.cpu_pinning = Some(pinning);
        self
    }
    
    pub fn security(mut self, sec: VmSecuritySpec) -> Self {
        self.spec.security = Some(sec);
        self
    }
    
    pub fn metadata(mut self, key: &str, value: &str) -> Self {
        self.spec.metadata.insert(key.to_string(), value.to_string());
        self
    }
    
    pub fn build(self) -> VmSpec {
        self.spec
    }
}

impl Default for VmSpecBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Disk specification
#[derive(Debug, Clone)]
pub struct DiskSpec {
    /// Disk path or identifier
    pub path: PathBuf,
    /// Disk size in bytes
    pub size: u64,
    /// Disk format
    pub format: DiskFormat,
    /// Read-only
    pub readonly: bool,
    /// Disk interface (virtio, scsi, ide, sata)
    pub interface: DiskInterface,
    /// Boot disk
    pub bootable: bool,
    /// Cache mode
    pub cache_mode: CacheMode,
    /// IO mode
    pub io_mode: IoMode,
}

impl DiskSpec {
    pub fn new(path: &str, size: u64) -> Self {
        Self {
            path: PathBuf::from(path),
            size,
            format: DiskFormat::Qcow2,
            readonly: false,
            interface: DiskInterface::Virtio,
            bootable: false,
            cache_mode: CacheMode::Writeback,
            io_mode: IoMode::Native,
        }
    }
    
    pub fn bootable(mut self) -> Self {
        self.bootable = true;
        self
    }
    
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }
    
    pub fn format(mut self, fmt: DiskFormat) -> Self {
        self.format = fmt;
        self
    }
    
    pub fn interface(mut self, iface: DiskInterface) -> Self {
        self.interface = iface;
        self
    }
}

/// Disk format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    /// Raw disk image
    Raw,
    /// QEMU Copy-On-Write v2
    Qcow2,
    /// VMware disk format
    Vmdk,
    /// VirtualBox disk image
    Vdi,
    /// Virtual Hard Disk (Microsoft)
    Vhd,
    /// Virtual Hard Disk v2 (Microsoft)
    Vhdx,
}

impl Default for DiskFormat {
    fn default() -> Self {
        Self::Qcow2
    }
}

/// Disk interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskInterface {
    Virtio,
    Scsi,
    Ide,
    Sata,
    Nvme,
}

impl Default for DiskInterface {
    fn default() -> Self {
        Self::Virtio
    }
}

/// Disk cache mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    None,
    Writeback,
    Writethrough,
    DirectSync,
    Unsafe,
}

impl Default for CacheMode {
    fn default() -> Self {
        Self::Writeback
    }
}

/// IO mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoMode {
    Native,
    Threads,
    IoUring,
}

impl Default for IoMode {
    fn default() -> Self {
        Self::Native
    }
}

/// Network specification
#[derive(Debug, Clone)]
pub struct NetworkSpec {
    /// Network type
    pub net_type: NetworkType,
    /// MAC address (auto-generated if None)
    pub mac_address: Option<String>,
    /// Device model
    pub model: NicModel,
    /// VLAN ID
    pub vlan_id: Option<u16>,
    /// QoS configuration
    pub qos: Option<NetworkQosSpec>,
    /// Bridge name (for bridged mode)
    pub bridge: Option<String>,
    /// Network name (for virtual network)
    pub network: Option<String>,
}

impl NetworkSpec {
    pub fn bridged(bridge: &str) -> Self {
        Self {
            net_type: NetworkType::Bridge,
            mac_address: None,
            model: NicModel::Virtio,
            vlan_id: None,
            qos: None,
            bridge: Some(bridge.to_string()),
            network: None,
        }
    }
    
    pub fn nat() -> Self {
        Self {
            net_type: NetworkType::Nat,
            mac_address: None,
            model: NicModel::Virtio,
            vlan_id: None,
            qos: None,
            bridge: None,
            network: None,
        }
    }
    
    pub fn internal(name: &str) -> Self {
        Self {
            net_type: NetworkType::Internal,
            mac_address: None,
            model: NicModel::Virtio,
            vlan_id: None,
            qos: None,
            bridge: None,
            network: Some(name.to_string()),
        }
    }
    
    pub fn with_mac(mut self, mac: &str) -> Self {
        self.mac_address = Some(mac.to_string());
        self
    }
    
    pub fn with_model(mut self, model: NicModel) -> Self {
        self.model = model;
        self
    }
    
    pub fn with_vlan(mut self, vlan: u16) -> Self {
        self.vlan_id = Some(vlan);
        self
    }
}

/// Network type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    /// Bridged networking
    Bridge,
    /// NAT networking
    Nat,
    /// Internal-only networking
    Internal,
    /// Host-only networking
    HostOnly,
    /// Direct passthrough (macvtap)
    Passthrough,
    /// SR-IOV Virtual Function
    SriovVf,
}

impl Default for NetworkType {
    fn default() -> Self {
        Self::Nat
    }
}

/// NIC model
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicModel {
    Virtio,
    E1000,
    E1000e,
    Rtl8139,
    Vmxnet3,
}

impl Default for NicModel {
    fn default() -> Self {
        Self::Virtio
    }
}

/// Network QoS specification
#[derive(Debug, Clone)]
pub struct NetworkQosSpec {
    /// Inbound bandwidth limit (bytes/sec)
    pub inbound_limit: Option<u64>,
    /// Outbound bandwidth limit (bytes/sec)
    pub outbound_limit: Option<u64>,
    /// Inbound burst (bytes)
    pub inbound_burst: Option<u64>,
    /// Outbound burst (bytes)
    pub outbound_burst: Option<u64>,
}

/// Boot device
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootDevice {
    Disk,
    CdRom,
    Network,
    Floppy,
}

/// Firmware type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareType {
    Bios,
    Uefi,
    UefiSecure,
}

impl Default for FirmwareType {
    fn default() -> Self {
        Self::Bios
    }
}

/// CPU model
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuModel {
    /// Host passthrough (best performance)
    Host,
    /// Named CPU model
    Named(String),
    /// Custom CPU with features
    Custom {
        base: String,
        features_add: Vec<String>,
        features_remove: Vec<String>,
    },
}

impl Default for CpuModel {
    fn default() -> Self {
        Self::Host
    }
}

/// Machine type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineType {
    /// Intel Q35 chipset (recommended)
    Q35,
    /// Intel i440FX (legacy)
    I440fx,
    /// QEMU virt (ARM)
    Virt,
    /// Custom
    Custom,
}

impl Default for MachineType {
    fn default() -> Self {
        Self::Q35
    }
}

/// NUMA specification
#[derive(Debug, Clone)]
pub struct NumaSpec {
    /// NUMA nodes
    pub nodes: Vec<NumaNode>,
}

/// NUMA node
#[derive(Debug, Clone)]
pub struct NumaNode {
    /// Node ID
    pub id: u32,
    /// Memory size in MB
    pub memory_mb: u64,
    /// CPUs assigned to this node
    pub cpus: Vec<u32>,
    /// Memory latency to other nodes
    pub distances: HashMap<u32, u32>,
}

/// CPU pinning configuration
#[derive(Debug, Clone)]
pub struct CpuPinning {
    /// vCPU to pCPU mapping
    pub vcpu_to_pcpu: HashMap<u32, Vec<u32>>,
    /// Emulator thread pinning
    pub emulator_pin: Option<Vec<u32>>,
    /// IO thread pinning
    pub io_pin: Option<Vec<u32>>,
}

/// VM security specification
#[derive(Debug, Clone)]
pub struct VmSecuritySpec {
    /// Enable secure boot
    pub secure_boot: bool,
    /// Enable TPM
    pub tpm: bool,
    /// TPM version (1.2 or 2.0)
    pub tpm_version: Option<String>,
    /// Memory encryption
    pub memory_encryption: bool,
    /// SEV (AMD Secure Encrypted Virtualization)
    pub sev: bool,
    /// SEV-ES
    pub sev_es: bool,
    /// SEV-SNP
    pub sev_snp: bool,
    /// TDX (Intel Trust Domain Extensions)
    pub tdx: bool,
}

impl Default for VmSecuritySpec {
    fn default() -> Self {
        Self {
            secure_boot: false,
            tpm: false,
            tpm_version: None,
            memory_encryption: false,
            sev: false,
            sev_es: false,
            sev_snp: false,
            tdx: false,
        }
    }
}

/// VM runtime information
#[derive(Debug, Clone)]
pub struct VmInfo {
    /// VM ID
    pub id: VmId,
    /// VM name
    pub name: String,
    /// Current status
    pub status: VmStatus,
    /// Creation time
    pub created_at: Instant,
    /// Last status change
    pub status_changed_at: Instant,
    /// vCPU count
    pub vcpus: u32,
    /// Memory size (MB)
    pub memory_mb: u64,
    /// Disk count
    pub disk_count: usize,
    /// Network interface count
    pub nic_count: usize,
    /// Total CPU time used (nanoseconds)
    pub cpu_time_ns: u64,
    /// Current CPU usage (percentage, 0-100 * vcpus)
    pub cpu_usage: f64,
    /// Current memory usage (bytes)
    pub memory_usage: u64,
    /// Disk read bytes
    pub disk_read_bytes: u64,
    /// Disk write bytes
    pub disk_write_bytes: u64,
    /// Network receive bytes
    pub net_rx_bytes: u64,
    /// Network transmit bytes
    pub net_tx_bytes: u64,
    /// Host where VM is running
    pub host: Option<String>,
    /// Snapshot count
    pub snapshot_count: usize,
    /// Migration state
    pub migration_state: Option<String>,
}

/// Hypervisor error types
#[derive(Debug, Clone)]
pub enum HypervisorError {
    /// VM not found
    VmNotFound(VmId),
    /// VM already exists with this name
    VmAlreadyExists(String),
    /// Invalid VM state for operation
    InvalidVmState { current: VmStatus, expected: Vec<VmStatus> },
    /// Resource not available
    ResourceUnavailable { resource: String, requested: u64, available: u64 },
    /// Configuration error
    ConfigError(String),
    /// Storage error
    StorageError(String),
    /// Network error
    NetworkError(String),
    /// Security error
    SecurityError(String),
    /// Migration error
    MigrationError(String),
    /// Snapshot error
    SnapshotError(String),
    /// Cluster error
    ClusterError(String),
    /// Scheduler error
    SchedulerError(String),
    /// Resource limit exceeded
    ResourceLimit(String),
    /// Not found
    NotFound(String),
    /// Invalid state
    InvalidState(String),
    /// Not supported
    NotSupported(String),
    /// Invalid argument
    InvalidArgument(String),
    /// Internal error
    InternalError(String),
    /// Invalid operation
    InvalidOperation(String),
    /// Quota exceeded
    QuotaExceeded(String),
}

impl std::fmt::Display for HypervisorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VmNotFound(id) => write!(f, "VM not found: {}", id),
            Self::VmAlreadyExists(name) => write!(f, "VM already exists: {}", name),
            Self::InvalidVmState { current, expected } => {
                write!(f, "Invalid VM state: {} (expected one of {:?})", current, expected)
            }
            Self::ResourceUnavailable { resource, requested, available } => {
                write!(f, "Resource unavailable: {} (requested {}, available {})", 
                       resource, requested, available)
            }
            Self::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            Self::StorageError(msg) => write!(f, "Storage error: {}", msg),
            Self::NetworkError(msg) => write!(f, "Network error: {}", msg),
            Self::SecurityError(msg) => write!(f, "Security error: {}", msg),
            Self::MigrationError(msg) => write!(f, "Migration error: {}", msg),
            Self::SnapshotError(msg) => write!(f, "Snapshot error: {}", msg),
            Self::ClusterError(msg) => write!(f, "Cluster error: {}", msg),
            Self::SchedulerError(msg) => write!(f, "Scheduler error: {}", msg),
            Self::ResourceLimit(msg) => write!(f, "Resource limit: {}", msg),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            Self::NotSupported(msg) => write!(f, "Not supported: {}", msg),
            Self::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
            Self::InvalidOperation(msg) => write!(f, "Invalid operation: {}", msg),
            Self::QuotaExceeded(msg) => write!(f, "Quota exceeded: {}", msg),
        }
    }
}

impl std::error::Error for HypervisorError {}

/// Result type for hypervisor operations
pub type HypervisorResult<T> = Result<T, HypervisorError>;

/// Handle to a running VM
pub struct VmHandle {
    id: VmId,
    hypervisor: Arc<Hypervisor>,
}

impl VmHandle {
    pub fn new(id: VmId, hypervisor: Arc<Hypervisor>) -> Self {
        Self { id, hypervisor }
    }
    
    pub fn id(&self) -> VmId {
        self.id
    }
    
    pub fn info(&self) -> HypervisorResult<VmInfo> {
        self.hypervisor.vm_info(self.id)
    }
    
    pub fn status(&self) -> HypervisorResult<VmStatus> {
        self.hypervisor.vm_status(self.id)
    }
    
    pub fn start(&self) -> HypervisorResult<()> {
        self.hypervisor.start_vm(self.id)
    }
    
    pub fn stop(&self) -> HypervisorResult<()> {
        self.hypervisor.stop_vm(self.id)
    }
    
    pub fn pause(&self) -> HypervisorResult<()> {
        self.hypervisor.pause_vm(self.id)
    }
    
    pub fn resume(&self) -> HypervisorResult<()> {
        self.hypervisor.resume_vm(self.id)
    }
    
    pub fn reset(&self) -> HypervisorResult<()> {
        self.hypervisor.reset_vm(self.id)
    }
    
    pub fn snapshot(&self, name: &str) -> HypervisorResult<String> {
        self.hypervisor.snapshot_vm(self.id, name)
    }
    
    pub fn restore_snapshot(&self, name: &str) -> HypervisorResult<()> {
        self.hypervisor.restore_vm_snapshot(self.id, name)
    }
}

/// Hypervisor configuration
#[derive(Debug, Clone)]
pub struct HypervisorConfig {
    /// Node name
    pub node_name: String,
    /// Data directory
    pub data_dir: PathBuf,
    /// Log directory
    pub log_dir: PathBuf,
    /// Maximum VMs
    pub max_vms: u64,
    /// Total CPU cores available
    pub total_cpus: u32,
    /// Total memory available (MB)
    pub total_memory_mb: u64,
    /// Storage pools configuration
    pub storage_pools: Vec<StoragePoolConfig>,
    /// Network configuration
    pub networks: Vec<NetworkConfig>,
    /// Feature flags
    pub features: HypervisorFeatures,
    /// Enable overcommit for CPU
    pub cpu_overcommit: bool,
    /// Enable overcommit for memory
    pub memory_overcommit: bool,
    /// Memory overcommit ratio (e.g., 1.5 = 150%)
    pub memory_overcommit_ratio: f64,
    /// Enable KSM
    pub enable_ksm: bool,
    /// API listen address
    pub api_listen: String,
    /// API port
    pub api_port: u16,
}

impl Default for HypervisorConfig {
    fn default() -> Self {
        Self {
            node_name: String::from("localhost"),
            data_dir: PathBuf::from("/var/lib/nexahv"),
            log_dir: PathBuf::from("/var/log/nexahv"),
            max_vms: 1000,
            total_cpus: 64,
            total_memory_mb: 256 * 1024, // 256GB
            storage_pools: Vec::new(),
            networks: Vec::new(),
            features: HypervisorFeatures::default(),
            cpu_overcommit: true,
            memory_overcommit: true,
            memory_overcommit_ratio: 1.5,
            enable_ksm: true,
            api_listen: String::from("0.0.0.0"),
            api_port: 9090,
        }
    }
}

/// Storage pool configuration
#[derive(Debug, Clone)]
pub struct StoragePoolConfig {
    pub name: String,
    pub path: PathBuf,
    pub capacity: u64,
}

/// Network configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub name: String,
    pub bridge: Option<String>,
    pub net_type: NetworkType,
}

/// Main Hypervisor struct
pub struct Hypervisor {
    config: HypervisorConfig,
    /// VM registry
    vms: RwLock<HashMap<VmId, Arc<VmInstance>>>,
    /// Name to ID mapping
    vm_names: RwLock<HashMap<String, VmId>>,
    /// ID generator
    next_id: AtomicU64,
    /// Start time
    start_time: Instant,
    /// Global statistics
    stats: RwLock<HypervisorStats>,
    /// Resource pools
    cpu_pool: Arc<CpuPool>,
    memory_pool: Arc<MemoryPool>,
    storage_pools: RwLock<HashMap<String, Arc<StoragePool>>>,
    network_pools: RwLock<HashMap<String, Arc<NetworkPool>>>,
    /// VM scheduler
    scheduler: Arc<VmScheduler>,
    /// Memory manager
    memory_manager: Arc<MemoryManager>,
}

impl Hypervisor {
    /// Create a new hypervisor with default configuration
    pub fn new() -> Arc<Self> {
        Self::with_config(HypervisorConfig::default())
    }
    
    /// Create a new hypervisor with custom configuration
    pub fn with_config(config: HypervisorConfig) -> Arc<Self> {
        let cpu_pool = Arc::new(CpuPool::new(config.total_cpus));
        let memory_pool = Arc::new(MemoryPool::new(config.total_memory_mb));
        let scheduler = Arc::new(VmScheduler::new());
        let memory_manager = Arc::new(MemoryManager::new(memory_pool.clone(), config.enable_ksm));
        
        Arc::new(Self {
            config,
            vms: RwLock::new(HashMap::new()),
            vm_names: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            start_time: Instant::now(),
            stats: RwLock::new(HypervisorStats::default()),
            cpu_pool,
            memory_pool,
            storage_pools: RwLock::new(HashMap::new()),
            network_pools: RwLock::new(HashMap::new()),
            scheduler,
            memory_manager,
        })
    }
    
    /// Create a builder for configuration
    pub fn builder() -> HypervisorBuilder {
        HypervisorBuilder::new()
    }
    
    /// Generate a new VM ID
    fn next_vm_id(&self) -> VmId {
        VmId::new(self.next_id.fetch_add(1, Ordering::SeqCst))
    }
    
    /// Create a new VM
    pub fn create_vm(self: &Arc<Self>, spec: VmSpec) -> HypervisorResult<VmId> {
        // Check if name already exists
        if self.vm_names.read().unwrap().contains_key(&spec.name) {
            return Err(HypervisorError::VmAlreadyExists(spec.name));
        }
        
        // Check resource availability
        self.check_resources(&spec)?;
        
        // Generate ID and create VM instance
        let id = self.next_vm_id();
        let vm = Arc::new(VmInstance::new(id, spec.clone()));
        
        // Register VM
        {
            let mut vms = self.vms.write().unwrap();
            let mut names = self.vm_names.write().unwrap();
            
            vms.insert(id, vm.clone());
            names.insert(spec.name.clone(), id);
        }
        
        // Allocate resources
        self.allocate_resources(&spec)?;
        
        // Update statistics
        {
            let mut stats = self.stats.write().unwrap();
            stats.total_vms += 1;
            stats.total_vcpus += spec.vcpus as u64;
            stats.total_memory += spec.memory_mb * 1024 * 1024;
        }
        
        Ok(id)
    }
    
    /// Check resource availability
    fn check_resources(&self, spec: &VmSpec) -> HypervisorResult<()> {
        // Check CPU
        let available_cpus = self.cpu_pool.available();
        let requested_cpus = if self.config.cpu_overcommit {
            spec.vcpus / 2 // Allow 2:1 overcommit
        } else {
            spec.vcpus
        };
        
        if requested_cpus > available_cpus {
            return Err(HypervisorError::ResourceUnavailable {
                resource: "CPU".to_string(),
                requested: requested_cpus as u64,
                available: available_cpus as u64,
            });
        }
        
        // Check memory
        let available_memory = self.memory_pool.available();
        let requested_memory = if self.config.memory_overcommit {
            (spec.memory_mb as f64 / self.config.memory_overcommit_ratio) as u64
        } else {
            spec.memory_mb
        };
        
        if requested_memory > available_memory {
            return Err(HypervisorError::ResourceUnavailable {
                resource: "Memory".to_string(),
                requested: requested_memory,
                available: available_memory,
            });
        }
        
        Ok(())
    }
    
    /// Allocate resources for a VM
    fn allocate_resources(&self, spec: &VmSpec) -> HypervisorResult<()> {
        self.cpu_pool.allocate(spec.vcpus)?;
        self.memory_pool.allocate(spec.memory_mb)?;
        Ok(())
    }
    
    /// Start a VM
    pub fn start_vm(&self, id: VmId) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.start()?;
        
        let mut stats = self.stats.write().unwrap();
        stats.running_vms += 1;
        
        Ok(())
    }
    
    /// Stop a VM
    pub fn stop_vm(&self, id: VmId) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.stop()?;
        
        let mut stats = self.stats.write().unwrap();
        if stats.running_vms > 0 {
            stats.running_vms -= 1;
        }
        
        Ok(())
    }
    
    /// Pause a VM
    pub fn pause_vm(&self, id: VmId) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.pause()?;
        
        let mut stats = self.stats.write().unwrap();
        if stats.running_vms > 0 {
            stats.running_vms -= 1;
        }
        stats.paused_vms += 1;
        
        Ok(())
    }
    
    /// Resume a VM
    pub fn resume_vm(&self, id: VmId) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.resume()?;
        
        let mut stats = self.stats.write().unwrap();
        if stats.paused_vms > 0 {
            stats.paused_vms -= 1;
        }
        stats.running_vms += 1;
        
        Ok(())
    }
    
    /// Reset a VM
    pub fn reset_vm(&self, id: VmId) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.reset()
    }
    
    /// Destroy a VM
    pub fn destroy_vm(&self, id: VmId) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        
        // Stop if running
        if vm.status() == VmStatus::Running || vm.status() == VmStatus::Paused {
            vm.stop()?;
        }
        
        // Release resources
        let spec = vm.spec();
        self.cpu_pool.release(spec.vcpus);
        self.memory_pool.release(spec.memory_mb);
        
        // Remove from registry
        {
            let mut vms = self.vms.write().unwrap();
            let mut names = self.vm_names.write().unwrap();
            
            vms.remove(&id);
            names.remove(&spec.name);
        }
        
        // Update statistics
        {
            let mut stats = self.stats.write().unwrap();
            if stats.total_vms > 0 {
                stats.total_vms -= 1;
            }
            stats.total_vcpus -= spec.vcpus as u64;
            stats.total_memory -= spec.memory_mb * 1024 * 1024;
        }
        
        Ok(())
    }
    
    /// Get VM status
    pub fn vm_status(&self, id: VmId) -> HypervisorResult<VmStatus> {
        let vm = self.get_vm(id)?;
        Ok(vm.status())
    }
    
    /// Get VM information
    pub fn vm_info(&self, id: VmId) -> HypervisorResult<VmInfo> {
        let vm = self.get_vm(id)?;
        Ok(vm.info())
    }
    
    /// Create a VM snapshot
    pub fn snapshot_vm(&self, id: VmId, name: &str) -> HypervisorResult<String> {
        let vm = self.get_vm(id)?;
        let snapshot_id = vm.snapshot(name)?;
        
        let mut stats = self.stats.write().unwrap();
        stats.snapshots_created += 1;
        
        Ok(snapshot_id)
    }
    
    /// Restore VM from snapshot
    pub fn restore_vm_snapshot(&self, id: VmId, name: &str) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.restore_snapshot(name)
    }
    
    /// List all VMs
    pub fn list_vms(&self) -> Vec<VmInfo> {
        self.vms.read().unwrap()
            .values()
            .map(|vm| vm.info())
            .collect()
    }
    
    // ========================================================================
    // VGA / Console Access
    // ========================================================================
    
    /// Get VGA framebuffer for a VM
    pub fn get_vm_vga_framebuffer(&self, id: VmId) -> HypervisorResult<Option<Vec<u8>>> {
        let vm = self.get_vm(id)?;
        Ok(vm.get_vga_framebuffer())
    }
    
    /// Get VGA dimensions for a VM
    pub fn get_vm_vga_dimensions(&self, id: VmId) -> HypervisorResult<Option<(u32, u32)>> {
        let vm = self.get_vm(id)?;
        Ok(vm.get_vga_dimensions())
    }
    
    /// Check if VM has VGA device
    pub fn vm_has_vga(&self, id: VmId) -> HypervisorResult<bool> {
        let vm = self.get_vm(id)?;
        Ok(vm.has_vga())
    }
    
    /// Write to VM's VGA console
    pub fn vm_vga_write(&self, id: VmId, text: &str) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.vga_write(text);
        Ok(())
    }
    
    /// Get VM by ID
    fn get_vm(&self, id: VmId) -> HypervisorResult<Arc<VmInstance>> {
        self.vms.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::VmNotFound(id))
    }
    
    /// Get VM by name
    pub fn get_vm_by_name(&self, name: &str) -> HypervisorResult<Arc<VmInstance>> {
        let id = self.vm_names.read().unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| HypervisorError::VmNotFound(VmId::new(0)))?;
        self.get_vm(id)
    }
    
    /// Get hypervisor statistics
    pub fn statistics(&self) -> HypervisorStats {
        let mut stats = self.stats.read().unwrap().clone();
        stats.uptime_seconds = self.start_time.elapsed().as_secs();
        stats
    }
    
    /// Get configuration
    pub fn config(&self) -> &HypervisorConfig {
        &self.config
    }
    
    /// Get feature flags
    pub fn features(&self) -> &HypervisorFeatures {
        &self.config.features
    }
}

impl Default for Hypervisor {
    fn default() -> Self {
        // This returns the inner value, not Arc
        // Use Hypervisor::new() to get Arc<Hypervisor>
        Self {
            config: HypervisorConfig::default(),
            vms: RwLock::new(HashMap::new()),
            vm_names: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            start_time: Instant::now(),
            stats: RwLock::new(HypervisorStats::default()),
            cpu_pool: Arc::new(CpuPool::new(64)),
            memory_pool: Arc::new(MemoryPool::new(256 * 1024)),
            storage_pools: RwLock::new(HashMap::new()),
            network_pools: RwLock::new(HashMap::new()),
            scheduler: Arc::new(VmScheduler::new()),
            memory_manager: Arc::new(MemoryManager::new(
                Arc::new(MemoryPool::new(256 * 1024)),
                true,
            )),
        }
    }
}

/// Hypervisor builder
pub struct HypervisorBuilder {
    config: HypervisorConfig,
}

impl HypervisorBuilder {
    pub fn new() -> Self {
        Self {
            config: HypervisorConfig::default(),
        }
    }
    
    pub fn node_name(mut self, name: &str) -> Self {
        self.config.node_name = name.to_string();
        self
    }
    
    pub fn data_dir(mut self, path: &str) -> Self {
        self.config.data_dir = PathBuf::from(path);
        self
    }
    
    pub fn total_cpus(mut self, count: u32) -> Self {
        self.config.total_cpus = count;
        self
    }
    
    pub fn total_memory_mb(mut self, mb: u64) -> Self {
        self.config.total_memory_mb = mb;
        self
    }
    
    pub fn features(mut self, features: HypervisorFeatures) -> Self {
        self.config.features = features;
        self
    }
    
    pub fn enable_ksm(mut self, enable: bool) -> Self {
        self.config.enable_ksm = enable;
        self
    }
    
    pub fn memory_overcommit(mut self, enable: bool, ratio: f64) -> Self {
        self.config.memory_overcommit = enable;
        self.config.memory_overcommit_ratio = ratio;
        self
    }
    
    pub fn api_listen(mut self, addr: &str, port: u16) -> Self {
        self.config.api_listen = addr.to_string();
        self.config.api_port = port;
        self
    }
    
    pub fn build(self) -> Arc<Hypervisor> {
        Hypervisor::with_config(self.config)
    }
}

impl Default for HypervisorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// VM instance (internal representation)
pub struct VmInstance {
    id: VmId,
    spec: VmSpec,
    status: RwLock<VmStatus>,
    created_at: Instant,
    status_changed_at: RwLock<Instant>,
    stats: RwLock<VmInstanceStats>,
    snapshots: RwLock<HashMap<String, VmSnapshot>>,
    vm: RwLock<Option<Arc<crate::vm::VirtualMachine>>>,
}

#[derive(Debug, Clone, Default)]
struct VmInstanceStats {
    cpu_time_ns: u64,
    cpu_usage: f64,
    memory_usage: u64,
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    net_rx_bytes: u64,
    net_tx_bytes: u64,
}

/// VM snapshot
#[derive(Debug, Clone)]
struct VmSnapshot {
    name: String,
    created_at: Instant,
    parent: Option<String>,
    description: Option<String>,
}

impl VmInstance {
    pub fn new(id: VmId, spec: VmSpec) -> Self {
        Self {
            id,
            spec,
            status: RwLock::new(VmStatus::Created),
            created_at: Instant::now(),
            status_changed_at: RwLock::new(Instant::now()),
            stats: RwLock::new(VmInstanceStats::default()),
            snapshots: RwLock::new(HashMap::new()),
            vm: RwLock::new(None),
        }
    }
    
    pub fn id(&self) -> VmId {
        self.id
    }
    
    pub fn spec(&self) -> &VmSpec {
        &self.spec
    }
    
    pub fn status(&self) -> VmStatus {
        *self.status.read().unwrap()
    }
    
    fn set_status(&self, status: VmStatus) {
        *self.status.write().unwrap() = status;
        *self.status_changed_at.write().unwrap() = Instant::now();
    }
    
    pub fn start(&self) -> HypervisorResult<()> {
        let current = self.status();
        if current != VmStatus::Created && current != VmStatus::Stopped && current != VmStatus::Saved {
            return Err(HypervisorError::InvalidVmState {
                current,
                expected: vec![VmStatus::Created, VmStatus::Stopped, VmStatus::Saved],
            });
        }
        
        self.set_status(VmStatus::Starting);
        
        // Determine firmware type from spec
        let firmware_type = match self.spec.firmware {
            FirmwareType::Bios => crate::firmware::FirmwareType::Bios,
            FirmwareType::Uefi => crate::firmware::FirmwareType::Uefi,
            FirmwareType::UefiSecure => crate::firmware::FirmwareType::UefiSecure,
        };
        
        // Create underlying VM
        let vm_config = crate::vm::VmConfig {
            memory_mb: self.spec.memory_mb as usize,
            cpus: self.spec.vcpus as usize,
            firmware_type,
            enable_pic: true,
            enable_pit: true,
            enable_serial: true,
            enable_rtc: true,
            enable_apic: self.spec.vcpus > 1,
            enable_vga: true,  // Enable VGA for console display
            enable_tracing: true,
            max_trace_size: 10000,
            name: self.spec.name.clone(),
            nested_virt: self.spec.nested_virt,
            numa_nodes: Vec::new(),
        };
        
        let vm = crate::vm::VirtualMachine::with_config(vm_config);
        vm.start();
        
        *self.vm.write().unwrap() = Some(Arc::new(vm));
        self.set_status(VmStatus::Running);
        
        Ok(())
    }
    
    pub fn stop(&self) -> HypervisorResult<()> {
        let current = self.status();
        if current != VmStatus::Running && current != VmStatus::Paused {
            return Err(HypervisorError::InvalidVmState {
                current,
                expected: vec![VmStatus::Running, VmStatus::Paused],
            });
        }
        
        self.set_status(VmStatus::Stopping);
        
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.stop();
        }
        
        *self.vm.write().unwrap() = None;
        self.set_status(VmStatus::Stopped);
        
        Ok(())
    }
    
    pub fn pause(&self) -> HypervisorResult<()> {
        let current = self.status();
        if current != VmStatus::Running {
            return Err(HypervisorError::InvalidVmState {
                current,
                expected: vec![VmStatus::Running],
            });
        }
        
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.pause_vm();
        }
        
        self.set_status(VmStatus::Paused);
        Ok(())
    }
    
    pub fn resume(&self) -> HypervisorResult<()> {
        let current = self.status();
        if current != VmStatus::Paused {
            return Err(HypervisorError::InvalidVmState {
                current,
                expected: vec![VmStatus::Paused],
            });
        }
        
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.resume_vm();
        }
        
        self.set_status(VmStatus::Running);
        Ok(())
    }
    
    pub fn reset(&self) -> HypervisorResult<()> {
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.reset();
        }
        Ok(())
    }
    
    /// Get VGA framebuffer data for console display
    pub fn get_vga_framebuffer(&self) -> Option<Vec<u8>> {
        self.vm.read().unwrap()
            .as_ref()
            .and_then(|vm| vm.get_vga_framebuffer())
    }
    
    /// Get VGA display dimensions (width, height)
    pub fn get_vga_dimensions(&self) -> Option<(u32, u32)> {
        self.vm.read().unwrap()
            .as_ref()
            .and_then(|vm| vm.get_vga_dimensions())
    }
    
    /// Check if VM has VGA device
    pub fn has_vga(&self) -> bool {
        self.vm.read().unwrap()
            .as_ref()
            .map(|vm| vm.has_vga())
            .unwrap_or(false)
    }
    
    /// Write to VGA console (text mode)
    pub fn vga_write(&self, text: &str) {
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.vga_write(text);
        }
    }

    pub fn snapshot(&self, name: &str) -> HypervisorResult<String> {
        let snap = VmSnapshot {
            name: name.to_string(),
            created_at: Instant::now(),
            parent: None,
            description: None,
        };
        
        // Take underlying VM snapshot
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.snapshot(name);
        }
        
        self.snapshots.write().unwrap().insert(name.to_string(), snap);
        Ok(name.to_string())
    }
    
    pub fn restore_snapshot(&self, name: &str) -> HypervisorResult<()> {
        if !self.snapshots.read().unwrap().contains_key(name) {
            return Err(HypervisorError::SnapshotError(
                format!("Snapshot '{}' not found", name)
            ));
        }
        
        if let Some(vm) = self.vm.read().unwrap().as_ref() {
            vm.restore_by_name(name);
        }
        
        Ok(())
    }
    
    pub fn info(&self) -> VmInfo {
        let stats = self.stats.read().unwrap();
        
        VmInfo {
            id: self.id,
            name: self.spec.name.clone(),
            status: self.status(),
            created_at: self.created_at,
            status_changed_at: *self.status_changed_at.read().unwrap(),
            vcpus: self.spec.vcpus,
            memory_mb: self.spec.memory_mb,
            disk_count: self.spec.disks.len(),
            nic_count: self.spec.networks.len(),
            cpu_time_ns: stats.cpu_time_ns,
            cpu_usage: stats.cpu_usage,
            memory_usage: stats.memory_usage,
            disk_read_bytes: stats.disk_read_bytes,
            disk_write_bytes: stats.disk_write_bytes,
            net_rx_bytes: stats.net_rx_bytes,
            net_tx_bytes: stats.net_tx_bytes,
            host: None,
            snapshot_count: self.snapshots.read().unwrap().len(),
            migration_state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vm_spec_builder() {
        let spec = VmSpec::builder()
            .name("test-vm")
            .vcpus(4)
            .memory_mb(4096)
            .disk(DiskSpec::new("test.qcow2", 10 * 1024 * 1024 * 1024).bootable())
            .network(NetworkSpec::nat())
            .build();
        
        assert_eq!(spec.name, "test-vm");
        assert_eq!(spec.vcpus, 4);
        assert_eq!(spec.memory_mb, 4096);
        assert_eq!(spec.disks.len(), 1);
        assert_eq!(spec.networks.len(), 1);
    }
    
    #[test]
    fn test_hypervisor_create_vm() {
        let hv = Hypervisor::new();
        
        let spec = VmSpec::builder()
            .name("test-vm-1")
            .vcpus(2)
            .memory_mb(2048)
            .build();
        
        let id = hv.create_vm(spec).unwrap();
        assert_eq!(hv.vm_status(id).unwrap(), VmStatus::Created);
    }
    
    #[test]
    fn test_hypervisor_vm_lifecycle() {
        let hv = Hypervisor::new();
        
        let spec = VmSpec::builder()
            .name("lifecycle-test")
            .vcpus(1)
            .memory_mb(512)
            .build();
        
        let id = hv.create_vm(spec).unwrap();
        
        // Start
        hv.start_vm(id).unwrap();
        assert_eq!(hv.vm_status(id).unwrap(), VmStatus::Running);
        
        // Pause
        hv.pause_vm(id).unwrap();
        assert_eq!(hv.vm_status(id).unwrap(), VmStatus::Paused);
        
        // Resume
        hv.resume_vm(id).unwrap();
        assert_eq!(hv.vm_status(id).unwrap(), VmStatus::Running);
        
        // Stop
        hv.stop_vm(id).unwrap();
        assert_eq!(hv.vm_status(id).unwrap(), VmStatus::Stopped);
        
        // Destroy
        hv.destroy_vm(id).unwrap();
        assert!(hv.vm_status(id).is_err());
    }
    
    #[test]
    fn test_hypervisor_snapshot() {
        let hv = Hypervisor::new();
        
        let spec = VmSpec::builder()
            .name("snapshot-test")
            .vcpus(1)
            .memory_mb(512)
            .build();
        
        let id = hv.create_vm(spec).unwrap();
        hv.start_vm(id).unwrap();
        
        // Create snapshot
        let snap_name = hv.snapshot_vm(id, "snap1").unwrap();
        assert_eq!(snap_name, "snap1");
        
        // Restore snapshot
        hv.restore_vm_snapshot(id, "snap1").unwrap();
        
        hv.stop_vm(id).unwrap();
    }
    
    #[test]
    fn test_hypervisor_statistics() {
        let hv = Hypervisor::new();
        
        let spec = VmSpec::builder()
            .name("stats-test")
            .vcpus(2)
            .memory_mb(1024)
            .build();
        
        hv.create_vm(spec).unwrap();
        
        let stats = hv.statistics();
        assert_eq!(stats.total_vms, 1);
        assert_eq!(stats.total_vcpus, 2);
    }
    
    #[test]
    fn test_vm_duplicate_name() {
        let hv = Hypervisor::new();
        
        let spec1 = VmSpec::builder()
            .name("duplicate")
            .vcpus(1)
            .memory_mb(512)
            .build();
        
        let spec2 = VmSpec::builder()
            .name("duplicate")
            .vcpus(1)
            .memory_mb(512)
            .build();
        
        hv.create_vm(spec1).unwrap();
        let result = hv.create_vm(spec2);
        
        assert!(matches!(result, Err(HypervisorError::VmAlreadyExists(_))));
    }
}
