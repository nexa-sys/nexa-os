//! Event System Module
//!
//! Real-time event bus with WebSocket broadcasting, event persistence,
//! and audit logging for enterprise compliance.

pub mod bus;
pub mod types;
pub mod audit;
pub mod persistence;

pub use bus::EventBus;
pub use types::*;
pub use audit::AuditLogger;
