//! Backup handlers
//!
//! Backup job and schedule management using real vmstate data

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::{vm_state, BackupRecord, BackupSchedule as StateBackupSchedule, BackupStatus};
use std::sync::Arc;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Query, Json},
    http::StatusCode,
    response::IntoResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupJob {
    pub id: String,
    pub vm_id: String,
    pub vm_name: String,
    pub backup_type: String,
    pub status: String,
    pub progress: f64,
    pub size_bytes: u64,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupScheduleResponse {
    pub id: String,
    pub name: String,
    pub vms: Vec<String>,
    pub backup_type: String,
    pub schedule: String,
    pub target: String,
    pub retention_days: u32,
    pub enabled: bool,
    pub last_run: Option<u64>,
    pub next_run: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBackupRequest {
    pub vm_id: String,
    pub backup_type: String,
    pub target: String,
    pub compress: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduleRequest {
    pub name: String,
    pub vms: Vec<String>,
    pub backup_type: String,
    pub schedule: String,
    pub target: String,
    pub retention_days: u32,
}

#[cfg(feature = "webgui")]
pub async fn list_jobs(
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let backup_records = state_mgr.list_backups();
    
    // Convert to API format
    let jobs: Vec<BackupJob> = backup_records.iter().map(|b| {
        BackupJob {
            id: b.id.clone(),
            vm_id: b.vm_id.clone(),
            vm_name: b.vm_name.clone(),
            backup_type: b.backup_type.clone(),
            status: b.status.to_string(),
            progress: b.progress,
            size_bytes: b.size_bytes,
            started_at: b.started_at,
            finished_at: b.finished_at,
            target: b.target_path.to_string_lossy().to_string(),
        }
    }).collect();
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: jobs.len() as u64,
        total_pages: ((jobs.len() as f64) / (params.per_page as f64)).ceil() as u32,
    };
    
    Json(ApiResponse::success(jobs).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn create_job(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateBackupRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    // Get VM info
    let vm_name = state_mgr.get_vm(&req.vm_id)
        .map(|vm| vm.name)
        .unwrap_or_else(|| req.vm_id.clone());
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let backup = BackupRecord {
        id: String::new(),
        vm_id: req.vm_id.clone(),
        vm_name,
        backup_type: req.backup_type,
        status: BackupStatus::Pending,
        progress: 0.0,
        size_bytes: 0,
        started_at: now,
        finished_at: None,
        target_path: PathBuf::from(&req.target),
        description: req.description,
    };
    
    match state_mgr.create_backup(backup) {
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
pub async fn list_schedules(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let schedules = state_mgr.list_backup_schedules();
    
    // Convert to API format
    let response: Vec<BackupScheduleResponse> = schedules.iter().map(|s| {
        BackupScheduleResponse {
            id: s.id.clone(),
            name: s.name.clone(),
            vms: s.vm_ids.clone(),
            backup_type: s.backup_type.clone(),
            schedule: s.cron_schedule.clone(),
            target: s.target_path.to_string_lossy().to_string(),
            retention_days: s.retention_days,
            enabled: s.enabled,
            last_run: s.last_run,
            next_run: s.next_run,
        }
    }).collect();
    
    Json(ApiResponse::success(response))
}

#[cfg(feature = "webgui")]
pub async fn create_schedule(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateScheduleRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    let schedule = StateBackupSchedule {
        id: String::new(),
        name: req.name.clone(),
        vm_ids: req.vms,
        backup_type: req.backup_type,
        cron_schedule: req.schedule,
        target_path: PathBuf::from(&req.target),
        retention_days: req.retention_days,
        enabled: true,
        last_run: None,
        next_run: Some(now + 86400), // Simple: next day
        created_at: now,
    };
    
    match state_mgr.create_backup_schedule(schedule) {
        Ok(id) => (
            StatusCode::CREATED,
            Json(ApiResponse::success(serde_json::json!({
                "id": id,
                "name": req.name
            }))),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<serde_json::Value>::error(400, &e)),
        ),
    }
}
