//! VM management handlers
//!
//! Enterprise-grade VM lifecycle management supporting:
//! - Multi-disk configurations (QCOW2, RAW, VMDK)
//! - Multiple network interfaces with VLAN and QoS
//! - UEFI/BIOS firmware selection with Secure Boot
//! - TPM 2.0 and vTPM support
//! - CPU pinning and NUMA topology
//! - Hot-pluggable devices

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::{vm_state, VmStatus as StateVmStatus};
use std::sync::Arc;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json},
    http::StatusCode,
    response::IntoResponse,
};

/// VM list item - matches frontend Vm interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub description: Option<String>,
    pub host_node: Option<String>,
    pub template: Option<String>,
    pub config: VmListConfig,
    pub stats: Option<VmListStats>,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
}

/// VM config for list items - matches frontend VmConfig interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListConfig {
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub network: String,
    pub boot_order: Vec<String>,
}

/// VM stats for list items - matches frontend VmStats interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListStats {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub network_rx_bps: u64,
    pub network_tx_bps: u64,
}

/// VM details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDetails {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub config: VmConfig,
    pub hardware: VmHardware,
    pub metrics: Option<VmMetrics>,
    pub snapshots: Vec<VmSnapshot>,
    pub node: String,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub os_type: String,
    pub boot_order: Vec<String>,
    pub bios_type: String,
    pub secure_boot: bool,
    pub tpm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmHardware {
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disks: Vec<VmDisk>,
    pub networks: Vec<VmNetwork>,
    pub cdrom: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDisk {
    pub id: String,
    pub name: String,
    pub size_gb: u64,
    pub format: String,
    pub storage_pool: String,
    pub bus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmNetwork {
    pub id: String,
    pub mac: String,
    pub network: String,
    pub model: String,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    pub cpu_percent: f64,
    pub memory_used_mb: u64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSnapshot {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub size_mb: u64,
    pub description: Option<String>,
}

/// Create VM request - Enterprise-grade configuration
/// Supports both simple frontend config format and full enterprise API format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    
    // Frontend sends config object with these fields (simple mode)
    #[serde(default)]
    pub config: Option<CreateVmConfig>,
    
    // Enterprise configuration (advanced mode)
    #[serde(default)]
    pub hardware: Option<EnterpriseHardwareConfig>,
    #[serde(default)]
    pub boot: Option<BootConfig>,
    #[serde(default)]
    pub security: Option<SecurityConfig>,
    
    // Direct fields (for backward compatibility)
    #[serde(default)]
    pub os_type: Option<String>,
    #[serde(default)]
    pub vcpus: Option<u32>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
    #[serde(default)]
    pub disks: Option<Vec<CreateDiskSpec>>,
    #[serde(default)]
    pub networks: Option<Vec<CreateNetworkSpec>>,
    #[serde(default)]
    pub iso: Option<String>,
    #[serde(default)]
    pub start_after_create: Option<bool>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub host_node: Option<String>,
    /// Execution backend: "jit" (software, 5-15% faster), "vmx" (Intel), "svm" (AMD), "auto"
    #[serde(default)]
    pub backend: Option<String>,
}

/// Enterprise hardware configuration - ESXi/vCenter style
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterpriseHardwareConfig {
    /// CPU configuration
    pub cpu: CpuConfig,
    /// Memory configuration
    pub memory: MemoryConfig,
    /// Disk configurations (multiple disks supported)
    pub disks: Vec<EnterpriseDiskSpec>,
    /// Network interface configurations
    pub networks: Vec<EnterpriseNetworkSpec>,
    /// CD/DVD drive configuration
    #[serde(default)]
    pub cdrom: Option<CdromConfig>,
    /// USB controller and devices
    #[serde(default)]
    pub usb: Option<UsbConfig>,
    /// GPU/vGPU configuration
    #[serde(default)]
    pub gpu: Option<GpuConfig>,
    /// Serial/parallel ports
    #[serde(default)]
    pub serial_ports: Vec<SerialPortConfig>,
}

/// CPU configuration (ESXi-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuConfig {
    /// Number of sockets
    #[serde(default = "default_sockets")]
    pub sockets: u32,
    /// Cores per socket
    #[serde(default = "default_cores_per_socket")]
    pub cores_per_socket: u32,
    /// Threads per core (hyperthreading)
    #[serde(default = "default_threads_per_core")]
    pub threads_per_core: u32,
    /// CPU model (host-passthrough, host-model, custom)
    #[serde(default = "default_cpu_model")]
    pub model: String,
    /// CPU features to enable
    #[serde(default)]
    pub features_add: Vec<String>,
    /// CPU features to disable
    #[serde(default)]
    pub features_remove: Vec<String>,
    /// Allow hot-add CPUs
    #[serde(default)]
    pub hot_add: bool,
    /// CPU resource limit (MHz) - like ESXi limit
    #[serde(default)]
    pub limit_mhz: Option<u64>,
    /// CPU reservation (MHz) - guaranteed resources
    #[serde(default)]
    pub reservation_mhz: Option<u64>,
    /// CPU shares (low/normal/high or custom value)
    #[serde(default = "default_cpu_shares")]
    pub shares: String,
    /// Enable nested virtualization
    #[serde(default)]
    pub nested_virt: bool,
    /// NUMA configuration
    #[serde(default)]
    pub numa: Option<NumaConfig>,
    /// CPU pinning/affinity
    #[serde(default)]
    pub affinity: Option<Vec<u32>>,
}

fn default_sockets() -> u32 { 1 }
fn default_cores_per_socket() -> u32 { 2 }
fn default_threads_per_core() -> u32 { 1 }
fn default_cpu_model() -> String { "host-passthrough".to_string() }
fn default_cpu_shares() -> String { "normal".to_string() }

/// NUMA configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumaConfig {
    /// NUMA nodes configuration
    pub nodes: Vec<NumaNode>,
}

/// NUMA node specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumaNode {
    /// Node ID
    pub id: u32,
    /// Memory assigned to this node (MB)
    pub memory_mb: u64,
    /// vCPUs assigned to this node
    pub vcpus: Vec<u32>,
    /// Host NUMA node to bind to (optional)
    #[serde(default)]
    pub host_node: Option<u32>,
}

