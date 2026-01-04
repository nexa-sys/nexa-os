//! VM State Manager
//!
//! Centralized VM state management for both CLI and WebGUI.
//! Provides persistence and synchronization of VM states.
//! Enterprise features: event logging, audit trail, metrics history.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

/// Maximum number of events to keep in memory
const MAX_EVENTS: usize = 1000;

/// Result of cleanup operations during VM deletion
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteCleanupResult {
    /// Number of disk files successfully deleted
    pub disks_deleted: usize,
    /// Number of backup files successfully deleted
    pub backups_deleted: usize,
    /// Number of backup schedules deleted (single-VM schedules)
    pub schedules_deleted: usize,
    /// Whether the config file was deleted
    pub config_deleted: bool,
    /// Errors encountered while deleting disks
    pub disk_errors: Vec<String>,
    /// Errors encountered while deleting backups
    pub backup_errors: Vec<String>,
}

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
    /// Event log (recent events)
    events: RwLock<VecDeque<SystemEvent>>,
    /// VM Templates
    templates: RwLock<HashMap<String, VmTemplate>>,
    /// Backup jobs
    backups: RwLock<HashMap<String, BackupRecord>>,
    /// Backup schedules
    backup_schedules: RwLock<HashMap<String, BackupSchedule>>,
}

/// VM Template record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub os_type: String,
    pub os_version: Option<String>,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub disk_path: Option<PathBuf>,
    pub created_at: u64,
    pub updated_at: u64,
    pub size_bytes: u64,
    pub tags: Vec<String>,
    pub public: bool,
    pub owner: String,
}

/// Backup record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: String,
    pub vm_id: String,
    pub vm_name: String,
    pub backup_type: String,  // "full" or "incremental"
    pub status: BackupStatus,
    pub progress: f64,
    pub size_bytes: u64,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub target_path: PathBuf,
    pub description: Option<String>,
}

/// Backup status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for BackupStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupStatus::Pending => write!(f, "pending"),
            BackupStatus::Running => write!(f, "running"),
            BackupStatus::Completed => write!(f, "completed"),
            BackupStatus::Failed => write!(f, "failed"),
            BackupStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Backup schedule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSchedule {
    pub id: String,
    pub name: String,
    pub vm_ids: Vec<String>,
    pub backup_type: String,
    pub cron_schedule: String,  // Cron format: "0 2 * * *"
    pub target_path: PathBuf,
    pub retention_days: u32,
    pub enabled: bool,
    pub last_run: Option<u64>,
    pub next_run: Option<u64>,
    pub created_at: u64,
}

/// System event for audit logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub id: String,
    pub timestamp: u64,
    pub event_type: EventType,
    pub severity: EventSeverity,
    pub source: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub user: Option<String>,
}

/// Event type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    VmCreate,
    VmStart,
    VmStop,
    VmDelete,
    VmMigrate,
    VmSnapshot,
    VmClone,
    VmError,
    StorageCreate,
    StorageDelete,
    NetworkCreate,
    NetworkDelete,
    UserLogin,
    UserLogout,
    SystemStart,
    SystemShutdown,
    ConfigChange,
    SecurityAlert,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EventType::VmCreate => "vm_create",
            EventType::VmStart => "vm_start",
            EventType::VmStop => "vm_stop",
            EventType::VmDelete => "vm_delete",
            EventType::VmMigrate => "vm_migrate",
            EventType::VmSnapshot => "vm_snapshot",
            EventType::VmClone => "vm_clone",
            EventType::VmError => "vm_error",
            EventType::StorageCreate => "storage_create",
            EventType::StorageDelete => "storage_delete",
            EventType::NetworkCreate => "network_create",
            EventType::NetworkDelete => "network_delete",
            EventType::UserLogin => "user_login",
            EventType::UserLogout => "user_logout",
            EventType::SystemStart => "system_start",
            EventType::SystemShutdown => "system_shutdown",
            EventType::ConfigChange => "config_change",
            EventType::SecurityAlert => "security_alert",
        };
        write!(f, "{}", s)
    }
}

