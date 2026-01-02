//! Enterprise WebGUI Module
//!
//! Full-featured web management interface similar to:
//! - VMware vSphere Web Client
//! - Proxmox VE Web Interface
//! - Hyper-V Manager (web version)
//!
//! ## Features
//!
//! - Real-time dashboard with cluster overview
//! - VM management (create, edit, console, snapshots)
//! - Storage and network management
//! - User/role management with RBAC
//! - Monitoring and alerting
//! - Task and event logs
//! - WebSocket-based real-time updates
//! - noVNC/SPICE console integration

pub mod server;
pub mod routes;
pub mod handlers;
pub mod websocket;
pub mod console;
pub mod dashboard;
pub mod auth;
pub mod middleware;
pub mod templates;
pub mod assets;

pub use server::{WebGuiServer, WebGuiConfig, WebGuiState};
pub use websocket::{WebSocketManager, ClientConnection, WsMessage};
pub use console::{VmConsole, ConsoleType, ConsoleSession};
pub use auth::{WebAuthManager, SessionManager, Session, SessionToken};

use std::sync::Arc;
use std::net::SocketAddr;

/// Default WebGUI port
pub const DEFAULT_PORT: u16 = 8006;
/// Default bind address
pub const DEFAULT_BIND: &str = "0.0.0.0";
/// API version
pub const API_VERSION: &str = "v2";
