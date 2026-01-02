//! Virtual Machine Manager
//!
//! This module provides high-level VM management capabilities similar to
//! libvirt, proxmox, or vCenter. It orchestrates all hypervisor components
//! to provide a unified VM management interface.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, Ordering}};
use std::time::{Duration, Instant};

use super::core::{VmId, VmStatus, VmSpec, VmInstance, VmInfo, Hypervisor, HypervisorError, HypervisorResult};
use super::resources::{CpuPool, MemoryPool, StoragePool, NetworkPool};
use super::memory::{MemoryManager, BalloonManager, KsmManager, NumaManager};
use super::storage::{StorageManager, VirtualDisk, DiskFormat};
use super::network::{NetworkManager, VirtualSwitch, VirtualNic};
use super::scheduler::{VmScheduler, SchedulerPolicy};
use super::cluster::{ClusterManager, ClusterHost};
use super::security::{SecurityManager, SecurityPolicy};
use super::live_migration::{MigrationManager, MigrationType};
use super::api::{ApiServer, ApiConfig};

// ============================================================================
// VM Manager Configuration
// ============================================================================

/// VM Manager configuration
#[derive(Debug, Clone)]
pub struct VmManagerConfig {
    /// Maximum number of VMs
    pub max_vms: u32,
    /// Maximum VCPUs per VM
    pub max_vcpus_per_vm: u32,
    /// Maximum memory per VM (bytes)
    pub max_memory_per_vm: u64,
    /// Default CPU overcommit ratio
    pub default_cpu_overcommit: f64,
    /// Default memory overcommit ratio
    pub default_memory_overcommit: f64,
    /// Enable live migration
    pub live_migration_enabled: bool,
    /// Enable snapshots
    pub snapshots_enabled: bool,
    /// Enable hot plug
    pub hot_plug_enabled: bool,
    /// Enable KSM (Kernel Same-page Merging)
    pub ksm_enabled: bool,
    /// Enable memory ballooning
    pub ballooning_enabled: bool,
    /// Enable NUMA awareness
    pub numa_enabled: bool,
    /// Enable high availability
    pub ha_enabled: bool,
    /// Enable fault tolerance
    pub ft_enabled: bool,
    /// Enable distributed resource scheduling
    pub drs_enabled: bool,
    /// API configuration
    pub api_config: Option<ApiConfig>,
    /// Storage paths
    pub storage_paths: Vec<String>,
    /// ISO paths
    pub iso_paths: Vec<String>,
    /// Template paths
    pub template_paths: Vec<String>,
    /// Log path
    pub log_path: Option<String>,
}

impl Default for VmManagerConfig {
    fn default() -> Self {
        Self {
            max_vms: 256,
            max_vcpus_per_vm: 128,
            max_memory_per_vm: 1024 * 1024 * 1024 * 1024, // 1TB
            default_cpu_overcommit: 4.0,
            default_memory_overcommit: 1.5,
            live_migration_enabled: true,
            snapshots_enabled: true,
            hot_plug_enabled: true,
            ksm_enabled: true,
            ballooning_enabled: true,
            numa_enabled: true,
            ha_enabled: true,
            ft_enabled: false,
            drs_enabled: true,
            api_config: None,
            storage_paths: vec!["/var/lib/hypervisor/storage".to_string()],
            iso_paths: vec!["/var/lib/hypervisor/iso".to_string()],
            template_paths: vec!["/var/lib/hypervisor/templates".to_string()],
            log_path: Some("/var/log/hypervisor".to_string()),
        }
    }
}

// ============================================================================
// VM Manager
// ============================================================================

