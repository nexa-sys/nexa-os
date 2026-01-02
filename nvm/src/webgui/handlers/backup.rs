//! Backup handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
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
pub struct BackupSchedule {
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
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().timestamp() as u64;
    
    let jobs = vec![
        BackupJob {
            id: "backup-001".to_string(),
            vm_id: "vm-001".to_string(),
            vm_name: "web-server-01".to_string(),
            backup_type: "full".to_string(),
            status: "completed".to_string(),
            progress: 100.0,
            size_bytes: 25_000_000_000,
            started_at: now - 7200,
            finished_at: Some(now - 3600),
            target: "backup-storage".to_string(),
        },
        BackupJob {
            id: "backup-002".to_string(),
            vm_id: "vm-002".to_string(),
            vm_name: "db-server-01".to_string(),
            backup_type: "incremental".to_string(),
            status: "running".to_string(),
            progress: 45.5,
            size_bytes: 0,
            started_at: now - 1800,
            finished_at: None,
            target: "backup-storage".to_string(),
        },
    ];
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: jobs.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(jobs).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn create_job(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateBackupRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": format!("backup-{}", &Uuid::new_v4().to_string()[..8]),
            "task_id": Uuid::new_v4().to_string()
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn list_schedules(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().timestamp() as u64;
    
    let schedules = vec![
        BackupSchedule {
            id: "schedule-001".to_string(),
            name: "Daily production backup".to_string(),
            vms: vec!["vm-001".to_string(), "vm-002".to_string()],
            backup_type: "incremental".to_string(),
            schedule: "0 2 * * *".to_string(),
            target: "backup-storage".to_string(),
            retention_days: 30,
            enabled: true,
            last_run: Some(now - 86400),
            next_run: Some(now + 43200),
        },
        BackupSchedule {
            id: "schedule-002".to_string(),
            name: "Weekly full backup".to_string(),
            vms: vec!["vm-001".to_string(), "vm-002".to_string(), "vm-003".to_string()],
            backup_type: "full".to_string(),
            schedule: "0 3 * * 0".to_string(),
            target: "offsite-storage".to_string(),
            retention_days: 90,
            enabled: true,
            last_run: Some(now - 86400 * 3),
            next_run: Some(now + 86400 * 4),
        },
    ];
    
    Json(ApiResponse::success(schedules))
}
