//! Cluster Management
//!
//! Multi-node cluster features including:
//! - Node discovery and membership
//! - High availability (HA)
//! - Distributed Resource Scheduler (DRS)
//! - Fault tolerance

use std::collections::HashMap;
use std::sync::{Arc, RwLock, atomic::{AtomicU64, Ordering}};
use std::time::Instant;

/// Cluster manager
pub struct ClusterManager {
    nodes: RwLock<HashMap<String, ClusterNode>>,
    config: RwLock<ClusterConfig>,
    leader: RwLock<Option<String>>,
    stats: RwLock<ClusterStats>,
}

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub name: String,
    pub ha_enabled: bool,
    pub drs_enabled: bool,
    pub drs_threshold: f64,
    pub heartbeat_interval_ms: u64,
    pub failover_timeout_ms: u64,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            name: "default-cluster".to_string(),
            ha_enabled: true,
            drs_enabled: true,
            drs_threshold: 0.8,
            heartbeat_interval_ms: 1000,
            failover_timeout_ms: 30000,
        }
    }
}

/// Cluster node
#[derive(Debug, Clone)]
pub struct ClusterNode {
    pub id: String,
    pub hostname: String,
    pub address: String,
    pub status: NodeStatus,
    pub role: NodeRole,
    pub resources: NodeResources,
    pub last_heartbeat: Option<Instant>,
    pub joined_at: Instant,
}

/// Node status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Online,
    Offline,
    Maintenance,
    Joining,
    Leaving,
}

/// Node role
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Follower,
    Witness,
}

/// Node resources
#[derive(Debug, Clone, Default)]
pub struct NodeResources {
    pub cpu_cores: u32,
    pub cpu_used: f64,
    pub memory_total: u64,
    pub memory_used: u64,
    pub storage_total: u64,
    pub storage_used: u64,
    pub vm_count: u32,
}

/// Cluster statistics
#[derive(Debug, Clone, Default)]
pub struct ClusterStats {
    pub total_nodes: u32,
    pub online_nodes: u32,
    pub total_vms: u32,
    pub running_vms: u32,
    pub total_cpu_cores: u32,
    pub total_memory: u64,
    pub total_storage: u64,
}

impl ClusterManager {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            config: RwLock::new(ClusterConfig::default()),
            leader: RwLock::new(None),
            stats: RwLock::new(ClusterStats::default()),
        }
    }
    
    pub fn configure(&self, config: ClusterConfig) {
        *self.config.write().unwrap() = config;
    }
    
    pub fn add_node(&self, node: ClusterNode) {
        let mut nodes = self.nodes.write().unwrap();
        
        // First node becomes leader
        if nodes.is_empty() {
            *self.leader.write().unwrap() = Some(node.id.clone());
        }
        
        nodes.insert(node.id.clone(), node);
        self.update_stats();
    }
    
    pub fn remove_node(&self, id: &str) -> Option<ClusterNode> {
        let node = self.nodes.write().unwrap().remove(id);
        
        // Elect new leader if leader removed
        if self.leader.read().unwrap().as_deref() == Some(id) {
            self.elect_leader();
        }
        
        self.update_stats();
        node
    }
    
    pub fn get_node(&self, id: &str) -> Option<ClusterNode> {
        self.nodes.read().unwrap().get(id).cloned()
    }
    
    pub fn list_nodes(&self) -> Vec<ClusterNode> {
        self.nodes.read().unwrap().values().cloned().collect()
    }
    
    pub fn get_leader(&self) -> Option<String> {
        self.leader.read().unwrap().clone()
    }
    
    pub fn elect_leader(&self) {
        let nodes = self.nodes.read().unwrap();
        let new_leader = nodes.values()
            .filter(|n| n.status == NodeStatus::Online && n.role != NodeRole::Witness)
            .min_by_key(|n| n.resources.cpu_used as u64)
            .map(|n| n.id.clone());
        
        *self.leader.write().unwrap() = new_leader;
    }
    
    pub fn update_heartbeat(&self, id: &str) {
        if let Some(node) = self.nodes.write().unwrap().get_mut(id) {
            node.last_heartbeat = Some(Instant::now());
            if node.status == NodeStatus::Offline {
                node.status = NodeStatus::Online;
            }
        }
    }
    
    pub fn check_node_health(&self) {
        let timeout = self.config.read().unwrap().failover_timeout_ms;
        let mut nodes = self.nodes.write().unwrap();
        
        for node in nodes.values_mut() {
            if let Some(last) = node.last_heartbeat {
                if last.elapsed().as_millis() > timeout as u128 {
                    node.status = NodeStatus::Offline;
                }
            }
        }
    }
    
    pub fn find_best_node_for_vm(&self, vcpus: u32, memory_mb: u64) -> Option<String> {
        let nodes = self.nodes.read().unwrap();
        nodes.values()
            .filter(|n| {
                n.status == NodeStatus::Online &&
                n.resources.cpu_cores - (n.resources.cpu_used as u32) >= vcpus &&
                n.resources.memory_total - n.resources.memory_used >= memory_mb * 1024 * 1024
            })
            .min_by_key(|n| (n.resources.cpu_used * 100.0) as u64)
            .map(|n| n.id.clone())
    }
    
    fn update_stats(&self) {
        let nodes = self.nodes.read().unwrap();
        let mut stats = ClusterStats::default();
        
        for node in nodes.values() {
            stats.total_nodes += 1;
            if node.status == NodeStatus::Online {
                stats.online_nodes += 1;
            }
            stats.total_cpu_cores += node.resources.cpu_cores;
            stats.total_memory += node.resources.memory_total;
            stats.total_storage += node.resources.storage_total;
            stats.total_vms += node.resources.vm_count;
        }
        
        *self.stats.write().unwrap() = stats;
    }
    
    pub fn stats(&self) -> ClusterStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for ClusterManager {
    fn default() -> Self { Self::new() }
}
