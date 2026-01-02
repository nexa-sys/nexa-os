//! Dashboard handlers

use super::{ApiResponse, ResponseMeta};
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[cfg(feature = "webgui")]
use axum::{extract::State, Json, response::IntoResponse};

/// Dashboard overview data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardOverview {
    pub cluster: ClusterSummary,
    pub vms: VmSummary,
    pub storage: StorageSummary,
    pub network: NetworkSummary,
    pub recent_events: Vec<RecentEvent>,
    pub active_tasks: Vec<ActiveTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSummary {
    pub name: String,
    pub status: String,
    pub total_nodes: u32,
    pub online_nodes: u32,
    pub total_cpu_cores: u32,
    pub used_cpu_cores: u32,
    pub total_memory_gb: u64,
    pub used_memory_gb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSummary {
    pub total: u32,
    pub running: u32,
    pub stopped: u32,
    pub paused: u32,
    pub error: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSummary {
    pub pools: u32,
    pub total_tb: f64,
    pub used_tb: f64,
    pub volumes: u32,
    pub snapshots: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSummary {
    pub switches: u32,
    pub networks: u32,
    pub active_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEvent {
    pub id: String,
    pub timestamp: u64,
    pub severity: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTask {
    pub id: String,
    pub task_type: String,
    pub target: String,
    pub progress: f64,
    pub status: String,
}

/// Dashboard stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStats {
    pub cpu: ResourceStats,
    pub memory: ResourceStats,
    pub storage: ResourceStats,
    pub network: NetworkStats,
    pub history: Vec<HistoryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStats {
    pub current: f64,
    pub average: f64,
    pub peak: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub packets_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPoint {
    pub timestamp: u64,
    pub cpu: f64,
    pub memory: f64,
    pub disk_io: f64,
    pub network_io: f64,
}

/// Get dashboard overview
#[cfg(feature = "webgui")]
pub async fn overview(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    // In real implementation, gather data from hypervisor
    let overview = DashboardOverview {
        cluster: ClusterSummary {
            name: "default-cluster".to_string(),
            status: "healthy".to_string(),
            total_nodes: 3,
            online_nodes: 3,
            total_cpu_cores: 96,
            used_cpu_cores: 48,
            total_memory_gb: 384,
            used_memory_gb: 256,
        },
        vms: VmSummary {
            total: 25,
            running: 18,
            stopped: 5,
            paused: 1,
            error: 1,
        },
        storage: StorageSummary {
            pools: 4,
            total_tb: 100.0,
            used_tb: 45.5,
            volumes: 30,
            snapshots: 150,
        },
        network: NetworkSummary {
            switches: 3,
            networks: 8,
            active_connections: 25,
        },
        recent_events: vec![
            RecentEvent {
                id: "evt-001".to_string(),
                timestamp: chrono::Utc::now().timestamp() as u64 - 60,
                severity: "info".to_string(),
                source: "vm-web-01".to_string(),
                message: "VM started successfully".to_string(),
            },
        ],
        active_tasks: vec![],
    };
    
    Json(ApiResponse::success(overview))
}

/// Get dashboard stats
#[cfg(feature = "webgui")]
pub async fn stats(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let stats = DashboardStats {
        cpu: ResourceStats {
            current: 45.2,
            average: 42.5,
            peak: 78.3,
        },
        memory: ResourceStats {
            current: 66.7,
            average: 65.0,
            peak: 85.2,
        },
        storage: ResourceStats {
            current: 45.5,
            average: 44.0,
            peak: 45.5,
        },
        network: NetworkStats {
            rx_bps: 1_250_000_000,
            tx_bps: 850_000_000,
            packets_per_sec: 150_000,
        },
        history: vec![],
    };
    
    Json(ApiResponse::success(stats))
}
