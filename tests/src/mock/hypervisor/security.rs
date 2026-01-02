//! Security and Isolation
//!
//! This module provides enterprise security features including:
//! - VM isolation and sandboxing
//! - Secure boot and measured boot
//! - TPM emulation
//! - Memory encryption (SEV-like)
//! - Security policies and auditing

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Instant, Duration};

use super::core::{VmId, HypervisorError, HypervisorResult};

// ============================================================================
// Security Manager
// ============================================================================

/// Central security manager
pub struct SecurityManager {
    /// Security policies
    policies: RwLock<HashMap<String, SecurityPolicy>>,
    /// VM security contexts
    vm_contexts: RwLock<HashMap<VmId, VmSecurityContext>>,
    /// TPM instances
    tpms: RwLock<HashMap<VmId, Arc<TpmEmulator>>>,
    /// Audit log
    audit_log: RwLock<Vec<AuditEvent>>,
    /// Configuration
    config: RwLock<SecurityConfig>,
    /// Statistics
    stats: RwLock<SecurityStats>,
}

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Enable secure boot enforcement
    pub enforce_secure_boot: bool,
    /// Enable TPM requirement
    pub require_tpm: bool,
    /// Enable memory encryption
    pub memory_encryption: bool,
    /// Enable audit logging
    pub audit_enabled: bool,
    /// Audit log retention (days)
    pub audit_retention_days: u32,
    /// Enable VM isolation
    pub vm_isolation: bool,
    /// Isolation level
    pub isolation_level: IsolationLevel,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enforce_secure_boot: false,
            require_tpm: false,
            memory_encryption: false,
            audit_enabled: true,
            audit_retention_days: 90,
            vm_isolation: true,
            isolation_level: IsolationLevel::Standard,
        }
    }
}

/// Isolation level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    /// Basic isolation
    Basic,
    /// Standard isolation (default)
    Standard,
    /// Enhanced isolation
    Enhanced,
    /// Maximum isolation (for sensitive workloads)
    Maximum,
}

/// Security statistics
#[derive(Debug, Clone, Default)]
pub struct SecurityStats {
    pub secure_boot_vms: u64,
    pub tpm_enabled_vms: u64,
    pub encrypted_vms: u64,
    pub policy_violations: u64,
    pub audit_events: u64,
}

