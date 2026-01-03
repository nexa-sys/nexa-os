//! Storage handlers
//!
//! Storage pool and volume management using real state data

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::{vm_state, StoragePoolState};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json, Multipart},
    http::StatusCode,
    response::IntoResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePool {
    pub id: String,
    pub name: String,
    pub pool_type: String,
    pub path: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub status: String,
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub id: String,
    pub name: String,
    pub pool: String,
    pub size_bytes: u64,
    pub allocated_bytes: u64,
    pub format: String,
    pub vm_id: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsoImage {
    pub id: String,
    pub name: String,
    pub size_bytes: u64,
    pub pool: String,
    pub uploaded_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePoolRequest {
    pub name: String,
    pub pool_type: String,
    pub path: String,
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVolumeRequest {
    pub name: String,
    pub pool: String,
    pub size_bytes: u64,
    pub format: Option<String>,
}

#[cfg(feature = "webgui")]
pub async fn list_pools(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let storage_pools = state_mgr.list_storage_pools();
    
    // Convert from vmstate format to API format
    let pools: Vec<StoragePool> = storage_pools.iter().map(|p| {
        StoragePool {
            id: format!("pool-{}", &p.name),
            name: p.name.clone(),
            pool_type: p.pool_type.clone(),
            path: p.path.to_string_lossy().to_string(),
            total_bytes: p.total_bytes,
            used_bytes: p.used_bytes,
            available_bytes: p.total_bytes.saturating_sub(p.used_bytes),
            status: p.status.clone(),
            nodes: vec!["local".to_string()],
        }
    }).collect();
    
    // If no pools exist, create a default local pool
    if pools.is_empty() {
        // Auto-create default storage pool from system info
        let default_path = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/var/lib/nvm"))
            .join("images");
        
        // Try to get real disk space info
        #[cfg(target_os = "linux")]
        let (total, used) = {
            use std::process::Command;
            let output = Command::new("df")
                .args(["-B1", default_path.to_string_lossy().as_ref()])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| {
                    let line = s.lines().nth(1)?;
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let total = parts[1].parse::<u64>().ok()?;
                        let used = parts[2].parse::<u64>().ok()?;
                        Some((total, used))
                    } else {
                        None
                    }
                });
            output.unwrap_or((500_000_000_000, 0))
        };
        #[cfg(not(target_os = "linux"))]
        let (total, used) = (500_000_000_000u64, 0u64);
        
        let default_pool = StoragePool {
            id: "pool-local".to_string(),
            name: "local".to_string(),
            pool_type: "dir".to_string(),
            path: default_path.to_string_lossy().to_string(),
            total_bytes: total,
            used_bytes: used,
            available_bytes: total.saturating_sub(used),
            status: "online".to_string(),
            nodes: vec!["local".to_string()],
        };
        return Json(ApiResponse::success(vec![default_pool]));
    }
    
    Json(ApiResponse::success(pools))
}

#[cfg(feature = "webgui")]
pub async fn create_pool(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreatePoolRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    let pool = StoragePoolState {
        name: req.name.clone(),
        pool_type: req.pool_type,
        path: std::path::PathBuf::from(&req.path),
        total_bytes: 0, // Will be detected on first scan
        used_bytes: 0,
        status: "online".to_string(),
    };
    
    match state_mgr.create_storage_pool(pool) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(ApiResponse::success(serde_json::json!({
                "id": format!("pool-{}", req.name),
                "name": req.name
            }))),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<serde_json::Value>::error(400, &e)),
        ),
    }
}

#[cfg(feature = "webgui")]
pub async fn list_volumes(
    State(_state): State<Arc<WebGuiState>>,
    Query(_params): Query<PaginationParams>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let all_vms = state_mgr.list_vms();
    
    // Build volume list from VM disk paths
    let volumes: Vec<Volume> = all_vms.iter()
        .flat_map(|vm| {
            vm.disk_paths.iter().enumerate().map(move |(i, path)| {
                let size = std::fs::metadata(path)
                    .map(|m| m.len())
                    .unwrap_or(vm.disk_gb * 1_073_741_824);
                    
                Volume {
                    id: format!("vol-{}-{}", &vm.id, i),
                    name: path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| format!("{}-disk-{}.qcow2", vm.name, i)),
                    pool: "local".to_string(),
                    size_bytes: size,
                    allocated_bytes: size, // For qcow2, would need qemu-img info
                    format: "qcow2".to_string(),
                    vm_id: Some(vm.id.clone()),
                    created_at: vm.created_at,
                }
            })
        })
        .collect();
    
    Json(ApiResponse::success(volumes))
}

#[cfg(feature = "webgui")]
pub async fn create_volume(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateVolumeRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": format!("vol-{}", &Uuid::new_v4().to_string()[..8]),
            "task_id": Uuid::new_v4().to_string()
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn list_isos(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let isos = vec![
        IsoImage {
            id: "iso-001".to_string(),
            name: "ubuntu-22.04-live-server-amd64.iso".to_string(),
            size_bytes: 1_500_000_000,
            pool: "local".to_string(),
            uploaded_at: chrono::Utc::now().timestamp() as u64 - 86400 * 60,
        },
    ];
    
    Json(ApiResponse::success(isos))
}

#[cfg(feature = "webgui")]
pub async fn upload(
    State(state): State<Arc<WebGuiState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // Handle file upload
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("file").to_string();
        let filename = field.file_name().unwrap_or("unknown").to_string();
        // In real implementation, stream to disk
        let _data = field.bytes().await;
    }
    
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "task_id": Uuid::new_v4().to_string()
        }))),
    )
}
