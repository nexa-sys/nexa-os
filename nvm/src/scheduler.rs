//! Resource Scheduler
//!
//! Distributed Resource Scheduler (DRS) features:
//! - Load balancing across nodes
//! - Resource reservation
//! - Affinity/anti-affinity rules
//! - Power management

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::hypervisor::VmId;

/// Resource scheduler
pub struct ResourceScheduler {
    rules: RwLock<Vec<SchedulingRule>>,
    reservations: RwLock<HashMap<VmId, ResourceReservation>>,
    config: RwLock<SchedulerConfig>,
}

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub balance_threshold: f64,
    pub check_interval_s: u64,
    pub migration_threshold: f64,
    pub power_management: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            balance_threshold: 0.25,
            check_interval_s: 300,
            migration_threshold: 0.5,
            power_management: false,
        }
    }
}

/// Scheduling rule
#[derive(Debug, Clone)]
pub struct SchedulingRule {
    pub id: u64,
    pub name: String,
    pub rule_type: RuleType,
    pub priority: u32,
    pub enabled: bool,
}

/// Rule type
#[derive(Debug, Clone)]
pub enum RuleType {
    /// VMs must run on same host
    Affinity { vm_ids: Vec<VmId> },
    /// VMs must not run on same host
    AntiAffinity { vm_ids: Vec<VmId> },
    /// VM must run on specific hosts
    HostAffinity { vm_id: VmId, host_ids: Vec<String> },
    /// VM must not run on specific hosts
    HostAntiAffinity { vm_id: VmId, host_ids: Vec<String> },
    /// Resource limit on host
    ResourceLimit { host_id: String, cpu_limit: f64, memory_limit: f64 },
}

/// Resource reservation
#[derive(Debug, Clone)]
pub struct ResourceReservation {
    pub vm_id: VmId,
    pub cpu_shares: u32,
    pub cpu_reservation_mhz: u32,
    pub cpu_limit_mhz: Option<u32>,
    pub memory_reservation_mb: u64,
    pub memory_limit_mb: Option<u64>,
    pub io_priority: IoPriority,
}

/// I/O priority
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoPriority {
    Low,
    Normal,
    High,
}

impl Default for IoPriority {
    fn default() -> Self { Self::Normal }
}

/// Scheduling decision
#[derive(Debug, Clone)]
pub struct SchedulingDecision {
    pub vm_id: VmId,
    pub recommended_host: String,
    pub score: f64,
    pub reason: String,
}

impl ResourceScheduler {
    pub fn new() -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            reservations: RwLock::new(HashMap::new()),
            config: RwLock::new(SchedulerConfig::default()),
        }
    }
    
    pub fn configure(&self, config: SchedulerConfig) {
        *self.config.write().unwrap() = config;
    }
    
    pub fn add_rule(&self, rule: SchedulingRule) {
        let mut rules = self.rules.write().unwrap();
        rules.push(rule);
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }
    
    pub fn remove_rule(&self, id: u64) {
        self.rules.write().unwrap().retain(|r| r.id != id);
    }
    
    pub fn set_reservation(&self, reservation: ResourceReservation) {
        self.reservations.write().unwrap().insert(reservation.vm_id, reservation);
    }
    
    pub fn get_reservation(&self, vm_id: VmId) -> Option<ResourceReservation> {
        self.reservations.read().unwrap().get(&vm_id).cloned()
    }
    
    pub fn remove_reservation(&self, vm_id: VmId) {
        self.reservations.write().unwrap().remove(&vm_id);
    }
    
    /// Evaluate placement for a new VM
    pub fn evaluate_placement(
        &self,
        vm_id: VmId,
        vcpus: u32,
        memory_mb: u64,
        hosts: &[(String, f64, u64)], // (id, cpu_used%, memory_available)
    ) -> Option<SchedulingDecision> {
        let rules = self.rules.read().unwrap();
        
        // Filter hosts based on rules
        let valid_hosts: Vec<_> = hosts.iter()
            .filter(|(host_id, _, mem_avail)| {
                // Check anti-affinity rules
                for rule in rules.iter() {
                    if let RuleType::HostAntiAffinity { vm_id: rule_vm, host_ids } = &rule.rule_type {
                        if *rule_vm == vm_id && host_ids.contains(host_id) {
                            return false;
                        }
                    }
                }
                // Check memory
                *mem_avail >= memory_mb * 1024 * 1024
            })
            .collect();
        
        // Score and select best host
        valid_hosts.into_iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(host_id, cpu_used, _)| SchedulingDecision {
                vm_id,
                recommended_host: host_id.clone(),
                score: 100.0 - cpu_used,
                reason: "Lowest CPU utilization".to_string(),
            })
    }
    
    pub fn list_rules(&self) -> Vec<SchedulingRule> {
        self.rules.read().unwrap().clone()
    }
}

impl Default for ResourceScheduler {
    fn default() -> Self { Self::new() }
}
