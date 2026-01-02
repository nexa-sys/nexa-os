//! Storage handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
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
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let pools = vec![
        StoragePool {
            id: "pool-local".to_string(),
            name: "local".to_string(),
            pool_type: "dir".to_string(),
            path: "/var/lib/nvm/images".to_string(),
            total_bytes: 1_000_000_000_000,
            used_bytes: 450_000_000_000,
            available_bytes: 550_000_000_000,
            status: "online".to_string(),
            nodes: vec!["node-01".to_string()],
        },
        StoragePool {
            id: "pool-nfs".to_string(),
            name: "shared-nfs".to_string(),
            pool_type: "nfs".to_string(),
            path: "nfs-server:/exports/vms".to_string(),
            total_bytes: 10_000_000_000_000,
            used_bytes: 3_500_000_000_000,
            available_bytes: 6_500_000_000_000,
            status: "online".to_string(),
            nodes: vec!["node-01".to_string(), "node-02".to_string(), "node-03".to_string()],
        },
    ];
    
    Json(ApiResponse::success(pools))
}

#[cfg(feature = "webgui")]
pub async fn create_pool(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreatePoolRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": format!("pool-{}", req.name)
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn list_volumes(
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let volumes = vec![
        Volume {
            id: "vol-001".to_string(),
            name: "web-server-01-root.qcow2".to_string(),
            pool: "local".to_string(),
            size_bytes: 107_374_182_400,
            allocated_bytes: 25_000_000_000,
            format: "qcow2".to_string(),
            vm_id: Some("vm-001".to_string()),
            created_at: chrono::Utc::now().timestamp() as u64 - 86400 * 30,
        },
    ];
    
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
