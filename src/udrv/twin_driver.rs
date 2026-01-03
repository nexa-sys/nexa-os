//! Twin Driver Architecture
//!
//! Implements HongMeng's twin driver design for control/data plane separation.
//!
//! # Design Philosophy
//!
//! Traditional driver isolation puts all driver code in userspace, causing
//! performance overhead on the data plane (every I/O requires IPC).
//!
//! Twin drivers separate:
//! - **Control Plane**: Configuration, initialization, error handling (userspace)
//! - **Data Plane**: Fast I/O operations (kernel stub with direct DMA/MMIO)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │              User Space (IC2)                    │
//! │  ┌───────────────────────────────────────────┐  │
//! │  │         Control Plane Driver               │  │
//! │  │  - Device initialization                   │  │
//! │  │  - Configuration changes                   │  │
//! │  │  - Error handling                          │  │
//! │  │  - Power management                        │  │
//! │  └────────────────────┬──────────────────────┘  │
//! └───────────────────────┼──────────────────────────┘
//!                         │ IPC (infrequent)
//! ┌───────────────────────┼──────────────────────────┐
//! │              Kernel (IC0/IC1)                     │
//! │  ┌────────────────────┴──────────────────────┐  │
//! │  │          Data Plane Stub                   │  │
//! │  │  - DMA descriptor management               │  │
//! │  │  - Direct MMIO access                      │  │
//! │  │  - Interrupt handling                      │  │
//! │  │  - Ring buffer operations                  │  │
//! │  └────────────────────┬──────────────────────┘  │
//! └───────────────────────┼──────────────────────────┘
//!                         │ DMA/MMIO
//!                    ┌────┴────┐
//!                    │ Hardware │
//!                    └─────────┘
//! ```
//!
//! # Performance Gains
//!
//! - Data plane operations avoid IPC overhead
//! - Control plane changes are infrequent
//! - Interrupts handled in kernel for low latency
//! - DMA completes without userspace involvement

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

/// Twin driver ID type
pub type TwinDriverId = u32;

/// Maximum twin drivers
pub const MAX_TWIN_DRIVERS: usize = 32;

/// Twin driver instance
#[derive(Debug)]
pub struct TwinDriver {
    /// Driver ID
    pub id: TwinDriverId,
    /// Driver name
    pub name: [u8; 32],
    /// Driver class
    pub class: TwinDriverClass,
    /// Control plane (userspace)
    pub control: ControlPlane,
    /// Data plane (kernel stub)
    pub data: DataPlane,
    /// Driver state
    pub state: TwinDriverState,
    /// Shared memory region for plane communication
    pub shared_region: Option<super::SharedRegionId>,
}

/// Twin driver classes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TwinDriverClass {
    /// Network interface
    Network = 0,
    /// Block device
    Block = 1,
    /// GPU/Display
    Graphics = 2,
    /// USB host controller
    UsbHost = 3,
    /// Audio
    Audio = 4,
    /// Input device
    Input = 5,
    /// Custom
    Custom = 255,
}

/// Twin driver state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TwinDriverState {
    /// Not initialized
    Uninitialized = 0,
    /// Control plane loaded
    ControlLoaded = 1,
    /// Data plane stub loaded
    DataLoaded = 2,
    /// Both planes linked
    Linked = 3,
    /// Running
    Running = 4,
    /// Stopped
    Stopped = 5,
    /// Error
    Error = 6,
}

/// Control plane (userspace driver)
#[derive(Debug, Clone)]
pub struct ControlPlane {
    /// Container hosting the driver
    pub container_id: super::DriverContainerId,
    /// Driver process PID
    pub pid: Option<u32>,
    /// Entry point for control operations
    pub entry_point: u64,
    /// IPC channel for control messages
    pub ipc_channel: Option<u32>,
    /// Supported control operations
    pub operations: ControlOperations,
}

impl Default for ControlPlane {
    fn default() -> Self {
        Self {
            container_id: 0,
            pid: None,
            entry_point: 0,
            ipc_channel: None,
            operations: ControlOperations::default(),
        }
    }
}

/// Control plane operations
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlOperations {
    /// Initialize device
    pub init: bool,
    /// Configure device
    pub configure: bool,
    /// Reset device
    pub reset: bool,
    /// Power management
    pub power: bool,
    /// Get statistics
    pub stats: bool,
    /// Handle errors
    pub error: bool,
}

