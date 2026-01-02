//! Web Authentication System
//!
//! Session management, JWT tokens, and authentication middleware

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Session token (opaque string)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionToken(pub String);

impl SessionToken {
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// User session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: SessionToken,
    pub user_id: String,
    pub username: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub created_at: u64,
    pub expires_at: u64,
    pub last_activity: u64,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub csrf_token: String,
}

impl Session {
    pub fn new(user_id: &str, username: &str, roles: Vec<String>, ttl: Duration) -> Self {
        let now = chrono::Utc::now().timestamp() as u64;
        let permissions = Self::roles_to_permissions(&roles);
        
        Self {
            token: SessionToken::generate(),
            user_id: user_id.to_string(),
            username: username.to_string(),
            roles,
            permissions,
            created_at: now,
            expires_at: now + ttl.as_secs(),
            last_activity: now,
            ip_address: None,
            user_agent: None,
            csrf_token: Uuid::new_v4().to_string(),
        }
    }

    fn roles_to_permissions(roles: &[String]) -> Vec<String> {
        let mut perms = Vec::new();
        for role in roles {
            match role.as_str() {
                "admin" | "Administrator" => {
                    perms.extend(vec![
                        "vm.create", "vm.delete", "vm.start", "vm.stop", "vm.migrate",
                        "vm.snapshot", "vm.backup", "vm.console",
                        "storage.manage", "network.manage", "cluster.manage",
                        "user.manage", "system.manage", "audit.view"
                    ].into_iter().map(String::from));
                }
                "operator" | "Operator" => {
                    perms.extend(vec![
                        "vm.create", "vm.start", "vm.stop", "vm.snapshot", "vm.console",
                        "storage.view", "network.view"
                    ].into_iter().map(String::from));
                }
                "viewer" | "Viewer" | "readonly" => {
                    perms.extend(vec![
                        "vm.view", "storage.view", "network.view", "cluster.view"
                    ].into_iter().map(String::from));
                }
                _ => {}
            }
        }
        perms.sort();
        perms.dedup();
        perms
    }

    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now > self.expires_at
    }

    pub fn has_permission(&self, perm: &str) -> bool {
        self.permissions.contains(&perm.to_string()) ||
        self.permissions.contains(&"*".to_string())
    }

    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now().timestamp() as u64;
    }
}

/// Session manager
pub struct SessionManager {
    sessions: RwLock<HashMap<SessionToken, Session>>,
    user_sessions: RwLock<HashMap<String, Vec<SessionToken>>>,
    default_ttl: Duration,
    max_sessions_per_user: usize,
}

impl SessionManager {
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            user_sessions: RwLock::new(HashMap::new()),
            default_ttl,
            max_sessions_per_user: 10,
        }
    }

    /// Create new session
    pub fn create(
        &self,
        user_id: &str,
        username: &str,
        roles: Vec<String>,
    ) -> Session {
        // Remove excess sessions for user
        self.cleanup_user_sessions(user_id);
        
        let session = Session::new(user_id, username, roles, self.default_ttl);
        
        self.sessions.write().insert(session.token.clone(), session.clone());
        self.user_sessions.write()
            .entry(user_id.to_string())
            .or_insert_with(Vec::new)
            .push(session.token.clone());
        
        session
    }

    /// Get session by token
    pub fn get(&self, token: &SessionToken) -> Option<Session> {
        let sessions = self.sessions.read();
        sessions.get(token).filter(|s| !s.is_expired()).cloned()
    }

    /// Validate and get session (updates last activity)
    pub fn validate(&self, token: &SessionToken) -> Option<Session> {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(token) {
            if session.is_expired() {
                return None;
            }
            session.touch();
            return Some(session.clone());
        }
        None
    }

    /// Destroy session
    pub fn destroy(&self, token: &SessionToken) {
        if let Some(session) = self.sessions.write().remove(token) {
            if let Some(tokens) = self.user_sessions.write().get_mut(&session.user_id) {
                tokens.retain(|t| t != token);
            }
        }
    }

    /// Destroy all sessions for a user
    pub fn destroy_user_sessions(&self, user_id: &str) {
        if let Some(tokens) = self.user_sessions.write().remove(user_id) {
            let mut sessions = self.sessions.write();
            for token in tokens {
                sessions.remove(&token);
            }
        }
    }

    /// Cleanup expired sessions
    pub fn cleanup_expired(&self) {
        let expired: Vec<SessionToken> = self.sessions.read()
            .iter()
            .filter(|(_, s)| s.is_expired())
            .map(|(t, _)| t.clone())
            .collect();
        
        for token in expired {
            self.destroy(&token);
        }
    }

    fn cleanup_user_sessions(&self, user_id: &str) {
        let mut user_sessions = self.user_sessions.write();
        if let Some(tokens) = user_sessions.get_mut(user_id) {
            while tokens.len() >= self.max_sessions_per_user {
                if let Some(oldest) = tokens.first().cloned() {
                    tokens.remove(0);
                    self.sessions.write().remove(&oldest);
                }
            }
        }
    }

    /// Get active session count
    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    /// Get sessions for user
    pub fn user_sessions(&self, user_id: &str) -> Vec<Session> {
        let sessions = self.sessions.read();
        self.user_sessions.read()
            .get(user_id)
            .map(|tokens| {
                tokens.iter()
                    .filter_map(|t| sessions.get(t))
                    .filter(|s| !s.is_expired())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Web authentication manager (coordinates with security module)
pub struct WebAuthManager {
    session_manager: Arc<SessionManager>,
    jwt_secret: Vec<u8>,
    jwt_issuer: String,
    jwt_expiry: Duration,
}

impl WebAuthManager {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut secret = [0u8; 64];
        rng.fill(&mut secret);
        
        Self {
            session_manager,
            jwt_secret: secret.to_vec(),
            jwt_issuer: "nvm".to_string(),
            jwt_expiry: Duration::from_secs(3600),
        }
    }

    pub fn sessions(&self) -> Arc<SessionManager> {
        self.session_manager.clone()
    }

    /// Generate JWT token for API access
    pub fn generate_jwt(&self, session: &Session) -> Result<String, JwtError> {
        use jsonwebtoken::{encode, Header, EncodingKey, Algorithm};
        
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = JwtClaims {
            sub: session.user_id.clone(),
            iss: self.jwt_issuer.clone(),
            iat: now,
            exp: now + self.jwt_expiry.as_secs(),
            username: session.username.clone(),
            roles: session.roles.clone(),
        };
        
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.jwt_secret),
        ).map_err(|_| JwtError::EncodingFailed)
    }

    /// Validate JWT token
    pub fn validate_jwt(&self, token: &str) -> Result<JwtClaims, JwtError> {
        use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
        
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[&self.jwt_issuer]);
        
        let data = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(&self.jwt_secret),
            &validation,
        ).map_err(|_| JwtError::ValidationFailed)?;
        
        Ok(data.claims)
    }
}

/// JWT claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
    pub username: String,
    pub roles: Vec<String>,
}

/// JWT errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum JwtError {
    #[error("Failed to encode JWT")]
    EncodingFailed,
    #[error("Failed to validate JWT")]
    ValidationFailed,
    #[error("Token expired")]
    Expired,
}

/// Login request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub realm: Option<String>,
    pub totp: Option<String>,
}

/// Login response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub success: bool,
    pub token: Option<String>,
    pub csrf_token: Option<String>,
    pub user: Option<UserInfo>,
    pub error: Option<String>,
}

/// User info for responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}
