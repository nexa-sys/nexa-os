//! Cluster Management and High Availability
//!
//! This module provides enterprise cluster features including:
//! - High Availability (HA) with automatic failover
//! - Fault Tolerance (FT) with VM mirroring
//! - Distributed Resource Scheduler (DRS)
//! - Host clustering and management
//! - Shared storage management

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Instant, Duration};

use super::core::{VmId, VmStatus, HypervisorError, HypervisorResult};
use super::scheduler::{LoadBalancer, MigrationRecommendation};

// ============================================================================
// Cluster Manager
// ============================================================================

/// Central cluster manager
pub struct ClusterManager {
    /// Cluster configuration
    config: RwLock<ClusterConfig>,
    /// Cluster hosts
    hosts: RwLock<HashMap<String, ClusterHost>>,
    /// VMs in cluster
    vms: RwLock<HashMap<VmId, ClusterVm>>,
    /// High availability manager
    ha_manager: RwLock<HaManager>,
    /// Fault tolerance manager
    ft_manager: RwLock<FtManager>,
    /// DRS manager
    drs_manager: RwLock<DrsManager>,
    /// Cluster events
    events: RwLock<VecDeque<ClusterEvent>>,
    /// Statistics
    stats: RwLock<ClusterStats>,
    /// Cluster state
    state: RwLock<ClusterState>,
}

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Cluster name
    pub name: String,
    /// Enable HA
    pub ha_enabled: bool,
    /// Enable FT
    pub ft_enabled: bool,
    /// Enable DRS
    pub drs_enabled: bool,
    /// DRS automation level
    pub drs_automation: DrsAutomation,
    /// Host failure response
    pub host_failure_response: HostFailureResponse,
    /// VM restart priority
    pub vm_restart_priority: RestartPriority,
    /// Admission control enabled
    pub admission_control: bool,
    /// Reserved capacity percent
    pub reserved_capacity_percent: u32,
    /// Heartbeat interval (seconds)
    pub heartbeat_interval: u64,
    /// Host failure timeout (seconds)
    pub host_failure_timeout: u64,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            name: "cluster".to_string(),
            ha_enabled: true,
            ft_enabled: false,
            drs_enabled: true,
            drs_automation: DrsAutomation::FullyAutomated,
            host_failure_response: HostFailureResponse::RestartVms,
            vm_restart_priority: RestartPriority::Medium,
            admission_control: true,
            reserved_capacity_percent: 25,
            heartbeat_interval: 10,
            host_failure_timeout: 60,
        }
    }
}

/// DRS automation level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrsAutomation {
    /// No automation - manual mode
    Manual,
    /// Suggest migrations only
    PartiallyAutomated,
    /// Automatic migration
    FullyAutomated,
}

/// Host failure response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFailureResponse {
    /// Leave VMs powered off
    LeaveOff,
    /// Restart VMs on other hosts
    RestartVms,
    /// Restart VMs with strict admission control
    RestartWithControl,
}

/// VM restart priority
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPriority {
    Disabled,
    Low,
    Medium,
    High,
    Highest,
}

/// Cluster state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterState {
    /// Cluster is healthy
    Healthy,
    /// Cluster has warnings
    Warning,
    /// Cluster is degraded
    Degraded,
    /// Cluster is in error state
    Error,
    /// Cluster is in maintenance
    Maintenance,
}

/// Cluster statistics
#[derive(Debug, Clone, Default)]
pub struct ClusterStats {
    pub total_hosts: u32,
    pub healthy_hosts: u32,
    pub total_vms: u32,
    pub running_vms: u32,
    pub total_cpu_mhz: u64,
    pub used_cpu_mhz: u64,
    pub total_memory: u64,
    pub used_memory: u64,
    pub ha_failovers: u64,
    pub drs_migrations: u64,
    pub ft_switches: u64,
}

/// Cluster event
#[derive(Debug, Clone)]
pub struct ClusterEvent {
    pub timestamp: Instant,
    pub event_type: ClusterEventType,
    pub host: Option<String>,
    pub vm_id: Option<VmId>,
    pub message: String,
}

