//! NexaOS Virtual Machine (NVM) - Enterprise-Grade Hypervisor Platform
//!
//! NVM is a comprehensive virtualization platform similar to QEMU-KVM, Hyper-V,
//! or VMware ESXi. It provides complete hardware emulation and enterprise
//! virtualization features.
//!
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │                         NVM Enterprise Hypervisor Platform                       │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────────────────────────────────────────────────────────────────┐  │
//! │  │                    Management Layer (REST API / CLI)                      │  │
//! │  │  • VM Lifecycle    • Resource Scheduling   • Cluster Management          │  │
//! │  │  • Live Migration  • High Availability     • Monitoring & Alerts         │  │
//! │  │  • Multi-Tenant    • Backup/Recovery       • Compliance/Audit            │  │
//! │  └──────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌──────────────────────────────────────────────────────────────────────────┐  │
//! │  │                    Virtual Machine Monitor (VMM)                          │  │
//! │  │  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐             │  │
//! │  │  │    VM 1    │ │    VM 2    │ │    VM 3    │ │    ...     │             │  │
//! │  │  │  ┌──────┐  │ │  ┌──────┐  │ │  ┌──────┐  │ │            │             │  │
//! │  │  │  │vCPUs │  │ │  │vCPUs │  │ │  │vCPUs │  │ │            │             │  │
//! │  │  │  │vMem  │  │ │  │vMem  │  │ │  │vMem  │  │ │            │             │  │
//! │  │  │  │vDisk │  │ │  │vDisk │  │ │  │vDisk │  │ │            │             │  │
//! │  │  │  │vNIC  │  │ │  │vNIC  │  │ │  │vNIC  │  │ │            │             │  │
//! │  │  │  └──────┘  │ │  └──────┘  │ │  └──────┘  │ │            │             │  │
//! │  │  └────────────┘ └────────────┘ └────────────┘ └────────────┘             │  │
//! │  └──────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌──────────────────────────────────────────────────────────────────────────┐  │
//! │  │                    Hardware Virtualization Layer                          │  │
//! │  │  ┌─────────────────────┐  ┌─────────────────────────────────────────────┐│  │
//! │  │  │  VT-x / AMD-V Core  │  │              Device Emulation               ││  │
//! │  │  │  ┌───────────────┐  │  │  ┌────────┐ ┌────────┐ ┌────────┐          ││  │
//! │  │  │  │ VMCS/VMCB Mgr │  │  │  │  PIC   │ │  PIT   │ │  RTC   │          ││  │
//! │  │  │  │ EPT/NPT Trans │  │  │  │  APIC  │ │  IOAPIC│ │  UART  │          ││  │
//! │  │  │  │ VM Entry/Exit │  │  │  │  PCI   │ │  IDE   │ │  AHCI  │          ││  │
//! │  │  │  │ Nested Virt   │  │  │  │  E1000 │ │ Virtio │ │  NVMe  │          ││  │
//! │  │  │  └───────────────┘  │  │  └────────┘ └────────┘ └────────┘          ││  │
//! │  │  └─────────────────────┘  └─────────────────────────────────────────────┘│  │
//! │  └──────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌──────────────────────────────────────────────────────────────────────────┐  │
//! │  │                      Resource Pool Manager                                │  │
//! │  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │  │
//! │  │  │   CPU Pool   │ │ Memory Pool  │ │ Storage Pool │ │ Network Pool │    │  │
//! │  │  │  • Pinning   │ │  • Balloon   │ │  • QCOW2     │ │  • vSwitch   │    │  │
//! │  │  │  • Overcom   │ │  • KSM       │ │  • Snapshot  │ │  • VLAN      │    │  │
//! │  │  │  • NUMA      │ │  • Hot-plug  │ │  • Thin      │ │  • SR-IOV    │    │  │
//! │  │  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘    │  │
//! │  └──────────────────────────────────────────────────────────────────────────┘  │
//! │  ┌──────────────────────────────────────────────────────────────────────────┐  │
//! │  │                    Security & Compliance Layer                            │  │
//! │  │  • VM Isolation  • Secure Boot  • vTPM  • Memory Encryption  • Audit     │  │
//! │  └──────────────────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! ## Core Virtualization
//! - Hardware-assisted virtualization (VT-x/AMD-V emulation)
//! - Nested virtualization support
//! - Multi-CPU (SMP) support with NUMA awareness
//! - Memory management with ballooning, KSM, and hot-plug
//!
//! ## Enterprise Features
//! - Live migration (vMotion-style)
//! - High availability with automatic failover
//! - Distributed Resource Scheduler (DRS)
//! - Fault tolerance with VM checkpointing
//!
//! ## Storage
//! - Multiple disk formats (QCOW2, VMDK, VDI, RAW)
//! - Snapshot trees and linked clones
//! - Thin provisioning and deduplication
//! - Distributed storage backend
//!
//! ## Networking
//! - Virtual switches with VLANs
//! - Software-defined networking (SDN)
//! - SR-IOV passthrough
//! - Security groups and QoS
//!
//! ## Security
//! - VM isolation and sandboxing
//! - Secure boot and TPM emulation
//! - Memory encryption
//! - Compliance auditing
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use nvm::{Hypervisor, VmSpec, VmBuilder};
//!
//! // Create hypervisor
//! let hypervisor = Hypervisor::new()
//!     .cpu_cores(32)
//!     .memory_gb(128)
//!     .build();
//!
//! // Create a VM
//! let vm = VmBuilder::new("production-web-01")
//!     .vcpus(4)
//!     .memory_mb(8192)
//!     .disk("/var/lib/nvm/images/web-01.qcow2", 100 * 1024 * 1024 * 1024)
//!     .network_bridge("virbr0")
//!     .build();
//!
//! let vm_id = hypervisor.create_vm(vm)?;
//! hypervisor.start_vm(vm_id)?;
//!
//! // Take a snapshot
//! hypervisor.snapshot(vm_id, "before-upgrade")?;
//!
//! // Live migrate to another host
//! hypervisor.migrate(vm_id, "host-02")?;
//! ```

