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
//! - **Embedded Vue.js frontend**
//! - **PostgreSQL database backend**

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
pub mod frontend;

pub use server::{WebGuiServer, WebGuiConfig, WebGuiState};
pub use websocket::{WebSocketManager, ClientConnection, WsMessage};
pub use console::{VmConsole, ConsoleType, ConsoleSession};
pub use auth::{WebAuthManager, SessionManager, Session, SessionToken};
pub use frontend::{serve_frontend, serve_index, has_frontend, FrontendAssets};