impl SecurityManager {
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
            vm_contexts: RwLock::new(HashMap::new()),
            tpms: RwLock::new(HashMap::new()),
            audit_log: RwLock::new(Vec::new()),
            config: RwLock::new(SecurityConfig::default()),
            stats: RwLock::new(SecurityStats::default()),
        }
    }
    
    /// Configure security
    pub fn configure(&self, config: SecurityConfig) {
        *self.config.write().unwrap() = config;
    }
    
    // ========== Security Policies ==========
    
    /// Create security policy
    pub fn create_policy(&self, policy: SecurityPolicy) {
        self.policies.write().unwrap().insert(policy.name.clone(), policy);
    }
    
    /// Delete security policy
    pub fn delete_policy(&self, name: &str) {
        self.policies.write().unwrap().remove(name);
    }
    
    /// Apply policy to VM
    pub fn apply_policy(&self, vm_id: VmId, policy_name: &str) -> HypervisorResult<()> {
        let policies = self.policies.read().unwrap();
        let policy = policies.get(policy_name)
            .ok_or_else(|| HypervisorError::SecurityError(
                format!("Policy '{}' not found", policy_name)
            ))?;
        
        let mut contexts = self.vm_contexts.write().unwrap();
        let context = contexts.entry(vm_id).or_insert_with(|| VmSecurityContext::new(vm_id));
        context.applied_policies.push(policy_name.to_string());
        
        self.audit_event(
            AuditEventType::PolicyApplied,
            Some(vm_id),
            &format!("Policy '{}' applied to VM", policy_name),
        );
        
        Ok(())
    }
    
    /// Check policy compliance
    pub fn check_compliance(&self, vm_id: VmId) -> ComplianceResult {
        let contexts = self.vm_contexts.read().unwrap();
        let policies = self.policies.read().unwrap();
        
        let mut result = ComplianceResult {
            vm_id,
            compliant: true,
            violations: Vec::new(),
        };
        
        if let Some(context) = contexts.get(&vm_id) {
            for policy_name in &context.applied_policies {
                if let Some(policy) = policies.get(policy_name) {
                    let violations = self.check_policy_violations(context, policy);
                    if !violations.is_empty() {
                        result.compliant = false;
                        result.violations.extend(violations);
                    }
                }
            }
        }
        
        if !result.compliant {
            self.stats.write().unwrap().policy_violations += result.violations.len() as u64;
        }
        
        result
    }
    
    fn check_policy_violations(&self, context: &VmSecurityContext, policy: &SecurityPolicy) -> Vec<PolicyViolation> {
        let mut violations = Vec::new();
        
        // Check secure boot requirement
        if policy.require_secure_boot && !context.secure_boot_enabled {
            violations.push(PolicyViolation {
                policy: policy.name.clone(),
                rule: "require_secure_boot".to_string(),
                severity: ViolationSeverity::High,
                message: "Secure boot not enabled".to_string(),
            });
        }
        
        // Check TPM requirement
        if policy.require_tpm && !context.tpm_enabled {
            violations.push(PolicyViolation {
                policy: policy.name.clone(),
                rule: "require_tpm".to_string(),
                severity: ViolationSeverity::High,
                message: "TPM not enabled".to_string(),
            });
        }
        
        // Check encryption requirement
        if policy.require_encryption && !context.memory_encrypted {
            violations.push(PolicyViolation {
                policy: policy.name.clone(),
                rule: "require_encryption".to_string(),
                severity: ViolationSeverity::Critical,
                message: "Memory encryption not enabled".to_string(),
            });
        }
        
        violations
    }
    
    // ========== VM Security Context ==========
    
    /// Create security context for VM
    pub fn create_vm_context(&self, vm_id: VmId) -> VmSecurityContext {
        let context = VmSecurityContext::new(vm_id);
        self.vm_contexts.write().unwrap().insert(vm_id, context.clone());
        context
    }
    
    /// Get VM security context
    pub fn get_vm_context(&self, vm_id: VmId) -> Option<VmSecurityContext> {
        self.vm_contexts.read().unwrap().get(&vm_id).cloned()
    }
    
    /// Enable secure boot for VM
    pub fn enable_secure_boot(&self, vm_id: VmId, keys: SecureBootKeys) -> HypervisorResult<()> {
        let mut contexts = self.vm_contexts.write().unwrap();
        let context = contexts.entry(vm_id).or_insert_with(|| VmSecurityContext::new(vm_id));
        
        context.secure_boot_enabled = true;
        context.secure_boot_keys = Some(keys);
        
        self.stats.write().unwrap().secure_boot_vms += 1;
        
        self.audit_event(
            AuditEventType::SecureBootEnabled,
            Some(vm_id),
            "Secure boot enabled",
        );
        
        Ok(())
    }
    
    /// Verify secure boot
    pub fn verify_secure_boot(&self, vm_id: VmId, boot_hash: &[u8]) -> HypervisorResult<bool> {
        let contexts = self.vm_contexts.read().unwrap();
        
        if let Some(context) = contexts.get(&vm_id) {
            if !context.secure_boot_enabled {
                return Ok(true); // No secure boot = always pass
            }
            
            // Simplified verification
            if let Some(ref keys) = context.secure_boot_keys {
                // In real implementation, would verify against PKI
                let valid = !keys.db.is_empty();
                
                self.audit_event(
                    if valid { AuditEventType::SecureBootSuccess } else { AuditEventType::SecureBootFailure },
                    Some(vm_id),
                    &format!("Secure boot verification: {}", if valid { "passed" } else { "failed" }),
                );
                
                return Ok(valid);
            }
        }
        
        Ok(false)
    }
    
    // ========== TPM ==========
    
    /// Create TPM for VM
    pub fn create_tpm(&self, vm_id: VmId) -> HypervisorResult<()> {
        let tpm = Arc::new(TpmEmulator::new());
        self.tpms.write().unwrap().insert(vm_id, tpm);
        
        // Update context
        let mut contexts = self.vm_contexts.write().unwrap();
        let context = contexts.entry(vm_id).or_insert_with(|| VmSecurityContext::new(vm_id));
        context.tpm_enabled = true;
        
        self.stats.write().unwrap().tpm_enabled_vms += 1;
        
        self.audit_event(AuditEventType::TpmCreated, Some(vm_id), "TPM created");
        
        Ok(())
    }
    
    /// Get TPM for VM
    pub fn get_tpm(&self, vm_id: VmId) -> Option<Arc<TpmEmulator>> {
        self.tpms.read().unwrap().get(&vm_id).cloned()
    }
    
    /// Delete TPM for VM
    pub fn delete_tpm(&self, vm_id: VmId) {
        self.tpms.write().unwrap().remove(&vm_id);
        
        if let Some(context) = self.vm_contexts.write().unwrap().get_mut(&vm_id) {
            context.tpm_enabled = false;
        }
    }
    
    // ========== Memory Encryption ==========
    
    /// Enable memory encryption for VM
    pub fn enable_memory_encryption(&self, vm_id: VmId, config: MemoryEncryptionConfig) -> HypervisorResult<()> {
        let mut contexts = self.vm_contexts.write().unwrap();
        let context = contexts.entry(vm_id).or_insert_with(|| VmSecurityContext::new(vm_id));
        
        context.memory_encrypted = true;
        context.encryption_config = Some(config);
        
        self.stats.write().unwrap().encrypted_vms += 1;
        
        self.audit_event(
            AuditEventType::EncryptionEnabled,
            Some(vm_id),
            "Memory encryption enabled",
        );
        
        Ok(())
    }
    
    /// Disable memory encryption for VM
    pub fn disable_memory_encryption(&self, vm_id: VmId) {
        if let Some(context) = self.vm_contexts.write().unwrap().get_mut(&vm_id) {
            context.memory_encrypted = false;
            context.encryption_config = None;
        }
    }
    
    // ========== Audit ==========
    
    fn audit_event(&self, event_type: AuditEventType, vm_id: Option<VmId>, message: &str) {
        let config = self.config.read().unwrap();
        
        if !config.audit_enabled {
            return;
        }
        
        let event = AuditEvent {
            timestamp: Instant::now(),
            event_type,
            vm_id,
            message: message.to_string(),
            details: HashMap::new(),
        };
        
        self.audit_log.write().unwrap().push(event);
        self.stats.write().unwrap().audit_events += 1;
    }
    
    /// Get audit log
    pub fn get_audit_log(&self, limit: usize) -> Vec<AuditEvent> {
        self.audit_log.read().unwrap()
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
    
    /// Clear old audit entries
    pub fn cleanup_audit_log(&self) {
        let config = self.config.read().unwrap();
        let retention = Duration::from_secs(config.audit_retention_days as u64 * 24 * 3600);
        let cutoff = Instant::now() - retention;
        
        self.audit_log.write().unwrap().retain(|e| e.timestamp > cutoff);
    }
    
    /// Get statistics
    pub fn stats(&self) -> SecurityStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Security Policy
// ============================================================================

/// Security policy
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    pub name: String,
    pub description: String,
    /// Require secure boot
    pub require_secure_boot: bool,
    /// Require TPM
    pub require_tpm: bool,
    /// Require memory encryption
    pub require_encryption: bool,
    /// Minimum isolation level
    pub min_isolation_level: IsolationLevel,
    /// Allowed features
    pub allowed_features: AllowedFeatures,
    /// Network restrictions
    pub network_restrictions: NetworkRestrictions,
    /// Storage restrictions
    pub storage_restrictions: StorageRestrictions,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            description: "Default security policy".to_string(),
            require_secure_boot: false,
            require_tpm: false,
            require_encryption: false,
            min_isolation_level: IsolationLevel::Standard,
            allowed_features: AllowedFeatures::default(),
            network_restrictions: NetworkRestrictions::default(),
            storage_restrictions: StorageRestrictions::default(),
        }
    }
}

