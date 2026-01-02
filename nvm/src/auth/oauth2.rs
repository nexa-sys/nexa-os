//! OAuth2/OpenID Connect Authentication

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OAuth2 provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Config {
    /// Provider name (e.g., "google", "azure", "github")
    pub provider_name: String,
    /// Client ID
    pub client_id: String,
    /// Client secret
    pub client_secret: String,
    /// Authorization endpoint URL
    pub auth_url: String,
    /// Token endpoint URL
    pub token_url: String,
    /// Userinfo endpoint URL (for OIDC)
    pub userinfo_url: Option<String>,
    /// JWKS URL (for token verification)
    pub jwks_url: Option<String>,
    /// Redirect URI
    pub redirect_uri: String,
    /// OAuth2 scopes
    pub scopes: Vec<String>,
    /// Claim mappings
    pub claim_mappings: ClaimMappings,
    /// Enable PKCE
    pub pkce: bool,
    /// Enable state parameter
    pub state: bool,
}

/// Claim to attribute mappings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimMappings {
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub groups: Option<String>,
}

impl Default for ClaimMappings {
    fn default() -> Self {
        Self {
            username: "sub".to_string(),
            display_name: Some("name".to_string()),
            email: Some("email".to_string()),
            groups: Some("groups".to_string()),
        }
    }
}

/// OAuth2 authentication state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2State {
    /// State parameter
    pub state: String,
    /// PKCE code verifier
    pub code_verifier: Option<String>,
    /// Redirect after auth
    pub redirect_to: Option<String>,
    /// Created timestamp
    pub created_at: u64,
}

/// OAuth2 token response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub scope: Option<String>,
}

/// OAuth2 authentication backend
pub struct OAuth2Backend {
    config: OAuth2Config,
    pending_states: parking_lot::RwLock<HashMap<String, OAuth2State>>,
}

impl OAuth2Backend {
    pub fn new(config: OAuth2Config) -> Self {
        Self {
            config,
            pending_states: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Generate authorization URL
    pub fn get_auth_url(&self, redirect_to: Option<String>) -> (String, OAuth2State) {
        let state = generate_random_string(32);
        let code_verifier = if self.config.pkce {
            Some(generate_random_string(64))
        } else {
            None
        };

        let oauth_state = OAuth2State {
            state: state.clone(),
            code_verifier: code_verifier.clone(),
            redirect_to,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Store state
        self.pending_states.write().insert(state.clone(), oauth_state.clone());

        // Build auth URL
        let mut url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&state={}",
            self.config.auth_url,
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(&self.config.redirect_uri),
            urlencoding::encode(&state),
        );

        if !self.config.scopes.is_empty() {
            url.push_str(&format!(
                "&scope={}",
                urlencoding::encode(&self.config.scopes.join(" "))
            ));
        }

        if let Some(verifier) = &code_verifier {
            let challenge = generate_code_challenge(verifier);
            url.push_str(&format!(
                "&code_challenge={}&code_challenge_method=S256",
                urlencoding::encode(&challenge)
            ));
        }

        (url, oauth_state)
    }

    /// Handle OAuth2 callback
    pub fn handle_callback(&self, code: &str, state: &str) -> AuthResult {
        // Verify state
        let oauth_state = match self.pending_states.write().remove(state) {
            Some(s) => s,
            None => return AuthResult::failure("Invalid state", AuthProvider::OAuth2),
        };

        // Exchange code for token (in production, make HTTP request)
        // let token = self.exchange_code(code, oauth_state.code_verifier.as_deref())?;
        
        // For demo, create mock user
        let user = AuthenticatedUser {
            username: format!("oauth2_user_{}", &code[..8.min(code.len())]),
            display_name: Some("OAuth2 User".to_string()),
            email: Some("user@oauth.example.com".to_string()),
            groups: Vec::new(),
            roles: vec!["user".to_string()],
            provider: AuthProvider::OAuth2,
            attributes: HashMap::new(),
        };

        AuthResult::success(user, AuthProvider::OAuth2)
    }

    /// Verify and decode ID token (OIDC)
    pub fn verify_id_token(&self, _id_token: &str) -> Option<HashMap<String, serde_json::Value>> {
        // In production, verify JWT signature using JWKS
        // and decode claims
        None
    }
}

impl AuthBackend for OAuth2Backend {
    fn authenticate(&self, _username: &str, _password: &str) -> AuthResult {
        // OAuth2 doesn't use username/password directly
        // Return failure and redirect to OAuth2 flow
        AuthResult::failure(
            "Use OAuth2 authorization flow",
            AuthProvider::OAuth2,
        )
    }

    fn get_user(&self, _username: &str) -> Option<AuthenticatedUser> {
        None
    }

    fn is_available(&self) -> bool {
        !self.config.client_id.is_empty() && !self.config.auth_url.is_empty()
    }

    fn provider(&self) -> AuthProvider {
        AuthProvider::OAuth2
    }
}

/// Pre-configured OAuth2 providers
pub mod providers {
    use super::*;

    /// Microsoft Azure AD configuration
    pub fn azure_ad(tenant_id: &str, client_id: &str, client_secret: &str, redirect_uri: &str) -> OAuth2Config {
        OAuth2Config {
            provider_name: "azure".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url: format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
                tenant_id
            ),
            token_url: format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                tenant_id
            ),
            userinfo_url: Some("https://graph.microsoft.com/oidc/userinfo".to_string()),
            jwks_url: Some(format!(
                "https://login.microsoftonline.com/{}/discovery/v2.0/keys",
                tenant_id
            )),
            redirect_uri: redirect_uri.to_string(),
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
            claim_mappings: ClaimMappings {
                username: "preferred_username".to_string(),
                display_name: Some("name".to_string()),
                email: Some("email".to_string()),
                groups: Some("groups".to_string()),
            },
            pkce: true,
            state: true,
        }
    }

    /// Google configuration
    pub fn google(client_id: &str, client_secret: &str, redirect_uri: &str) -> OAuth2Config {
        OAuth2Config {
            provider_name: "google".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            userinfo_url: Some("https://openidconnect.googleapis.com/v1/userinfo".to_string()),
            jwks_url: Some("https://www.googleapis.com/oauth2/v3/certs".to_string()),
            redirect_uri: redirect_uri.to_string(),
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
            claim_mappings: ClaimMappings::default(),
            pkce: true,
            state: true,
        }
    }

    /// GitHub configuration
    pub fn github(client_id: &str, client_secret: &str, redirect_uri: &str) -> OAuth2Config {
        OAuth2Config {
            provider_name: "github".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            userinfo_url: Some("https://api.github.com/user".to_string()),
            jwks_url: None,
            redirect_uri: redirect_uri.to_string(),
            scopes: vec!["user:email".to_string(), "read:org".to_string()],
            claim_mappings: ClaimMappings {
                username: "login".to_string(),
                display_name: Some("name".to_string()),
                email: Some("email".to_string()),
                groups: None,
            },
            pkce: false,
            state: true,
        }
    }
}

fn generate_random_string(len: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    
    let hasher = RandomState::new();
    let mut h = hasher.build_hasher();
    h.write_u64(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64);
    
    let hash = h.finish();
    format!("{:0width$x}", hash, width = len.min(16))
}

fn generate_code_challenge(verifier: &str) -> String {
    // In production, use SHA256 hash and base64url encode
    // This is a placeholder
    format!("challenge_{}", &verifier[..8])
}