/// High-level VM Manager
pub struct VmManager {
    /// Configuration
    config: RwLock<VmManagerConfig>,
    /// Hypervisor instance
    hypervisor: Arc<Hypervisor>,
    /// Storage manager
    storage_manager: Arc<StorageManager>,
    /// Network manager
    network_manager: Arc<NetworkManager>,
    /// Memory manager
    memory_manager: Arc<MemoryManager>,
    /// Scheduler
    scheduler: Arc<VmScheduler>,
    /// Migration manager
    migration_manager: Arc<MigrationManager>,
    /// Cluster manager
    cluster_manager: Option<Arc<ClusterManager>>,
    /// Security manager
    security_manager: Arc<SecurityManager>,
    /// API server
    api_server: Option<Arc<ApiServer>>,
    /// VM metadata
    vm_metadata: RwLock<HashMap<VmId, VmMetadata>>,
    /// Templates
    templates: RwLock<HashMap<String, VmTemplate>>,
    /// Tags
    tags: RwLock<HashMap<String, Vec<VmId>>>,
    /// Statistics
    stats: RwLock<ManagerStats>,
    /// Event subscribers
    subscribers: RwLock<Vec<Box<dyn EventSubscriber>>>,
    /// Manager state
    state: RwLock<ManagerState>,
}

/// Manager state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerState {
    Initializing,
    Running,
    Maintenance,
    Shutdown,
}

/// Manager statistics
#[derive(Debug, Clone, Default)]
pub struct ManagerStats {
    pub total_vms: u32,
    pub running_vms: u32,
    pub stopped_vms: u32,
    pub paused_vms: u32,
    pub total_vcpus: u32,
    pub allocated_vcpus: u32,
    pub total_memory: u64,
    pub allocated_memory: u64,
    pub total_storage: u64,
    pub used_storage: u64,
    pub migrations_completed: u64,
    pub snapshots_created: u64,
    pub errors: u64,
    pub uptime: Duration,
}

/// VM metadata
#[derive(Debug, Clone)]
pub struct VmMetadata {
    pub id: VmId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub annotations: HashMap<String, String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub template_id: Option<String>,
}

/// VM template
#[derive(Debug, Clone)]
pub struct VmTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub spec: VmSpec,
    pub os_type: OsType,
    pub version: String,
    pub created_at: Instant,
}

/// OS type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsType {
    Linux,
    Windows,
    FreeBSD,
    Other(String),
}

impl VmManager {
    /// Create new VM manager
    pub fn new(config: VmManagerConfig) -> Self {
        let hypervisor = Hypervisor::new();
        let memory_pool = Arc::new(MemoryPool::new(config.max_memory_per_vm / (1024 * 1024)));
        let storage_manager = Arc::new(StorageManager::new());
        let network_manager = Arc::new(NetworkManager::new());
        let memory_manager = Arc::new(MemoryManager::new(memory_pool, config.ksm_enabled));
        let scheduler = Arc::new(VmScheduler::new());
        let migration_manager = Arc::new(MigrationManager::new());
        let security_manager = Arc::new(SecurityManager::new());
        
        let api_server = config.api_config.as_ref().map(|api_config| {
            let server = Arc::new(ApiServer::new());
            server.configure(api_config.clone());
            server
        });
        
        Self {
            config: RwLock::new(config),
            hypervisor,
            storage_manager,
            network_manager,
            memory_manager,
            scheduler,
            migration_manager,
            cluster_manager: None,
            security_manager,
            api_server,
            vm_metadata: RwLock::new(HashMap::new()),
            templates: RwLock::new(HashMap::new()),
            tags: RwLock::new(HashMap::new()),
            stats: RwLock::new(ManagerStats::default()),
            subscribers: RwLock::new(Vec::new()),
            state: RwLock::new(ManagerState::Initializing),
        }
    }
    
    /// Initialize manager
    pub fn initialize(&self) -> HypervisorResult<()> {
        *self.state.write().unwrap() = ManagerState::Running;
        self.emit_event(ManagerEvent::Started);
        Ok(())
    }
    
    /// Shutdown manager
    pub fn shutdown(&self) -> HypervisorResult<()> {
        *self.state.write().unwrap() = ManagerState::Shutdown;
        self.emit_event(ManagerEvent::Shutdown);
        Ok(())
    }
    
    /// Get current state
    pub fn state(&self) -> ManagerState {
        *self.state.read().unwrap()
    }
    
    // ========================================================================
    // VM Lifecycle Management
    // ========================================================================
    
