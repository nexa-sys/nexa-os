//! VM Execution Engine
//!
//! This module bridges the WebGUI/API layer with the native NVM hypervisor.
//! It translates API requests into hypervisor operations and manages VM runtime state.
//!
//! ## Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────────┐
//! │                     WebGUI / REST API                             │
//! └───────────────────────────────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌───────────────────────────────────────────────────────────────────┐
//! │                       VmExecutor                                  │
//! │  • API config → VmSpec translation                                │
//! │  • Runtime state management                                       │
//! │  • Console/VNC session management                                 │
//! └───────────────────────────────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌───────────────────────────────────────────────────────────────────┐
//! │                    NVM Hypervisor Core                            │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐               │
//! │  │  VMX/SVM    │  │  Memory     │  │  Devices    │               │
//! │  │  VT-x/AMD-V │  │  EPT/NPT    │  │  Emulation  │               │
//! │  └─────────────┘  └─────────────┘  └─────────────┘               │
//! └───────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use parking_lot::Mutex;
use lazy_static::lazy_static;

use crate::hypervisor::{Hypervisor, VmId, VmStatus};

// ============================================================================
// Global executor instance
// ============================================================================

lazy_static! {
    static ref VM_EXECUTOR: Mutex<VmExecutor> = Mutex::new(VmExecutor::new());
}

/// Get global VM executor instance
pub fn vm_executor() -> parking_lot::MutexGuard<'static, VmExecutor> {
    VM_EXECUTOR.lock()
}

// ============================================================================
// Error types
// ============================================================================

/// VM execution errors
#[derive(Debug, Clone)]
pub enum VmExecError {
    NotFound(String),
    AlreadyRunning(String),
    NotRunning(String),
    InvalidConfig(String),
    HypervisorError(String),
    ResourceError(String),
    ConsoleError(String),
    DiskCreationFailed(String),
    InternalError(String),
}

impl std::fmt::Display for VmExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "VM not found: {}", id),
            Self::AlreadyRunning(id) => write!(f, "VM already running: {}", id),
            Self::NotRunning(id) => write!(f, "VM not running: {}", id),
            Self::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
            Self::HypervisorError(msg) => write!(f, "Hypervisor error: {}", msg),
            Self::ResourceError(msg) => write!(f, "Resource error: {}", msg),
            Self::ConsoleError(msg) => write!(f, "Console error: {}", msg),
            Self::DiskCreationFailed(msg) => write!(f, "Disk creation failed: {}", msg),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for VmExecError {}

pub type VmExecResult<T> = Result<T, VmExecError>;

// ============================================================================
// Configuration types (compatible with handlers)
// ============================================================================

/// Firmware type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareType {
    Bios,
    Uefi,
    UefiSecure,
}

impl Default for FirmwareType {
    fn default() -> Self {
        Self::Uefi
    }
}

/// Network type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkType {
    User,
    Bridge(String),
    Tap(String),
    MacVtap(String),
}

impl Default for NetworkType {
    fn default() -> Self {
        Self::User
    }
}

/// VM execution configuration from API
#[derive(Debug, Clone)]
pub struct VmExecConfig {
    pub vm_id: String,
    pub name: String,
    pub vcpus: u32,
    pub cpu_sockets: u32,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub cpu_model: String,
    pub memory_mb: u64,
    pub memory_balloon: bool,
    pub disks: Vec<DiskExecConfig>,
    pub networks: Vec<NetworkExecConfig>,
    pub cdrom_iso: Option<PathBuf>,
    pub firmware: FirmwareType,
    pub secure_boot: bool,
    pub tpm_enabled: bool,
    pub tpm_version: String,
    pub machine_type: String,
    pub nested_virt: bool,
    pub vnc_display: Option<u16>,
    pub qmp_socket: Option<PathBuf>,
    pub enable_kvm: bool,
    pub extra_args: Vec<String>,
}

/// Disk configuration for execution
#[derive(Debug, Clone)]
pub struct DiskExecConfig {
    pub path: PathBuf,
    pub format: String,
    pub bus: String,
    pub cache: String,
    pub io: String,
    pub bootable: bool,
    pub discard: bool,
    pub readonly: bool,
    pub serial: Option<String>,
}

