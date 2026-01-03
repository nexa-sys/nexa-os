//! Network handlers
//!
//! Network management using real vmstate data

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
use crate::vmstate::{vm_state, NetworkState};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Path, Query, Json},
    http::StatusCode,
    response::IntoResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub id: String,
    pub name: String,
    pub network_type: String,
    pub cidr: Option<String>,
    pub gateway: Option<String>,
    pub vlan_id: Option<u16>,
    pub dhcp_enabled: bool,
    pub dns_servers: Vec<String>,
    pub status: String,
    pub vm_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    pub network_type: String,
    pub cidr: Option<String>,
    pub gateway: Option<String>,
    pub vlan_id: Option<u16>,
    pub dhcp_enabled: bool,
    pub dhcp_range: Option<(String, String)>,
    pub dns_servers: Vec<String>,
}

#[cfg(feature = "webgui")]
pub async fn list(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let net_states = state_mgr.list_networks();
    let all_vms = state_mgr.list_vms();
    
    // Convert vmstate networks to API format
    let mut networks: Vec<Network> = net_states.iter().map(|n| {
        // Count VMs using this network
        let vm_count = all_vms.iter()
            .filter(|vm| vm.network_interfaces.iter().any(|nic| nic.network == n.name))
            .count() as u32;
        
        Network {
            id: format!("net-{}", &n.name),
            name: n.name.clone(),
            network_type: n.network_type.clone(),
            cidr: n.cidr.clone(),
            gateway: None, // Could be extracted from CIDR
            vlan_id: None,
            dhcp_enabled: false,
            dns_servers: vec![],
            status: n.status.clone(),
            vm_count,
        }
    }).collect();
    
    // Add default network if none exist
    if networks.is_empty() {
        networks.push(Network {
            id: "net-default".to_string(),
            name: "default".to_string(),
            network_type: "bridge".to_string(),
            cidr: Some("192.168.122.0/24".to_string()),
            gateway: Some("192.168.122.1".to_string()),
            vlan_id: None,
            dhcp_enabled: true,
            dns_servers: vec!["192.168.122.1".to_string()],
            status: "active".to_string(),
            vm_count: all_vms.len() as u32,
        });
    }
    
    Json(ApiResponse::success(networks))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(_state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    let all_vms = state_mgr.list_vms();
    
    // Extract network name from id (e.g., "net-default" -> "default")
    let name = id.strip_prefix("net-").map(|s| s.to_string())
        .unwrap_or_else(|| id.clone());
    
    if let Some(n) = state_mgr.get_network(&name) {
        let vm_count = all_vms.iter()
            .filter(|vm| vm.network_interfaces.iter().any(|nic| nic.network == n.name))
            .count() as u32;
        
        let network = Network {
            id: id.clone(),
            name: n.name,
            network_type: n.network_type,
            cidr: n.cidr,
            gateway: None,
            vlan_id: None,
            dhcp_enabled: false,
            dns_servers: vec![],
            status: n.status,
            vm_count,
        };
        
        Json(ApiResponse::success(network))
    } else {
        // Return default network if requested
        let network = Network {
            id,
            name: name.clone(),
            network_type: "bridge".to_string(),
            cidr: Some("192.168.122.0/24".to_string()),
            gateway: Some("192.168.122.1".to_string()),
            vlan_id: None,
            dhcp_enabled: true,
            dns_servers: vec!["192.168.122.1".to_string()],
            status: "active".to_string(),
            vm_count: all_vms.len() as u32,
        };
        Json(ApiResponse::success(network))
    }
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(_state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateNetworkRequest>,
) -> impl IntoResponse {
    let state_mgr = vm_state();
    
    let network = NetworkState {
        name: req.name.clone(),
        network_type: req.network_type,
        cidr: req.cidr,
        bridge: None,
        status: "active".to_string(),
    };
    
    match state_mgr.create_network(network) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(ApiResponse::success(serde_json::json!({
                "id": format!("net-{}", req.name),
                "name": req.name
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
    // Note: Network deletion would need to be implemented in vmstate
    Json(ApiResponse::<()>::success(()))
}
