//! VM State Manager
//!
//! Centralized VM state management for both CLI and WebGUI.
//! Provides persistence and synchronization of VM states.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Global VM state manager
pub struct VmStateManager {
    /// In-memory VM states
    vms: RwLock<HashMap<String, VmState>>,
    /// Persistence path
    state_file: PathBuf,
    /// Storage pools
    storage_pools: RwLock<HashMap<String, StoragePoolState>>,
    /// Networks
    networks: RwLock<HashMap<String, NetworkState>>,
}

/// VM state record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmState {
    pub id: String,
    pub name: String,
    pub status: VmStatus,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub node: Option<String>,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub config_path: Option<PathBuf>,
    pub disk_paths: Vec<PathBuf>,
    pub network_interfaces: Vec<NetworkInterface>,
    pub tags: Vec<String>,
    pub description: Option<String>,
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
        let s = match self {
            VmStatus::Stopped => "stopped",
            VmStatus::Running => "running",
            VmStatus::Paused => "paused",
            VmStatus::Suspended => "suspended",
            VmStatus::Creating => "creating",
            VmStatus::Migrating => "migrating",
            VmStatus::Error => "error",
        };
        write!(f, "{}", s)
    }
}

/// Network interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub id: String,
    pub mac: String,
    pub network: String,
    pub model: String,
    pub ip: Option<String>,
}

/// Storage pool state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePoolState {
    pub name: String,
    pub pool_type: String,
    pub path: PathBuf,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub status: String,
}

/// Network state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    pub name: String,
    pub network_type: String,
    pub cidr: Option<String>,
    pub bridge: Option<String>,
    pub status: String,
}

/// Persistent state file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    vms: HashMap<String, VmState>,
    storage_pools: HashMap<String, StoragePoolState>,
    networks: HashMap<String, NetworkState>,
    version: u32,
}

impl VmStateManager {
    /// Create a new VM state manager
    pub fn new() -> Self {
        let state_file = Self::default_state_path();
        let mut manager = Self {
            vms: RwLock::new(HashMap::new()),
            state_file: state_file.clone(),
            storage_pools: RwLock::new(HashMap::new()),
            networks: RwLock::new(HashMap::new()),
        };
        
        // Load persisted state
        manager.load_state();
        manager
    }
    
    /// Get default state file path
    fn default_state_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("nvm")
            .join("vmstate.json")
    }
    
    /// Load state from disk
    fn load_state(&mut self) {
        if self.state_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&self.state_file) {
                if let Ok(state) = serde_json::from_str::<PersistedState>(&content) {
                    *self.vms.write() = state.vms;
                    *self.storage_pools.write() = state.storage_pools;
                    *self.networks.write() = state.networks;
                    log::info!("Loaded {} VMs from state file", self.vms.read().len());
                    return;
                }
            }
        }
        log::info!("Starting with empty VM state");
    }
    
    /// Save state to disk
    pub fn save_state(&self) -> std::io::Result<()> {
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let state = PersistedState {
            vms: self.vms.read().clone(),
            storage_pools: self.storage_pools.read().clone(),
            networks: self.networks.read().clone(),
            version: 1,
        };
        
        let content = serde_json::to_string_pretty(&state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&self.state_file, content)?;
        Ok(())
    }
    
    // ========== VM Operations ==========
    
    /// List all VMs
    pub fn list_vms(&self) -> Vec<VmState> {
        self.vms.read().values().cloned().collect()
    }
    
    /// Get a VM by ID
    pub fn get_vm(&self, id: &str) -> Option<VmState> {
        self.vms.read().get(id).cloned()
    }
    
    /// Get a VM by name
    pub fn get_vm_by_name(&self, name: &str) -> Option<VmState> {
        self.vms.read().values().find(|vm| vm.name == name).cloned()
    }
    
    /// Create a new VM
    pub fn create_vm(&self, mut vm: VmState) -> Result<String, String> {
        let mut vms = self.vms.write();
        
        // Generate ID if not provided
        if vm.id.is_empty() {
            vm.id = format!("vm-{:06x}", rand::random::<u32>() & 0xffffff);
        }
        
        // Check for duplicate name
        if vms.values().any(|v| v.name == vm.name) {
            return Err(format!("VM with name '{}' already exists", vm.name));
        }
        
        let id = vm.id.clone();
        vms.insert(id.clone(), vm);
        drop(vms);
        
        let _ = self.save_state();
        Ok(id)
    }
    
    /// Update VM status
    pub fn set_vm_status(&self, id: &str, status: VmStatus) -> Result<(), String> {
        let mut vms = self.vms.write();
        if let Some(vm) = vms.get_mut(id) {
            vm.status = status;
            if status == VmStatus::Running {
                vm.started_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                );
            }
            drop(vms);
            let _ = self.save_state();
            Ok(())
        } else {
            Err(format!("VM '{}' not found", id))
        }
    }
    
    /// Delete a VM
    pub fn delete_vm(&self, id: &str) -> Result<VmState, String> {
        let mut vms = self.vms.write();
        if let Some(vm) = vms.remove(id) {
            drop(vms);
            let _ = self.save_state();
            Ok(vm)
        } else {
            Err(format!("VM '{}' not found", id))
        }
    }
    
    // ========== Storage Pool Operations ==========
    
    /// List storage pools
    pub fn list_storage_pools(&self) -> Vec<StoragePoolState> {
        self.storage_pools.read().values().cloned().collect()
    }
    
    /// Get a storage pool
    pub fn get_storage_pool(&self, name: &str) -> Option<StoragePoolState> {
        self.storage_pools.read().get(name).cloned()
    }
    
    /// Create a storage pool
    pub fn create_storage_pool(&self, pool: StoragePoolState) -> Result<(), String> {
        let mut pools = self.storage_pools.write();
        if pools.contains_key(&pool.name) {
            return Err(format!("Storage pool '{}' already exists", pool.name));
        }
        pools.insert(pool.name.clone(), pool);
        drop(pools);
        let _ = self.save_state();
        Ok(())
    }
    
    // ========== Network Operations ==========
    
    /// List networks
    pub fn list_networks(&self) -> Vec<NetworkState> {
        self.networks.read().values().cloned().collect()
    }
    
    /// Get a network
    pub fn get_network(&self, name: &str) -> Option<NetworkState> {
        self.networks.read().get(name).cloned()
    }
    
    /// Create a network
    pub fn create_network(&self, network: NetworkState) -> Result<(), String> {
        let mut networks = self.networks.write();
        if networks.contains_key(&network.name) {
            return Err(format!("Network '{}' already exists", network.name));
        }
        networks.insert(network.name.clone(), network);
        drop(networks);
        let _ = self.save_state();
        Ok(())
    }
}

impl Default for VmStateManager {
    fn default() -> Self {
        Self::new()
    }
}

// Global instance (lazy initialized)
lazy_static::lazy_static! {
    /// Global VM state manager instance
    pub static ref VM_STATE: Arc<VmStateManager> = Arc::new(VmStateManager::new());
}

/// Get the global VM state manager
pub fn vm_state() -> &'static Arc<VmStateManager> {
    &VM_STATE
}
