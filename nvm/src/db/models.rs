//! Database Models - Enterprise Data Structures
//!
//! Defines all database models for the NVM platform.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// User Management Models
// ============================================================================

/// User account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::FromRow))]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: Option<String>,
    pub role: UserRole,
    pub is_active: bool,
    pub is_locked: bool,
    pub failed_login_attempts: i32,
    pub last_login: Option<DateTime<Utc>>,
    pub last_password_change: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

/// User role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(feature = "database", sqlx(type_name = "user_role", rename_all = "lowercase"))]
pub enum UserRole {
    Admin,
    Operator,
    Viewer,
    Auditor,
}

impl UserRole {
    pub fn permissions(&self) -> Vec<&'static str> {
        match self {
            UserRole::Admin => vec![
                "vm.*", "storage.*", "network.*", "cluster.*",
                "user.*", "system.*", "audit.*", "backup.*",
            ],
            UserRole::Operator => vec![
                "vm.create", "vm.start", "vm.stop", "vm.console", "vm.snapshot",
                "storage.view", "network.view", "cluster.view",
            ],
            UserRole::Viewer => vec![
                "vm.view", "storage.view", "network.view", "cluster.view",
            ],
            UserRole::Auditor => vec![
                "vm.view", "storage.view", "network.view", "cluster.view",
                "audit.view", "audit.export",
            ],
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRole::Admin => write!(f, "admin"),
            UserRole::Operator => write!(f, "operator"),
            UserRole::Viewer => write!(f, "viewer"),
            UserRole::Auditor => write!(f, "auditor"),
        }
    }
}

/// Create user request
#[derive(Debug, Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub role: UserRole,
}

/// Update user request
#[derive(Debug, Deserialize)]
pub struct UpdateUser {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role: Option<UserRole>,
    pub is_active: Option<bool>,
}

// ============================================================================
// Session Management Models
// ============================================================================

/// User session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::FromRow))]
pub struct DbSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub csrf_token: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub is_revoked: bool,
}

/// Create session request
pub struct CreateSession {
    pub user_id: Uuid,
    pub token: String,
    pub csrf_token: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub expires_at: DateTime<Utc>,
}

// ============================================================================
// Audit Log Models
// ============================================================================

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::FromRow))]
pub struct AuditLog {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub resource_name: Option<String>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub status: AuditStatus,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(feature = "database", sqlx(type_name = "audit_status", rename_all = "lowercase"))]
pub enum AuditStatus {
    Success,
    Failure,
    Denied,
}

/// Create audit log entry
pub struct CreateAuditLog {
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub resource_name: Option<String>,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub status: AuditStatus,
    pub error_message: Option<String>,
}

// ============================================================================
// VM Models (Database-backed)
// ============================================================================

/// Virtual Machine record
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::FromRow))]
pub struct DbVm {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub vcpus: i32,
    pub memory_mb: i64,
    pub disk_gb: i64,
    pub status: String,
    pub os_type: Option<String>,
    pub template_id: Option<Uuid>,
    pub node_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub tags: Option<serde_json::Value>,
    pub config: Option<serde_json::Value>,
}

// ============================================================================
// System Settings Models
// ============================================================================

/// System setting
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::FromRow))]
pub struct Setting {
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub category: String,
    pub is_secret: bool,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

// ============================================================================
// API Key Models
// ============================================================================

/// API Key for programmatic access
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "database", derive(sqlx::FromRow))]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub key_hash: String,
    pub prefix: String,  // First 8 chars for identification
    pub permissions: serde_json::Value,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub is_revoked: bool,
}

// ============================================================================
// Response/DTO Models
// ============================================================================

/// User response (without sensitive data)
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub role: UserRole,
    pub is_active: bool,
    pub last_login: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            role: u.role,
            is_active: u.is_active,
            last_login: u.last_login,
            created_at: u.created_at,
        }
    }
}