/// Cluster event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterEventType {
    HostAdded,
    HostRemoved,
    HostFailed,
    HostRecovered,
    VmFailover,
    VmMigration,
    FtSwitch,
    ClusterWarning,
    ClusterError,
}

impl ClusterManager {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(ClusterConfig::default()),
            hosts: RwLock::new(HashMap::new()),
            vms: RwLock::new(HashMap::new()),
            ha_manager: RwLock::new(HaManager::new()),
            ft_manager: RwLock::new(FtManager::new()),
            drs_manager: RwLock::new(DrsManager::new()),
            events: RwLock::new(VecDeque::new()),
            stats: RwLock::new(ClusterStats::default()),
            state: RwLock::new(ClusterState::Healthy),
        }
    }
    
    /// Configure cluster
    pub fn configure(&self, config: ClusterConfig) {
        *self.config.write().unwrap() = config.clone();
        
        // Update sub-managers
        self.ha_manager.write().unwrap().configure(&config);
        self.drs_manager.write().unwrap().configure(&config);
    }
    
    // ========== Host Management ==========
    
    /// Add host to cluster
    pub fn add_host(&self, host: ClusterHost) -> HypervisorResult<()> {
        let name = host.name.clone();
        
        // Check admission control
        if self.config.read().unwrap().admission_control {
            self.check_admission_control()?;
        }
        
        self.hosts.write().unwrap().insert(name.clone(), host);
        
        // Update DRS
        self.drs_manager.write().unwrap().add_host(&name);
        
        // Log event
        self.log_event(ClusterEventType::HostAdded, Some(name), None, "Host added to cluster");
        
        self.update_stats();
        Ok(())
    }
    
    /// Remove host from cluster
    pub fn remove_host(&self, name: &str) -> HypervisorResult<()> {
        // Check if host has VMs
        let host = self.hosts.read().unwrap().get(name).cloned();
        
        if let Some(host) = host {
            if !host.vms.is_empty() {
                return Err(HypervisorError::ClusterError(
                    "Cannot remove host with running VMs".to_string()
                ));
            }
        }
        
        self.hosts.write().unwrap().remove(name);
        self.drs_manager.write().unwrap().remove_host(name);
        
        self.log_event(ClusterEventType::HostRemoved, Some(name.to_string()), None, "Host removed from cluster");
        
        self.update_stats();
        Ok(())
    }
    
    /// Put host in maintenance mode
    pub fn enter_maintenance(&self, name: &str) -> HypervisorResult<Vec<MigrationRecommendation>> {
        let mut hosts = self.hosts.write().unwrap();
        
        if let Some(host) = hosts.get_mut(name) {
            host.maintenance = true;
            
            // Get migration recommendations for VMs on this host
            let vms: Vec<VmId> = host.vms.iter().copied().collect();
            let recommendations = self.drs_manager.read().unwrap()
                .get_evacuation_plan(name, &vms);
            
            return Ok(recommendations);
        }
        
        Err(HypervisorError::ClusterError("Host not found".to_string()))
    }
    
    /// Exit maintenance mode
    pub fn exit_maintenance(&self, name: &str) -> HypervisorResult<()> {
        let mut hosts = self.hosts.write().unwrap();
        
        if let Some(host) = hosts.get_mut(name) {
            host.maintenance = false;
            return Ok(());
        }
        
        Err(HypervisorError::ClusterError("Host not found".to_string()))
    }
    
    // ========== VM Management ==========
    
    /// Register VM in cluster
    pub fn register_vm(&self, vm: ClusterVm) {
        let vm_id = vm.vm_id;
        let host = vm.host.clone();
        
        self.vms.write().unwrap().insert(vm_id, vm);
        
        // Add to host
        if let Some(host) = self.hosts.write().unwrap().get_mut(&host) {
            host.vms.insert(vm_id);
        }
        
        // Register with HA if protected
        self.ha_manager.write().unwrap().register_vm(vm_id);
        
        self.update_stats();
    }
    
    /// Unregister VM from cluster
    pub fn unregister_vm(&self, vm_id: VmId) {
        if let Some(vm) = self.vms.write().unwrap().remove(&vm_id) {
            // Remove from host
            if let Some(host) = self.hosts.write().unwrap().get_mut(&vm.host) {
                host.vms.remove(&vm_id);
            }
            
            // Unregister from HA
            self.ha_manager.write().unwrap().unregister_vm(vm_id);
            
            // Unregister from FT
            self.ft_manager.write().unwrap().unregister_vm(vm_id);
        }
        
        self.update_stats();
    }
    
    /// Move VM to different host
    pub fn move_vm(&self, vm_id: VmId, target_host: &str) -> HypervisorResult<()> {
        let mut vms = self.vms.write().unwrap();
        let mut hosts = self.hosts.write().unwrap();
        
        let vm = vms.get_mut(&vm_id)
            .ok_or_else(|| HypervisorError::ClusterError("VM not found".to_string()))?;
        
        let old_host = vm.host.clone();
        
        // Remove from old host
        if let Some(host) = hosts.get_mut(&old_host) {
            host.vms.remove(&vm_id);
        }
        
        // Add to new host
        let new_host = hosts.get_mut(target_host)
            .ok_or_else(|| HypervisorError::ClusterError("Target host not found".to_string()))?;
        
        new_host.vms.insert(vm_id);
        vm.host = target_host.to_string();
        
        self.log_event(
            ClusterEventType::VmMigration,
            Some(target_host.to_string()),
            Some(vm_id),
            &format!("VM migrated from {} to {}", old_host, target_host),
        );
        
        Ok(())
    }
    
    // ========== High Availability ==========
    
    /// Handle host failure
    pub fn handle_host_failure(&self, host_name: &str) -> HypervisorResult<Vec<VmId>> {
        let config = self.config.read().unwrap();
        
        if !config.ha_enabled {
            return Ok(Vec::new());
        }
        
        // Mark host as failed
        if let Some(host) = self.hosts.write().unwrap().get_mut(host_name) {
            host.state = HostState::Failed;
        }
        
        self.log_event(
            ClusterEventType::HostFailed,
            Some(host_name.to_string()),
            None,
            "Host failure detected",
        );
        
        // Get VMs to restart
        let vms_to_restart: Vec<VmId> = self.hosts.read().unwrap()
            .get(host_name)
            .map(|h| h.vms.iter().copied().collect())
            .unwrap_or_default();
        
        // Handle based on policy
        match config.host_failure_response {
            HostFailureResponse::LeaveOff => {
                return Ok(Vec::new());
            }
            HostFailureResponse::RestartVms | HostFailureResponse::RestartWithControl => {
                let restarted = self.ha_manager.write().unwrap()
                    .restart_vms(&vms_to_restart, &self.hosts.read().unwrap());
                
                self.stats.write().unwrap().ha_failovers += restarted.len() as u64;
                
                for &vm_id in &restarted {
                    self.log_event(
                        ClusterEventType::VmFailover,
                        None,
                        Some(vm_id),
                        "VM restarted after host failure",
                    );
                }
                
                return Ok(restarted);
            }
        }
    }
    
    /// Handle host recovery
    pub fn handle_host_recovery(&self, host_name: &str) {
        if let Some(host) = self.hosts.write().unwrap().get_mut(host_name) {
            host.state = HostState::Connected;
        }
        
        self.log_event(
            ClusterEventType::HostRecovered,
            Some(host_name.to_string()),
            None,
            "Host recovered",
        );
        
        self.update_cluster_state();
    }
    
    // ========== Fault Tolerance ==========
    
    /// Enable FT for VM
    pub fn enable_ft(&self, vm_id: VmId, secondary_host: &str) -> HypervisorResult<()> {
        let config = self.config.read().unwrap();
        
        if !config.ft_enabled {
            return Err(HypervisorError::ClusterError(
                "Fault Tolerance not enabled on cluster".to_string()
            ));
        }
        
        // Verify secondary host exists
        if !self.hosts.read().unwrap().contains_key(secondary_host) {
            return Err(HypervisorError::ClusterError(
                "Secondary host not found".to_string()
            ));
        }
        
        self.ft_manager.write().unwrap().enable_ft(vm_id, secondary_host)?;
        
        // Update VM
        if let Some(vm) = self.vms.write().unwrap().get_mut(&vm_id) {
            vm.ft_enabled = true;
            vm.ft_secondary_host = Some(secondary_host.to_string());
        }
        
        Ok(())
    }
    
    /// Disable FT for VM
    pub fn disable_ft(&self, vm_id: VmId) -> HypervisorResult<()> {
        self.ft_manager.write().unwrap().disable_ft(vm_id)?;
        
        if let Some(vm) = self.vms.write().unwrap().get_mut(&vm_id) {
            vm.ft_enabled = false;
            vm.ft_secondary_host = None;
        }
        
        Ok(())
    }
    
    /// Perform FT failover
    pub fn ft_failover(&self, vm_id: VmId) -> HypervisorResult<String> {
        let new_host = self.ft_manager.write().unwrap().failover(vm_id)?;
        
        // Update VM location
        self.move_vm(vm_id, &new_host)?;
        
        self.stats.write().unwrap().ft_switches += 1;
        
        self.log_event(
            ClusterEventType::FtSwitch,
            Some(new_host.clone()),
            Some(vm_id),
            "FT failover completed",
        );
        
        Ok(new_host)
    }
    
    // ========== DRS ==========
    
    /// Run DRS load balancing
    pub fn run_drs(&self) -> Vec<MigrationRecommendation> {
        let config = self.config.read().unwrap();
        
        if !config.drs_enabled {
            return Vec::new();
        }
        
        let hosts = self.hosts.read().unwrap();
        let vms = self.vms.read().unwrap();
        
        let recommendations = self.drs_manager.write().unwrap()
            .get_recommendations(&hosts, &vms);
        
        // Auto-apply if fully automated
        if config.drs_automation == DrsAutomation::FullyAutomated {
            for rec in &recommendations {
                if let Err(e) = self.move_vm(rec.vm_id, &rec.target_host) {
                    eprintln!("DRS migration failed: {:?}", e);
                }
            }
            
            self.stats.write().unwrap().drs_migrations += recommendations.len() as u64;
        }
        
        recommendations
    }
    
    // ========== Utilities ==========
    
    fn check_admission_control(&self) -> HypervisorResult<()> {
        let config = self.config.read().unwrap();
        let hosts = self.hosts.read().unwrap();
        
        let total_hosts = hosts.len() as u32;
        let healthy_hosts = hosts.values().filter(|h| h.state == HostState::Connected).count() as u32;
        
        let min_required = (total_hosts as f32 * (1.0 - config.reserved_capacity_percent as f32 / 100.0)).ceil() as u32;
        
        if healthy_hosts < min_required {
            return Err(HypervisorError::ClusterError(
                "Admission control: insufficient cluster resources".to_string()
            ));
        }
        
        Ok(())
    }
    
    fn update_stats(&self) {
        let hosts = self.hosts.read().unwrap();
        let vms = self.vms.read().unwrap();
        
        let mut stats = self.stats.write().unwrap();
        
        stats.total_hosts = hosts.len() as u32;
        stats.healthy_hosts = hosts.values()
            .filter(|h| h.state == HostState::Connected)
            .count() as u32;
        
        stats.total_vms = vms.len() as u32;
        stats.running_vms = vms.values()
            .filter(|v| v.state == VmClusterState::Running)
            .count() as u32;
        
        stats.total_cpu_mhz = hosts.values().map(|h| h.cpu_mhz).sum();
        stats.used_cpu_mhz = hosts.values().map(|h| h.used_cpu_mhz).sum();
        stats.total_memory = hosts.values().map(|h| h.memory).sum();
        stats.used_memory = hosts.values().map(|h| h.used_memory).sum();
    }
    
    fn update_cluster_state(&self) {
        let hosts = self.hosts.read().unwrap();
        
        let total = hosts.len();
        let healthy = hosts.values().filter(|h| h.state == HostState::Connected).count();
        let failed = hosts.values().filter(|h| h.state == HostState::Failed).count();
        
        let state = if failed > 0 {
            if healthy == 0 {
                ClusterState::Error
            } else if healthy < total / 2 {
                ClusterState::Degraded
            } else {
                ClusterState::Warning
            }
        } else if hosts.values().all(|h| h.maintenance) {
            ClusterState::Maintenance
        } else {
            ClusterState::Healthy
        };
        
        *self.state.write().unwrap() = state;
    }
    
    fn log_event(&self, event_type: ClusterEventType, host: Option<String>, vm_id: Option<VmId>, message: &str) {
        let event = ClusterEvent {
            timestamp: Instant::now(),
            event_type,
            host,
            vm_id,
            message: message.to_string(),
        };
        
        let mut events = self.events.write().unwrap();
        events.push_back(event);
        
        // Keep only last 1000 events
        while events.len() > 1000 {
            events.pop_front();
        }
    }
    
    /// Get cluster state
    pub fn state(&self) -> ClusterState {
        *self.state.read().unwrap()
    }
    
    /// Get cluster statistics
    pub fn stats(&self) -> ClusterStats {
        self.stats.read().unwrap().clone()
    }
    
    /// Get recent events
    pub fn get_events(&self, limit: usize) -> Vec<ClusterEvent> {
        self.events.read().unwrap()
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for ClusterManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Cluster Host
// ============================================================================

/// Host in cluster
#[derive(Debug, Clone)]
pub struct ClusterHost {
    pub name: String,
    pub state: HostState,
    pub maintenance: bool,
    pub cpu_mhz: u64,
    pub used_cpu_mhz: u64,
    pub memory: u64,
    pub used_memory: u64,
    pub vms: HashSet<VmId>,
    pub last_heartbeat: Instant,
}

/// Host state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostState {
    Connected,
    Disconnected,
    Failed,
    NotResponding,
}

