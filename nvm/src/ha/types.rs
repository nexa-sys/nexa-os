//! HA Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HA Error
#[derive(Debug)]
pub enum HaError {
    /// Node not found
    NodeNotFound(NodeId),
    /// Resource not found
    ResourceNotFound(String),
    /// Quorum lost
    QuorumLost,
    /// No healthy nodes
    NoHealthyNodes,
    /// Fencing failed
    FencingFailed(String),
    /// Election failed
    ElectionFailed(String),
    /// State machine error
    StateMachine(String),
    /// IO error
    Io(std::io::Error),
}

impl std::fmt::Display for HaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeNotFound(id) => write!(f, "Node not found: {}", id),
            Self::ResourceNotFound(id) => write!(f, "Resource not found: {}", id),
            Self::QuorumLost => write!(f, "Quorum lost"),
            Self::NoHealthyNodes => write!(f, "No healthy nodes available"),
            Self::FencingFailed(s) => write!(f, "Fencing failed: {}", s),
            Self::ElectionFailed(s) => write!(f, "Election failed: {}", s),
            Self::StateMachine(s) => write!(f, "State machine error: {}", s),
            Self::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for HaError {}

impl From<std::io::Error> for HaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Node ID type
pub type NodeId = String;

/// HA cluster state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClusterState {
    /// Cluster is healthy
    Healthy,
    /// Cluster is degraded (some nodes down)
    Degraded,
    /// Cluster has lost quorum
    NoQuorum,
    /// Split-brain detected
    SplitBrain,
    /// Cluster is initializing
    Initializing,
}

/// Node role in HA cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    /// Leader node (Raft leader)
    Leader,
    /// Follower node
    Follower,
    /// Candidate (during election)
    Candidate,
    /// Observer (non-voting)
    Observer,
}

/// Node health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeHealth {
    Online,
    Offline,
    Unknown,
    Fenced,
    Maintenance,
    Healthy,
    Unhealthy,
    Failed,
}

/// HA node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaNode {
    pub id: NodeId,
    pub hostname: String,
    pub address: String,
    pub port: u16,
    pub role: NodeRole,
    pub health: NodeHealth,
    pub last_seen: u64,
    pub term: u64,
    pub vote_granted: bool,
    pub resources: Vec<HaResource>,
    pub priority: u32,
}

/// HA-managed resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaResource {
    pub id: String,
    pub resource_type: ResourceType,
    pub state: ResourceState,
    pub node_id: Option<NodeId>,
    pub target_node_id: Option<NodeId>,
    pub restart_policy: RestartPolicy,
    pub max_restarts: u32,
    pub restart_count: u32,
    pub last_state_change: u64,
}

/// Resource types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    Vm,
    Container,
    Service,
    VirtualIp,
}

/// Resource state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceState {
    Started,
    Stopped,
    Starting,
    Stopping,
    Migrating,
    Error,
    Unknown,
}

/// Restart policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
    Manual,
}

/// HA configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaConfig {
    /// Enable HA
    pub enabled: bool,
    /// Heartbeat interval (ms)
    pub heartbeat_interval_ms: u64,
    /// Heartbeat timeout (seconds) - for failover manager
    pub heartbeat_timeout: u32,
    /// Maximum consecutive failures before fencing
    pub max_failures: u32,
    /// Election timeout range (ms)
    pub election_timeout_min_ms: u64,
    pub election_timeout_max_ms: u64,
    /// Node failure timeout (ms)
    pub failure_timeout_ms: u64,
    /// Fencing enabled
    pub fencing_enabled: bool,
    /// Fencing method
    pub fencing_method: FencingMethod,
    /// Quorum policy
    pub quorum_policy: QuorumPolicy,
    /// Migration on failover
    pub migrate_on_failover: bool,
    /// Watchdog enabled
    pub watchdog_enabled: bool,
    /// Data directory
    pub data_dir: String,
}

impl Default for HaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            heartbeat_interval_ms: 1000,
            heartbeat_timeout: 30,
            max_failures: 3,
            election_timeout_min_ms: 5000,
            election_timeout_max_ms: 10000,
            failure_timeout_ms: 30000,
            fencing_enabled: true,
            fencing_method: FencingMethod::Ipmi,
            quorum_policy: QuorumPolicy::Majority,
            migrate_on_failover: true,
            watchdog_enabled: true,
            data_dir: "/var/lib/nvm/ha".to_string(),
        }
    }
}

/// Fencing method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FencingMethod {
    Ipmi,
    Pdu,
    Ssh,
    Sbd,
    Custom,
    None,
}

/// Quorum policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuorumPolicy {
    /// Simple majority
    Majority,
    /// All nodes must be online
    All,
    /// Custom quorum device
    QuorumDevice,
    /// Ignore quorum (dangerous)
    Ignore,
}

/// HA event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEvent {
    pub timestamp: u64,
    pub event_type: HaEventType,
    pub node_id: Option<NodeId>,
    pub resource_id: Option<String>,
    pub message: String,
    pub details: HashMap<String, String>,
}

/// HA event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaEventType {
    LeaderElected,
    LeaderLost,
    NodeJoined,
    NodeLeft,
    NodeFailed,
    NodeFenced,
    ResourceStarted,
    ResourceStopped,
    ResourceMigrated,
    ResourceFailed,
    QuorumGained,
    QuorumLost,
    SplitBrainDetected,
    SplitBrainResolved,
}

/// Raft log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub command: Command,
}

/// Raft commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    /// Add node to cluster
    AddNode(HaNode),
    /// Remove node from cluster
    RemoveNode(NodeId),
    /// Update node state
    UpdateNode { node_id: NodeId, health: NodeHealth },
    /// Add resource
    AddResource(HaResource),
    /// Remove resource
    RemoveResource(String),
    /// Start resource
    StartResource { resource_id: String, node_id: NodeId },
    /// Stop resource
    StopResource(String),
    /// Migrate resource
    MigrateResource { resource_id: String, from: NodeId, to: NodeId },
    /// No-op (for leader election)
    Noop,
}
