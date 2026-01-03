//! RPC-like IPC for User-space Driver Framework
//!
//! Implements HongMeng's synchronous RPC-like IPC fastpath for service
//! invocations, addressing resource allocation, exhaustion, and accounting.
//!
//! # Design Philosophy
//!
//! Traditional IPC assumes symmetric endpoints. However, most driver
//! invocations are procedure calls where caller and callee are clearly
//! identified. Synchronous RPC is more appropriate.
//!
//! # Key Features
//!
//! ## Thread Migration
//! - Direct switch bypassing scheduling
//! - Switch only stack/instruction pointer
//! - Avoid switching other registers
//!
//! ## Resource Management
//! - Pre-bind stacks for frequently-used services
//! - Adaptive stack pool sizing
//! - Reserved pool for OOM recovery
//!
//! ## Resource Accounting
//! - Track root caller through IPC chains
//! - Attribute resources to original caller
//! - Support energy and memory accounting
//!
//! # Architecture
//!
//! ```text
//! Application (IC2)
//!     │
//!     │ RPC Call
//!     ▼
//! ┌───────────────┐
//! │ RPC Channel   │
//! │  ┌─────────┐  │
//! │  │ Args    │  │ ◄─ Registers + Shared Memory
//! │  ├─────────┤  │
//! │  │ Stack   │  │ ◄─ Pre-allocated stack pool
//! │  ├─────────┤  │
//! │  │ Context │  │ ◄─ Invocation context
//! │  └─────────┘  │
//! └───────┬───────┘
//!         │
//!         │ Direct Switch (bypass scheduler)
//!         ▼
//! Service Handler (IC1/IC2)
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

/// Maximum RPC channels
pub const MAX_RPC_CHANNELS: usize = 256;

/// Maximum message size in registers
pub const RPC_REG_ARGS: usize = 6;

/// Maximum message size in shared memory
pub const RPC_SHMEM_SIZE: usize = 4096;

/// Maximum invocation stack depth
pub const MAX_INVOCATION_DEPTH: usize = 16;

/// Stack pool size per service
pub const STACK_POOL_SIZE: usize = 32;

/// Reserved stacks for OOM recovery
pub const RESERVED_STACKS: usize = 4;

/// Stack size for RPC handlers
pub const RPC_STACK_SIZE: usize = 16384; // 16KB

/// RPC channel for service invocation
#[derive(Debug)]
pub struct RpcChannel {
    /// Channel ID
    pub id: u32,
    /// Service ID this channel connects to
    pub service_id: u32,
    /// Caller process ID
    pub caller_pid: u32,
    /// Channel state
    pub state: RpcChannelState,
    /// Pre-bound stack for this channel (if any)
    pub bound_stack: Option<u64>,
    /// Invocation context stack
    pub invocation_stack: InvocationStack,
    /// Resource accounting
    pub accounting: RpcAccounting,
}

/// RPC channel state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RpcChannelState {
    /// Channel is idle
    Idle = 0,
    /// Call in progress
    Calling = 1,
    /// Waiting for return
    Waiting = 2,
    /// Call completed
    Completed = 3,
    /// Error state
    Error = 4,
}

/// Invocation context entry
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InvocationContext {
    /// Saved instruction pointer
    pub rip: u64,
    /// Saved stack pointer
    pub rsp: u64,
    /// Saved base pointer
    pub rbp: u64,
    /// Saved CR3 (page table)
    pub cr3: u64,
    /// Caller domain ID
    pub domain_id: u8,
    /// Flags
    pub flags: u8,
    /// Reserved
    pub _reserved: [u8; 6],
}

impl InvocationContext {
    const fn empty() -> Self {
        Self {
            rip: 0,
            rsp: 0,
            rbp: 0,
            cr3: 0,
            domain_id: 0,
            flags: 0,
            _reserved: [0; 6],
        }
    }
}

/// Invocation stack for nested RPC calls
#[derive(Debug)]
pub struct InvocationStack {
    entries: [InvocationContext; MAX_INVOCATION_DEPTH],
    depth: usize,
}

impl InvocationStack {
    const fn new() -> Self {
        Self {
            entries: [InvocationContext::empty(); MAX_INVOCATION_DEPTH],
            depth: 0,
        }
    }

    fn push(&mut self, ctx: InvocationContext) -> Result<(), RpcError> {
        if self.depth >= MAX_INVOCATION_DEPTH {
            return Err(RpcError::StackOverflow);
        }
        self.entries[self.depth] = ctx;
        self.depth += 1;
        Ok(())
    }

    fn pop(&mut self) -> Result<InvocationContext, RpcError> {
        if self.depth == 0 {
            return Err(RpcError::StackUnderflow);
        }
        self.depth -= 1;
        Ok(self.entries[self.depth])
    }

