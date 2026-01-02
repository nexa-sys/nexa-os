//! User management handlers

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
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub full_name: Option<String>,
    pub roles: Vec<String>,
    pub enabled: bool,
    pub realm: String,
    pub created_at: u64,
    pub last_login: Option<u64>,
    pub mfa_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub full_name: Option<String>,
    pub roles: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub full_name: Option<String>,
    pub roles: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub password: Option<String>,
}

#[cfg(feature = "webgui")]
pub async fn list(
    State(state): State<Arc<WebGuiState>>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let users = vec![
        User {
            id: "user-admin".to_string(),
            username: "admin".to_string(),
            email: Some("admin@example.com".to_string()),
            full_name: Some("Administrator".to_string()),
            roles: vec!["admin".to_string()],
            enabled: true,
            realm: "local".to_string(),
            created_at: 0,
            last_login: Some(chrono::Utc::now().timestamp() as u64 - 3600),
            mfa_enabled: true,
        },
        User {
            id: "user-operator".to_string(),
            username: "operator".to_string(),
            email: Some("operator@example.com".to_string()),
            full_name: Some("Operator User".to_string()),
            roles: vec!["operator".to_string()],
            enabled: true,
            realm: "local".to_string(),
            created_at: chrono::Utc::now().timestamp() as u64 - 86400 * 30,
            last_login: Some(chrono::Utc::now().timestamp() as u64 - 7200),
            mfa_enabled: false,
        },
    ];
    
    let meta = ResponseMeta {
        page: params.page,
        per_page: params.per_page,
        total: users.len() as u64,
        total_pages: 1,
    };
    
    Json(ApiResponse::success(users).with_meta(meta))
}

#[cfg(feature = "webgui")]
pub async fn get(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let user = User {
        id: id.clone(),
        username: "admin".to_string(),
        email: Some("admin@example.com".to_string()),
        full_name: Some("Administrator".to_string()),
        roles: vec!["admin".to_string()],
        enabled: true,
        realm: "local".to_string(),
        created_at: 0,
        last_login: Some(chrono::Utc::now().timestamp() as u64 - 3600),
        mfa_enabled: true,
    };
    
    Json(ApiResponse::success(user))
}

#[cfg(feature = "webgui")]
pub async fn create(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "id": format!("user-{}", req.username)
        }))),
    )
}

#[cfg(feature = "webgui")]
pub async fn update(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({"id": id})))
}

#[cfg(feature = "webgui")]
pub async fn delete(
    State(state): State<Arc<WebGuiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::<()>::success(()))
}

#[cfg(feature = "webgui")]
pub async fn list_roles(
    State(state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let roles = vec![
        Role {
            id: "role-admin".to_string(),
            name: "admin".to_string(),
            description: "Full administrative access".to_string(),
            permissions: vec!["*".to_string()],
            builtin: true,
        },
        Role {
            id: "role-operator".to_string(),
            name: "operator".to_string(),
            description: "VM operations and basic management".to_string(),
            permissions: vec![
                "vm.create".to_string(), "vm.start".to_string(),
                "vm.stop".to_string(), "vm.console".to_string(),
            ],
            builtin: true,
        },
        Role {
            id: "role-viewer".to_string(),
            name: "viewer".to_string(),
            description: "Read-only access".to_string(),
            permissions: vec!["*.view".to_string()],
            builtin: true,
        },
    ];
    
    Json(ApiResponse::success(roles))
}
