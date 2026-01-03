//! Enterprise-Grade Hypervisor Framework
//!
//! This module provides a comprehensive virtualization platform similar to QEMU-KVM,
//! Hyper-V, or VMware ESXi. It's designed to be production-ready and can be extracted
//! as a standalone enterprise virtualization product.
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────────────┐
//! │                       Enterprise Hypervisor Platform                           │
//! ├────────────────────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────────────────────────┐  │
//! │  │                      Management Layer (REST API)                        │  │
//! │  │  • VM Lifecycle Management  • Resource Scheduling  • Cluster Management │  │
//! │  │  • Live Migration          • High Availability    • Monitoring & Alerts │  │
//! │  └─────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐  │
//! │  │                         VM Manager (Orchestration)                      │  │
//! │  │  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐ │  │
//! │  │  │    VM 1   │ │    VM 2   │ │    VM 3   │ │    VM 4   │ │    ...    │ │  │
//! │  │  └───────────┘ └───────────┘ └───────────┘ └───────────┘ └───────────┘ │  │
//! │  └─────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐  │
//! │  │                    Resource Pool Manager                                │  │
//! │  │  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐           │  │
//! │  │  │    CPU Pool     │ │   Memory Pool   │ │  Storage Pool   │           │  │
//! │  │  │  • vCPU alloc   │ │  • Balloon      │ │  • QCOW2/VMDK   │           │  │
//! │  │  │  • Pinning      │ │  • KSM          │ │  • Snapshots    │           │  │
//! │  │  │  • Overcommit   │ │  • NUMA         │ │  • Migration    │           │  │
//! │  │  └─────────────────┘ └─────────────────┘ └─────────────────┘           │  │
//! │  │  ┌─────────────────┐ ┌─────────────────┐                               │  │
//! │  │  │  Network Pool   │ │   Device Pool   │                               │  │
//! │  │  │  • vSwitch      │ │  • Passthrough  │                               │  │
//! │  │  │  • VLAN         │ │  • Mediated     │                               │  │
//! │  │  │  • SR-IOV       │ │  • Emulated     │                               │  │
//! │  │  └─────────────────┘ └─────────────────┘                               │  │
//! │  └─────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐  │
//! │  │                  Hardware Abstraction Layer (HAL)                       │  │
//! │  │  • CPU Emulation  • Memory Management  • Device Emulation  • Interrupts│  │
//! │  └─────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐  │
//! │  │                     Security & Isolation Layer                          │  │
//! │  │  • VM Isolation  • Secure Boot  • TPM Emulation  • Encryption          │  │
//! │  └─────────────────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! ### VM Management
//! - Full VM lifecycle (create, start, stop, pause, resume, destroy)
//! - VMware-style snapshots with snapshot trees
//! - Live migration between hosts
//! - Clone and template support
//!
//! ### Resource Management  
//! - CPU: Hot-plug, pinning, overcommit, NUMA-aware scheduling
//! - Memory: Balloon driver, KSM, overcommit, hot-plug
//! - Storage: QCOW2/VMDK formats, thin provisioning, live resize
//! - Network: vSwitch, VLAN, QoS, SR-IOV passthrough
//!
//! ### Enterprise Features
//! - High availability with automatic failover
//! - Distributed resource scheduling (DRS)
//! - Fault tolerance with VM checkpointing
//! - vMotion-style live migration
//!
//! ## Usage
//!
//! ```rust,ignore
//! use nexa_os_tests::mock::hypervisor::{Hypervisor, VmSpec, ResourcePool};
//!
//! // Create hypervisor with resource pools
//! let hypervisor = Hypervisor::builder()
//!     .cpu_pool(CpuPool::new(32))      // 32 physical cores
//!     .memory_pool(MemoryPool::new(128 * 1024))  // 128GB
//!     .storage_pool(StoragePool::new("/var/lib/nexa/images"))
//!     .network_pool(NetworkPool::with_bridge("virbr0"))
//!     .build();
//!
//! // Create a VM
//! let vm_spec = VmSpec::builder()
//!     .name("production-web-01")
//!     .vcpus(4)
//!     .memory_mb(8192)
//!     .disk(DiskSpec::new("web-01.qcow2", 100 * 1024 * 1024 * 1024))
//!     .network(NetworkSpec::bridged("virbr0"))
//!     .build();
//!
//! let vm_id = hypervisor.create_vm(vm_spec)?;
//! hypervisor.start_vm(vm_id)?;
//!
//! // Take a snapshot
//! hypervisor.snapshot_vm(vm_id, "before-upgrade")?;
//!
//! // Live migrate to another host
//! hypervisor.migrate_vm(vm_id, "host-02", MigrationOptions::live())?;
//! ```

pub mod api;
pub mod cluster;
pub mod core;
pub mod live_migration;
pub mod manager;
pub mod memory;
pub mod network;
pub mod resources;
pub mod scheduler;
pub mod security;
pub mod storage;

