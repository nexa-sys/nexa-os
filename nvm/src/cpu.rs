//! Virtual CPU Emulation
//!
//! This module provides a comprehensive x86-64 CPU emulation layer for kernel testing.
//! Unlike a full emulator (QEMU/Bochs), we focus on the CPU state and privileged
//! instructions the kernel uses, rather than general instruction execution.
//!
//! ## Features
//!
//! - **Control registers** (CR0, CR2, CR3, CR4, CR8)
//! - **MSRs** (Model Specific Registers) - EFER, STAR, LSTAR, APIC_BASE, etc.
//! - **CPUID** - Emulated CPU feature detection
//! - **Interrupt state** - IF flag, NMI, virtual interrupts
//! - **Debug registers** - DR0-DR7 for hardware breakpoints
//! - **Execution control** - Single-step, breakpoints, execution hooks
//! - **Performance counters** - Virtual PMU for performance monitoring
//! - **Exception handling** - #PF, #GP, #UD simulation
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                       VirtualCpu                               │
//! ├────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐│
//! │  │  Registers  │  │   Control   │  │     Execution Engine    ││
//! │  │  RAX-R15    │  │   CR0-CR8   │  │  - Breakpoints          ││
//! │  │  RIP, RSP   │  │   DR0-DR7   │  │  - Single-step          ││
//! │  │  RFLAGS     │  │   MSRs      │  │  - Event hooks          ││
//! │  └─────────────┘  └─────────────┘  └─────────────────────────┘│
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐│
//! │  │   CPUID     │  │  Interrupts │  │   Performance Monitor   ││
//! │  │   Cache     │  │  IDT, APIC  │  │   PMC0-PMC7, Fixed      ││
//! │  └─────────────┘  └─────────────┘  └─────────────────────────┘│
//! └────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Condvar};

/// x86-64 general purpose registers
#[derive(Debug, Clone, Default)]
pub struct Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl Registers {
    /// Create registers from a saved context (for debugging)
    pub fn from_context(ctx: &[u64; 18]) -> Self {
        Self {
            rax: ctx[0], rbx: ctx[1], rcx: ctx[2], rdx: ctx[3],
            rsi: ctx[4], rdi: ctx[5], rbp: ctx[6], rsp: ctx[7],
            r8: ctx[8], r9: ctx[9], r10: ctx[10], r11: ctx[11],
            r12: ctx[12], r13: ctx[13], r14: ctx[14], r15: ctx[15],
            rip: ctx[16], rflags: ctx[17],
        }
    }
    
    /// Export to array (for saving/restoring)
    pub fn to_array(&self) -> [u64; 18] {
        [
            self.rax, self.rbx, self.rcx, self.rdx,
            self.rsi, self.rdi, self.rbp, self.rsp,
            self.r8, self.r9, self.r10, self.r11,
            self.r12, self.r13, self.r14, self.r15,
            self.rip, self.rflags,
        ]
    }
}

/// CPU control registers
#[derive(Debug, Clone)]
pub struct ControlRegisters {
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,  // Task Priority Register (TPR)
}

impl Default for ControlRegisters {
    fn default() -> Self {
        Self {
            cr0: 0x8000_0011, // PE + ET + PG enabled
            cr2: 0,
            cr3: 0, // Will be set when paging is initialized
            cr4: 0x20, // PAE enabled
            cr8: 0,    // TPR = 0 (all interrupts enabled)
        }
    }
}

/// Debug registers (DR0-DR7)
#[derive(Debug, Clone, Default)]
pub struct DebugRegisters {
    pub dr0: u64, // Breakpoint 0 linear address
    pub dr1: u64, // Breakpoint 1 linear address
    pub dr2: u64, // Breakpoint 2 linear address
    pub dr3: u64, // Breakpoint 3 linear address
    pub dr6: u64, // Debug status
    pub dr7: u64, // Debug control
}

/// RFLAGS register bits
pub mod rflags {
    pub const CF: u64 = 1 << 0;   // Carry flag
    pub const PF: u64 = 1 << 2;   // Parity flag
    pub const AF: u64 = 1 << 4;   // Auxiliary carry flag
    pub const ZF: u64 = 1 << 6;   // Zero flag
    pub const SF: u64 = 1 << 7;   // Sign flag
    pub const TF: u64 = 1 << 8;   // Trap flag (single-step)
    pub const IF: u64 = 1 << 9;   // Interrupt enable flag
    pub const DF: u64 = 1 << 10;  // Direction flag
    pub const OF: u64 = 1 << 11;  // Overflow flag
    pub const IOPL: u64 = 3 << 12; // I/O privilege level
    pub const NT: u64 = 1 << 14;   // Nested task flag
    pub const RF: u64 = 1 << 16;   // Resume flag
    pub const VM: u64 = 1 << 17;   // Virtual 8086 mode
    pub const AC: u64 = 1 << 18;   // Alignment check
    pub const VIF: u64 = 1 << 19;  // Virtual interrupt flag
    pub const VIP: u64 = 1 << 20;  // Virtual interrupt pending
    pub const ID: u64 = 1 << 21;   // ID flag (CPUID support)
    
    /// Extract IOPL (I/O Privilege Level) from RFLAGS
    pub fn get_iopl(rflags: u64) -> u8 {
        ((rflags >> 12) & 3) as u8
    }
    
    /// Set IOPL in RFLAGS
    pub fn set_iopl(rflags: u64, iopl: u8) -> u64 {
        (rflags & !IOPL) | (((iopl as u64) & 3) << 12)
    }
}

/// MSR addresses used by the kernel
pub mod msr {
    pub const IA32_APIC_BASE: u32 = 0x1B;
    pub const IA32_MTRRCAP: u32 = 0xFE;
    pub const IA32_SYSENTER_CS: u32 = 0x174;
    pub const IA32_SYSENTER_ESP: u32 = 0x175;
    pub const IA32_SYSENTER_EIP: u32 = 0x176;
    pub const IA32_MCG_CAP: u32 = 0x179;
    pub const IA32_MCG_STATUS: u32 = 0x17A;
    pub const IA32_MCG_CTL: u32 = 0x17B;
    pub const IA32_PERFEVTSEL0: u32 = 0x186;
    pub const IA32_PERFEVTSEL1: u32 = 0x187;
    pub const IA32_PERF_STATUS: u32 = 0x198;
    pub const IA32_PERF_CTL: u32 = 0x199;
    pub const IA32_MISC_ENABLE: u32 = 0x1A0;
    pub const IA32_PAT: u32 = 0x277;
    pub const IA32_FIXED_CTR0: u32 = 0x309; // Instruction retired
    pub const IA32_FIXED_CTR1: u32 = 0x30A; // Unhalted core cycles
    pub const IA32_FIXED_CTR2: u32 = 0x30B; // Unhalted reference cycles
    pub const IA32_FIXED_CTR_CTRL: u32 = 0x38D;
    pub const IA32_PERF_GLOBAL_STATUS: u32 = 0x38E;
    pub const IA32_PERF_GLOBAL_CTRL: u32 = 0x38F;
    pub const IA32_PERF_GLOBAL_OVF_CTRL: u32 = 0x390;
    pub const IA32_PMC0: u32 = 0xC1;
    pub const IA32_PMC1: u32 = 0xC2;
    pub const IA32_PMC2: u32 = 0xC3;
    pub const IA32_PMC3: u32 = 0xC4;
    pub const IA32_EFER: u32 = 0xC0000080;
    pub const IA32_STAR: u32 = 0xC0000081;
    pub const IA32_LSTAR: u32 = 0xC0000082;
    pub const IA32_CSTAR: u32 = 0xC0000083;
    pub const IA32_FMASK: u32 = 0xC0000084;
    pub const IA32_FS_BASE: u32 = 0xC0000100;
    pub const IA32_GS_BASE: u32 = 0xC0000101;
    pub const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
    pub const IA32_TSC_AUX: u32 = 0xC0000103;
    
