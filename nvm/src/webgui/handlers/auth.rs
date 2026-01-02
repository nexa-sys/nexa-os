//! Authentication handlers

use super::{ApiResponse, ApiError};
use crate::webgui::auth::{LoginRequest, LoginResponse, UserInfo, Session};
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};

/// Login handler
#[cfg(feature = "webgui")]
pub async fn login(
    State(state): State<Arc<WebGuiState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    // In real implementation, validate credentials against security module
    // For now, accept any non-empty credentials for demo
    if req.username.is_empty() || req.password.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                token: None,
                csrf_token: None,
                user: None,
                error: Some("Invalid credentials".to_string()),
            }),
        );
    }

    // Create session
    let roles = if req.username == "admin" {
        vec!["admin".to_string()]
    } else {
        vec!["operator".to_string()]
    };

    let session = state.sessions.create(&req.username, &req.username, roles.clone());
    
    (
        StatusCode::OK,
        Json(LoginResponse {
            success: true,
            token: Some(session.token.0.clone()),
            csrf_token: Some(session.csrf_token.clone()),
            user: Some(UserInfo {
                id: req.username.clone(),
                username: req.username,
                email: None,
                roles,
                permissions: session.permissions,
            }),
            error: None,
        }),
    )
}

/// Logout handler
#[cfg(feature = "webgui")]
pub async fn logout(
    State(state): State<Arc<WebGuiState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(auth) = headers.get("Authorization") {
        if let Ok(auth_str) = auth.to_str() {
            if auth_str.starts_with("Bearer ") {
                let token = crate::webgui::auth::SessionToken(auth_str[7..].to_string());
                state.sessions.destroy(&token);
            }
        }
    }
    
    Json(ApiResponse::<()>::success(()))
}

/// Refresh token handler
#[cfg(feature = "webgui")]
pub async fn refresh_token(
    State(state): State<Arc<WebGuiState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(auth) = headers.get("Authorization") {
        if let Ok(auth_str) = auth.to_str() {
            if auth_str.starts_with("Bearer ") {
                let token = crate::webgui::auth::SessionToken(auth_str[7..].to_string());
                if let Some(session) = state.sessions.validate(&token) {
                    return (
                        StatusCode::OK,
                        Json(LoginResponse {
                            success: true,
                            token: Some(session.token.0),
                            csrf_token: Some(session.csrf_token),
                            user: Some(UserInfo {
                                id: session.user_id,
                                username: session.username,
                                email: None,
                                roles: session.roles,
                                permissions: session.permissions,
                            }),
                            error: None,
                        }),
                    );
                }
            }
        }
    }
    
    (
        StatusCode::UNAUTHORIZED,
        Json(LoginResponse {
            success: false,
            token: None,
            csrf_token: None,
            user: None,
            error: Some("Invalid or expired token".to_string()),
        }),
    )
}

/// Get current user
#[cfg(feature = "webgui")]
pub async fn current_user(
    State(state): State<Arc<WebGuiState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(auth) = headers.get("Authorization") {
        if let Ok(auth_str) = auth.to_str() {
            if auth_str.starts_with("Bearer ") {
                let token = crate::webgui::auth::SessionToken(auth_str[7..].to_string());
                if let Some(session) = state.sessions.get(&token) {
                    return (
                        StatusCode::OK,
                        Json(ApiResponse::success(UserInfo {
                            id: session.user_id,
                            username: session.username,
                            email: None,
                            roles: session.roles,
                            permissions: session.permissions,
                        })),
                    );
                }
            }
        }
    }
    
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiResponse::<UserInfo>::error(401, "Not authenticated")),
    )
}
