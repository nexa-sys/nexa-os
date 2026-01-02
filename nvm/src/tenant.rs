//! Multi-Tenant Management System
//!
//! Enterprise multi-tenancy features including:
//! - Tenant isolation
//! - Resource quotas and limits
//! - Usage tracking and billing
//! - Access control
//! - Tenant-specific configurations

use std::collections::HashMap;
use std::sync::{Arc, RwLock, atomic::{AtomicU64, Ordering}};
use std::time::{Instant, Duration};

use crate::hypervisor::{VmId, HypervisorResult, HypervisorError};

/// Tenant manager
pub struct TenantManager {
    /// Tenants
    tenants: RwLock<HashMap<TenantId, Tenant>>,
    /// Resource quotas
    quotas: RwLock<HashMap<TenantId, TenantQuota>>,
    /// Usage tracking
    usage: RwLock<HashMap<TenantId, UsageMetrics>>,
    /// Billing info
    billing: RwLock<HashMap<TenantId, BillingInfo>>,
    /// Configuration
    config: RwLock<TenantConfig>,
    /// Next tenant ID
    next_id: AtomicU64,
}

/// Tenant ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TenantId(u64);

impl TenantId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Tenant
#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub created_at: Instant,
    pub contact_email: String,
    pub contact_phone: Option<String>,
    pub organization: Option<String>,
    pub tags: HashMap<String, String>,
    pub vms: Vec<VmId>,
    pub networks: Vec<String>,
    pub storage_pools: Vec<String>,
}

/// Tenant configuration
#[derive(Debug, Clone)]
pub struct TenantConfig {
    /// Enable resource quotas
    pub enable_quotas: bool,
    /// Enable usage tracking
    pub enable_usage_tracking: bool,
    /// Enable billing
    pub enable_billing: bool,
    /// Default quota
    pub default_quota: ResourceQuota,
    /// Billing cycle (days)
    pub billing_cycle_days: u32,
    /// Allow tenant self-service
    pub allow_self_service: bool,
    /// Require approval for new VMs
    pub require_vm_approval: bool,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            enable_quotas: true,
            enable_usage_tracking: true,
            enable_billing: false,
            default_quota: ResourceQuota::default(),
            billing_cycle_days: 30,
            allow_self_service: true,
            require_vm_approval: false,
        }
    }
}

/// Tenant quota
#[derive(Debug, Clone)]
pub struct TenantQuota {
    pub tenant_id: TenantId,
    pub resource_quota: ResourceQuota,
    pub network_quota: NetworkQuota,
    pub storage_quota: StorageQuota,
}

/// Resource quota
#[derive(Debug, Clone)]
pub struct ResourceQuota {
    /// Maximum number of VMs
    pub max_vms: u32,
    /// Maximum total vCPUs
    pub max_vcpus: u32,
    /// Maximum total memory (MB)
    pub max_memory_mb: u64,
    /// Maximum running VMs
    pub max_running_vms: u32,
    /// Maximum vCPUs per VM
    pub max_vcpus_per_vm: u32,
    /// Maximum memory per VM (MB)
    pub max_memory_per_vm_mb: u64,
}

impl Default for ResourceQuota {
    fn default() -> Self {
        Self {
            max_vms: 10,
            max_vcpus: 32,
            max_memory_mb: 64 * 1024,
            max_running_vms: 5,
            max_vcpus_per_vm: 8,
            max_memory_per_vm_mb: 16 * 1024,
        }
    }
}

/// Network quota
#[derive(Debug, Clone)]
pub struct NetworkQuota {
    /// Maximum number of networks
    pub max_networks: u32,
    /// Maximum IPs per network
    pub max_ips_per_network: u32,
    /// Maximum bandwidth (Mbps)
    pub max_bandwidth_mbps: u64,
    /// Allow external networks
    pub allow_external: bool,
}

impl Default for NetworkQuota {
    fn default() -> Self {
        Self {
            max_networks: 3,
            max_ips_per_network: 100,
            max_bandwidth_mbps: 1000,
            allow_external: true,
        }
    }
}

/// Storage quota
#[derive(Debug, Clone)]
pub struct StorageQuota {
    /// Maximum total storage (GB)
    pub max_storage_gb: u64,
    /// Maximum number of disks
    pub max_disks: u32,
    /// Maximum disk size (GB)
    pub max_disk_size_gb: u64,
    /// Maximum snapshots
    pub max_snapshots: u32,
    /// Maximum backups
    pub max_backups: u32,
}

