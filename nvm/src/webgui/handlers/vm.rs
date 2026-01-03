//! VM management handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::{vm_state, VmStatus as StateVmStatus};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json},
    http::StatusCode,
    response::IntoResponse,
};

/// VM list item - matches frontend Vm interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub description: Option<String>,
    pub host_node: Option<String>,
    pub template: Option<String>,
    pub config: VmListConfig,
    pub stats: Option<VmListStats>,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
}

/// VM config for list items - matches frontend VmConfig interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListConfig {
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub network: String,
    pub boot_order: Vec<String>,
}

/// VM stats for list items - matches frontend VmStats interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListStats {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub network_rx_bps: u64,
    pub network_tx_bps: u64,
}

/// VM details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDetails {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub config: VmConfig,
    pub hardware: VmHardware,
    pub metrics: Option<VmMetrics>,
    pub snapshots: Vec<VmSnapshot>,
    pub node: String,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub os_type: String,
    pub boot_order: Vec<String>,
    pub bios_type: String,
    pub secure_boot: bool,
    pub tpm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmHardware {
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disks: Vec<VmDisk>,
    pub networks: Vec<VmNetwork>,
    pub cdrom: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDisk {
    pub id: String,
    pub name: String,
    pub size_gb: u64,
    pub format: String,
    pub storage_pool: String,
    pub bus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmNetwork {
    pub id: String,
    pub mac: String,
    pub network: String,
    pub model: String,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetrics {
    pub cpu_percent: f64,
    pub memory_used_mb: u64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSnapshot {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub size_mb: u64,
    pub description: Option<String>,
}

/// Create VM request - supports both frontend config format and direct format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    
    // Frontend sends config object with these fields
    #[serde(default)]
    pub config: Option<CreateVmConfig>,
    
    // Direct fields (for backward compatibility)
    #[serde(default)]
    pub os_type: Option<String>,
    #[serde(default)]
    pub vcpus: Option<u32>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
    #[serde(default)]
    pub disks: Option<Vec<CreateDiskSpec>>,
    #[serde(default)]
    pub networks: Option<Vec<CreateNetworkSpec>>,
    #[serde(default)]
    pub iso: Option<String>,
    #[serde(default)]
    pub start_after_create: Option<bool>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Config object from frontend form
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmConfig {
    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: u32,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
    #[serde(default = "default_disk_gb")]
    pub disk_gb: u64,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default)]
    pub boot_order: Vec<String>,
}

fn default_cpu_cores() -> u32 { 2 }
fn default_memory_mb() -> u64 { 2048 }
fn default_disk_gb() -> u64 { 20 }
fn default_network() -> String { "default".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDiskSpec {
    pub size_gb: u64,
    #[serde(default = "default_storage_pool")]
    pub storage_pool: String,
    #[serde(default)]
    pub format: Option<String>,
}

fn default_storage_pool() -> String { "local".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNetworkSpec {
    pub network: String,
    #[serde(default)]
    pub mac: Option<String>,
}

/// Snapshot request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub name: String,
    pub description: Option<String>,
    pub include_memory: bool,
}

/// Clone request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    pub name: String,
    pub full_clone: bool,
    pub target_node: Option<String>,
}

/// Migrate request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateRequest {
    pub target_node: String,
    pub live: bool,
    pub with_storage: bool,
}

/// Console ticket response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleTicket {
    pub ticket: String,
    pub port: u16,
    pub console_type: String,
    pub url: String,
    pub expires_at: u64,
}

// Handlers

