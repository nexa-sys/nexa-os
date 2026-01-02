//! Raft Consensus Implementation

use super::*;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Raft node state
pub struct RaftNode {
    /// Node ID
    id: NodeId,
    /// Current term
    current_term: RwLock<u64>,
    /// Voted for in current term
    voted_for: RwLock<Option<NodeId>>,
    /// Current role
    role: RwLock<NodeRole>,
    /// Leader ID
    leader_id: RwLock<Option<NodeId>>,
    /// Log entries
    log: RwLock<Vec<LogEntry>>,
    /// Commit index
    commit_index: RwLock<u64>,
    /// Last applied
    last_applied: RwLock<u64>,
    /// Cluster nodes
    nodes: RwLock<HashMap<NodeId, HaNode>>,
    /// Configuration
    config: HaConfig,
    /// State machine
    state_machine: Arc<dyn StateMachine>,
}

/// State machine trait
pub trait StateMachine: Send + Sync {
    fn apply(&self, command: &Command) -> Result<(), HaError>;
    fn snapshot(&self) -> Vec<u8>;
    fn restore(&self, snapshot: &[u8]) -> Result<(), HaError>;
}

impl RaftNode {
    pub fn new(
        id: NodeId,
        config: HaConfig,
        state_machine: Arc<dyn StateMachine>,
    ) -> Self {
        Self {
            id,
            current_term: RwLock::new(0),
            voted_for: RwLock::new(None),
            role: RwLock::new(NodeRole::Follower),
            leader_id: RwLock::new(None),
            log: RwLock::new(Vec::new()),
            commit_index: RwLock::new(0),
            last_applied: RwLock::new(0),
            nodes: RwLock::new(HashMap::new()),
            config,
            state_machine,
        }
    }

    /// Get current role
    pub fn role(&self) -> NodeRole {
        *self.role.read()
    }

    /// Get current term
    pub fn term(&self) -> u64 {
        *self.current_term.read()
    }

