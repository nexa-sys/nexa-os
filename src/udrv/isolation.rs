//! Isolation Classes for User-space Drivers
//!
//! Implements HongMeng's differentiated isolation classes:
//!
//! - **IC0**: Core TCB - No isolation, direct function calls
//! - **IC1**: Mechanism-enforced isolation (hardware-assisted)
//! - **IC2**: Full address space isolation (process-based)
//!
//! # Design Rationale
//!
//! Not all drivers require the same level of isolation. Mature, verified,
//! and performance-critical drivers can use weaker isolation (IC1) for
//! optimal performance, while third-party or untrusted drivers use
//! stronger isolation (IC2).
//!
//! # Isolation Class Properties
//!
//! | Class | Isolation | IPC Cost | Security | Use Case |
//! |-------|-----------|----------|----------|----------|
//! | IC0   | None      | ~18 cycles | Core TCB | ABI shim |
//! | IC1   | PKS/Watch | ~500 cycles | Verified | FS, MM |
//! | IC2   | Address   | ~1000 cycles | Full | Drivers |
//!
//! # Architecture
//!
//! ```text
//! IC0 (Core TCB)
//! ├── Direct function calls
//! ├── No privilege switch
//! └── Part of kernel TCB
//!
//! IC1 (Mechanism Isolation)
//! ├── Kernel address space
//! ├── Hardware domain isolation (PKS/Watchpoint)
//! ├── CFI + secure monitor
//! └── Fast IPC via gate
//!
//! IC2 (Address Space Isolation)
//! ├── Separate address space
//! ├── Ring 3 privilege level
//! ├── Full syscall overhead
//! └── Standard IPC
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

/// Maximum number of IC1 domains (hardware limited)
/// Intel PKS: 16 domains, ARM Watchpoint: 4 domains
pub const MAX_IC1_DOMAINS: usize = 16;

/// Isolation class levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum IsolationClass {
    /// Core TCB - No isolation, function calls only
    /// Security: Part of trusted computing base
    /// Performance: ~18 cycles roundtrip IPC
    IC0 = 0,

    /// Mechanism-enforced isolation in kernel space
    /// Security: Hardware domain isolation (PKS/Watchpoint)
    /// Performance: ~500 cycles roundtrip IPC
    IC1 = 1,

    /// Full address space isolation (userspace)
    /// Security: Process isolation + privilege separation
    /// Performance: ~1000 cycles roundtrip IPC
    IC2 = 2,
}

impl IsolationClass {
    /// Get the security level (higher = more secure)
    pub fn security_level(&self) -> u8 {
        match self {
            IsolationClass::IC0 => 0, // Least secure (in TCB)
            IsolationClass::IC1 => 1, // Medium security
            IsolationClass::IC2 => 2, // Most secure
        }
    }

    /// Get approximate IPC latency in cycles
    pub fn ipc_latency_cycles(&self) -> u32 {
        match self {
            IsolationClass::IC0 => 18,   // Direct function call
            IsolationClass::IC1 => 500,  // Gate + domain switch
            IsolationClass::IC2 => 1000, // Full context switch
        }
    }

    /// Check if this class can access another class
    pub fn can_access(&self, target: IsolationClass) -> bool {
        // Lower isolation classes can access higher ones
        // But not vice versa (security principle)
        *self <= target
    }
}

/// IC1 Domain descriptor
#[derive(Debug, Clone, Copy)]
pub struct IC1Domain {
    /// Domain ID (0-15 for PKS)
    pub id: u8,
    /// Whether domain is allocated
    pub allocated: bool,
    /// Associated service/driver ID
    pub owner_id: u32,
    /// Memory region base address
    pub mem_base: u64,
    /// Memory region size
    pub mem_size: u64,
}

impl IC1Domain {
    const fn empty() -> Self {
        Self {
            id: 0,
            allocated: false,
            owner_id: 0,
            mem_base: 0,
            mem_size: 0,
        }
    }
}

/// IC1 Domain manager
struct IC1DomainManager {
    domains: [IC1Domain; MAX_IC1_DOMAINS],
    allocated_count: usize,
}

impl IC1DomainManager {
    const fn new() -> Self {
        Self {
            domains: [IC1Domain::empty(); MAX_IC1_DOMAINS],
            allocated_count: 0,
        }
    }

