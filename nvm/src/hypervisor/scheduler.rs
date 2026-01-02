//! VM Scheduler
//!
//! This module provides enterprise VM scheduling features including:
//! - Fair share scheduling
//! - Resource reservation and limits
//! - CPU/Memory affinity
//! - NUMA-aware scheduling
//! - Load balancing across hosts

use std::collections::{HashMap, HashSet, BinaryHeap, VecDeque};
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Instant, Duration};
use std::cmp::Ordering as CmpOrdering;

use super::core::{VmId, VmStatus, HypervisorError, HypervisorResult};

// ============================================================================
// VM Scheduler
// ============================================================================

/// VM scheduler for managing VM execution
pub struct VmScheduler {
    /// Scheduler policy
    policy: RwLock<SchedulerPolicy>,
    /// VM scheduling entries
    entries: RwLock<HashMap<VmId, SchedulingEntry>>,
    /// Run queue (ready VMs)
    run_queue: Mutex<BinaryHeap<SchedulingPriority>>,
    /// Blocked VMs
    blocked: RwLock<HashSet<VmId>>,
    /// Resource pools
    resource_pools: RwLock<HashMap<String, ResourcePool>>,
    /// Affinity rules
    affinity_rules: RwLock<Vec<AffinityRule>>,
    /// Statistics
    stats: RwLock<SchedulerStats>,
    /// Configuration
    config: RwLock<SchedulerConfig>,
    /// Running flag
    running: AtomicBool,
}

/// Scheduler policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerPolicy {
    /// Round-robin scheduling
    RoundRobin,
    /// Fair share (proportional to shares)
    FairShare,
    /// Priority-based
    Priority,
    /// Completely Fair Scheduler (CFS-like)
    Cfs,
    /// Real-time scheduling
    RealTime,
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self::FairShare
    }
}

/// Scheduling entry for a VM
#[derive(Debug, Clone)]
pub struct SchedulingEntry {
    pub vm_id: VmId,
    /// CPU shares (relative weight, default 1000)
    pub cpu_shares: u32,
    /// CPU reservation (MHz)
    pub cpu_reservation: u64,
    /// CPU limit (MHz, 0 = unlimited)
    pub cpu_limit: u64,
    /// Memory shares
    pub memory_shares: u32,
    /// Memory reservation (bytes)
    pub memory_reservation: u64,
    /// Priority (0-100, higher = more important)
    pub priority: u32,
    /// Nice value (-20 to 19)
    pub nice: i32,
    /// Resource pool
    pub resource_pool: Option<String>,
    /// CPU affinity mask
    pub cpu_affinity: Option<Vec<u32>>,
    /// NUMA node affinity
    pub numa_affinity: Option<Vec<u32>>,
    /// Virtual runtime (for CFS)
    pub vruntime: u64,
    /// Accumulated CPU time
    pub cpu_time: u64,
    /// Last scheduled time
    pub last_scheduled: Option<Instant>,
    /// Time slice used
    pub time_slice_used: Duration,
}

impl Default for SchedulingEntry {
    fn default() -> Self {
        Self {
            vm_id: VmId::new(0),
            cpu_shares: 1000,
            cpu_reservation: 0,
            cpu_limit: 0,
            memory_shares: 1000,
            memory_reservation: 0,
            priority: 50,
            nice: 0,
            resource_pool: None,
            cpu_affinity: None,
            numa_affinity: None,
            vruntime: 0,
            cpu_time: 0,
            last_scheduled: None,
            time_slice_used: Duration::ZERO,
        }
    }
}

/// Scheduling priority wrapper for heap
#[derive(Debug, Clone, Eq, PartialEq)]
struct SchedulingPriority {
    vm_id: VmId,
    priority: u32,
    vruntime: u64,
}

impl Ord for SchedulingPriority {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Higher priority first (reverse order for max-heap), lower vruntime first
        self.priority.cmp(&other.priority)
            .then_with(|| other.vruntime.cmp(&self.vruntime))
    }
}