/// Memory configuration (ESXi-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Base memory size in MB
    pub size_mb: u64,
    /// Maximum memory for hot-add (MB)
    #[serde(default)]
    pub max_size_mb: Option<u64>,
    /// Enable memory hot-add
    #[serde(default)]
    pub hot_add: bool,
    /// Memory reservation (MB) - guaranteed physical memory
    #[serde(default)]
    pub reservation_mb: Option<u64>,
    /// Memory limit (MB)
    #[serde(default)]
    pub limit_mb: Option<u64>,
    /// Memory shares (low/normal/high or custom)
    #[serde(default = "default_memory_shares")]
    pub shares: String,
    /// Enable memory ballooning
    #[serde(default = "default_true")]
    pub ballooning: bool,
    /// Enable Kernel Same-page Merging
    #[serde(default)]
    pub ksm: bool,
    /// Enable huge pages
    #[serde(default)]
    pub huge_pages: bool,
    /// Huge page size (2M or 1G)
    #[serde(default)]
    pub huge_page_size: Option<String>,
}

fn default_memory_shares() -> String { "normal".to_string() }
fn default_true() -> bool { true }

/// Enterprise disk specification (QEMU/ESXi/Proxmox style)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterpriseDiskSpec {
    /// Disk name/label
    #[serde(default)]
    pub name: Option<String>,
    /// Disk size in GB
    pub size_gb: u64,
    /// Storage pool to use
    #[serde(default = "default_storage_pool")]
    pub storage_pool: String,
    /// Disk format (qcow2, raw, vmdk, vdi)
    #[serde(default = "default_disk_format")]
    pub format: String,
    /// Bus type (virtio, scsi, ide, sata, nvme)
    #[serde(default = "default_disk_bus")]
    pub bus: String,
    /// Cache mode (none, writeback, writethrough, unsafe, directsync)
    #[serde(default = "default_cache_mode")]
    pub cache: String,
    /// IO mode (native, threads, io_uring)
    #[serde(default = "default_io_mode")]
    pub io_mode: String,
    /// Discard/TRIM support (ignore, unmap)
    #[serde(default = "default_discard")]
    pub discard: String,
    /// Enable SSD emulation (for TRIM)
    #[serde(default)]
    pub ssd_emulation: bool,
    /// Thin provisioning
    #[serde(default = "default_true")]
    pub thin_provisioning: bool,
    /// Is this the boot disk?
    #[serde(default)]
    pub bootable: bool,
    /// IOPS limit (read)
    #[serde(default)]
    pub iops_rd_limit: Option<u64>,
    /// IOPS limit (write)
    #[serde(default)]
    pub iops_wr_limit: Option<u64>,
    /// Bandwidth limit (MB/s read)
    #[serde(default)]
    pub bps_rd_limit: Option<u64>,
    /// Bandwidth limit (MB/s write)
    #[serde(default)]
    pub bps_wr_limit: Option<u64>,
    /// Existing disk path (for attaching existing disks)
    #[serde(default)]
    pub existing_path: Option<String>,
    /// SCSI controller type (virtio-scsi-pci, lsi, megasas)
    #[serde(default)]
    pub scsi_controller: Option<String>,
}

fn default_disk_format() -> String { "qcow2".to_string() }
fn default_disk_bus() -> String { "virtio".to_string() }
fn default_cache_mode() -> String { "writeback".to_string() }
fn default_io_mode() -> String { "native".to_string() }
fn default_discard() -> String { "unmap".to_string() }

/// Enterprise network specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterpriseNetworkSpec {
    /// Network/port group name
    pub network: String,
    /// MAC address (auto-generated if not specified)
    #[serde(default)]
    pub mac: Option<String>,
    /// NIC model (virtio, e1000, e1000e, vmxnet3, rtl8139)
    #[serde(default = "default_nic_model")]
    pub model: String,
    /// VLAN ID (trunk mode if multiple)
    #[serde(default)]
    pub vlan_id: Option<u16>,
    /// VLAN trunk (multiple VLANs)
    #[serde(default)]
    pub vlan_trunk: Option<Vec<u16>>,
    /// Enable promiscuous mode
    #[serde(default)]
    pub promiscuous: bool,
    /// QoS - Inbound bandwidth limit (Mbps)
    #[serde(default)]
    pub inbound_limit_mbps: Option<u64>,
    /// QoS - Outbound bandwidth limit (Mbps)
    #[serde(default)]
    pub outbound_limit_mbps: Option<u64>,
    /// Security group ID
    #[serde(default)]
    pub security_group: Option<String>,
    /// Enable multiqueue (virtio only)
    #[serde(default)]
    pub multiqueue: bool,
    /// Number of queues
    #[serde(default)]
    pub queues: Option<u32>,
    /// SR-IOV Virtual Function (for passthrough)
    #[serde(default)]
    pub sriov_vf: Option<String>,
}

fn default_nic_model() -> String { "virtio".to_string() }

/// CD/DVD drive configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdromConfig {
    /// ISO image path
    #[serde(default)]
    pub iso: Option<String>,
    /// Bus type (ide, sata, scsi)
    #[serde(default = "default_cdrom_bus")]
    pub bus: String,
    /// Media passthrough (host device)
    #[serde(default)]
    pub passthrough: Option<String>,
}

fn default_cdrom_bus() -> String { "sata".to_string() }

/// USB configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbConfig {
    /// USB controller type (usb2, usb3, xhci)
    #[serde(default = "default_usb_controller")]
    pub controller: String,
    /// USB device passthrough
    #[serde(default)]
    pub devices: Vec<UsbDevicePassthrough>,
}

fn default_usb_controller() -> String { "usb3".to_string() }

/// USB device passthrough
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbDevicePassthrough {
    /// Vendor ID
    pub vendor_id: String,
    /// Product ID
    pub product_id: String,
}

/// GPU/vGPU configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    /// GPU passthrough (PCI address)
    #[serde(default)]
    pub passthrough: Option<String>,
    /// vGPU profile (for NVIDIA vGPU, Intel GVT-g)
    #[serde(default)]
    pub vgpu_profile: Option<String>,
    /// Display type (none, vnc, spice, gtk)
    #[serde(default = "default_display")]
    pub display: String,
    /// VGA type (std, cirrus, qxl, virtio)
    #[serde(default = "default_vga")]
    pub vga: String,
    /// Video memory (MB)
    #[serde(default)]
    pub video_memory_mb: Option<u32>,
    /// 3D acceleration
    #[serde(default)]
    pub acceleration_3d: bool,
}

fn default_display() -> String { "vnc".to_string() }
fn default_vga() -> String { "qxl".to_string() }

/// Serial port configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialPortConfig {
    /// Port number (0-3)
    pub port: u8,
    /// Backend type (pty, socket, file, chardev)
    pub backend: String,
    /// Path for file/socket backend
    #[serde(default)]
    pub path: Option<String>,
}