    /// Create a new VM
    pub fn create_vm(&self, spec: VmSpec) -> HypervisorResult<VmId> {
        let config = self.config.read().unwrap();
        
        // Validate resource limits
        if spec.vcpus > config.max_vcpus_per_vm {
            return Err(HypervisorError::ResourceLimit(
                format!("VCPUs exceed limit: {} > {}", spec.vcpus, config.max_vcpus_per_vm)
            ));
        }
        
        if spec.memory_mb as u64 * 1024 * 1024 > config.max_memory_per_vm {
            return Err(HypervisorError::ResourceLimit(
                format!("Memory exceeds limit")
            ));
        }
        
        // Check total VM count
        let stats = self.stats.read().unwrap();
        if stats.total_vms >= config.max_vms {
            return Err(HypervisorError::ResourceLimit(
                format!("Maximum VM count reached: {}", config.max_vms)
            ));
        }
        drop(config);
        drop(stats);
        
        // Create VM
        let vm_id = self.hypervisor.create_vm(spec.clone())?;
        
        // Create metadata
        let metadata = VmMetadata {
            id: vm_id,
            name: spec.name.clone(),
            description: String::new(),
            tags: Vec::new(),
            created_at: Instant::now(),
            updated_at: Instant::now(),
            annotations: HashMap::new(),
            owner: None,
            group: None,
            template_id: None,
        };
        
        self.vm_metadata.write().unwrap().insert(vm_id, metadata);
        
        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.total_vms += 1;
        stats.stopped_vms += 1;
        stats.allocated_vcpus += spec.vcpus;
        stats.allocated_memory += spec.memory_mb as u64 * 1024 * 1024;
        
        self.emit_event(ManagerEvent::VmCreated(vm_id));
        
        Ok(vm_id)
    }
    
    /// Create VM from template
    pub fn create_vm_from_template(&self, template_id: &str, name: &str) -> HypervisorResult<VmId> {
        let templates = self.templates.read().unwrap();
        
        let template = templates.get(template_id)
            .ok_or_else(|| HypervisorError::NotFound(format!("Template not found: {}", template_id)))?;
        
        let mut spec = template.spec.clone();
        spec.name = name.to_string();
        
        drop(templates);
        
        let vm_id = self.create_vm(spec)?;
        
        // Set template reference
        if let Some(metadata) = self.vm_metadata.write().unwrap().get_mut(&vm_id) {
            metadata.template_id = Some(template_id.to_string());
        }
        
        Ok(vm_id)
    }
    
    /// Delete a VM
    pub fn delete_vm(&self, vm_id: VmId) -> HypervisorResult<()> {
        // Get VM info before deletion
        let status = self.hypervisor.vm_status(vm_id)?;
        
        // Cannot delete running VM
        if status == VmStatus::Running {
            return Err(HypervisorError::InvalidState(
                "Cannot delete running VM".to_string()
            ));
        }
        
        // Delete VM
        self.hypervisor.destroy_vm(vm_id)?;
        
        // Remove metadata
        self.vm_metadata.write().unwrap().remove(&vm_id);
        
        // Remove from tags
        let mut tags = self.tags.write().unwrap();
        for vm_ids in tags.values_mut() {
            vm_ids.retain(|id| *id != vm_id);
        }
        
        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.total_vms = stats.total_vms.saturating_sub(1);
        stats.stopped_vms = stats.stopped_vms.saturating_sub(1);
        
        self.emit_event(ManagerEvent::VmDeleted(vm_id));
        
        Ok(())
    }
    
    /// Start a VM
    pub fn start_vm(&self, vm_id: VmId) -> HypervisorResult<()> {
        self.hypervisor.start_vm(vm_id)?;
        
        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.stopped_vms = stats.stopped_vms.saturating_sub(1);
        stats.running_vms += 1;
        
        self.emit_event(ManagerEvent::VmStarted(vm_id));
        
        Ok(())
    }
    