/// Allowed features
#[derive(Debug, Clone)]
pub struct AllowedFeatures {
    pub nested_virtualization: bool,
    pub device_passthrough: bool,
    pub usb_passthrough: bool,
    pub clipboard_sharing: bool,
    pub file_sharing: bool,
}

impl Default for AllowedFeatures {
    fn default() -> Self {
        Self {
            nested_virtualization: false,
            device_passthrough: false,
            usb_passthrough: true,
            clipboard_sharing: true,
            file_sharing: true,
        }
    }
}

/// Network restrictions
#[derive(Debug, Clone)]
pub struct NetworkRestrictions {
    pub allow_external: bool,
    pub allowed_networks: Vec<String>,
    pub blocked_ports: Vec<u16>,
    pub max_bandwidth: Option<u64>,
}

impl Default for NetworkRestrictions {
    fn default() -> Self {
        Self {
            allow_external: true,
            allowed_networks: Vec::new(),
            blocked_ports: Vec::new(),
            max_bandwidth: None,
        }
    }
}

/// Storage restrictions
#[derive(Debug, Clone)]
pub struct StorageRestrictions {
    pub allow_removable: bool,
    pub allowed_datastores: Vec<String>,
    pub max_disk_size: Option<u64>,
    pub require_encryption: bool,
}