/// Boot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootConfig {
    /// Firmware type (bios, uefi)
    #[serde(default = "default_firmware")]
    pub firmware: String,
    /// UEFI variant (default, secureboot)
    #[serde(default)]
    pub uefi_type: Option<String>,
    /// Boot order (disk, cdrom, network, floppy)
    #[serde(default)]
    pub order: Vec<String>,
    /// Secure Boot enabled
    #[serde(default)]
    pub secure_boot: bool,
    /// Boot menu timeout (seconds, 0 = disabled)
    #[serde(default)]
    pub menu_timeout: u32,
    /// OVMF code path (custom UEFI firmware)
    #[serde(default)]
    pub ovmf_code: Option<String>,
    /// Machine type (q35, i440fx, virt)
    #[serde(default = "default_machine_type")]
    pub machine_type: String,
    /// SMBIOS/DMI settings
    #[serde(default)]
    pub smbios: Option<SmbiosConfig>,
}

fn default_firmware() -> String { "uefi".to_string() }
fn default_machine_type() -> String { "q35".to_string() }

/// SMBIOS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmbiosConfig {
    /// System manufacturer
    #[serde(default)]
    pub manufacturer: Option<String>,
    /// Product name
    #[serde(default)]
    pub product: Option<String>,
    /// Serial number
    #[serde(default)]
    pub serial: Option<String>,
    /// UUID (auto-generated if not set)
    #[serde(default)]
    pub uuid: Option<String>,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable TPM 2.0 emulation
    #[serde(default)]
    pub tpm: bool,
    /// TPM version (1.2 or 2.0)
    #[serde(default = "default_tpm_version")]
    pub tpm_version: String,
    /// Enable AMD SEV (Secure Encrypted Virtualization)
    #[serde(default)]
    pub sev: bool,
    /// SEV policy
    #[serde(default)]
    pub sev_policy: Option<u32>,
    /// Enable Intel TDX
    #[serde(default)]
    pub tdx: bool,
    /// Encryption key ID
    #[serde(default)]
    pub encryption_key: Option<String>,
    /// VM isolation level (none, hypervisor, hardware)
    #[serde(default = "default_isolation")]
    pub isolation: String,
}

fn default_tpm_version() -> String { "2.0".to_string() }
fn default_isolation() -> String { "hypervisor".to_string() }

// Legacy/simple config structures kept for backward compatibility

/// Config object from frontend form (simple mode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmConfig {
    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: u32,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
    #[serde(default = "default_disk_gb")]
    pub disk_gb: u64,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default)]
    pub boot_order: Vec<String>,
    // Extended simple config options
    #[serde(default)]
    pub disks: Option<Vec<SimpleDiskConfig>>,
    #[serde(default)]
    pub networks: Option<Vec<SimpleNetworkConfig>>,
    #[serde(default = "default_firmware")]
    pub firmware: String,
    #[serde(default)]
    pub secure_boot: bool,
    #[serde(default)]
    pub tpm: bool,
    #[serde(default)]
    pub iso_path: Option<String>,
    /// Execution backend: "jit" (software, 5-15% faster), "vmx" (Intel), "svm" (AMD), "auto"
    #[serde(default = "default_backend")]
    pub backend: String,
}

fn default_backend() -> String {
    "jit".to_string()
}

/// Simple disk configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleDiskConfig {
    pub size_gb: u64,
    #[serde(default = "default_storage_pool")]
    pub storage_pool: String,
    #[serde(default = "default_disk_format")]
    pub format: String,
    #[serde(default = "default_disk_bus")]
    pub bus: String,
}

/// Simple network configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleNetworkConfig {
    pub network: String,
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default = "default_nic_model")]
    pub model: String,
    #[serde(default)]
    pub vlan_id: Option<u16>,
}

fn default_cpu_cores() -> u32 { 2 }
fn default_memory_mb() -> u64 { 2048 }
fn default_disk_gb() -> u64 { 20 }
fn default_network() -> String { "default".to_string() }
fn default_storage_pool() -> String { "local".to_string() }

/// Legacy disk spec for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDiskSpec {
    pub size_gb: u64,
    #[serde(default = "default_storage_pool")]
    pub storage_pool: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub bus: Option<String>,
    #[serde(default)]
    pub cache: Option<String>,
}

/// Legacy network spec for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNetworkSpec {
    pub network: String,
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Snapshot request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub name: String,
    pub description: Option<String>,
    pub include_memory: bool,
}

/// Clone request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    pub name: String,
    pub full_clone: bool,
    pub target_node: Option<String>,
}

/// Migrate request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateRequest {
    pub target_node: String,
    pub live: bool,
    pub with_storage: bool,
}

/// Console ticket response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleTicket {
    pub ticket: String,
    pub port: u16,
    pub console_type: String,
    pub url: String,
    pub expires_at: u64,
}

// Handlers