impl PartialOrd for SchedulingPriority {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

/// Resource pool for grouping VMs
#[derive(Debug, Clone)]
pub struct ResourcePool {
    pub name: String,
    /// CPU shares for the pool
    pub cpu_shares: u32,
    /// CPU reservation (MHz)
    pub cpu_reservation: u64,
    /// CPU limit (MHz)
    pub cpu_limit: u64,
    /// Memory shares
    pub memory_shares: u32,
    /// Memory reservation (bytes)
    pub memory_reservation: u64,
    /// Memory limit (bytes)
    pub memory_limit: u64,
    /// Member VMs
    pub members: HashSet<VmId>,
    /// Expandable reservation
    pub expandable: bool,
}

impl Default for ResourcePool {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            cpu_shares: 4000,
            cpu_reservation: 0,
            cpu_limit: 0,
            memory_shares: 4000,
            memory_reservation: 0,
            memory_limit: 0,
            members: HashSet::new(),
            expandable: true,
        }
    }
}

/// Affinity/anti-affinity rule
#[derive(Debug, Clone)]
pub struct AffinityRule {
    pub name: String,
    pub rule_type: AffinityType,
    pub vms: Vec<VmId>,
    pub hosts: Option<Vec<String>>,
    pub enabled: bool,
}

/// Affinity rule type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffinityType {
    /// VMs should run together
    Affinity,
    /// VMs should run apart
    AntiAffinity,
    /// VM should run on specific hosts
    HostAffinity,
    /// VM should not run on specific hosts
    HostAntiAffinity,
}

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Default time slice (milliseconds)
    pub time_slice_ms: u64,
    /// Minimum time slice (milliseconds)
    pub min_time_slice_ms: u64,
    /// Maximum time slice (milliseconds)
    pub max_time_slice_ms: u64,
    /// Scheduler tick interval (milliseconds)
    pub tick_interval_ms: u64,
    /// Enable CPU burst
    pub cpu_burst: bool,
    /// CPU burst limit
    pub cpu_burst_limit: f64,
    /// Enable NUMA balancing
    pub numa_balancing: bool,
    /// Load balance interval (seconds)
    pub load_balance_interval: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            time_slice_ms: 100,
            min_time_slice_ms: 10,
            max_time_slice_ms: 1000,
            tick_interval_ms: 10,
            cpu_burst: true,
            cpu_burst_limit: 2.0,
            numa_balancing: true,
            load_balance_interval: 60,
        }
    }
}

/// Scheduler statistics
#[derive(Debug, Clone, Default)]
pub struct SchedulerStats {
    pub total_scheduled: u64,
    pub context_switches: u64,
    pub voluntary_switches: u64,
    pub involuntary_switches: u64,
    pub migrations: u64,
    pub load_balances: u64,
    pub average_latency_us: u64,
    pub average_time_slice_ms: u64,
}

impl VmScheduler {
    pub fn new() -> Self {
        Self {
            policy: RwLock::new(SchedulerPolicy::default()),
            entries: RwLock::new(HashMap::new()),
            run_queue: Mutex::new(BinaryHeap::new()),
            blocked: RwLock::new(HashSet::new()),
            resource_pools: RwLock::new(HashMap::new()),
            affinity_rules: RwLock::new(Vec::new()),
            stats: RwLock::new(SchedulerStats::default()),
            config: RwLock::new(SchedulerConfig::default()),
            running: AtomicBool::new(false),
        }
    }
    
    /// Set scheduler policy
    pub fn set_policy(&self, policy: SchedulerPolicy) {
        *self.policy.write().unwrap() = policy;
    }
    
    /// Configure scheduler
    pub fn configure(&self, config: SchedulerConfig) {
        *self.config.write().unwrap() = config;
    }
    
    // ========== VM Management ==========
    