// ============================================================================
// Cluster VM
// ============================================================================

/// VM in cluster
#[derive(Debug, Clone)]
pub struct ClusterVm {
    pub vm_id: VmId,
    pub host: String,
    pub state: VmClusterState,
    pub ha_protected: bool,
    pub ft_enabled: bool,
    pub ft_secondary_host: Option<String>,
    pub restart_priority: RestartPriority,
}

/// VM state in cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmClusterState {
    Running,
    Stopped,
    Suspended,
    Migrating,
    FailedOver,
}

// ============================================================================
// High Availability Manager
// ============================================================================

/// HA manager
pub struct HaManager {
    /// Protected VMs
    protected_vms: HashSet<VmId>,
    /// VM restart queue
    restart_queue: VecDeque<VmId>,
    /// Configuration
    host_failure_response: HostFailureResponse,
    /// Statistics
    failovers: u64,
}

impl HaManager {
    pub fn new() -> Self {
        Self {
            protected_vms: HashSet::new(),
            restart_queue: VecDeque::new(),
            host_failure_response: HostFailureResponse::RestartVms,
            failovers: 0,
        }
    }
    
    pub fn configure(&mut self, config: &ClusterConfig) {
        self.host_failure_response = config.host_failure_response;
    }
    
    pub fn register_vm(&mut self, vm_id: VmId) {
        self.protected_vms.insert(vm_id);
    }
    