impl Default for StorageRestrictions {
    fn default() -> Self {
        Self {
            allow_removable: true,
            allowed_datastores: Vec::new(),
            max_disk_size: None,
            require_encryption: false,
        }
    }
}

// ============================================================================
// VM Security Context
// ============================================================================

/// VM security context
#[derive(Debug, Clone)]
pub struct VmSecurityContext {
    pub vm_id: VmId,
    /// Secure boot enabled
    pub secure_boot_enabled: bool,
    /// Secure boot keys
    pub secure_boot_keys: Option<SecureBootKeys>,
    /// TPM enabled
    pub tpm_enabled: bool,
    /// Memory encrypted
    pub memory_encrypted: bool,
    /// Encryption configuration
    pub encryption_config: Option<MemoryEncryptionConfig>,
    /// Isolation level
    pub isolation_level: IsolationLevel,
    /// Applied policies
    pub applied_policies: Vec<String>,
    /// Security labels
    pub labels: HashMap<String, String>,
}

impl VmSecurityContext {
    pub fn new(vm_id: VmId) -> Self {
        Self {
            vm_id,
            secure_boot_enabled: false,
            secure_boot_keys: None,
            tpm_enabled: false,
            memory_encrypted: false,
            encryption_config: None,
            isolation_level: IsolationLevel::Standard,
            applied_policies: Vec::new(),
            labels: HashMap::new(),
        }
    }
}

// ============================================================================
// Secure Boot
// ============================================================================

/// Secure boot keys
#[derive(Debug, Clone)]
pub struct SecureBootKeys {
    /// Platform Key
    pub pk: Vec<u8>,
    /// Key Exchange Keys
    pub kek: Vec<Vec<u8>>,
    /// Signature Database
    pub db: Vec<Vec<u8>>,
    /// Forbidden Signature Database
    pub dbx: Vec<Vec<u8>>,
}

impl Default for SecureBootKeys {
    fn default() -> Self {
        Self {
            pk: Vec::new(),
            kek: Vec::new(),
            db: Vec::new(),
            dbx: Vec::new(),
        }
    }
}

// ============================================================================
// TPM Emulator
// ============================================================================

/// TPM emulator (TPM 2.0)
pub struct TpmEmulator {
    /// TPM state
    state: RwLock<TpmState>,
    /// PCR banks
    pcrs: RwLock<HashMap<u32, PcrBank>>,
    /// NV storage
    nv_storage: RwLock<HashMap<u32, Vec<u8>>>,
    /// Keys
    keys: RwLock<HashMap<u32, TpmKey>>,
}

/// TPM state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmState {
    Uninitialized,
    Ready,
    Locked,
    Failed,
}

/// PCR bank
#[derive(Debug, Clone)]
pub struct PcrBank {
    pub algorithm: HashAlgorithm,
    pub values: HashMap<u32, Vec<u8>>,
}

/// Hash algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    pub fn digest_size(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

/// TPM key
#[derive(Debug, Clone)]
pub struct TpmKey {
    pub handle: u32,
    pub key_type: TpmKeyType,
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}

/// TPM key type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmKeyType {
    Rsa2048,
    Rsa4096,
    EccP256,
    EccP384,
}

impl TpmEmulator {
    pub fn new() -> Self {
        let mut pcrs = HashMap::new();
        
        // Initialize SHA-256 PCR bank
        let mut sha256_bank = PcrBank {
            algorithm: HashAlgorithm::Sha256,
            values: HashMap::new(),
        };
        
        // Initialize PCRs 0-23
        for i in 0..24 {
            sha256_bank.values.insert(i, vec![0u8; 32]);
        }
        
        pcrs.insert(0, sha256_bank);
        
        Self {
            state: RwLock::new(TpmState::Uninitialized),
            pcrs: RwLock::new(pcrs),
            nv_storage: RwLock::new(HashMap::new()),
            keys: RwLock::new(HashMap::new()),
        }
    }
    
    /// Initialize TPM
    pub fn initialize(&self) -> HypervisorResult<()> {
        *self.state.write().unwrap() = TpmState::Ready;
        Ok(())
    }
    
    /// Get TPM state
    pub fn state(&self) -> TpmState {
        *self.state.read().unwrap()
    }
    