    /// EFER bits
    pub mod efer {
        pub const SCE: u64 = 1 << 0;   // System call extensions
        pub const LME: u64 = 1 << 8;   // Long mode enable
        pub const LMA: u64 = 1 << 10;  // Long mode active
        pub const NXE: u64 = 1 << 11;  // No-execute enable
        pub const SVME: u64 = 1 << 12; // Secure virtual machine enable
        pub const LMSLE: u64 = 1 << 13; // Long mode segment limit enable
        pub const FFXSR: u64 = 1 << 14; // Fast FXSAVE/FXRSTOR
        pub const TCE: u64 = 1 << 15;   // Translation cache extension
    }
}

/// CPUID feature flags
#[derive(Debug, Clone)]
pub struct CpuidFeatures {
    pub vendor: [u8; 12],
    pub brand: [u8; 48],
    pub max_basic_leaf: u32,
    pub max_extended_leaf: u32,
    
    // Feature bits (ECX:EDX for leaf 1)
    pub features_ecx: u32,
    pub features_edx: u32,
    
    // Extended features (ECX:EDX for leaf 0x80000001)
    pub ext_features_ecx: u32,
    pub ext_features_edx: u32,
    
    // Structured extended features (EBX:ECX:EDX for leaf 7)
    pub struct_ext_ebx: u32,
    pub struct_ext_ecx: u32,
    pub struct_ext_edx: u32,
    
    // Cache/TLB info
    pub cache_line_size: u8,
    pub l1d_cache_size: u32,
    pub l1i_cache_size: u32,
    pub l2_cache_size: u32,
    pub l3_cache_size: u32,
    
    // Topology info
    pub logical_processors: u8,
    pub physical_cores: u8,
}

/// CPUID feature bits for leaf 1 EDX
pub mod cpuid_edx {
    pub const FPU: u32 = 1 << 0;
    pub const VME: u32 = 1 << 1;
    pub const DE: u32 = 1 << 2;
    pub const PSE: u32 = 1 << 3;
    pub const TSC: u32 = 1 << 4;
    pub const MSR: u32 = 1 << 5;
    pub const PAE: u32 = 1 << 6;
    pub const MCE: u32 = 1 << 7;
    pub const CX8: u32 = 1 << 8;
    pub const APIC: u32 = 1 << 9;
    pub const SEP: u32 = 1 << 11;
    pub const MTRR: u32 = 1 << 12;
    pub const PGE: u32 = 1 << 13;
    pub const MCA: u32 = 1 << 14;
    pub const CMOV: u32 = 1 << 15;
    pub const PAT: u32 = 1 << 16;
    pub const PSE36: u32 = 1 << 17;
    pub const PSN: u32 = 1 << 18;
    pub const CLFSH: u32 = 1 << 19;
    pub const DS: u32 = 1 << 21;
    pub const ACPI: u32 = 1 << 22;
    pub const MMX: u32 = 1 << 23;
    pub const FXSR: u32 = 1 << 24;
    pub const SSE: u32 = 1 << 25;
    pub const SSE2: u32 = 1 << 26;
    pub const SS: u32 = 1 << 27;
    pub const HTT: u32 = 1 << 28;
    pub const TM: u32 = 1 << 29;
    pub const PBE: u32 = 1 << 31;
}

/// CPUID feature bits for leaf 1 ECX
pub mod cpuid_ecx {
    pub const SSE3: u32 = 1 << 0;
    pub const PCLMULQDQ: u32 = 1 << 1;
    pub const DTES64: u32 = 1 << 2;
    pub const MONITOR: u32 = 1 << 3;
    pub const DS_CPL: u32 = 1 << 4;
    pub const VMX: u32 = 1 << 5;
    pub const SMX: u32 = 1 << 6;
    pub const EST: u32 = 1 << 7;
    pub const TM2: u32 = 1 << 8;
    pub const SSSE3: u32 = 1 << 9;
    pub const CID: u32 = 1 << 10;
    pub const SDBG: u32 = 1 << 11;
    pub const FMA: u32 = 1 << 12;
    pub const CX16: u32 = 1 << 13;
    pub const XTPR: u32 = 1 << 14;
    pub const PDCM: u32 = 1 << 15;
    pub const PCID: u32 = 1 << 17;
    pub const DCA: u32 = 1 << 18;
    pub const SSE4_1: u32 = 1 << 19;
    pub const SSE4_2: u32 = 1 << 20;
    pub const X2APIC: u32 = 1 << 21;
    pub const MOVBE: u32 = 1 << 22;
    pub const POPCNT: u32 = 1 << 23;
    pub const TSC_DEADLINE: u32 = 1 << 24;
    pub const AES: u32 = 1 << 25;
    pub const XSAVE: u32 = 1 << 26;
    pub const OSXSAVE: u32 = 1 << 27;
    pub const AVX: u32 = 1 << 28;
    pub const F16C: u32 = 1 << 29;
    pub const RDRAND: u32 = 1 << 30;
    pub const HYPERVISOR: u32 = 1 << 31;
}

