//! Template management handlers
//!
//! VM template management using real vmstate data

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::{vm_state, VmTemplate as StateVmTemplate};
use std::sync::Arc;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json},
    http::StatusCode,
    response::IntoResponse,
};

/// VM Template (API response format)
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
    pub os_type: Option<String>,
    pub vcpus: Option<u32>,
    pub memory_mb: Option<u64>,
    pub disk_gb: Option<u64>,
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

impl From<StateVmTemplate> for Template {
    fn from(t: StateVmTemplate) -> Self {
        Template {
            id: t.id,
            name: t.name,
            description: t.description,
            os_type: t.os_type,
            os_version: t.os_version,
            vcpus: t.vcpus,
            memory_mb: t.memory_mb,
            disk_gb: t.disk_gb,
            created_at: t.created_at,
            updated_at: t.updated_at,
            size_bytes: t.size_bytes,
            tags: t.tags,
            public: t.public,
            owner: t.owner,
        }
    }
}

#[cfg(feature = "webgui")]
pub async fn list(
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let state_templates = state_mgr.list_templates();
    
    let templates: Vec<Template> = state_templates.into_iter()
        .map(|t| t.into())
        .collect();
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: templates.len() as u64,
        total_pages: ((templates.len() as f64) / (params.per_page as f64)).ceil().max(1.0) as u32,
    };
    
    Json(ApiResponse::success(templates).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.get_template(&id) {
        Some(t) => {
            let template: Template = t.into();
            Json(ApiResponse::success(template))
        }
        None => Json(ApiResponse::<Template>::error(404, &format!("Template '{}' not found", id))),
    }
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateTemplateRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    // If creating from source VM, copy its config
    let (vcpus, memory_mb, disk_gb, os_type) = if let Some(ref vm_id) = req.source_vm {
        if let Some(vm) = state_mgr.get_vm(vm_id) {
            (vm.vcpus, vm.memory_mb, vm.disk_gb, "linux".to_string())
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<serde_json::Value>::error(400, &format!("Source VM '{}' not found", vm_id))),
            );
        }
    } else {
        (
            req.vcpus.unwrap_or(2),
            req.memory_mb.unwrap_or(4096),
            req.disk_gb.unwrap_or(20),
            req.os_type.clone().unwrap_or_else(|| "linux".to_string()),
        )
    };
    
    let template = StateVmTemplate {
        id: String::new(),
        name: req.name.clone(),
        description: req.description,
        os_type,
        os_version: None,
        vcpus,
        memory_mb,
        disk_gb,
        disk_path: None,
        created_at: now,
        updated_at: now,
        size_bytes: 0,
        tags: req.tags,
        public: req.public,
        owner: "admin".to_string(),
    };
    
    match state_mgr.create_template(template) {
        Ok(id) => (
            StatusCode::CREATED,
            Json(ApiResponse::success(serde_json::json!({
                "id": id,
                "task_id": Uuid::new_v4().to_string()
            }))),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<serde_json::Value>::error(400, &e)),
        ),
    }
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    match state_mgr.delete_template(&id) {
        Ok(_) => Json(ApiResponse::<()>::success(())),
        Err(e) => Json(ApiResponse::<()>::error(404, &e)),
    }
}

#[cfg(feature = "webgui")]
pub async fn deploy(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<DeployTemplateRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    // Get template
    let template = match state_mgr.get_template(&id) {
        Some(t) => t,
        None => return (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<serde_json::Value>::error(404, &format!("Template '{}' not found", id))),
        ),
    };
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    // Create VM from template
    let vm = crate::vmstate::VmState {
        id: String::new(),
        name: req.name.clone(),
        status: crate::vmstate::VmStatus::Stopped,
        vcpus: req.vcpus.unwrap_or(template.vcpus),
        memory_mb: req.memory_mb.unwrap_or(template.memory_mb),
        disk_gb: template.disk_gb,
        node: req.target_node,
        created_at: now,
        started_at: None,
        config_path: None,
        disk_paths: vec![],
        network_interfaces: vec![],
        tags: template.tags.clone(),
        description: Some(format!("Created from template '{}'", template.name)),
    };
    
    match state_mgr.create_vm(vm) {
        Ok(vm_id) => (
            StatusCode::CREATED,
            Json(ApiResponse::success(serde_json::json!({
                "vm_id": vm_id,
                "task_id": Uuid::new_v4().to_string()
            }))),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<serde_json::Value>::error(400, &e)),
        ),
    }
}