// Re-export main types
pub use self::core::{
    Hypervisor, HypervisorConfig, HypervisorError, HypervisorResult,
    VmHandle, VmId, VmSpec, VmSpecBuilder, VmStatus, VmInfo, VmInstance,
    // Disk types
    DiskSpec, DiskFormat, DiskInterface, CacheMode, IoMode,
    // Network types
    NetworkSpec, NetworkType, NicModel, NetworkQosSpec,
    // Boot and firmware types
    BootDevice, FirmwareType, CpuModel, MachineType,
    // NUMA and CPU types
    NumaSpec, NumaNode, CpuPinning,
    // Security types
    VmSecuritySpec,
};
pub use self::manager::{
    VmManager, VmManagerConfig, VmManagerBuilder, VmMetadata, VmTemplate,
    ManagerState, ManagerStats, ManagerEvent, EventSubscriber, OsType,
};
pub use self::resources::{
    ResourcePool, ResourceType, ResourceAllocation,
    CpuPool, MemoryPool, StoragePool, NetworkPool,
};
pub use self::memory::{
    MemoryManager, BalloonManager, KsmManager, NumaManager, NumaConfig,
    MemoryHotplugManager, MemoryOvercommitManager,
};
pub use self::storage::{
    StorageManager, VirtualDisk, Snapshot,
    StoragePoolType, StorageCache, StoragePoolId, VirtualDiskId, SnapshotId,
};
pub use self::network::{
    NetworkManager, VirtualSwitch, VirtualNic, MacAddress, VlanInfo,
    QosPolicy, SriovConfig, SecurityGroup, PortGroup, SwitchType,
};
pub use self::scheduler::{
    VmScheduler, SchedulerPolicy, AffinityRule, SchedulingEntry,
    LoadBalancer, MigrationRecommendation,
};
pub use self::live_migration::{
    MigrationManager, MigrationConfig, MigrationState, MigrationProgress,
    MigrationType, MigrationId,
};
pub use self::cluster::{
    ClusterManager, ClusterHost, ClusterVm, ClusterConfig,
    HaManager, FtManager, DrsManager, DrsAutomation,
};
pub use self::security::{
    SecurityManager, SecurityPolicy, VmSecurityContext, SecureBootKeys,
    TpmEmulator, MemoryEncryptionConfig, ComplianceResult, AuditEvent,
};
pub use self::api::{
    ApiServer, ApiConfig, ApiRequest, ApiResponse, ApiKey,
    HttpMethod, Endpoint, OpenApiGenerator,
};

use std::sync::Arc;

/// Hypervisor version information
pub const VERSION: &str = "1.0.0";
pub const VERSION_MAJOR: u32 = 1;
pub const VERSION_MINOR: u32 = 0;
pub const VERSION_PATCH: u32 = 0;

/// Feature flags for the hypervisor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HypervisorFeatures {
    /// Support for live migration
    pub live_migration: bool,
    /// Support for nested virtualization
    pub nested_virt: bool,
    /// Support for SR-IOV passthrough
    pub sriov: bool,
    /// Support for memory ballooning
    pub ballooning: bool,
    /// Support for KSM (Kernel Same-page Merging)
    pub ksm: bool,
    /// Support for NUMA-aware scheduling
    pub numa: bool,
    /// Support for CPU hot-plug
    pub cpu_hotplug: bool,
    /// Support for memory hot-plug
    pub memory_hotplug: bool,
    /// Support for GPU passthrough
    pub gpu_passthrough: bool,
    /// Support for vTPM
    pub vtpm: bool,
    /// Support for secure boot
    pub secure_boot: bool,
    /// Support for VM encryption
    pub encryption: bool,
}

impl Default for HypervisorFeatures {
    fn default() -> Self {
        Self {
            live_migration: true,
            nested_virt: true,
            sriov: true,
            ballooning: true,
            ksm: true,
            numa: true,
            cpu_hotplug: true,
            memory_hotplug: true,
            gpu_passthrough: true,
            vtpm: true,
            secure_boot: true,
            encryption: true,
        }
    }
}

impl HypervisorFeatures {
    /// Create with minimal features (testing/lightweight deployment)
    pub fn minimal() -> Self {
        Self {
            live_migration: false,
            nested_virt: false,
            sriov: false,
            ballooning: true,
            ksm: false,
            numa: false,
            cpu_hotplug: false,
            memory_hotplug: false,
            gpu_passthrough: false,
            vtpm: false,
            secure_boot: false,
            encryption: false,
        }
    }
    
    /// Create for production deployment
    pub fn production() -> Self {
        Self::default()
    }
}

/// Global hypervisor statistics
#[derive(Debug, Clone, Default)]
pub struct HypervisorStats {
    /// Total VMs managed
    pub total_vms: u64,
    /// Running VMs
    pub running_vms: u64,
    /// Paused VMs
    pub paused_vms: u64,
    /// Total vCPUs allocated
    pub total_vcpus: u64,
    /// Total memory allocated (bytes)
    pub total_memory: u64,
    /// Total storage used (bytes)
    pub total_storage: u64,
    /// Active migrations
    pub active_migrations: u64,
    /// Completed migrations
    pub completed_migrations: u64,
    /// Failed migrations
    pub failed_migrations: u64,
    /// Snapshots created
    pub snapshots_created: u64,
    /// Uptime in seconds
    pub uptime_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hypervisor_features() {
        let default = HypervisorFeatures::default();
        assert!(default.live_migration);
        assert!(default.ballooning);
        
        let minimal = HypervisorFeatures::minimal();
        assert!(!minimal.live_migration);
        assert!(minimal.ballooning);
    }
}