/// Network configuration for execution
#[derive(Debug, Clone)]
pub struct NetworkExecConfig {
    pub id: String,
    pub mac: String,
    pub net_type: NetworkType,
    pub bridge: Option<String>,
    pub model: String,
    pub multiqueue: bool,
    pub queues: u32,
    pub vlan_id: Option<u16>,
}

// ============================================================================
// Running VM handle
// ============================================================================

/// Running VM handle with runtime information
#[derive(Debug)]
pub struct RunningVm {
    pub vm_id: String,
    pub hv_id: VmId,
    pub started_at: std::time::Instant,
    pub vnc_port: u16,
    pub paused: AtomicBool,
}

// ============================================================================
// VM Executor
// ============================================================================

/// VM Executor - bridges API to hypervisor
pub struct VmExecutor {
    /// NVM hypervisor instance
    hypervisor: Arc<Hypervisor>,
    /// Running VMs mapping: api_id -> RunningVm
    running_vms: HashMap<String, Arc<RunningVm>>,
    /// VM data directory
    data_dir: PathBuf,
    /// Next VNC port
    next_vnc_port: u16,
    /// KVM available
    kvm_available: bool,
}

impl VmExecutor {
    /// Create a new VM executor
    pub fn new() -> Self {
        let hypervisor = Hypervisor::new();
        
        // Check KVM availability
        let kvm_available = std::path::Path::new("/dev/kvm").exists();
        
        Self {
            hypervisor,
            running_vms: HashMap::new(),
            data_dir: PathBuf::from("/var/lib/nvm"),
            next_vnc_port: 5900,
            kvm_available,
        }
    }
    