    /// Register VM with scheduler
    pub fn register_vm(&self, vm_id: VmId, entry: SchedulingEntry) {
        let mut entry = entry;
        entry.vm_id = vm_id;
        
        self.entries.write().unwrap().insert(vm_id, entry.clone());
        
        // Add to resource pool if specified
        if let Some(ref pool_name) = entry.resource_pool {
            if let Some(pool) = self.resource_pools.write().unwrap().get_mut(pool_name) {
                pool.members.insert(vm_id);
            }
        }
    }
    
    /// Unregister VM from scheduler
    pub fn unregister_vm(&self, vm_id: VmId) {
        // Remove from resource pool
        if let Some(entry) = self.entries.read().unwrap().get(&vm_id) {
            if let Some(ref pool_name) = entry.resource_pool {
                if let Some(pool) = self.resource_pools.write().unwrap().get_mut(pool_name) {
                    pool.members.remove(&vm_id);
                }
            }
        }
        
        self.entries.write().unwrap().remove(&vm_id);
        self.blocked.write().unwrap().remove(&vm_id);
        
        // Remove from run queue
        let mut queue = self.run_queue.lock().unwrap();
        let items: Vec<_> = queue.drain().filter(|p| p.vm_id != vm_id).collect();
        for item in items {
            queue.push(item);
        }
    }
    
    /// Make VM ready (add to run queue)
    pub fn make_ready(&self, vm_id: VmId) {
        self.blocked.write().unwrap().remove(&vm_id);
        
        if let Some(entry) = self.entries.read().unwrap().get(&vm_id) {
            let prio = SchedulingPriority {
                vm_id,
                priority: entry.priority,
                vruntime: entry.vruntime,
            };
            self.run_queue.lock().unwrap().push(prio);
        }
    }
    
    /// Block VM (remove from run queue)
    pub fn block_vm(&self, vm_id: VmId) {
        self.blocked.write().unwrap().insert(vm_id);
        
        // Remove from run queue
        let mut queue = self.run_queue.lock().unwrap();
        let items: Vec<_> = queue.drain().filter(|p| p.vm_id != vm_id).collect();
        for item in items {
            queue.push(item);
        }
    }
    
    // ========== Scheduling ==========
    
    /// Get next VM to run
    pub fn schedule(&self) -> Option<VmId> {
        let policy = *self.policy.read().unwrap();
        
        match policy {
            SchedulerPolicy::RoundRobin => self.schedule_round_robin(),
            SchedulerPolicy::FairShare => self.schedule_fair_share(),
            SchedulerPolicy::Priority => self.schedule_priority(),
            SchedulerPolicy::Cfs => self.schedule_cfs(),
            SchedulerPolicy::RealTime => self.schedule_realtime(),
        }
    }
    
    fn schedule_round_robin(&self) -> Option<VmId> {
        let mut queue = self.run_queue.lock().unwrap();
        
        if let Some(prio) = queue.pop() {
            // Re-add to back of queue
            queue.push(prio.clone());
            
            self.stats.write().unwrap().total_scheduled += 1;
            Some(prio.vm_id)
        } else {
            None
        }
    }
    
    fn schedule_fair_share(&self) -> Option<VmId> {
        let queue = self.run_queue.lock().unwrap();
        let entries = self.entries.read().unwrap();
        
        // Calculate total shares
        let total_shares: u32 = queue.iter()
            .filter_map(|p| entries.get(&p.vm_id))
            .map(|e| e.cpu_shares)
            .sum();
        
        if total_shares == 0 {
            return None;
        }
        
        // Find VM with lowest cpu_time / shares ratio
        let mut best: Option<(VmId, f64)> = None;
        
        for prio in queue.iter() {
            if let Some(entry) = entries.get(&prio.vm_id) {
                let ratio = entry.cpu_time as f64 / entry.cpu_shares as f64;
                
                match &best {
                    None => best = Some((prio.vm_id, ratio)),
                    Some((_, best_ratio)) if ratio < *best_ratio => {
                        best = Some((prio.vm_id, ratio));
                    }
                    _ => {}
                }
            }
        }
        
        if let Some((vm_id, _)) = best {
            self.stats.write().unwrap().total_scheduled += 1;
            Some(vm_id)
        } else {
            None
        }
    }
    
