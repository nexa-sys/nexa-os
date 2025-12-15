//! Driver Container for User-space Drivers
//!
//! Implements HongMeng's driver container concept - a lightweight container
//! that hosts user-space drivers with proper isolation and resource management.
//!
//! # Design Philosophy
//!
//! Driver containers provide:
//! - Runtime environment for driver execution
//! - Mediated hardware access
//! - Fault isolation from kernel
//! - Resource accounting and limits
//!
//! # Container Lifecycle
//!
//! ```text
//! Created → Initializing → Running → Stopping → Stopped
//!                ↓                       ↓
//!              Failed ←───────────── Crashed
//! ```
//!
//! # Resource Limits
//!
//! Each container has configurable limits:
//! - CPU time quota
//! - Memory allocation
//! - IPC message rate
//! - Hardware resource access

use super::isolation::{IsolationClass, IC2Context};
use spin::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

/// Maximum drivers per container
pub const MAX_DRIVERS_PER_CONTAINER: usize = 8;

/// Container ID type
pub type DriverContainerId = u32;

/// Driver container
#[derive(Debug)]
pub struct DriverContainer {
    /// Container ID
    pub id: DriverContainerId,
    /// Container name
    pub name: [u8; 32],
    /// Isolation class
    pub isolation: IsolationClass,
    /// Container state
    pub state: ContainerState,
    /// Hosted driver IDs
    pub drivers: [Option<super::DriverId>; MAX_DRIVERS_PER_CONTAINER],
    /// Number of hosted drivers
    pub driver_count: usize,
    /// Resource limits
    pub limits: ContainerLimits,
    /// Resource usage
    pub usage: ContainerUsage,
    /// IC2 context (for userspace containers)
    pub ic2_context: Option<IC2Context>,
    /// IC1 domain ID (for kernel-space containers)
    pub ic1_domain: Option<u8>,
    /// Entry point for container init
    pub init_entry: u64,
    /// PID if running as process
    pub pid: Option<u32>,
}

/// Container state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ContainerState {
    /// Container created but not initialized
    Created = 0,
    /// Container is initializing
    Initializing = 1,
    /// Container is running
    Running = 2,
    /// Container is stopping
    Stopping = 3,
    /// Container is stopped
    Stopped = 4,
    /// Container has crashed
    Crashed = 5,
    /// Container initialization failed
    Failed = 6,
}

/// Container resource limits
#[derive(Debug, Clone, Copy)]
pub struct ContainerLimits {
    /// CPU time limit (microseconds per second)
    pub cpu_time_us: u64,
    /// Memory limit (bytes)
    pub memory_bytes: u64,
    /// Maximum IPC messages per second
    pub ipc_rate: u32,
    /// Maximum open file descriptors
    pub max_fds: u32,
    /// Maximum threads
    pub max_threads: u32,
    /// Can access hardware directly
    pub hw_access: bool,
}

impl Default for ContainerLimits {
    fn default() -> Self {
        Self {
            cpu_time_us: 100_000,     // 10% CPU
            memory_bytes: 16 * 1024 * 1024, // 16MB
            ipc_rate: 10_000,          // 10k msgs/sec
            max_fds: 256,
            max_threads: 16,
            hw_access: false,
        }
    }
}

/// Container resource usage
#[derive(Debug, Clone, Copy, Default)]
pub struct ContainerUsage {
    /// CPU time used (microseconds)
    pub cpu_time_us: u64,
    /// Memory allocated (bytes)
    pub memory_bytes: u64,
    /// IPC messages sent
    pub ipc_sent: u64,
    /// IPC messages received
    pub ipc_recv: u64,
    /// Page faults
    pub page_faults: u64,
    /// Hardware interrupts handled
    pub interrupts: u64,
}

/// Hardware resource grant
#[derive(Debug, Clone, Copy)]
pub struct HardwareGrant {
    /// Resource type
    pub resource_type: HardwareResourceType,
    /// Physical address (for MMIO)
    pub phys_addr: u64,
    /// Size
    pub size: u64,
    /// IRQ number (if applicable)
    pub irq: Option<u8>,
    /// Access flags
    pub flags: u32,
}

/// Hardware resource types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HardwareResourceType {
    /// Memory-mapped I/O region
    Mmio = 0,
    /// I/O port range
    IoPort = 1,
    /// DMA buffer
    DmaBuffer = 2,
    /// Interrupt line
    Interrupt = 3,
}

/// Hardware grant flags
pub mod hw_grant_flags {
    pub const GRANT_READ: u32 = 1 << 0;
    pub const GRANT_WRITE: u32 = 1 << 1;
    pub const GRANT_CACHEABLE: u32 = 1 << 2;
    pub const GRANT_PREFETCH: u32 = 1 << 3;
}

