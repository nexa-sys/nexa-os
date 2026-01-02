//! Feature Gating

use super::{License, LicenseValidator, LicenseError};
use parking_lot::RwLock;
use std::sync::Arc;

/// Feature gate for license-based access control
pub struct FeatureGate {
    validator: Arc<LicenseValidator>,
    /// Cached feature flags
    cache: RwLock<FeatureCache>,
}

#[derive(Debug, Clone, Default)]
struct FeatureCache {
    features: Vec<String>,
    last_updated: u64,
}

impl FeatureGate {
    pub fn new(validator: Arc<LicenseValidator>) -> Self {
        let cache = FeatureCache {
            features: validator.license().features.clone(),
            last_updated: now(),
        };
        
        Self {
            validator,
            cache: RwLock::new(cache),
        }
    }

    /// Check if feature is enabled
    pub fn is_enabled(&self, feature: &str) -> bool {
        self.refresh_cache();
        self.cache.read().features.iter().any(|f| f == feature)
    }

    /// Require feature (returns error if not enabled)
    pub fn require(&self, feature: &str) -> Result<(), LicenseError> {
        if self.is_enabled(feature) {
            Ok(())
        } else {
            Err(LicenseError::FeatureNotAvailable(feature.to_string()))
        }
    }

    /// Get all enabled features
    pub fn enabled_features(&self) -> Vec<String> {
        self.refresh_cache();
        self.cache.read().features.clone()
    }

    /// Refresh cache if stale
    fn refresh_cache(&self) {
        let mut cache = self.cache.write();
        let age = now() - cache.last_updated;
        
        // Refresh every 5 minutes
        if age > 300 {
            cache.features = self.validator.license().features.clone();
            cache.last_updated = now();
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Feature flag names
pub mod features {
    // Core features
    pub const VM_MANAGEMENT: &str = "vm_management";
    pub const SNAPSHOTS: &str = "snapshots";
    pub const TEMPLATES: &str = "templates";
    pub const WEBGUI: &str = "webgui";
    pub const API: &str = "api";
    
    // Storage features
    pub const STORAGE_LOCAL: &str = "storage_local";
    pub const STORAGE_NFS: &str = "storage_nfs";
    pub const STORAGE_ISCSI: &str = "storage_iscsi";
    pub const STORAGE_CEPH: &str = "storage_ceph";
    
    // Network features
    pub const NETWORK_BRIDGE: &str = "network_bridge";
    pub const NETWORK_VLAN: &str = "network_vlan";
    pub const NETWORK_SDN: &str = "network_sdn";
    
    // Migration features
    pub const LIVE_MIGRATION: &str = "live_migration";
    pub const REPLICATION: &str = "replication";
    
    // Backup features
    pub const BACKUP_LOCAL: &str = "backup_local";
    pub const BACKUP_REMOTE: &str = "backup_remote";
    
    // Cluster features
    pub const CLUSTER: &str = "cluster";
    pub const HA_BASIC: &str = "ha_basic";
    pub const HA_ADVANCED: &str = "ha_advanced";
    pub const DR: &str = "dr";
    
    // Security features
    pub const LDAP: &str = "ldap";
    pub const SSO: &str = "sso";
    pub const AUDIT: &str = "audit";
    pub const ENCRYPTION: &str = "encryption";
    
    // Multi-tenancy
    pub const MULTI_TENANT: &str = "multi_tenant";
    
    // Enterprise features
    pub const METRICS: &str = "metrics";
    pub const API_RATE_LIMIT: &str = "api_rate_limit";
    pub const CUSTOM_BRANDING: &str = "custom_branding";
    pub const PRIORITY_SUPPORT: &str = "priority_support";
}

/// Feature gate decorator macro
#[macro_export]
macro_rules! require_feature {
    ($gate:expr, $feature:expr) => {
        $gate.require($feature)?;
    };
}

/// Check multiple features (all must be enabled)
pub fn check_all(gate: &FeatureGate, required: &[&str]) -> Result<(), LicenseError> {
    for feature in required {
        gate.require(feature)?;
    }
    Ok(())
}

/// Check multiple features (any must be enabled)
pub fn check_any(gate: &FeatureGate, required: &[&str]) -> bool {
    required.iter().any(|f| gate.is_enabled(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_community_features() {
        let validator = Arc::new(LicenseValidator::new(PathBuf::from("/tmp/test-license.json")));
        let gate = FeatureGate::new(validator);
        
        // Community features should be enabled
        assert!(gate.is_enabled(features::VM_MANAGEMENT));
        assert!(gate.is_enabled(features::WEBGUI));
        
        // Enterprise features should not be enabled
        assert!(!gate.is_enabled(features::HA_ADVANCED));
        assert!(!gate.is_enabled(features::SSO));
    }
}
