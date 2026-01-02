//! License Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// License error
#[derive(Debug)]
pub enum LicenseError {
    /// IO error
    Io(std::io::Error),
    /// Parse error
    Parse(String),
    /// Invalid license
    Invalid(String),
    /// License expired
    Expired,
    /// Feature not available
    FeatureNotAvailable(String),
    /// Limit exceeded
    LimitExceeded(String),
    /// Activation failed
    ActivationFailed(String),
    /// Network error
    Network(String),
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse(s) => write!(f, "Parse error: {}", s),
            Self::Invalid(s) => write!(f, "Invalid license: {}", s),
            Self::Expired => write!(f, "License expired"),
            Self::FeatureNotAvailable(s) => write!(f, "Feature not available: {}", s),
            Self::LimitExceeded(s) => write!(f, "Limit exceeded: {}", s),
            Self::ActivationFailed(s) => write!(f, "Activation failed: {}", s),
            Self::Network(s) => write!(f, "Network error: {}", s),
        }
    }
}

impl std::error::Error for LicenseError {}

impl From<std::io::Error> for LicenseError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for LicenseError {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

/// License edition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    /// Free/Community edition
    Community,
    /// Standard edition
    Standard,
    /// Professional edition
    Professional,
    /// Enterprise edition
    Enterprise,
    /// Developer/Trial edition
    Developer,
}

impl Edition {
    pub fn display_name(&self) -> &'static str {
        match self {
            Edition::Community => "Community",
            Edition::Standard => "Standard",
            Edition::Professional => "Professional",
            Edition::Enterprise => "Enterprise",
            Edition::Developer => "Developer",
        }
    }
}

/// License status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseStatus {
    Valid,
    Expired,
    Invalid,
    GracePeriod,
    NotActivated,
}

/// License information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct License {
    /// License key
    pub key: String,
    /// Edition
    pub edition: Edition,
    /// Status
    pub status: LicenseStatus,
    /// Licensed to (company/organization)
    pub licensed_to: String,
    /// Contact email
    pub email: String,
    /// Issue date (Unix timestamp)
    pub issued_at: u64,
    /// Expiration date (Unix timestamp, 0 = perpetual)
    pub expires_at: u64,
    /// Last validation timestamp
    pub last_validated: u64,
    /// Maximum nodes
    pub max_nodes: u32,
    /// Maximum VMs
    pub max_vms: u32,
    /// Maximum sockets per node
    pub max_sockets: u32,
    /// Features enabled
    pub features: Vec<String>,
    /// Support tier
    pub support_tier: SupportTier,
    /// Subscription ID (for cloud/SaaS)
    pub subscription_id: Option<String>,
    /// Hardware fingerprint (for binding)
    pub hardware_id: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl Default for License {
    fn default() -> Self {
        Self::community()
    }
}

impl License {
    /// Create community (free) license
    pub fn community() -> Self {
        Self {
            key: "COMMUNITY".to_string(),
            edition: Edition::Community,
            status: LicenseStatus::Valid,
            licensed_to: "Community User".to_string(),
            email: String::new(),
            issued_at: 0,
            expires_at: 0, // Never expires
            last_validated: 0,
            max_nodes: 3,
            max_vms: 16,
            max_sockets: 4,
            features: community_features(),
            support_tier: SupportTier::Community,
            subscription_id: None,
            hardware_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Check if license is valid
    pub fn is_valid(&self) -> bool {
        matches!(self.status, LicenseStatus::Valid | LicenseStatus::GracePeriod)
    }

    /// Check if license is expired
    pub fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false; // Perpetual
        }
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now > self.expires_at
    }

    /// Get days until expiration
    pub fn days_until_expiration(&self) -> Option<i64> {
        if self.expires_at == 0 {
            return None; // Perpetual
        }
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let diff = self.expires_at as i64 - now as i64;
        Some(diff / 86400)
    }

    /// Check if feature is enabled
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// Support tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportTier {
    Community,
    Basic,
    Standard,
    Premium,
    Enterprise,
}

impl SupportTier {
    pub fn response_time_hours(&self) -> Option<u32> {
        match self {
            SupportTier::Community => None,
            SupportTier::Basic => Some(48),
            SupportTier::Standard => Some(24),
            SupportTier::Premium => Some(4),
            SupportTier::Enterprise => Some(1),
        }
    }
}

/// License activation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationRequest {
    pub license_key: String,
    pub hardware_id: String,
    pub hostname: String,
    pub node_count: u32,
    pub product_version: String,
}

/// License activation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationResponse {
    pub success: bool,
    pub license: Option<License>,
    pub error: Option<String>,
    pub message: Option<String>,
}

/// Community edition features
fn community_features() -> Vec<String> {
    vec![
        "vm_management".to_string(),
        "storage_local".to_string(),
        "network_bridge".to_string(),
        "snapshots".to_string(),
        "webgui".to_string(),
        "api".to_string(),
    ]
}

/// Standard edition features (includes community)
pub fn standard_features() -> Vec<String> {
    let mut features = community_features();
    features.extend(vec![
        "storage_nfs".to_string(),
        "storage_iscsi".to_string(),
        "live_migration".to_string(),
        "templates".to_string(),
        "backup_local".to_string(),
        "metrics".to_string(),
    ]);
    features
}

/// Professional edition features
pub fn professional_features() -> Vec<String> {
    let mut features = standard_features();
    features.extend(vec![
        "cluster".to_string(),
        "ha_basic".to_string(),
        "backup_remote".to_string(),
        "ldap".to_string(),
        "replication".to_string(),
        "storage_ceph".to_string(),
    ]);
    features
}

/// Enterprise edition features
pub fn enterprise_features() -> Vec<String> {
    let mut features = professional_features();
    features.extend(vec![
        "ha_advanced".to_string(),
        "dr".to_string(),
        "sso".to_string(),
        "audit".to_string(),
        "encryption".to_string(),
        "multi_tenant".to_string(),
        "api_rate_limit".to_string(),
        "custom_branding".to_string(),
        "priority_support".to_string(),
    ]);
    features
}