// Global state
static CONTAINERS: Mutex<[Option<DriverContainer>; super::MAX_CONTAINERS]> = 
    Mutex::new([const { None }; super::MAX_CONTAINERS]);
static NEXT_CONTAINER_ID: AtomicU32 = AtomicU32::new(1);

/// Initialize container subsystem
pub fn init() {
    crate::kinfo!("UDRV/Container: Initializing driver container subsystem");
    crate::kinfo!("UDRV/Container: {} max containers, {} drivers per container",
                  super::MAX_CONTAINERS, MAX_DRIVERS_PER_CONTAINER);
}

/// Create a new driver container
pub fn create(isolation: IsolationClass) -> Result<DriverContainerId, super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    
    // Find empty slot
    for slot in containers.iter_mut() {
        if slot.is_none() {
            let id = NEXT_CONTAINER_ID.fetch_add(1, Ordering::SeqCst);
            
            let container = DriverContainer {
                id,
                name: [0; 32],
                isolation,
                state: ContainerState::Created,
                drivers: [None; MAX_DRIVERS_PER_CONTAINER],
                driver_count: 0,
                limits: ContainerLimits::default(),
                usage: ContainerUsage::default(),
                ic2_context: None,
                ic1_domain: None,
                init_entry: 0,
                pid: None,
            };
            
            *slot = Some(container);
            
            crate::kinfo!("UDRV/Container: Created container {} with IC{:?}",
                          id, isolation);
            
            return Ok(id);
        }
    }
    
    Err(super::ContainerError::TableFull)
}

/// Configure container limits
pub fn configure_limits(id: DriverContainerId, limits: ContainerLimits) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    let container = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == id))
        .ok_or(super::ContainerError::NotFound)?;
    
    if container.state != ContainerState::Created {
        return Err(super::ContainerError::InvalidState);
    }
    
    container.limits = limits;
    Ok(())
}

/// Set container name
pub fn set_name(id: DriverContainerId, name: &str) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    let container = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == id))
        .ok_or(super::ContainerError::NotFound)?;
    
    let name_bytes = name.as_bytes();
    let len = core::cmp::min(name_bytes.len(), 31);
    container.name[..len].copy_from_slice(&name_bytes[..len]);
    container.name[len] = 0;
    
    Ok(())
}

/// Spawn a driver in a container
pub fn spawn(container_id: DriverContainerId, driver_id: super::DriverId) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    let container = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == container_id))
        .ok_or(super::ContainerError::NotFound)?;
    
    // Check driver count limit
    if container.driver_count >= MAX_DRIVERS_PER_CONTAINER {
        return Err(super::ContainerError::TableFull);
    }
    
    // Add driver to container
    for slot in container.drivers.iter_mut() {
        if slot.is_none() {
            *slot = Some(driver_id);
            container.driver_count += 1;
            break;
        }
    }
    
    crate::kinfo!("UDRV/Container: Spawned driver {} in container {}",
                  driver_id, container_id);
    
    Ok(())
}

/// Initialize container (set up isolation and runtime)
pub fn initialize(id: DriverContainerId) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    let container = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == id))
        .ok_or(super::ContainerError::NotFound)?;
    
    if container.state != ContainerState::Created {
        return Err(super::ContainerError::InvalidState);
    }
    
    container.state = ContainerState::Initializing;
    
    // Set up isolation based on class
    match container.isolation {
        IsolationClass::IC0 => {
            // IC0 - no isolation setup needed
            crate::kinfo!("UDRV/Container: Container {} using IC0 (no isolation)", id);
        }
        IsolationClass::IC1 => {
            // IC1 - allocate domain
            let domain = super::isolation::allocate_ic1_domain(id)
                .map_err(super::ContainerError::IsolationError)?;
            container.ic1_domain = Some(domain);
            crate::kinfo!("UDRV/Container: Container {} using IC1 domain {}", id, domain);
        }
        IsolationClass::IC2 => {
            // IC2 - create process context
            // This would create a new address space
            let cr3 = crate::paging::kernel_pml4_phys(); // Placeholder
            let ctx = super::isolation::create_ic2_context(cr3);
            container.ic2_context = Some(ctx);
            crate::kinfo!("UDRV/Container: Container {} using IC2 process isolation", id);
        }
    }
    
    container.state = ContainerState::Running;
    
    Ok(())
}