    pub fn unregister_vm(&mut self, vm_id: VmId) {
        self.protected_vms.remove(&vm_id);
    }
    
    pub fn restart_vms(&mut self, vms: &[VmId], hosts: &HashMap<String, ClusterHost>) -> Vec<VmId> {
        let mut restarted = Vec::new();
        
        // Find available hosts
        let available_hosts: Vec<_> = hosts.values()
            .filter(|h| h.state == HostState::Connected && !h.maintenance)
            .collect();
        
        if available_hosts.is_empty() {
            return restarted;
        }
        
        for &vm_id in vms {
            if self.protected_vms.contains(&vm_id) {
                // Find host with least VMs
                if let Some(host) = available_hosts.iter().min_by_key(|h| h.vms.len()) {
                    restarted.push(vm_id);
                    self.failovers += 1;
                }
            }
        }
        
        restarted
    }
}

impl Default for HaManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Fault Tolerance Manager
// ============================================================================

/// FT manager
pub struct FtManager {
    /// FT pairs (primary -> secondary)
    ft_pairs: HashMap<VmId, FtPair>,
}

/// FT pair
#[derive(Debug, Clone)]
pub struct FtPair {
    pub primary_vm: VmId,
    pub secondary_host: String,
    pub state: FtState,
    pub last_sync: Instant,
}