impl Default for StorageQuota {
    fn default() -> Self {
        Self {
            max_storage_gb: 500,
            max_disks: 20,
            max_disk_size_gb: 100,
            max_snapshots: 50,
            max_backups: 10,
        }
    }
}

/// Usage metrics
#[derive(Debug, Clone, Default)]
pub struct UsageMetrics {
    pub tenant_id: TenantId,
    pub period_start: Option<Instant>,
    pub period_end: Option<Instant>,
    
    // VM usage
    pub vm_count: u32,
    pub running_vm_count: u32,
    pub vcpu_hours: f64,
    pub memory_gb_hours: f64,
    
    // Storage usage
    pub storage_gb: f64,
    pub storage_gb_hours: f64,
    pub snapshot_count: u32,
    pub backup_count: u32,
    
    // Network usage
    pub network_rx_gb: f64,
    pub network_tx_gb: f64,
    pub network_count: u32,
    
    // Operations
    pub vm_creates: u64,
    pub vm_deletes: u64,
    pub snapshots_created: u64,
    pub backups_created: u64,
}

impl UsageMetrics {
    pub fn new(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            period_start: Some(Instant::now()),
            ..Default::default()
        }
    }
}

/// Billing information
#[derive(Debug, Clone)]
pub struct BillingInfo {
    pub tenant_id: TenantId,
    pub billing_type: BillingType,
    pub current_balance: f64,
    pub credit_limit: f64,
    pub currency: String,
    pub payment_method: Option<PaymentMethod>,
    pub invoices: Vec<Invoice>,
}

/// Billing type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillingType {
    /// No billing (free)
    Free,
    /// Prepaid credits
    Prepaid,
    /// Postpaid (invoice)
    Postpaid,
    /// Fixed monthly fee
    Fixed,
}

/// Payment method
#[derive(Debug, Clone)]
pub struct PaymentMethod {
    pub method_type: PaymentMethodType,
    pub last_four: Option<String>,
    pub expiry: Option<String>,
}

/// Payment method type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMethodType {
    CreditCard,
    BankTransfer,
    PayPal,
    Invoice,
}

/// Invoice
#[derive(Debug, Clone)]
pub struct Invoice {
    pub id: String,
    pub tenant_id: TenantId,
    pub period_start: Instant,
    pub period_end: Instant,
    pub amount: f64,
    pub currency: String,
    pub status: InvoiceStatus,
    pub items: Vec<InvoiceItem>,
    pub created_at: Instant,
    pub due_at: Instant,
    pub paid_at: Option<Instant>,
}

/// Invoice status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvoiceStatus {
    Draft,
    Pending,
    Paid,
    Overdue,
    Cancelled,
}

/// Invoice item
#[derive(Debug, Clone)]
pub struct InvoiceItem {
    pub description: String,
    pub quantity: f64,
    pub unit: String,
    pub unit_price: f64,
    pub amount: f64,
}

impl TenantManager {
    pub fn new() -> Self {
        Self {
            tenants: RwLock::new(HashMap::new()),
            quotas: RwLock::new(HashMap::new()),
            usage: RwLock::new(HashMap::new()),
            billing: RwLock::new(HashMap::new()),
            config: RwLock::new(TenantConfig::default()),
            next_id: AtomicU64::new(1),
        }
    }
    
    /// Configure tenant management
    pub fn configure(&self, config: TenantConfig) {
        *self.config.write().unwrap() = config;
    }
    
    /// Create tenant
    pub fn create_tenant(&self, name: &str, email: &str) -> HypervisorResult<TenantId> {
        let id = TenantId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let config = self.config.read().unwrap();
        
        let tenant = Tenant {
            id,
            name: name.to_string(),
            description: None,
            enabled: true,
            created_at: Instant::now(),
            contact_email: email.to_string(),
            contact_phone: None,
            organization: None,
            tags: HashMap::new(),
            vms: Vec::new(),
            networks: Vec::new(),
            storage_pools: Vec::new(),
        };
        
        self.tenants.write().unwrap().insert(id, tenant);
        
        // Set default quota
        let quota = TenantQuota {
            tenant_id: id,
            resource_quota: config.default_quota.clone(),
            network_quota: NetworkQuota::default(),
            storage_quota: StorageQuota::default(),
        };
        self.quotas.write().unwrap().insert(id, quota);
        
        // Initialize usage tracking
        self.usage.write().unwrap().insert(id, UsageMetrics::new(id));
        
        Ok(id)
    }
    