impl Default for CpuidFeatures {
    fn default() -> Self {
        // Default to a reasonable modern CPU (similar to Intel Skylake)
        Self {
            vendor: *b"NexaOSVirtua",  // 12 bytes: CPUID vendor string
            brand: *b"NexaOS Virtual CPU v2.0 @ 3.6GHz\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            max_basic_leaf: 0x16,
            max_extended_leaf: 0x8000001F,
            // SSE, SSE2, SSE3, SSSE3, SSE4.1, SSE4.2, POPCNT, AVX, XSAVE, HYPERVISOR
            features_ecx: cpuid_ecx::SSE3 | cpuid_ecx::PCLMULQDQ | cpuid_ecx::SSSE3 | 
                          cpuid_ecx::CX16 | cpuid_ecx::SSE4_1 | cpuid_ecx::SSE4_2 |
                          cpuid_ecx::POPCNT | cpuid_ecx::AES | cpuid_ecx::XSAVE |
                          cpuid_ecx::AVX | cpuid_ecx::RDRAND | cpuid_ecx::HYPERVISOR,
            // FPU, VME, DE, PSE, TSC, MSR, PAE, MCE, CX8, APIC, SEP, MTRR, PGE, MCA, CMOV, PAT, PSE36, CLFSH, MMX, FXSR, SSE, SSE2
            features_edx: cpuid_edx::FPU | cpuid_edx::VME | cpuid_edx::DE | cpuid_edx::PSE |
                          cpuid_edx::TSC | cpuid_edx::MSR | cpuid_edx::PAE | cpuid_edx::MCE |
                          cpuid_edx::CX8 | cpuid_edx::APIC | cpuid_edx::SEP | cpuid_edx::MTRR |
                          cpuid_edx::PGE | cpuid_edx::MCA | cpuid_edx::CMOV | cpuid_edx::PAT |
                          cpuid_edx::PSE36 | cpuid_edx::CLFSH | cpuid_edx::MMX | cpuid_edx::FXSR |
                          cpuid_edx::SSE | cpuid_edx::SSE2,
            // LAHF/SAHF, ABM, SSE4A, 3DNowPrefetch, RDTSCP, LM
            ext_features_ecx: 0x00000121, // LAHF/SAHF, ABM, Prefetch
            ext_features_edx: 0x2C100800, // RDTSCP, LM, etc.
            // FSGSBASE, TSC_ADJUST, BMI1, AVX2, SMEP, BMI2, ERMS, INVPCID
            struct_ext_ebx: 0x029C67AF,
            struct_ext_ecx: 0x00000000,
            struct_ext_edx: 0x00000000,
            // Cache info
            cache_line_size: 64,
            l1d_cache_size: 32 * 1024,  // 32KB
            l1i_cache_size: 32 * 1024,  // 32KB
            l2_cache_size: 256 * 1024,  // 256KB
            l3_cache_size: 8 * 1024 * 1024, // 8MB
            // Topology
            logical_processors: 4,
            physical_cores: 4,
        }
    }
}

/// CPU exception types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuException {
    DivideError = 0,
    Debug = 1,
    Nmi = 2,
    Breakpoint = 3,
    Overflow = 4,
    BoundRange = 5,
    InvalidOpcode = 6,
    DeviceNotAvailable = 7,
    DoubleFault = 8,
    InvalidTss = 10,
    SegmentNotPresent = 11,
    StackSegmentFault = 12,
    GeneralProtection = 13,
    PageFault = 14,
    X87FloatingPoint = 16,
    AlignmentCheck = 17,
    MachineCheck = 18,
    SimdFloatingPoint = 19,
    VirtualizationException = 20,
    ControlProtection = 21,
}

/// Pending interrupt info
#[derive(Debug, Clone)]
pub struct PendingInterrupt {
    pub vector: u8,
    pub error_code: Option<u32>,
    pub is_nmi: bool,
    pub is_external: bool,
}

/// Virtual performance counters (PMU emulation)
#[derive(Debug, Clone, Default)]
pub struct PerformanceCounters {
    /// Instructions retired (fixed counter 0)
    pub instructions_retired: u64,
    /// Unhalted core cycles (fixed counter 1)
    pub core_cycles: u64,
    /// Unhalted reference cycles (fixed counter 2)
    pub ref_cycles: u64,
    /// General purpose counters (PMC0-PMC7)
    pub pmc: [u64; 8],
    /// Performance event select registers
    pub perf_evtsel: [u64; 8],
    /// Fixed counter control
    pub fixed_ctr_ctrl: u64,
    /// Global control
    pub perf_global_ctrl: u64,
    /// Global status
    pub perf_global_status: u64,
}

/// Full CPU state (complete snapshot for save/restore)
#[derive(Debug, Clone)]
pub struct CpuState {
    pub regs: Registers,
    pub cr: ControlRegisters,
    pub dr: DebugRegisters,
    pub msrs: HashMap<u32, u64>,
    pub interrupts_enabled: bool,
    pub nmi_pending: bool,
    pub halted: bool,
    pub cpuid: CpuidFeatures,
    pub pmu: PerformanceCounters,
    /// Current privilege level (0 = kernel, 3 = user)
    pub cpl: u8,
    /// Pending exceptions
    pub pending_exception: Option<(CpuException, Option<u32>)>,
    /// Pending external interrupts
    pub pending_interrupts: VecDeque<PendingInterrupt>,
    /// Single-step mode enabled
    pub single_step: bool,
}

impl Default for CpuState {
    fn default() -> Self {
        let mut msrs = HashMap::new();
        // Initialize important MSRs
        msrs.insert(msr::IA32_EFER, msr::efer::LME | msr::efer::LMA | msr::efer::SCE);
        msrs.insert(msr::IA32_APIC_BASE, 0xFEE0_0900); // APIC enabled, BSP
        msrs.insert(msr::IA32_PAT, 0x0007040600070406); // Default PAT
        msrs.insert(msr::IA32_MISC_ENABLE, 0x00000001); // Fast string enabled
        
        Self {
            regs: Registers::default(),
            cr: ControlRegisters::default(),
            dr: DebugRegisters::default(),
            msrs,
            interrupts_enabled: false,
            nmi_pending: false,
            halted: false,
            cpuid: CpuidFeatures::default(),
            pmu: PerformanceCounters::default(),
            cpl: 0, // Start in ring 0
            pending_exception: None,
            pending_interrupts: VecDeque::new(),
            single_step: false,
        }
    }
}

impl CpuState {
    /// Create a snapshot of this CPU state
    pub fn snapshot(&self) -> CpuStateSnapshot {
        CpuStateSnapshot {
            regs: self.regs.clone(),
            cr: self.cr.clone(),
            dr: self.dr.clone(),
            msrs: self.msrs.clone(),
            interrupts_enabled: self.interrupts_enabled,
            nmi_pending: self.nmi_pending,
            halted: self.halted,
            cpl: self.cpl,
            pmu: self.pmu.clone(),
        }
    }
    
    /// Restore from a snapshot
    pub fn restore(&mut self, snapshot: &CpuStateSnapshot) {
        self.regs = snapshot.regs.clone();
        self.cr = snapshot.cr.clone();
        self.dr = snapshot.dr.clone();
        self.msrs = snapshot.msrs.clone();
        self.interrupts_enabled = snapshot.interrupts_enabled;
        self.nmi_pending = snapshot.nmi_pending;
        self.halted = snapshot.halted;
        self.cpl = snapshot.cpl;
        self.pmu = snapshot.pmu.clone();
        self.pending_exception = None;
        self.pending_interrupts.clear();
    }
}

/// Serializable CPU state snapshot (for VM snapshots)
#[derive(Debug, Clone)]
pub struct CpuStateSnapshot {
    pub regs: Registers,
    pub cr: ControlRegisters,
    pub dr: DebugRegisters,
    pub msrs: HashMap<u32, u64>,
    pub interrupts_enabled: bool,
    pub nmi_pending: bool,
    pub halted: bool,
    pub cpl: u8,
    pub pmu: PerformanceCounters,
}