    /// Create with custom data directory
    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        let mut executor = Self::new();
        executor.data_dir = data_dir;
        executor
    }
    
    /// Check if KVM is available
    pub fn is_kvm_available(&self) -> bool {
        self.kvm_available
    }
    
    /// Get the hypervisor instance
    pub fn hypervisor(&self) -> Arc<Hypervisor> {
        self.hypervisor.clone()
    }
    
    /// Start a VM using NVM's native hypervisor
    pub fn start_vm(&mut self, config: VmExecConfig) -> VmExecResult<Arc<RunningVm>> {
        // Check if already running
        if self.is_running(&config.vm_id) {
            return Err(VmExecError::AlreadyRunning(config.vm_id.clone()));
        }
        
        // Build VM spec from config
        let spec = self.build_vm_spec(&config)?;
        
        // Create VM in hypervisor
        let hv_id = self.hypervisor.create_vm(spec)
            .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        
        // Start VM
        self.hypervisor.start_vm(hv_id)
            .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        
        // Allocate VNC port
        let vnc_port = config.vnc_display.unwrap_or_else(|| {
            let port = self.next_vnc_port;
            self.next_vnc_port += 1;
            port
        });
        
        // Create running VM handle
        let running_vm = Arc::new(RunningVm {
            vm_id: config.vm_id.clone(),
            hv_id,
            started_at: std::time::Instant::now(),
            vnc_port,
            paused: AtomicBool::new(false),
        });
        
        // Store in running VMs
        self.running_vms.insert(config.vm_id.clone(), running_vm.clone());
        
        log::info!("Started VM {} (hypervisor ID: {}, VNC port: {})", 
            config.vm_id, hv_id, vnc_port);
        
        Ok(running_vm)
    }
    
    /// Stop a VM
    pub fn stop_vm(&mut self, vm_id: &str, force: bool) -> VmExecResult<()> {
        let running_vm = self.get_running_vm(vm_id)?;
        
        if force {
            self.hypervisor.destroy_vm(running_vm.hv_id)
                .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        } else {
            self.hypervisor.stop_vm(running_vm.hv_id)
                .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        }
        
        self.running_vms.remove(vm_id);
        
        log::info!("Stopped VM {} (force: {})", vm_id, force);
        
        Ok(())
    }
    
    /// Pause a VM
    pub fn pause_vm(&mut self, vm_id: &str) -> VmExecResult<()> {
        let running_vm = self.get_running_vm(vm_id)?;
        
        if running_vm.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        self.hypervisor.pause_vm(running_vm.hv_id)
            .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        
        running_vm.paused.store(true, Ordering::SeqCst);
        
        log::info!("Paused VM {}", vm_id);
        
        Ok(())
    }
    
    /// Resume a VM
    pub fn resume_vm(&mut self, vm_id: &str) -> VmExecResult<()> {
        let running_vm = self.get_running_vm(vm_id)?;
        
        if !running_vm.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        self.hypervisor.resume_vm(running_vm.hv_id)
            .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        
        running_vm.paused.store(false, Ordering::SeqCst);
        
        log::info!("Resumed VM {}", vm_id);
        
        Ok(())
    }
    
    /// Reset a VM
    pub fn reset_vm(&mut self, vm_id: &str) -> VmExecResult<()> {
        let running_vm = self.get_running_vm(vm_id)?;
        
        self.hypervisor.reset_vm(running_vm.hv_id)
            .map_err(|e| VmExecError::HypervisorError(e.to_string()))?;
        
        running_vm.paused.store(false, Ordering::SeqCst);
        
        log::info!("Reset VM {}", vm_id);
        
        Ok(())
    }
    
    /// Check if a VM is running
    pub fn is_running(&self, vm_id: &str) -> bool {
        self.running_vms.contains_key(vm_id)
    }
    
    /// Get running VM handle
    pub fn get_running_vm(&self, vm_id: &str) -> VmExecResult<Arc<RunningVm>> {
        self.running_vms
            .get(vm_id)
            .cloned()
            .ok_or_else(|| VmExecError::NotRunning(vm_id.to_string()))
    }
    
    /// Get VNC port for a running VM
    pub fn get_vnc_port(&self, vm_id: &str) -> VmExecResult<u16> {
        let running_vm = self.get_running_vm(vm_id)?;
        Ok(running_vm.vnc_port)
    }
    
    /// Get VM status
    pub fn get_vm_status(&self, vm_id: &str) -> VmExecResult<VmStatus> {
        if let Some(running_vm) = self.running_vms.get(vm_id) {
            if running_vm.paused.load(Ordering::SeqCst) {
                Ok(VmStatus::Paused)
            } else {
                Ok(VmStatus::Running)
            }
        } else {
            Ok(VmStatus::Stopped)
        }
    }
    
    /// List all running VMs
    pub fn list_running(&self) -> Vec<String> {
        self.running_vms.keys().cloned().collect()
    }
    
    /// Get VM data directory
    pub fn vm_data_dir(&self, vm_id: &str) -> PathBuf {
        self.data_dir.join("vms").join(vm_id)
    }
    
    /// Create disk image using hypervisor storage backend
    pub fn create_disk_image(&self, path: &std::path::Path, size_gb: u64, format: &str) -> VmExecResult<()> {
        // Create parent directory
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| VmExecError::DiskCreationFailed(e.to_string()))?;
        }
        
        // Create disk image file (placeholder implementation)
        // In production, this would use qemu-img or native disk creation
        let file = std::fs::File::create(path)
            .map_err(|e| VmExecError::DiskCreationFailed(e.to_string()))?;
        
        // Allocate space (sparse file)
        let size_bytes = size_gb * 1024 * 1024 * 1024;
        file.set_len(size_bytes)
            .map_err(|e| VmExecError::DiskCreationFailed(e.to_string()))?;
        
        log::info!("Created disk image: {} ({} GB, {})", path.display(), size_gb, format);
        
        Ok(())
    }
    
    // ========================================================================
    // Internal helpers
    // ========================================================================
    
    /// Build VmSpec from execution config
    fn build_vm_spec(&self, config: &VmExecConfig) -> VmExecResult<crate::hypervisor::VmSpec> {
        use crate::hypervisor::{
            VmSpecBuilder, DiskSpec, NetworkSpec, FirmwareType as HvFirmwareType, 
            CpuModel, MachineType, DiskFormat, DiskInterface, CacheMode, 
            NetworkType as HvNetworkType, NicModel, VmSecuritySpec,
        };
        
        let mut builder = VmSpecBuilder::new()
            .name(&config.name)
            .vcpus(config.vcpus)
            .memory_mb(config.memory_mb)
            .nested_virt(config.nested_virt);
        
        // Firmware
        let firmware = match config.firmware {
            FirmwareType::Uefi | FirmwareType::UefiSecure => HvFirmwareType::Uefi,
            FirmwareType::Bios => HvFirmwareType::Bios,
        };
        builder = builder.firmware(firmware);
        
        // CPU model - Host is the primary option
        let cpu_model = match config.cpu_model.to_lowercase().as_str() {
            "host-passthrough" | "host" | "host-model" => CpuModel::Host,
            name => CpuModel::Named(name.to_string()),
        };
        builder = builder.cpu_model(cpu_model);
        
        // Machine type
        let machine_type = match config.machine_type.to_lowercase().as_str() {
            "q35" => MachineType::Q35,
            "i440fx" => MachineType::I440fx,
            _ => MachineType::Q35,
        };
        builder = builder.machine_type(machine_type);
        
        // Disks
        for disk in &config.disks {
            let format = match disk.format.to_lowercase().as_str() {
                "qcow2" => DiskFormat::Qcow2,
                "raw" => DiskFormat::Raw,
                "vmdk" => DiskFormat::Vmdk,
                "vdi" => DiskFormat::Vdi,
                _ => DiskFormat::Qcow2,
            };
            
            let interface = match disk.bus.to_lowercase().as_str() {
                "virtio" => DiskInterface::Virtio,
                "scsi" => DiskInterface::Scsi,
                "ide" => DiskInterface::Ide,
                "sata" => DiskInterface::Sata,
                "nvme" => DiskInterface::Nvme,
                _ => DiskInterface::Virtio,
            };
            
            let cache = match disk.cache.to_lowercase().as_str() {
                "none" => CacheMode::None,
                "writeback" => CacheMode::Writeback,
                "writethrough" => CacheMode::Writethrough,
                "directsync" => CacheMode::DirectSync,
                "unsafe" => CacheMode::Unsafe,
                _ => CacheMode::Writeback,
            };
            
            let mut disk_spec = DiskSpec::new(
                disk.path.to_str().unwrap_or(""),
                0  // Size is read from existing file
            );
            disk_spec = disk_spec.format(format).interface(interface);
            disk_spec.cache_mode = cache;
            
            if disk.bootable {
                disk_spec = disk_spec.bootable();
            }
            if disk.readonly {
                disk_spec = disk_spec.readonly();
            }
            
            builder = builder.disk(disk_spec);
        }
        
        // CD-ROM
        if let Some(iso_path) = &config.cdrom_iso {
            let cdrom_spec = DiskSpec::new(
                iso_path.to_str().unwrap_or(""),
                0
            ).readonly().interface(DiskInterface::Sata);
            builder = builder.disk(cdrom_spec);
        }
        
        // Networks
        for net in &config.networks {
            let model = match net.model.to_lowercase().as_str() {
                "virtio" | "virtio-net-pci" => NicModel::Virtio,
                "e1000" => NicModel::E1000,
                "e1000e" => NicModel::E1000e,
                _ => NicModel::Virtio,
            };
            
            // Build NetworkSpec using the available constructors
            let mut net_spec = match &net.net_type {
                NetworkType::Bridge(br) => NetworkSpec::bridged(br).with_model(model),
                NetworkType::User => NetworkSpec::nat().with_model(model),
                NetworkType::Tap(tap) => NetworkSpec::bridged(tap).with_model(model),
                NetworkType::MacVtap(_) => NetworkSpec::nat().with_model(model),  // Fallback to NAT
            };
            
            if !net.mac.is_empty() {
                net_spec = net_spec.with_mac(&net.mac);
            }
            if let Some(vlan) = net.vlan_id {
                net_spec = net_spec.with_vlan(vlan);
            }
            
            builder = builder.network(net_spec);
        }
        
        // Security (TPM, Secure Boot)
        if config.tpm_enabled || config.secure_boot {
            let security = VmSecuritySpec {
                secure_boot: config.secure_boot,
                tpm: config.tpm_enabled,
                ..Default::default()
            };
            builder = builder.security(security);
        }
        
        Ok(builder.build())
    }
}

impl Default for VmExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// VM runtime metrics
#[derive(Debug, Clone, Default)]
pub struct VmMetrics {
    pub cpu_percent: f64,
    pub memory_used_mb: u64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub uptime_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_executor_creation() {
        let executor = VmExecutor::new();
        assert!(executor.list_running().is_empty());
    }
    
    #[test]
    fn test_vm_data_dir() {
        let executor = VmExecutor::with_data_dir(PathBuf::from("/tmp/nvm"));
        let dir = executor.vm_data_dir("test-vm");
        assert_eq!(dir, PathBuf::from("/tmp/nvm/vms/test-vm"));
    }
}