    /// Get tenant
    pub fn get_tenant(&self, id: TenantId) -> Option<Tenant> {
        self.tenants.read().unwrap().get(&id).cloned()
    }
    
    /// Update tenant
    pub fn update_tenant(&self, tenant: Tenant) -> HypervisorResult<()> {
        let mut tenants = self.tenants.write().unwrap();
        if !tenants.contains_key(&tenant.id) {
            return Err(HypervisorError::InvalidOperation("Tenant not found".to_string()));
        }
        tenants.insert(tenant.id, tenant);
        Ok(())
    }
    
    /// Delete tenant
    pub fn delete_tenant(&self, id: TenantId) -> HypervisorResult<()> {
        let tenant = self.get_tenant(id).ok_or_else(|| {
            HypervisorError::InvalidOperation("Tenant not found".to_string())
        })?;
        
        // Check if tenant has resources
        if !tenant.vms.is_empty() {
            return Err(HypervisorError::InvalidOperation(
                "Cannot delete tenant with VMs".to_string()
            ));
        }
        
        self.tenants.write().unwrap().remove(&id);
        self.quotas.write().unwrap().remove(&id);
        self.usage.write().unwrap().remove(&id);
        self.billing.write().unwrap().remove(&id);
        
        Ok(())
    }
    
    /// Enable/disable tenant
    pub fn set_tenant_enabled(&self, id: TenantId, enabled: bool) -> HypervisorResult<()> {
        let mut tenants = self.tenants.write().unwrap();
        if let Some(tenant) = tenants.get_mut(&id) {
            tenant.enabled = enabled;
            Ok(())
        } else {
            Err(HypervisorError::InvalidOperation("Tenant not found".to_string()))
        }
    }
    
    /// Set tenant quota
    pub fn set_quota(&self, id: TenantId, quota: TenantQuota) -> HypervisorResult<()> {
        if !self.tenants.read().unwrap().contains_key(&id) {
            return Err(HypervisorError::InvalidOperation("Tenant not found".to_string()));
        }
        self.quotas.write().unwrap().insert(id, quota);
        Ok(())
    }
    
    /// Get tenant quota
    pub fn get_quota(&self, id: TenantId) -> Option<TenantQuota> {
        self.quotas.read().unwrap().get(&id).cloned()
    }
    
    /// Check if operation is within quota
    pub fn check_quota(&self, id: TenantId, operation: QuotaCheckOperation) -> HypervisorResult<()> {
        let quota = self.get_quota(id).ok_or_else(|| {
            HypervisorError::InvalidOperation("Tenant quota not found".to_string())
        })?;
        
        let usage = self.usage.read().unwrap();
        let current = usage.get(&id);
        
        match operation {
            QuotaCheckOperation::CreateVm { vcpus, memory_mb } => {
                let rq = &quota.resource_quota;
                
                // Check VM count
                let current_vms = current.map(|u| u.vm_count).unwrap_or(0);
                if current_vms >= rq.max_vms {
                    return Err(HypervisorError::QuotaExceeded(
                        format!("Maximum VM count ({}) exceeded", rq.max_vms)
                    ));
                }
                
                // Check per-VM limits
                if vcpus > rq.max_vcpus_per_vm {
                    return Err(HypervisorError::QuotaExceeded(
                        format!("Maximum vCPUs per VM ({}) exceeded", rq.max_vcpus_per_vm)
                    ));
                }
                if memory_mb > rq.max_memory_per_vm_mb {
                    return Err(HypervisorError::QuotaExceeded(
                        format!("Maximum memory per VM ({} MB) exceeded", rq.max_memory_per_vm_mb)
                    ));
                }
            }
            QuotaCheckOperation::CreateDisk { size_gb } => {
                let sq = &quota.storage_quota;
                if size_gb > sq.max_disk_size_gb {
                    return Err(HypervisorError::QuotaExceeded(
                        format!("Maximum disk size ({} GB) exceeded", sq.max_disk_size_gb)
                    ));
                }
            }
            QuotaCheckOperation::CreateNetwork => {
                let nq = &quota.network_quota;
                let current_nets = current.map(|u| u.network_count).unwrap_or(0);
                if current_nets >= nq.max_networks {
                    return Err(HypervisorError::QuotaExceeded(
                        format!("Maximum network count ({}) exceeded", nq.max_networks)
                    ));
                }
            }
        }
        
        Ok(())
    }
    