#[cfg(feature = "webgui")]
pub async fn list(
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // Get VMs from state manager
    let state_mgr = vm_state();
    let all_vms = state_mgr.list_vms();
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let vms: Vec<VmListItem> = all_vms.iter().map(|vm| {
        // Format timestamps as ISO8601 strings
        let created_at = chrono::DateTime::from_timestamp(vm.created_at as i64, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        
        let updated_at = vm.started_at
            .and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| created_at.clone());
        
        VmListItem {
            id: vm.id.clone(),
            name: vm.name.clone(),
            status: vm.status.to_string(),
            description: vm.description.clone(),
            host_node: vm.node.clone(),
            template: None,
            config: VmListConfig {
                cpu_cores: vm.vcpus,
                memory_mb: vm.memory_mb,
                disk_gb: vm.disk_gb,
                network: vm.network_interfaces.first()
                    .map(|nic| nic.network.clone())
                    .unwrap_or_else(|| "default".to_string()),
                boot_order: vec!["disk".to_string(), "cdrom".to_string()],
            },
            stats: if vm.status == StateVmStatus::Running {
                Some(VmListStats {
                    cpu_usage: 0.0,      // Would come from monitoring
                    memory_usage: 0.0,
                    disk_read_bps: 0,
                    disk_write_bps: 0,
                    network_rx_bps: 0,
                    network_tx_bps: 0,
                })
            } else {
                None
            },
            created_at,
            updated_at,
            tags: vm.tags.clone(),
        }
    }).collect();
    
    let total = vms.len() as u64;
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total,
        total_pages: ((total as f64) / (params.per_page as f64)).ceil() as u32,
    };
    
    Json(ApiResponse::success(vms).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.get_vm(&id) {
        Some(vm) => {
            let details = VmDetails {
                id: vm.id.clone(),
                name: vm.name.clone(),
                description: vm.description.clone(),
                status: vm.status.to_string(),
                config: VmConfig {
                    os_type: "linux".to_string(),
                    boot_order: vec!["disk".to_string(), "cdrom".to_string()],
                    bios_type: "uefi".to_string(),
                    secure_boot: false,
                    tpm: false,
                },
                hardware: VmHardware {
                    vcpus: vm.vcpus,
                    memory_mb: vm.memory_mb,
                    disks: vec![VmDisk {
                        id: "disk-001".to_string(),
                        name: "root".to_string(),
                        size_gb: vm.disk_gb,
                        format: "qcow2".to_string(),
                        storage_pool: "local".to_string(),
                        bus: "virtio".to_string(),
                    }],
                    networks: vm.network_interfaces.iter().map(|nic| VmNetwork {
                        id: nic.id.clone(),
                        mac: nic.mac.clone(),
                        network: nic.network.clone(),
                        model: nic.model.clone(),
                        ip: nic.ip.clone(),
                    }).collect(),
                    cdrom: None,
                },
                metrics: None, // Would come from monitoring
                snapshots: vec![],
                node: vm.node.clone().unwrap_or_else(|| "local".to_string()),
                created_at: vm.created_at,
                started_at: vm.started_at,
                tags: vm.tags.clone(),
            };
            Json(ApiResponse::success(details))
        }
        None => {
            Json(ApiResponse::<VmDetails>::error(404, &format!("VM '{}' not found", id)))
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateVmRequest>,
) -> impl IntoResponse {
    use crate::vmstate::{VmState, NetworkInterface};
    use crate::executor::{vm_executor, VmExecConfig, DiskExecConfig, NetworkExecConfig, FirmwareType, NetworkType};
    
    let state_mgr = vm_state();
    let mut executor = vm_executor();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    // Extract disk configuration from request
    // Enterprise feature: Support diskless VMs for PXE boot, live ISO, thin clients
    let disks_from_hardware: Vec<EnterpriseDiskSpec> = req.hardware.as_ref()
        .map(|hw| hw.disks.clone())
        .unwrap_or_default();
    let disks_from_legacy: Vec<CreateDiskSpec> = req.disks.clone().unwrap_or_default();
    let has_disks = !disks_from_hardware.is_empty() || !disks_from_legacy.is_empty();
    
    // Extract values from config object (frontend format) or direct fields (API format)
    let (vcpus, memory_mb, disk_gb, network) = if let Some(config) = &req.config {
        (config.cpu_cores, config.memory_mb, config.disk_gb, config.network.clone())
    } else {
        (
            req.vcpus.unwrap_or(2),
            req.memory_mb.unwrap_or(2048),
            disks_from_legacy.first().map(|d| d.size_gb)
                .or_else(|| disks_from_hardware.first().map(|d| d.size_gb))
                .unwrap_or(0),
            req.networks.as_ref().and_then(|n| n.first().map(|n| n.network.clone())).unwrap_or_else(|| "default".to_string()),
        )
    };
    
    let tags = req.tags.clone().unwrap_or_default();
    
    // Generate network interface
    let nic_id = format!("nic-{}", &Uuid::new_v4().to_string()[..8]);
    let mac = format!("52:54:00:{:02x}:{:02x}:{:02x}",
        rand::random::<u8>(), rand::random::<u8>(), rand::random::<u8>());
    
    let network_interfaces = vec![NetworkInterface {
        id: nic_id,
        mac,
        network: network.clone(),
        model: "virtio".to_string(),
        ip: None,
    }];
    
    let vm = VmState {
        id: String::new(), // Will be generated
        name: req.name.clone(),
        status: StateVmStatus::Stopped,
        vcpus,
        memory_mb,
        disk_gb,
        node: None,
        created_at: now,
        started_at: None,
        config_path: None,
        disk_paths: vec![],
        network_interfaces: network_interfaces.clone(),
        tags: tags.clone(),
        description: req.description.clone(),
    };
    
    match state_mgr.create_vm(vm) {
        Ok(vm_id) => {
            let data_dir = executor.vm_data_dir(&vm_id);
            let mut disk_configs: Vec<DiskExecConfig> = vec![];
            let mut created_disk_paths: Vec<std::path::PathBuf> = vec![];
            
            // Create disks only if configured (enterprise: diskless VMs supported)
            if has_disks {
                // Use hardware disks (enterprise format) if available, else fall back to legacy format
                if !disks_from_hardware.is_empty() {
                    for (idx, disk) in disks_from_hardware.iter().enumerate() {
                        let disk_path = data_dir.join(format!("{}-disk{}.{}", req.name, idx, &disk.format));
                        
                        // Create disk image
                        if let Err(e) = executor.create_disk_image(&disk_path, disk.size_gb, &disk.format) {
                            // Rollback: delete VM state and already created disks
                            for path in &created_disk_paths {
                                let _ = std::fs::remove_file(path);
                            }
                            let _ = state_mgr.delete_vm(&vm_id);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ApiResponse::<VmListItem>::error(500, &format!("Failed to create disk {}: {}", idx, e))),
                            );
                        }
                        created_disk_paths.push(disk_path.clone());
                        
                        disk_configs.push(DiskExecConfig {
                            path: disk_path,
                            format: disk.format.clone(),
                            bus: disk.bus.clone(),
                            cache: disk.cache.clone(),
                            io: "native".to_string(),
                            bootable: disk.bootable,
                            discard: disk.discard != "ignore",
                            readonly: false,
                            serial: None,
                        });
                    }
                } else {
                    // Legacy format (simple disks)
                    for (idx, disk) in disks_from_legacy.iter().enumerate() {
                        let format = disk.format.as_deref().unwrap_or("qcow2");
                        let disk_path = data_dir.join(format!("{}-disk{}.{}", req.name, idx, format));
                        
                        // Create disk image
                        if let Err(e) = executor.create_disk_image(&disk_path, disk.size_gb, format) {
                            // Rollback: delete VM state and already created disks
                            for path in &created_disk_paths {
                                let _ = std::fs::remove_file(path);
                            }
                            let _ = state_mgr.delete_vm(&vm_id);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ApiResponse::<VmListItem>::error(500, &format!("Failed to create disk {}: {}", idx, e))),
                            );
                        }
                        created_disk_paths.push(disk_path.clone());
                        
                        disk_configs.push(DiskExecConfig {
                            path: disk_path,
                            format: format.to_string(),
                            bus: disk.bus.clone().unwrap_or_else(|| "virtio".to_string()),
                            cache: disk.cache.clone().unwrap_or_else(|| "writeback".to_string()),
                            io: "native".to_string(),
                            bootable: idx == 0,
                            discard: true,
                            readonly: false,
                            serial: None,
                        });
                    }
                }
            } else {
                log::info!("Creating diskless VM '{}' (ID: {})", req.name, vm_id);
            }
            
            let networks: Vec<NetworkExecConfig> = network_interfaces.iter().map(|nic| {
                NetworkExecConfig {
                    id: nic.id.clone(),
                    mac: nic.mac.clone(),
                    net_type: NetworkType::User,
                    bridge: None,
                    model: "virtio-net-pci".to_string(),
                    multiqueue: false,
                    queues: 1,
                    vlan_id: None,
                }
            }).collect();
            
            // Extract boot configuration
            let boot_order = req.boot.as_ref()
                .map(|b| b.order.clone())
                .unwrap_or_else(|| {
                    if has_disks {
                        vec!["disk".to_string(), "cdrom".to_string(), "network".to_string()]
                    } else {
                        // Diskless VM: boot from CD or network first
                        vec!["cdrom".to_string(), "network".to_string()]
                    }
                });
            
            let firmware = req.boot.as_ref()
                .map(|b| if b.firmware == "bios" { FirmwareType::Bios } else { FirmwareType::Uefi })
                .unwrap_or(FirmwareType::Uefi);
            
            // Backend selection: prefer from config, then from direct field, default to "jit"
            let backend = req.config.as_ref()
                .map(|c| c.backend.clone())
                .or_else(|| req.backend.clone())
                .unwrap_or_else(|| "jit".to_string());
            
            let exec_config = VmExecConfig {
                vm_id: vm_id.clone(),
                name: req.name.clone(),
                vcpus,
                cpu_sockets: 1,
                cpu_cores: vcpus,
                cpu_threads: 1,
                cpu_model: "host".to_string(),
                memory_mb,
                memory_balloon: true,
                disks: disk_configs,
                networks,
                cdrom_iso: req.hardware.as_ref()
                    .and_then(|hw| hw.cdrom.as_ref())
                    .filter(|cd| cd.iso.is_some())
                    .and_then(|cd| cd.iso.clone())
                    .map(|iso| std::path::PathBuf::from(iso)),
                firmware,
                secure_boot: req.boot.as_ref().map(|b| b.secure_boot).unwrap_or(false),
                tpm_enabled: req.security.as_ref().map(|s| s.tpm).unwrap_or(false),
                tpm_version: req.security.as_ref()
                    .map(|s| s.tpm_version.clone())
                    .unwrap_or_else(|| "2.0".to_string()),
                machine_type: req.boot.as_ref()
                    .map(|b| b.machine_type.clone())
                    .unwrap_or_else(|| "q35".to_string()),
                nested_virt: req.hardware.as_ref()
                    .map(|hw| hw.cpu.nested_virt)
                    .unwrap_or(false),
                vnc_display: None,
                qmp_socket: None,
                enable_kvm: executor.is_kvm_available(),
                extra_args: vec![],
                backend,
            };
            
            // Register VM in hypervisor (ESXi-style: create = allocate resources)
            if let Err(e) = executor.register_vm(exec_config) {
                log::warn!("Failed to register VM in hypervisor: {} (VM state created)", e);
                // Don't fail - the VM state is created, hypervisor registration can be retried on start
            }
            
            // Return full VM object matching frontend Vm interface
            let created_at = chrono::DateTime::from_timestamp(now as i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            
            let response_vm = VmListItem {
                id: vm_id.clone(),
                name: req.name,
                status: "stopped".to_string(),
                description: req.description,
                host_node: None,
                template: req.template,
                config: VmListConfig {
                    cpu_cores: vcpus,
                    memory_mb,
                    disk_gb,
                    network,
                    boot_order: boot_order,
                },
                stats: None,
                created_at: created_at.clone(),
                updated_at: created_at,
                tags,
            };
            
            (
                StatusCode::CREATED,
                Json(ApiResponse::success(response_vm)),
            )
        }
        Err(e) => {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<VmListItem>::error(400, &e)),
            )
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn update(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(_req): Json<serde_json::Value>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({"id": id})))
}

