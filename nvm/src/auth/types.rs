//! Authentication Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Authentication provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthProvider {
    Local,
    Ldap,
    OAuth2,
    Saml,
    Oidc,
}

/// Authentication result
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub success: bool,
    pub user: Option<AuthenticatedUser>,
    pub error: Option<String>,
    pub provider: AuthProvider,
}

impl AuthResult {
    pub fn success(user: AuthenticatedUser, provider: AuthProvider) -> Self {
        Self {
            success: true,
            user: Some(user),
            error: None,
            provider,
        }
    }

    pub fn failure(error: impl Into<String>, provider: AuthProvider) -> Self {
        Self {
            success: false,
            user: None,
            error: Some(error.into()),
            provider,
        }
    }
}

/// Authenticated user info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedUser {
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub groups: Vec<String>,
    pub roles: Vec<String>,
    pub provider: AuthProvider,
    pub attributes: HashMap<String, String>,
}

/// Authentication backend trait
pub trait AuthBackend: Send + Sync {
    /// Authenticate user with credentials
    fn authenticate(&self, username: &str, password: &str) -> AuthResult;
    
    /// Get user info (if already authenticated)
    fn get_user(&self, username: &str) -> Option<AuthenticatedUser>;
    
    /// List users (if supported)
    fn list_users(&self) -> Vec<AuthenticatedUser> {
        Vec::new()
    }
    
    /// List groups (if supported)
    fn list_groups(&self) -> Vec<String> {
        Vec::new()
    }
    
    /// Check if backend is available
    fn is_available(&self) -> bool {
        true
    }
    
    /// Get provider type
    fn provider(&self) -> AuthProvider;
}

/// Multi-backend authentication manager
pub struct AuthManager {
    backends: Vec<Box<dyn AuthBackend>>,
    default_provider: AuthProvider,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            default_provider: AuthProvider::Local,
        }
    }

    pub fn add_backend(&mut self, backend: Box<dyn AuthBackend>) {
        self.backends.push(backend);
    }

    pub fn set_default(&mut self, provider: AuthProvider) {
        self.default_provider = provider;
    }

    /// Authenticate against all backends (in order)
    pub fn authenticate(&self, username: &str, password: &str) -> AuthResult {
        // Try default provider first
        for backend in &self.backends {
            if backend.provider() == self.default_provider && backend.is_available() {
                let result = backend.authenticate(username, password);
                if result.success {
                    return result;
                }
            }
        }

        // Try other providers
        for backend in &self.backends {
            if backend.provider() != self.default_provider && backend.is_available() {
                let result = backend.authenticate(username, password);
                if result.success {
                    return result;
                }
            }
        }

        AuthResult::failure("Authentication failed", self.default_provider)
    }

    /// Authenticate against specific provider
    pub fn authenticate_with(&self, provider: AuthProvider, username: &str, password: &str) -> AuthResult {
        for backend in &self.backends {
            if backend.provider() == provider {
                return backend.authenticate(username, password);
            }
        }

        AuthResult::failure(format!("Provider {:?} not configured", provider), provider)
    }

    pub fn get_user(&self, username: &str) -> Option<AuthenticatedUser> {
        for backend in &self.backends {
            if let Some(user) = backend.get_user(username) {
                return Some(user);
            }
        }
        None
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}