    /// Stop a VM
    pub fn stop_vm(&self, vm_id: VmId, _force: bool) -> HypervisorResult<()> {
        self.hypervisor.stop_vm(vm_id)?;
        
        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.running_vms = stats.running_vms.saturating_sub(1);
        stats.stopped_vms += 1;
        
        self.emit_event(ManagerEvent::VmStopped(vm_id));
        
        Ok(())
    }
    
    /// Pause a VM
    pub fn pause_vm(&self, vm_id: VmId) -> HypervisorResult<()> {
        self.hypervisor.pause_vm(vm_id)?;
        
        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.running_vms = stats.running_vms.saturating_sub(1);
        stats.paused_vms += 1;
        
        self.emit_event(ManagerEvent::VmPaused(vm_id));
        
        Ok(())
    }
    
    /// Resume a VM
    pub fn resume_vm(&self, vm_id: VmId) -> HypervisorResult<()> {
        self.hypervisor.resume_vm(vm_id)?;
        
        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.paused_vms = stats.paused_vms.saturating_sub(1);
        stats.running_vms += 1;
        
        self.emit_event(ManagerEvent::VmResumed(vm_id));
        
        Ok(())
    }
    
    /// Reboot a VM
    pub fn reboot_vm(&self, vm_id: VmId, _force: bool) -> HypervisorResult<()> {
        self.hypervisor.reset_vm(vm_id)?;
        
        self.emit_event(ManagerEvent::VmRebooted(vm_id));
        
        Ok(())
    }
    
    // ========================================================================
    // VM Information
    // ========================================================================
    
    /// Get VM info
    pub fn get_vm_info(&self, vm_id: VmId) -> HypervisorResult<VmInfo> {
        self.hypervisor.vm_info(vm_id)
    }
    
    /// List all VMs
    pub fn list_vms(&self) -> Vec<VmInfo> {
        self.hypervisor.list_vms()
    }
    
    /// List VM IDs
    pub fn list_vm_ids(&self) -> Vec<VmId> {
        self.hypervisor.list_vms().into_iter().map(|info| info.id).collect()
    }
    
    /// Get VM metadata
    pub fn get_vm_metadata(&self, vm_id: VmId) -> Option<VmMetadata> {
        self.vm_metadata.read().unwrap().get(&vm_id).cloned()
    }
    
    /// Update VM metadata
    pub fn update_vm_metadata(&self, vm_id: VmId, metadata: VmMetadata) -> HypervisorResult<()> {
        let mut map = self.vm_metadata.write().unwrap();
        
        if !map.contains_key(&vm_id) {
            return Err(HypervisorError::NotFound(format!("VM not found: {:?}", vm_id)));
        }
        
        map.insert(vm_id, metadata);
        
        Ok(())
    }
    
    /// Get VMs by tag
    pub fn get_vms_by_tag(&self, tag: &str) -> Vec<VmId> {
        self.tags.read().unwrap()
            .get(tag)
            .cloned()
            .unwrap_or_default()
    }
    
    /// Add tag to VM
    pub fn add_vm_tag(&self, vm_id: VmId, tag: &str) -> HypervisorResult<()> {
        // Update metadata
        if let Some(metadata) = self.vm_metadata.write().unwrap().get_mut(&vm_id) {
            if !metadata.tags.contains(&tag.to_string()) {
                metadata.tags.push(tag.to_string());
            }
        }
        
        // Update tags index
        self.tags.write().unwrap()
            .entry(tag.to_string())
            .or_default()
            .push(vm_id);
        
        Ok(())
    }
    
    /// Remove tag from VM
    pub fn remove_vm_tag(&self, vm_id: VmId, tag: &str) -> HypervisorResult<()> {
        // Update metadata
        if let Some(metadata) = self.vm_metadata.write().unwrap().get_mut(&vm_id) {
            metadata.tags.retain(|t| t != tag);
        }
        
        // Update tags index
        if let Some(vms) = self.tags.write().unwrap().get_mut(tag) {
            vms.retain(|id| *id != vm_id);
        }
        
        Ok(())
    }
    
    // ========================================================================
    // Template Management
    // ========================================================================
    