/// Event severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSeverity {
    Info,
    Warning,
    Error,
    Critical,
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
    events: Vec<SystemEvent>,
    templates: HashMap<String, VmTemplate>,
    backups: HashMap<String, BackupRecord>,
    backup_schedules: HashMap<String, BackupSchedule>,
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
            events: RwLock::new(VecDeque::with_capacity(MAX_EVENTS)),
            templates: RwLock::new(HashMap::new()),
            backups: RwLock::new(HashMap::new()),
            backup_schedules: RwLock::new(HashMap::new()),
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
                if let Ok(mut state) = serde_json::from_str::<PersistedState>(&content) {
                    // Reset all "running" VMs to "stopped" on server restart
                    // because they are not actually running anymore
                    for vm in state.vms.values_mut() {
                        if vm.status == VmStatus::Running {
                            log::info!("VM '{}' was marked running, resetting to stopped (server restarted)", vm.name);
                            vm.status = VmStatus::Stopped;
                            vm.started_at = None;
                        }
                    }
                    
                    *self.vms.write() = state.vms;
                    *self.storage_pools.write() = state.storage_pools;
                    *self.networks.write() = state.networks;
                    *self.events.write() = state.events.into_iter().collect();
                    *self.templates.write() = state.templates;
                    *self.backups.write() = state.backups;
                    *self.backup_schedules.write() = state.backup_schedules;
                    log::info!("Loaded {} VMs from state file", self.vms.read().len());
                    
                    // Save immediately to persist the status reset
                    let _ = self.save_state_internal();
                    return;
                }
            }
        }
        log::info!("Starting with empty VM state");
        
        // Log system start event
        self.log_event(EventType::SystemStart, EventSeverity::Info, "system", 
            "NVM Enterprise Platform started", None, None);
    }
    
    /// Internal save (used during load when we already hold data)
    fn save_state_internal(&self) -> std::io::Result<()> {
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let state = PersistedState {
            vms: self.vms.read().clone(),
            storage_pools: self.storage_pools.read().clone(),
            networks: self.networks.read().clone(),
            events: self.events.read().iter().cloned().collect(),
            templates: self.templates.read().clone(),
            backups: self.backups.read().clone(),
            backup_schedules: self.backup_schedules.read().clone(),
            version: 1,
        };
        
        let content = serde_json::to_string_pretty(&state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&self.state_file, content)?;
        Ok(())
    }
    
    /// Save state to disk
    pub fn save_state(&self) -> std::io::Result<()> {
        self.save_state_internal()
    }
    
    // ========== Event Logging ==========
    
    /// Log a system event
    pub fn log_event(
        &self,
        event_type: EventType,
        severity: EventSeverity,
        source: &str,
        message: &str,
        details: Option<serde_json::Value>,
        user: Option<String>,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        
        let event = SystemEvent {
            id: format!("evt-{:08x}", rand::random::<u32>()),
            timestamp: now,
            event_type,
            severity,
            source: source.to_string(),
            message: message.to_string(),
            details,
            user,
        };
        
        let mut events = self.events.write();
        if events.len() >= MAX_EVENTS {
            events.pop_front();
        }
        events.push_back(event);
    }
    
    /// Get recent events (newest first)
    pub fn get_events(&self, limit: usize) -> Vec<SystemEvent> {
        self.events.read()
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Get events filtered by type
    pub fn get_events_by_type(&self, event_type: EventType, limit: usize) -> Vec<SystemEvent> {
        self.events.read()
            .iter()
            .rev()
            .filter(|e| e.event_type == event_type)
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Get events since timestamp
    pub fn get_events_since(&self, since: u64) -> Vec<SystemEvent> {
        self.events.read()
            .iter()
            .filter(|e| e.timestamp >= since)
            .cloned()
            .collect()
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
        let name = vm.name.clone();
        vms.insert(id.clone(), vm);
        drop(vms);
        
        // Log event
        self.log_event(
            EventType::VmCreate,
            EventSeverity::Info,
            &name,
            &format!("VM '{}' created with ID {}", name, id),
            Some(serde_json::json!({"vm_id": id})),
            None,
        );
        
        let _ = self.save_state();
        Ok(id)
    }
    
    /// Update VM status
    pub fn set_vm_status(&self, id: &str, status: VmStatus) -> Result<(), String> {
        let mut vms = self.vms.write();
        if let Some(vm) = vms.get_mut(id) {
            let old_status = vm.status;
            let vm_name = vm.name.clone();
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
            
            // Log appropriate event based on status change
            let (event_type, message) = match status {
                VmStatus::Running => (EventType::VmStart, format!("VM '{}' started", vm_name)),
                VmStatus::Stopped => (EventType::VmStop, format!("VM '{}' stopped", vm_name)),
                VmStatus::Error => (EventType::VmError, format!("VM '{}' entered error state", vm_name)),
                _ => (EventType::VmStop, format!("VM '{}' status changed to {:?}", vm_name, status)),
            };
            
            let severity = if status == VmStatus::Error {
                EventSeverity::Error
            } else {
                EventSeverity::Info
            };
            
            self.log_event(
                event_type,
                severity,
                &vm_name,
                &message,
                Some(serde_json::json!({
                    "vm_id": id,
                    "old_status": format!("{:?}", old_status),
                    "new_status": format!("{:?}", status),
                })),
                None,
            );
            
            let _ = self.save_state();
            Ok(())
        } else {
            Err(format!("VM '{}' not found", id))
        }
    }
    
    /// Delete a VM with options for cleaning up associated resources
    /// 
    /// # Arguments
    /// * `id` - VM ID to delete
    /// * `delete_disks` - Whether to delete associated disk files
    /// * `delete_backups` - Whether to delete associated backups
    /// 
    /// Returns the deleted VM state and a summary of cleanup operations
    pub fn delete_vm_with_cleanup(
        &self, 
        id: &str, 
        delete_disks: bool, 
        delete_backups: bool
    ) -> Result<(VmState, DeleteCleanupResult), String> {
        let mut cleanup_result = DeleteCleanupResult::default();
        
        // Get VM info first
        let vm = {
            let vms = self.vms.read();
            match vms.get(id) {
                Some(vm) => vm.clone(),
                None => return Err(format!("VM '{}' not found", id)),
            }
        };
        
        // Collect backups for this VM before deletion
        if delete_backups {
            let backups: Vec<BackupRecord> = self.backups.read()
                .values()
                .filter(|b| b.vm_id == id)
                .cloned()
                .collect();
            
            // Delete backup files and records
            for backup in &backups {
                // Delete backup file
                if backup.target_path.exists() {
                    match std::fs::remove_file(&backup.target_path) {
                        Ok(_) => {
                            log::info!("Deleted backup file: {:?}", backup.target_path);
                            cleanup_result.backups_deleted += 1;
                        }
                        Err(e) => {
                            log::warn!("Failed to delete backup file {:?}: {}", backup.target_path, e);
                            cleanup_result.backup_errors.push(format!(
                                "Failed to delete {}: {}", 
                                backup.target_path.display(), 
                                e
                            ));
                        }
                    }
                }
                
                // Also try to delete backup directory if it exists
                if let Some(parent) = backup.target_path.parent() {
                    // Only remove if it's a VM-specific backup directory and empty
                    if parent.exists() && parent.to_string_lossy().contains(&backup.vm_id) {
                        if let Ok(entries) = std::fs::read_dir(parent) {
                            if entries.count() == 0 {
                                let _ = std::fs::remove_dir(parent);
                            }
                        }
                    }
                }
            }
            
            // Remove backup records from state
            {
                let mut backups_write = self.backups.write();
                let backup_ids: Vec<String> = backups.iter().map(|b| b.id.clone()).collect();
                for backup_id in backup_ids {
                    backups_write.remove(&backup_id);
                }
            }
            
            // Also clean up backup schedules that only reference this VM
            {
                let mut schedules = self.backup_schedules.write();
                let schedule_ids_to_remove: Vec<String> = schedules
                    .iter()
                    .filter(|(_, s)| s.vm_ids.len() == 1 && s.vm_ids[0] == id)
                    .map(|(id, _)| id.clone())
                    .collect();
                
                for schedule_id in schedule_ids_to_remove {
                    schedules.remove(&schedule_id);
                    cleanup_result.schedules_deleted += 1;
                }
                
                // Remove this VM from multi-VM schedules
                for schedule in schedules.values_mut() {
                    schedule.vm_ids.retain(|vm_id| vm_id != id);
                }
            }
        }
        
        // Delete disk files
        if delete_disks {
            for disk_path in &vm.disk_paths {
                if disk_path.exists() {
                    match std::fs::remove_file(disk_path) {
                        Ok(_) => {
                            log::info!("Deleted disk file: {:?}", disk_path);
                            cleanup_result.disks_deleted += 1;
                        }
                        Err(e) => {
                            log::warn!("Failed to delete disk file {:?}: {}", disk_path, e);
                            cleanup_result.disk_errors.push(format!(
                                "Failed to delete {}: {}", 
                                disk_path.display(), 
                                e
                            ));
                        }
                    }
                }
            }
            
            // Also delete VM config file if exists
            if let Some(ref config_path) = vm.config_path {
                if config_path.exists() {
                    match std::fs::remove_file(config_path) {
                        Ok(_) => {
                            log::info!("Deleted config file: {:?}", config_path);
                            cleanup_result.config_deleted = true;
                        }
                        Err(e) => {
                            log::warn!("Failed to delete config file {:?}: {}", config_path, e);
                        }
                    }
                }
            }
        }
        
        // Remove VM from state
        let vm_name = vm.name.clone();
        {
            let mut vms = self.vms.write();
            vms.remove(id);
        }
        
        // Log deletion event with cleanup details
        self.log_event(
            EventType::VmDelete,
            EventSeverity::Warning,
            &vm_name,
            &format!("VM '{}' (ID: {}) deleted with cleanup", vm_name, id),
            Some(serde_json::json!({
                "vm_id": id, 
                "vm_name": vm_name,
                "disks_deleted": cleanup_result.disks_deleted,
                "backups_deleted": cleanup_result.backups_deleted,
                "delete_disks_requested": delete_disks,
                "delete_backups_requested": delete_backups,
            })),
            None,
        );
        
        let _ = self.save_state();
        Ok((vm, cleanup_result))
    }
    
    /// Delete a VM (basic, no resource cleanup - for backward compatibility)
    pub fn delete_vm(&self, id: &str) -> Result<VmState, String> {
        let mut vms = self.vms.write();
        if let Some(vm) = vms.remove(id) {
            let vm_name = vm.name.clone();
            drop(vms);
            
            // Log deletion event
            self.log_event(
                EventType::VmDelete,
                EventSeverity::Warning,
                &vm_name,
                &format!("VM '{}' (ID: {}) deleted", vm_name, id),
                Some(serde_json::json!({"vm_id": id, "vm_name": vm_name})),
                None,
            );
            
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
    
    // ========== Template Operations ==========
    
    /// List all templates
    pub fn list_templates(&self) -> Vec<VmTemplate> {
        self.templates.read().values().cloned().collect()
    }
    
    /// Get a template by ID
    pub fn get_template(&self, id: &str) -> Option<VmTemplate> {
        self.templates.read().get(id).cloned()
    }
    
    /// Create a template
    pub fn create_template(&self, mut template: VmTemplate) -> Result<String, String> {
        let mut templates = self.templates.write();
        
        if template.id.is_empty() {
            template.id = format!("tpl-{:06x}", rand::random::<u32>() & 0xffffff);
        }
        
        if templates.values().any(|t| t.name == template.name) {
            return Err(format!("Template with name '{}' already exists", template.name));
        }
        
        let id = template.id.clone();
        templates.insert(id.clone(), template);
        drop(templates);
        let _ = self.save_state();
        Ok(id)
    }
    
    /// Delete a template
    pub fn delete_template(&self, id: &str) -> Result<VmTemplate, String> {
        let mut templates = self.templates.write();
        if let Some(template) = templates.remove(id) {
            drop(templates);
            let _ = self.save_state();
            Ok(template)
        } else {
            Err(format!("Template '{}' not found", id))
        }
    }
    
    // ========== Backup Operations ==========
    
    /// List all backup records
    pub fn list_backups(&self) -> Vec<BackupRecord> {
        self.backups.read().values().cloned().collect()
    }
    
    /// Get backup by ID
    pub fn get_backup(&self, id: &str) -> Option<BackupRecord> {
        self.backups.read().get(id).cloned()
    }
    
    /// Create a backup record
    pub fn create_backup(&self, mut backup: BackupRecord) -> Result<String, String> {
        let mut backups = self.backups.write();
        
        if backup.id.is_empty() {
            backup.id = format!("backup-{:06x}", rand::random::<u32>() & 0xffffff);
        }
        
        let id = backup.id.clone();
        backups.insert(id.clone(), backup);
        drop(backups);
        let _ = self.save_state();
        Ok(id)
    }
    
    /// Update backup status
    pub fn update_backup(&self, id: &str, status: BackupStatus, progress: f64, size: Option<u64>) -> Result<(), String> {
        let mut backups = self.backups.write();
        if let Some(backup) = backups.get_mut(id) {
            backup.status = status;
            backup.progress = progress;
            if let Some(s) = size {
                backup.size_bytes = s;
            }
            if status == BackupStatus::Completed || status == BackupStatus::Failed {
                backup.finished_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                );
            }
            drop(backups);
            let _ = self.save_state();
            Ok(())
        } else {
            Err(format!("Backup '{}' not found", id))
        }
    }
    
    /// List backup schedules
    pub fn list_backup_schedules(&self) -> Vec<BackupSchedule> {
        self.backup_schedules.read().values().cloned().collect()
    }
    
    /// Create a backup schedule
    pub fn create_backup_schedule(&self, mut schedule: BackupSchedule) -> Result<String, String> {
        let mut schedules = self.backup_schedules.write();
        
        if schedule.id.is_empty() {
            schedule.id = format!("sched-{:06x}", rand::random::<u32>() & 0xffffff);
        }
        
        if schedules.values().any(|s| s.name == schedule.name) {
            return Err(format!("Schedule with name '{}' already exists", schedule.name));
        }
        
        let id = schedule.id.clone();
        schedules.insert(id.clone(), schedule);
        drop(schedules);
        let _ = self.save_state();
        Ok(id)
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
