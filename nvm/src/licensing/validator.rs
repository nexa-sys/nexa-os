//! License Validator

use super::*;
use parking_lot::RwLock;
use std::path::PathBuf;

/// License validator
pub struct LicenseValidator {
    /// Current license
    license: RwLock<License>,
    /// License file path
    license_path: PathBuf,
    /// Activation server URL
    activation_server: Option<String>,
    /// Offline mode
    offline_mode: bool,
}

impl LicenseValidator {
    /// Create new validator
    pub fn new(license_path: PathBuf) -> Self {
        let license = Self::load_license(&license_path).unwrap_or_default();
        
        Self {
            license: RwLock::new(license),
            license_path,
            activation_server: None,
            offline_mode: false,
        }
    }

    /// Set activation server
    pub fn with_server(mut self, url: &str) -> Self {
        self.activation_server = Some(url.to_string());
        self
    }

    /// Set offline mode
    pub fn offline(mut self) -> Self {
        self.offline_mode = true;
        self
    }

    /// Load license from file
    fn load_license(path: &PathBuf) -> Option<License> {
        if !path.exists() {
            return None;
        }
        
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save license to file
    fn save_license(&self, license: &License) -> Result<(), LicenseError> {
        if let Some(parent) = self.license_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let content = serde_json::to_string_pretty(license)
            .map_err(|e| LicenseError::Serialization(e.to_string()))?;
        
        std::fs::write(&self.license_path, content)?;
        Ok(())
    }

    /// Get current license
    pub fn license(&self) -> License {
        self.license.read().clone()
    }

    /// Activate license with key
    pub fn activate(&self, key: &str) -> Result<License, LicenseError> {
        // Validate key format
        if !Self::validate_key_format(key) {
            return Err(LicenseError::InvalidKey("Invalid license key format".to_string()));
        }

        // In offline mode, decode and validate locally
        if self.offline_mode {
            return self.activate_offline(key);
        }

        // Online activation
        if let Some(ref server) = self.activation_server {
            self.activate_online(key, server)
        } else {
            self.activate_offline(key)
        }
    }

    /// Offline activation (decode key locally)
    fn activate_offline(&self, key: &str) -> Result<License, LicenseError> {
        // Parse license key (simplified - in production use proper encoding/signing)
        let license = Self::decode_license_key(key)?;
        
        // Validate
        if license.is_expired() {
            return Err(LicenseError::Expired);
        }

        // Save and update
        self.save_license(&license)?;
        *self.license.write() = license.clone();
        
        Ok(license)
    }

    /// Online activation
    fn activate_online(&self, key: &str, _server: &str) -> Result<License, LicenseError> {
        // In production, make HTTP request to activation server
        // For now, fall back to offline
        self.activate_offline(key)
    }

    /// Decode license key
    fn decode_license_key(key: &str) -> Result<License, LicenseError> {
        // Simplified key format: EDITION-XXXX-XXXX-XXXX
        // In production, use proper cryptographic signatures
        
        let parts: Vec<&str> = key.split('-').collect();
        if parts.len() < 4 {
            return Err(LicenseError::InvalidKey("Invalid key format".to_string()));
        }

        let edition = match parts[0].to_uppercase().as_str() {
            "COM" | "COMMUNITY" => Edition::Community,
            "STD" | "STANDARD" => Edition::Standard,
            "PRO" | "PROFESSIONAL" => Edition::Professional,
            "ENT" | "ENTERPRISE" => Edition::Enterprise,
            "DEV" | "DEVELOPER" => Edition::Developer,
            _ => return Err(LicenseError::InvalidKey("Unknown edition".to_string())),
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let features = match edition {
            Edition::Community => community_features(),
            Edition::Standard => standard_features(),
            Edition::Professional => professional_features(),
            Edition::Enterprise => enterprise_features(),
            Edition::Developer => enterprise_features(), // Developer gets all features
        };

        let (max_nodes, max_vms) = match edition {
            Edition::Community => (3, 16),
            Edition::Standard => (8, 64),
            Edition::Professional => (32, 256),
            Edition::Enterprise => (u32::MAX, u32::MAX),
            Edition::Developer => (4, 32),
        };

        Ok(License {
            key: key.to_string(),
            edition,
            status: LicenseStatus::Valid,
            licensed_to: "Licensed User".to_string(),
            email: String::new(),
            issued_at: now,
            expires_at: if edition == Edition::Developer {
                now + 30 * 86400 // 30 days for developer
            } else {
                0 // Perpetual for others
            },
            last_validated: now,
            max_nodes,
            max_vms,
            max_sockets: match edition {
                Edition::Community => 4,
                Edition::Standard => 8,
                _ => u32::MAX,
            },
            features,
            support_tier: match edition {
                Edition::Community => SupportTier::Community,
                Edition::Standard => SupportTier::Basic,
                Edition::Professional => SupportTier::Standard,
                Edition::Enterprise => SupportTier::Premium,
                Edition::Developer => SupportTier::Community,
            },
            subscription_id: None,
            hardware_id: None,
            metadata: std::collections::HashMap::new(),
        })
    }

    /// Validate key format
    fn validate_key_format(key: &str) -> bool {
        // Basic format check
        let parts: Vec<&str> = key.split('-').collect();
        parts.len() >= 4 && parts.iter().all(|p| !p.is_empty())
    }

    /// Validate current license
    pub fn validate(&self) -> Result<(), LicenseError> {
        let license = self.license.read();
        
        if license.is_expired() {
            return Err(LicenseError::Expired);
        }

        if !license.is_valid() {
            return Err(LicenseError::Invalid(license.status));
        }

        Ok(())
    }

    /// Check node limit
    pub fn check_node_limit(&self, current_nodes: u32) -> Result<(), LicenseError> {
        let license = self.license.read();
        
        if current_nodes > license.max_nodes {
            return Err(LicenseError::LimitExceeded {
                resource: "nodes".to_string(),
                current: current_nodes,
                limit: license.max_nodes,
            });
        }
        
        Ok(())
    }

    /// Check VM limit
    pub fn check_vm_limit(&self, current_vms: u32) -> Result<(), LicenseError> {
        let license = self.license.read();
        
        if current_vms > license.max_vms {
            return Err(LicenseError::LimitExceeded {
                resource: "vms".to_string(),
                current: current_vms,
                limit: license.max_vms,
            });
        }
        
        Ok(())
    }

    /// Deactivate license
    pub fn deactivate(&self) -> Result<(), LicenseError> {
        *self.license.write() = License::community();
        
        if self.license_path.exists() {
            std::fs::remove_file(&self.license_path)?;
        }
        
        Ok(())
    }

    /// Get hardware ID for license binding
    pub fn get_hardware_id() -> String {
        // In production, generate from hardware serial numbers
        // (motherboard, CPU, etc.) for tamper resistance
        
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        
        format!("NVM-{}", hash_string(&hostname))
    }
}

fn hash_string(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016X}", hasher.finish())
}

/// License errors
#[derive(Debug, thiserror::Error)]
pub enum LicenseError {
    #[error("License expired")]
    Expired,
    
    #[error("Invalid license key: {0}")]
    InvalidKey(String),
    
    #[error("Invalid license status: {0:?}")]
    Invalid(LicenseStatus),
    
    #[error("License limit exceeded: {resource} ({current}/{limit})")]
    LimitExceeded {
        resource: String,
        current: u32,
        limit: u32,
    },
    
    #[error("Feature not licensed: {0}")]
    FeatureNotLicensed(String),
    
    #[error("Activation failed: {0}")]
    ActivationFailed(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
}

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

impl Default for LicenseValidator {
    fn default() -> Self {
        Self::new(PathBuf::from("/etc/nvm/license.json"))
    }
}
