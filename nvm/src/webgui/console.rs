//! VM Console Support
//!
//! Remote console access via:
//! - noVNC (browser-based VNC)
//! - SPICE (high performance)
//! - Serial console (text mode)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Console type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleType {
    /// VNC console (via noVNC)
    Vnc,
    /// SPICE console
    Spice,
    /// Serial console (text)
    Serial,
    /// Virtual terminal
    Vterm,
}

/// Console session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleSession {
    pub id: Uuid,
    pub vm_id: String,
    pub console_type: ConsoleType,
    pub user: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub ticket: String,
    pub port: u16,
    pub password: Option<String>,
}

impl ConsoleSession {
    pub fn new(vm_id: &str, console_type: ConsoleType, user: &str, port: u16) -> Self {
        let now = chrono::Utc::now().timestamp() as u64;
        let ticket = Self::generate_ticket();
        
        Self {
            id: Uuid::new_v4(),
            vm_id: vm_id.to_string(),
            console_type,
            user: user.to_string(),
            created_at: now,
            expires_at: now + 600, // 10 minutes
            ticket,
            port,
            password: Some(Self::generate_password()),
        }
    }

    fn generate_ticket() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: [u8; 32] = rng.gen();
        hex::encode(bytes)
    }

    fn generate_password() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: [u8; 8] = rng.gen();
        hex::encode(bytes)
    }

    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now > self.expires_at
    }
}

/// VM Console manager
pub struct VmConsole {
    sessions: RwLock<HashMap<Uuid, ConsoleSession>>,
    vm_sessions: RwLock<HashMap<String, Vec<Uuid>>>,
    next_port: RwLock<u16>,
    config: ConsoleConfig,
}

/// Console configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleConfig {
    /// VNC port range start
    pub vnc_port_start: u16,
    /// VNC port range end
    pub vnc_port_end: u16,
    /// SPICE port range start
    pub spice_port_start: u16,
    /// SPICE port range end
    pub spice_port_end: u16,
    /// Session timeout (seconds)
    pub session_timeout: u64,
    /// Max sessions per VM
    pub max_sessions_per_vm: u32,
    /// Enable TLS for VNC
    pub vnc_tls: bool,
    /// noVNC path
    pub novnc_path: String,
}

impl Default for ConsoleConfig {
    fn default() -> Self {
        Self {
            vnc_port_start: 5900,
            vnc_port_end: 5999,
            spice_port_start: 5900,
            spice_port_end: 5999,
            session_timeout: 600,
            max_sessions_per_vm: 5,
            vnc_tls: false,
            novnc_path: "/usr/share/novnc".to_string(),
        }
    }
}

impl VmConsole {
    pub fn new(config: ConsoleConfig) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            vm_sessions: RwLock::new(HashMap::new()),
            next_port: RwLock::new(config.vnc_port_start),
            config,
        }
    }

    /// Create console session
    pub fn create_session(
        &self,
        vm_id: &str,
        console_type: ConsoleType,
        user: &str,
    ) -> Result<ConsoleSession, ConsoleError> {
        // Check session limit
        let vm_sids = self.vm_sessions.read();
        if let Some(sids) = vm_sids.get(vm_id) {
            if sids.len() >= self.config.max_sessions_per_vm as usize {
                return Err(ConsoleError::TooManySessions);
            }
        }
        drop(vm_sids);

        // Allocate port
        let port = self.allocate_port(console_type)?;
        
        // Create session
        let session = ConsoleSession::new(vm_id, console_type, user, port);
        
        // Store session
        self.sessions.write().insert(session.id, session.clone());
        self.vm_sessions.write()
            .entry(vm_id.to_string())
            .or_insert_with(Vec::new)
            .push(session.id);
        
        Ok(session)
    }

    /// Get session by ticket
    pub fn get_session_by_ticket(&self, ticket: &str) -> Option<ConsoleSession> {
        self.sessions.read().values()
            .find(|s| s.ticket == ticket && !s.is_expired())
            .cloned()
    }

    /// Get session by ID
    pub fn get_session(&self, id: Uuid) -> Option<ConsoleSession> {
        self.sessions.read().get(&id).cloned()
    }

    /// Close session
    pub fn close_session(&self, id: Uuid) {
        if let Some(session) = self.sessions.write().remove(&id) {
            if let Some(sids) = self.vm_sessions.write().get_mut(&session.vm_id) {
                sids.retain(|sid| *sid != id);
            }
        }
    }

    /// Close all sessions for a VM
    pub fn close_vm_sessions(&self, vm_id: &str) {
        if let Some(sids) = self.vm_sessions.write().remove(vm_id) {
            let mut sessions = self.sessions.write();
            for sid in sids {
                sessions.remove(&sid);
            }
        }
    }

    /// Cleanup expired sessions
    pub fn cleanup_expired(&self) {
        let expired: Vec<Uuid> = self.sessions.read()
            .iter()
            .filter(|(_, s)| s.is_expired())
            .map(|(id, _)| *id)
            .collect();
        
        for id in expired {
            self.close_session(id);
        }
    }

    fn allocate_port(&self, console_type: ConsoleType) -> Result<u16, ConsoleError> {
        let (start, end) = match console_type {
            ConsoleType::Vnc | ConsoleType::Serial | ConsoleType::Vterm => {
                (self.config.vnc_port_start, self.config.vnc_port_end)
            }
            ConsoleType::Spice => {
                (self.config.spice_port_start, self.config.spice_port_end)
            }
        };

        let mut next = self.next_port.write();
        if *next > end {
            *next = start;
        }
        let port = *next;
        *next += 1;
        
        Ok(port)
    }

    /// Get noVNC proxy URL for a session
    pub fn get_novnc_url(&self, session: &ConsoleSession) -> String {
        format!(
            "/novnc/vnc.html?host={}&port={}&password={}&autoconnect=true&resize=scale",
            "localhost",
            session.port,
            session.password.as_deref().unwrap_or("")
        )
    }
}

impl Default for VmConsole {
    fn default() -> Self {
        Self::new(ConsoleConfig::default())
    }
}

/// Console errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConsoleError {
    #[error("Too many active sessions for this VM")]
    TooManySessions,
    #[error("No available ports")]
    NoAvailablePorts,
    #[error("Session not found")]
    SessionNotFound,
    #[error("Session expired")]
    SessionExpired,
    #[error("VM not found")]
    VmNotFound,
    #[error("Console not available")]
    NotAvailable,
}

// noVNC WebSocket proxy handler
#[cfg(feature = "webgui")]
pub async fn novnc_handler(
    axum::extract::Path(vmid): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
    axum::extract::State(state): axum::extract::State<std::sync::Arc<super::server::WebGuiState>>,
) -> impl axum::response::IntoResponse {
    // This would serve the noVNC HTML page with appropriate parameters
    let html = format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>NVM Console - {}</title>
    <script src="/static/novnc/app/ui.js"></script>
</head>
<body>
    <div id="noVNC_container">
        <canvas id="noVNC_canvas"></canvas>
    </div>
    <script>
        // Initialize noVNC connection
        const vnc = new VNC({{
            target: document.getElementById('noVNC_canvas'),
            host: window.location.hostname,
            port: {},
            password: '{}'
        }});
        vnc.connect();
    </script>
</body>
</html>
"#, vmid, params.get("port").unwrap_or(&"5900".to_string()), 
    params.get("password").unwrap_or(&"".to_string()));
    
    axum::response::Html(html)
}
