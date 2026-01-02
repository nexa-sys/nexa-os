//! WebSocket Management
//!
//! Real-time communication for:
//! - Live metrics updates
//! - VM console streaming
//! - Event notifications
//! - Task progress updates

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crossbeam_channel::{Sender, Receiver, unbounded};

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMessage {
    // Client -> Server
    Subscribe { channel: String },
    Unsubscribe { channel: String },
    Ping,
    
    // Server -> Client
    Pong,
    Event(EventMessage),
    Metrics(MetricsMessage),
    TaskUpdate(TaskUpdateMessage),
    VmStatus(VmStatusMessage),
    ConsoleData(ConsoleDataMessage),
    Error { code: u32, message: String },
}

/// Event message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMessage {
    pub id: Uuid,
    pub timestamp: u64,
    pub event_type: String,
    pub severity: EventSeverity,
    pub source: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

/// Event severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Metrics update message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsMessage {
    pub timestamp: u64,
    pub target: String,
    pub target_type: MetricTarget,
    pub cpu_percent: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

/// Metric target type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricTarget {
    Node,
    Vm,
    Storage,
    Network,
}

/// Task update message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdateMessage {
    pub task_id: Uuid,
    pub status: String,
    pub progress: f64,
    pub message: Option<String>,
}

/// VM status change message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmStatusMessage {
    pub vm_id: String,
    pub status: String,
    pub uptime: Option<u64>,
}

/// Console data message (for VM console)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleDataMessage {
    pub vm_id: String,
    pub data: String, // base64 encoded
}

/// Client connection state
pub struct ClientConnection {
    pub id: Uuid,
    pub user_id: String,
    pub connected_at: u64,
    pub subscriptions: RwLock<Vec<String>>,
    pub tx: Sender<WsMessage>,
}

impl ClientConnection {
    pub fn new(user_id: String, tx: Sender<WsMessage>) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            connected_at: chrono::Utc::now().timestamp() as u64,
            subscriptions: RwLock::new(Vec::new()),
            tx,
        }
    }

    pub fn subscribe(&self, channel: &str) {
        let mut subs = self.subscriptions.write();
        if !subs.contains(&channel.to_string()) {
            subs.push(channel.to_string());
        }
    }

    pub fn unsubscribe(&self, channel: &str) {
        let mut subs = self.subscriptions.write();
        subs.retain(|c| c != channel);
    }

    pub fn is_subscribed(&self, channel: &str) -> bool {
        self.subscriptions.read().contains(&channel.to_string())
    }

    pub fn send(&self, msg: WsMessage) -> Result<(), crossbeam_channel::SendError<WsMessage>> {
        self.tx.send(msg)
    }
}

/// WebSocket connection manager
pub struct WebSocketManager {
    connections: RwLock<HashMap<Uuid, Arc<ClientConnection>>>,
    channels: RwLock<HashMap<String, Vec<Uuid>>>,
    connection_count: AtomicU64,
}

impl WebSocketManager {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            channels: RwLock::new(HashMap::new()),
            connection_count: AtomicU64::new(0),
        }
    }

    /// Register a new connection
    pub fn register(&self, user_id: String) -> (Arc<ClientConnection>, Receiver<WsMessage>) {
        let (tx, rx) = unbounded();
        let conn = Arc::new(ClientConnection::new(user_id, tx));
        
        self.connections.write().insert(conn.id, conn.clone());
        self.connection_count.fetch_add(1, Ordering::SeqCst);
        
        (conn, rx)
    }

    /// Unregister a connection
    pub fn unregister(&self, id: Uuid) {
        if let Some(conn) = self.connections.write().remove(&id) {
            // Remove from all channels
            let mut channels = self.channels.write();
            for subscribers in channels.values_mut() {
                subscribers.retain(|cid| *cid != id);
            }
            self.connection_count.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Subscribe connection to a channel
    pub fn subscribe(&self, conn_id: Uuid, channel: &str) {
        if let Some(conn) = self.connections.read().get(&conn_id) {
            conn.subscribe(channel);
            
            let mut channels = self.channels.write();
            channels.entry(channel.to_string())
                .or_insert_with(Vec::new)
                .push(conn_id);
        }
    }

    /// Unsubscribe connection from a channel
    pub fn unsubscribe(&self, conn_id: Uuid, channel: &str) {
        if let Some(conn) = self.connections.read().get(&conn_id) {
            conn.unsubscribe(channel);
            
            let mut channels = self.channels.write();
            if let Some(subscribers) = channels.get_mut(channel) {
                subscribers.retain(|id| *id != conn_id);
            }
        }
    }

    /// Broadcast message to all connections
    pub fn broadcast(&self, msg: WsMessage) {
        let conns = self.connections.read();
        for conn in conns.values() {
            let _ = conn.send(msg.clone());
        }
    }

    /// Send message to specific channel subscribers
    pub fn send_to_channel(&self, channel: &str, msg: WsMessage) {
        let channels = self.channels.read();
        let conns = self.connections.read();
        
        if let Some(subscribers) = channels.get(channel) {
            for conn_id in subscribers {
                if let Some(conn) = conns.get(conn_id) {
                    let _ = conn.send(msg.clone());
                }
            }
        }
    }

    /// Send message to specific user
    pub fn send_to_user(&self, user_id: &str, msg: WsMessage) {
        let conns = self.connections.read();
        for conn in conns.values() {
            if conn.user_id == user_id {
                let _ = conn.send(msg.clone());
            }
        }
    }

    /// Get connection count
    pub fn connection_count(&self) -> u64 {
        self.connection_count.load(Ordering::SeqCst)
    }

    /// Get all active channels
    pub fn active_channels(&self) -> Vec<String> {
        self.channels.read().keys().cloned().collect()
    }
}

impl Default for WebSocketManager {
    fn default() -> Self {
        Self::new()
    }
}

// Axum WebSocket handler (only compiled with webgui feature)
#[cfg(feature = "webgui")]
pub async fn ws_handler(
    ws: axum::extract::WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<std::sync::Arc<super::server::WebGuiState>>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

#[cfg(feature = "webgui")]
async fn handle_socket(
    socket: axum::extract::ws::WebSocket,
    state: std::sync::Arc<super::server::WebGuiState>,
) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    
    let (mut sender, mut receiver) = socket.split();
    
    // For now, use anonymous user
    let (conn, rx) = state.ws_manager.register("anonymous".to_string());
    let conn_id = conn.id;
    
    // Spawn task to forward messages from channel to WebSocket
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv() {
            let json = serde_json::to_string(&msg).unwrap_or_default();
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });
    
    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        match ws_msg {
                            WsMessage::Subscribe { channel } => {
                                state.ws_manager.subscribe(conn_id, &channel);
                            }
                            WsMessage::Unsubscribe { channel } => {
                                state.ws_manager.unsubscribe(conn_id, &channel);
                            }
                            WsMessage::Ping => {
                                let _ = conn.send(WsMessage::Pong);
                            }
                            _ => {}
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }
    
    // Cleanup
    state.ws_manager.unregister(conn_id);
    send_task.abort();
}