/// Data plane (kernel stub)
#[derive(Debug, Clone)]
pub struct DataPlane {
    /// Stub isolation class (IC0 or IC1)
    pub isolation: super::IsolationClass,
    /// MMIO regions
    pub mmio_regions: Vec<MmioMapping>,
    /// DMA descriptors
    pub dma_descriptors: Vec<DmaDescriptor>,
    /// IRQ handlers
    pub irq_handlers: Vec<IrqHandler>,
    /// Ring buffers for packet/block I/O
    pub ring_buffers: Vec<RingBuffer>,
    /// Stub entry points
    pub stub_ops: DataPlaneOps,
}

impl Default for DataPlane {
    fn default() -> Self {
        Self {
            isolation: super::IsolationClass::IC0,
            mmio_regions: Vec::new(),
            dma_descriptors: Vec::new(),
            irq_handlers: Vec::new(),
            ring_buffers: Vec::new(),
            stub_ops: DataPlaneOps::default(),
        }
    }
}

/// Data plane operations (kernel stub function pointers)
#[derive(Debug, Clone, Copy, Default)]
pub struct DataPlaneOps {
    /// Send packet/write block (data plane fast path)
    pub xmit: Option<fn(*const u8, usize) -> i32>,
    /// Receive packet/read block (data plane fast path)
    pub recv: Option<fn(*mut u8, usize) -> i32>,
    /// Handle interrupt
    pub irq_handler: Option<fn(u8)>,
    /// Poll for completion
    pub poll: Option<fn() -> i32>,
}

/// MMIO region mapping
#[derive(Debug, Clone, Copy)]
pub struct MmioMapping {
    /// Physical address
    pub phys_addr: u64,
    /// Virtual address (in kernel)
    pub virt_addr: u64,
    /// Size
    pub size: u64,
    /// Cacheability
    pub cacheable: bool,
}

/// DMA descriptor
#[derive(Debug, Clone, Copy)]
pub struct DmaDescriptor {
    /// Physical address of buffer
    pub phys_addr: u64,
    /// Virtual address (in kernel)
    pub virt_addr: u64,
    /// Size
    pub size: u64,
    /// Direction
    pub direction: DmaDirection,
}

/// DMA direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DmaDirection {
    /// Device to memory
    ToDevice = 0,
    /// Memory to device
    FromDevice = 1,
    /// Bidirectional
    Bidirectional = 2,
}

/// IRQ handler descriptor
#[derive(Debug, Clone, Copy)]
pub struct IrqHandler {
    /// IRQ number
    pub irq: u8,
    /// Handler function
    pub handler: Option<fn(u8)>,
    /// Shared IRQ
    pub shared: bool,
}

/// Ring buffer for packet/block I/O
#[derive(Debug, Clone)]
pub struct RingBuffer {
    /// Buffer type
    pub buf_type: RingBufferType,
    /// Physical address
    pub phys_addr: u64,
    /// Virtual address
    pub virt_addr: u64,
    /// Number of entries
    pub entries: u32,
    /// Entry size
    pub entry_size: u32,
    /// Head index (consumer)
    pub head: u32,
    /// Tail index (producer)
    pub tail: u32,
}

/// Ring buffer types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RingBufferType {
    /// TX descriptor ring
    TxDesc = 0,
    /// RX descriptor ring
    RxDesc = 1,
    /// TX data ring
    TxData = 2,
    /// RX data ring
    RxData = 3,
    /// Command ring
    Command = 4,
    /// Event ring
    Event = 5,
}

// Global state
static TWIN_DRIVERS: Mutex<[Option<TwinDriver>; MAX_TWIN_DRIVERS]> =
    Mutex::new([const { None }; MAX_TWIN_DRIVERS]);
static NEXT_TWIN_ID: AtomicU32 = AtomicU32::new(1);

/// Initialize twin driver subsystem
pub fn init() {
    crate::kinfo!("UDRV/Twin: Initializing twin driver subsystem");
    crate::kinfo!("UDRV/Twin: {} max twin drivers", MAX_TWIN_DRIVERS);
}

