//! VM management handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json},
    http::StatusCode,
    response::IntoResponse,
};

/// VM list item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmListItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub node: String,
    pub uptime: Option<u64>,
    pub cpu_usage: Option<f64>,
    pub memory_usage: Option<f64>,
    pub tags: Vec<String>,
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

/// Create VM request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    pub description: Option<String>,
    pub template: Option<String>,
    pub os_type: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disks: Vec<CreateDiskSpec>,
    pub networks: Vec<CreateNetworkSpec>,
    pub iso: Option<String>,
    pub start_after_create: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDiskSpec {
    pub size_gb: u64,
    pub storage_pool: String,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNetworkSpec {
    pub network: String,
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
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // Demo data
    let vms = vec![
        VmListItem {
            id: "vm-001".to_string(),
            name: "web-server-01".to_string(),
            status: "running".to_string(),
            vcpus: 4,
            memory_mb: 8192,
            disk_gb: 100,
            node: "node-01".to_string(),
            uptime: Some(86400),
            cpu_usage: Some(25.5),
            memory_usage: Some(45.2),
            tags: vec!["production".to_string(), "web".to_string()],
        },
        VmListItem {
            id: "vm-002".to_string(),
            name: "db-server-01".to_string(),
            status: "running".to_string(),
            vcpus: 8,
            memory_mb: 32768,
            disk_gb: 500,
            node: "node-02".to_string(),
            uptime: Some(172800),
            cpu_usage: Some(45.0),
            memory_usage: Some(78.5),
            tags: vec!["production".to_string(), "database".to_string()],
        },
    ];
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: vms.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(vms).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let vm = VmDetails {
        id: id.clone(),
        name: "web-server-01".to_string(),
        description: Some("Production web server".to_string()),
        status: "running".to_string(),
        config: VmConfig {
            os_type: "linux".to_string(),
            boot_order: vec!["disk".to_string(), "cdrom".to_string()],
            bios_type: "uefi".to_string(),
            secure_boot: true,
            tpm: true,
        },
        hardware: VmHardware {
            vcpus: 4,
            memory_mb: 8192,
            disks: vec![VmDisk {
                id: "disk-001".to_string(),
                name: "root".to_string(),
                size_gb: 100,
                format: "qcow2".to_string(),
                storage_pool: "local".to_string(),
                bus: "virtio".to_string(),
            }],
            networks: vec![VmNetwork {
                id: "nic-001".to_string(),
                mac: "52:54:00:12:34:56".to_string(),
                network: "default".to_string(),
                model: "virtio".to_string(),
                ip: Some("192.168.1.100".to_string()),
            }],
            cdrom: None,
        },
        metrics: Some(VmMetrics {
            cpu_percent: 25.5,
            memory_used_mb: 3700,
            disk_read_bps: 10_000_000,
            disk_write_bps: 5_000_000,
            net_rx_bps: 50_000_000,
            net_tx_bps: 30_000_000,
        }),
        snapshots: vec![],
        node: "node-01".to_string(),
        created_at: chrono::Utc::now().timestamp() as u64 - 86400 * 30,
        started_at: Some(chrono::Utc::now().timestamp() as u64 - 86400),
        tags: vec!["production".to_string(), "web".to_string()],
    };
    
    Json(ApiResponse::success(vm))
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateVmRequest>,
) -> impl IntoResponse {
    let vm_id = format!("vm-{}", Uuid::new_v4().to_string()[..8].to_string());
    
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": vm_id,
            "task_id": Uuid::new_v4().to_string()
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn update(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({"id": id})))
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::<()>::success(()))
}

#[cfg(feature = "webgui")]
pub async fn start(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn stop(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn restart(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn pause(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({"status": "paused"})))
}

#[cfg(feature = "webgui")]
pub async fn resume(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({"status": "running"})))
}

#[cfg(feature = "webgui")]
pub async fn snapshot(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<SnapshotRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "snapshot_id": Uuid::new_v4().to_string(),
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn clone(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<CloneRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "vm_id": format!("vm-{}", &Uuid::new_v4().to_string()[..8]),
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn migrate(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<MigrateRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "task_id": Uuid::new_v4().to_string()
    })))
}

#[cfg(feature = "webgui")]
pub async fn console_ticket(
    State(state): State<Arc<WebGuiState>>,
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
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let metrics = VmMetrics {
        cpu_percent: 25.5,
        memory_used_mb: 3700,
        disk_read_bps: 10_000_000,
        disk_write_bps: 5_000_000,
        net_rx_bps: 50_000_000,
        net_tx_bps: 30_000_000,
    };
    
    Json(ApiResponse::success(metrics))
}
