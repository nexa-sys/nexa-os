//! Dashboard Module

use serde::{Deserialize, Serialize};

/// Dashboard data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub cluster_status: String,
    pub total_nodes: u32,
    pub online_nodes: u32,
    pub total_vms: u32,
    pub running_vms: u32,
    pub total_cpu_cores: u32,
    pub used_cpu_percent: f64,
    pub total_memory_gb: u64,
    pub used_memory_gb: u64,
    pub total_storage_gb: u64,
    pub used_storage_gb: u64,
}

impl Default for DashboardData {
    fn default() -> Self {
        Self {
            cluster_status: "healthy".into(),
            total_nodes: 0,
            online_nodes: 0,
            total_vms: 0,
            running_vms: 0,
            total_cpu_cores: 0,
            used_cpu_percent: 0.0,
            total_memory_gb: 0,
            used_memory_gb: 0,
            total_storage_gb: 0,
            used_storage_gb: 0,
        }
    }
}