    /// Get leader ID
    pub fn leader(&self) -> Option<NodeId> {
        self.leader_id.read().clone()
    }

    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        *self.role.read() == NodeRole::Leader
    }

    /// Start election
    pub fn start_election(&self) {
        let mut term = self.current_term.write();
        *term += 1;
        
        *self.role.write() = NodeRole::Candidate;
        *self.voted_for.write() = Some(self.id.clone());
        *self.leader_id.write() = None;
        
        // In production: send RequestVote RPCs to all nodes
    }

    /// Handle vote request
    pub fn handle_vote_request(
        &self,
        term: u64,
        candidate_id: &NodeId,
        last_log_index: u64,
        last_log_term: u64,
    ) -> (u64, bool) {
        let current_term = *self.current_term.read();
        
        // Reply false if term < currentTerm
        if term < current_term {
            return (current_term, false);
        }

        // Update term if needed
        if term > current_term {
            *self.current_term.write() = term;
            *self.role.write() = NodeRole::Follower;
            *self.voted_for.write() = None;
        }

        let voted_for = self.voted_for.read();
        let can_vote = voted_for.is_none() || voted_for.as_ref() == Some(candidate_id);
        
        // Check if candidate's log is at least as up-to-date
        let log = self.log.read();
        let our_last_term = log.last().map(|e| e.term).unwrap_or(0);
        let our_last_index = log.last().map(|e| e.index).unwrap_or(0);
        
        let log_ok = last_log_term > our_last_term
            || (last_log_term == our_last_term && last_log_index >= our_last_index);

        if can_vote && log_ok {
            drop(voted_for);
            *self.voted_for.write() = Some(candidate_id.clone());
            (term, true)
        } else {
            (term, false)
        }
    }

    /// Handle append entries (heartbeat/replication)
    pub fn handle_append_entries(
        &self,
        term: u64,
        leader_id: &NodeId,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<LogEntry>,
        leader_commit: u64,
    ) -> (u64, bool) {
        let current_term = *self.current_term.read();
        
        // Reply false if term < currentTerm
        if term < current_term {
            return (current_term, false);
        }

        // Update term and role
        if term > current_term {
            *self.current_term.write() = term;
            *self.voted_for.write() = None;
        }
        
        *self.role.write() = NodeRole::Follower;
        *self.leader_id.write() = Some(leader_id.clone());

        // Verify prev log entry
        let mut log = self.log.write();
        if prev_log_index > 0 {
            if let Some(entry) = log.get(prev_log_index as usize - 1) {
                if entry.term != prev_log_term {
                    return (term, false);
                }
            } else {
                return (term, false);
            }
        }

        // Append new entries
        for entry in entries {
            let idx = entry.index as usize;
            if idx <= log.len() {
                if idx > 0 && log[idx - 1].term != entry.term {
                    log.truncate(idx - 1);
                    log.push(entry);
                }
            } else {
                log.push(entry);
            }
        }

        // Update commit index
        if leader_commit > *self.commit_index.read() {
            let last_index = log.last().map(|e| e.index).unwrap_or(0);
            *self.commit_index.write() = leader_commit.min(last_index);
        }

        (term, true)
    }

    /// Become leader
    pub fn become_leader(&self) {
        *self.role.write() = NodeRole::Leader;
        *self.leader_id.write() = Some(self.id.clone());
        
        // Append no-op entry to establish leadership
        let term = *self.current_term.read();
        let mut log = self.log.write();
        let index = log.last().map(|e| e.index + 1).unwrap_or(1);
        
        log.push(LogEntry {
            term,
            index,
            command: Command::Noop,
        });
    }

    /// Submit command (leader only)
    pub fn submit(&self, command: Command) -> Result<u64, HaError> {
        if !self.is_leader() {
            return Err(HaError::NotLeader(self.leader()));
        }

        let term = *self.current_term.read();
        let mut log = self.log.write();
        let index = log.last().map(|e| e.index + 1).unwrap_or(1);
        
        log.push(LogEntry {
            term,
            index,
            command,
        });

        // In production: replicate to followers
        Ok(index)
    }

    /// Apply committed entries to state machine
    pub fn apply_committed(&self) -> Result<(), HaError> {
        let commit_index = *self.commit_index.read();
        let mut last_applied = self.last_applied.write();
        let log = self.log.read();

        while *last_applied < commit_index {
            let idx = *last_applied as usize;
            if let Some(entry) = log.get(idx) {
                self.state_machine.apply(&entry.command)?;
                *last_applied = entry.index;
            }
        }

        Ok(())
    }

    /// Get cluster status
    pub fn cluster_status(&self) -> ClusterStatus {
        let nodes = self.nodes.read();
        let online_count = nodes.values().filter(|n| n.health == NodeHealth::Online).count();
        let total_count = nodes.len();
        
        let has_quorum = match self.config.quorum_policy {
            QuorumPolicy::Majority => online_count > total_count / 2,
            QuorumPolicy::All => online_count == total_count,
            QuorumPolicy::QuorumDevice => true, // Would check external device
            QuorumPolicy::Ignore => true,
        };

        let state = if !has_quorum {
            ClusterState::NoQuorum
        } else if online_count == total_count {
            ClusterState::Healthy
        } else {
            ClusterState::Degraded
        };

        ClusterStatus {
            state,
            leader: self.leader(),
            term: self.term(),
            nodes: nodes.values().cloned().collect(),
            has_quorum,
            commit_index: *self.commit_index.read(),
        }
    }
}

/// Cluster status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub state: ClusterState,
    pub leader: Option<NodeId>,
    pub term: u64,
    pub nodes: Vec<HaNode>,
    pub has_quorum: bool,
    pub commit_index: u64,
}

/// HA errors
#[derive(Debug, thiserror::Error)]
pub enum HaError {
    #[error("Not the leader (leader is: {0:?})")]
    NotLeader(Option<NodeId>),
    
    #[error("No quorum")]
    NoQuorum,
    
    #[error("Node not found: {0}")]
    NodeNotFound(NodeId),
    
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    
    #[error("Fencing failed: {0}")]
    FencingFailed(String),
    
    #[error("State machine error: {0}")]
    StateMachine(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
