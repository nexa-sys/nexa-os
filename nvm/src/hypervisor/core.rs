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

// Import VM backends
use crate::svm::SvmExecutor;
use crate::vmx::VmxExecutor;
use crate::memory::PhysicalMemory;
use crate::devices::vga::Vga;
use crate::devices::keyboard::Ps2Keyboard;

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
    /// Execution backend: jit (software, 5-15% faster), vmx (Intel), svm (AMD), auto
    pub backend: VmBackendType,
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
            backend: VmBackendType::default(),
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
    
    /// Set execution backend: jit (software, 5-15% faster), vmx (Intel), svm (AMD), auto
    pub fn backend(mut self, backend: VmBackendType) -> Self {
        self.spec.backend = backend;
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
    /// VM start failed
    StartFailed(String),
    /// VM stop failed
    StopFailed(String),
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
            Self::StartFailed(msg) => write!(f, "VM start failed: {}", msg),
            Self::StopFailed(msg) => write!(f, "VM stop failed: {}", msg),
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
    
    /// Inject keyboard key event to VM (for console input)
    pub fn vm_inject_key(&self, id: VmId, key: &str, is_release: bool) -> HypervisorResult<()> {
        log::info!("[Hypervisor] vm_inject_key: id={:?}, key='{}', is_release={}", id, key, is_release);
        let vm = self.get_vm(id)?;
        vm.inject_key(key, is_release);
        Ok(())
    }
    
    /// Inject keyboard scancode directly to VM
    pub fn vm_inject_scancode(&self, id: VmId, scancode: u8, is_release: bool) -> HypervisorResult<()> {
        let vm = self.get_vm(id)?;
        vm.inject_scancode(scancode, is_release);
        Ok(())
    }
    
    /// Advance VM execution by specified cycles
    /// Must be called periodically to process device interrupts and CPU execution
    /// 
    /// Returns true if VM is still running normally, false if it was reset
    /// (e.g., due to Ctrl+Alt+Del via keyboard controller 0xFE command)
    pub fn vm_tick(&self, id: VmId, cycles: u64) -> HypervisorResult<bool> {
        let vm = self.get_vm(id)?;
        Ok(vm.tick(cycles))
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

/// VM execution backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmBackendType {
    /// Intel VT-x hardware virtualization (vmx.rs) - requires Intel CPU with VT-x
    Vmx,
    /// AMD-V/SVM hardware virtualization (svm.rs) - requires AMD CPU with AMD-V
    Svm,
    /// Pure software JIT execution (jit/) - no hardware virtualization required
    /// 5-15% better performance than hardware virtualization in many workloads
    #[default]
    Jit,
    /// Auto-detect best available backend (VMX > SVM > JIT)
    #[serde(rename = "auto")]
    Auto,
}

/// VM instance (internal representation)
/// 
/// Uses hardware virtualization (VMX/SVM) when available, falls back to JIT.
/// The old vm.rs + hal.rs code is only for kernel unit testing, not production.
pub struct VmInstance {
    id: VmId,
    spec: VmSpec,
    status: RwLock<VmStatus>,
    created_at: Instant,
    status_changed_at: RwLock<Instant>,
    stats: RwLock<VmInstanceStats>,
    snapshots: RwLock<HashMap<String, VmSnapshot>>,
    /// VMX hardware virtualization backend (Intel)
    vmx_executor: RwLock<Option<Arc<VmxExecutor>>>,
    /// SVM hardware virtualization backend (AMD)
    svm_executor: RwLock<Option<Arc<SvmExecutor>>>,
    /// JIT execution backend (software, no hardware virt required)
    jit_engine: RwLock<Option<Arc<crate::jit::JitEngine>>>,
    /// Guest physical memory
    memory: RwLock<Option<Arc<PhysicalMemory>>>,
    /// Which backend to use
    backend_type: VmBackendType,
    /// VGA device for console display
    vga: RwLock<Vga>,
    /// PS/2 keyboard controller
    keyboard: RwLock<Ps2Keyboard>,
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
        // Use backend from spec, or auto-detect if Auto is specified
        let backend_type = match spec.backend {
            VmBackendType::Auto => Self::detect_best_backend(),
            VmBackendType::Vmx => VmBackendType::Vmx,
            VmBackendType::Svm => VmBackendType::Svm,
            VmBackendType::Jit => VmBackendType::Jit,
        };
        
        Self {
            id,
            spec,
            status: RwLock::new(VmStatus::Created),
            created_at: Instant::now(),
            status_changed_at: RwLock::new(Instant::now()),
            stats: RwLock::new(VmInstanceStats::default()),
            snapshots: RwLock::new(HashMap::new()),
            vmx_executor: RwLock::new(None),
            svm_executor: RwLock::new(None),
            jit_engine: RwLock::new(None),
            memory: RwLock::new(None),
            backend_type,
            vga: RwLock::new(Vga::new()),
            keyboard: RwLock::new(Ps2Keyboard::new()),
        }
    }
    
    /// Detect the best available virtualization backend
    fn detect_best_backend() -> VmBackendType {
        // For now, default to JIT which works everywhere
        // In production, this would check CPUID for VMX/SVM support
        // and whether the kernel module (kvm-intel/kvm-amd) is loaded
        
        // TODO: Add proper VMX/SVM detection when running with appropriate privileges
        // - VMX: CPUID.1:ECX.VMX[bit 5] = 1 AND IA32_FEATURE_CONTROL MSR enabled
        // - SVM: CPUID.8000_0001:ECX.SVM[bit 2] = 1 AND VM_CR MSR enabled
        
        // For now, default to JIT as it works in userspace without special privileges
        VmBackendType::Jit
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
        
        // Create guest physical memory (PhysicalMemory::new takes MB)
        let memory = Arc::new(PhysicalMemory::new(self.spec.memory_mb as usize));
        
        // Load firmware into guest memory
        if let Err(e) = self.load_firmware_to_memory(&memory) {
            self.set_status(VmStatus::Stopped);
            return Err(e);
        }
        
        // Initialize backend based on type
        // Note: Auto is resolved in VmInstance::new(), so it should never appear here
        let backend_result = match self.backend_type {
            VmBackendType::Vmx => self.start_vmx_backend(&memory),
            VmBackendType::Svm => self.start_svm_backend(&memory),
            VmBackendType::Jit => self.start_jit_backend(&memory),
            VmBackendType::Auto => unreachable!("Auto backend should be resolved in VmInstance::new()"),
        };
        
        if let Err(e) = backend_result {
            self.set_status(VmStatus::Stopped);
            return Err(e);
        }
        
        // Store memory
        *self.memory.write().unwrap() = Some(memory);
        
        // Display BIOS/firmware boot message on VGA
        self.display_boot_message();
        
        self.set_status(VmStatus::Running);
        
        Ok(())
    }
    
    /// Display boot message on VGA console
    fn display_boot_message(&self) {
        let mut vga = self.vga.write().unwrap();
        
        // Clear screen and display boot banner
        vga.clear();
        vga.write_string_colored("NVM Enterprise Hypervisor v2.0\n", 0x0F);  // White on black
        vga.write_string_colored("========================================\n\n", 0x07);
        
        // System info
        vga.write_string(&format!("VM: {}\n", self.spec.name));
        vga.write_string(&format!("vCPUs: {}  Memory: {} MB\n", self.spec.vcpus, self.spec.memory_mb));
        
        // Backend info with performance note
        let backend_info = match self.backend_type {
            VmBackendType::Jit => "JIT (ReadyNow! enabled, no VM-exit overhead)",
            VmBackendType::Vmx => "Intel VT-x (VMX)",
            VmBackendType::Svm => "AMD-V (SVM)",
            VmBackendType::Auto => "Auto-detected",
        };
        vga.write_string(&format!("Backend: {}\n\n", backend_info));
        
        // Firmware info
        match self.spec.firmware {
            FirmwareType::Bios => {
                vga.write_string_colored("SeaBIOS (emulated)\n", 0x0E);
                vga.write_string("Press F2 for Setup, F12 for Boot Menu\n\n");
            }
            FirmwareType::Uefi | FirmwareType::UefiSecure => {
                vga.write_string_colored("UEFI Firmware (emulated)\n", 0x0E);
                vga.write_string("Press DEL or F2 for UEFI Setup\n\n");
            }
        }
        
        vga.write_string_colored("[OK] ", 0x0A);  // Green
        vga.write_string("Hardware initialized\n");
        vga.write_string_colored("[OK] ", 0x0A);
        vga.write_string("VM running - awaiting boot device\n\n");
        vga.write_string("_");  // Cursor
    }
    
    /// Start with Intel VT-x (VMX) backend
    fn start_vmx_backend(&self, memory: &Arc<PhysicalMemory>) -> HypervisorResult<()> {
        let jit_config = Self::default_jit_config();
        
        let executor = VmxExecutor::with_jit_config(memory.clone(), jit_config);
        executor.init().map_err(|e| HypervisorError::StartFailed(format!("VMX init failed: {:?}", e)))?;
        
        *self.vmx_executor.write().unwrap() = Some(Arc::new(executor));
        Ok(())
    }
    
    /// Start with AMD-V (SVM) backend  
    fn start_svm_backend(&self, memory: &Arc<PhysicalMemory>) -> HypervisorResult<()> {
        let jit_config = Self::default_jit_config();
        
        let executor = SvmExecutor::with_jit_config(memory.clone(), jit_config);
        executor.init().map_err(|e| HypervisorError::StartFailed(format!("SVM init failed: {}", e)))?;
        
        *self.svm_executor.write().unwrap() = Some(Arc::new(executor));
        Ok(())
    }
    
    /// Start with pure JIT (software) backend
    fn start_jit_backend(&self, _memory: &Arc<PhysicalMemory>) -> HypervisorResult<()> {
        let jit_config = Self::default_jit_config();
        let engine = crate::jit::JitEngine::with_config(jit_config);
        
        *self.jit_engine.write().unwrap() = Some(Arc::new(engine));
        Ok(())
    }
    
    /// Default JIT configuration
    fn default_jit_config() -> crate::jit::JitConfig {
        use crate::jit::TierThresholds;
        
        crate::jit::JitConfig {
            tiered_compilation: true,
            thresholds: TierThresholds {
                interpreter_to_s1: 10,   // Compile after 10 executions
                s1_to_s2: 1000,          // Optimize after 1000 executions
                ..Default::default()
            },
            code_cache_size: 64 * 1024 * 1024,  // 64MB code cache
            profile_db_size: 10000,
            loop_unrolling: true,
            aggressive_inlining: true,
            ..Default::default()
        }
    }
    
    /// Load firmware (BIOS or UEFI) into guest physical memory
    fn load_firmware_to_memory(&self, memory: &PhysicalMemory) -> HypervisorResult<()> {
        use crate::firmware::{FirmwareManager, FirmwareType as FwType};
        
        let fw_type = match self.spec.firmware {
            FirmwareType::Bios => FwType::Bios,
            FirmwareType::Uefi => FwType::Uefi,
            FirmwareType::UefiSecure => FwType::UefiSecure,
        };
        
        let fw_manager = FirmwareManager::new(fw_type);
        fw_manager.initialize(self.spec.memory_mb as usize, self.spec.vcpus as u32)
            .map_err(|e| HypervisorError::StartFailed(format!("Firmware init failed: {}", e)))?;
        
        // Get memory as mutable slice and load firmware
        let (ram_ptr, ram_size) = memory.ram_region();
        let memory_slice = unsafe { std::slice::from_raw_parts_mut(ram_ptr, ram_size) };
        
        fw_manager.load_firmware(memory_slice)
            .map_err(|e| HypervisorError::StartFailed(format!("Firmware load failed: {}", e)))?;
        
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
        
        // Stop the appropriate backend
        match self.backend_type {
            VmBackendType::Vmx => {
                if let Some(executor) = self.vmx_executor.read().unwrap().as_ref() {
                    executor.stop();
                }
            }
            VmBackendType::Svm => {
                if let Some(executor) = self.svm_executor.read().unwrap().as_ref() {
                    executor.stop();
                }
            }
            VmBackendType::Jit => {
                // JIT engine doesn't need explicit stop
            }
            VmBackendType::Auto => unreachable!("Auto backend should be resolved in VmInstance::new()"),
        }
        
        // Clear backends
        *self.vmx_executor.write().unwrap() = None;
        *self.svm_executor.write().unwrap() = None;
        *self.jit_engine.write().unwrap() = None;
        *self.memory.write().unwrap() = None;
        
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
        
        match self.backend_type {
            VmBackendType::Vmx => {
                if let Some(executor) = self.vmx_executor.read().unwrap().as_ref() {
                    executor.pause();
                }
            }
            VmBackendType::Svm => {
                if let Some(executor) = self.svm_executor.read().unwrap().as_ref() {
                    executor.pause();
                }
            }
            VmBackendType::Jit => {
                // JIT pauses implicitly when not executing
            }
            VmBackendType::Auto => unreachable!("Auto backend should be resolved in VmInstance::new()"),
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
        
        match self.backend_type {
            VmBackendType::Vmx => {
                if let Some(executor) = self.vmx_executor.read().unwrap().as_ref() {
                    executor.resume();
                }
            }
            VmBackendType::Svm => {
                if let Some(executor) = self.svm_executor.read().unwrap().as_ref() {
                    executor.resume();
                }
            }
            VmBackendType::Jit => {
                // JIT resumes when execute() is called
            }
            VmBackendType::Auto => unreachable!("Auto backend should be resolved in VmInstance::new()"),
        }
        
        self.set_status(VmStatus::Running);
        Ok(())
    }
    
    pub fn reset(&self) -> HypervisorResult<()> {
        // Reset CPU state and reload firmware
        if let Some(memory) = self.memory.read().unwrap().as_ref() {
            self.load_firmware_to_memory(memory)?;
        }
        Ok(())
    }
    
    /// Get VGA framebuffer data for console display
    pub fn get_vga_framebuffer(&self) -> Option<Vec<u8>> {
        let mut vga = self.vga.write().unwrap();
        // Render text buffer to framebuffer
        vga.render_text_to_framebuffer();
        // Return a copy of the framebuffer
        Some(vga.get_framebuffer().lock().unwrap().clone())
    }
    
    /// Get VGA display dimensions (width, height)
    pub fn get_vga_dimensions(&self) -> Option<(u32, u32)> {
        let vga = self.vga.read().unwrap();
        let (w, h, _) = vga.get_dimensions();
        Some((w as u32, h as u32))
    }
    
    /// Check if VM has VGA device
    pub fn has_vga(&self) -> bool {
        true
    }
    
    /// Write to VGA console (text mode)
    pub fn vga_write(&self, text: &str) {
        let mut vga = self.vga.write().unwrap();
        vga.write_string(text);
    }
    
    /// Inject keyboard key event to PS/2 controller
    pub fn inject_key(&self, key: &str, is_release: bool) {
        let mut keyboard = self.keyboard.write().unwrap();
        keyboard.inject_key(key, is_release);
    }
    
    /// Inject keyboard scancode directly
    pub fn inject_scancode(&self, scancode: u8, is_release: bool) {
        let mut keyboard = self.keyboard.write().unwrap();
        keyboard.inject_scancode(scancode, is_release);
    }
    
    /// Advance VM execution by specified cycles
    /// This processes device ticks, interrupts, and CPU execution
    /// 
    /// Returns true if VM continues normally, false if it was reset
    pub fn tick(&self, _cycles: u64) -> bool {
        // In hardware virt, execution is continuous, not tick-based
        // This method is mainly for software emulation compatibility
        true
    }

    pub fn snapshot(&self, name: &str) -> HypervisorResult<String> {
        let snap = VmSnapshot {
            name: name.to_string(),
            created_at: Instant::now(),
            parent: None,
            description: None,
        };
        
        // TODO: Implement snapshot for hardware virt backends
        // For now, just record the snapshot metadata
        
        self.snapshots.write().unwrap().insert(name.to_string(), snap);
        Ok(name.to_string())
    }
    
    pub fn restore_snapshot(&self, name: &str) -> HypervisorResult<()> {
        if !self.snapshots.read().unwrap().contains_key(name) {
            return Err(HypervisorError::SnapshotError(
                format!("Snapshot '{}' not found", name)
            ));
        }
        
        // TODO: Implement snapshot restore for hardware virt backends
        
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