#[cfg(feature = "webgui")]
pub async fn list(
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // Get VMs from state manager
    let state_mgr = vm_state();
    let all_vms = state_mgr.list_vms();
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let vms: Vec<VmListItem> = all_vms.iter().map(|vm| {
        // Format timestamps as ISO8601 strings
        let created_at = chrono::DateTime::from_timestamp(vm.created_at as i64, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        
        let updated_at = vm.started_at
            .and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| created_at.clone());
        
        VmListItem {
            id: vm.id.clone(),
            name: vm.name.clone(),
            status: vm.status.to_string(),
            description: vm.description.clone(),
            host_node: vm.node.clone(),
            template: None,
            config: VmListConfig {
                cpu_cores: vm.vcpus,
                memory_mb: vm.memory_mb,
                disk_gb: vm.disk_gb,
                network: vm.network_interfaces.first()
                    .map(|nic| nic.network.clone())
                    .unwrap_or_else(|| "default".to_string()),
                boot_order: vec!["disk".to_string(), "cdrom".to_string()],
            },
            stats: if vm.status == StateVmStatus::Running {
                Some(VmListStats {
                    cpu_usage: 0.0,      // Would come from monitoring
                    memory_usage: 0.0,
                    disk_read_bps: 0,
                    disk_write_bps: 0,
                    network_rx_bps: 0,
                    network_tx_bps: 0,
                })
            } else {
                None
            },
            created_at,
            updated_at,
            tags: vm.tags.clone(),
        }
    }).collect();
    
    let total = vms.len() as u64;
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total,
        total_pages: ((total as f64) / (params.per_page as f64)).ceil() as u32,
    };
    
    Json(ApiResponse::success(vms).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.get_vm(&id) {
        Some(vm) => {
            let details = VmDetails {
                id: vm.id.clone(),
                name: vm.name.clone(),
                description: vm.description.clone(),
                status: vm.status.to_string(),
                config: VmConfig {
                    os_type: "linux".to_string(),
                    boot_order: vec!["disk".to_string(), "cdrom".to_string()],
                    bios_type: "uefi".to_string(),
                    secure_boot: false,
                    tpm: false,
                },
                hardware: VmHardware {
                    vcpus: vm.vcpus,
                    memory_mb: vm.memory_mb,
                    disks: vec![VmDisk {
                        id: "disk-001".to_string(),
                        name: "root".to_string(),
                        size_gb: vm.disk_gb,
                        format: "qcow2".to_string(),
                        storage_pool: "local".to_string(),
                        bus: "virtio".to_string(),
                    }],
                    networks: vm.network_interfaces.iter().map(|nic| VmNetwork {
                        id: nic.id.clone(),
                        mac: nic.mac.clone(),
                        network: nic.network.clone(),
                        model: nic.model.clone(),
                        ip: nic.ip.clone(),
                    }).collect(),
                    cdrom: None,
                },
                metrics: None, // Would come from monitoring
                snapshots: vec![],
                node: vm.node.clone().unwrap_or_else(|| "local".to_string()),
                created_at: vm.created_at,
                started_at: vm.started_at,
                tags: vm.tags.clone(),
            };
            Json(ApiResponse::success(details))
        }
        None => {
            Json(ApiResponse::<VmDetails>::error(404, &format!("VM '{}' not found", id)))
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateVmRequest>,
) -> impl IntoResponse {
    use crate::vmstate::{VmState, NetworkInterface};
    
    let state_mgr = vm_state();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    // Extract values from config object (frontend format) or direct fields (API format)
    let (vcpus, memory_mb, disk_gb, network) = if let Some(config) = &req.config {
        (config.cpu_cores, config.memory_mb, config.disk_gb, config.network.clone())
    } else {
        (
            req.vcpus.unwrap_or(2),
            req.memory_mb.unwrap_or(2048),
            req.disks.as_ref().and_then(|d| d.first().map(|d| d.size_gb)).unwrap_or(20),
            req.networks.as_ref().and_then(|n| n.first().map(|n| n.network.clone())).unwrap_or_else(|| "default".to_string()),
        )
    };
    
    let tags = req.tags.clone().unwrap_or_default();
    
    // Generate network interface
    let nic_id = format!("nic-{}", &Uuid::new_v4().to_string()[..8]);
    let mac = format!("52:54:00:{:02x}:{:02x}:{:02x}",
        rand::random::<u8>(), rand::random::<u8>(), rand::random::<u8>());
    
    let network_interfaces = vec![NetworkInterface {
        id: nic_id,
        mac,
        network: network.clone(),
        model: "virtio".to_string(),
        ip: None,
    }];
    
    let vm = VmState {
        id: String::new(), // Will be generated
        name: req.name.clone(),
        status: StateVmStatus::Stopped,
        vcpus,
        memory_mb,
        disk_gb,
        node: None,
        created_at: now,
        started_at: None,
        config_path: None,
        disk_paths: vec![],
        network_interfaces,
        tags: tags.clone(),
        description: req.description.clone(),
    };
    
    match state_mgr.create_vm(vm) {
        Ok(vm_id) => {
            // Return full VM object matching frontend Vm interface
            let created_at = chrono::DateTime::from_timestamp(now as i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            
            let response_vm = VmListItem {
                id: vm_id.clone(),
                name: req.name,
                status: "stopped".to_string(),
                description: req.description,
                host_node: None,
                template: req.template,
                config: VmListConfig {
                    cpu_cores: vcpus,
                    memory_mb,
                    disk_gb,
                    network,
                    boot_order: req.config.as_ref()
                        .map(|c| c.boot_order.clone())
                        .unwrap_or_else(|| vec!["disk".to_string(), "cdrom".to_string()]),
                },
                stats: None,
                created_at: created_at.clone(),
                updated_at: created_at,
                tags,
            };
            
            (
                StatusCode::CREATED,
                Json(ApiResponse::success(response_vm)),
            )
        }
        Err(e) => {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<VmListItem>::error(400, &e)),
            )
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn update(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(_req): Json<serde_json::Value>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({"id": id})))
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.delete_vm(&id) {
        Ok(_) => Json(ApiResponse::<()>::success(())),
        Err(e) => Json(ApiResponse::<()>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn start(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Running) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string(),
            "status": "running"
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn stop(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Stopped) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string(),
            "status": "stopped"
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn restart(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    // Stop then start
    let _ = state_mgr.set_vm_status(&id, StateVmStatus::Stopped);
    match state_mgr.set_vm_status(&id, StateVmStatus::Running) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string(),
            "status": "running"
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn pause(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Paused) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({"status": "paused"}))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn resume(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Running) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({"status": "running"}))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn snapshot(
    State(_state): State<Arc<WebGuiState>>,
    Path(_id): Path<String>,
    Json(_req): Json<SnapshotRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "snapshot_id": Uuid::new_v4().to_string(),
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn clone(
    State(_state): State<Arc<WebGuiState>>,
    Path(_id): Path<String>,
    Json(_req): Json<CloneRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "vm_id": format!("vm-{}", &Uuid::new_v4().to_string()[..8]),
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn migrate(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(_req): Json<MigrateRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.set_vm_status(&id, StateVmStatus::Migrating) {
        Ok(_) => Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string()
        }))),
        Err(e) => Json(ApiResponse::<serde_json::Value>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn console_ticket(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let ticket = ConsoleTicket {
        ticket: Uuid::new_v4().to_string(),
        port: 5900,
        console_type: "vnc".to_string(),
        url: format!("/novnc/{}?autoconnect=true", id),
        expires_at: chrono::Utc::now().timestamp() as u64 + 600,
    };
    
    Json(ApiResponse::success(ticket))
}

#[cfg(feature = "webgui")]
pub async fn metrics(
    State(_state): State<Arc<WebGuiState>>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    // In a real implementation, this would come from a monitoring system
    let metrics = VmMetrics {
        cpu_percent: 0.0,
        memory_used_mb: 0,
        disk_read_bps: 0,
        disk_write_bps: 0,
        net_rx_bps: 0,
        net_tx_bps: 0,
    };
    
    Json(ApiResponse::success(metrics))
}

/// WebSocket console handler for VM
#[cfg(feature = "webgui")]
pub async fn console_ws(
    ws: axum::extract::WebSocketUpgrade,
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Verify VM exists and is running
    let state_mgr = vm_state();
    let vm = state_mgr.list_vms().into_iter().find(|v| v.id == id);
    
    match vm {
        Some(v) if v.status == StateVmStatus::Running => {
            ws.on_upgrade(move |socket| handle_console_socket(socket, state, id))
        }
        Some(_) => {
            // VM exists but not running - return upgrade anyway but close immediately
            ws.on_upgrade(move |socket| handle_console_error(socket, 4001, "VM is not running"))
        }
        None => {
            ws.on_upgrade(move |socket| handle_console_error(socket, 4004, "VM not found"))
        }
    }
}

#[cfg(feature = "webgui")]
async fn handle_console_error(
    mut socket: axum::extract::ws::WebSocket,
    code: u16,
    reason: &'static str,
) {
    use axum::extract::ws::Message;
    
    let _ = socket.send(Message::Close(Some(axum::extract::ws::CloseFrame {
        code,
        reason: reason.into(),
    }))).await;
}

#[cfg(feature = "webgui")]
async fn handle_console_socket(
    socket: axum::extract::ws::WebSocket,
    state: Arc<WebGuiState>,
    vm_id: String,
) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    
    let (mut sender, mut receiver) = socket.split();
    
    // Send initial connection success message
    let _ = sender.send(Message::Text(serde_json::json!({
        "type": "connected",
        "vm_id": vm_id,
        "console_type": "vnc",
        "message": "Console connection established"
    }).to_string().into())).await;
    
    // In a real implementation, this would connect to QEMU's VNC server
    // and proxy the VNC protocol over WebSocket (like noVNC does)
    
    // For now, we'll just echo messages and handle basic commands
    while let Some(msg) = receiver.next().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    // Handle console commands
                    if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                        match cmd.get("type").and_then(|t| t.as_str()) {
                            Some("key") => {
                                // Would forward key events to QEMU
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "key"
                                }).to_string().into())).await;
                            }
                            Some("mouse") => {
                                // Would forward mouse events to QEMU
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "mouse"
                                }).to_string().into())).await;
                            }
                            Some("resize") => {
                                // Would handle screen resize
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "ack",
                                    "command": "resize"
                                }).to_string().into())).await;
                            }
                            Some("ping") => {
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "pong"
                                }).to_string().into())).await;
                            }
                            _ => {
                                let _ = sender.send(Message::Text(serde_json::json!({
                                    "type": "error",
                                    "message": "Unknown command"
                                }).to_string().into())).await;
                            }
                        }
                    }
                }
                Message::Binary(data) => {
                    // Binary data would be VNC protocol frames
                    // Echo back for now (in real impl, proxy to QEMU VNC)
                    let _ = sender.send(Message::Binary(data)).await;
                }
                Message::Ping(data) => {
                    let _ = sender.send(Message::Pong(data)).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }
    
    log::debug!("Console WebSocket closed for VM {}", vm_id);
}