/// Execution event for tracing/debugging
#[derive(Debug, Clone)]
pub enum CpuEvent {
    /// Instruction executed
    InstructionExecuted { rip: u64 },
    /// MSR read
    MsrRead { msr: u32, value: u64 },
    /// MSR write
    MsrWrite { msr: u32, old_value: u64, new_value: u64 },
    /// CR write
    CrWrite { cr: u8, old_value: u64, new_value: u64 },
    /// Exception raised
    Exception { vector: CpuException, error_code: Option<u32> },
    /// Interrupt delivered
    InterruptDelivered { vector: u8 },
    /// Privilege level change
    PrivilegeChange { from_cpl: u8, to_cpl: u8 },
    /// CPU halted
    Halted,
    /// CPU woken from halt
    Woken,
    /// Breakpoint hit
    BreakpointHit { address: u64, bp_type: BreakpointType },
}

/// Breakpoint types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakpointType {
    /// Execution breakpoint (INT3 or DR)
    Execution,
    /// Data write watchpoint
    DataWrite,
    /// Data read/write watchpoint
    DataReadWrite,
    /// I/O port breakpoint
    IoPort,
}

/// Software breakpoint info
#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub address: u64,
    pub bp_type: BreakpointType,
    pub enabled: bool,
    pub hit_count: u64,
    pub condition: Option<String>,
}

/// Virtual CPU emulation
/// 
/// This provides a complete virtual CPU with execution control, debugging support,
/// and performance monitoring. Designed to behave like a hypervisor's vCPU.
pub struct VirtualCpu {
    /// CPU ID (for SMP support)
    pub id: u32,
    /// CPU state (protected by lock for thread safety)
    state: RwLock<CpuState>,
    /// Time stamp counter (atomic for performance)
    tsc: AtomicU64,
    /// Cycle counter for TSC advancement
    cycle_count: AtomicU64,
    /// Execution paused flag
    paused: AtomicBool,
    /// Request exit flag (for vmexit-like behavior)
    exit_requested: AtomicBool,
    /// Event trace buffer
    event_trace: Mutex<VecDeque<CpuEvent>>,
    /// Maximum trace buffer size
    max_trace_size: usize,
    /// Breakpoints
    breakpoints: RwLock<HashMap<u64, Breakpoint>>,
    /// Event hooks (callbacks on specific events)
    event_hooks: RwLock<Vec<Box<dyn Fn(&CpuEvent) + Send + Sync>>>,
    /// Condition variable for pause/resume
    pause_cv: (Mutex<bool>, Condvar),
}

