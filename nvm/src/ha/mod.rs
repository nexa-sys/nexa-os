//! High Availability Module
//!
//! Raft consensus, split-brain detection, automatic failover.

pub mod raft;
pub mod fencing;
pub mod failover;
pub mod types;

pub use types::*;
pub use raft::RaftNode;
pub use fencing::FencingManager;
pub use failover::FailoverManager;