    fn allocate(&mut self, owner_id: u32) -> Result<u8, IsolationError> {
        // Domain 0 is reserved for kernel
        for i in 1..MAX_IC1_DOMAINS {
            if !self.domains[i].allocated {
                self.domains[i] = IC1Domain {
                    id: i as u8,
                    allocated: true,
                    owner_id,
                    mem_base: 0,
                    mem_size: 0,
                };
                self.allocated_count += 1;
                return Ok(i as u8);
            }
        }
        Err(IsolationError::DomainsFull)
    }

    fn deallocate(&mut self, domain_id: u8) -> Result<(), IsolationError> {
        let idx = domain_id as usize;
        if idx >= MAX_IC1_DOMAINS || idx == 0 {
            return Err(IsolationError::InvalidDomain);
        }
        if !self.domains[idx].allocated {
            return Err(IsolationError::NotAllocated);
        }
        self.domains[idx] = IC1Domain::empty();
        self.domains[idx].id = domain_id;
        self.allocated_count -= 1;
        Ok(())
    }

    fn get(&self, domain_id: u8) -> Option<&IC1Domain> {
        let idx = domain_id as usize;
        if idx < MAX_IC1_DOMAINS && self.domains[idx].allocated {
            Some(&self.domains[idx])
        } else {
            None
        }
    }

    fn set_memory_region(
        &mut self,
        domain_id: u8,
        base: u64,
        size: u64,
    ) -> Result<(), IsolationError> {
        let idx = domain_id as usize;
        if idx >= MAX_IC1_DOMAINS {
            return Err(IsolationError::InvalidDomain);
        }
        if !self.domains[idx].allocated {
            return Err(IsolationError::NotAllocated);
        }
        self.domains[idx].mem_base = base;
        self.domains[idx].mem_size = size;
        Ok(())
    }
}

/// Isolation gate for IC1 transitions
#[repr(C)]
pub struct IsolationGate {
    /// Target domain ID
    pub target_domain: u8,
    /// Entry point address
    pub entry_point: u64,
    /// Stack pointer for target domain
    pub stack_ptr: u64,
    /// Gate flags
    pub flags: u32,
}

/// Gate flags
pub mod gate_flags {
    pub const GATE_ENABLED: u32 = 1 << 0;
    pub const GATE_REENTRANT: u32 = 1 << 1;
    pub const GATE_TRACE: u32 = 1 << 2;
}

/// IC2 Process isolation context
#[derive(Debug, Clone)]
pub struct IC2Context {
    /// Process ID
    pub pid: u32,
    /// Page table root (CR3)
    pub cr3: u64,
    /// User stack base
    pub stack_base: u64,
    /// User stack size
    pub stack_size: u64,
    /// Heap base
    pub heap_base: u64,
    /// Current heap end (brk)
    pub heap_end: u64,
}

impl IC2Context {
    pub fn new(pid: u32, cr3: u64) -> Self {
        Self {
            pid,
            cr3,
            stack_base: crate::process::STACK_BASE as u64,
            stack_size: crate::process::STACK_SIZE as u64,
            heap_base: crate::process::HEAP_BASE as u64,
            heap_end: crate::process::HEAP_BASE as u64,
        }
    }
}

/// Isolation error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationError {
    /// IC1 domains exhausted
    DomainsFull,
    /// Invalid domain ID
    InvalidDomain,
    /// Domain not allocated
    NotAllocated,
    /// Permission denied
    PermissionDenied,
    /// Invalid isolation class transition
    InvalidTransition,
    /// Hardware not supported
    HardwareNotSupported,
    /// Memory region conflict
    MemoryConflict,
}

// Global state
static IC1_MANAGER: Mutex<IC1DomainManager> = Mutex::new(IC1DomainManager::new());
static NEXT_IC2_ID: AtomicU32 = AtomicU32::new(1);

