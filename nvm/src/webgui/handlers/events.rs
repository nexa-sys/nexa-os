//! Events and audit log handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Query, Json},
    response::IntoResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub timestamp: u64,
    pub event_type: String,
    pub severity: String,
    pub source: String,
    pub message: String,
    pub user: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: u64,
    pub user: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub ip_address: String,
    pub user_agent: Option<String>,
    pub result: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub severity: Option<String>,
    pub event_type: Option<String>,
    pub source: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuditQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub user: Option<String>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
}

#[cfg(feature = "webgui")]
pub async fn list(
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<EventQuery>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().timestamp() as u64;
    
    let events = vec![
        Event {
            id: "evt-001".to_string(),
            timestamp: now - 60,
            event_type: "vm.started".to_string(),
            severity: "info".to_string(),
            source: "vm-web-01".to_string(),
            message: "Virtual machine started successfully".to_string(),
            user: Some("admin".to_string()),
            target: Some("vm-001".to_string()),
        },
        Event {
            id: "evt-002".to_string(),
            timestamp: now - 300,
            event_type: "node.joined".to_string(),
            severity: "info".to_string(),
            source: "node-03".to_string(),
            message: "Node joined the cluster".to_string(),
            user: None,
            target: Some("node-03".to_string()),
        },
        Event {
            id: "evt-003".to_string(),
            timestamp: now - 600,
            event_type: "storage.warning".to_string(),
            severity: "warning".to_string(),
            source: "pool-local".to_string(),
            message: "Storage pool reaching 80% capacity".to_string(),
            user: None,
            target: Some("pool-local".to_string()),
        },
    ];
    
    let meta = ResponseMeta {
        page: params.pagination.page,
        per_page: params.pagination.per_page,
        total: events.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(events).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn audit_log(
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<AuditQuery>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().timestamp() as u64;
    
    let entries = vec![
        AuditEntry {
            id: "audit-001".to_string(),
            timestamp: now - 120,
            user: "admin".to_string(),
            action: "vm.create".to_string(),
            resource_type: "vm".to_string(),
            resource_id: "vm-005".to_string(),
            ip_address: "192.168.1.100".to_string(),
            user_agent: Some("Mozilla/5.0".to_string()),
            result: "success".to_string(),
            details: Some(serde_json::json!({
                "vm_name": "test-vm",
                "vcpus": 4,
                "memory_mb": 8192
            })),
        },
        AuditEntry {
            id: "audit-002".to_string(),
            timestamp: now - 180,
            user: "operator".to_string(),
            action: "vm.start".to_string(),
            resource_type: "vm".to_string(),
            resource_id: "vm-001".to_string(),
            ip_address: "192.168.1.101".to_string(),
            user_agent: Some("NVM CLI/2.0".to_string()),
            result: "success".to_string(),
            details: None,
        },
        AuditEntry {
            id: "audit-003".to_string(),
            timestamp: now - 300,
            user: "admin".to_string(),
            action: "user.create".to_string(),
            resource_type: "user".to_string(),
            resource_id: "user-new".to_string(),
            ip_address: "192.168.1.100".to_string(),
            user_agent: Some("Mozilla/5.0".to_string()),
            result: "success".to_string(),
            details: Some(serde_json::json!({
                "username": "newuser",
                "roles": ["operator"]
            })),
        },
    ];
    
    let meta = ResponseMeta {
        page: params.pagination.page,
        per_page: params.pagination.per_page,
        total: entries.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(entries).with_meta(meta))
}
