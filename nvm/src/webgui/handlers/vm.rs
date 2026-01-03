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
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // Get VMs from state manager
    let state_mgr = vm_state();
    let all_vms = state_mgr.list_vms();
    
    let vms: Vec<VmListItem> = all_vms.iter().map(|vm| {
        VmListItem {
            id: vm.id.clone(),
            name: vm.name.clone(),
            status: vm.status.to_string(),
            vcpus: vm.vcpus,
            memory_mb: vm.memory_mb,
            disk_gb: vm.disk_gb,
            node: vm.node.clone().unwrap_or_else(|| "local".to_string()),
            uptime: vm.started_at.map(|start| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                now.saturating_sub(start)
            }),
            cpu_usage: None, // Would be from monitoring
            memory_usage: None,
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
    use crate::vmstate::VmState;
    
    let state_mgr = vm_state();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let vm = VmState {
        id: String::new(), // Will be generated
        name: req.name,
        status: StateVmStatus::Stopped,
        vcpus: req.vcpus,
        memory_mb: req.memory_mb,
        disk_gb: req.disks.first().map(|d| d.size_gb).unwrap_or(20),
        node: None,
        created_at: now,
        started_at: None,
        config_path: None,
        disk_paths: vec![],
        network_interfaces: vec![],
        tags: req.tags,
        description: req.description,
    };
    
    match state_mgr.create_vm(vm) {
        Ok(vm_id) => {
            (
                StatusCode::CREATED,
                Json(ApiResponse::success(serde_json::json!({
                    "id": vm_id,
                    "task_id": Uuid::new_v4().to_string()
                }))),
            )
        }
        Err(e) => {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<serde_json::Value>::error(400, &e)),
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