impl VirtualCpu {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            state: RwLock::new(CpuState::default()),
            tsc: AtomicU64::new(0),
            cycle_count: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            exit_requested: AtomicBool::new(false),
            event_trace: Mutex::new(VecDeque::with_capacity(10000)),
            max_trace_size: 10000,
            breakpoints: RwLock::new(HashMap::new()),
            event_hooks: RwLock::new(Vec::new()),
            pause_cv: (Mutex::new(false), Condvar::new()),
        }
    }
    
    /// Create bootstrap processor (BSP) 
    pub fn new_bsp() -> Self {
        Self::new(0)
    }
    
    /// Create application processor (AP)
    pub fn new_ap(id: u32) -> Self {
        assert!(id > 0, "AP ID must be > 0");
        let vcpu = Self::new(id);
        // AP starts halted
        vcpu.state.write().unwrap().halted = true;
        vcpu
    }
    
    // ========================================================================
    // Execution Control (VMware/Hyper-V style)
    // ========================================================================
    
    /// Pause CPU execution
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }
    
    /// Resume CPU execution
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        let (lock, cv) = &self.pause_cv;
        let mut paused = lock.lock().unwrap();
        *paused = false;
        cv.notify_all();
    }
    
    /// Check if CPU is paused
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    
    /// Request execution exit (like VMEXIT)
    pub fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::SeqCst);
    }
    
    /// Clear exit request
    pub fn clear_exit_request(&self) {
        self.exit_requested.store(false, Ordering::SeqCst);
    }
    
    /// Check if exit was requested
    pub fn exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::SeqCst)
    }
    
    /// Enable single-step mode (TF flag)
    pub fn enable_single_step(&self) {
        let mut state = self.state.write().unwrap();
        state.single_step = true;
        state.regs.rflags |= rflags::TF;
    }
    
    /// Disable single-step mode
    pub fn disable_single_step(&self) {
        let mut state = self.state.write().unwrap();
        state.single_step = false;
        state.regs.rflags &= !rflags::TF;
    }
    
    /// Check if single-stepping
    pub fn is_single_stepping(&self) -> bool {
        self.state.read().unwrap().single_step
    }
    
    // ========================================================================
    // Breakpoint Support (GDB-style debugging)
    // ========================================================================
    
    /// Add an execution breakpoint
    pub fn add_breakpoint(&self, address: u64) -> bool {
        let mut bps = self.breakpoints.write().unwrap();
        if bps.contains_key(&address) {
            return false;
        }
        bps.insert(address, Breakpoint {
            address,
            bp_type: BreakpointType::Execution,
            enabled: true,
            hit_count: 0,
            condition: None,
        });
        true
    }
    
    /// Add a watchpoint (data breakpoint)
    pub fn add_watchpoint(&self, address: u64, write_only: bool) -> bool {
        let bp_type = if write_only {
            BreakpointType::DataWrite
        } else {
            BreakpointType::DataReadWrite
        };
        
        let mut bps = self.breakpoints.write().unwrap();
        bps.insert(address, Breakpoint {
            address,
            bp_type,
            enabled: true,
            hit_count: 0,
            condition: None,
        });
        true
    }
    
    /// Remove a breakpoint
    pub fn remove_breakpoint(&self, address: u64) -> bool {
        self.breakpoints.write().unwrap().remove(&address).is_some()
    }
    
    /// Enable/disable a breakpoint
    pub fn set_breakpoint_enabled(&self, address: u64, enabled: bool) -> bool {
        if let Some(bp) = self.breakpoints.write().unwrap().get_mut(&address) {
            bp.enabled = enabled;
            true
        } else {
            false
        }
    }
    
    /// List all breakpoints
    pub fn list_breakpoints(&self) -> Vec<Breakpoint> {
        self.breakpoints.read().unwrap().values().cloned().collect()
    }
    
    /// Check if address hits a breakpoint
    pub fn check_breakpoint(&self, address: u64) -> Option<BreakpointType> {
        let mut bps = self.breakpoints.write().unwrap();
        if let Some(bp) = bps.get_mut(&address) {
            if bp.enabled {
                bp.hit_count += 1;
                self.record_event(CpuEvent::BreakpointHit {
                    address,
                    bp_type: bp.bp_type,
                });
                return Some(bp.bp_type);
            }
        }
        None
    }
    
    // ========================================================================
    // Event Tracing
    // ========================================================================
    
    /// Record a CPU event
    fn record_event(&self, event: CpuEvent) {
        // Call hooks
        for hook in self.event_hooks.read().unwrap().iter() {
            hook(&event);
        }
        
        // Add to trace buffer
        let mut trace = self.event_trace.lock().unwrap();
        if trace.len() >= self.max_trace_size {
            trace.pop_front();
        }
        trace.push_back(event);
    }
    
    /// Get recent events
    pub fn get_events(&self, count: usize) -> Vec<CpuEvent> {
        let trace = self.event_trace.lock().unwrap();
        trace.iter().rev().take(count).cloned().collect()
    }
    
    /// Clear event trace
    pub fn clear_events(&self) {
        self.event_trace.lock().unwrap().clear();
    }
    
    /// Add an event hook
    pub fn add_event_hook<F>(&self, hook: F)
    where
        F: Fn(&CpuEvent) + Send + Sync + 'static,
    {
        self.event_hooks.write().unwrap().push(Box::new(hook));
    }
    
    // ========================================================================
    // Snapshot/Restore (VMware style)
    // ========================================================================
    
    /// Create a snapshot of CPU state
    pub fn snapshot(&self) -> CpuStateSnapshot {
        self.state.read().unwrap().snapshot()
    }
    
    /// Restore from a snapshot
    pub fn restore(&self, snapshot: &CpuStateSnapshot) {
        self.state.write().unwrap().restore(snapshot);
        self.tsc.store(0, Ordering::SeqCst); // Reset TSC on restore
    }
    
    // ========================================================================
    // Control Register Operations
    // ========================================================================
    
    pub fn read_cr0(&self) -> u64 {
        self.state.read().unwrap().cr.cr0
    }
    
    pub fn write_cr0(&self, value: u64) {
        let mut state = self.state.write().unwrap();
        let old = state.cr.cr0;
        state.cr.cr0 = value;
        drop(state);
        self.record_event(CpuEvent::CrWrite { cr: 0, old_value: old, new_value: value });
    }
    
    pub fn read_cr2(&self) -> u64 {
        self.state.read().unwrap().cr.cr2
    }
    
    pub fn write_cr2(&self, value: u64) {
        let mut state = self.state.write().unwrap();
        let old = state.cr.cr2;
        state.cr.cr2 = value;
        drop(state);
        self.record_event(CpuEvent::CrWrite { cr: 2, old_value: old, new_value: value });
    }
    
    pub fn read_cr3(&self) -> u64 {
        self.state.read().unwrap().cr.cr3
    }
    
    pub fn write_cr3(&self, value: u64) {
        let mut state = self.state.write().unwrap();
        let old = state.cr.cr3;
        state.cr.cr3 = value;
        drop(state);
        self.record_event(CpuEvent::CrWrite { cr: 3, old_value: old, new_value: value });
        // CR3 write invalidates TLB - could track this if needed
    }
    
    pub fn read_cr4(&self) -> u64 {
        self.state.read().unwrap().cr.cr4
    }
    
    pub fn write_cr4(&self, value: u64) {
        let mut state = self.state.write().unwrap();
        let old = state.cr.cr4;
        state.cr.cr4 = value;
        drop(state);
        self.record_event(CpuEvent::CrWrite { cr: 4, old_value: old, new_value: value });
    }
    
    pub fn read_cr8(&self) -> u64 {
        self.state.read().unwrap().cr.cr8
    }
    
    pub fn write_cr8(&self, value: u64) {
        let mut state = self.state.write().unwrap();
        let old = state.cr.cr8;
        state.cr.cr8 = value;
        drop(state);
        self.record_event(CpuEvent::CrWrite { cr: 8, old_value: old, new_value: value });
    }
    
    // ========================================================================
    // Debug Register Operations
    // ========================================================================
    
    pub fn read_dr(&self, reg: u8) -> u64 {
        let state = self.state.read().unwrap();
        match reg {
            0 => state.dr.dr0,
            1 => state.dr.dr1,
            2 => state.dr.dr2,
            3 => state.dr.dr3,
            6 => state.dr.dr6,
            7 => state.dr.dr7,
            _ => 0,
        }
    }
    
    pub fn write_dr(&self, reg: u8, value: u64) {
        let mut state = self.state.write().unwrap();
        match reg {
            0 => state.dr.dr0 = value,
            1 => state.dr.dr1 = value,
            2 => state.dr.dr2 = value,
            3 => state.dr.dr3 = value,
            6 => state.dr.dr6 = value,
            7 => state.dr.dr7 = value,
            _ => {}
        }
    }
    
    // ========================================================================
    // MSR Operations
    // ========================================================================
    
    pub fn read_msr(&self, msr_addr: u32) -> u64 {
        let state = self.state.read().unwrap();
        let value = state.msrs.get(&msr_addr).copied().unwrap_or(0);
        drop(state);
        self.record_event(CpuEvent::MsrRead { msr: msr_addr, value });
        
        // Handle special MSRs (performance counters)
        match msr_addr {
            msr::IA32_FIXED_CTR0 => self.state.read().unwrap().pmu.instructions_retired,
            msr::IA32_FIXED_CTR1 => self.state.read().unwrap().pmu.core_cycles,
            msr::IA32_FIXED_CTR2 => self.state.read().unwrap().pmu.ref_cycles,
            msr::IA32_PMC0..=msr::IA32_PMC3 => {
                let idx = (msr_addr - msr::IA32_PMC0) as usize;
                self.state.read().unwrap().pmu.pmc[idx]
            }
            _ => value,
        }
    }
    
    pub fn write_msr(&self, msr_addr: u32, value: u64) {
        let mut state = self.state.write().unwrap();
        let old = state.msrs.get(&msr_addr).copied().unwrap_or(0);
        state.msrs.insert(msr_addr, value);
        
        // Handle special MSRs
        match msr_addr {
            msr::IA32_FIXED_CTR0 => state.pmu.instructions_retired = value,
            msr::IA32_FIXED_CTR1 => state.pmu.core_cycles = value,
            msr::IA32_FIXED_CTR2 => state.pmu.ref_cycles = value,
            msr::IA32_PMC0..=msr::IA32_PMC3 => {
                let idx = (msr_addr - msr::IA32_PMC0) as usize;
                state.pmu.pmc[idx] = value;
            }
            msr::IA32_PERFEVTSEL0..=msr::IA32_PERFEVTSEL1 => {
                let idx = (msr_addr - msr::IA32_PERFEVTSEL0) as usize;
                state.pmu.perf_evtsel[idx] = value;
            }
            msr::IA32_FIXED_CTR_CTRL => state.pmu.fixed_ctr_ctrl = value,
            msr::IA32_PERF_GLOBAL_CTRL => state.pmu.perf_global_ctrl = value,
            _ => {}
        }
        drop(state);
        
        self.record_event(CpuEvent::MsrWrite { msr: msr_addr, old_value: old, new_value: value });
    }
    
    // ========================================================================
    // CPUID
    // ========================================================================
    
    pub fn cpuid(&self, leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
        let state = self.state.read().unwrap();
        let cpuid = &state.cpuid;
        
        match leaf {
            0 => {
                // Vendor string
                let vendor = &cpuid.vendor;
                (
                    cpuid.max_basic_leaf,
                    u32::from_le_bytes([vendor[0], vendor[1], vendor[2], vendor[3]]),
                    u32::from_le_bytes([vendor[8], vendor[9], vendor[10], vendor[11]]),
                    u32::from_le_bytes([vendor[4], vendor[5], vendor[6], vendor[7]]),
                )
            }
            1 => {
                // Feature information
                let family = 6u32;
                let model = 15u32;
                let stepping = 1u32;
                let signature = (family << 8) | (model << 4) | stepping;
                let logical_cpus = cpuid.logical_processors as u32;
                (
                    signature,
                    (self.id << 24) | (logical_cpus << 16) | 0x0800, // EBX: APIC ID, max logical CPUs, CLFLUSH
                    cpuid.features_ecx,
                    cpuid.features_edx,
                )
            }
            4 if subleaf <= 3 => {
                // Cache parameters (deterministic cache info)
                match subleaf {
                    0 => (0x121, 0x01C0003F, 0x0000003F, 0x00000000), // L1D
                    1 => (0x122, 0x01C0003F, 0x0000003F, 0x00000000), // L1I
                    2 => (0x143, 0x01C0003F, 0x000003FF, 0x00000000), // L2
                    3 => (0x163, 0x02C0003F, 0x00003FFF, 0x00000002), // L3
                    _ => (0, 0, 0, 0),
                }
            }
            6 => {
                // Thermal and power management
                (0x77, 0x02, 0x09, 0x00)
            }
            7 if subleaf == 0 => {
                // Structured extended features
                (0, cpuid.struct_ext_ebx, cpuid.struct_ext_ecx, cpuid.struct_ext_edx)
            }
            0xA => {
                // Architectural performance monitoring
                let version = 4u32; // Version 4
                let num_counters = 4u32;
                let counter_width = 48u32;
                let ebx_mask = 0u32;
                (
                    version | (num_counters << 8) | (counter_width << 16) | (ebx_mask << 24),
                    0, // EBX
                    0, // ECX
                    3 | (3 << 5), // EDX: 3 fixed counters, 48-bit width
                )
            }
            0xB if subleaf <= 1 => {
                // Extended topology enumeration
                match subleaf {
                    0 => (1, 2, 0x100, self.id), // Thread level
                    1 => (4, cpuid.physical_cores as u32, 0x201, self.id), // Core level
                    _ => (0, 0, 0, 0),
                }
            }
            0x15 => {
                // TSC/Core Crystal Clock info
                (1, 1, 0, 0) // 1:1 ratio (simplified)
            }
            0x16 => {
                // Processor frequency info
                (3600, 4000, 100, 0) // Base: 3.6GHz, Max: 4.0GHz, Bus: 100MHz
            }
            0x40000000 => {
                // Hypervisor vendor leaf
                let vendor = b"NexaOSVMTest";
                (
                    0x40000001, // Max hypervisor leaf
                    u32::from_le_bytes([vendor[0], vendor[1], vendor[2], vendor[3]]),
                    u32::from_le_bytes([vendor[4], vendor[5], vendor[6], vendor[7]]),
                    u32::from_le_bytes([vendor[8], vendor[9], vendor[10], vendor[11]]),
                )
            }
            0x40000001 => {
                // Hypervisor features
                (0x01, 0x00, 0x00, 0x00) // Basic features
            }
            0x80000000 => {
                // Extended CPUID max leaf
                (cpuid.max_extended_leaf, 0, 0, 0)
            }
            0x80000001 => {
                // Extended features
                (0, 0, cpuid.ext_features_ecx, cpuid.ext_features_edx)
            }
            0x80000002..=0x80000004 => {
                // Processor brand string
                let offset = ((leaf - 0x80000002) * 16) as usize;
                let brand = &cpuid.brand[offset..offset + 16];
                (
                    u32::from_le_bytes([brand[0], brand[1], brand[2], brand[3]]),
                    u32::from_le_bytes([brand[4], brand[5], brand[6], brand[7]]),
                    u32::from_le_bytes([brand[8], brand[9], brand[10], brand[11]]),
                    u32::from_le_bytes([brand[12], brand[13], brand[14], brand[15]]),
                )
            }
            0x80000006 => {
                // L2/L3 cache info
                let l2_size_kb = (cpuid.l2_cache_size / 1024) as u32;
                (0, 0, (l2_size_kb << 16) | 0x0140, 0)
            }
            0x80000007 => {
                // Advanced power management
                (0, 0, 0, 0x100) // Invariant TSC
            }
            0x80000008 => {
                // Address sizes
                (0x3028, 0, cpuid.physical_cores as u32, 0) // 48-bit virt, 40-bit phys
            }
            _ => (0, 0, 0, 0),
        }
    }
    
    // ========================================================================
    // Time Stamp Counter & Performance Monitoring
    // ========================================================================
    
    pub fn rdtsc(&self) -> u64 {
        // Advance TSC on each read to simulate time passing
        self.advance_cycles(100);
        self.tsc.load(Ordering::SeqCst)
    }
    
    pub fn rdtscp(&self) -> (u64, u32) {
        let tsc = self.rdtsc();
        let aux = self.read_msr(msr::IA32_TSC_AUX) as u32;
        (tsc, aux)
    }
    
    pub fn advance_cycles(&self, cycles: u64) {
        self.cycle_count.fetch_add(cycles, Ordering::SeqCst);
        self.tsc.fetch_add(cycles, Ordering::SeqCst);
        
        // Update performance counters
        let mut state = self.state.write().unwrap();
        if state.pmu.perf_global_ctrl != 0 {
            state.pmu.core_cycles += cycles;
            state.pmu.ref_cycles += cycles;
            // Instructions approximated as cycles/4
            state.pmu.instructions_retired += cycles / 4;
        }
    }
    
    pub fn set_tsc(&self, value: u64) {
        self.tsc.store(value, Ordering::SeqCst);
    }
    
    /// Get total cycles executed
    pub fn get_cycle_count(&self) -> u64 {
        self.cycle_count.load(Ordering::SeqCst)
    }
    
    /// Get performance counter value
    pub fn get_pmc(&self, index: usize) -> u64 {
        self.state.read().unwrap().pmu.pmc.get(index).copied().unwrap_or(0)
    }
    
    /// Get instructions retired counter
    pub fn get_instructions_retired(&self) -> u64 {
        self.state.read().unwrap().pmu.instructions_retired
    }
    
    // ========================================================================
    // Interrupt State & Exception Handling
    // ========================================================================
    
    pub fn interrupts_enabled(&self) -> bool {
        self.state.read().unwrap().interrupts_enabled
    }
    
    pub fn enable_interrupts(&self) {
        let mut state = self.state.write().unwrap();
        state.interrupts_enabled = true;
        state.regs.rflags |= rflags::IF;
    }
    
    pub fn disable_interrupts(&self) {
        let mut state = self.state.write().unwrap();
        state.interrupts_enabled = false;
        state.regs.rflags &= !rflags::IF;
    }
    
    /// Inject an external interrupt
    pub fn inject_interrupt(&self, vector: u8) {
        let mut state = self.state.write().unwrap();
        state.pending_interrupts.push_back(PendingInterrupt {
            vector,
            error_code: None,
            is_nmi: false,
            is_external: true,
        });
    }
    
    /// Inject NMI
    pub fn inject_nmi(&self) {
        self.state.write().unwrap().nmi_pending = true;
    }
    
    /// Inject an exception
    pub fn inject_exception(&self, exception: CpuException, error_code: Option<u32>) {
        let mut state = self.state.write().unwrap();
        state.pending_exception = Some((exception, error_code));
        self.record_event(CpuEvent::Exception { vector: exception, error_code });
    }
    
    /// Check for pending interrupts that can be delivered
    pub fn has_pending_interrupt(&self) -> bool {
        let state = self.state.read().unwrap();
        if state.nmi_pending {
            return true;
        }
        if state.interrupts_enabled && !state.pending_interrupts.is_empty() {
            let tpr = state.cr.cr8 << 4;
            if let Some(intr) = state.pending_interrupts.front() {
                return (intr.vector as u64) > tpr;
            }
        }
        false
    }
    
    /// Deliver pending interrupt (returns vector if delivered)
    pub fn deliver_interrupt(&self) -> Option<u8> {
        let mut state = self.state.write().unwrap();
        
        // NMI has highest priority
        if state.nmi_pending {
            state.nmi_pending = false;
            drop(state);
            self.record_event(CpuEvent::InterruptDelivered { vector: 2 });
            return Some(2);
        }
        
        // Check if interrupts are enabled
        if !state.interrupts_enabled {
            return None;
        }
        
        // Check TPR
        let tpr = state.cr.cr8 << 4;
        if let Some(intr) = state.pending_interrupts.front() {
            if (intr.vector as u64) > tpr {
                let vector = intr.vector;
                state.pending_interrupts.pop_front();
                drop(state);
                self.record_event(CpuEvent::InterruptDelivered { vector });
                return Some(vector);
            }
        }
        
        None
    }
    
    pub fn is_halted(&self) -> bool {
        self.state.read().unwrap().halted
    }
    
    pub fn halt(&self) {
        self.state.write().unwrap().halted = true;
        self.record_event(CpuEvent::Halted);
    }
    
    pub fn wake(&self) {
        self.state.write().unwrap().halted = false;
        self.record_event(CpuEvent::Woken);
    }
    
    // ========================================================================
    // Privilege Level
    // ========================================================================
    
    pub fn get_cpl(&self) -> u8 {
        self.state.read().unwrap().cpl
    }
    
    pub fn set_cpl(&self, cpl: u8) {
        let mut state = self.state.write().unwrap();
        let old_cpl = state.cpl;
        state.cpl = cpl;
        drop(state);
        
        if old_cpl != cpl {
            self.record_event(CpuEvent::PrivilegeChange { from_cpl: old_cpl, to_cpl: cpl });
        }
    }
    
    // ========================================================================
    // Register Operations
    // ========================================================================
    
    pub fn read_rsp(&self) -> u64 {
        self.state.read().unwrap().regs.rsp
    }
    
    pub fn write_rsp(&self, value: u64) {
        self.state.write().unwrap().regs.rsp = value;
    }
    
    pub fn read_rip(&self) -> u64 {
        self.state.read().unwrap().regs.rip
    }
    
    pub fn write_rip(&self, value: u64) {
        self.state.write().unwrap().regs.rip = value;
    }
    
    pub fn read_rflags(&self) -> u64 {
        self.state.read().unwrap().regs.rflags
    }
    
    pub fn write_rflags(&self, value: u64) {
        let mut state = self.state.write().unwrap();
        state.regs.rflags = value;
        // Sync interrupts_enabled with IF flag (bit 9)
        state.interrupts_enabled = (value & rflags::IF) != 0;
    }
    
    /// Read a general purpose register by index (0=RAX, 1=RCX, 2=RDX, etc.)
    pub fn read_gpr(&self, reg: u8) -> u64 {
        let state = self.state.read().unwrap();
        match reg {
            0 => state.regs.rax,
            1 => state.regs.rcx,
            2 => state.regs.rdx,
            3 => state.regs.rbx,
            4 => state.regs.rsp,
            5 => state.regs.rbp,
            6 => state.regs.rsi,
            7 => state.regs.rdi,
            8 => state.regs.r8,
            9 => state.regs.r9,
            10 => state.regs.r10,
            11 => state.regs.r11,
            12 => state.regs.r12,
            13 => state.regs.r13,
            14 => state.regs.r14,
            15 => state.regs.r15,
            _ => 0,
        }
    }
    
    /// Write a general purpose register by index
    pub fn write_gpr(&self, reg: u8, value: u64) {
        let mut state = self.state.write().unwrap();
        match reg {
            0 => state.regs.rax = value,
            1 => state.regs.rcx = value,
            2 => state.regs.rdx = value,
            3 => state.regs.rbx = value,
            4 => state.regs.rsp = value,
            5 => state.regs.rbp = value,
            6 => state.regs.rsi = value,
            7 => state.regs.rdi = value,
            8 => state.regs.r8 = value,
            9 => state.regs.r9 = value,
            10 => state.regs.r10 = value,
            11 => state.regs.r11 = value,
            12 => state.regs.r12 = value,
            13 => state.regs.r13 = value,
            14 => state.regs.r14 = value,
            15 => state.regs.r15 = value,
            _ => {}
        }
    }
    
    // ========================================================================
    // Full State Access (for debugging/assertions)
    // ========================================================================
    
    pub fn get_state(&self) -> CpuState {
        self.state.read().unwrap().clone()
    }
    
    pub fn set_state(&self, state: CpuState) {
        *self.state.write().unwrap() = state;
    }
    
    /// Get registers snapshot
    pub fn get_registers(&self) -> Registers {
        self.state.read().unwrap().regs.clone()
    }
    
    /// Set registers
    pub fn set_registers(&self, regs: Registers) {
        self.state.write().unwrap().regs = regs;
    }
}