    fn schedule_priority(&self) -> Option<VmId> {
        let mut queue = self.run_queue.lock().unwrap();
        
        // BinaryHeap already ordered by priority
        if let Some(prio) = queue.pop() {
            queue.push(prio.clone());
            self.stats.write().unwrap().total_scheduled += 1;
            Some(prio.vm_id)
        } else {
            None
        }
    }
    
    fn schedule_cfs(&self) -> Option<VmId> {
        let queue = self.run_queue.lock().unwrap();
        let entries = self.entries.read().unwrap();
        
        // Find VM with smallest virtual runtime
        let mut best: Option<(VmId, u64)> = None;
        
        for prio in queue.iter() {
            if let Some(entry) = entries.get(&prio.vm_id) {
                match &best {
                    None => best = Some((prio.vm_id, entry.vruntime)),
                    Some((_, best_vruntime)) if entry.vruntime < *best_vruntime => {
                        best = Some((prio.vm_id, entry.vruntime));
                    }
                    _ => {}
                }
            }
        }
        
        if let Some((vm_id, _)) = best {
            self.stats.write().unwrap().total_scheduled += 1;
            Some(vm_id)
        } else {
            None
        }
    }
    
    fn schedule_realtime(&self) -> Option<VmId> {
        // Real-time: strict priority ordering
        self.schedule_priority()
    }
    
    /// Report VM execution time
    pub fn report_execution(&self, vm_id: VmId, duration: Duration) {
        let mut entries = self.entries.write().unwrap();
        
        if let Some(entry) = entries.get_mut(&vm_id) {
            entry.cpu_time += duration.as_micros() as u64;
            entry.time_slice_used += duration;
            entry.last_scheduled = Some(Instant::now());
            
            // Update virtual runtime (CFS)
            let weight = 1024u64 * 1024 / entry.cpu_shares as u64;
            entry.vruntime += duration.as_nanos() as u64 * weight / 1024;
        }
    }
    
    /// Check if time slice expired
    pub fn time_slice_expired(&self, vm_id: VmId) -> bool {
        let config = self.config.read().unwrap();
        let entries = self.entries.read().unwrap();
        
        if let Some(entry) = entries.get(&vm_id) {
            let time_slice = self.calculate_time_slice(entry);
            entry.time_slice_used >= time_slice
        } else {
            true
        }
    }
    
    /// Reset time slice for VM
    pub fn reset_time_slice(&self, vm_id: VmId) {
        if let Some(entry) = self.entries.write().unwrap().get_mut(&vm_id) {
            entry.time_slice_used = Duration::ZERO;
        }
    }
    
    fn calculate_time_slice(&self, entry: &SchedulingEntry) -> Duration {
        let config = self.config.read().unwrap();
        
        // Base time slice adjusted by nice value
        let base = config.time_slice_ms as i64;
        let adjustment = entry.nice as i64 * 5; // 5ms per nice level
        
        let slice = (base - adjustment)
            .max(config.min_time_slice_ms as i64)
            .min(config.max_time_slice_ms as i64);
        
        Duration::from_millis(slice as u64)
    }
    
    // ========== Resource Pools ==========
    
    /// Create resource pool
    pub fn create_resource_pool(&self, pool: ResourcePool) {
        self.resource_pools.write().unwrap().insert(pool.name.clone(), pool);
    }
    
    /// Delete resource pool
    pub fn delete_resource_pool(&self, name: &str) -> HypervisorResult<()> {
        let pools = self.resource_pools.read().unwrap();
        
        if let Some(pool) = pools.get(name) {
            if !pool.members.is_empty() {
                return Err(HypervisorError::SchedulerError(
                    "Cannot delete pool with member VMs".to_string()
                ));
            }
        }
        
        drop(pools);
        self.resource_pools.write().unwrap().remove(name);
        Ok(())
    }
    