/// Start container execution
pub fn start(id: DriverContainerId) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    let container = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == id))
        .ok_or(super::ContainerError::NotFound)?;
    
    if container.state != ContainerState::Running {
        return Err(super::ContainerError::InvalidState);
    }
    
    if container.init_entry == 0 {
        // No init entry set - drivers will be started individually
        return Ok(());
    }
    
    // Call container init
    match container.isolation {
        IsolationClass::IC0 | IsolationClass::IC1 => {
            // Direct or gate call
            let init_fn: fn() = unsafe {
                core::mem::transmute(container.init_entry)
            };
            init_fn();
        }
        IsolationClass::IC2 => {
            // Create process and start
            // This would spawn a userspace process
        }
    }
    
    Ok(())
}

/// Stop a container
pub fn stop(id: DriverContainerId) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    let container = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == id))
        .ok_or(super::ContainerError::NotFound)?;
    
    if container.state != ContainerState::Running {
        return Err(super::ContainerError::InvalidState);
    }
    
    container.state = ContainerState::Stopping;
    
    // Stop all drivers
    for driver_id in container.drivers.iter().flatten() {
        // Would signal driver to stop
        crate::kinfo!("UDRV/Container: Stopping driver {} in container {}", driver_id, id);
    }
    
    container.state = ContainerState::Stopped;
    
    // Clean up isolation resources
    if let Some(domain) = container.ic1_domain {
        let _ = super::isolation::deallocate_ic1_domain(domain);
    }
    
    crate::kinfo!("UDRV/Container: Container {} stopped", id);
    
    Ok(())
}

/// Destroy a container
pub fn destroy(id: DriverContainerId) -> Result<(), super::ContainerError> {
    let mut containers = CONTAINERS.lock();
    
    for slot in containers.iter_mut() {
        if let Some(container) = slot {
            if container.id == id {
                if container.state == ContainerState::Running {
                    return Err(super::ContainerError::InvalidState);
                }
                *slot = None;
                crate::kinfo!("UDRV/Container: Container {} destroyed", id);
                return Ok(());
            }
        }
    }
    
    Err(super::ContainerError::NotFound)
}

/// Get container info
pub fn get_info(id: DriverContainerId) -> Option<ContainerInfo> {
    let containers = CONTAINERS.lock();
    let container = containers.iter()
        .find_map(|slot| slot.as_ref().filter(|c| c.id == id))?;
    
    Some(ContainerInfo {
        id: container.id,
        isolation: container.isolation,
        state: container.state,
        driver_count: container.driver_count,
        limits: container.limits,
        usage: container.usage,
    })
}

/// Container info (read-only view)
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: DriverContainerId,
    pub isolation: IsolationClass,
    pub state: ContainerState,
    pub driver_count: usize,
    pub limits: ContainerLimits,
    pub usage: ContainerUsage,
}

/// List all containers
pub fn list_containers() -> Vec<DriverContainerId> {
    let containers = CONTAINERS.lock();
    containers.iter()
        .filter_map(|slot| slot.as_ref().map(|c| c.id))
        .collect()
}

/// Grant hardware resource to container
pub fn grant_hardware(id: DriverContainerId, grant: HardwareGrant) -> Result<(), super::ContainerError> {
    let containers = CONTAINERS.lock();
    let container = containers.iter()
        .find_map(|slot| slot.as_ref().filter(|c| c.id == id))
        .ok_or(super::ContainerError::NotFound)?;
    
    if !container.limits.hw_access {
        return Err(super::ContainerError::InvalidState);
    }
    
    // Map hardware resource into container's address space
    match grant.resource_type {
        HardwareResourceType::Mmio => {
            // Map MMIO region
            crate::kinfo!("UDRV/Container: Granted MMIO {:#x}+{:#x} to container {}",
                          grant.phys_addr, grant.size, id);
        }
        HardwareResourceType::IoPort => {
            // Set I/O port permissions
            crate::kinfo!("UDRV/Container: Granted I/O ports {:#x}+{} to container {}",
                          grant.phys_addr, grant.size, id);
        }
        HardwareResourceType::DmaBuffer => {
            // Allocate DMA buffer
            crate::kinfo!("UDRV/Container: Granted DMA buffer {} bytes to container {}",
                          grant.size, id);
        }
        HardwareResourceType::Interrupt => {
            if let Some(irq) = grant.irq {
                crate::kinfo!("UDRV/Container: Granted IRQ {} to container {}", irq, id);
            }
        }
    }
    
    Ok(())
}

/// Handle container crash
pub fn handle_crash(id: DriverContainerId, reason: &str) {
    let mut containers = CONTAINERS.lock();
    if let Some(container) = containers.iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == id))
    {
        container.state = ContainerState::Crashed;
        crate::kerror!("UDRV/Container: Container {} crashed: {}", id, reason);
        
        // Could implement auto-restart here
    }
}
