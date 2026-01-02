//! Network handlers

use super::{ApiResponse, ResponseMeta, PaginationParams};
use crate::webgui::server::WebGuiState;
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
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let networks = vec![
        Network {
            id: "net-default".to_string(),
            name: "default".to_string(),
            network_type: "bridge".to_string(),
            cidr: Some("192.168.122.0/24".to_string()),
            gateway: Some("192.168.122.1".to_string()),
            vlan_id: None,
            dhcp_enabled: true,
            dns_servers: vec!["192.168.122.1".to_string()],
            status: "active".to_string(),
            vm_count: 15,
        },
        Network {
            id: "net-prod".to_string(),
            name: "production".to_string(),
            network_type: "vlan".to_string(),
            cidr: Some("10.0.1.0/24".to_string()),
            gateway: Some("10.0.1.1".to_string()),
            vlan_id: Some(100),
            dhcp_enabled: false,
            dns_servers: vec!["10.0.0.10".to_string(), "10.0.0.11".to_string()],
            status: "active".to_string(),
            vm_count: 8,
        },
    ];
    
    Json(ApiResponse::success(networks))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let network = Network {
        id: id.clone(),
        name: "default".to_string(),
        network_type: "bridge".to_string(),
        cidr: Some("192.168.122.0/24".to_string()),
        gateway: Some("192.168.122.1".to_string()),
        vlan_id: None,
        dhcp_enabled: true,
        dns_servers: vec!["192.168.122.1".to_string()],
        status: "active".to_string(),
        vm_count: 15,
    };
    
    Json(ApiResponse::success(network))
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateNetworkRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": format!("net-{}", req.name)
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::<()>::success(()))
}