    /// Move VM to resource pool
    pub fn move_to_pool(&self, vm_id: VmId, pool_name: &str) -> HypervisorResult<()> {
        let mut entries = self.entries.write().unwrap();
        let mut pools = self.resource_pools.write().unwrap();
        
        // Remove from old pool
        if let Some(entry) = entries.get(&vm_id) {
            if let Some(ref old_pool) = entry.resource_pool {
                if let Some(pool) = pools.get_mut(old_pool) {
                    pool.members.remove(&vm_id);
                }
            }
        }
        
        // Add to new pool
        if let Some(pool) = pools.get_mut(pool_name) {
            pool.members.insert(vm_id);
        } else {
            return Err(HypervisorError::SchedulerError(
                format!("Resource pool '{}' not found", pool_name)
            ));
        }
        
        // Update entry
        if let Some(entry) = entries.get_mut(&vm_id) {
            entry.resource_pool = Some(pool_name.to_string());
        }
        
        Ok(())
    }
    
    // ========== Affinity Rules ==========
    
    /// Add affinity rule
    pub fn add_affinity_rule(&self, rule: AffinityRule) {
        self.affinity_rules.write().unwrap().push(rule);
    }
    
    /// Remove affinity rule
    pub fn remove_affinity_rule(&self, name: &str) {
        self.affinity_rules.write().unwrap().retain(|r| r.name != name);
    }
    