/// Query parameters for VM deletion
#[derive(Debug, Deserialize, Default)]
pub struct DeleteVmQuery {
    /// Delete associated disk files (default: false for safety)
    #[serde(default)]
    pub delete_disks: bool,
    /// Delete associated backup files (default: false for safety)
    #[serde(default)]
    pub delete_backups: bool,
    /// Force deletion even if VM is running (will stop VM first)
    #[serde(default)]
    pub force: bool,
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Query(query): Query<DeleteVmQuery>,
) -> impl IntoResponse {
    use crate::executor::vm_executor;
    
    let state_mgr = vm_state();
    let mut executor = vm_executor();
    
    // Check if VM is running and handle accordingly
    if let Some(vm) = state_mgr.get_vm(&id) {
        if vm.status == crate::vmstate::VmStatus::Running {
            if query.force {
                // Force stop the VM first
                log::info!("Force stopping VM {} before deletion", id);
                if let Err(e) = executor.stop_vm(&id, true) {
                    log::warn!("Failed to stop VM {} before deletion: {}", id, e);
                }
            } else {
                return Json(ApiResponse::<serde_json::Value>::error(
                    400, 
                    "VM is running. Stop the VM first or use force=true to force deletion"
                ));
            }
        }
    }
    
    // Delete from hypervisor first (this also stops the VM if running)
    if executor.is_registered(&id) {
        if let Err(e) = executor.delete_vm(&id) {
            log::warn!("Failed to delete VM from hypervisor: {}", e);
            // Continue with state deletion even if hypervisor delete fails
        }
    }
    
    // Delete VM state with cleanup options
    if query.delete_disks || query.delete_backups {
        match state_mgr.delete_vm_with_cleanup(&id, query.delete_disks, query.delete_backups) {
            Ok((vm, cleanup_result)) => {
                log::info!(
                    "VM {} deleted: {} disks deleted, {} backups deleted, {} disk errors, {} backup errors",
                    vm.name,
                    cleanup_result.disks_deleted,
                    cleanup_result.backups_deleted,
                    cleanup_result.disk_errors.len(),
                    cleanup_result.backup_errors.len()
                );
                
                Json(ApiResponse::success(serde_json::json!({
                    "message": format!("VM '{}' deleted successfully", vm.name),
                    "cleanup": {
                        "disks_deleted": cleanup_result.disks_deleted,
                        "backups_deleted": cleanup_result.backups_deleted,
                        "schedules_deleted": cleanup_result.schedules_deleted,
                        "config_deleted": cleanup_result.config_deleted,
                        "disk_errors": cleanup_result.disk_errors,
                        "backup_errors": cleanup_result.backup_errors,
                    }
                })))
            }
            Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
        }
    } else {
        // Basic deletion without cleanup (backward compatible)
        match state_mgr.delete_vm(&id) {
            Ok(vm) => Json(ApiResponse::success(serde_json::json!({
                "message": format!("VM '{}' deleted successfully (disk files and backups preserved)", vm.name),
                "cleanup": {
                    "disks_deleted": 0,
                    "backups_deleted": 0,
                    "schedules_deleted": 0,
                    "config_deleted": false,
                    "disk_errors": [],
                    "backup_errors": [],
                }
            }))),
            Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn start(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::executor::vm_executor;
    
    let state_mgr = vm_state();
    let mut executor = vm_executor();
    
    // Get VM state
    let vm = match state_mgr.get_vm(&id) {
        Some(vm) => vm,
        None => return Json(ApiResponse::<serde_json::Value>::error(404, &format!("VM '{}' not found", id))),
    };
    
    // Check if already running
    if vm.status == StateVmStatus::Running {
        return Json(ApiResponse::<serde_json::Value>::error(400, "VM is already running"));
    }
    
    // VM must be registered in hypervisor - if not, it's a corrupted state
    if !executor.is_registered(&id) {
        log::error!("VM {} not registered in hypervisor - possible data corruption", id);
        return Json(ApiResponse::<serde_json::Value>::error(500, 
            "VM not registered in hypervisor. This may indicate disk or state corruption. Please delete and recreate the VM."));
    }
    
    // Start the VM (just calls hypervisor.start_vm)
    match executor.start_vm(&id) {
        Ok(_running_vm) => {
            // Update state to running
            let _ = state_mgr.set_vm_status(&id, StateVmStatus::Running);
            
            // Get VNC port for response
            let vnc_port = executor.get_vnc_port(&id).unwrap_or(5900);
            
            Json(ApiResponse::success(serde_json::json!({
                "task_id": Uuid::new_v4().to_string(),
                "status": "running",
                "vnc_port": vnc_port,
                "message": "VM started successfully"
            })))
        }
        Err(e) => {
            log::warn!("VM execution failed: {}", e);
            
            Json(ApiResponse::<serde_json::Value>::error(500, &format!("Failed to start VM: {}", e)))
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn stop(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::executor::vm_executor;
    
    let state_mgr = vm_state();
    let mut executor = vm_executor();
    
    // Try to stop the actual VM if running
    if executor.is_running(&id) {
        match executor.stop_vm(&id, false) {
            Ok(_) => log::info!("VM {} stopped via executor", id),
            Err(e) => log::warn!("Failed to stop VM via executor: {}", e),
        }
    }
    
    // Update state
    match state_mgr.set_vm_status(&id, StateVmStatus::Stopped) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string(),
            "status": "stopped"
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn restart(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    // Stop then start
    let _ = state_mgr.set_vm_status(&id, StateVmStatus::Stopped);
    match state_mgr.set_vm_status(&id, StateVmStatus::Running) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string(),
            "status": "running"
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn pause(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::executor::vm_executor;
    
    let state_mgr = vm_state();
    let mut executor = vm_executor();
    
    // Try to pause the actual VM if running
    if executor.is_running(&id) {
        if let Err(e) = executor.pause_vm(&id) {
            log::warn!("Failed to pause VM via executor: {}", e);
        }
    }
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Paused) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({"status": "paused"}))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn resume(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::executor::vm_executor;
    
    let state_mgr = vm_state();
    let mut executor = vm_executor();
    
    // Try to resume the actual VM if it exists
    if executor.is_running(&id) {
        if let Err(e) = executor.resume_vm(&id) {
            log::warn!("Failed to resume VM via executor: {}", e);
        }
    }
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Running) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({"status": "running"}))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn snapshot(
    State(_state): State<Arc<WebGuiState>>,
    Path(_id): Path<String>,
    Json(_req): Json<SnapshotRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "snapshot_id": Uuid::new_v4().to_string(),
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn clone(
    State(_state): State<Arc<WebGuiState>>,
    Path(_id): Path<String>,
    Json(_req): Json<CloneRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "vm_id": format!("vm-{}", &Uuid::new_v4().to_string()[..8]),
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn migrate(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(_req): Json<MigrateRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Migrating) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string()
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn console_ticket(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let ticket = ConsoleTicket {
        ticket: Uuid::new_v4().to_string(),
        port: 5900,
        console_type: "vnc".to_string(),
        url: format!("/novnc/{}?autoconnect=true", id),
        expires_at: chrono::Utc::now().timestamp() as u64 + 600,
    };
    
    Json(ApiResponse::success(ticket))
}

#[cfg(feature = "webgui")]
pub async fn metrics(
    State(_state): State<Arc<WebGuiState>>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    // In a real implementation, this would come from a monitoring system
    let metrics = VmMetrics {
        cpu_percent: 0.0,
        memory_used_mb: 0,
        disk_read_bps: 0,
        disk_write_bps: 0,
        net_rx_bps: 0,
        net_tx_bps: 0,
    };
    
    Json(ApiResponse::success(metrics))
}

/// WebSocket console handler for VM
#[cfg(feature = "webgui")]
pub async fn console_ws(
    ws: axum::extract::WebSocketUpgrade,
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Verify VM exists and is running
    let state_mgr = vm_state();
    let vm = state_mgr.list_vms().into_iter().find(|v| v.id == id);
    
    match vm {
        Some(v) if v.status == StateVmStatus::Running => {
            ws.on_upgrade(move |socket| handle_console_socket(socket, state, id))
        }
        Some(_) => {
            // VM exists but not running - return upgrade anyway but close immediately
            ws.on_upgrade(move |socket| handle_console_error(socket, 4001, "VM is not running"))
        }
        None => {
            ws.on_upgrade(move |socket| handle_console_error(socket, 4004, "VM not found"))
        }
    }
}

#[cfg(feature = "webgui")]
async fn handle_console_error(
    mut socket: axum::extract::ws::WebSocket,
    code: u16,
    reason: &'static str,
) {
    use axum::extract::ws::Message;
    
    let _ = socket.send(Message::Close(Some(axum::extract::ws::CloseFrame {
        code,
        reason: reason.into(),
    }))).await;
}

#[cfg(feature = "webgui")]
async fn handle_console_socket(
    socket: axum::extract::ws::WebSocket,
    _state: Arc<WebGuiState>,
    vm_id: String,
) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    use crate::executor::vm_executor;
    
    let (mut sender, mut receiver) = socket.split();
    
    // Get VGA dimensions
    let (width, height) = {
        let executor = vm_executor();
        executor.get_vga_dimensions(&vm_id)
            .ok()
            .flatten()
            .unwrap_or((800, 600))
    };
    
    // Send initial connection success message with display info
    let _ = sender.send(Message::Text(serde_json::json!({
        "type": "connected",
        "vm_id": vm_id,
        "console_type": "framebuffer",
        "width": width,
        "height": height,
        "message": "Console connection established"
    }).to_string().into())).await;
    
    // Spawn framebuffer update task
    let vm_id_clone = vm_id.clone();
    let frame_sender = Arc::new(tokio::sync::Mutex::new(sender));
    let frame_sender_clone = frame_sender.clone();
    
    let update_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100)); // 10 FPS
        loop {
            interval.tick().await;
            
            // Advance VM execution - process device ticks, interrupts, CPU cycles
            // This is critical for keyboard IRQ delivery and other device processing
            // 10000 cycles per 100ms tick = ~100kHz effective emulation speed
            {
                let executor = vm_executor();
                match executor.tick_vm(&vm_id_clone, 10000) {
                    Ok(true) => {}, // Normal operation
                    Ok(false) => {
                        // VM was reset (e.g., Ctrl+Alt+Del via keyboard controller)
                        log::info!("[Console] VM {} was reset via keyboard controller", vm_id_clone);
                        // Continue running - VM will restart from BIOS
                    },
                    Err(_) => break, // VM stopped or error
                }
            }
            
            // Get framebuffer from VM
            let framebuffer = {
                let executor = vm_executor();
                match executor.get_vga_framebuffer(&vm_id_clone) {
                    Ok(Some(fb)) => fb,
                    Ok(None) => continue, // No VGA device
                    Err(_) => break, // VM stopped
                }
            };
            
            // Send framebuffer as binary data with header
            let mut data = Vec::with_capacity(8 + framebuffer.len());
            // Header: [type(1), width(2), height(2), reserved(3)]
            data.push(0x01); // Frame type
            data.extend_from_slice(&(width as u16).to_le_bytes());
            data.extend_from_slice(&(height as u16).to_le_bytes());
            data.extend_from_slice(&[0u8; 3]); // Reserved
            data.extend_from_slice(&framebuffer);
            
            let mut sender = frame_sender_clone.lock().await;
            if sender.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
    });
    
    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    // Handle console commands
                    log::trace!("[Console] WS text: {}", text);
                    if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                        let mut sender = frame_sender.lock().await;
                        match cmd.get("type").and_then(|t| t.as_str()) {
                            Some("key") => {
                                // Forward key events to VM PS/2 keyboard
                                // Keys are injected to keyboard controller, which raises IRQ1
                                // BIOS/OS handles key combinations like Ctrl+Alt+Del
                                log::info!("[Console] Key event received: {:?}", cmd);
                                {
                                    let executor = vm_executor();
                                    
                                    let action = cmd.get("action").and_then(|a| a.as_str()).unwrap_or("press");
                                    let is_release = action == "up";
                                    
                                    if let Some(keys) = cmd.get("keys").and_then(|k| k.as_array()) {
                                        // Key combination - press all then release in reverse
                                        log::info!("[Console] Key combo: {:?} for VM {}", keys, vm_id);
                                        for key in keys {
                                            if let Some(key_str) = key.as_str() {
                                                log::info!("[Console] Injecting key press: {} to VM {}", key_str, vm_id);
                                                if let Err(e) = executor.inject_key(&vm_id, key_str, false) {
                                                    log::error!("[Console] inject_key press failed: {}", e);
                                                }
                                            }
                                        }
                                        for key in keys.iter().rev() {
                                            if let Some(key_str) = key.as_str() {
                                                log::info!("[Console] Injecting key release: {} to VM {}", key_str, vm_id);
                                                if let Err(e) = executor.inject_key(&vm_id, key_str, true) {
                                                    log::error!("[Console] inject_key release failed: {}", e);
                                                }
                                            }
                                        }
                                    } else if let Some(code) = cmd.get("code").and_then(|c| c.as_str()) {
                                        // Prefer code over key - code is physical key location, more reliable
                                        if !code.is_empty() {
                                            let key_mapped = map_js_code_to_ps2(code);
                                            log::info!("[Console] Key by code: code='{}' -> mapped='{}' is_release={} for VM {}", 
                                                       code, key_mapped, is_release, vm_id);
                                            if let Err(e) = executor.inject_key(&vm_id, &key_mapped, is_release) {
                                                log::error!("[Console] inject_key failed: {}", e);
                                            }
                                        }
                                    } else if let Some(key) = cmd.get("key").and_then(|k| k.as_str()) {
                                        // Fallback to key if code is not available
                                        if !key.is_empty() {
                                            let key_mapped = map_js_key_to_ps2(key, &cmd);
                                            log::info!("[Console] Key by key: key='{}' -> mapped='{}' is_release={} for VM {}", 
                                                       key, key_mapped, is_release, vm_id);
                                            if let Err(e) = executor.inject_key(&vm_id, &key_mapped, is_release) {
                                                log::error!("[Console] inject_key failed: {}", e);
                                            }
                                        }
                                    }
                                }
                                
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "key"
                                }).to_string().into())).await;
                            }
                            Some("mouse") => {
                                // Forward mouse events to VM
                                // TODO: Implement mouse input handling
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "mouse"
                                }).to_string().into())).await;
                            }
                            Some("resize") => {
                                // Handle screen resize request
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "resize"
                                }).to_string().into())).await;
                            }
                            Some("ping") => {
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "pong"
                                }).to_string().into())).await;
                            }
                            Some("request_frame") => {
                                // Client requesting immediate frame update
                                let framebuffer = {
                                    let executor = vm_executor();
                                    executor.get_vga_framebuffer(&vm_id).ok().flatten()
                                };
                                if let Some(fb) = framebuffer {
                                    let mut data = Vec::with_capacity(8 + fb.len());
                                    data.push(0x01);
                                    data.extend_from_slice(&(width as u16).to_le_bytes());
                                    data.extend_from_slice(&(height as u16).to_le_bytes());
                                    data.extend_from_slice(&[0u8; 3]);
                                    data.extend_from_slice(&fb);
                                    let _ = sender.send(Message::Binary(data.into())).await;
                                }
                            }
                            _ => {
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "error",
                                    "message": "Unknown command"
                                }).to_string().into())).await;
                            }
                        }
                    }
                }
                Message::Binary(_data) => {
                    // Binary input (keyboard scancodes, etc.) - not yet implemented
                }
                Message::Ping(data) => {
                    let mut sender = frame_sender.lock().await;
                    let _ = sender.send(Message::Pong(data)).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }
    
    // Cancel update task
    update_task.abort();
    
    log::debug!("Console WebSocket closed for VM {}", vm_id);
}