    fn current_depth(&self) -> usize {
        self.depth
    }
}

/// Resource accounting for RPC
#[derive(Debug, Clone, Copy)]
pub struct RpcAccounting {
    /// Root caller PID (for resource attribution)
    pub root_caller: u32,
    /// CPU time consumed (in cycles)
    pub cpu_cycles: u64,
    /// Memory allocated (in bytes)
    pub memory_bytes: u64,
    /// IPC count
    pub ipc_count: u64,
    /// Start timestamp of current call
    pub call_start: u64,
}

impl RpcAccounting {
    const fn new(root_caller: u32) -> Self {
        Self {
            root_caller,
            cpu_cycles: 0,
            memory_bytes: 0,
            ipc_count: 0,
            call_start: 0,
        }
    }
}

/// RPC message structure
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RpcMessage {
    /// Message type
    pub msg_type: RpcMessageType,
    /// Flags
    pub flags: u32,
    /// Register arguments
    pub args: [u64; RPC_REG_ARGS],
    /// Shared memory offset (if any)
    pub shmem_offset: u64,
    /// Shared memory length
    pub shmem_len: u64,
}

/// RPC message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RpcMessageType {
    /// Call request
    Call = 0,
    /// Call reply
    Reply = 1,
    /// Error notification
    Error = 2,
    /// Resource exhaustion notification
    ResourceExhausted = 3,
}

/// RPC result type
pub type RpcResult<T> = Result<T, RpcError>;

/// RPC error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcError {
    /// Channel not found
    ChannelNotFound,
    /// Service not found
    ServiceNotFound,
    /// Invalid state
    InvalidState,
    /// Invocation stack overflow
    StackOverflow,
    /// Invocation stack underflow
    StackUnderflow,
    /// No available stacks
    NoStacks,
    /// Resource exhausted (OOM)
    ResourceExhausted,
    /// Permission denied
    PermissionDenied,
    /// Timeout
    Timeout,
    /// Channel table full
    TableFull,
    /// Invalid message
    InvalidMessage,
}

/// Stack pool for RPC handlers
#[derive(Debug, Clone)]
struct StackPool {
    /// Available stacks
    stacks: [u64; STACK_POOL_SIZE],
    /// Number of available stacks
    available: usize,
    /// Number of reserved stacks (for OOM recovery)
    reserved: usize,
    /// Total allocated stacks
    total_allocated: usize,
}

impl StackPool {
    const fn new() -> Self {
        Self {
            stacks: [0; STACK_POOL_SIZE],
            available: 0,
            reserved: RESERVED_STACKS,
            total_allocated: 0,
        }
    }

    fn allocate(&mut self, for_recovery: bool) -> Option<u64> {
        // If not for recovery, leave reserved stacks
        let min_available = if for_recovery { 0 } else { self.reserved };

        if self.available <= min_available {
            return None;
        }

        self.available -= 1;
        let stack = self.stacks[self.available];
        self.stacks[self.available] = 0;
        Some(stack)
    }

    fn return_stack(&mut self, stack: u64) {
        if self.available < STACK_POOL_SIZE {
            self.stacks[self.available] = stack;
            self.available += 1;
        }
    }

    fn add_stacks(&mut self, stacks: &[u64]) {
        for &stack in stacks {
            if self.available < STACK_POOL_SIZE {
                self.stacks[self.available] = stack;
                self.available += 1;
                self.total_allocated += 1;
            }
        }
    }

    fn needs_more(&self) -> bool {
        // Request more when below threshold
        self.available < self.reserved + 4
    }
}

/// Service endpoint descriptor
#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    /// Service ID
    pub id: u32,
    /// Entry point address
    pub entry_point: u64,
    /// Service domain ID
    pub domain_id: u8,
    /// Isolation class
    pub isolation_class: super::IsolationClass,
    /// Stack pool for this service
    pub stack_pool: StackPool,
    /// Bound channels
    pub bound_channels: Vec<u32>,
}

// Global state
static CHANNELS: Mutex<[Option<RpcChannel>; MAX_RPC_CHANNELS]> =
    Mutex::new([const { None }; MAX_RPC_CHANNELS]);
static NEXT_CHANNEL_ID: AtomicU32 = AtomicU32::new(1);
static SERVICES: Mutex<Vec<ServiceEndpoint>> = Mutex::new(Vec::new());

/// Initialize RPC subsystem
pub fn init() {
    crate::kinfo!("UDRV/RPC: Initializing RPC-like IPC subsystem");
    crate::kinfo!(
        "UDRV/RPC: {} max channels, {} args, {} shmem",
        MAX_RPC_CHANNELS,
        RPC_REG_ARGS,
        RPC_SHMEM_SIZE
    );
}