    /// Check affinity rules for VM placement
    pub fn check_affinity(&self, vm_id: VmId, host: &str) -> bool {
        let rules = self.affinity_rules.read().unwrap();
        
        for rule in rules.iter() {
            if !rule.enabled {
                continue;
            }
            
            if !rule.vms.contains(&vm_id) {
                continue;
            }
            
            match rule.rule_type {
                AffinityType::HostAffinity => {
                    if let Some(ref hosts) = rule.hosts {
                        if !hosts.contains(&host.to_string()) {
                            return false;
                        }
                    }
                }
                AffinityType::HostAntiAffinity => {
                    if let Some(ref hosts) = rule.hosts {
                        if hosts.contains(&host.to_string()) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        
        true
    }
    
    // ========== CPU Affinity ==========
    
    /// Set CPU affinity for VM
    pub fn set_cpu_affinity(&self, vm_id: VmId, cpus: Vec<u32>) {
        if let Some(entry) = self.entries.write().unwrap().get_mut(&vm_id) {
            entry.cpu_affinity = Some(cpus);
        }
    }
    
    /// Set NUMA affinity for VM
    pub fn set_numa_affinity(&self, vm_id: VmId, nodes: Vec<u32>) {
        if let Some(entry) = self.entries.write().unwrap().get_mut(&vm_id) {
            entry.numa_affinity = Some(nodes);
        }
    }
    
    /// Get recommended CPU for VM
    pub fn get_recommended_cpu(&self, vm_id: VmId) -> Option<u32> {
        let entries = self.entries.read().unwrap();
        
        if let Some(entry) = entries.get(&vm_id) {
            if let Some(ref affinity) = entry.cpu_affinity {
                // Return first CPU in affinity mask
                return affinity.first().copied();
            }
        }
        
        None
    }
    
    // ========== Statistics ==========
    
    /// Get scheduler statistics
    pub fn stats(&self) -> SchedulerStats {
        self.stats.read().unwrap().clone()
    }
    
    /// Get scheduling entry for VM
    pub fn get_entry(&self, vm_id: VmId) -> Option<SchedulingEntry> {
        self.entries.read().unwrap().get(&vm_id).cloned()
    }
    
    /// Get run queue length
    pub fn run_queue_length(&self) -> usize {
        self.run_queue.lock().unwrap().len()
    }
    
    /// Get blocked VMs count
    pub fn blocked_count(&self) -> usize {
        self.blocked.read().unwrap().len()
    }
}

impl Default for VmScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Load Balancer
// ============================================================================

/// Load balancer for distributing VMs across hosts
pub struct LoadBalancer {
    /// Host information
    hosts: RwLock<HashMap<String, HostInfo>>,
    /// Load balance policy
    policy: RwLock<LoadBalancePolicy>,
    /// Balance threshold
    threshold: RwLock<f64>,
    /// Statistics
    stats: RwLock<LoadBalanceStats>,
}

/// Host information
#[derive(Debug, Clone)]
pub struct HostInfo {
    pub name: String,
    pub total_cpu: u64,
    pub used_cpu: u64,
    pub total_memory: u64,
    pub used_memory: u64,
    pub vms: HashSet<VmId>,
    pub available: bool,
    pub maintenance: bool,
}

/// Load balance policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalancePolicy {
    /// Balance by CPU usage
    Cpu,
    /// Balance by memory usage
    Memory,
    /// Balance by VM count
    VmCount,
    /// Combined (weighted)
    Combined,
    /// Distributed Resource Scheduler (VMware DRS style)
    Drs,
}

impl Default for LoadBalancePolicy {
    fn default() -> Self {
        Self::Drs
    }
}

/// Load balance statistics
#[derive(Debug, Clone, Default)]
pub struct LoadBalanceStats {
    pub balance_checks: u64,
    pub migrations_triggered: u64,
    pub migrations_completed: u64,
    pub imbalance_detected: u64,
}

/// Migration recommendation
#[derive(Debug, Clone)]
pub struct MigrationRecommendation {
    pub vm_id: VmId,
    pub source_host: String,
    pub target_host: String,
    pub reason: String,
    pub priority: u32,
    pub estimated_benefit: f64,
}

impl LoadBalancer {
    pub fn new() -> Self {
        Self {
            hosts: RwLock::new(HashMap::new()),
            policy: RwLock::new(LoadBalancePolicy::default()),
            threshold: RwLock::new(0.25), // 25% imbalance threshold
            stats: RwLock::new(LoadBalanceStats::default()),
        }
    }
    
    /// Set load balance policy
    pub fn set_policy(&self, policy: LoadBalancePolicy) {
        *self.policy.write().unwrap() = policy;
    }
    
    /// Set imbalance threshold
    pub fn set_threshold(&self, threshold: f64) {
        *self.threshold.write().unwrap() = threshold;
    }
    
    /// Register host
    pub fn register_host(&self, info: HostInfo) {
        self.hosts.write().unwrap().insert(info.name.clone(), info);
    }
    
    /// Unregister host
    pub fn unregister_host(&self, name: &str) {
        self.hosts.write().unwrap().remove(name);
    }
    
    /// Update host statistics
    pub fn update_host(&self, name: &str, used_cpu: u64, used_memory: u64) {
        if let Some(host) = self.hosts.write().unwrap().get_mut(name) {
            host.used_cpu = used_cpu;
            host.used_memory = used_memory;
        }
    }
    
    /// Add VM to host
    pub fn add_vm_to_host(&self, vm_id: VmId, host: &str) {
        if let Some(host_info) = self.hosts.write().unwrap().get_mut(host) {
            host_info.vms.insert(vm_id);
        }
    }
    
    /// Remove VM from host
    pub fn remove_vm_from_host(&self, vm_id: VmId, host: &str) {
        if let Some(host_info) = self.hosts.write().unwrap().get_mut(host) {
            host_info.vms.remove(&vm_id);
        }
    }
    
    /// Get best host for new VM
    pub fn get_best_host(&self, cpu_required: u64, memory_required: u64) -> Option<String> {
        let hosts = self.hosts.read().unwrap();
        let policy = *self.policy.read().unwrap();
        
        let mut candidates: Vec<_> = hosts.values()
            .filter(|h| h.available && !h.maintenance)
            .filter(|h| h.total_cpu - h.used_cpu >= cpu_required)
            .filter(|h| h.total_memory - h.used_memory >= memory_required)
            .collect();
        
        if candidates.is_empty() {
            return None;
        }
        
        // Sort by load (ascending)
        candidates.sort_by(|a, b| {
            let load_a = self.calculate_host_load(a, policy);
            let load_b = self.calculate_host_load(b, policy);
            load_a.partial_cmp(&load_b).unwrap_or(CmpOrdering::Equal)
        });
        
        candidates.first().map(|h| h.name.clone())
    }
    
    /// Check if cluster is balanced
    pub fn is_balanced(&self) -> bool {
        let hosts = self.hosts.read().unwrap();
        let policy = *self.policy.read().unwrap();
        let threshold = *self.threshold.read().unwrap();
        
        let active_hosts: Vec<_> = hosts.values()
            .filter(|h| h.available && !h.maintenance)
            .collect();
        
        if active_hosts.len() < 2 {
            return true;
        }
        
        let loads: Vec<f64> = active_hosts.iter()
            .map(|h| self.calculate_host_load(h, policy))
            .collect();
        
        let avg_load: f64 = loads.iter().sum::<f64>() / loads.len() as f64;
        
        // Check if any host deviates more than threshold from average
        for load in &loads {
            if (load - avg_load).abs() > threshold {
                return false;
            }
        }
        
        true
    }
    
    /// Get migration recommendations
    pub fn get_recommendations(&self) -> Vec<MigrationRecommendation> {
        self.stats.write().unwrap().balance_checks += 1;
        
        let hosts = self.hosts.read().unwrap();
        let policy = *self.policy.read().unwrap();
        let threshold = *self.threshold.read().unwrap();
        
        let mut recommendations = Vec::new();
        
        let active_hosts: Vec<_> = hosts.values()
            .filter(|h| h.available && !h.maintenance)
            .collect();
        
        if active_hosts.len() < 2 {
            return recommendations;
        }
        
        // Calculate average load
        let loads: Vec<(String, f64)> = active_hosts.iter()
            .map(|h| (h.name.clone(), self.calculate_host_load(h, policy)))
            .collect();
        
        let avg_load: f64 = loads.iter().map(|(_, l)| l).sum::<f64>() / loads.len() as f64;
        
        // Find overloaded and underloaded hosts
        let overloaded: Vec<_> = loads.iter()
            .filter(|(_, load)| *load > avg_load + threshold)
            .collect();
        
        let underloaded: Vec<_> = loads.iter()
            .filter(|(_, load)| *load < avg_load - threshold)
            .collect();
        
        // Generate recommendations
        for (source, source_load) in overloaded {
            for (target, target_load) in &underloaded {
                if let Some(source_host) = hosts.get(source) {
                    for &vm_id in &source_host.vms {
                        recommendations.push(MigrationRecommendation {
                            vm_id,
                            source_host: source.clone(),
                            target_host: target.clone(),
                            reason: format!(
                                "Load imbalance: {} ({:.1}%) -> {} ({:.1}%)",
                                source, source_load * 100.0, target, target_load * 100.0
                            ),
                            priority: 50,
                            estimated_benefit: source_load - target_load,
                        });
                    }
                }
            }
        }
        
        if !recommendations.is_empty() {
            self.stats.write().unwrap().imbalance_detected += 1;
        }
        
        recommendations
    }
    
    fn calculate_host_load(&self, host: &HostInfo, policy: LoadBalancePolicy) -> f64 {
        match policy {
            LoadBalancePolicy::Cpu => {
                host.used_cpu as f64 / host.total_cpu as f64
            }
            LoadBalancePolicy::Memory => {
                host.used_memory as f64 / host.total_memory as f64
            }
            LoadBalancePolicy::VmCount => {
                host.vms.len() as f64 / 100.0 // Normalize to ~1.0
            }
            LoadBalancePolicy::Combined | LoadBalancePolicy::Drs => {
                let cpu_load = host.used_cpu as f64 / host.total_cpu as f64;
                let mem_load = host.used_memory as f64 / host.total_memory as f64;
                cpu_load * 0.5 + mem_load * 0.5
            }
        }
    }
    
    /// Get statistics
    pub fn stats(&self) -> LoadBalanceStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for LoadBalancer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scheduler_round_robin() {
        let scheduler = VmScheduler::new();
        scheduler.set_policy(SchedulerPolicy::RoundRobin);
        
        // Register VMs
        scheduler.register_vm(VmId::new(1), SchedulingEntry::default());
        scheduler.register_vm(VmId::new(2), SchedulingEntry::default());
        
        // Make ready
        scheduler.make_ready(VmId::new(1));
        scheduler.make_ready(VmId::new(2));
        
        // Schedule
        let vm1 = scheduler.schedule();
        assert!(vm1.is_some());
        
        let vm2 = scheduler.schedule();
        assert!(vm2.is_some());
    }
    
    #[test]
    fn test_scheduler_priority() {
        let scheduler = VmScheduler::new();
        scheduler.set_policy(SchedulerPolicy::Priority);
        
        // Register VMs with different priorities
        let mut entry1 = SchedulingEntry::default();
        entry1.priority = 10;
        scheduler.register_vm(VmId::new(1), entry1);
        
        let mut entry2 = SchedulingEntry::default();
        entry2.priority = 90;
        scheduler.register_vm(VmId::new(2), entry2);
        
        scheduler.make_ready(VmId::new(1));
        scheduler.make_ready(VmId::new(2));
        
        // Higher priority should be scheduled first
        let first = scheduler.schedule();
        assert_eq!(first, Some(VmId::new(2)));
    }
    
    #[test]
    fn test_resource_pool() {
        let scheduler = VmScheduler::new();
        
        // Create pool
        scheduler.create_resource_pool(ResourcePool {
            name: "production".to_string(),
            cpu_shares: 8000,
            ..Default::default()
        });
        
        // Register VM
        scheduler.register_vm(VmId::new(1), SchedulingEntry::default());
        
        // Move to pool
        scheduler.move_to_pool(VmId::new(1), "production").unwrap();
        
        let entry = scheduler.get_entry(VmId::new(1)).unwrap();
        assert_eq!(entry.resource_pool, Some("production".to_string()));
    }
    
    #[test]
    fn test_load_balancer() {
        let lb = LoadBalancer::new();
        
        // Register hosts
        lb.register_host(HostInfo {
            name: "host1".to_string(),
            total_cpu: 10000,
            used_cpu: 2000,
            total_memory: 16 * 1024 * 1024 * 1024,
            used_memory: 4 * 1024 * 1024 * 1024,
            vms: HashSet::new(),
            available: true,
            maintenance: false,
        });
        
        lb.register_host(HostInfo {
            name: "host2".to_string(),
            total_cpu: 10000,
            used_cpu: 8000,
            total_memory: 16 * 1024 * 1024 * 1024,
            used_memory: 12 * 1024 * 1024 * 1024,
            vms: HashSet::new(),
            available: true,
            maintenance: false,
        });
        
        // Get best host (should be host1)
        let best = lb.get_best_host(1000, 1024 * 1024 * 1024);
        assert_eq!(best, Some("host1".to_string()));
        
        // Check balance
        assert!(!lb.is_balanced());
    }
    
    #[test]
    fn test_cpu_affinity() {
        let scheduler = VmScheduler::new();
        
        scheduler.register_vm(VmId::new(1), SchedulingEntry::default());
        scheduler.set_cpu_affinity(VmId::new(1), vec![0, 2, 4]);
        
        let cpu = scheduler.get_recommended_cpu(VmId::new(1));
        assert_eq!(cpu, Some(0));
    }
}