/// FT state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtState {
    Running,
    Syncing,
    NeedResync,
    Disabled,
}

impl FtManager {
    pub fn new() -> Self {
        Self {
            ft_pairs: HashMap::new(),
        }
    }
    
    pub fn enable_ft(&mut self, vm_id: VmId, secondary_host: &str) -> HypervisorResult<()> {
        let pair = FtPair {
            primary_vm: vm_id,
            secondary_host: secondary_host.to_string(),
            state: FtState::Syncing,
            last_sync: Instant::now(),
        };
        
        self.ft_pairs.insert(vm_id, pair);
        Ok(())
    }
    
    pub fn disable_ft(&mut self, vm_id: VmId) -> HypervisorResult<()> {
        self.ft_pairs.remove(&vm_id);
        Ok(())
    }
    
    pub fn unregister_vm(&mut self, vm_id: VmId) {
        self.ft_pairs.remove(&vm_id);
    }
    
    pub fn failover(&mut self, vm_id: VmId) -> HypervisorResult<String> {
        if let Some(pair) = self.ft_pairs.get(&vm_id) {
            let new_host = pair.secondary_host.clone();
            self.ft_pairs.remove(&vm_id);
            Ok(new_host)
        } else {
            Err(HypervisorError::ClusterError("FT not enabled for VM".to_string()))
        }
    }
}

