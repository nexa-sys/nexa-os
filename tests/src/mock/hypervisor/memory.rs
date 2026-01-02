//! Advanced Memory Management
//!
//! This module provides enterprise-grade memory management features including:
//! - Memory ballooning (dynamic memory adjustment)
//! - KSM (Kernel Same-page Merging) for memory deduplication
//! - NUMA-aware memory allocation
//! - Memory hot-plug support
//! - Memory overcommit with intelligent reclaim

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Instant, Duration};

use super::resources::MemoryPool;
use super::core::HypervisorError;

// ============================================================================
// Memory Manager
// ============================================================================

/// Central memory manager coordinating all memory features
pub struct MemoryManager {
    /// Memory pool reference
    pool: Arc<MemoryPool>,
    /// Balloon driver manager
    balloon: Arc<BalloonManager>,
    /// KSM manager
    ksm: Option<Arc<KsmManager>>,
    /// NUMA manager
    numa: Option<Arc<NumaManager>>,
    /// Memory hot-plug manager
    hotplug: Arc<MemoryHotplugManager>,
    /// Overcommit manager
    overcommit: Arc<MemoryOvercommitManager>,
    /// Per-VM memory state
    vm_states: RwLock<HashMap<String, VmMemoryState>>,
    /// Global statistics
    stats: RwLock<MemoryManagerStats>,
}

#[derive(Debug, Clone)]
struct VmMemoryState {
    vm_name: String,
    allocated_mb: u64,
    balloon_current_mb: u64,
    balloon_target_mb: Option<u64>,
    ksm_pages: u64,
    numa_node: Option<u32>,
    hotplugged_mb: u64,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryManagerStats {
    pub total_balloon_savings_mb: u64,
    pub total_ksm_savings_mb: u64,
    pub balloon_operations: u64,
    pub ksm_scans: u64,
    pub hotplug_operations: u64,
    pub oom_events: u64,
}

impl MemoryManager {
    pub fn new(pool: Arc<MemoryPool>, enable_ksm: bool) -> Self {
        let ksm = if enable_ksm {
            Some(Arc::new(KsmManager::new()))
        } else {
            None
        };
        
        Self {
            pool: pool.clone(),
            balloon: Arc::new(BalloonManager::new()),
            ksm,
            numa: None,
            hotplug: Arc::new(MemoryHotplugManager::new()),
            overcommit: Arc::new(MemoryOvercommitManager::new(pool)),
            vm_states: RwLock::new(HashMap::new()),
            stats: RwLock::new(MemoryManagerStats::default()),
        }
    }
    
    pub fn with_numa(mut self, config: NumaConfig) -> Self {
        self.numa = Some(Arc::new(NumaManager::new(config)));
        self
    }
    
    /// Register a VM with the memory manager
    pub fn register_vm(&self, vm_name: &str, allocated_mb: u64, numa_node: Option<u32>) {
        let state = VmMemoryState {
            vm_name: vm_name.to_string(),
            allocated_mb,
            balloon_current_mb: allocated_mb,
            balloon_target_mb: None,
            ksm_pages: 0,
            numa_node,
            hotplugged_mb: 0,
        };
        
        self.vm_states.write().unwrap().insert(vm_name.to_string(), state);
    }
    
    /// Unregister a VM
    pub fn unregister_vm(&self, vm_name: &str) {
        self.vm_states.write().unwrap().remove(vm_name);
    }
    
    /// Get balloon driver manager
    pub fn balloon(&self) -> Arc<BalloonManager> {
        self.balloon.clone()
    }
    
    /// Get KSM manager
    pub fn ksm(&self) -> Option<Arc<KsmManager>> {
        self.ksm.clone()
    }
    
    /// Get NUMA manager
    pub fn numa(&self) -> Option<Arc<NumaManager>> {
        self.numa.clone()
    }
    
    /// Get hot-plug manager
    pub fn hotplug(&self) -> Arc<MemoryHotplugManager> {
        self.hotplug.clone()
    }
    
    /// Get overcommit manager
    pub fn overcommit(&self) -> Arc<MemoryOvercommitManager> {
        self.overcommit.clone()
    }
    
