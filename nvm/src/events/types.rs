//! Event Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique event identifier
pub type EventId = u64;

/// Event severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSeverity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

/// Event category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    Vm,
    Storage,
    Network,
    Cluster,
    Security,
    Backup,
    System,
    User,
    Task,
    License,
    HighAvailability,
}

/// System event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event ID
    pub id: EventId,
    /// Event timestamp (Unix epoch seconds)
    pub timestamp: u64,
    /// Event category
    pub category: EventCategory,
    /// Event severity
    pub severity: EventSeverity,
    /// Event type/action
    pub event_type: String,
    /// Human-readable message
    pub message: String,
    /// Source node ID
    pub node_id: Option<String>,
    /// Related resource ID (VM ID, volume ID, etc.)
    pub resource_id: Option<String>,
    /// Related resource type
    pub resource_type: Option<String>,
    /// User who triggered the event (if applicable)
    pub user: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl Event {
    /// Create a new event
    pub fn new(
        category: EventCategory,
        severity: EventSeverity,
        event_type: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        
        Self {
            id: COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            category,
            severity,
            event_type: event_type.into(),
            message: message.into(),
            node_id: None,
            resource_id: None,
            resource_type: None,
            user: None,
            metadata: HashMap::new(),
        }
    }

    /// Set node ID
    pub fn with_node(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    /// Set resource info
    pub fn with_resource(mut self, resource_type: impl Into<String>, resource_id: impl Into<String>) -> Self {
        self.resource_type = Some(resource_type.into());
        self.resource_id = Some(resource_id.into());
        self
    }

    /// Set user
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Event filter for subscriptions
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Filter by categories
    pub categories: Option<Vec<EventCategory>>,
    /// Filter by minimum severity
    pub min_severity: Option<EventSeverity>,
    /// Filter by resource type
    pub resource_type: Option<String>,
    /// Filter by resource ID
    pub resource_id: Option<String>,
    /// Filter by node ID
    pub node_id: Option<String>,
}

impl EventFilter {
    /// Create filter for specific category
    pub fn category(category: EventCategory) -> Self {
        Self {
            categories: Some(vec![category]),
            ..Default::default()
        }
    }

    /// Create filter for specific resource
    pub fn resource(resource_type: impl Into<String>, resource_id: impl Into<String>) -> Self {
        Self {
            resource_type: Some(resource_type.into()),
            resource_id: Some(resource_id.into()),
            ..Default::default()
        }
    }

    /// Check if event matches filter
    pub fn matches(&self, event: &Event) -> bool {
        // Check category
        if let Some(cats) = &self.categories {
            if !cats.contains(&event.category) {
                return false;
            }
        }

        // Check severity (>= min)
        if let Some(min) = &self.min_severity {
            let event_level = severity_level(event.severity);
            let min_level = severity_level(*min);
            if event_level < min_level {
                return false;
            }
        }

        // Check resource type
        if let Some(rt) = &self.resource_type {
            if event.resource_type.as_ref() != Some(rt) {
                return false;
            }
        }

        // Check resource ID
        if let Some(rid) = &self.resource_id {
            if event.resource_id.as_ref() != Some(rid) {
                return false;
            }
        }

        // Check node ID
        if let Some(nid) = &self.node_id {
            if event.node_id.as_ref() != Some(nid) {
                return false;
            }
        }

        true
    }
}

fn severity_level(sev: EventSeverity) -> u8 {
    match sev {
        EventSeverity::Debug => 0,
        EventSeverity::Info => 1,
        EventSeverity::Warning => 2,
        EventSeverity::Error => 3,
        EventSeverity::Critical => 4,
    }
}

/// Common VM events
pub mod vm_events {
    use super::*;

    pub fn created(vm_id: &str, vm_name: &str, user: Option<&str>) -> Event {
        let mut e = Event::new(
            EventCategory::Vm,
            EventSeverity::Info,
            "vm.created",
            format!("VM '{}' created", vm_name),
        )
        .with_resource("vm", vm_id);
        
        if let Some(u) = user {
            e = e.with_user(u);
        }
        e
    }

    pub fn started(vm_id: &str, vm_name: &str) -> Event {
        Event::new(
            EventCategory::Vm,
            EventSeverity::Info,
            "vm.started",
            format!("VM '{}' started", vm_name),
        )
        .with_resource("vm", vm_id)
    }

    pub fn stopped(vm_id: &str, vm_name: &str) -> Event {
        Event::new(
            EventCategory::Vm,
            EventSeverity::Info,
            "vm.stopped",
            format!("VM '{}' stopped", vm_name),
        )
        .with_resource("vm", vm_id)
    }

    pub fn migrated(vm_id: &str, vm_name: &str, from_node: &str, to_node: &str) -> Event {
        Event::new(
            EventCategory::Vm,
            EventSeverity::Info,
            "vm.migrated",
            format!("VM '{}' migrated from {} to {}", vm_name, from_node, to_node),
        )
        .with_resource("vm", vm_id)
        .with_metadata("from_node", from_node)
        .with_metadata("to_node", to_node)
    }
}

/// Common cluster events
pub mod cluster_events {
    use super::*;

    pub fn node_joined(node_id: &str, hostname: &str) -> Event {
        Event::new(
            EventCategory::Cluster,
            EventSeverity::Info,
            "cluster.node_joined",
            format!("Node '{}' joined the cluster", hostname),
        )
        .with_node(node_id)
    }

    pub fn node_left(node_id: &str, hostname: &str) -> Event {
        Event::new(
            EventCategory::Cluster,
            EventSeverity::Warning,
            "cluster.node_left",
            format!("Node '{}' left the cluster", hostname),
        )
        .with_node(node_id)
    }

    pub fn quorum_lost() -> Event {
        Event::new(
            EventCategory::Cluster,
            EventSeverity::Critical,
            "cluster.quorum_lost",
            "Cluster quorum lost - operations may be unavailable",
        )
    }

    pub fn failover(failed_node: &str, services: &[String]) -> Event {
        Event::new(
            EventCategory::HighAvailability,
            EventSeverity::Warning,
            "ha.failover",
            format!("Failover initiated from node '{}'", failed_node),
        )
        .with_metadata("failed_node", failed_node)
        .with_metadata("services", services.join(","))
    }
}