impl Default for FtManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DRS Manager
// ============================================================================

/// DRS manager
pub struct DrsManager {
    /// Load balancer
    load_balancer: LoadBalancer,
    /// DRS automation level
    automation: DrsAutomation,
    /// Migration threshold
    migration_threshold: f64,
}

impl DrsManager {
    pub fn new() -> Self {
        Self {
            load_balancer: LoadBalancer::new(),
            automation: DrsAutomation::FullyAutomated,
            migration_threshold: 0.25,
        }
    }
    
    pub fn configure(&mut self, config: &ClusterConfig) {
        self.automation = config.drs_automation;
    }
    
    pub fn add_host(&mut self, _name: &str) {
        // Would register with load balancer
    }
    
    pub fn remove_host(&mut self, _name: &str) {
        // Would unregister from load balancer
    }
    
    pub fn get_recommendations(
        &mut self,
        hosts: &HashMap<String, ClusterHost>,
        vms: &HashMap<VmId, ClusterVm>,
    ) -> Vec<MigrationRecommendation> {
        // Simplified DRS algorithm
        let mut recommendations = Vec::new();
        
        // Calculate average load
        let total_cpu: u64 = hosts.values().map(|h| h.cpu_mhz).sum();
        let used_cpu: u64 = hosts.values().map(|h| h.used_cpu_mhz).sum();
        let avg_load = if total_cpu > 0 { used_cpu as f64 / total_cpu as f64 } else { 0.0 };
        
        // Find overloaded hosts
        for (host_name, host) in hosts {
            if host.state != HostState::Connected || host.maintenance {
                continue;
            }
            
            let host_load = if host.cpu_mhz > 0 {
                host.used_cpu_mhz as f64 / host.cpu_mhz as f64
            } else {
                0.0
            };
            
            if host_load > avg_load + self.migration_threshold {
                // Find VM to migrate
                for &vm_id in &host.vms {
                    // Find best target
                    for (target_name, target) in hosts {
                        if target_name == host_name || target.state != HostState::Connected || target.maintenance {
                            continue;
                        }
                        
                        let target_load = if target.cpu_mhz > 0 {
                            target.used_cpu_mhz as f64 / target.cpu_mhz as f64
                        } else {
                            0.0
                        };
                        
                        if target_load < avg_load - self.migration_threshold {
                            recommendations.push(MigrationRecommendation {
                                vm_id,
                                source_host: host_name.clone(),
                                target_host: target_name.clone(),
                                reason: format!("DRS: balance load from {:.1}% to {:.1}%", host_load * 100.0, target_load * 100.0),
                                priority: 50,
                                estimated_benefit: host_load - target_load,
                            });
                            break;
                        }
                    }
                }
            }
        }
        
        recommendations
    }
    