// Core modules
pub mod cpu;
pub mod memory;
pub mod hal;
pub mod devices;
pub mod pci;
pub mod vm;
pub mod debugger;
pub mod vmstate;  // VM state persistence
pub mod executor; // VM execution engine (QEMU integration)

// Hardware virtualization extensions
pub mod vmx;      // Intel VT-x
pub mod svm;      // AMD-V (SVM)

// Enterprise hypervisor modules
pub mod hypervisor;

// Enterprise features
pub mod storage;
pub mod network;
pub mod cluster;
pub mod migration;
pub mod scheduler;
pub mod security;
pub mod monitoring;
pub mod backup;
pub mod tenant;
pub mod api;

// Database backend (PostgreSQL or SQLite)
#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub mod db;

// New Enterprise Platform Features (v2.0)
pub mod webgui;     // Web-based management UI
pub mod cli;        // Command-line interface tools
pub mod events;     // Event system and audit logging
pub mod templates;  // VM template management (OVA/OVF)
pub mod auth;       // Enhanced authentication (LDAP/OAuth2/SAML)
pub mod licensing;  // License management and feature gating
pub mod ha;         // High availability (Raft consensus, fencing, failover)
pub mod firmware;   // BIOS/UEFI firmware emulation

// Re-export from hypervisor module (which has its own re-exports)
pub use hypervisor::{
    Hypervisor, HypervisorConfig, HypervisorError, HypervisorResult,
    HypervisorFeatures, HypervisorStats,
    VmId, VmStatus, VmSpec, VmInfo, VmInstance, VmHandle,
    VmManager, VmManagerConfig, VmManagerBuilder, VmTemplate, VmMetadata,
    ResourcePool, ResourceType, ResourceAllocation,
    CpuPool as HvCpuPool, MemoryPool as HvMemoryPool, StoragePool as HvStoragePool, NetworkPool,
};

// Re-export new enterprise features
pub use webgui::WebGuiServer;
pub use cli::{CliConfig, OutputFormat};
pub use events::{Event, EventBus, AuditLogger};
pub use templates::{Template, TemplateLibrary, OvaImporter, OvaExporter};
pub use auth::{AuthManager, AuthBackend, AuthenticatedUser};
pub use licensing::{License, LicenseValidator, FeatureGate, Edition};
pub use ha::{RaftNode, FencingManager, FailoverManager, HaConfig};

/// NVM version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const VERSION_MAJOR: u32 = 2;
pub const VERSION_MINOR: u32 = 0;
pub const VERSION_PATCH: u32 = 0;

/// Platform name
pub const PLATFORM_NAME: &str = "NexaOS Virtual Machine";
pub const PLATFORM_VENDOR: &str = "NexaOS Team";
pub const PLATFORM_EDITION: &str = "Enterprise";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