    /// Create template
    pub fn create_template(&self, template: VmTemplate) -> HypervisorResult<String> {
        let id = template.id.clone();
        
        self.templates.write().unwrap().insert(id.clone(), template);
        
        Ok(id)
    }
    
    /// Get template
    pub fn get_template(&self, id: &str) -> Option<VmTemplate> {
        self.templates.read().unwrap().get(id).cloned()
    }
    
    /// List templates
    pub fn list_templates(&self) -> Vec<VmTemplate> {
        self.templates.read().unwrap().values().cloned().collect()
    }
    
    /// Delete template
    pub fn delete_template(&self, id: &str) -> HypervisorResult<()> {
        self.templates.write().unwrap().remove(id);
        Ok(())
    }
    
    /// Convert VM to template
    pub fn convert_to_template(&self, vm_id: VmId, name: &str) -> HypervisorResult<String> {
        let info = self.hypervisor.vm_info(vm_id)?;
        
        let template = VmTemplate {
            id: format!("tmpl-{}", uuid_simple()),
            name: name.to_string(),
            description: String::new(),
            spec: VmSpec::builder()
                .name(&info.name)
                .vcpus(info.vcpus)
                .memory_mb(info.memory_mb)
                .build(),
            os_type: OsType::Linux,
            version: "1.0".to_string(),
            created_at: Instant::now(),
        };
        
        self.create_template(template)
    }
    
    // ========================================================================
    // Snapshot Management
    // ========================================================================
    
    /// Create snapshot
    pub fn create_snapshot(&self, vm_id: VmId, name: &str) -> HypervisorResult<String> {
        let config = self.config.read().unwrap();
        
        if !config.snapshots_enabled {
            return Err(HypervisorError::NotSupported("Snapshots disabled".to_string()));
        }
        
        drop(config);
        
        // Create snapshot (simplified)
        let snapshot_id = format!("snap-{}", uuid_simple());
        
        self.stats.write().unwrap().snapshots_created += 1;
        
        self.emit_event(ManagerEvent::SnapshotCreated(vm_id, snapshot_id.clone()));
        
        Ok(snapshot_id)
    }
    
    /// Restore snapshot
    pub fn restore_snapshot(&self, vm_id: VmId, snapshot_id: &str) -> HypervisorResult<()> {
        let status = self.hypervisor.vm_status(vm_id)?;
        
        // Cannot restore running VM
        if status == VmStatus::Running {
            return Err(HypervisorError::InvalidState(
                "Cannot restore snapshot on running VM".to_string()
            ));
        }
        
        self.emit_event(ManagerEvent::SnapshotRestored(vm_id, snapshot_id.to_string()));
        
        Ok(())
    }
    
    /// Delete snapshot
    pub fn delete_snapshot(&self, vm_id: VmId, snapshot_id: &str) -> HypervisorResult<()> {
        self.emit_event(ManagerEvent::SnapshotDeleted(vm_id, snapshot_id.to_string()));
        Ok(())
    }
    
    // ========================================================================
    // Live Migration
    // ========================================================================
    
    /// Migrate VM to another host
    pub fn migrate_vm(&self, vm_id: VmId, target_host: &str, migration_type: MigrationType) -> HypervisorResult<String> {
        let config = self.config.read().unwrap();
        
        if !config.live_migration_enabled {
            return Err(HypervisorError::NotSupported("Live migration disabled".to_string()));
        }
        
        drop(config);
        
        // Create migration options based on type
        let options = match migration_type {
            MigrationType::PreCopy => super::live_migration::MigrationOptions::live(),
            MigrationType::PostCopy => super::live_migration::MigrationOptions::postcopy(),
            MigrationType::Hybrid => super::live_migration::MigrationOptions::live(),
            MigrationType::Offline => super::live_migration::MigrationOptions::offline(),
        };
        
        let migration_id = self.migration_manager.start_migration(
            vm_id,
            "localhost", // source host
            target_host,
            options,
        )?;
        
        self.stats.write().unwrap().migrations_completed += 1;
        
        self.emit_event(ManagerEvent::MigrationStarted(vm_id, target_host.to_string()));
        
        Ok(migration_id.to_string())
    }
    