    /// Record usage
    pub fn record_usage(&self, id: TenantId, event: UsageEvent) {
        let mut usage = self.usage.write().unwrap();
        if let Some(metrics) = usage.get_mut(&id) {
            match event {
                UsageEvent::VmCreated => {
                    metrics.vm_count += 1;
                    metrics.vm_creates += 1;
                }
                UsageEvent::VmDeleted => {
                    metrics.vm_count = metrics.vm_count.saturating_sub(1);
                    metrics.vm_deletes += 1;
                }
                UsageEvent::VmStarted => {
                    metrics.running_vm_count += 1;
                }
                UsageEvent::VmStopped => {
                    metrics.running_vm_count = metrics.running_vm_count.saturating_sub(1);
                }
                UsageEvent::SnapshotCreated => {
                    metrics.snapshot_count += 1;
                    metrics.snapshots_created += 1;
                }
                UsageEvent::BackupCreated => {
                    metrics.backup_count += 1;
                    metrics.backups_created += 1;
                }
                UsageEvent::NetworkTraffic { rx_bytes, tx_bytes } => {
                    metrics.network_rx_gb += rx_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                    metrics.network_tx_gb += tx_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                }
            }
        }
    }
    
    /// Get usage metrics
    pub fn get_usage(&self, id: TenantId) -> Option<UsageMetrics> {
        self.usage.read().unwrap().get(&id).cloned()
    }
    
    /// List all tenants
    pub fn list_tenants(&self) -> Vec<Tenant> {
        self.tenants.read().unwrap().values().cloned().collect()
    }
    
    /// Add VM to tenant
    pub fn add_vm_to_tenant(&self, tenant_id: TenantId, vm_id: VmId) -> HypervisorResult<()> {
        let mut tenants = self.tenants.write().unwrap();
        if let Some(tenant) = tenants.get_mut(&tenant_id) {
            tenant.vms.push(vm_id);
            Ok(())
        } else {
            Err(HypervisorError::InvalidOperation("Tenant not found".to_string()))
        }
    }
    
    /// Remove VM from tenant
    pub fn remove_vm_from_tenant(&self, tenant_id: TenantId, vm_id: VmId) -> HypervisorResult<()> {
        let mut tenants = self.tenants.write().unwrap();
        if let Some(tenant) = tenants.get_mut(&tenant_id) {
            tenant.vms.retain(|id| *id != vm_id);
            Ok(())
        } else {
            Err(HypervisorError::InvalidOperation("Tenant not found".to_string()))
        }
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Quota check operation
pub enum QuotaCheckOperation {
    CreateVm { vcpus: u32, memory_mb: u64 },
    CreateDisk { size_gb: u64 },
    CreateNetwork,
}

/// Usage event
pub enum UsageEvent {
    VmCreated,
    VmDeleted,
    VmStarted,
    VmStopped,
    SnapshotCreated,
    BackupCreated,
    NetworkTraffic { rx_bytes: u64, tx_bytes: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_tenant() {
        let manager = TenantManager::new();
        
        let id = manager.create_tenant("test-tenant", "test@example.com").unwrap();
        let tenant = manager.get_tenant(id).unwrap();
        
        assert_eq!(tenant.name, "test-tenant");
        assert!(tenant.enabled);
    }
    
    #[test]
    fn test_quota_check() {
        let manager = TenantManager::new();
        
        let id = manager.create_tenant("test", "test@example.com").unwrap();
        
        // Should pass - within limits
        manager.check_quota(id, QuotaCheckOperation::CreateVm { 
            vcpus: 4, 
            memory_mb: 8192 
        }).unwrap();
        
        // Should fail - too many vCPUs
        let result = manager.check_quota(id, QuotaCheckOperation::CreateVm { 
            vcpus: 100, 
            memory_mb: 8192 
        });
        assert!(result.is_err());
    }
    
    #[test]
    fn test_usage_tracking() {
        let manager = TenantManager::new();
        
        let id = manager.create_tenant("test", "test@example.com").unwrap();
        
        manager.record_usage(id, UsageEvent::VmCreated);
        manager.record_usage(id, UsageEvent::VmStarted);
        
        let usage = manager.get_usage(id).unwrap();
        assert_eq!(usage.vm_count, 1);
        assert_eq!(usage.running_vm_count, 1);
    }
}
