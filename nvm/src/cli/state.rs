//! Local State Storage for NVM
//!
//! Persists VM and resource state to disk for offline operations
//! and nvmserver startup recovery.

use super::{CliResult, CliError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

/// Local state database
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalState {
    /// Registered VMs
    pub vms: HashMap<String, VmRecord>,
    /// Storage pools
    pub storage_pools: HashMap<String, StoragePoolRecord>,
    /// Networks
    pub networks: HashMap<String, NetworkRecord>,
    /// Last update timestamp
    pub last_updated: u64,
}

/// VM record in local state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmRecord {
    pub id: String,
    pub name: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub status: VmStatus,
    pub node: Option<String>,
    pub created_at: u64,
    pub config_path: Option<PathBuf>,
    pub disk_path: Option<PathBuf>,
}

/// VM status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmStatus {
    Stopped,
    Running,
    Paused,
    Suspended,
    Creating,
    Migrating,
    Error,
}

impl std::fmt::Display for VmStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmStatus::Stopped => write!(f, "stopped"),
            VmStatus::Running => write!(f, "running"),
            VmStatus::Paused => write!(f, "paused"),
            VmStatus::Suspended => write!(f, "suspended"),
            VmStatus::Creating => write!(f, "creating"),
            VmStatus::Migrating => write!(f, "migrating"),
            VmStatus::Error => write!(f, "error"),
        }
    }
}

/// Storage pool record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePoolRecord {
    pub name: String,
    pub pool_type: String,
    pub path: PathBuf,
    pub total_gb: u64,
    pub used_gb: u64,
}

/// Network record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRecord {
    pub name: String,
    pub network_type: String,
    pub cidr: Option<String>,
    pub bridge: Option<String>,
}

impl LocalState {
    /// Load state from disk or create empty state
    pub fn load() -> Self {
        let path = state_file_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(state) = serde_json::from_str(&content) {
                    return state;
                }
            }
        }
        Self::default()
    }
    
    /// Save state to disk
    pub fn save(&self) -> CliResult<()> {
        let path = state_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CliError::Config(e.to_string()))?;
        fs::write(&path, content)?;
        Ok(())
    }
    
    /// Update last_updated timestamp
    pub fn touch(&mut self) {
        self.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
    
    /// Add or update a VM
    pub fn upsert_vm(&mut self, vm: VmRecord) {
        self.vms.insert(vm.id.clone(), vm);
        self.touch();
    }
    
    /// Remove a VM
    pub fn remove_vm(&mut self, vm_id: &str) -> Option<VmRecord> {
        let result = self.vms.remove(vm_id);
        if result.is_some() {
            self.touch();
        }
        result
    }
    
    /// Get a VM by ID
    pub fn get_vm(&self, vm_id: &str) -> Option<&VmRecord> {
        self.vms.get(vm_id)
    }
    
    /// Get a VM by name
    pub fn get_vm_by_name(&self, name: &str) -> Option<&VmRecord> {
        self.vms.values().find(|vm| vm.name == name)
    }
    
    /// List all VMs
    pub fn list_vms(&self) -> Vec<&VmRecord> {
        self.vms.values().collect()
    }
    
    /// Update VM status
    pub fn set_vm_status(&mut self, vm_id: &str, status: VmStatus) -> Option<()> {
        if let Some(vm) = self.vms.get_mut(vm_id) {
            vm.status = status;
            self.touch();
            Some(())
        } else {
            None
        }
    }
}

/// Get the state file path
pub fn state_file_path() -> PathBuf {
    let base = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nvm").join("state.json")
}

/// Get the VMs directory path
pub fn vms_dir() -> PathBuf {
    let base = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nvm").join("vms")
}

/// Get the images directory path  
pub fn images_dir() -> PathBuf {
    let base = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nvm").join("images")
}
