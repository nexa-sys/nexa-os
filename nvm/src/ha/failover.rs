//! Automatic Failover Management

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Failover manager
pub struct FailoverManager {
    /// HA configuration
    config: HaConfig,
    /// Node health status
    node_health: HashMap<NodeId, NodeHealthInfo>,
    /// Resource assignments
    resource_assignments: HashMap<ResourceId, NodeId>,
    /// Failover history
    history: Vec<FailoverEvent>,
    /// Fencing manager
    fencing: FencingManager,
}

type ResourceId = String;

/// Node health info
#[derive(Debug, Clone)]
pub struct NodeHealthInfo {
    pub node_id: NodeId,
    pub health: NodeHealth,
    pub last_heartbeat: u64,
    pub consecutive_failures: u32,
    pub resources: Vec<ResourceId>,
}

impl FailoverManager {
    pub fn new(config: HaConfig) -> Self {
        Self {
            config,
            node_health: HashMap::new(),
            resource_assignments: HashMap::new(),
            history: Vec::new(),
            fencing: FencingManager::default(),
        }
    }

    /// Register a node
    pub fn register_node(&mut self, node_id: NodeId) {
        self.node_health.insert(node_id.clone(), NodeHealthInfo {
            node_id,
            health: NodeHealth::Unknown,
            last_heartbeat: now(),
            consecutive_failures: 0,
            resources: Vec::new(),
        });
    }

    /// Process heartbeat from a node
    pub fn heartbeat(&mut self, node_id: &NodeId) -> Result<(), HaError> {
        let info = self.node_health.get_mut(node_id)
            .ok_or_else(|| HaError::NodeNotFound(node_id.clone()))?;
        
        info.last_heartbeat = now();
        info.consecutive_failures = 0;
        info.health = NodeHealth::Healthy;
        
        Ok(())
    }

    /// Check all nodes and trigger failover if needed
    pub fn check_nodes(&mut self) -> Vec<FailoverAction> {
        let now = now();
        let timeout = self.config.heartbeat_timeout as u64;
        let max_failures = self.config.max_failures;
        let mut actions = Vec::new();

        let failed_nodes: Vec<NodeId> = self.node_health.iter()
            .filter(|(_, info)| {
                now.saturating_sub(info.last_heartbeat) > timeout
            })
            .map(|(id, _)| id.clone())
            .collect();

        for node_id in failed_nodes {
            if let Some(info) = self.node_health.get_mut(&node_id) {
                info.consecutive_failures += 1;
                info.health = NodeHealth::Unhealthy;

                if info.consecutive_failures >= max_failures {
                    info.health = NodeHealth::Failed;
                    
                    // Collect resources to migrate
                    let resources = info.resources.clone();
                    for resource_id in resources {
                        actions.push(FailoverAction::MigrateResource {
                            resource_id,
                            from_node: node_id.clone(),
                            reason: "Node failed".into(),
                        });
                    }

                    // Fence the node if configured
                    if !matches!(self.config.fencing_method, FencingMethod::None) {
                        actions.push(FailoverAction::FenceNode {
                            node_id: node_id.clone(),
                            reason: "Node unresponsive".into(),
                        });
                    }
                }
            }
        }

        actions
    }

    /// Execute a failover action
    pub fn execute(&mut self, action: FailoverAction) -> Result<FailoverResult, HaError> {
        match action {
            FailoverAction::MigrateResource { resource_id, from_node, reason } => {
                self.migrate_resource(&resource_id, &from_node, &reason)
            }
            FailoverAction::FenceNode { node_id, reason } => {
                self.fence_node(&node_id, &reason)
            }
            FailoverAction::RestartResource { resource_id, node_id } => {
                self.restart_resource(&resource_id, &node_id)
            }
        }
    }

    /// Migrate a resource to another node
    fn migrate_resource(&mut self, resource_id: &str, from_node: &NodeId, reason: &str) 
        -> Result<FailoverResult, HaError> 
    {
        // Find a healthy target node
        let target_node = self.find_target_node(resource_id)?;

        // Remove from old node
        if let Some(info) = self.node_health.get_mut(from_node) {
            info.resources.retain(|r| r != resource_id);
        }

        // Assign to new node
        if let Some(info) = self.node_health.get_mut(&target_node) {
            info.resources.push(resource_id.to_string());
        }
        self.resource_assignments.insert(resource_id.to_string(), target_node.clone());

        let event = FailoverEvent {
            timestamp: now(),
            event_type: FailoverEventType::ResourceMigrated,
            resource_id: Some(resource_id.to_string()),
            from_node: Some(from_node.clone()),
            to_node: Some(target_node.clone()),
            reason: reason.to_string(),
            success: true,
        };
        self.history.push(event);

        Ok(FailoverResult::ResourceMigrated {
            resource_id: resource_id.to_string(),
            to_node: target_node,
        })
    }