/// Register a service endpoint
pub fn register_service(
    entry_point: u64,
    domain_id: u8,
    isolation_class: super::IsolationClass,
) -> RpcResult<u32> {
    let mut services = SERVICES.lock();
    let id = services.len() as u32 + 1;

    services.push(ServiceEndpoint {
        id,
        entry_point,
        domain_id,
        isolation_class,
        stack_pool: StackPool::new(),
        bound_channels: Vec::new(),
    });

    crate::kinfo!(
        "UDRV/RPC: Registered service {} at {:#x} (IC{:?})",
        id,
        entry_point,
        isolation_class
    );

    Ok(id)
}

/// Create an RPC channel to a service
pub fn create_channel(service_id: u32, caller_pid: u32) -> RpcResult<u32> {
    // Verify service exists
    let services = SERVICES.lock();
    if !services.iter().any(|s| s.id == service_id) {
        return Err(RpcError::ServiceNotFound);
    }
    drop(services);

    let mut channels = CHANNELS.lock();

    // Find empty slot
    for slot in channels.iter_mut() {
        if slot.is_none() {
            let id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::SeqCst);

            *slot = Some(RpcChannel {
                id,
                service_id,
                caller_pid,
                state: RpcChannelState::Idle,
                bound_stack: None,
                invocation_stack: InvocationStack::new(),
                accounting: RpcAccounting::new(caller_pid),
            });

            return Ok(id);
        }
    }

    Err(RpcError::TableFull)
}

/// Perform an RPC call
pub fn call(channel_id: u32, msg: &RpcMessage) -> RpcResult<RpcMessage> {
    let mut channels = CHANNELS.lock();

    let channel = channels
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == channel_id))
        .ok_or(RpcError::ChannelNotFound)?;

    if channel.state != RpcChannelState::Idle {
        return Err(RpcError::InvalidState);
    }

    // Get service endpoint
    let services = SERVICES.lock();
    let service = services
        .iter()
        .find(|s| s.id == channel.service_id)
        .ok_or(RpcError::ServiceNotFound)?;

    let entry_point = service.entry_point;
    let domain_id = service.domain_id;
    let isolation_class = service.isolation_class;
    drop(services);

    // Save caller context
    let ctx = InvocationContext {
        rip: 0, // Would be set by actual call mechanism
        rsp: 0,
        rbp: 0,
        cr3: 0,
        domain_id: 0, // Caller domain
        flags: 0,
        _reserved: [0; 6],
    };
    channel.invocation_stack.push(ctx)?;

    // Start accounting
    channel.accounting.call_start = crate::safety::rdtsc();
    channel.accounting.ipc_count += 1;
    channel.state = RpcChannelState::Calling;

    // Get stack for handler
    let stack = channel
        .bound_stack
        .or_else(|| {
            let mut services = SERVICES.lock();
            services
                .iter_mut()
                .find(|s| s.id == channel.service_id)?
                .stack_pool
                .allocate(false)
        })
        .ok_or(RpcError::NoStacks)?;

    drop(channels);

    // Perform the actual call based on isolation class
    let result = match isolation_class {
        super::IsolationClass::IC0 => {
            // Direct function call
            call_ic0(entry_point, msg, stack)
        }
        super::IsolationClass::IC1 => {
            // Gate-based call
            call_ic1(entry_point, msg, stack, domain_id)
        }
        super::IsolationClass::IC2 => {
            // Full context switch
            call_ic2(entry_point, msg, stack, domain_id)
        }
    };

    // Update accounting
    let mut channels = CHANNELS.lock();
    if let Some(channel) = channels
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == channel_id))
    {
        let end_time = crate::safety::rdtsc();
        channel.accounting.cpu_cycles += end_time - channel.accounting.call_start;

        // Return stack if not bound
        if channel.bound_stack.is_none() {
            let mut services = SERVICES.lock();
            if let Some(service) = services.iter_mut().find(|s| s.id == channel.service_id) {
                service.stack_pool.return_stack(stack);
            }
        }

        // Pop invocation context
        let _ = channel.invocation_stack.pop();
        channel.state = RpcChannelState::Idle;
    }

    result
}

/// IC0 call - direct function call (kernel internal)
fn call_ic0(entry_point: u64, msg: &RpcMessage, _stack: u64) -> RpcResult<RpcMessage> {
    // In IC0, we just call the function directly
    // This is the fastest path (~18 cycles)

    // Safety: IC0 services are part of TCB
    let handler: fn(&RpcMessage) -> RpcResult<RpcMessage> =
        unsafe { core::mem::transmute(entry_point) };

    handler(msg)
}