    /// Cancel migration
    pub fn cancel_migration(&self, migration_id: &str) -> HypervisorResult<()> {
        // Parse migration ID
        let id = migration_id.parse::<u64>()
            .map_err(|_| HypervisorError::InvalidArgument("Invalid migration ID".to_string()))?;
        
        self.migration_manager.cancel_migration(super::live_migration::MigrationId::new(id))
    }
    
    // ========================================================================
    // Resource Management
    // ========================================================================
    
    /// Resize VM CPU
    pub fn resize_vm_cpu(&self, vm_id: VmId, vcpus: u32) -> HypervisorResult<()> {
        let config = self.config.read().unwrap();
        
        if !config.hot_plug_enabled {
            return Err(HypervisorError::NotSupported("Hot plug disabled".to_string()));
        }
        
        if vcpus > config.max_vcpus_per_vm {
            return Err(HypervisorError::ResourceLimit(
                format!("VCPUs exceed limit: {} > {}", vcpus, config.max_vcpus_per_vm)
            ));
        }
        
        drop(config);
        
        // Hot-add/remove CPUs (simplified)
        self.emit_event(ManagerEvent::VmResized(vm_id));
        
        Ok(())
    }
    
    /// Resize VM memory
    pub fn resize_vm_memory(&self, vm_id: VmId, memory_mb: u32) -> HypervisorResult<()> {
        let config = self.config.read().unwrap();
        
        if !config.hot_plug_enabled {
            return Err(HypervisorError::NotSupported("Hot plug disabled".to_string()));
        }
        
        if memory_mb as u64 * 1024 * 1024 > config.max_memory_per_vm {
            return Err(HypervisorError::ResourceLimit("Memory exceeds limit".to_string()));
        }
        
        drop(config);
        
        // Hot-add/remove memory (simplified)
        self.emit_event(ManagerEvent::VmResized(vm_id));
        
        Ok(())
    }
    
    // ========================================================================
    // Statistics
    // ========================================================================
    
    /// Get manager statistics
    pub fn stats(&self) -> ManagerStats {
        self.stats.read().unwrap().clone()
    }
    
    /// Update statistics
    pub fn update_stats(&self) {
        let vms = self.hypervisor.list_vms();
        
        let mut running = 0;
        let mut stopped = 0;
        let mut paused = 0;
        
        for vm_info in &vms {
            match vm_info.status {
                VmStatus::Running => running += 1,
                VmStatus::Stopped => stopped += 1,
                VmStatus::Paused => paused += 1,
                _ => {}
            }
        }
        
        let mut stats = self.stats.write().unwrap();
        stats.total_vms = vms.len() as u32;
        stats.running_vms = running;
        stats.stopped_vms = stopped;
        stats.paused_vms = paused;
    }
    
    // ========================================================================
    // Event Handling
    // ========================================================================
    
    /// Subscribe to events
    pub fn subscribe(&self, subscriber: Box<dyn EventSubscriber>) {
        self.subscribers.write().unwrap().push(subscriber);
    }
    
    /// Emit event
    fn emit_event(&self, event: ManagerEvent) {
        for subscriber in self.subscribers.read().unwrap().iter() {
            subscriber.on_event(&event);
        }
    }
    
    // ========================================================================
    // Accessors
    // ========================================================================
    
    pub fn hypervisor(&self) -> &Arc<Hypervisor> {
        &self.hypervisor
    }
    
    pub fn storage_manager(&self) -> &Arc<StorageManager> {
        &self.storage_manager
    }
    
    pub fn network_manager(&self) -> &Arc<NetworkManager> {
        &self.network_manager
    }
    
    pub fn memory_manager(&self) -> &Arc<MemoryManager> {
        &self.memory_manager
    }
    
    pub fn scheduler(&self) -> &Arc<VmScheduler> {
        &self.scheduler
    }
    
    pub fn migration_manager(&self) -> &Arc<MigrationManager> {
        &self.migration_manager
    }
    