/// Create a new twin driver
pub fn create(name: &str, class: TwinDriverClass) -> Result<TwinDriverId, TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();

    // Find empty slot
    for slot in drivers.iter_mut() {
        if slot.is_none() {
            let id = NEXT_TWIN_ID.fetch_add(1, Ordering::SeqCst);

            let mut name_buf = [0u8; 32];
            let name_bytes = name.as_bytes();
            let len = core::cmp::min(name_bytes.len(), 31);
            name_buf[..len].copy_from_slice(&name_bytes[..len]);

            *slot = Some(TwinDriver {
                id,
                name: name_buf,
                class,
                control: ControlPlane::default(),
                data: DataPlane::default(),
                state: TwinDriverState::Uninitialized,
                shared_region: None,
            });

            crate::kinfo!("UDRV/Twin: Created twin driver {} ({})", id, name);

            return Ok(id);
        }
    }

    Err(TwinDriverError::TableFull)
}

/// Load control plane for twin driver
pub fn load_control_plane(
    id: TwinDriverId,
    container_id: super::DriverContainerId,
    entry_point: u64,
) -> Result<(), TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    if driver.state != TwinDriverState::Uninitialized {
        return Err(TwinDriverError::InvalidState);
    }

    driver.control.container_id = container_id;
    driver.control.entry_point = entry_point;
    driver.state = TwinDriverState::ControlLoaded;

    crate::kinfo!(
        "UDRV/Twin: Loaded control plane for driver {} in container {}",
        id,
        container_id
    );

    Ok(())
}

/// Load data plane stub for twin driver
pub fn load_data_plane(
    id: TwinDriverId,
    isolation: super::IsolationClass,
    ops: DataPlaneOps,
) -> Result<(), TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    // Data plane must be IC0 or IC1 for performance
    if isolation == super::IsolationClass::IC2 {
        return Err(TwinDriverError::InvalidIsolation);
    }

    driver.data.isolation = isolation;
    driver.data.stub_ops = ops;

    if driver.state == TwinDriverState::ControlLoaded {
        driver.state = TwinDriverState::Linked;
    } else {
        driver.state = TwinDriverState::DataLoaded;
    }

    crate::kinfo!(
        "UDRV/Twin: Loaded data plane stub for driver {} (IC{:?})",
        id,
        isolation
    );

    Ok(())
}

/// Add MMIO region to data plane
pub fn add_mmio_region(
    id: TwinDriverId,
    phys_addr: u64,
    size: u64,
    cacheable: bool,
) -> Result<u64, TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    // Map physical to virtual (would use actual mapping in real implementation)
    let virt_addr = phys_addr + 0xFFFF_8000_0000_0000; // Direct map offset

    driver.data.mmio_regions.push(MmioMapping {
        phys_addr,
        virt_addr,
        size,
        cacheable,
    });

    crate::kinfo!(
        "UDRV/Twin: Added MMIO region {:#x}+{:#x} to driver {}",
        phys_addr,
        size,
        id
    );

    Ok(virt_addr)
}

/// Add DMA descriptor to data plane
pub fn add_dma_buffer(
    id: TwinDriverId,
    size: u64,
    direction: DmaDirection,
) -> Result<DmaDescriptor, TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    // Allocate DMA buffer (would use DMA allocator in real implementation)
    let phys_addr = 0x1000_0000 + (driver.data.dma_descriptors.len() as u64 * size);
    let virt_addr = phys_addr + 0xFFFF_8000_0000_0000;

    let desc = DmaDescriptor {
        phys_addr,
        virt_addr,
        size,
        direction,
    };

    driver.data.dma_descriptors.push(desc);

    crate::kinfo!(
        "UDRV/Twin: Added DMA buffer {:#x} ({} bytes) to driver {}",
        phys_addr,
        size,
        id
    );

    Ok(desc)
}

/// Register IRQ handler
pub fn register_irq(id: TwinDriverId, irq: u8, shared: bool) -> Result<(), TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    // Get handler from stub ops
    let handler = driver.data.stub_ops.irq_handler;

    driver.data.irq_handlers.push(IrqHandler {
        irq,
        handler,
        shared,
    });

    crate::kinfo!(
        "UDRV/Twin: Registered IRQ {} for driver {} (shared={})",
        irq,
        id,
        shared
    );

    Ok(())
}

/// Start twin driver
pub fn start(id: TwinDriverId) -> Result<(), TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    if driver.state != TwinDriverState::Linked {
        return Err(TwinDriverError::InvalidState);
    }

    // Initialize via control plane
    // Would send IPC to control plane to initialize device

    driver.state = TwinDriverState::Running;

    crate::kinfo!("UDRV/Twin: Started twin driver {}", id);

    Ok(())
}