    pub fn get_evacuation_plan(&self, host: &str, vms: &[VmId]) -> Vec<MigrationRecommendation> {
        vms.iter().map(|&vm_id| {
            MigrationRecommendation {
                vm_id,
                source_host: host.to_string(),
                target_host: "auto".to_string(),
                reason: "Host entering maintenance mode".to_string(),
                priority: 100,
                estimated_benefit: 1.0,
            }
        }).collect()
    }
}

impl Default for DrsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cluster_manager() {
        let cluster = ClusterManager::new();
        
        // Add host
        let host = ClusterHost {
            name: "host1".to_string(),
            state: HostState::Connected,
            maintenance: false,
            cpu_mhz: 10000,
            used_cpu_mhz: 2000,
            memory: 16 * 1024 * 1024 * 1024,
            used_memory: 4 * 1024 * 1024 * 1024,
            vms: HashSet::new(),
            last_heartbeat: Instant::now(),
        };
        
        cluster.add_host(host).unwrap();
        
        assert_eq!(cluster.stats().total_hosts, 1);
        assert_eq!(cluster.stats().healthy_hosts, 1);
    }
    
    #[test]
    fn test_ha_failover() {
        let cluster = ClusterManager::new();
        cluster.configure(ClusterConfig {
            ha_enabled: true,
            ..Default::default()
        });
        
        // Add two hosts
        for i in 1..=2 {
            let host = ClusterHost {
                name: format!("host{}", i),
                state: HostState::Connected,
                maintenance: false,
                cpu_mhz: 10000,
                used_cpu_mhz: 0,
                memory: 16 * 1024 * 1024 * 1024,
                used_memory: 0,
                vms: HashSet::new(),
                last_heartbeat: Instant::now(),
            };
            cluster.add_host(host).unwrap();
        }
        
        // Add VM
        let vm = ClusterVm {
            vm_id: VmId::new(1),
            host: "host1".to_string(),
            state: VmClusterState::Running,
            ha_protected: true,
            ft_enabled: false,
            ft_secondary_host: None,
            restart_priority: RestartPriority::High,
        };
        cluster.register_vm(vm);
        
        // Simulate host failure
        let restarted = cluster.handle_host_failure("host1").unwrap();
        assert!(!restarted.is_empty());
    }
    
    #[test]
    fn test_cluster_state() {
        let cluster = ClusterManager::new();
        
        assert_eq!(cluster.state(), ClusterState::Healthy);
        
        // Add failed host
        let host = ClusterHost {
            name: "host1".to_string(),
            state: HostState::Failed,
            maintenance: false,
            cpu_mhz: 10000,
            used_cpu_mhz: 0,
            memory: 16 * 1024 * 1024 * 1024,
            used_memory: 0,
            vms: HashSet::new(),
            last_heartbeat: Instant::now(),
        };
        cluster.add_host(host).unwrap();
        
        cluster.handle_host_recovery("host1");
        assert_eq!(cluster.state(), ClusterState::Healthy);
    }
}