/// IC1 call - gate-based call with domain switch
fn call_ic1(
    entry_point: u64,
    msg: &RpcMessage,
    stack: u64,
    domain_id: u8,
) -> RpcResult<RpcMessage> {
    // IC1 uses hardware domain isolation (PKS/Watchpoint)
    // Switch domain, change stack, call handler

    // Switch to target domain
    unsafe {
        super::isolation::switch_ic1_domain(domain_id);
    }

    // Call handler with new stack
    let result = unsafe { call_with_stack(entry_point, msg, stack) };

    // Switch back to kernel domain (0)
    unsafe {
        super::isolation::switch_ic1_domain(0);
    }

    result
}

/// IC2 call - full context switch to userspace
fn call_ic2(
    entry_point: u64,
    msg: &RpcMessage,
    stack: u64,
    domain_id: u8,
) -> RpcResult<RpcMessage> {
    // IC2 uses full address space isolation
    // This requires a syscall return to userspace and wait for completion

    // For now, simulate the call
    // In real implementation, this would:
    // 1. Switch CR3 to user process page table
    // 2. Switch to Ring 3
    // 3. Jump to entry point with msg in registers
    // 4. Wait for sysret

    // Placeholder - actual implementation requires scheduler integration
    Err(RpcError::InvalidState)
}

/// Call handler with specified stack
///
/// # Safety
/// Caller must ensure entry_point and stack are valid
unsafe fn call_with_stack(entry_point: u64, msg: &RpcMessage, stack: u64) -> RpcResult<RpcMessage> {
    // This would use inline assembly to switch stacks
    // For now, just call directly
    let handler: fn(&RpcMessage) -> RpcResult<RpcMessage> = core::mem::transmute(entry_point);
    handler(msg)
}

/// Bind a stack to a channel for frequent calls
pub fn bind_stack(channel_id: u32, stack: u64) -> RpcResult<()> {
    let mut channels = CHANNELS.lock();
    let channel = channels
        .iter_mut()
        .find_map(|slot| slot.as_mut().filter(|c| c.id == channel_id))
        .ok_or(RpcError::ChannelNotFound)?;

    channel.bound_stack = Some(stack);
    Ok(())
}

/// Get accounting info for a channel
pub fn get_accounting(channel_id: u32) -> RpcResult<RpcAccounting> {
    let channels = CHANNELS.lock();
    let channel = channels
        .iter()
        .find_map(|slot| slot.as_ref().filter(|c| c.id == channel_id))
        .ok_or(RpcError::ChannelNotFound)?;

    Ok(channel.accounting)
}

/// Destroy an RPC channel
pub fn destroy_channel(channel_id: u32) -> RpcResult<()> {
    let mut channels = CHANNELS.lock();

    for slot in channels.iter_mut() {
        if let Some(channel) = slot {
            if channel.id == channel_id {
                // Return bound stack if any
                if let Some(stack) = channel.bound_stack {
                    let mut services = SERVICES.lock();
                    if let Some(service) = services.iter_mut().find(|s| s.id == channel.service_id)
                    {
                        service.stack_pool.return_stack(stack);
                    }
                }
                *slot = None;
                return Ok(());
            }
        }
    }

    Err(RpcError::ChannelNotFound)
}

/// Handle OOM during RPC
/// Uses reserved stacks to call memory manager for reclaim
pub fn handle_oom_recovery() -> RpcResult<()> {
    crate::kwarn!("UDRV/RPC: OOM during RPC, attempting recovery");

    // Find memory manager service
    let services = SERVICES.lock();
    let mem_mgr = services
        .iter()
        .find(|s| s.id == 1) // Assume service 1 is memory manager
        .ok_or(RpcError::ServiceNotFound)?;

    let entry_point = mem_mgr.entry_point;
    drop(services);

    // Use reserved stack for recovery
    let mut services = SERVICES.lock();
    let mem_mgr = services
        .iter_mut()
        .find(|s| s.id == 1)
        .ok_or(RpcError::ServiceNotFound)?;

    let stack = mem_mgr
        .stack_pool
        .allocate(true)
        .ok_or(RpcError::NoStacks)?;

    drop(services);

    // Call memory reclaim
    let msg = RpcMessage {
        msg_type: RpcMessageType::Call,
        flags: 0,
        args: [0; RPC_REG_ARGS], // Reclaim request
        shmem_offset: 0,
        shmem_len: 0,
    };

    let _ = call_ic0(entry_point, &msg, stack)?;

    // Return stack
    let mut services = SERVICES.lock();
    if let Some(mem_mgr) = services.iter_mut().find(|s| s.id == 1) {
        mem_mgr.stack_pool.return_stack(stack);
    }

    Ok(())
}