/// Initialize isolation subsystem
pub fn init() {
    crate::kinfo!("UDRV/Isolation: Initializing isolation classes");

    // Check hardware support for IC1
    let pks_supported = check_pks_support();
    let watchpoint_supported = check_watchpoint_support();

    if pks_supported {
        crate::kinfo!("UDRV/Isolation: Intel PKS supported - IC1 available");
    } else if watchpoint_supported {
        crate::kinfo!("UDRV/Isolation: ARM Watchpoint supported - IC1 available");
    } else {
        crate::kwarn!("UDRV/Isolation: No IC1 hardware support - falling back to IC2 only");
    }

    // Reserve domain 0 for kernel
    let mut manager = IC1_MANAGER.lock();
    manager.domains[0] = IC1Domain {
        id: 0,
        allocated: true,
        owner_id: 0, // Kernel
        mem_base: 0,
        mem_size: 0,
    };

    crate::kinfo!(
        "UDRV/Isolation: {} IC1 domains available",
        MAX_IC1_DOMAINS - 1
    );
}

/// Check for Intel PKS support
fn check_pks_support() -> bool {
    // CPUID check for PKS (bit 31 of ECX in leaf 7, subleaf 0)
    #[cfg(target_arch = "x86_64")]
    {
        let result = unsafe { core::arch::x86_64::__cpuid_count(7, 0) };
        (result.ecx & (1 << 31)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Check for ARM Watchpoint support
fn check_watchpoint_support() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        // Check debug architecture version
        true // Assume supported on AArch64
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}

/// Allocate an IC1 domain
pub fn allocate_ic1_domain(owner_id: u32) -> Result<u8, IsolationError> {
    IC1_MANAGER.lock().allocate(owner_id)
}

/// Deallocate an IC1 domain
pub fn deallocate_ic1_domain(domain_id: u8) -> Result<(), IsolationError> {
    IC1_MANAGER.lock().deallocate(domain_id)
}

/// Configure IC1 domain memory region
pub fn configure_ic1_memory(domain_id: u8, base: u64, size: u64) -> Result<(), IsolationError> {
    IC1_MANAGER.lock().set_memory_region(domain_id, base, size)
}

/// Get IC1 domain info
pub fn get_ic1_domain(domain_id: u8) -> Option<IC1Domain> {
    IC1_MANAGER.lock().get(domain_id).copied()
}

/// Create IC2 context for a new driver process
pub fn create_ic2_context(cr3: u64) -> IC2Context {
    let pid = NEXT_IC2_ID.fetch_add(1, Ordering::SeqCst);
    IC2Context::new(pid, cr3)
}

/// Switch to IC1 domain (kernel-internal)
///
/// # Safety
/// Caller must ensure domain is valid and properly configured
#[inline(always)]
pub unsafe fn switch_ic1_domain(domain_id: u8) {
    #[cfg(target_arch = "x86_64")]
    {
        // Use WRPKRU to switch protection key
        // Domain ID maps to protection key
        let pkru_value = if domain_id == 0 {
            0 // Full access for kernel domain
        } else {
            // Disable write for all other domains
            !((3u32) << (domain_id as u32 * 2))
        };
        core::arch::asm!(
            "wrpkru",
            in("eax") pkru_value,
            in("ecx") 0u32,
            in("edx") 0u32,
            options(nomem, nostack)
        );
    }
}

/// Enter IC1 gate (transition between IC1 domains)
pub fn enter_ic1_gate(gate: &IsolationGate) -> Result<(), IsolationError> {
    if gate.flags & gate_flags::GATE_ENABLED == 0 {
        return Err(IsolationError::PermissionDenied);
    }

    // Verify target domain exists
    if get_ic1_domain(gate.target_domain).is_none() {
        return Err(IsolationError::InvalidDomain);
    }

    // Switch domain and jump to entry point
    unsafe {
        switch_ic1_domain(gate.target_domain);
        // Entry point call would happen here in real implementation
    }

    Ok(())
}

/// Get current isolation class for a process
pub fn current_isolation_class() -> IsolationClass {
    // In kernel context, we're at IC0
    // This would be determined by checking privilege level and domain
    #[cfg(target_arch = "x86_64")]
    {
        let cs: u16;
        unsafe {
            core::arch::asm!("mov {:x}, cs", out(reg) cs, options(nomem, nostack));
        }
        let cpl = cs & 0x3;
        if cpl == 0 {
            // Kernel mode - could be IC0 or IC1
            IsolationClass::IC0
        } else {
            // User mode - IC2
            IsolationClass::IC2
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        IsolationClass::IC0
    }
}