/// Map JavaScript key value to PS/2 keyboard key name
fn map_js_key_to_ps2(key: &str, cmd: &serde_json::Value) -> String {
    // Check for modifier states
    let shift = cmd.get("shift").and_then(|v| v.as_bool()).unwrap_or(false);
    let ctrl = cmd.get("ctrl").and_then(|v| v.as_bool()).unwrap_or(false);
    let alt = cmd.get("alt").and_then(|v| v.as_bool()).unwrap_or(false);
    
    // If key is a single character and not a modifier
    if key.len() == 1 {
        let c = key.chars().next().unwrap();
        // For letter keys, our PS2 keyboard expects lowercase
        if c.is_ascii_alphabetic() {
            return c.to_lowercase().to_string();
        }
        // For numbers and symbols, return as-is
        return key.to_string();
    }
    
    // Map special keys
    match key {
        // Modifier keys
        "Shift" | "ShiftLeft" | "ShiftRight" => "lshift".to_string(),
        "Control" | "ControlLeft" | "ControlRight" => "lctrl".to_string(),
        "Alt" | "AltLeft" => "lalt".to_string(),
        "AltRight" | "AltGraph" => "ralt".to_string(),
        "Meta" | "MetaLeft" | "MetaRight" => "lmeta".to_string(),
        
        // Arrow keys
        "ArrowUp" => "up".to_string(),
        "ArrowDown" => "down".to_string(),
        "ArrowLeft" => "left".to_string(),
        "ArrowRight" => "right".to_string(),
        
        // Function keys
        "F1" => "f1".to_string(),
        "F2" => "f2".to_string(),
        "F3" => "f3".to_string(),
        "F4" => "f4".to_string(),
        "F5" => "f5".to_string(),
        "F6" => "f6".to_string(),
        "F7" => "f7".to_string(),
        "F8" => "f8".to_string(),
        "F9" => "f9".to_string(),
        "F10" => "f10".to_string(),
        "F11" => "f11".to_string(),
        "F12" => "f12".to_string(),
        
        // Navigation/editing keys
        "Enter" => "enter".to_string(),
        "Tab" => "tab".to_string(),
        "Backspace" => "backspace".to_string(),
        "Delete" => "delete".to_string(),
        "Insert" => "insert".to_string(),
        "Home" => "home".to_string(),
        "End" => "end".to_string(),
        "PageUp" => "pageup".to_string(),
        "PageDown" => "pagedown".to_string(),
        "Escape" => "escape".to_string(),
        " " => "space".to_string(),
        
        // Lock keys
        "CapsLock" => "capslock".to_string(),
        "NumLock" => "numlock".to_string(),
        "ScrollLock" => "scrolllock".to_string(),
        
        // Other special keys
        "PrintScreen" => "printscreen".to_string(),
        "Pause" => "pause".to_string(),
        "ContextMenu" => "menu".to_string(),
        
        // Numpad keys
        "Numpad0" => "kp0".to_string(),
        "Numpad1" => "kp1".to_string(),
        "Numpad2" => "kp2".to_string(),
        "Numpad3" => "kp3".to_string(),
        "Numpad4" => "kp4".to_string(),
        "Numpad5" => "kp5".to_string(),
        "Numpad6" => "kp6".to_string(),
        "Numpad7" => "kp7".to_string(),
        "Numpad8" => "kp8".to_string(),
        "Numpad9" => "kp9".to_string(),
        "NumpadEnter" => "kpenter".to_string(),
        "NumpadAdd" => "kpplus".to_string(),
        "NumpadSubtract" => "kpminus".to_string(),
        "NumpadMultiply" => "kpasterisk".to_string(),
        "NumpadDivide" => "kpslash".to_string(),
        "NumpadDecimal" => "kpdot".to_string(),
        
        // Default: return as lowercase
        _ => key.to_lowercase(),
    }
}