    /// Find best target node for a resource
    fn find_target_node(&self, _resource_id: &str) -> Result<NodeId, HaError> {
        // Simple: find healthy node with least resources
        self.node_health.iter()
            .filter(|(_, info)| matches!(info.health, NodeHealth::Healthy))
            .min_by_key(|(_, info)| info.resources.len())
            .map(|(id, _)| id.clone())
            .ok_or(HaError::NoHealthyNodes)
    }

    /// Fence a failed node
    fn fence_node(&mut self, node_id: &NodeId, reason: &str) -> Result<FailoverResult, HaError> {
        self.fencing.fence(node_id, reason)?;

        if let Some(info) = self.node_health.get_mut(node_id) {
            info.health = NodeHealth::Fenced;
        }

        let event = FailoverEvent {
            timestamp: now(),
            event_type: FailoverEventType::NodeFenced,
            resource_id: None,
            from_node: Some(node_id.clone()),
            to_node: None,
            reason: reason.to_string(),
            success: true,
        };
        self.history.push(event);

        Ok(FailoverResult::NodeFenced { node_id: node_id.clone() })
    }

    /// Restart a resource on the same node
    fn restart_resource(&mut self, resource_id: &str, node_id: &NodeId) 
        -> Result<FailoverResult, HaError> 
    {
        let event = FailoverEvent {
            timestamp: now(),
            event_type: FailoverEventType::ResourceRestarted,
            resource_id: Some(resource_id.to_string()),
            from_node: Some(node_id.clone()),
            to_node: Some(node_id.clone()),
            reason: "Resource restart requested".to_string(),
            success: true,
        };
        self.history.push(event);

        Ok(FailoverResult::ResourceRestarted {
            resource_id: resource_id.to_string(),
            node_id: node_id.clone(),
        })
    }

    /// Assign a resource to a node
    pub fn assign_resource(&mut self, resource_id: String, node_id: NodeId) -> Result<(), HaError> {
        if !self.node_health.contains_key(&node_id) {
            return Err(HaError::NodeNotFound(node_id));
        }

        if let Some(info) = self.node_health.get_mut(&node_id) {
            info.resources.push(resource_id.clone());
        }
        self.resource_assignments.insert(resource_id, node_id);
        
        Ok(())
    }

    /// Get node health
    pub fn get_health(&self, node_id: &NodeId) -> Option<&NodeHealthInfo> {
        self.node_health.get(node_id)
    }

    /// Get all healthy nodes
    pub fn healthy_nodes(&self) -> Vec<&NodeId> {
        self.node_health.iter()
            .filter(|(_, info)| matches!(info.health, NodeHealth::Healthy))
            .map(|(id, _)| id)
            .collect()
    }

    /// Get failover history
    pub fn history(&self) -> &[FailoverEvent] {
        &self.history
    }

    /// Get fencing manager
    pub fn fencing_mut(&mut self) -> &mut FencingManager {
        &mut self.fencing
    }
}

/// Failover action
#[derive(Debug, Clone)]
pub enum FailoverAction {
    MigrateResource {
        resource_id: String,
        from_node: NodeId,
        reason: String,
    },
    FenceNode {
        node_id: NodeId,
        reason: String,
    },
    RestartResource {
        resource_id: String,
        node_id: NodeId,
    },
}

/// Failover result
#[derive(Debug, Clone)]
pub enum FailoverResult {
    ResourceMigrated {
        resource_id: String,
        to_node: NodeId,
    },
    NodeFenced {
        node_id: NodeId,
    },
    ResourceRestarted {
        resource_id: String,
        node_id: NodeId,
    },
}

/// Failover event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverEvent {
    pub timestamp: u64,
    pub event_type: FailoverEventType,
    pub resource_id: Option<String>,
    pub from_node: Option<NodeId>,
    pub to_node: Option<NodeId>,
    pub reason: String,
    pub success: bool,
}

/// Failover event type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailoverEventType {
    ResourceMigrated,
    ResourceRestarted,
    ResourceFailed,
    NodeFenced,
    NodeRecovered,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Default for FailoverManager {
    fn default() -> Self {
        Self::new(HaConfig::default())
    }
}