    /// Extend PCR
    pub fn pcr_extend(&self, pcr_index: u32, digest: &[u8]) -> HypervisorResult<()> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(HypervisorError::SecurityError("TPM not ready".to_string()));
        }
        
        let mut pcrs = self.pcrs.write().unwrap();
        
        if let Some(bank) = pcrs.get_mut(&0) {
            if let Some(current) = bank.values.get_mut(&pcr_index) {
                // PCR extend: new_value = hash(old_value || digest)
                let mut data = current.clone();
                data.extend_from_slice(digest);
                
                // Simplified hash (would use real SHA-256 in production)
                let new_value = self.simple_hash(&data, bank.algorithm.digest_size());
                *current = new_value;
                
                return Ok(());
            }
        }
        
        Err(HypervisorError::SecurityError("Invalid PCR index".to_string()))
    }
    
    /// Read PCR
    pub fn pcr_read(&self, pcr_index: u32) -> HypervisorResult<Vec<u8>> {
        let pcrs = self.pcrs.read().unwrap();
        
        if let Some(bank) = pcrs.get(&0) {
            if let Some(value) = bank.values.get(&pcr_index) {
                return Ok(value.clone());
            }
        }
        
        Err(HypervisorError::SecurityError("Invalid PCR index".to_string()))
    }
    
    /// Create key
    pub fn create_key(&self, key_type: TpmKeyType) -> HypervisorResult<u32> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(HypervisorError::SecurityError("TPM not ready".to_string()));
        }
        
        let mut keys = self.keys.write().unwrap();
        let handle = keys.len() as u32 + 0x80000000; // Transient key handle
        
        // Generate fake key material
        let key = TpmKey {
            handle,
            key_type,
            public_key: vec![0u8; 256],
            private_key: vec![0u8; 256],
        };
        
        keys.insert(handle, key);
        Ok(handle)
    }
    
    /// NV define space
    pub fn nv_define_space(&self, index: u32, size: usize) -> HypervisorResult<()> {
        let mut storage = self.nv_storage.write().unwrap();
        
        if storage.contains_key(&index) {
            return Err(HypervisorError::SecurityError("NV index already defined".to_string()));
        }
        
        storage.insert(index, vec![0u8; size]);
        Ok(())
    }
    
    /// NV write
    pub fn nv_write(&self, index: u32, data: &[u8]) -> HypervisorResult<()> {
        let mut storage = self.nv_storage.write().unwrap();
        
        if let Some(space) = storage.get_mut(&index) {
            if data.len() > space.len() {
                return Err(HypervisorError::SecurityError("Data too large for NV space".to_string()));
            }
            space[..data.len()].copy_from_slice(data);
            return Ok(());
        }
        
        Err(HypervisorError::SecurityError("NV index not defined".to_string()))
    }
    
    /// NV read
    pub fn nv_read(&self, index: u32) -> HypervisorResult<Vec<u8>> {
        let storage = self.nv_storage.read().unwrap();
        
        storage.get(&index)
            .cloned()
            .ok_or_else(|| HypervisorError::SecurityError("NV index not defined".to_string()))
    }
    
    fn simple_hash(&self, data: &[u8], size: usize) -> Vec<u8> {
        // Simple hash for testing (not cryptographically secure)
        let mut result = vec![0u8; size];
        for (i, &byte) in data.iter().enumerate() {
            result[i % size] ^= byte;
            result[(i + 1) % size] = result[(i + 1) % size].wrapping_add(byte);
        }
        result
    }
}

impl Default for TpmEmulator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Memory Encryption
// ============================================================================

/// Memory encryption configuration (SEV-like)
#[derive(Debug, Clone)]
pub struct MemoryEncryptionConfig {
    /// Encryption type
    pub encryption_type: MemoryEncryptionType,
    /// Encryption key (for simulation)
    pub key: Vec<u8>,
    /// Enable encrypted state (SEV-ES)
    pub encrypted_state: bool,
    /// Enable secure nested paging (SEV-SNP)
    pub secure_nested_paging: bool,
}

impl Default for MemoryEncryptionConfig {
    fn default() -> Self {
        Self {
            encryption_type: MemoryEncryptionType::Sev,
            key: vec![0u8; 32],
            encrypted_state: false,
            secure_nested_paging: false,
        }
    }
}

/// Memory encryption type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEncryptionType {
    /// AMD SEV
    Sev,
    /// AMD SEV-ES (Encrypted State)
    SevEs,
    /// AMD SEV-SNP (Secure Nested Paging)
    SevSnp,
    /// Intel TDX
    Tdx,
    /// Intel MKTME
    Mktme,
}

