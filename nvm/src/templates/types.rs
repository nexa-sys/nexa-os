//! Template Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Template error
#[derive(Debug)]
pub enum TemplateError {
    /// IO error
    Io(std::io::Error),
    /// Not found
    NotFound(String),
    /// Already exists
    AlreadyExists(String),
    /// Invalid format
    InvalidFormat(String),
    /// Import error
    Import(String),
    /// Export error
    Export(String),
    /// Validation error
    Validation(String),
    /// Serialization error
    Serialization(String),
    /// Invalid template
    Invalid(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::NotFound(s) => write!(f, "Template not found: {}", s),
            Self::AlreadyExists(s) => write!(f, "Template already exists: {}", s),
            Self::InvalidFormat(s) => write!(f, "Invalid format: {}", s),
            Self::Import(s) => write!(f, "Import error: {}", s),
            Self::Export(s) => write!(f, "Export error: {}", s),
            Self::Validation(s) => write!(f, "Validation error: {}", s),
            Self::Serialization(s) => write!(f, "Serialization error: {}", s),
            Self::Invalid(s) => write!(f, "Invalid template: {}", s),
        }
    }
}

impl std::error::Error for TemplateError {}

impl From<std::io::Error> for TemplateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Template ID
pub type TemplateId = String;

/// VM Template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// Unique template ID
    pub id: TemplateId,
    /// Template name
    pub name: String,
    /// Description
    pub description: String,
    /// Version
    pub version: String,
    /// Operating system type
    pub os_type: OsType,
    /// OS variant (e.g., "Ubuntu 22.04", "Windows Server 2022")
    pub os_variant: String,
    /// Architecture
    pub arch: Architecture,
    /// Default VM configuration
    pub default_config: TemplateConfig,
    /// Disk images
    pub disks: Vec<TemplateDisk>,
    /// Template format
    pub format: TemplateFormat,
    /// Creation timestamp
    pub created_at: u64,
    /// Last modified timestamp
    pub updated_at: u64,
    /// Template size in bytes
    pub size: u64,
    /// Checksum (SHA256)
    pub checksum: Option<String>,
    /// Custom properties
    pub properties: HashMap<String, String>,
    /// Tags for organization
    pub tags: Vec<String>,
    /// Whether template is public
    pub public: bool,
    /// Owner (tenant/user)
    pub owner: Option<String>,
}

/// Operating system type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsType {
    Linux,
    Windows,
    FreeBsd,
    Other,
}

/// CPU architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    X86_64,
    Aarch64,
    Other,
}

/// Template format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TemplateFormat {
    /// Native NVM format
    #[default]
    Native,
    /// OVA (Open Virtual Appliance)
    Ova,
    /// OVF (Open Virtualization Format)
    Ovf,
    /// QCOW2 with metadata
    Qcow2,
    /// Raw disk image
    Raw,
}

/// Template VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    /// Default vCPUs
    pub vcpus: u32,
    /// Minimum vCPUs
    pub min_vcpus: u32,
    /// Maximum vCPUs
    pub max_vcpus: u32,
    /// Default memory (MB)
    pub memory_mb: u64,
    /// Minimum memory (MB)
    pub min_memory_mb: u64,
    /// Maximum memory (MB)
    pub max_memory_mb: u64,
    /// Network interfaces
    pub network_interfaces: u32,
    /// Boot order
    pub boot_order: Vec<String>,
    /// UEFI boot
    pub uefi: bool,
    /// Secure boot
    pub secure_boot: bool,
    /// TPM required
    pub tpm: bool,
    /// Additional options
    pub options: HashMap<String, String>,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            vcpus: 2,
            min_vcpus: 1,
            max_vcpus: 64,
            memory_mb: 2048,
            min_memory_mb: 512,
            max_memory_mb: 262144, // 256GB
            network_interfaces: 1,
            boot_order: vec!["disk".to_string(), "network".to_string()],
            uefi: false,
            secure_boot: false,
            tpm: false,
            options: HashMap::new(),
        }
    }
}

/// Template disk definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateDisk {
    /// Disk ID within template
    pub id: String,
    /// Disk name
    pub name: String,
    /// Disk size in bytes
    pub size: u64,
    /// Disk format
    pub format: DiskFormat,
    /// Storage path (relative to template)
    pub path: String,
    /// Is boot disk
    pub bootable: bool,
    /// Bus type
    pub bus: DiskBus,
    /// Checksum
    pub checksum: Option<String>,
}

/// Disk format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskFormat {
    Raw,
    Qcow2,
    Vmdk,
    Vdi,
    Vhd,
    Vhdx,
}

/// Disk bus type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskBus {
    Virtio,
    Scsi,
    Sata,
    Ide,
    Nvme,
}

/// Template deployment options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployOptions {
    /// VM name
    pub name: String,
    /// Override vCPUs
    pub vcpus: Option<u32>,
    /// Override memory
    pub memory_mb: Option<u64>,
    /// Override disk size (expand only)
    pub disk_size: Option<u64>,
    /// Target storage pool
    pub storage_pool: Option<String>,
    /// Target network
    pub network: Option<String>,
    /// Target node
    pub node: Option<String>,
    /// Start after deploy
    pub start: bool,
    /// Cloud-init user data
    pub cloud_init: Option<CloudInitConfig>,
    /// Custom properties
    pub properties: HashMap<String, String>,
}

/// Cloud-init configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudInitConfig {
    /// User data (YAML)
    pub user_data: Option<String>,
    /// Meta data
    pub meta_data: Option<String>,
    /// Network config
    pub network_config: Option<String>,
    /// SSH authorized keys
    pub ssh_keys: Vec<String>,
    /// Default user password (hashed)
    pub password: Option<String>,
    /// Hostname
    pub hostname: Option<String>,
}

/// Template import result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub template_id: TemplateId,
    pub name: String,
    pub warnings: Vec<String>,
    pub converted_disks: Vec<String>,
}

/// Template export options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportOptions {
    /// Target format
    pub format: TemplateFormat,
    /// Output path
    pub output_path: String,
    /// Include checksum
    pub include_checksum: bool,
    /// Compress
    pub compress: bool,
    /// OVF version (1.0, 1.1, 2.0)
    pub ovf_version: Option<String>,
}
