//! Task handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::{WebGuiState, TaskInfo, TaskStatus, TaskType};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json},
    response::IntoResponse,
};

#[cfg(feature = "webgui")]
pub async fn list(
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let tasks = state.tasks.read().values().cloned().collect::<Vec<_>>();
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: tasks.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(tasks).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Ok(uuid) = Uuid::parse_str(&id) {
        if let Some(task) = state.tasks.read().get(&uuid) {
            return Json(ApiResponse::success(task.clone()));
        }
    }
    
    Json(ApiResponse::<TaskInfo>::error(404, "Task not found"))
}

#[cfg(feature = "webgui")]
pub async fn cancel(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Ok(uuid) = Uuid::parse_str(&id) {
        if let Some(task) = state.tasks.write().get_mut(&uuid) {
            if task.status == TaskStatus::Running || task.status == TaskStatus::Pending {
                task.status = TaskStatus::Cancelled;
                return Json(ApiResponse::success(serde_json::json!({"cancelled": true})));
            }
        }
    }
    
    Json(ApiResponse::<serde_json::Value>::error(400, "Cannot cancel task"))
}