/// Map JavaScript key code to PS/2 keyboard key name
fn map_js_code_to_ps2(code: &str) -> String {
    match code {
        // Letter keys (KeyA through KeyZ)
        c if c.starts_with("Key") && c.len() == 4 => {
            c[3..].to_lowercase()
        }
        
        // Digit keys (Digit0 through Digit9)
        c if c.starts_with("Digit") && c.len() == 6 => {
            c[5..].to_string()
        }
        
        // Numpad keys (same mapping as above)
        "Numpad0" => "kp0".to_string(),
        "Numpad1" => "kp1".to_string(),
        "Numpad2" => "kp2".to_string(),
        "Numpad3" => "kp3".to_string(),
        "Numpad4" => "kp4".to_string(),
        "Numpad5" => "kp5".to_string(),
        "Numpad6" => "kp6".to_string(),
        "Numpad7" => "kp7".to_string(),
        "Numpad8" => "kp8".to_string(),
        "Numpad9" => "kp9".to_string(),
        "NumpadEnter" => "kpenter".to_string(),
        "NumpadAdd" => "kpplus".to_string(),
        "NumpadSubtract" => "kpminus".to_string(),
        "NumpadMultiply" => "kpasterisk".to_string(),
        "NumpadDivide" => "kpslash".to_string(),
        "NumpadDecimal" => "kpdot".to_string(),
        
        // Arrow keys
        "ArrowUp" => "up".to_string(),
        "ArrowDown" => "down".to_string(),
        "ArrowLeft" => "left".to_string(),
        "ArrowRight" => "right".to_string(),
        
        // Function keys
        c if c.starts_with("F") && c.len() >= 2 && c.len() <= 3 => {
            c.to_lowercase()
        }
        
        // Modifier keys
        "ShiftLeft" => "lshift".to_string(),
        "ShiftRight" => "rshift".to_string(),
        "ControlLeft" => "lctrl".to_string(),
        "ControlRight" => "rctrl".to_string(),
        "AltLeft" => "lalt".to_string(),
        "AltRight" => "ralt".to_string(),
        "MetaLeft" => "lmeta".to_string(),
        "MetaRight" => "rmeta".to_string(),
        
        // Special keys
        "Enter" => "enter".to_string(),
        "Tab" => "tab".to_string(),
        "Backspace" => "backspace".to_string(),
        "Delete" => "delete".to_string(),
        "Insert" => "insert".to_string(),
        "Home" => "home".to_string(),
        "End" => "end".to_string(),
        "PageUp" => "pageup".to_string(),
        "PageDown" => "pagedown".to_string(),
        "Escape" => "escape".to_string(),
        "Space" => "space".to_string(),
        "CapsLock" => "capslock".to_string(),
        "NumLock" => "numlock".to_string(),
        "ScrollLock" => "scrolllock".to_string(),
        "PrintScreen" => "printscreen".to_string(),
        "Pause" => "pause".to_string(),
        "ContextMenu" => "menu".to_string(),
        
        // Punctuation keys
        "Minus" => "-".to_string(),
        "Equal" => "=".to_string(),
        "BracketLeft" => "[".to_string(),
        "BracketRight" => "]".to_string(),
        "Backslash" => "\\".to_string(),
        "Semicolon" => ";".to_string(),
        "Quote" => "'".to_string(),
        "Backquote" => "`".to_string(),
        "Comma" => ",".to_string(),
        "Period" => ".".to_string(),
        "Slash" => "/".to_string(),
        
        // Default: return as lowercase
        _ => code.to_lowercase(),
    }
}