    pub fn security_manager(&self) -> &Arc<SecurityManager> {
        &self.security_manager
    }
    
    pub fn api_server(&self) -> Option<&Arc<ApiServer>> {
        self.api_server.as_ref()
    }
}

// ============================================================================
// Events
// ============================================================================

/// Manager event
#[derive(Debug, Clone)]
pub enum ManagerEvent {
    Started,
    Shutdown,
    VmCreated(VmId),
    VmDeleted(VmId),
    VmStarted(VmId),
    VmStopped(VmId),
    VmPaused(VmId),
    VmResumed(VmId),
    VmRebooted(VmId),
    VmResized(VmId),
    SnapshotCreated(VmId, String),
    SnapshotRestored(VmId, String),
    SnapshotDeleted(VmId, String),
    MigrationStarted(VmId, String),
    MigrationCompleted(VmId, String),
    MigrationFailed(VmId, String),
    Error(String),
}

/// Event subscriber trait
pub trait EventSubscriber: Send + Sync {
    fn on_event(&self, event: &ManagerEvent);
}

// ============================================================================
// Utilities
// ============================================================================

/// Generate simple UUID
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    
    format!("{:016x}", now)
}

// ============================================================================
// Builder
// ============================================================================

/// VM Manager builder
pub struct VmManagerBuilder {
    config: VmManagerConfig,
}

impl VmManagerBuilder {
    pub fn new() -> Self {
        Self {
            config: VmManagerConfig::default(),
        }
    }
    
    pub fn max_vms(mut self, count: u32) -> Self {
        self.config.max_vms = count;
        self
    }
    
    pub fn max_vcpus_per_vm(mut self, count: u32) -> Self {
        self.config.max_vcpus_per_vm = count;
        self
    }
    
    pub fn max_memory_per_vm(mut self, bytes: u64) -> Self {
        self.config.max_memory_per_vm = bytes;
        self
    }
    
    pub fn cpu_overcommit(mut self, ratio: f64) -> Self {
        self.config.default_cpu_overcommit = ratio;
        self
    }
    
    pub fn memory_overcommit(mut self, ratio: f64) -> Self {
        self.config.default_memory_overcommit = ratio;
        self
    }
    
    pub fn enable_live_migration(mut self, enabled: bool) -> Self {
        self.config.live_migration_enabled = enabled;
        self
    }
    
    pub fn enable_snapshots(mut self, enabled: bool) -> Self {
        self.config.snapshots_enabled = enabled;
        self
    }
    
    pub fn enable_hot_plug(mut self, enabled: bool) -> Self {
        self.config.hot_plug_enabled = enabled;
        self
    }
    
    pub fn enable_ksm(mut self, enabled: bool) -> Self {
        self.config.ksm_enabled = enabled;
        self
    }
    
    pub fn enable_ballooning(mut self, enabled: bool) -> Self {
        self.config.ballooning_enabled = enabled;
        self
    }
    
    pub fn enable_numa(mut self, enabled: bool) -> Self {
        self.config.numa_enabled = enabled;
        self
    }
    
    pub fn enable_ha(mut self, enabled: bool) -> Self {
        self.config.ha_enabled = enabled;
        self
    }
    
    pub fn enable_ft(mut self, enabled: bool) -> Self {
        self.config.ft_enabled = enabled;
        self
    }
    
    pub fn enable_drs(mut self, enabled: bool) -> Self {
        self.config.drs_enabled = enabled;
        self
    }
    
    pub fn with_api(mut self, config: ApiConfig) -> Self {
        self.config.api_config = Some(config);
        self
    }
    
    pub fn storage_path(mut self, path: &str) -> Self {
        self.config.storage_paths.push(path.to_string());
        self
    }
    
    pub fn iso_path(mut self, path: &str) -> Self {
        self.config.iso_paths.push(path.to_string());
        self
    }
    
    pub fn template_path(mut self, path: &str) -> Self {
        self.config.template_paths.push(path.to_string());
        self
    }
    
    pub fn log_path(mut self, path: &str) -> Self {
        self.config.log_path = Some(path.to_string());
        self
    }
    
