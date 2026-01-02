//! Template management handlers

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

/// VM Template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub os_type: String,
    pub os_version: Option<String>,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub size_bytes: u64,
    pub tags: Vec<String>,
    pub public: bool,
    pub owner: String,
}

/// Create template request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub description: Option<String>,
    pub source_vm: Option<String>,
    pub import_url: Option<String>,
    pub tags: Vec<String>,
    pub public: bool,
}

/// Deploy template request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployTemplateRequest {
    pub name: String,
    pub target_node: Option<String>,
    pub vcpus: Option<u32>,
    pub memory_mb: Option<u64>,
    pub networks: Vec<super::vm::CreateNetworkSpec>,
    pub start_after_deploy: bool,
}

#[cfg(feature = "webgui")]
pub async fn list(
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let templates = vec![
        Template {
            id: "tpl-001".to_string(),
            name: "Ubuntu 22.04 LTS".to_string(),
            description: Some("Ubuntu Server with cloud-init".to_string()),
            os_type: "linux".to_string(),
            os_version: Some("22.04".to_string()),
            vcpus: 2,
            memory_mb: 4096,
            disk_gb: 20,
            created_at: chrono::Utc::now().timestamp() as u64 - 86400 * 30,
            updated_at: chrono::Utc::now().timestamp() as u64 - 86400 * 7,
            size_bytes: 2_500_000_000,
            tags: vec!["ubuntu".to_string(), "linux".to_string()],
            public: true,
            owner: "system".to_string(),
        },
        Template {
            id: "tpl-002".to_string(),
            name: "Windows Server 2022".to_string(),
            description: Some("Windows Server with sysprep".to_string()),
            os_type: "windows".to_string(),
            os_version: Some("2022".to_string()),
            vcpus: 4,
            memory_mb: 8192,
            disk_gb: 60,
            created_at: chrono::Utc::now().timestamp() as u64 - 86400 * 60,
            updated_at: chrono::Utc::now().timestamp() as u64 - 86400 * 14,
            size_bytes: 15_000_000_000,
            tags: vec!["windows".to_string(), "server".to_string()],
            public: true,
            owner: "system".to_string(),
        },
    ];
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: templates.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(templates).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let template = Template {
        id: id.clone(),
        name: "Ubuntu 22.04 LTS".to_string(),
        description: Some("Ubuntu Server with cloud-init".to_string()),
        os_type: "linux".to_string(),
        os_version: Some("22.04".to_string()),
        vcpus: 2,
        memory_mb: 4096,
        disk_gb: 20,
        created_at: chrono::Utc::now().timestamp() as u64 - 86400 * 30,
        updated_at: chrono::Utc::now().timestamp() as u64 - 86400 * 7,
        size_bytes: 2_500_000_000,
        tags: vec!["ubuntu".to_string(), "linux".to_string()],
        public: true,
        owner: "system".to_string(),
    };
    
    Json(ApiResponse::success(template))
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateTemplateRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": format!("tpl-{}", &Uuid::new_v4().to_string()[..8]),
            "task_id": Uuid::new_v4().to_string()
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::<()>::success(()))
}

#[cfg(feature = "webgui")]
pub async fn deploy(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<DeployTemplateRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "vm_id": format!("vm-{}", &Uuid::new_v4().to_string()[..8]),
            "task_id": Uuid::new_v4().to_string()
        }))),
    )
}
