//! User-space Driver Framework (UDRV) for NexaOS
//!
//! This module implements a microkernel-style user-space driver framework
//! inspired by HongMeng's production microkernel design (OSDI'24).
//!
//! # Design Philosophy
//!
//! Following HongMeng's principles, we prioritize:
//! - **Minimality**: Keep only essential functionality in kernel
//! - **Performance**: Structural support for flexible isolation
//! - **Compatibility**: Reuse existing driver ecosystems
//!
//! # Key Components
//!
//! ## Isolation Classes (IC)
//! - **IC0**: Core TCB - No isolation, function calls (kernel-internal only)
//! - **IC1**: Mechanism-enforced isolation in kernel space (PKS/watchpoint)
//! - **IC2**: Address space isolation in userspace (full process isolation)
//!
//! ## Driver Container
//! A lightweight container that hosts user-space drivers with:
//! - Runtime environment for driver execution
//! - Mediated access to hardware resources
//! - Fault isolation from the kernel
//!
//! ## Twin Drivers
//! Control/data plane separation for performance:
//! - **Control Plane**: Configuration, initialization via IPC
//! - **Data Plane**: Direct DMA/MMIO access for fast I/O
//!
//! ## Address Tokens
//! Efficient kernel object access without capability overhead:
//! - Direct memory-mapped access to kernel objects
//! - Read-only or read-write grants
//! - Lock-free ring buffers for async messaging
//!
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    User Applications                         │
//! └─────────────────────────┬───────────────────────────────────┘
//!                           │ syscalls
//! ┌─────────────────────────┴───────────────────────────────────┐
//! │                  Driver Container (IC2)                      │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
//! │  │ Net Driver  │  │ Blk Driver  │  │ GPU Driver  │          │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘          │
//! │         │ Control Plane  │                │                  │
//! └─────────┼────────────────┼────────────────┼──────────────────┘
//!           │ IPC            │                │
//! ┌─────────┴────────────────┴────────────────┴──────────────────┐
//! │                    Core Kernel (IC0)                         │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
//! │  │ Twin Stub   │  │ Twin Stub   │  │ Twin Stub   │          │
//! │  │ (Net)       │  │ (Block)     │  │ (GPU)       │          │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘          │
//! │         │ Data Plane (Direct DMA/MMIO)                       │
//! └─────────┴────────────────────────────────────────────────────┘
//!           │
//! ┌─────────┴────────────────────────────────────────────────────┐
//! │                      Hardware                                │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! # References
//!
//! - HongMeng Microkernel (OSDI'24): "Microkernel Goes General"
//! - seL4 Microkernel: Capability-based access control
//! - L4 Family: Fast IPC mechanisms

pub mod address_token;
pub mod container;
pub mod isolation;
pub mod ipc_rpc;
pub mod registry;
pub mod shared_mem;
pub mod twin_driver;

// Re-export core types
pub use address_token::{AddressToken, TokenAccess, TokenError};
pub use container::{DriverContainer, DriverContainerId, ContainerState};
pub use isolation::{IsolationClass, IsolationError};
pub use ipc_rpc::{RpcChannel, RpcMessage, RpcError, RpcResult};
pub use registry::{DriverInfo, DriverRegistry, DriverId, DriverClass};
pub use shared_mem::{SharedRegion, SharedRegionId, SharedMemError};
pub use twin_driver::{TwinDriver, ControlPlane, DataPlane, TwinDriverId};

use spin::Mutex;
use alloc::vec::Vec;

/// Maximum number of user-space drivers
pub const MAX_UDRV_DRIVERS: usize = 64;

/// Maximum number of driver containers
pub const MAX_CONTAINERS: usize = 16;

/// Global driver registry
static DRIVER_REGISTRY: Mutex<Option<DriverRegistry>> = Mutex::new(None);

/// Initialize the user-space driver framework
pub fn init() {
    crate::kinfo!("UDRV: Initializing user-space driver framework");
    
    // Initialize subsystems
    isolation::init();
    registry::init();
    container::init();
    ipc_rpc::init();
    shared_mem::init();
    address_token::init();
    
    // Create global registry
    let mut registry = DRIVER_REGISTRY.lock();
    *registry = Some(DriverRegistry::new());
    
    crate::kinfo!("UDRV: Framework initialized with {} max drivers, {} max containers",
                  MAX_UDRV_DRIVERS, MAX_CONTAINERS);
}

/// Check if framework is initialized
pub fn is_initialized() -> bool {
    DRIVER_REGISTRY.lock().is_some()
}

/// Register a new user-space driver
pub fn register_driver(info: DriverInfo) -> Result<DriverId, RegistryError> {
    let mut registry = DRIVER_REGISTRY.lock();
    registry
        .as_mut()
        .ok_or(RegistryError::NotInitialized)?
        .register(info)
}

/// Unregister a driver
pub fn unregister_driver(id: DriverId) -> Result<(), RegistryError> {
    let mut registry = DRIVER_REGISTRY.lock();
    registry
        .as_mut()
        .ok_or(RegistryError::NotInitialized)?
        .unregister(id)
}

/// Get driver information
pub fn get_driver_info(id: DriverId) -> Option<DriverInfo> {
    let registry = DRIVER_REGISTRY.lock();
    registry.as_ref()?.get_info(id)
}

/// List all registered drivers
pub fn list_drivers() -> Vec<DriverId> {
    let registry = DRIVER_REGISTRY.lock();
    registry
        .as_ref()
        .map(|r| r.list_drivers())
        .unwrap_or_default()
}

/// Create a driver container
pub fn create_container(isolation: IsolationClass) -> Result<DriverContainerId, ContainerError> {
    container::create(isolation)
}

/// Spawn a driver in a container
pub fn spawn_driver(
    container_id: DriverContainerId,
    driver_id: DriverId,
) -> Result<(), ContainerError> {
    container::spawn(container_id, driver_id)
}

/// Registry error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    NotInitialized,
    TableFull,
    NotFound,
    AlreadyExists,
    InvalidState,
}

/// Container error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerError {
    NotInitialized,
    TableFull,
    NotFound,
    InvalidState,
    IsolationError(IsolationError),
    DriverNotFound,
}

impl From<IsolationError> for ContainerError {
    fn from(e: IsolationError) -> Self {
        ContainerError::IsolationError(e)
    }
}