// ============================================================================
// Compliance
// ============================================================================

/// Compliance check result
#[derive(Debug, Clone)]
pub struct ComplianceResult {
    pub vm_id: VmId,
    pub compliant: bool,
    pub violations: Vec<PolicyViolation>,
}

/// Policy violation
#[derive(Debug, Clone)]
pub struct PolicyViolation {
    pub policy: String,
    pub rule: String,
    pub severity: ViolationSeverity,
    pub message: String,
}

/// Violation severity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

// ============================================================================
// Audit
// ============================================================================

/// Audit event
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub timestamp: Instant,
    pub event_type: AuditEventType,
    pub vm_id: Option<VmId>,
    pub message: String,
    pub details: HashMap<String, String>,
}

/// Audit event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEventType {
    /// Policy applied
    PolicyApplied,
    /// Policy removed
    PolicyRemoved,
    /// Compliance check
    ComplianceCheck,
    /// Secure boot enabled
    SecureBootEnabled,
    /// Secure boot verification success
    SecureBootSuccess,
    /// Secure boot verification failure
    SecureBootFailure,
    /// TPM created
    TpmCreated,
    /// TPM operation
    TpmOperation,
    /// Encryption enabled
    EncryptionEnabled,
    /// Security violation
    SecurityViolation,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_security_manager() {
        let manager = SecurityManager::new();
        
        // Create policy
        let policy = SecurityPolicy {
            name: "strict".to_string(),
            require_secure_boot: true,
            require_tpm: true,
            ..Default::default()
        };
        
        manager.create_policy(policy);
        
        // Create VM context
        let vm_id = VmId::new(1);
        manager.create_vm_context(vm_id);
        
        // Apply policy
        manager.apply_policy(vm_id, "strict").unwrap();
        
        // Check compliance (should fail - no secure boot or TPM)
        let result = manager.check_compliance(vm_id);
        assert!(!result.compliant);
        assert_eq!(result.violations.len(), 2);
    }
    
    #[test]
    fn test_tpm_emulator() {
        let tpm = TpmEmulator::new();
        
        // Initialize
        tpm.initialize().unwrap();
        assert_eq!(tpm.state(), TpmState::Ready);
        
        // Read initial PCR value
        let pcr0 = tpm.pcr_read(0).unwrap();
        assert_eq!(pcr0.len(), 32);
        assert!(pcr0.iter().all(|&b| b == 0));
        
        // Extend PCR
        tpm.pcr_extend(0, &[1, 2, 3, 4]).unwrap();
        
        let pcr0_after = tpm.pcr_read(0).unwrap();
        assert_ne!(pcr0, pcr0_after);
    }
    
    #[test]
    fn test_tpm_nv_storage() {
        let tpm = TpmEmulator::new();
        tpm.initialize().unwrap();
        
        // Define NV space
        tpm.nv_define_space(0x1500000, 32).unwrap();
        
        // Write data
        tpm.nv_write(0x1500000, &[1, 2, 3, 4]).unwrap();
        
        // Read data
        let data = tpm.nv_read(0x1500000).unwrap();
        assert_eq!(&data[..4], &[1, 2, 3, 4]);
    }
    
    #[test]
    fn test_secure_boot() {
        let manager = SecurityManager::new();
        let vm_id = VmId::new(1);
        
        manager.create_vm_context(vm_id);
        
        // Enable secure boot
        let keys = SecureBootKeys {
            pk: vec![1, 2, 3],
            db: vec![vec![4, 5, 6]],
            ..Default::default()
        };
        
        manager.enable_secure_boot(vm_id, keys).unwrap();
        
        // Verify
        let result = manager.verify_secure_boot(vm_id, &[0u8; 32]).unwrap();
        assert!(result);
    }
    
    #[test]
    fn test_memory_encryption() {
        let manager = SecurityManager::new();
        let vm_id = VmId::new(1);
        
        manager.create_vm_context(vm_id);
        
        // Enable encryption
        let config = MemoryEncryptionConfig {
            encryption_type: MemoryEncryptionType::SevSnp,
            encrypted_state: true,
            secure_nested_paging: true,
            ..Default::default()
        };
        
        manager.enable_memory_encryption(vm_id, config).unwrap();
        
        let context = manager.get_vm_context(vm_id).unwrap();
        assert!(context.memory_encrypted);
    }
}
