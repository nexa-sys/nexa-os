//! Events and audit log handlers
//!
//! Real event data from vmstate event logging system

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::vm_state;
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
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<EventQuery>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    // Get events from state manager
    let limit = params.pagination.per_page as usize;
    let raw_events = if let Some(since) = params.from {
        state_mgr.get_events_since(since)
    } else {
        state_mgr.get_events(limit * 10) // Get more for filtering
    };
    
    // Convert and filter events
    let mut events: Vec<Event> = raw_events.iter()
        .filter(|e| {
            // Filter by severity if specified
            if let Some(ref sev) = params.severity {
                let event_sev = format!("{:?}", e.severity).to_lowercase();
                if &event_sev != sev {
                    return false;
                }
            }
            // Filter by event type if specified  
            if let Some(ref et) = params.event_type {
                if !e.event_type.to_string().contains(et) {
                    return false;
                }
            }
            // Filter by source if specified
            if let Some(ref src) = params.source {
                if !e.source.contains(src) {
                    return false;
                }
            }
            // Filter by time range
            if let Some(to) = params.to {
                if e.timestamp > to {
                    return false;
                }
            }
            true
        })
        .map(|e| Event {
            id: e.id.clone(),
            timestamp: e.timestamp,
            event_type: e.event_type.to_string(),
            severity: format!("{:?}", e.severity).to_lowercase(),
            source: e.source.clone(),
            message: e.message.clone(),
            user: e.user.clone(),
            target: e.details.as_ref()
                .and_then(|d| d.get("vm_id").or(d.get("vm_name")))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
        .take(limit)
        .collect();
    
    let total = events.len() as u64;
    let meta = ResponseMeta {
        page: params.pagination.page,
        per_page: params.pagination.per_page,
        total,
        total_pages: ((total as f64) / (params.pagination.per_page as f64)).ceil() as u32,
    };
    
    Json(ApiResponse::success(events).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn audit_log(
    State(_state): State<Arc<WebGuiState>>,
    Query(params): Query<AuditQuery>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    // Get events that have user information (audit-relevant)
    let limit = params.pagination.per_page as usize;
    let raw_events = state_mgr.get_events(limit * 10);
    
    // Convert events to audit entries (only events with user info or action-based)
    let entries: Vec<AuditEntry> = raw_events.iter()
        .filter(|e| {
            // Filter by user if specified
            if let Some(ref usr) = params.user {
                if e.user.as_ref().map(|u| u != usr).unwrap_or(true) {
                    return false;
                }
            }
            // Filter by action if specified
            if let Some(ref action) = params.action {
                if !e.event_type.to_string().contains(action) {
                    return false;
                }
            }
            // Filter by time range
            if let Some(from) = params.from {
                if e.timestamp < from {
                    return false;
                }
            }
            if let Some(to) = params.to {
                if e.timestamp > to {
                    return false;
                }
            }
            true
        })
        .map(|e| {
            let (resource_type, resource_id) = match &e.details {
                Some(d) => {
                    let rt = if d.get("vm_id").is_some() { "vm" }
                        else if d.get("pool_name").is_some() { "storage" }
                        else if d.get("network_name").is_some() { "network" }
                        else { "system" };
                    let ri = d.get("vm_id")
                        .or(d.get("pool_name"))
                        .or(d.get("network_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    (rt.to_string(), ri)
                }
                None => ("system".to_string(), "-".to_string()),
            };
            
            AuditEntry {
                id: format!("audit-{}", &e.id[4..]), // Convert evt-xxx to audit-xxx
                timestamp: e.timestamp,
                user: e.user.clone().unwrap_or_else(|| "system".to_string()),
                action: e.event_type.to_string(),
                resource_type,
                resource_id,
                ip_address: "127.0.0.1".to_string(), // Would need request context
                user_agent: None,
                result: if e.severity == crate::vmstate::EventSeverity::Error { 
                    "failure" 
                } else { 
                    "success" 
                }.to_string(),
                details: e.details.clone(),
            }
        })
        .take(limit)
        .collect();
    
    let total = entries.len() as u64;
    let meta = ResponseMeta {
        page: params.pagination.page,
        per_page: params.pagination.per_page,
        total,
        total_pages: ((total as f64) / (params.pagination.per_page as f64)).ceil() as u32,
    };
    
    Json(ApiResponse::success(entries).with_meta(meta))
}
