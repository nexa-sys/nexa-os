//! Cluster handlers

use super::{ApiResponse, ResponseMeta};
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Json},
    http::StatusCode,
    response::IntoResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub hostname: String,
    pub ip_address: String,
    pub status: String,
    pub role: String,
    pub cpu_cores: u32,
    pub cpu_usage: f64,
    pub memory_total_mb: u64,
    pub memory_used_mb: u64,
    pub vm_count: u32,
    pub uptime: u64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub load_1m: f64,
    pub load_5m: f64,
    pub load_15m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub name: String,
    pub status: String,
    pub quorum: bool,
    pub nodes_total: u32,
    pub nodes_online: u32,
    pub ha_enabled: bool,
    pub drs_enabled: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinClusterRequest {
    pub cluster_address: String,
    pub token: String,
}

#[cfg(feature = "webgui")]
pub async fn list_nodes(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let nodes = vec![
        Node {
            id: "node-01".to_string(),
            hostname: "nvm-node-01".to_string(),
            ip_address: "192.168.1.10".to_string(),
            status: "online".to_string(),
            role: "leader".to_string(),
            cpu_cores: 32,
            cpu_usage: 45.5,
            memory_total_mb: 131072,
            memory_used_mb: 85000,
            vm_count: 12,
            uptime: 864000,
            version: "2.0.0".to_string(),
        },
        Node {
            id: "node-02".to_string(),
            hostname: "nvm-node-02".to_string(),
            ip_address: "192.168.1.11".to_string(),
            status: "online".to_string(),
            role: "follower".to_string(),
            cpu_cores: 32,
            cpu_usage: 38.2,
            memory_total_mb: 131072,
            memory_used_mb: 72000,
            vm_count: 8,
            uptime: 864000,
            version: "2.0.0".to_string(),
        },
    ];
    
    Json(ApiResponse::success(nodes))
}

#[cfg(feature = "webgui")]
pub async fn get_node(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let node = Node {
        id: id.clone(),
        hostname: format!("nvm-{}", id),
        ip_address: "192.168.1.10".to_string(),
        status: "online".to_string(),
        role: "leader".to_string(),
        cpu_cores: 32,
        cpu_usage: 45.5,
        memory_total_mb: 131072,
        memory_used_mb: 85000,
        vm_count: 12,
        uptime: 864000,
        version: "2.0.0".to_string(),
    };
    
    Json(ApiResponse::success(node))
}

#[cfg(feature = "webgui")]
pub async fn node_metrics(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let metrics = NodeMetrics {
        cpu_percent: 45.5,
        memory_percent: 64.8,
        disk_read_bps: 150_000_000,
        disk_write_bps: 80_000_000,
        net_rx_bps: 500_000_000,
        net_tx_bps: 350_000_000,
        load_1m: 4.5,
        load_5m: 4.2,
        load_15m: 3.8,
    };
    
    Json(ApiResponse::success(metrics))
}

#[cfg(feature = "webgui")]
pub async fn status(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let status = ClusterStatus {
        name: "default-cluster".to_string(),
        status: "healthy".to_string(),
        quorum: true,
        nodes_total: 3,
        nodes_online: 3,
        ha_enabled: true,
        drs_enabled: true,
        version: "2.0.0".to_string(),
    };
    
    Json(ApiResponse::success(status))
}

#[cfg(feature = "webgui")]
pub async fn join(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<JoinClusterRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "status": "joining",
        "message": "Node is joining the cluster"
    })))
}

#[cfg(feature = "webgui")]
pub async fn leave(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "status": "leaving",
        "message": "Node is leaving the cluster"
    })))
}