    /// Request memory reclaim (called when memory pressure is high)
    pub fn request_reclaim(&self, needed_mb: u64) -> u64 {
        let mut reclaimed = 0u64;
        
        // First, try balloon inflation
        let balloon_reclaim = self.balloon.request_reclaim(needed_mb - reclaimed);
        reclaimed += balloon_reclaim;
        
        if reclaimed >= needed_mb {
            return reclaimed;
        }
        
        // Then, trigger KSM scan if available
        if let Some(ksm) = &self.ksm {
            let ksm_reclaim = ksm.trigger_urgent_scan();
            reclaimed += ksm_reclaim;
        }
        
        reclaimed
    }
    
    /// Get statistics
    pub fn stats(&self) -> MemoryManagerStats {
        self.stats.read().unwrap().clone()
    }
}

// ============================================================================
// Balloon Driver
// ============================================================================

/// Balloon driver manager for dynamic memory adjustment
pub struct BalloonManager {
    /// Per-VM balloon state
    vm_balloons: RwLock<HashMap<String, BalloonState>>,
    /// Global balloon configuration
    config: RwLock<BalloonConfig>,
    /// Statistics
    stats: RwLock<BalloonStats>,
    /// Is currently reclaiming
    reclaiming: AtomicBool,
}

#[derive(Debug, Clone)]
struct BalloonState {
    vm_name: String,
    /// Original allocated memory (MB)
    original_mb: u64,
    /// Current balloon size (inflated amount in MB)
    inflated_mb: u64,
    /// Target balloon size
    target_mb: u64,
    /// Minimum memory (MB)
    min_mb: u64,
    /// Last adjustment time
    last_adjusted: Instant,
}

#[derive(Debug, Clone)]
pub struct BalloonConfig {
    /// Enable automatic ballooning
    pub auto_balloon: bool,
    /// Target free memory ratio (0.0 - 1.0)
    pub target_free_ratio: f64,
    /// Adjustment interval
    pub adjust_interval: Duration,
    /// Maximum inflation rate (MB/s)
    pub max_inflate_rate: u64,
    /// Maximum deflation rate (MB/s)
    pub max_deflate_rate: u64,
}

impl Default for BalloonConfig {
    fn default() -> Self {
        Self {
            auto_balloon: true,
            target_free_ratio: 0.1,
            adjust_interval: Duration::from_secs(5),
            max_inflate_rate: 512,
            max_deflate_rate: 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BalloonStats {
    pub total_inflated_mb: u64,
    pub total_deflated_mb: u64,
    pub inflation_operations: u64,
    pub deflation_operations: u64,
    pub current_savings_mb: u64,
}

impl BalloonManager {
    pub fn new() -> Self {
        Self {
            vm_balloons: RwLock::new(HashMap::new()),
            config: RwLock::new(BalloonConfig::default()),
            stats: RwLock::new(BalloonStats::default()),
            reclaiming: AtomicBool::new(false),
        }
    }
    
    /// Register a VM's balloon driver
    pub fn register_vm(&self, vm_name: &str, memory_mb: u64, min_mb: u64) {
        let state = BalloonState {
            vm_name: vm_name.to_string(),
            original_mb: memory_mb,
            inflated_mb: 0,
            target_mb: 0,
            min_mb,
            last_adjusted: Instant::now(),
        };
        
        self.vm_balloons.write().unwrap().insert(vm_name.to_string(), state);
    }
    
    /// Unregister a VM's balloon driver
    pub fn unregister_vm(&self, vm_name: &str) {
        self.vm_balloons.write().unwrap().remove(vm_name);
    }
    
    /// Set balloon target for a VM
    pub fn set_target(&self, vm_name: &str, target_mb: u64) -> Result<(), HypervisorError> {
        let mut balloons = self.vm_balloons.write().unwrap();
        let state = balloons.get_mut(vm_name)
            .ok_or_else(|| HypervisorError::InternalError(
                format!("Balloon not registered for VM '{}'", vm_name)
            ))?;
        
        // Validate target
        let max_inflate = state.original_mb - state.min_mb;
        let actual_target = target_mb.min(max_inflate);
        
        state.target_mb = actual_target;
        Ok(())
    }
    
    /// Inflate balloon (reclaim memory from VM)
    pub fn inflate(&self, vm_name: &str, amount_mb: u64) -> Result<u64, HypervisorError> {
        let mut balloons = self.vm_balloons.write().unwrap();
        let state = balloons.get_mut(vm_name)
            .ok_or_else(|| HypervisorError::InternalError(
                format!("Balloon not registered for VM '{}'", vm_name)
            ))?;
        
        let max_inflate = state.original_mb - state.min_mb - state.inflated_mb;
        let actual_inflate = amount_mb.min(max_inflate);
        
        if actual_inflate > 0 {
            state.inflated_mb += actual_inflate;
            state.last_adjusted = Instant::now();
            
            let mut stats = self.stats.write().unwrap();
            stats.total_inflated_mb += actual_inflate;
            stats.inflation_operations += 1;
            stats.current_savings_mb += actual_inflate;
        }
        
        Ok(actual_inflate)
    }
    
    /// Deflate balloon (return memory to VM)
    pub fn deflate(&self, vm_name: &str, amount_mb: u64) -> Result<u64, HypervisorError> {
        let mut balloons = self.vm_balloons.write().unwrap();
        let state = balloons.get_mut(vm_name)
            .ok_or_else(|| HypervisorError::InternalError(
                format!("Balloon not registered for VM '{}'", vm_name)
            ))?;
        
        let actual_deflate = amount_mb.min(state.inflated_mb);
        
        if actual_deflate > 0 {
            state.inflated_mb -= actual_deflate;
            state.last_adjusted = Instant::now();
            
            let mut stats = self.stats.write().unwrap();
            stats.total_deflated_mb += actual_deflate;
            stats.deflation_operations += 1;
            stats.current_savings_mb = stats.current_savings_mb.saturating_sub(actual_deflate);
        }
        
        Ok(actual_deflate)
    }
    
    /// Request memory reclaim across all VMs
    pub fn request_reclaim(&self, needed_mb: u64) -> u64 {
        if self.reclaiming.swap(true, Ordering::SeqCst) {
            return 0; // Already reclaiming
        }
        
        let mut reclaimed = 0u64;
        let balloons = self.vm_balloons.read().unwrap();
        
        // Sort VMs by available memory for reclaim (most available first)
        let mut vm_available: Vec<_> = balloons.iter()
            .map(|(name, state)| {
                let available = state.original_mb - state.min_mb - state.inflated_mb;
                (name.clone(), available)
            })
            .collect();
        vm_available.sort_by_key(|(_, avail)| std::cmp::Reverse(*avail));
        
        drop(balloons);
        
        // Reclaim proportionally from VMs
        for (vm_name, available) in vm_available {
            if reclaimed >= needed_mb {
                break;
            }
            
            let to_reclaim = ((needed_mb - reclaimed) as f64 * 0.5).ceil() as u64;
            let to_reclaim = to_reclaim.min(available);
            
            if let Ok(inflated) = self.inflate(&vm_name, to_reclaim) {
                reclaimed += inflated;
            }
        }
        
        self.reclaiming.store(false, Ordering::SeqCst);
        reclaimed
    }
    
    /// Get current balloon state for a VM
    pub fn get_state(&self, vm_name: &str) -> Option<(u64, u64, u64)> {
        self.vm_balloons.read().unwrap().get(vm_name)
            .map(|s| (s.original_mb, s.inflated_mb, s.min_mb))
    }
    
    /// Get statistics
    pub fn stats(&self) -> BalloonStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for BalloonManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// KSM (Kernel Same-page Merging)
// ============================================================================

/// KSM manager for memory deduplication
pub struct KsmManager {
    /// Is KSM enabled
    enabled: AtomicBool,
    /// KSM configuration
    config: RwLock<KsmConfig>,
    /// Pages shared (deduplicated)
    pages_shared: AtomicU64,
    /// Pages sharing (pointing to shared pages)
    pages_sharing: AtomicU64,
    /// Pages unshared (broken COW)
    pages_unshared: AtomicU64,
    /// Full scans completed
    full_scans: AtomicU64,
    /// Statistics
    stats: RwLock<KsmStats>,
}

#[derive(Debug, Clone)]
pub struct KsmConfig {
    /// Enable KSM
    pub enabled: bool,
    /// Pages to scan per batch
    pub pages_to_scan: u64,
    /// Sleep between scans (ms)
    pub sleep_ms: u64,
    /// Maximum page sharing ratio
    pub max_page_sharing: f64,
    /// Merge across NUMA nodes
    pub merge_across_nodes: bool,
}

impl Default for KsmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pages_to_scan: 100,
            sleep_ms: 20,
            max_page_sharing: 0.5,
            merge_across_nodes: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct KsmStats {
    pub pages_shared: u64,
    pub pages_sharing: u64,
    pub pages_unshared: u64,
    pub full_scans: u64,
    pub memory_saved_mb: u64,
    pub scan_time_ms: u64,
}

impl KsmManager {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            config: RwLock::new(KsmConfig::default()),
            pages_shared: AtomicU64::new(0),
            pages_sharing: AtomicU64::new(0),
            pages_unshared: AtomicU64::new(0),
            full_scans: AtomicU64::new(0),
            stats: RwLock::new(KsmStats::default()),
        }
    }
    
    /// Enable or disable KSM
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
    
    /// Check if KSM is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    
    /// Update configuration
    pub fn configure(&self, config: KsmConfig) {
        *self.config.write().unwrap() = config;
    }
    
    /// Run a KSM scan iteration
    pub fn run_scan(&self) -> u64 {
        if !self.is_enabled() {
            return 0;
        }
        
        let config = self.config.read().unwrap().clone();
        let scan_start = Instant::now();
        
        // Simulate scanning and finding duplicate pages
        // In real implementation, this would interface with the kernel's KSM
        let pages_scanned = config.pages_to_scan;
        let new_shared = (pages_scanned as f64 * 0.1) as u64; // ~10% duplicate rate
        
        self.pages_shared.fetch_add(new_shared, Ordering::SeqCst);
        self.pages_sharing.fetch_add(new_shared * 2, Ordering::SeqCst);
        
        let scan_time = scan_start.elapsed().as_millis() as u64;
        
        let mut stats = self.stats.write().unwrap();
        stats.pages_shared = self.pages_shared.load(Ordering::SeqCst);
        stats.pages_sharing = self.pages_sharing.load(Ordering::SeqCst);
        stats.scan_time_ms += scan_time;
        stats.memory_saved_mb = (stats.pages_sharing * 4096) / (1024 * 1024);
        
        new_shared
    }
    
    /// Trigger urgent scan for memory reclaim
    pub fn trigger_urgent_scan(&self) -> u64 {
        // Run multiple scan iterations for urgent reclaim
        let mut total_saved = 0u64;
        for _ in 0..10 {
            total_saved += self.run_scan();
        }
        
        self.full_scans.fetch_add(1, Ordering::SeqCst);
        
        // Return memory saved in MB
        (total_saved * 4096) / (1024 * 1024)
    }
    
    /// Get memory savings (MB)
    pub fn memory_saved_mb(&self) -> u64 {
        let pages = self.pages_sharing.load(Ordering::SeqCst);
        (pages * 4096) / (1024 * 1024)
    }
    
    /// Get statistics
    pub fn stats(&self) -> KsmStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for KsmManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// NUMA Manager
// ============================================================================

/// NUMA-aware memory allocation manager
pub struct NumaManager {
    /// NUMA configuration
    config: RwLock<NumaConfig>,
    /// Per-node memory state
    node_states: RwLock<Vec<NumaNodeState>>,
    /// VM to node bindings
    vm_bindings: RwLock<HashMap<String, Vec<u32>>>,
}

/// NUMA configuration
#[derive(Debug, Clone)]
pub struct NumaConfig {
    /// NUMA nodes
    pub nodes: Vec<NumaNodeConfig>,
    /// Automatic node balancing
    pub auto_balance: bool,
    /// Prefer local memory
    pub local_memory_policy: LocalMemoryPolicy,
    /// Allow cross-node allocation
    pub allow_cross_node: bool,
}

#[derive(Debug, Clone)]
pub struct NumaNodeConfig {
    pub id: u32,
    pub cpus: Vec<u32>,
    pub memory_mb: u64,
    pub distances: HashMap<u32, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalMemoryPolicy {
    /// Strict local allocation (fail if no local memory)
    Strict,
    /// Prefer local, fall back to remote
    Preferred,
    /// Interleave across nodes
    Interleave,
    /// Bind to specific nodes
    Bind,
}

impl Default for LocalMemoryPolicy {
    fn default() -> Self {
        Self::Preferred
    }
}

#[derive(Debug, Clone)]
struct NumaNodeState {
    id: u32,
    total_mb: u64,
    available_mb: u64,
    allocated_vms: Vec<String>,
}

impl NumaManager {
    pub fn new(config: NumaConfig) -> Self {
        let node_states: Vec<_> = config.nodes.iter()
            .map(|n| NumaNodeState {
                id: n.id,
                total_mb: n.memory_mb,
                available_mb: n.memory_mb,
                allocated_vms: Vec::new(),
            })
            .collect();
        
        Self {
            config: RwLock::new(config),
            node_states: RwLock::new(node_states),
            vm_bindings: RwLock::new(HashMap::new()),
        }
    }
    
    /// Allocate memory for a VM on optimal NUMA node(s)
    pub fn allocate(&self, vm_name: &str, memory_mb: u64, preferred_node: Option<u32>) -> Result<Vec<u32>, HypervisorError> {
        let config = self.config.read().unwrap();
        let mut nodes = self.node_states.write().unwrap();
        
        // Try preferred node first
        if let Some(node_id) = preferred_node {
            if let Some(node) = nodes.iter_mut().find(|n| n.id == node_id) {
                if node.available_mb >= memory_mb {
                    node.available_mb -= memory_mb;
                    node.allocated_vms.push(vm_name.to_string());
                    self.vm_bindings.write().unwrap().insert(vm_name.to_string(), vec![node_id]);
                    return Ok(vec![node_id]);
                }
            }
        }
        
        match config.local_memory_policy {
            LocalMemoryPolicy::Strict => {
                Err(HypervisorError::ResourceUnavailable {
                    resource: "NUMA memory".to_string(),
                    requested: memory_mb,
                    available: 0,
                })
            }
            LocalMemoryPolicy::Preferred | LocalMemoryPolicy::Bind => {
                // Find best-fit node
                let mut best_node = None;
                let mut best_available = 0u64;
                
                for node in nodes.iter() {
                    if node.available_mb >= memory_mb && node.available_mb > best_available {
                        best_node = Some(node.id);
                        best_available = node.available_mb;
                    }
                }
                
                if let Some(node_id) = best_node {
                    let node = nodes.iter_mut().find(|n| n.id == node_id).unwrap();
                    node.available_mb -= memory_mb;
                    node.allocated_vms.push(vm_name.to_string());
                    self.vm_bindings.write().unwrap().insert(vm_name.to_string(), vec![node_id]);
                    Ok(vec![node_id])
                } else if config.allow_cross_node {
                    // Split across nodes
                    self.allocate_cross_node(vm_name, memory_mb, &mut nodes)
                } else {
                    Err(HypervisorError::ResourceUnavailable {
                        resource: "NUMA memory".to_string(),
                        requested: memory_mb,
                        available: nodes.iter().map(|n| n.available_mb).sum(),
                    })
                }
            }
            LocalMemoryPolicy::Interleave => {
                // Interleave across all nodes
                self.allocate_cross_node(vm_name, memory_mb, &mut nodes)
            }
        }
    }
    
    /// Allocate memory across multiple NUMA nodes
    fn allocate_cross_node(
        &self,
        vm_name: &str,
        memory_mb: u64,
        nodes: &mut Vec<NumaNodeState>,
    ) -> Result<Vec<u32>, HypervisorError> {
        let total_available: u64 = nodes.iter().map(|n| n.available_mb).sum();
        
        if total_available < memory_mb {
            return Err(HypervisorError::ResourceUnavailable {
                resource: "NUMA memory".to_string(),
                requested: memory_mb,
                available: total_available,
            });
        }
        
        let mut remaining = memory_mb;
        let mut allocated_nodes = Vec::new();
        
        // Allocate from each node proportionally
        let node_count = nodes.len() as u64;
        let per_node = memory_mb / node_count;
        
        for node in nodes.iter_mut() {
            if remaining == 0 {
                break;
            }
            
            let alloc = per_node.min(node.available_mb).min(remaining);
            if alloc > 0 {
                node.available_mb -= alloc;
                node.allocated_vms.push(vm_name.to_string());
                allocated_nodes.push(node.id);
                remaining -= alloc;
            }
        }
        
        if remaining > 0 {
            // Try to allocate remainder from any node with space
            for node in nodes.iter_mut() {
                if remaining == 0 {
                    break;
                }
                
                let alloc = remaining.min(node.available_mb);
                if alloc > 0 {
                    node.available_mb -= alloc;
                    if !allocated_nodes.contains(&node.id) {
                        allocated_nodes.push(node.id);
                    }
                    remaining -= alloc;
                }
            }
        }
        
        self.vm_bindings.write().unwrap().insert(vm_name.to_string(), allocated_nodes.clone());
        Ok(allocated_nodes)
    }
    
    /// Release memory for a VM
    pub fn release(&self, vm_name: &str) {
        let binding = self.vm_bindings.write().unwrap().remove(vm_name);
        if let Some(node_ids) = binding {
            let mut nodes = self.node_states.write().unwrap();
            for node in nodes.iter_mut() {
                if let Some(pos) = node.allocated_vms.iter().position(|n| n == vm_name) {
                    node.allocated_vms.remove(pos);
                    // Note: actual memory tracking would need more state
                }
            }
        }
    }
    
    /// Get VM's NUMA binding
    pub fn get_binding(&self, vm_name: &str) -> Option<Vec<u32>> {
        self.vm_bindings.read().unwrap().get(vm_name).cloned()
    }
    
    /// Get node status
    pub fn get_node_status(&self, node_id: u32) -> Option<(u64, u64)> {
        self.node_states.read().unwrap()
            .iter()
            .find(|n| n.id == node_id)
            .map(|n| (n.total_mb, n.available_mb))
    }
}

// ============================================================================
// Memory Hot-plug
// ============================================================================

/// Memory hot-plug manager
pub struct MemoryHotplugManager {
    /// Hot-plug operations log
    operations: RwLock<Vec<HotplugOperation>>,
    /// Per-VM hot-plug state
    vm_states: RwLock<HashMap<String, VmHotplugState>>,
    /// DIMM slot management
    dimm_slots: RwLock<DimmSlotManager>,
}

#[derive(Debug, Clone)]
struct HotplugOperation {
    vm_name: String,
    operation: HotplugOp,
    size_mb: u64,
    timestamp: Instant,
    success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotplugOp {
    Add,
    Remove,
}

#[derive(Debug, Clone)]
struct VmHotplugState {
    initial_memory_mb: u64,
    current_memory_mb: u64,
    max_memory_mb: u64,
    hotplugged_dimms: Vec<HotpluggedDimm>,
}

#[derive(Debug, Clone)]
struct HotpluggedDimm {
    slot: u32,
    size_mb: u64,
    node: Option<u32>,
}

#[derive(Debug, Clone)]
struct DimmSlotManager {
    total_slots: u32,
    used_slots: Vec<bool>,
}

impl Default for DimmSlotManager {
    fn default() -> Self {
        Self {
            total_slots: 256,
            used_slots: vec![false; 256],
        }
    }
}

impl DimmSlotManager {
    fn allocate_slot(&mut self) -> Option<u32> {
        for (i, used) in self.used_slots.iter_mut().enumerate() {
            if !*used {
                *used = true;
                return Some(i as u32);
            }
        }
        None
    }
    
    fn release_slot(&mut self, slot: u32) {
        if (slot as usize) < self.used_slots.len() {
            self.used_slots[slot as usize] = false;
        }
    }
}

impl MemoryHotplugManager {
    pub fn new() -> Self {
        Self {
            operations: RwLock::new(Vec::new()),
            vm_states: RwLock::new(HashMap::new()),
            dimm_slots: RwLock::new(DimmSlotManager::default()),
        }
    }
    
    /// Register a VM for hot-plug support
    pub fn register_vm(&self, vm_name: &str, initial_mb: u64, max_mb: u64) {
        let state = VmHotplugState {
            initial_memory_mb: initial_mb,
            current_memory_mb: initial_mb,
            max_memory_mb: max_mb,
            hotplugged_dimms: Vec::new(),
        };
        
        self.vm_states.write().unwrap().insert(vm_name.to_string(), state);
    }
    
    /// Unregister a VM
    pub fn unregister_vm(&self, vm_name: &str) {
        if let Some(state) = self.vm_states.write().unwrap().remove(vm_name) {
            let mut slots = self.dimm_slots.write().unwrap();
            for dimm in state.hotplugged_dimms {
                slots.release_slot(dimm.slot);
            }
        }
    }
    
    /// Add memory to a VM
    pub fn add_memory(&self, vm_name: &str, size_mb: u64, numa_node: Option<u32>) -> Result<(), HypervisorError> {
        let mut states = self.vm_states.write().unwrap();
        let state = states.get_mut(vm_name)
            .ok_or_else(|| HypervisorError::InternalError(
                format!("VM '{}' not registered for hot-plug", vm_name)
            ))?;
        
        // Check max memory limit
        if state.current_memory_mb + size_mb > state.max_memory_mb {
            return Err(HypervisorError::ResourceUnavailable {
                resource: "Memory (max limit)".to_string(),
                requested: size_mb,
                available: state.max_memory_mb - state.current_memory_mb,
            });
        }
        
        // Allocate DIMM slot
        let mut slots = self.dimm_slots.write().unwrap();
        let slot = slots.allocate_slot()
            .ok_or_else(|| HypervisorError::ResourceUnavailable {
                resource: "DIMM slot".to_string(),
                requested: 1,
                available: 0,
            })?;
        
        // Record hot-plugged DIMM
        let dimm = HotpluggedDimm {
            slot,
            size_mb,
            node: numa_node,
        };
        state.hotplugged_dimms.push(dimm);
        state.current_memory_mb += size_mb;
        
        // Record operation
        self.operations.write().unwrap().push(HotplugOperation {
            vm_name: vm_name.to_string(),
            operation: HotplugOp::Add,
            size_mb,
            timestamp: Instant::now(),
            success: true,
        });
        
        Ok(())
    }
    
    /// Remove memory from a VM
    pub fn remove_memory(&self, vm_name: &str, size_mb: u64) -> Result<(), HypervisorError> {
        let mut states = self.vm_states.write().unwrap();
        let state = states.get_mut(vm_name)
            .ok_or_else(|| HypervisorError::InternalError(
                format!("VM '{}' not registered for hot-plug", vm_name)
            ))?;
        
        // Find a DIMM to remove
        let dimm_idx = state.hotplugged_dimms.iter()
            .position(|d| d.size_mb == size_mb)
            .ok_or_else(|| HypervisorError::ConfigError(
                format!("No DIMM of size {}MB found for removal", size_mb)
            ))?;
        
        let dimm = state.hotplugged_dimms.remove(dimm_idx);
        state.current_memory_mb -= dimm.size_mb;
        
        // Release DIMM slot
        self.dimm_slots.write().unwrap().release_slot(dimm.slot);
        
        // Record operation
        self.operations.write().unwrap().push(HotplugOperation {
            vm_name: vm_name.to_string(),
            operation: HotplugOp::Remove,
            size_mb,
            timestamp: Instant::now(),
            success: true,
        });
        
        Ok(())
    }
    
    /// Get VM's current memory state
    pub fn get_state(&self, vm_name: &str) -> Option<(u64, u64, u64)> {
        self.vm_states.read().unwrap().get(vm_name)
            .map(|s| (s.initial_memory_mb, s.current_memory_mb, s.max_memory_mb))
    }
    
    /// Get hot-plug history
    pub fn get_history(&self, limit: usize) -> Vec<(String, String, u64, bool)> {
        self.operations.read().unwrap()
            .iter()
            .rev()
            .take(limit)
            .map(|op| {
                let op_str = match op.operation {
                    HotplugOp::Add => "add",
                    HotplugOp::Remove => "remove",
                };
                (op.vm_name.clone(), op_str.to_string(), op.size_mb, op.success)
            })
            .collect()
    }
}

impl Default for MemoryHotplugManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Memory Overcommit
// ============================================================================

/// Memory overcommit manager
pub struct MemoryOvercommitManager {
    pool: Arc<MemoryPool>,
    /// Overcommit ratio
    ratio: RwLock<f64>,
    /// Low memory threshold (percentage)
    low_threshold: RwLock<f64>,
    /// Critical memory threshold (percentage)
    critical_threshold: RwLock<f64>,
    /// Current memory pressure
    pressure: RwLock<MemoryPressure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    None,
    Low,
    Medium,
    High,
    Critical,
}

impl Default for MemoryPressure {
    fn default() -> Self {
        Self::None
    }
}

impl MemoryOvercommitManager {
    pub fn new(pool: Arc<MemoryPool>) -> Self {
        Self {
            pool,
            ratio: RwLock::new(1.5),
            low_threshold: RwLock::new(0.7),
            critical_threshold: RwLock::new(0.9),
            pressure: RwLock::new(MemoryPressure::None),
        }
    }
    
    /// Set overcommit ratio
    pub fn set_ratio(&self, ratio: f64) {
        *self.ratio.write().unwrap() = ratio;
    }
    
    /// Get overcommit ratio
    pub fn get_ratio(&self) -> f64 {
        *self.ratio.read().unwrap()
    }
    
    /// Set thresholds
    pub fn set_thresholds(&self, low: f64, critical: f64) {
        *self.low_threshold.write().unwrap() = low;
        *self.critical_threshold.write().unwrap() = critical;
    }
    
    /// Calculate effective available memory with overcommit
    pub fn effective_available(&self) -> u64 {
        let available = self.pool.available();
        let ratio = *self.ratio.read().unwrap();
        (available as f64 * ratio) as u64
    }
    
    /// Update memory pressure level
    pub fn update_pressure(&self) {
        let total = self.pool.total() as f64;
        let used = (self.pool.total() - self.pool.available()) as f64;
        let utilization = used / total;
        
        let low = *self.low_threshold.read().unwrap();
        let critical = *self.critical_threshold.read().unwrap();
        
        let pressure = if utilization >= critical {
            MemoryPressure::Critical
        } else if utilization >= 0.85 {
            MemoryPressure::High
        } else if utilization >= low {
            MemoryPressure::Medium
        } else if utilization >= 0.5 {
            MemoryPressure::Low
        } else {
            MemoryPressure::None
        };
        
        *self.pressure.write().unwrap() = pressure;
    }
    
    /// Get current memory pressure
    pub fn get_pressure(&self) -> MemoryPressure {
        *self.pressure.read().unwrap()
    }
    
    /// Check if allocation is safe with overcommit
    pub fn can_allocate(&self, size_mb: u64) -> bool {
        let effective = self.effective_available();
        size_mb <= effective
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_balloon_manager() {
        let manager = BalloonManager::new();
        
        manager.register_vm("test-vm", 4096, 1024);
        
        // Inflate balloon
        let inflated = manager.inflate("test-vm", 512).unwrap();
        assert_eq!(inflated, 512);
        
        let (original, current, min) = manager.get_state("test-vm").unwrap();
        assert_eq!(original, 4096);
        assert_eq!(current, 512);
        assert_eq!(min, 1024);
        
        // Deflate balloon
        let deflated = manager.deflate("test-vm", 256).unwrap();
        assert_eq!(deflated, 256);
        
        let (_, current, _) = manager.get_state("test-vm").unwrap();
        assert_eq!(current, 256);
    }
    
    #[test]
    fn test_ksm_manager() {
        let ksm = KsmManager::new();
        
        assert!(ksm.is_enabled());
        
        // Run scan
        let shared = ksm.run_scan();
        assert!(shared > 0);
        
        let stats = ksm.stats();
        assert!(stats.pages_shared > 0);
    }
    
    #[test]
    fn test_memory_hotplug() {
        let manager = MemoryHotplugManager::new();
        
        manager.register_vm("test-vm", 4096, 16384);
        
        // Add memory
        manager.add_memory("test-vm", 2048, None).unwrap();
        
        let (initial, current, max) = manager.get_state("test-vm").unwrap();
        assert_eq!(initial, 4096);
        assert_eq!(current, 6144);
        assert_eq!(max, 16384);
        
        // Remove memory
        manager.remove_memory("test-vm", 2048).unwrap();
        
        let (_, current, _) = manager.get_state("test-vm").unwrap();
        assert_eq!(current, 4096);
    }
    
    #[test]
    fn test_numa_allocation() {
        let config = NumaConfig {
            nodes: vec![
                NumaNodeConfig {
                    id: 0,
                    cpus: vec![0, 1, 2, 3],
                    memory_mb: 8192,
                    distances: HashMap::new(),
                },
                NumaNodeConfig {
                    id: 1,
                    cpus: vec![4, 5, 6, 7],
                    memory_mb: 8192,
                    distances: HashMap::new(),
                },
            ],
            auto_balance: true,
            local_memory_policy: LocalMemoryPolicy::Preferred,
            allow_cross_node: true,
        };
        
        let manager = NumaManager::new(config);
        
        // Allocate on preferred node
        let nodes = manager.allocate("vm1", 4096, Some(0)).unwrap();
        assert_eq!(nodes, vec![0]);
        
        // Allocate across nodes
        let nodes = manager.allocate("vm2", 12288, None).unwrap();
        assert!(nodes.len() >= 2);
    }
    
    #[test]
    fn test_overcommit_manager() {
        let pool = Arc::new(MemoryPool::new(16384));
        let manager = MemoryOvercommitManager::new(pool);
        
        manager.set_ratio(2.0);
        
        let effective = manager.effective_available();
        assert_eq!(effective, 32768);
        
        assert!(manager.can_allocate(20000));
    }
}