/// Stop twin driver
pub fn stop(id: TwinDriverId) -> Result<(), TwinDriverError> {
    let mut drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    if driver.state != TwinDriverState::Running {
        return Err(TwinDriverError::InvalidState);
    }

    driver.state = TwinDriverState::Stopped;

    crate::kinfo!("UDRV/Twin: Stopped twin driver {}", id);

    Ok(())
}

/// Transmit data via data plane (fast path)
#[inline]
pub fn xmit(id: TwinDriverId, data: &[u8]) -> Result<i32, TwinDriverError> {
    let drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter()
        .find_map(|slot| slot.as_ref().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    if driver.state != TwinDriverState::Running {
        return Err(TwinDriverError::InvalidState);
    }

    let xmit_fn = driver
        .data
        .stub_ops
        .xmit
        .ok_or(TwinDriverError::NotSupported)?;

    Ok(xmit_fn(data.as_ptr(), data.len()))
}

/// Receive data via data plane (fast path)
#[inline]
pub fn recv(id: TwinDriverId, buffer: &mut [u8]) -> Result<i32, TwinDriverError> {
    let drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter()
        .find_map(|slot| slot.as_ref().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    if driver.state != TwinDriverState::Running {
        return Err(TwinDriverError::InvalidState);
    }

    let recv_fn = driver
        .data
        .stub_ops
        .recv
        .ok_or(TwinDriverError::NotSupported)?;

    Ok(recv_fn(buffer.as_mut_ptr(), buffer.len()))
}

/// Send control message to control plane
pub fn control_message(
    id: TwinDriverId,
    msg: &ControlMessage,
) -> Result<ControlResponse, TwinDriverError> {
    let drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter()
        .find_map(|slot| slot.as_ref().filter(|d| d.id == id))
        .ok_or(TwinDriverError::NotFound)?;

    let _channel = driver
        .control
        .ipc_channel
        .ok_or(TwinDriverError::NotInitialized)?;

    // Would send IPC message to control plane
    // For now, return placeholder

    Ok(ControlResponse {
        status: 0,
        data: [0; 64],
    })
}

/// Control message for control plane
#[derive(Debug, Clone)]
pub struct ControlMessage {
    pub op: ControlOp,
    pub data: [u8; 64],
}

/// Control operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlOp {
    Init = 0,
    Configure = 1,
    Reset = 2,
    PowerUp = 3,
    PowerDown = 4,
    GetStats = 5,
    SetMac = 6, // Network specific
    SetMtu = 7,
    Custom = 255,
}

/// Control response
#[derive(Debug, Clone)]
pub struct ControlResponse {
    pub status: i32,
    pub data: [u8; 64],
}

/// Twin driver error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwinDriverError {
    TableFull,
    NotFound,
    InvalidState,
    InvalidIsolation,
    NotSupported,
    NotInitialized,
    IpcError,
}

/// Get twin driver info
pub fn get_info(id: TwinDriverId) -> Option<TwinDriverInfo> {
    let drivers = TWIN_DRIVERS.lock();
    let driver = drivers
        .iter()
        .find_map(|slot| slot.as_ref().filter(|d| d.id == id))?;

    Some(TwinDriverInfo {
        id: driver.id,
        class: driver.class,
        state: driver.state,
        control_container: driver.control.container_id,
        data_isolation: driver.data.isolation,
        mmio_count: driver.data.mmio_regions.len(),
        dma_count: driver.data.dma_descriptors.len(),
        irq_count: driver.data.irq_handlers.len(),
    })
}

/// Twin driver info (read-only view)
#[derive(Debug, Clone)]
pub struct TwinDriverInfo {
    pub id: TwinDriverId,
    pub class: TwinDriverClass,
    pub state: TwinDriverState,
    pub control_container: super::DriverContainerId,
    pub data_isolation: super::IsolationClass,
    pub mmio_count: usize,
    pub dma_count: usize,
    pub irq_count: usize,
}

/// List all twin drivers
pub fn list_drivers() -> Vec<TwinDriverId> {
    let drivers = TWIN_DRIVERS.lock();
    drivers
        .iter()
        .filter_map(|slot| slot.as_ref().map(|d| d.id))
        .collect()
}