    pub fn build(self) -> VmManager {
        VmManager::new(self.config)
    }
}

impl Default for VmManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vm_manager_creation() {
        let manager = VmManagerBuilder::new()
            .max_vms(100)
            .enable_snapshots(true)
            .build();
        
        assert_eq!(manager.state(), ManagerState::Initializing);
        
        manager.initialize().unwrap();
        assert_eq!(manager.state(), ManagerState::Running);
    }
    
    #[test]
    fn test_vm_lifecycle() {
        let manager = VmManagerBuilder::new().build();
        manager.initialize().unwrap();
        
        // Create VM
        let spec = VmSpec::builder()
            .name("test-vm")
            .vcpus(2)
            .memory_mb(1024)
            .build();
        
        let vm_id = manager.create_vm(spec).unwrap();
        
        // Check stats
        let stats = manager.stats();
        assert_eq!(stats.total_vms, 1);
        assert_eq!(stats.stopped_vms, 1);
        
        // Start VM
        manager.start_vm(vm_id).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.running_vms, 1);
        assert_eq!(stats.stopped_vms, 0);
        
        // Pause VM
        manager.pause_vm(vm_id).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.paused_vms, 1);
        assert_eq!(stats.running_vms, 0);
        
        // Resume VM
        manager.resume_vm(vm_id).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.running_vms, 1);
        assert_eq!(stats.paused_vms, 0);
        
        // Stop VM
        manager.stop_vm(vm_id, false).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.stopped_vms, 1);
        assert_eq!(stats.running_vms, 0);
    }
    
    #[test]
    fn test_template_management() {
        let manager = VmManagerBuilder::new().build();
        manager.initialize().unwrap();
        
        // Create template
        let template = VmTemplate {
            id: "tmpl-1".to_string(),
            name: "Ubuntu 22.04".to_string(),
            description: "Ubuntu Server".to_string(),
            spec: VmSpec::builder()
                .name("ubuntu")
                .vcpus(4)
                .memory_mb(4096)
                .build(),
            os_type: OsType::Linux,
            version: "1.0".to_string(),
            created_at: Instant::now(),
        };
        
        manager.create_template(template).unwrap();
        
        // Get template
        let tmpl = manager.get_template("tmpl-1").unwrap();
        assert_eq!(tmpl.name, "Ubuntu 22.04");
        
        // List templates
        let templates = manager.list_templates();
        assert_eq!(templates.len(), 1);
        
        // Create VM from template
        let vm_id = manager.create_vm_from_template("tmpl-1", "my-ubuntu").unwrap();
        
        // Check VM metadata
        let metadata = manager.get_vm_metadata(vm_id).unwrap();
        assert_eq!(metadata.template_id, Some("tmpl-1".to_string()));
    }
    
    #[test]
    fn test_tagging() {
        let manager = VmManagerBuilder::new().build();
        manager.initialize().unwrap();
        
        // Create VM
        let spec = VmSpec::builder()
            .name("test-vm")
            .vcpus(2)
            .memory_mb(1024)
            .build();
        
        let vm_id = manager.create_vm(spec).unwrap();
        
        // Add tags
        manager.add_vm_tag(vm_id, "production").unwrap();
        manager.add_vm_tag(vm_id, "web").unwrap();
        
        // Get VMs by tag
        let vms = manager.get_vms_by_tag("production");
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0], vm_id);
        
        // Remove tag
        manager.remove_vm_tag(vm_id, "production").unwrap();
        
        let vms = manager.get_vms_by_tag("production");
        assert_eq!(vms.len(), 0);
    }
    
    #[test]
    fn test_resource_limits() {
        let manager = VmManagerBuilder::new()
            .max_vcpus_per_vm(8)
            .build();
        manager.initialize().unwrap();
        
        // Try to create VM exceeding CPU limit
        let spec = VmSpec::builder()
            .name("test-vm")
            .vcpus(16)
            .memory_mb(1024)
            .build();
        
        let result = manager.create_vm(spec);
        assert!(result.is_err());
    }
}