impl Default for VirtualCpu {
    fn default() -> Self {
        Self::new_bsp()
    }
}

// ============================================================================
// CPU Pool (for SMP testing)
// ============================================================================

/// Pool of virtual CPUs for SMP testing
pub struct CpuPool {
    cpus: RwLock<Vec<Arc<VirtualCpu>>>,
    active_cpu: AtomicU64,
}

impl CpuPool {
    /// Create a new CPU pool with specified number of CPUs
    pub fn new(count: usize) -> Self {
        let mut cpus = Vec::with_capacity(count);
        cpus.push(Arc::new(VirtualCpu::new_bsp()));
        for i in 1..count {
            cpus.push(Arc::new(VirtualCpu::new_ap(i as u32)));
        }
        
        Self {
            cpus: RwLock::new(cpus),
            active_cpu: AtomicU64::new(0),
        }
    }
    
    /// Get CPU by ID
    pub fn get(&self, id: u32) -> Option<Arc<VirtualCpu>> {
        self.cpus.read().unwrap().get(id as usize).cloned()
    }
    
    /// Get current CPU
    pub fn current(&self) -> Arc<VirtualCpu> {
        let id = self.active_cpu.load(Ordering::SeqCst) as usize;
        self.cpus.read().unwrap()[id].clone()
    }
    
