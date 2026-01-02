//! LDAP/Active Directory Authentication

use super::*;
use serde::{Deserialize, Serialize};

/// LDAP configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapConfig {
    /// LDAP server URL (ldap:// or ldaps://)
    pub url: String,
    /// Base DN for searches
    pub base_dn: String,
    /// Bind DN (admin account)
    pub bind_dn: Option<String>,
    /// Bind password
    pub bind_password: Option<String>,
    /// User search filter (e.g., "(sAMAccountName={username})")
    pub user_filter: String,
    /// User attribute for username
    pub username_attr: String,
    /// User attribute for display name
    pub display_name_attr: String,
    /// User attribute for email
    pub email_attr: String,
    /// Group membership attribute
    pub member_of_attr: String,
    /// Enable TLS
    pub starttls: bool,
    /// Skip TLS verification (not recommended)
    pub skip_verify: bool,
    /// Connection timeout (seconds)
    pub timeout: u64,
    /// Group to role mappings
    pub group_mappings: std::collections::HashMap<String, Vec<String>>,
}

impl Default for LdapConfig {
    fn default() -> Self {
        Self {
            url: "ldap://localhost:389".to_string(),
            base_dn: "dc=example,dc=com".to_string(),
            bind_dn: None,
            bind_password: None,
            user_filter: "(sAMAccountName={username})".to_string(),
            username_attr: "sAMAccountName".to_string(),
            display_name_attr: "displayName".to_string(),
            email_attr: "mail".to_string(),
            member_of_attr: "memberOf".to_string(),
            starttls: false,
            skip_verify: false,
            timeout: 10,
            group_mappings: std::collections::HashMap::new(),
        }
    }
}

/// LDAP authentication backend
pub struct LdapBackend {
    config: LdapConfig,
}

impl LdapBackend {
    pub fn new(config: LdapConfig) -> Self {
        Self { config }
    }

    fn map_groups_to_roles(&self, groups: &[String]) -> Vec<String> {
        let mut roles = Vec::new();
        
        for group in groups {
            // Extract CN from full DN
            let cn = group
                .split(',')
                .find(|s| s.to_lowercase().starts_with("cn="))
                .map(|s| s[3..].to_string())
                .unwrap_or_else(|| group.clone());
            
            if let Some(mapped_roles) = self.config.group_mappings.get(&cn) {
                roles.extend(mapped_roles.clone());
            }
        }
        
        // Default role if none mapped
        if roles.is_empty() {
            roles.push("user".to_string());
        }
        
        roles.sort();
        roles.dedup();
        roles
    }
}

impl AuthBackend for LdapBackend {
    fn authenticate(&self, username: &str, password: &str) -> AuthResult {
        // In production, use ldap3 crate for actual LDAP connection
        // This is a stub implementation
        
        if username.is_empty() || password.is_empty() {
            return AuthResult::failure("Empty credentials", AuthProvider::Ldap);
        }

        // Placeholder - would actually bind to LDAP server
        // let conn = LdapConn::with_settings(settings).connect(&self.config.url)?;
        // conn.simple_bind(&user_dn, password)?;
        
        // For demo purposes, return a mock successful auth
        let user = AuthenticatedUser {
            username: username.to_string(),
            display_name: Some(format!("{} (LDAP)", username)),
            email: Some(format!("{}@example.com", username)),
            groups: vec!["Domain Users".to_string()],
            roles: vec!["user".to_string()],
            provider: AuthProvider::Ldap,
            attributes: std::collections::HashMap::new(),
        };

        AuthResult::success(user, AuthProvider::Ldap)
    }

    fn get_user(&self, username: &str) -> Option<AuthenticatedUser> {
        // Would query LDAP for user info
        Some(AuthenticatedUser {
            username: username.to_string(),
            display_name: None,
            email: None,
            groups: Vec::new(),
            roles: vec!["user".to_string()],
            provider: AuthProvider::Ldap,
            attributes: std::collections::HashMap::new(),
        })
    }

    fn list_groups(&self) -> Vec<String> {
        // Would query LDAP for groups
        Vec::new()
    }

    fn is_available(&self) -> bool {
        // Would check LDAP connection
        true
    }

    fn provider(&self) -> AuthProvider {
        AuthProvider::Ldap
    }
}

/// Active Directory specific helper functions
pub mod ad {
    use super::*;

    /// Create LDAP config for Active Directory
    pub fn create_ad_config(
        domain: &str,
        dc_hostname: &str,
        admin_user: Option<&str>,
        admin_password: Option<&str>,
    ) -> LdapConfig {
        let domain_parts: Vec<&str> = domain.split('.').collect();
        let base_dn = domain_parts
            .iter()
            .map(|p| format!("DC={}", p))
            .collect::<Vec<_>>()
            .join(",");

        let bind_dn = admin_user.map(|u| {
            if u.contains('@') {
                u.to_string()
            } else {
                format!("{}@{}", u, domain)
            }
        });

        LdapConfig {
            url: format!("ldaps://{}:636", dc_hostname),
            base_dn,
            bind_dn,
            bind_password: admin_password.map(|s| s.to_string()),
            user_filter: "(sAMAccountName={username})".to_string(),
            username_attr: "sAMAccountName".to_string(),
            display_name_attr: "displayName".to_string(),
            email_attr: "mail".to_string(),
            member_of_attr: "memberOf".to_string(),
            starttls: false,
            skip_verify: false,
            timeout: 10,
            group_mappings: default_ad_group_mappings(),
        }
    }

    /// Default AD group to NVM role mappings
    pub fn default_ad_group_mappings() -> std::collections::HashMap<String, Vec<String>> {
        let mut mappings = std::collections::HashMap::new();
        
        mappings.insert(
            "Domain Admins".to_string(),
            vec!["admin".to_string()],
        );
        mappings.insert(
            "NVM Administrators".to_string(),
            vec!["admin".to_string()],
        );
        mappings.insert(
            "NVM Operators".to_string(),
            vec!["operator".to_string()],
        );
        mappings.insert(
            "NVM Users".to_string(),
            vec!["user".to_string()],
        );
        mappings.insert(
            "NVM Auditors".to_string(),
            vec!["auditor".to_string()],
        );
        
        mappings
    }
}
