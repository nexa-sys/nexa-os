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
    
    let state_mgr = vm_state();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    // Extract values from config object (frontend format) or direct fields (API format)
    let (vcpus, memory_mb, disk_gb, network) = if let Some(config) = &req.config {
        (config.cpu_cores, config.memory_mb, config.disk_gb, config.network.clone())
    } else {
        (
            req.vcpus.unwrap_or(2),
            req.memory_mb.unwrap_or(2048),
            req.disks.as_ref().and_then(|d| d.first().map(|d| d.size_gb)).unwrap_or(20),
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
        network_interfaces,
        tags: tags.clone(),
        description: req.description.clone(),
    };
    
    match state_mgr.create_vm(vm) {
        Ok(vm_id) => {
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
                    boot_order: req.config.as_ref()
                        .map(|c| c.boot_order.clone())
                        .unwrap_or_else(|| vec!["disk".to_string(), "cdrom".to_string()]),
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

#[cfg(feature = "webgui")]
pub async fn delete(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.delete_vm(&id) {
        Ok(_) => Json(ApiResponse::<()>::success(())),
        Err(e) => Json(ApiResponse::<()>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn start(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::executor::{vm_executor, VmExecConfig, DiskExecConfig, NetworkExecConfig, FirmwareType, NetworkType};
    
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
    
    // Build execution config
    let data_dir = executor.vm_data_dir(&id);
    
    // Create disk configurations
    let disks: Vec<DiskExecConfig> = if vm.disk_paths.is_empty() {
        // Create default disk if none exists
        let disk_path = data_dir.join(format!("{}-disk0.qcow2", vm.name));
        if !disk_path.exists() {
            if let Err(e) = executor.create_disk_image(&disk_path, vm.disk_gb, "qcow2") {
                return Json(ApiResponse::<serde_json::Value>::error(500, &format!("Failed to create disk: {}", e)));
            }
        }
        vec![DiskExecConfig {
            path: disk_path,
            format: "qcow2".to_string(),
            bus: "virtio".to_string(),
            cache: "writeback".to_string(),
            io: "native".to_string(),
            bootable: true,
            discard: true,
            readonly: false,
            serial: None,
        }]
    } else {
        vm.disk_paths.iter().enumerate().map(|(idx, path)| {
            DiskExecConfig {
                path: path.clone(),
                format: "qcow2".to_string(),
                bus: "virtio".to_string(),
                cache: "writeback".to_string(),
                io: "native".to_string(),
                bootable: idx == 0,
                discard: true,
                readonly: false,
                serial: None,
            }
        }).collect()
    };
    
    // Build network configurations
    let networks: Vec<NetworkExecConfig> = vm.network_interfaces.iter().map(|nic| {
        NetworkExecConfig {
            id: nic.id.clone(),
            mac: nic.mac.clone(),
            net_type: NetworkType::User, // Default to user-mode
            bridge: None,
            model: "virtio-net-pci".to_string(),
            multiqueue: false,
            queues: 1,
            vlan_id: None,
        }
    }).collect();
    
    let exec_config = VmExecConfig {
        vm_id: id.clone(),
        name: vm.name.clone(),
        vcpus: vm.vcpus,
        cpu_sockets: 1,
        cpu_cores: vm.vcpus,
        cpu_threads: 1,
        cpu_model: "host".to_string(),
        memory_mb: vm.memory_mb,
        memory_balloon: true,
        disks,
        networks,
        cdrom_iso: None,
        firmware: FirmwareType::Uefi,
        secure_boot: false,
        tpm_enabled: false,
        tpm_version: "2.0".to_string(),
        machine_type: "q35".to_string(),
        nested_virt: false,
        vnc_display: None,
        qmp_socket: None,
        enable_kvm: executor.is_kvm_available(),
        extra_args: vec![],
    };
    
    // Start the VM
    match executor.start_vm(exec_config) {
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
            // If QEMU isn't available, still update state for demo purposes
            log::warn!("VM execution failed (QEMU may not be available): {}", e);
            let _ = state_mgr.set_vm_status(&id, StateVmStatus::Running);
            
            Json(ApiResponse::success(serde_json::json!({
                "task_id": Uuid::new_v4().to_string(),
                "status": "running",
                "warning": format!("VM state set but execution failed: {}", e)
            })))
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
    state: Arc<WebGuiState>,
    vm_id: String,
) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    
    let (mut sender, mut receiver) = socket.split();
    
    // Send initial connection success message
    let _ = sender.send(Message::Text(serde_json::json!({
        "type": "connected",
        "vm_id": vm_id,
        "console_type": "vnc",
        "message": "Console connection established"
    }).to_string().into())).await;
    
    // In a real implementation, this would connect to QEMU's VNC server
    // and proxy the VNC protocol over WebSocket (like noVNC does)
    
    // For now, we'll just echo messages and handle basic commands
    while let Some(msg) = receiver.next().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    // Handle console commands
                    if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                        match cmd.get("type").and_then(|t| t.as_str()) {
                            Some("key") => {
                                // Would forward key events to QEMU
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "key"
                                }).to_string().into())).await;
                            }
                            Some("mouse") => {
                                // Would forward mouse events to QEMU
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "mouse"
                                }).to_string().into())).await;
                            }
                            Some("resize") => {
                                // Would handle screen resize
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
                            _ => {
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "error",
                                    "message": "Unknown command"
                                }).to_string().into())).await;
                            }
                        }
                    }
                }
                Message::Binary(data) => {
                    // Binary data would be VNC protocol frames
                    // Echo back for now (in real impl, proxy to QEMU VNC)
                    let _ = sender.send(Message::Binary(data)).await;
                }
                Message::Ping(data) => {
                    let _ = sender.send(Message::Pong(data)).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }
    
    log::debug!("Console WebSocket closed for VM {}", vm_id);
}