    /// Switch to a different CPU
    pub fn switch_to(&self, id: u32) {
        self.active_cpu.store(id as u64, Ordering::SeqCst);
    }
    
    /// Get number of CPUs
    pub fn count(&self) -> usize {
        self.cpus.read().unwrap().len()
    }
    
    /// Create a snapshot of all CPUs
    pub fn snapshot_all(&self) -> Vec<CpuStateSnapshot> {
        self.cpus.read().unwrap().iter().map(|c| c.snapshot()).collect()
    }
    
    /// Restore all CPUs from snapshots
    pub fn restore_all(&self, snapshots: &[CpuStateSnapshot]) {
        let cpus = self.cpus.read().unwrap();
        for (cpu, snapshot) in cpus.iter().zip(snapshots.iter()) {
            cpu.restore(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vcpu_basic() {
        let vcpu = VirtualCpu::new_bsp();
        assert_eq!(vcpu.id, 0);
        assert!(!vcpu.is_halted());
    }
    
    #[test]
    fn test_vcpu_cr3() {
        let vcpu = VirtualCpu::new_bsp();
        vcpu.write_cr3(0x1000);
        assert_eq!(vcpu.read_cr3(), 0x1000);
    }
    
    #[test]
    fn test_vcpu_msr() {
        let vcpu = VirtualCpu::new_bsp();
        vcpu.write_msr(msr::IA32_LSTAR, 0xFFFF_8000_0000_1000);
        assert_eq!(vcpu.read_msr(msr::IA32_LSTAR), 0xFFFF_8000_0000_1000);
    }
    
    #[test]
    fn test_vcpu_cpuid_vendor() {
        let vcpu = VirtualCpu::new_bsp();
        let (eax, _ebx, _ecx, _edx) = vcpu.cpuid(0, 0);
        assert!(eax >= 0x16); // Max basic leaf
    }
    
    #[test]
    fn test_vcpu_cpuid_hypervisor() {
        let vcpu = VirtualCpu::new_bsp();
        let (_, _, ecx, _) = vcpu.cpuid(1, 0);
        assert!(ecx & cpuid_ecx::HYPERVISOR != 0); // Hypervisor bit set
    }
    
    #[test]
    fn test_vcpu_tsc_advances() {
        let vcpu = VirtualCpu::new_bsp();
        let tsc1 = vcpu.rdtsc();
        let tsc2 = vcpu.rdtsc();
        assert!(tsc2 > tsc1);
    }
    
    #[test]
    fn test_vcpu_interrupts() {
        let vcpu = VirtualCpu::new_bsp();
        assert!(!vcpu.interrupts_enabled());
        vcpu.enable_interrupts();
        assert!(vcpu.interrupts_enabled());
        vcpu.disable_interrupts();
        assert!(!vcpu.interrupts_enabled());
    }
    
    #[test]
    fn test_vcpu_breakpoints() {
        let vcpu = VirtualCpu::new_bsp();
        
        // Add a breakpoint
        assert!(vcpu.add_breakpoint(0x1000));
        assert!(!vcpu.add_breakpoint(0x1000)); // Duplicate
        
        // Check breakpoint
        let bp = vcpu.check_breakpoint(0x1000);
        assert_eq!(bp, Some(BreakpointType::Execution));
        
        // Remove breakpoint
        assert!(vcpu.remove_breakpoint(0x1000));
        assert!(vcpu.check_breakpoint(0x1000).is_none());
    }
    
    #[test]
    fn test_vcpu_snapshot_restore() {
        let vcpu = VirtualCpu::new_bsp();
        
        // Modify state
        vcpu.write_cr3(0x2000);
        vcpu.write_msr(msr::IA32_LSTAR, 0x1234);
        vcpu.enable_interrupts();
        
        // Take snapshot
        let snapshot = vcpu.snapshot();
        
        // Modify more
        vcpu.write_cr3(0x3000);
        vcpu.disable_interrupts();
        
        // Restore
        vcpu.restore(&snapshot);
        
        // Verify
        assert_eq!(vcpu.read_cr3(), 0x2000);
        assert!(vcpu.interrupts_enabled());
    }
    
    #[test]
    fn test_vcpu_interrupt_injection() {
        let vcpu = VirtualCpu::new_bsp();
        
        // Inject interrupt
        vcpu.inject_interrupt(0x30);
        
        // Check queue is not empty (has_pending only returns true if can deliver)
        let state = vcpu.get_state();
        assert!(!state.pending_interrupts.is_empty()); // Queued
        
        // Can't deliver with interrupts disabled
        assert!(vcpu.deliver_interrupt().is_none()); // Can't deliver
        
        // Enable interrupts
        vcpu.enable_interrupts();
        
        // Now has_pending returns true
        assert!(vcpu.has_pending_interrupt());
        
        // Now can deliver
        let vector = vcpu.deliver_interrupt();
        assert_eq!(vector, Some(0x30));
    }
    
    #[test]
    fn test_vcpu_nmi() {
        let vcpu = VirtualCpu::new_bsp();
        
        // NMI can be delivered even with interrupts disabled
        vcpu.inject_nmi();
        assert!(vcpu.has_pending_interrupt());
        
        let vector = vcpu.deliver_interrupt();
        assert_eq!(vector, Some(2)); // NMI = vector 2
    }
    
    #[test]
    fn test_vcpu_events() {
        let vcpu = VirtualCpu::new_bsp();
        vcpu.clear_events();
        
        // Generate some events
        vcpu.write_cr3(0x1000);
        vcpu.write_msr(msr::IA32_LSTAR, 0x2000);
        
        // Check events were recorded
        let events = vcpu.get_events(10);
        assert!(events.len() >= 2);
    }
    
    #[test]
    fn test_cpu_pool() {
        let pool = CpuPool::new(4);
        
        assert_eq!(pool.count(), 4);
        
        // BSP should not be halted
        let bsp = pool.get(0).unwrap();
        assert!(!bsp.is_halted());
        
        // APs should be halted
        let ap1 = pool.get(1).unwrap();
        assert!(ap1.is_halted());
        
        // Switch and verify
        pool.switch_to(1);
        assert_eq!(pool.current().id, 1);
    }
    
    #[test]
    fn test_performance_counters() {
        let vcpu = VirtualCpu::new_bsp();
        
        // Enable performance monitoring
        vcpu.write_msr(msr::IA32_PERF_GLOBAL_CTRL, 0x7); // Enable fixed counters
        
        // Advance cycles
        vcpu.advance_cycles(1000);
        
        // Check counters increased
        assert!(vcpu.state.read().unwrap().pmu.core_cycles >= 1000);
    }
}
