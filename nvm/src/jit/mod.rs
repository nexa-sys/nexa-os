//! x86-64 JIT Execution Engine
//!
//! Enterprise-grade JIT compiler for x86-64 guest code execution.
//! Provides tiered compilation similar to JVM's HotSpot:
//!
//! ## Execution Tiers
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                     NVM x86-64 JIT Execution Engine                         │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  ┌───────────────┐    ┌───────────────┐    ┌───────────────┐               │
//! │  │  Interpreter  │───▶│    S1 JIT     │───▶│    S2 JIT     │               │
//! │  │  (Cold Start) │    │ (Quick Comp)  │    │ (Optimizing)  │               │
//! │  │               │    │               │    │               │               │
//! │  │ • Zero warmup │    │ • Fast compile│    │ • Full opts   │               │
//! │  │ • Full compat │    │ • Basic opts  │    │ • Inlining    │               │
//! │  │ • Profile col │    │ • Type specs  │    │ • Loop opts   │               │
//! │  └───────────────┘    └───────────────┘    └───────────────┘               │
//! │         │                    │                    │                        │
//! │         ▼                    ▼                    ▼                        │
//! │  ┌─────────────────────────────────────────────────────────────────────┐  │
//! │  │                      Profile Database                                │  │
//! │  │  • Branch statistics  • Call targets  • Memory patterns             │  │
//! │  └─────────────────────────────────────────────────────────────────────┘  │
//! │                                                                             │
//! │  ┌─────────────────────────────────────────────────────────────────────┐  │
//! │  │                        NReady! Cache                                 │  │
//! │  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                 │  │
//! │  │  │   Profile    │ │      RI      │ │  Native Code │                 │  │
//! │  │  │ (Full Compat)│ │(Back Compat) │ │(Gen Compat)  │                 │  │
//! │  │  └──────────────┘ └──────────────┘ └──────────────┘                 │  │
//! │  └─────────────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Compilation Pipeline
//!
//! ```text
//! x86 Guest Code
//!       │
//!       ▼
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │   Decoder   │────▶│     RI      │────▶│  Optimizer  │────▶│  CodeGen    │
//! │  (x86→RI)   │     │ (SSA Form)  │     │  (S1/S2)    │     │ (RI→Native) │
//! └─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
//!                            │                                       │
//!                            ▼                                       ▼
//!                     ┌─────────────┐                         ┌─────────────┐
//!                     │  RI Cache   │                         │ Code Cache  │
//!                     │ (Persist)   │                         │ (Persist)   │
//!                     └─────────────┘                         └─────────────┘
//! ```

pub mod decoder;
pub mod ir;
pub mod interpreter;
pub mod compiler_s1;
pub mod compiler_s2;
pub mod codegen;
pub mod profile;
pub mod cache;
pub mod nready;
pub mod eviction;
pub mod async_runtime;
pub mod async_eviction;
pub mod async_restore;

use std::sync::{Arc, RwLock, atomic::{AtomicU64, AtomicU8, AtomicBool, Ordering}};
use std::collections::{HashMap, HashSet};

use crate::cpu::VirtualCpu;
use crate::memory::{PhysicalMemory, AddressSpace};

pub use decoder::{X86Decoder, DecodedInstr};
pub use ir::{IrBuilder, IrBlock, IrInstr, IrOp};
pub use interpreter::Interpreter;
pub use compiler_s1::S1Compiler;
pub use compiler_s2::{S2Compiler, S2Config};
pub use codegen::CodeGen;
pub use profile::{ProfileDb, BranchProfile, CallProfile, BlockProfile};
pub use cache::{CodeCache, CacheStats, CompiledBlock, CompileTier, CacheError, BlockPersistInfo, SmartEvictResult, EvictionCandidateInfo};
pub use nready::{NReadyCache, NativeBlockInfo, EvictableBlock, RestoredBlock};
pub use eviction::{HotnessTracker, HotnessEntry, EvictedBlockInfo, EvictionCandidate, HotnessSnapshot};
pub use async_runtime::{AsyncJitRuntime, CompileRequest, CompileResult, CompilePriority, CompileCallback, AsyncStatsSnapshot, CompilerContext, CodeCacheInstaller};
pub use async_eviction::{AsyncEvictionManager, EvictionState, EvictionStats, EvictionStatsSnapshot};
pub use async_restore::{AsyncRestoreManager, RestoreRequest, RestoreResult, RestorePriority, RestoreCallback, RestoreStatsSnapshot, PrefetchAnalyzer};

// ============================================================================
// JIT CPU State - Direct access structure for native code
// ============================================================================

/// JIT-accessible CPU state with known memory layout
/// 
/// Native JIT code accesses this structure directly via pointer arithmetic.
/// The layout MUST be stable and `#[repr(C)]` ensures C ABI compatibility.
///
/// ## Memory Layout (offsets from base pointer in RDI):
/// ```text
/// Offset  Size  Field
/// 0x000   8     rax
/// 0x008   8     rcx
/// 0x010   8     rdx
/// 0x018   8     rbx
/// 0x020   8     rsp
/// 0x028   8     rbp
/// 0x030   8     rsi
/// 0x038   8     rdi
/// 0x040   8     r8
/// 0x048   8     r9
/// 0x050   8     r10
/// 0x058   8     r11
/// 0x060   8     r12
/// 0x068   8     r13
/// 0x070   8     r14
/// 0x078   8     r15
/// 0x080   8     rip
/// 0x088   8     rflags
/// 0x090   8*    memory_base (pointer to guest physical memory)
/// ```
#[repr(C)]
#[derive(Debug, Clone)]
pub struct JitState {
    // General purpose registers (in x86-64 order)
    pub rax: u64,   // 0x00
    pub rcx: u64,   // 0x08
    pub rdx: u64,   // 0x10
    pub rbx: u64,   // 0x18
    pub rsp: u64,   // 0x20
    pub rbp: u64,   // 0x28
    pub rsi: u64,   // 0x30
    pub rdi: u64,   // 0x38
    pub r8: u64,    // 0x40
    pub r9: u64,    // 0x48
    pub r10: u64,   // 0x50
    pub r11: u64,   // 0x58
    pub r12: u64,   // 0x60
    pub r13: u64,   // 0x68
    pub r14: u64,   // 0x70
    pub r15: u64,   // 0x78
    pub rip: u64,   // 0x80
    pub rflags: u64, // 0x88
    pub memory_base: *mut u8, // 0x90
}

impl JitState {
    /// GPR offset for index 0-15
    pub const fn gpr_offset(idx: u8) -> i32 {
        (idx as i32) * 8
    }
    
    pub const RIP_OFFSET: i32 = 0x80;
    pub const RFLAGS_OFFSET: i32 = 0x88;
    pub const MEMORY_BASE_OFFSET: i32 = 0x90;  
    /// Create new JitState initialized to zero
    pub fn new() -> Self {
        Self {
            rax: 0, rcx: 0, rdx: 0, rbx: 0,
            rsp: 0, rbp: 0, rsi: 0, rdi: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            rip: 0, rflags: 0x2, // Bit 1 always set
            memory_base: std::ptr::null_mut(),
        }
    }
    
    /// Copy state from VirtualCpu
    pub fn from_vcpu(cpu: &VirtualCpu, memory: &AddressSpace) -> Self {
        let state = cpu.state();
        Self {
            rax: state.regs.rax,
            rcx: state.regs.rcx,
            rdx: state.regs.rdx,
            rbx: state.regs.rbx,
            rsp: state.regs.rsp,
            rbp: state.regs.rbp,
            rsi: state.regs.rsi,
            rdi: state.regs.rdi,
            r8: state.regs.r8,
            r9: state.regs.r9,
            r10: state.regs.r10,
            r11: state.regs.r11,
            r12: state.regs.r12,
            r13: state.regs.r13,
            r14: state.regs.r14,
            r15: state.regs.r15,
            rip: state.regs.rip,
            rflags: state.regs.rflags,
            memory_base: memory.ram_ptr() as *mut u8,
        }
    }
    
    /// Copy state back to VirtualCpu
    pub fn to_vcpu(&self, cpu: &VirtualCpu) {
        cpu.write_gpr(0, self.rax);
        cpu.write_gpr(1, self.rcx);
        cpu.write_gpr(2, self.rdx);
        cpu.write_gpr(3, self.rbx);
        cpu.write_gpr(4, self.rsp);
        cpu.write_gpr(5, self.rbp);
        cpu.write_gpr(6, self.rsi);
        cpu.write_gpr(7, self.rdi);
        cpu.write_gpr(8, self.r8);
        cpu.write_gpr(9, self.r9);
        cpu.write_gpr(10, self.r10);
        cpu.write_gpr(11, self.r11);
        cpu.write_gpr(12, self.r12);
        cpu.write_gpr(13, self.r13);
        cpu.write_gpr(14, self.r14);
        cpu.write_gpr(15, self.r15);
        cpu.write_rip(self.rip);
        cpu.write_rflags(self.rflags);
    }
    
    /// Read GPR by index
    pub fn read_gpr(&self, idx: u8) -> u64 {
        match idx {
            0 => self.rax,
            1 => self.rcx,
            2 => self.rdx,
            3 => self.rbx,
            4 => self.rsp,
            5 => self.rbp,
            6 => self.rsi,
            7 => self.rdi,
            8 => self.r8,
            9 => self.r9,
            10 => self.r10,
            11 => self.r11,
            12 => self.r12,
            13 => self.r13,
            14 => self.r14,
            15 => self.r15,
            _ => 0,
        }
    }
    
    /// Write GPR by index
    pub fn write_gpr(&mut self, idx: u8, value: u64) {
        match idx {
            0 => self.rax = value,
            1 => self.rcx = value,
            2 => self.rdx = value,
            3 => self.rbx = value,
            4 => self.rsp = value,
            5 => self.rbp = value,
            6 => self.rsi = value,
            7 => self.rdi = value,
            8 => self.r8 = value,
            9 => self.r9 = value,
            10 => self.r10 = value,
            11 => self.r11 = value,
            12 => self.r12 = value,
            13 => self.r13 = value,
            14 => self.r14 = value,
            15 => self.r15 = value,
            _ => {}
        }
    }
}

impl Default for JitState {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: JitState is plain data, safe to send across threads
unsafe impl Send for JitState {}
unsafe impl Sync for JitState {}


/// JIT execution result
pub type JitResult<T> = Result<T, JitError>;

/// JIT errors
#[derive(Debug, Clone)]
pub enum JitError {
    /// Decode error
    DecodeError { rip: u64, bytes: Vec<u8>, reason: String },
    /// Invalid opcode
    InvalidOpcode { rip: u64, opcode: u8 },
    /// Unsupported instruction
    UnsupportedInstruction { rip: u64, mnemonic: String },
    /// Memory access error
    MemoryError { addr: u64, size: usize, write: bool },
    /// Code cache full
    CodeCacheFull,
    /// Compilation error
    CompilationError(String),
    /// Deoptimization needed
    DeoptNeeded { reason: String, rip: u64 },
    /// Cache miss
    CacheMiss { rip: u64 },
    /// Profile mismatch
    ProfileMismatch,
    /// Unresolved label in code generation
    UnresolvedLabel,
    /// Invalid relocation
    InvalidRelocation,
    /// Unallocated register
    UnallocatedRegister,
    /// IO error
    IoError,
    /// Invalid format
    InvalidFormat,
    /// Incompatible version
    IncompatibleVersion,
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DecodeError { rip, reason, .. } => 
                write!(f, "Decode error at 0x{:x}: {}", rip, reason),
            Self::InvalidOpcode { rip, opcode } =>
                write!(f, "Invalid opcode 0x{:02x} at 0x{:x}", opcode, rip),
            Self::UnsupportedInstruction { rip, mnemonic } =>
                write!(f, "Unsupported instruction '{}' at 0x{:x}", mnemonic, rip),
            Self::MemoryError { addr, size, write } =>
                write!(f, "Memory {} error: {} bytes at 0x{:x}", 
                       if *write { "write" } else { "read" }, size, addr),
            Self::CodeCacheFull => write!(f, "Code cache full"),
            Self::CompilationError(msg) => write!(f, "Compilation error: {}", msg),
            Self::DeoptNeeded { reason, rip } =>
                write!(f, "Deoptimization needed at 0x{:x}: {}", rip, reason),
            Self::CacheMiss { rip } => write!(f, "Cache miss at 0x{:x}", rip),
            Self::ProfileMismatch => write!(f, "Profile mismatch"),
            Self::UnresolvedLabel => write!(f, "Unresolved label in code generation"),
            Self::InvalidRelocation => write!(f, "Invalid relocation type"),
            Self::UnallocatedRegister => write!(f, "Unallocated virtual register"),
            Self::IoError => write!(f, "IO error"),
            Self::InvalidFormat => write!(f, "Invalid cache format"),
            Self::IncompatibleVersion => write!(f, "Incompatible cache version"),
        }
    }
}

impl std::error::Error for JitError {}

impl From<CacheError> for JitError {
    fn from(e: CacheError) -> Self {
        match e {
            CacheError::OutOfMemory => JitError::CodeCacheFull,
            CacheError::InvalidBlock => JitError::CompilationError("Invalid block".to_string()),
            CacheError::CompilationFailed => JitError::CompilationError("Compilation failed".to_string()),
        }
    }
}

/// Execution tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionTier {
    /// Pure interpretation (slowest, zero warmup)
    Interpreter,
    /// S1 JIT - Quick compilation, basic optimizations
    S1,
    /// S2 JIT - Full optimization (inlining, loop opts, etc)
    S2,
}

impl ExecutionTier {
    /// Convert to u8 for atomic storage
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Interpreter => 0,
            Self::S1 => 1,
            Self::S2 => 2,
        }
    }
    
    /// Convert from u8
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::S1,
            2 => Self::S2,
            _ => Self::Interpreter,
        }
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            Self::Interpreter => "Interpreter",
            Self::S1 => "S1 (Quick)",
            Self::S2 => "S2 (Optimizing)",
        }
    }
}

/// NReady! persistence format
/// 
/// Three formats with different compatibility guarantees:
/// - Profile: Full forward AND backward compatibility (safest)
/// - RI: Backward compatible (old cache works on new JIT)
/// - Native: Same-generation only (fastest but version-locked)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistFormat {
    /// Profile data only - branch stats, call targets, hot blocks
    /// Full bidirectional compatibility guaranteed
    Profile,
    /// Runtime Intermediate representation (SSA IR)
    /// Backward compatible: old RI works on new JIT versions
    Ri,
    /// Pre-compiled native machine code
    /// Same-generation only: version must match exactly
    Native,
    /// All formats (Profile + RI + Native)
    All,
}

/// Tier promotion thresholds (ZingJDK-inspired defaults)
/// 
/// Reference: Azul Zing JDK defaults for tiered compilation:
/// - CompileThreshold: 100 (interpreter → baseline)
/// - Tier2CompileThreshold: 500 (for profiling tier)
/// - Tier3CompileThreshold: 2000 (baseline → optimizing)
/// - Tier4CompileThreshold: 15000 (full optimization)
#[derive(Debug, Clone)]
pub struct TierThresholds {
    /// Invocations before promoting Interpreter → S1 (baseline compilation)
    /// ZingJDK default: 100
    pub interpreter_to_s1: u64,
    /// Invocations before promoting S1 → S2 (optimizing compilation)
    /// ZingJDK default: 2000-5000 depending on method size
    pub s1_to_s2: u64,
    /// Back-edge count threshold for OSR (on-stack replacement)
    pub osr_threshold: u64,
    /// Minimum block size for S2 compilation (bytes)
    pub s2_min_block_size: usize,
}

impl Default for TierThresholds {
    fn default() -> Self {
        Self {
            interpreter_to_s1: 100,      // ZingJDK: CompileThreshold=100
            s1_to_s2: 2000,              // ZingJDK: Tier3CompileThreshold=2000
            osr_threshold: 5_000,        // OSR for hot loops
            s2_min_block_size: 64,       // Don't S2 tiny blocks
        }
    }
}

/// JIT configuration with ZingJDK-inspired defaults
#[derive(Debug, Clone)]
pub struct JitConfig {
    /// Enable tiered compilation
    pub tiered_compilation: bool,
    /// Tier promotion thresholds
    pub thresholds: TierThresholds,
    /// Initial code cache size (bytes)
    pub code_cache_initial_size: usize,
    /// Maximum code cache size (bytes) - cache grows dynamically up to this limit
    pub code_cache_max_size: usize,
    /// Code cache growth factor when expanding (1.5 = 50% growth)
    pub code_cache_growth_factor: f64,
    /// Profile database size (entries)
    pub profile_db_size: usize,
    /// Enable NReady! preloading (default: true)
    pub nready_enabled: bool,
    /// NReady! cache path (default: ~/.nvm/nready/)
    pub nready_path: Option<String>,
    /// Auto-save NReady! on VM shutdown
    pub nready_auto_save: bool,
    /// Periodic auto-save interval in seconds (0 = disabled, default: 60)
    pub nready_save_interval_secs: u64,
    /// Enable aggressive inlining in S2
    pub aggressive_inlining: bool,
    /// Enable loop unrolling
    pub loop_unrolling: bool,
    /// Max inline depth
    pub max_inline_depth: u32,
    /// Enable speculative optimizations
    pub speculative_opts: bool,
    /// Enable async (Zero-STW) compilation, eviction, and restoration
    pub async_compilation: bool,
}

/// Get system memory size in bytes
fn get_system_memory() -> usize {
    // Try to read from /proc/meminfo on Linux
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<usize>() {
                        return kb * 1024; // Convert KB to bytes
                    }
                }
            }
        }
    }
    // Fallback: assume 16GB if can't detect
    16 * 1024 * 1024 * 1024
}

impl Default for JitConfig {
    fn default() -> Self {
        // Default NReady! path: ~/.local/share/nvm/nready/
        let nready_path = dirs::data_local_dir()
            .or_else(|| dirs::home_dir().map(|p| p.join(".local").join("share")))
            .map(|p| p.join("nvm").join("nready").to_string_lossy().to_string());
        
        // CodeCache size based on system memory
        // Initial: 20% of system memory, Max: 30% of system memory
        let sys_mem = get_system_memory();
        let initial_size = sys_mem / 5;  // 20%
        let max_size = sys_mem * 3 / 10; // 30%
        
        // Apply reasonable bounds (min 64MB, max 8GB)
        let initial_size = initial_size.clamp(64 * 1024 * 1024, 8 * 1024 * 1024 * 1024);
        let max_size = max_size.clamp(128 * 1024 * 1024, 8 * 1024 * 1024 * 1024);
        
        Self {
            tiered_compilation: true,
            thresholds: TierThresholds::default(),
            code_cache_initial_size: initial_size,
            code_cache_max_size: max_size,
            code_cache_growth_factor: 1.5,              // Grow by 50% each expansion
            profile_db_size: 1_000_000,                 // 1M profile entries
            nready_enabled: true,                       // NReady! ON by default
            nready_path,                                // ~/.local/share/nvm/nready/
            nready_auto_save: true,                     // Auto-save on shutdown
            nready_save_interval_secs: 60,              // Periodic save every 60 seconds
            aggressive_inlining: true,
            loop_unrolling: true,
            max_inline_depth: 9,
            speculative_opts: true,
            async_compilation: true,                    // Zero-STW async JIT enabled by default
        }
    }
}

/// Block metadata (tracks execution statistics)
#[derive(Debug)]
pub struct BlockMeta {
    /// Guest RIP of block start
    pub guest_rip: u64,
    /// Block size in guest bytes
    pub guest_size: usize,
    /// Current execution tier (stored as AtomicU8 for lock-free updates)
    tier: AtomicU8,
    /// Invocation count
    pub invocations: AtomicU64,
    /// Back-edge count (for loop detection)
    pub back_edges: AtomicU64,
    /// Native code pointer (if compiled)
    pub native_code: Option<*const u8>,
    /// Native code size
    pub native_size: usize,
    /// IR representation (for recompilation)
    pub ir: Option<Arc<IrBlock>>,
    /// Is this block valid?
    pub valid: AtomicBool,
    /// Timestamp of last execution
    pub last_exec: AtomicU64,
}

impl Clone for BlockMeta {
    fn clone(&self) -> Self {
        Self {
            guest_rip: self.guest_rip,
            guest_size: self.guest_size,
            tier: AtomicU8::new(self.tier.load(Ordering::Relaxed)),
            invocations: AtomicU64::new(self.invocations.load(Ordering::Relaxed)),
            back_edges: AtomicU64::new(self.back_edges.load(Ordering::Relaxed)),
            native_code: self.native_code,
            native_size: self.native_size,
            ir: self.ir.clone(),
            valid: AtomicBool::new(self.valid.load(Ordering::Relaxed)),
            last_exec: AtomicU64::new(self.last_exec.load(Ordering::Relaxed)),
        }
    }
}

// Safety: BlockMeta is Send+Sync because native_code is only dereferenced
// within the JIT engine's controlled execution context
unsafe impl Send for BlockMeta {}
unsafe impl Sync for BlockMeta {}

impl BlockMeta {
    pub fn new(guest_rip: u64) -> Self {
        Self {
            guest_rip,
            guest_size: 0,
            tier: AtomicU8::new(ExecutionTier::Interpreter.as_u8()),
            invocations: AtomicU64::new(0),
            back_edges: AtomicU64::new(0),
            native_code: None,
            native_size: 0,
            ir: None,
            valid: AtomicBool::new(true),
            last_exec: AtomicU64::new(0),
        }
    }
    
    /// Get current execution tier
    pub fn get_tier(&self) -> ExecutionTier {
        ExecutionTier::from_u8(self.tier.load(Ordering::Acquire))
    }
    
    /// Set execution tier (only upgrades, never downgrades)
    pub fn set_tier(&self, tier: ExecutionTier) {
        let new_val = tier.as_u8();
        let mut current = self.tier.load(Ordering::Relaxed);
        // Only upgrade, never downgrade
        while new_val > current {
            match self.tier.compare_exchange_weak(
                current, new_val, Ordering::Release, Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }
    
    pub fn increment_invocations(&self) -> u64 {
        self.invocations.fetch_add(1, Ordering::Relaxed)
    }
    
    pub fn increment_back_edges(&self) -> u64 {
        self.back_edges.fetch_add(1, Ordering::Relaxed)
    }
}

/// Execution statistics
#[derive(Debug, Default)]
pub struct JitStats {
    /// Total instructions executed
    pub instructions_executed: AtomicU64,
    /// Interpreter executions
    pub interpreter_execs: AtomicU64,
    /// S1 executions
    pub s1_execs: AtomicU64,
    /// S2 executions
    pub s2_execs: AtomicU64,
    /// S1 compilations
    pub s1_compilations: AtomicU64,
    /// S2 compilations
    pub s2_compilations: AtomicU64,
    /// Deoptimizations
    pub deoptimizations: AtomicU64,
    /// Cache hits
    pub cache_hits: AtomicU64,
    /// Cache misses
    pub cache_misses: AtomicU64,
    /// NReady! loads
    pub nready_loads: AtomicU64,
    /// Total compilation time (ns)
    pub compilation_time_ns: AtomicU64,
    /// Smart evictions triggered
    pub smart_evictions: AtomicU64,
    /// Blocks evicted to disk
    pub blocks_evicted_to_disk: AtomicU64,
    /// Blocks restored from disk
    pub blocks_restored: AtomicU64,
}

/// The main JIT execution engine
pub struct JitEngine {
    /// Configuration
    config: JitConfig,
    /// x86 decoder
    decoder: X86Decoder,
    /// Interpreter
    interpreter: Interpreter,
    /// S1 compiler
    s1_compiler: S1Compiler,
    /// S2 compiler
    s2_compiler: S2Compiler,
    /// Native code generator
    codegen: CodeGen,
    /// Code cache (guest RIP → compiled code)
    code_cache: Arc<CodeCache>,
    /// Block metadata
    blocks: RwLock<HashMap<u64, Arc<BlockMeta>>>,
    /// Profile database
    profile_db: Arc<ProfileDb>,
    /// NReady! cache
    nready: Option<Arc<NReadyCache>>,
    /// Hotness tracker for smart eviction
    hotness_tracker: Arc<HotnessTracker>,
    /// Statistics
    stats: Arc<JitStats>,
    /// Is engine running?
    running: AtomicBool,
    /// Has logged first S1 compilation for this VM?
    logged_first_s1: AtomicBool,
    /// Has logged first S2 compilation for this VM?
    logged_first_s2: AtomicBool,
    /// Last NReady! save time (for periodic saves)
    last_nready_save: std::sync::atomic::AtomicU64,
    
    // ========== Async JIT Infrastructure ==========
    /// Async compilation runtime (Zero-STW)
    async_runtime: Option<std::sync::Mutex<AsyncJitRuntime>>,
    /// Async eviction manager (background cache management)
    async_eviction: Option<Arc<std::sync::Mutex<AsyncEvictionManager>>>,
    /// Async restore manager (prefetch-based restoration)
    async_restore: Option<Arc<std::sync::Mutex<AsyncRestoreManager>>>,
    /// Use async compilation path (vs legacy sync)
    async_enabled: AtomicBool,
}

impl JitEngine {
    /// Create new JIT engine with default config
    pub fn new() -> Self {
        Self::with_config(JitConfig::default(), None)
    }
    
    /// Create new JIT engine with VM ID
    pub fn new_with_vm_id(vm_id: &str) -> Self {
        Self::with_config(JitConfig::default(), Some(vm_id))
    }
    
    /// Create new JIT engine with custom config
    /// 
    /// # Arguments
    /// * `config` - JIT configuration
    /// * `vm_id` - Optional VM ID for NReady! cache isolation. If None, uses "default".
    pub fn with_config(config: JitConfig, vm_id: Option<&str>) -> Self {
        let instance_id = vm_id.unwrap_or("default");
        let nready = if config.nready_enabled {
            let cache_dir = config.nready_path.clone()
                .unwrap_or_else(|| "/tmp/nvm-jit".to_string());
            let cache = NReadyCache::new(&cache_dir, instance_id);
            // Ensure evicted blocks directory exists
            if let Err(e) = cache.ensure_evicted_dir() {
                log::warn!("[NReady!] Failed to create evicted dir: {:?}", e);
            }
            Some(Arc::new(cache))
        } else {
            None
        };
        
        let profile_db = Arc::new(ProfileDb::new(config.profile_db_size));
        let hotness_tracker = Arc::new(HotnessTracker::new());
        let code_cache = Arc::new(CodeCache::new_dynamic(
            config.code_cache_initial_size as u64,
            config.code_cache_max_size as u64,
            config.code_cache_growth_factor,
        ));
        
        // Create async compilation infrastructure
        let async_enabled = config.async_compilation;
        let (async_runtime, async_eviction, async_restore) = if async_enabled {
            // Create compiler context for workers
            let compiler_ctx = Arc::new(CompilerContext::new(profile_db.clone()));
            
            // Create callback that installs to code cache
            let installer = Arc::new(CodeCacheInstaller::new(code_cache.clone()));
            
            // Create async runtime with worker threads
            let mut runtime = AsyncJitRuntime::new(
                installer.clone(),
                compiler_ctx,
                None, // auto-detect worker count
            );
            runtime.start();
            
            // Create async eviction and restore managers (requires NReady!)
            let (eviction_mgr, restore_mgr) = if let Some(ref nready_cache) = nready {
                // Create async eviction manager
                let mut eviction = AsyncEvictionManager::new(
                    code_cache.clone(),
                    nready_cache.clone(),
                    hotness_tracker.clone(),
                );
                eviction.start();
                
                // Create restore callback - installs restored code to cache
                let restore_installer = Arc::new(async_restore::CacheRestoreInstaller::new(
                    code_cache.clone(),
                    hotness_tracker.clone(),
                ));
                
                // Create async restore manager
                let mut restore = AsyncRestoreManager::new(
                    nready_cache.clone(),
                    hotness_tracker.clone(),
                    restore_installer,
                    None, // auto-detect worker count
                );
                restore.start();
                
                (Some(Arc::new(std::sync::Mutex::new(eviction))), Some(Arc::new(std::sync::Mutex::new(restore))))
            } else {
                log::warn!("[JIT] Async eviction/restore disabled: NReady! not enabled");
                (None, None)
            };
            
            log::info!("[JIT] Async JIT enabled: Zero-STW compilation, eviction, restoration");
            
            (
                Some(std::sync::Mutex::new(runtime)),
                eviction_mgr,
                restore_mgr,
            )
        } else {
            (None, None, None)
        };
        
        let mut engine = Self {
            decoder: X86Decoder::new(),
            interpreter: Interpreter::new(),
            s1_compiler: S1Compiler::new(),
            s2_compiler: S2Compiler::with_config(S2Config {
                loop_unroll: config.loop_unrolling,
                inline: config.aggressive_inlining,
                max_inline_size: (config.max_inline_depth * 10) as usize,
                ..Default::default()
            }),
            codegen: CodeGen::new(),
            code_cache,
            blocks: RwLock::new(HashMap::new()),
            profile_db,
            nready,
            hotness_tracker,
            stats: Arc::new(JitStats::default()),
            running: AtomicBool::new(false),
            logged_first_s1: AtomicBool::new(false),
            logged_first_s2: AtomicBool::new(false),
            last_nready_save: std::sync::atomic::AtomicU64::new(0),
            async_runtime,
            async_eviction,
            async_restore,
            async_enabled: AtomicBool::new(async_enabled),
            config,
        };
        
        // Async managers are already started during creation above
        
        // Try to load NReady! cache for instant warmup
        if engine.nready.is_some() {
            match engine.load_nready() {
                Ok(stats) => {
                    let warmup_type = stats.warmup_type();
                    if stats.native_blocks_loaded > 0 || stats.ir_blocks_loaded > 0 || stats.profiles_loaded > 0 {
                        log::info!("[NReady!] {}", stats.summary());
                        
                        // Log warmup skip info based on type
                        match warmup_type {
                            NReadyWarmupType::Hot => {
                                log::info!("[NReady!] Warmup skipped: {} native blocks ready, VM runs at S1/S2 performance immediately",
                                    stats.native_blocks_loaded);
                            }
                            NReadyWarmupType::WarmHot => {
                                log::info!("[NReady!] Partial warmup skip: {} native + {} IR blocks loaded, fast codegen on first access",
                                    stats.native_blocks_loaded, stats.ir_blocks_loaded);
                            }
                            NReadyWarmupType::Warm => {
                                log::info!("[NReady!] Profile-guided mode: {} hotspot entries loaded, will guide JIT optimization",
                                    stats.profiles_loaded);
                            }
                            NReadyWarmupType::Cold => {
                                log::info!("[NReady¿] It's a bit cold here, Little Misaka. Let's start a fire");
                            }
                        }
                    }
                }
                Err(e) => {
                    log::debug!("[NReady!] No cache loaded (first run or error): {:?}", e);
                }
            }
        }
        
        engine
    }
    
    /// Execute guest code starting at RIP
    /// 
    /// This is the main entry point. It:
    /// 1. Checks code cache for compiled code
    /// 2. Falls back to interpreter if not compiled
    /// 3. Collects profile data
    /// 4. Triggers compilation when thresholds are met (async or sync)
    /// 5. Periodically saves NReady! cache
    ///
    /// ## Zero-STW Async Mode
    /// When async_compilation is enabled:
    /// - Compilation requests are queued and processed by background workers
    /// - VM continues interpreting while compilation happens in parallel
    /// - Compiled code becomes available on next execution (no pause)
    /// - Eviction and restoration also happen asynchronously
    pub fn execute(&self, cpu: &VirtualCpu, memory: &AddressSpace) -> JitResult<ExecuteResult> {
        self.running.store(true, Ordering::SeqCst);
        
        // Check for periodic NReady! save
        self.maybe_periodic_save();
        
        let rip = cpu.read_rip();
        
        // Get or create block metadata and increment invocations
        let block = self.get_or_create_block(rip);
        let invocations = block.increment_invocations();
        
        // Check tier promotion
        let tier = self.determine_tier(&block, invocations);
        let current_tier = block.get_tier();
        
        // Log at key thresholds that relate to JIT compilation decisions:
        // 100: S1 trigger, 500: warmup, 1000: hot, 2000: S2 trigger, 10000: very hot
        match invocations {
            100 | 500 | 1000 | 2000 | 10000 => {
                log::debug!("[JIT] Block {:#x}: invocations={}, tier={:?}", rip, invocations, tier);
            }
            _ => {
                log::trace!("[JIT] Block {:#x}: invocations={}, tier={:?}", rip, invocations, tier);
            }
        }
        
        // Record execution in hotness tracker
        self.hotness_tracker.record_execution(rip);
        
        // Check code cache for compiled code (cache hit = fast path)
        if let Some(entry) = self.code_cache.lookup(rip) {
            self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            log::trace!("[JIT] Cache hit at {:#x}, executing native", rip);
            
            // Even with cache hit, check if we should submit S2 promotion (async only)
            if self.async_enabled.load(Ordering::Relaxed) 
                && tier == ExecutionTier::S2 
                && current_tier != ExecutionTier::S2 
            {
                self.submit_async_s2(memory, rip, invocations, &block);
            }
            
            return self.execute_native(cpu, memory, entry);
        }
        
        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        
        // Try to restore from disk if this block was previously evicted
        if self.async_enabled.load(Ordering::Relaxed) {
            // Async path: submit restore request (non-blocking)
            self.submit_async_restore(rip);
        } else {
            // Sync path: try immediate restore
            if self.try_restore_block(rip)? {
                if let Some(code_ptr) = self.code_cache.lookup(rip) {
                    log::debug!("[JIT] Executing restored block {:#x}", rip);
                    return self.execute_native(cpu, memory, code_ptr);
                }
            }
        }
        
        // Use async or sync compilation path based on config
        if self.async_enabled.load(Ordering::Relaxed) {
            self.execute_async_path(cpu, memory, rip, invocations, tier, &block)
        } else {
            self.execute_sync_path(cpu, memory, rip, tier, &block)
        }
    }
    
    /// Execute using async (Zero-STW) compilation path
    ///
    /// Key difference from sync: compilation is non-blocking.
    /// VM continues interpreting while compilation proceeds in background.
    fn execute_async_path(
        &self,
        cpu: &VirtualCpu,
        memory: &AddressSpace,
        rip: u64,
        invocations: u64,
        tier: ExecutionTier,
        block: &BlockMeta,
    ) -> JitResult<ExecuteResult> {
        match tier {
            ExecutionTier::Interpreter => {
                self.stats.interpreter_execs.fetch_add(1, Ordering::Relaxed);
                self.interpret(cpu, memory, rip)
            }
            ExecutionTier::S1 => {
                // Submit async S1 compilation request (non-blocking)
                self.submit_async_s1(memory, rip, invocations);
                
                // Continue interpreting while compilation proceeds
                self.stats.interpreter_execs.fetch_add(1, Ordering::Relaxed);
                self.interpret(cpu, memory, rip)
            }
            ExecutionTier::S2 => {
                // If we have S1, use it. Otherwise submit S1 first.
                if block.get_tier() == ExecutionTier::S1 {
                    // Submit S2 promotion
                    self.submit_async_s2(memory, rip, invocations, block);
                    
                    // Execute S1 (if available)
                    if let Some(code_ptr) = self.code_cache.lookup(rip) {
                        self.stats.s1_execs.fetch_add(1, Ordering::Relaxed);
                        return self.execute_native(cpu, memory, code_ptr);
                    }
                }
                
                // Fall back to interpreter
                self.stats.interpreter_execs.fetch_add(1, Ordering::Relaxed);
                self.interpret(cpu, memory, rip)
            }
        }
    }
    
    /// Submit async S1 compilation request (non-blocking)
    fn submit_async_s1(&self, memory: &AddressSpace, rip: u64, exec_count: u64) {
        if let Some(ref runtime_mutex) = self.async_runtime {
            // Copy guest code bytes for async compilation
            let mut guest_code = vec![0u8; 4096]; // Max block size
            for (i, byte) in guest_code.iter_mut().enumerate() {
                *byte = memory.read_u8(rip + i as u64);
            }
            
            let guest_checksum = cache::compute_checksum(&guest_code);
            
            if let Ok(runtime) = runtime_mutex.lock() {
                runtime.request_s1(
                    rip,
                    guest_code.len() as u32,
                    guest_checksum,
                    exec_count,
                    guest_code,
                );
            }
        }
    }
    
    /// Submit async S2 compilation request (non-blocking)
    fn submit_async_s2(&self, memory: &AddressSpace, rip: u64, exec_count: u64, _block: &BlockMeta) {
        if let Some(ref runtime_mutex) = self.async_runtime {
            // Copy guest code bytes for async compilation
            let mut guest_code = vec![0u8; 4096];
            for (i, byte) in guest_code.iter_mut().enumerate() {
                *byte = memory.read_u8(rip + i as u64);
            }
            
            let guest_checksum = cache::compute_checksum(&guest_code);
            
            // TODO: Pass IR hint from existing S1 block if available
            if let Ok(runtime) = runtime_mutex.lock() {
                runtime.request_s2(
                    rip,
                    guest_code.len() as u32,
                    guest_checksum,
                    exec_count,
                    None, // IR hint
                    guest_code,
                );
            }
        }
    }
    
    /// Submit async restore request (non-blocking)
    fn submit_async_restore(&self, rip: u64) {
        if let Some(ref restore_mgr_mutex) = self.async_restore {
            if let Ok(restore_mgr) = restore_mgr_mutex.lock() {
                restore_mgr.request_restore(rip, true); // true = on-demand
            }
        }
    }
    
    /// Execute using legacy sync compilation path
    fn execute_sync_path(
        &self,
        cpu: &VirtualCpu,
        memory: &AddressSpace,
        rip: u64,
        tier: ExecutionTier,
        block: &BlockMeta,
    ) -> JitResult<ExecuteResult> {
        // Check if we need to upgrade from S1 to S2
        if tier == ExecutionTier::S2 && block.get_tier() != ExecutionTier::S2 {
            self.compile_s2(cpu, memory, rip, block)?;
        }
        
        match tier {
            ExecutionTier::Interpreter => {
                self.stats.interpreter_execs.fetch_add(1, Ordering::Relaxed);
                self.interpret(cpu, memory, rip)
            }
            ExecutionTier::S1 => {
                // Compile with S1 if not already in code cache
                log::debug!("[JIT] Compiling S1 for block {:#x}", rip);
                self.compile_s1(cpu, memory, rip, block)?;
                self.stats.s1_execs.fetch_add(1, Ordering::Relaxed);
                // Execute from code cache (compile_s1 stores there)
                if let Some(code_ptr) = self.code_cache.lookup(rip) {
                    self.execute_native(cpu, memory, code_ptr)
                } else {
                    // Compilation failed, fall back to interpreter
                    log::warn!("[JIT] S1 code not found after compile for {:#x}", rip);
                    self.interpret(cpu, memory, rip)
                }
            }
            ExecutionTier::S2 => {
                // Compile with S2 if not already in code cache
                self.compile_s2(cpu, memory, rip, block)?;
                self.stats.s2_execs.fetch_add(1, Ordering::Relaxed);
                // Execute from code cache
                if let Some(code_ptr) = self.code_cache.lookup(rip) {
                    self.execute_native(cpu, memory, code_ptr)
                } else {
                    log::warn!("[JIT] S2 code not found after compile for {:#x}", rip);
                    self.interpret(cpu, memory, rip)
                }
            }
        }
    }
    
    /// Execute a single instruction (for debugging/single-step)
    pub fn step(&self, cpu: &VirtualCpu, memory: &AddressSpace) -> JitResult<StepResult> {
        let rip = cpu.read_rip();
        
        // Fetch instruction bytes
        let mut bytes = [0u8; 15];
        for i in 0..15 {
            bytes[i] = memory.read_u8(rip + i as u64);
        }
        
        // Decode single instruction
        let instr = self.decoder.decode(&bytes, rip)?;
        
        // Execute via interpreter (always for single-step)
        self.interpreter.execute_single(cpu, memory, &instr)
    }
    
    /// Interpret code at RIP (tier 0)
    fn interpret(&self, cpu: &VirtualCpu, memory: &AddressSpace, rip: u64) -> JitResult<ExecuteResult> {
        self.interpreter.execute_block(cpu, memory, rip, &self.decoder, &self.profile_db)
    }
    
    /// Compile with S1 (quick compiler)
    fn compile_s1(&self, _cpu: &VirtualCpu, memory: &AddressSpace, rip: u64, _block: &BlockMeta) -> JitResult<()> {
        let start = std::time::Instant::now();
        
        // Fetch guest code bytes
        let mut guest_code = vec![0u8; 4096]; // Max block size
        for (i, byte) in guest_code.iter_mut().enumerate() {
            *byte = memory.read_u8(rip + i as u64);
        }
        
        // Build IR using S1 compiler
        let s1_block = self.s1_compiler.compile(&guest_code, rip, &self.decoder, &self.profile_db)?;
        
        // Get native code size
        let native_len = s1_block.native.len();
        
        // Log INFO only for VM's first S1 compilation
        if !self.logged_first_s1.swap(true, Ordering::Relaxed) {
            log::info!("[JIT] First S1 compilation triggered for this VM");
        }
        
        log::debug!("[JIT] S1 compiled {:#x}: {} bytes of native code", rip, native_len);
        
        // Try to allocate executable memory, with smart eviction if needed
        let host_ptr = match self.code_cache.allocate_code(&s1_block.native) {
            Some(ptr) => ptr,
            None => {
                // Cache is full - perform smart eviction instead of failing
                self.perform_smart_eviction(native_len as u64)?;
                
                // Retry allocation
                self.code_cache.allocate_code(&s1_block.native)
                    .ok_or(JitError::CodeCacheFull)?
            }
        };
        
        // Register block in hotness tracker
        self.hotness_tracker.register_block(rip, CompileTier::S1);
        
        // Count instructions in IR
        let guest_instrs: u32 = s1_block.ir.blocks.iter()
            .map(|bb| bb.instrs.len() as u32)
            .sum();
        
        // Install in code cache
        let block = CompiledBlock {
            guest_rip: rip,
            guest_size: s1_block.guest_size,
            host_code: host_ptr,
            host_size: native_len as u32,
            tier: CompileTier::S1,
            exec_count: AtomicU64::new(0),
            last_access: AtomicU64::new(0),
            guest_instrs,
            guest_checksum: cache::compute_checksum(&guest_code[..s1_block.guest_size as usize]),
            depends_on: Vec::new(),
            invalidated: false,
        };
        
        // Insert with smart eviction on failure
        if let Err(_) = self.code_cache.insert(block) {
            self.perform_smart_eviction(native_len as u64)?;
            // Recreate block since insert consumed it
            let host_ptr = self.code_cache.allocate_code(&s1_block.native)
                .ok_or(JitError::CodeCacheFull)?;
            let block = CompiledBlock {
                guest_rip: rip,
                guest_size: s1_block.guest_size,
                host_code: host_ptr,
                host_size: native_len as u32,
                tier: CompileTier::S1,
                exec_count: AtomicU64::new(0),
                last_access: AtomicU64::new(0),
                guest_instrs,
                guest_checksum: cache::compute_checksum(&guest_code[..s1_block.guest_size as usize]),
                depends_on: Vec::new(),
                invalidated: false,
            };
            self.code_cache.insert(block).map_err(|_| JitError::CodeCacheFull)?;
        }
        
        // Update block tier to S1
        _block.set_tier(ExecutionTier::S1);
        
        let elapsed = start.elapsed().as_nanos() as u64;
        self.stats.compilation_time_ns.fetch_add(elapsed, Ordering::Relaxed);
        self.stats.s1_compilations.fetch_add(1, Ordering::Relaxed);
        
        Ok(())
    }
    
    /// Compile with S2 (optimizing compiler)
    fn compile_s2(&self, _cpu: &VirtualCpu, memory: &AddressSpace, rip: u64, block: &BlockMeta) -> JitResult<()> {
        let start = std::time::Instant::now();
        
        // S2 requires an existing S1 block to optimize
        // First ensure we have S1 compiled
        let native_len = if block.ir.is_none() {
            // Need to compile with S1 first
            let mut guest_code = vec![0u8; 4096];
            for (i, byte) in guest_code.iter_mut().enumerate() {
                *byte = memory.read_u8(rip + i as u64);
            }
            let s1_block = self.s1_compiler.compile(&guest_code, rip, &self.decoder, &self.profile_db)?;
            
            // Recompile with S2 optimizations
            let s2_block = self.s2_compiler.compile_from_s1(&s1_block, &self.profile_db)?;
            let len = s2_block.native.len();
            
            // Replace in code cache (with smart eviction if needed)
            if let Err(_) = self.code_cache.replace(rip, s2_block.native.clone()) {
                self.perform_smart_eviction(len as u64)?;
                self.code_cache.replace(rip, s2_block.native)?;
            }
            
            // Update hotness tracker tier
            self.hotness_tracker.update_tier(rip, CompileTier::S2);
            
            len
        } else if let Some(ref _ir) = block.ir {
            // We have existing IR, find S1 block somehow
            // For now, recompile from scratch
            let mut guest_code = vec![0u8; 4096];
            for (i, byte) in guest_code.iter_mut().enumerate() {
                *byte = memory.read_u8(rip + i as u64);
            }
            let s1_block = self.s1_compiler.compile(&guest_code, rip, &self.decoder, &self.profile_db)?;
            let s2_block = self.s2_compiler.compile_from_s1(&s1_block, &self.profile_db)?;
            let len = s2_block.native.len();
            self.code_cache.replace(rip, s2_block.native)?;
            len
        } else {
            0
        };
        
        // Log INFO only for VM's first S2 compilation
        if !self.logged_first_s2.swap(true, Ordering::Relaxed) {
            log::info!("[JIT] First S2 compilation triggered for this VM");
        }
        
        log::debug!("[JIT] S2 compiled block {:#x}: {} bytes of native code", rip, native_len);
        
        // Update block tier to S2
        block.set_tier(ExecutionTier::S2);
        
        let elapsed = start.elapsed().as_nanos() as u64;
        self.stats.compilation_time_ns.fetch_add(elapsed, Ordering::Relaxed);
        self.stats.s2_compilations.fetch_add(1, Ordering::Relaxed);
        
        Ok(())
    }
    
    /// Execute native code from cache using JitState
    fn execute_native(&self, cpu: &VirtualCpu, memory: &AddressSpace, code_ptr: *const u8) -> JitResult<ExecuteResult> {
        // Create JitState from VirtualCpu - this is the JIT's private copy
        let mut jit_state = JitState::from_vcpu(cpu, memory);
        
        log::trace!("[JIT] Before native: JitState.rip={:#x}", jit_state.rip);
        
        // Safety: native code was generated by our codegen and expects JitState pointer
        let result = unsafe {
            // Native function signature: fn(*mut JitState) -> u64
            let func: extern "C" fn(*mut JitState) -> u64 = 
                std::mem::transmute(code_ptr);
            
            func(&mut jit_state as *mut JitState)
        };
        
        log::trace!("[JIT] After native: JitState.rip={:#x}, result={:#x}", jit_state.rip, result);
        
        // Copy JitState back to VirtualCpu
        jit_state.to_vcpu(cpu);
        
        Ok(ExecuteResult::from_native(result))
    }
    
    fn execute_s1(&self, cpu: &VirtualCpu, memory: &AddressSpace, block: &BlockMeta) -> JitResult<ExecuteResult> {
        if let Some(code_ptr) = block.native_code {
            // Create JitState from VirtualCpu
            let mut jit_state = JitState::from_vcpu(cpu, memory);
            
            let result = unsafe {
                let func: extern "C" fn(*mut JitState) -> u64 =
                    std::mem::transmute(code_ptr);
                func(&mut jit_state as *mut JitState)
            };
            
            // Copy back
            jit_state.to_vcpu(cpu);
            
            Ok(ExecuteResult::from_native(result))
        } else {
            // Fall back to interpreter
            self.interpret(cpu, memory, block.guest_rip)
        }
    }
    
    fn execute_s2(&self, cpu: &VirtualCpu, memory: &AddressSpace, block: &BlockMeta) -> JitResult<ExecuteResult> {
        self.execute_s1(cpu, memory, block)
    }
    
    /// Determine which tier to use for this block
    fn determine_tier(&self, block: &BlockMeta, invocations: u64) -> ExecutionTier {
        if !self.config.tiered_compilation {
            return ExecutionTier::Interpreter;
        }
        
        if invocations >= self.config.thresholds.s1_to_s2 {
            ExecutionTier::S2
        } else if invocations >= self.config.thresholds.interpreter_to_s1 {
            ExecutionTier::S1
        } else {
            ExecutionTier::Interpreter
        }
    }
    
    /// Get or create block metadata
    fn get_or_create_block(&self, rip: u64) -> Arc<BlockMeta> {
        // Fast path: read lock
        {
            let blocks = self.blocks.read().unwrap();
            if let Some(block) = blocks.get(&rip) {
                return block.clone();
            }
        }
        
        // Slow path: write lock
        let mut blocks = self.blocks.write().unwrap();
        blocks.entry(rip)
            .or_insert_with(|| Arc::new(BlockMeta::new(rip)))
            .clone()
    }
    
    /// Invalidate compiled code for a range (e.g., after SMC)
    pub fn invalidate_range(&self, start: u64, end: u64) {
        self.code_cache.invalidate_range(start, end);
        
        let mut blocks = self.blocks.write().unwrap();
        blocks.retain(|rip, block| {
            if *rip >= start && *rip < end {
                block.valid.store(false, Ordering::Release);
                false
            } else {
                true
            }
        });
    }
    
    // ========================================================================
    // NReady! Persistence - Three Formats with Different Compatibility
    // ========================================================================
    
    /// Load NReady! cache with tiered loading strategy
    /// 
    /// Loading priority:
    /// 1. Native code (instant, same-generation only)
    /// 2. RI (backward compatible, needs codegen)
    /// 3. Profile (full compat, guides future compilation)
    pub fn load_nready(&self) -> JitResult<NReadyStats> {
        let nready = self.nready.as_ref()
            .ok_or_else(|| JitError::CompilationError("NReady! not enabled".to_string()))?;
        
        let mut stats = NReadyStats::default();
        let start = std::time::Instant::now();
        
        // 1. Try loading native code first (zero warmup if version matches)
        match nready.load_native() {
            Ok(Some(native_blocks)) => {
                stats.native_blocks_loaded = native_blocks.len();
                // Install native blocks directly into code cache
                for (rip, block_info) in native_blocks {
                    if let Err(e) = self.install_native_block(rip, block_info) {
                        log::warn!("[NReady!] Failed to install native block {:#x}: {:?}", rip, e);
                    }
                }
                log::debug!("[NReady!] Installed {} native code blocks", stats.native_blocks_loaded);
            }
            Ok(None) => {
                log::debug!("[NReady!] Native cache version mismatch, will recompile");
            }
            Err(e) => {
                log::debug!("[NReady!] No native cache found: {:?}", e);
            }
        }
        
        // 2. Load RI blocks (backward compatible)
        match nready.load_ri() {
            Ok(ir_blocks) => {
                stats.ir_blocks_loaded = ir_blocks.len();
                // Store IR blocks for on-demand compilation
                let mut blocks = self.blocks.write().unwrap();
                for (rip, ir) in ir_blocks {
                    let meta = blocks.entry(rip).or_insert_with(|| Arc::new(BlockMeta::new(rip)));
                    // Note: We can't modify Arc<BlockMeta> directly, IR caching needs redesign
                    // For now, IR blocks will be recompiled on demand
                }
                log::debug!("[NReady!] Loaded {} IR blocks", stats.ir_blocks_loaded);
            }
            Err(e) => {
                log::debug!("[NReady!] No RI cache found: {:?}", e);
            }
        }
        
        // 3. Always load profile (full compat, guides compilation)
        match nready.load_profile() {
            Ok(profile) => {
                stats.profiles_loaded = profile.block_count();
                // Merge profile data to guide hot path detection
                self.profile_db.merge(&profile);
                log::debug!("[NReady!] Loaded {} profile entries", stats.profiles_loaded);
            }
            Err(e) => {
                log::debug!("[NReady!] No profile cache found: {:?}", e);
            }
        }
        
        stats.load_time_ms = start.elapsed().as_millis() as u64;
        self.stats.nready_loads.fetch_add(1, Ordering::Relaxed);
        
        Ok(stats)
    }
    
    /// Install a native code block from NReady! cache into code cache
    fn install_native_block(&self, rip: u64, info: nready::NativeBlockInfo) -> JitResult<()> {
        // Allocate executable memory and copy the native code
        let host_ptr = self.code_cache.allocate_code(&info.native_code)
            .ok_or(JitError::CodeCacheFull)?;
        
        // Create CompiledBlock and install in cache
        let block = CompiledBlock {
            guest_rip: rip,
            guest_size: info.guest_size,
            host_code: host_ptr,
            host_size: info.host_size,
            tier: info.tier,
            exec_count: AtomicU64::new(info.exec_count),
            last_access: AtomicU64::new(0),
            guest_instrs: info.guest_instrs,
            guest_checksum: info.guest_checksum,
            depends_on: Vec::new(),
            invalidated: false,
        };
        
        self.code_cache.insert(block).map_err(|_| JitError::CodeCacheFull)?;
        
        // Update block metadata tier
        let meta = self.get_or_create_block(rip);
        let tier = match info.tier {
            CompileTier::Interpreter => ExecutionTier::Interpreter,
            CompileTier::S1 => ExecutionTier::S1,
            CompileTier::S2 => ExecutionTier::S2,
        };
        meta.set_tier(tier);
        
        Ok(())
    }
    
    /// Save NReady! cache in specified format
    /// 
    /// Format compatibility guarantees:
    /// - Profile: Full forward AND backward compatibility
    /// - RI: Backward compatible (old RI works on new JIT)
    /// - Native: Same-generation only (version must match exactly)
    pub fn save_nready(&self, format: PersistFormat) -> JitResult<()> {
        let nready = self.nready.as_ref()
            .ok_or_else(|| JitError::CompilationError("NReady! not enabled".to_string()))?;
        
        match format {
            PersistFormat::Profile => {
                // Profile: Always safe, full compatibility
                nready.save_profile(&self.profile_db)
            }
            PersistFormat::Ri => {
                // RI: Backward compatible IR
                // Note: IR is stored in blocks metadata, not code_cache
                let blocks = self.blocks.read().unwrap();
                let ir_blocks: HashMap<u64, _> = blocks.iter()
                    .filter_map(|(rip, meta)| {
                        meta.ir.as_ref().map(|ir| (*rip, (**ir).clone()))
                    })
                    .collect();
                nready.save_ri(&ir_blocks)
            }
            PersistFormat::Native => {
                // Native: Same-generation only
                // Get compiled blocks from code_cache (this is where they actually live!)
                let persist_blocks = self.code_cache.get_all_blocks_for_persist();
                nready.save_native_from_persist(&persist_blocks)
            }
            PersistFormat::All => {
                // Save all formats for maximum flexibility
                self.save_nready(PersistFormat::Profile)?;
                self.save_nready(PersistFormat::Ri)?;
                self.save_nready(PersistFormat::Native)?;
                Ok(())
            }
        }
    }
    
    /// Check if periodic NReady! save is needed and perform it
    /// 
    /// This is called periodically from execute() to ensure data is saved
    /// even if the VM crashes or is killed without proper shutdown.
    fn maybe_periodic_save(&self) {
        // Skip if periodic save is disabled
        if self.config.nready_save_interval_secs == 0 || !self.config.nready_enabled {
            return;
        }
        
        // Get current time in seconds since UNIX epoch
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        
        let last_save = self.last_nready_save.load(Ordering::Relaxed);
        
        // Check if enough time has passed
        if last_save == 0 {
            // First execution, set initial time
            self.last_nready_save.store(now, Ordering::Relaxed);
            return;
        }
        
        if now.saturating_sub(last_save) >= self.config.nready_save_interval_secs {
            // Try to update atomically (avoid concurrent saves)
            if self.last_nready_save.compare_exchange(
                last_save,
                now,
                Ordering::SeqCst,
                Ordering::Relaxed
            ).is_ok() {
                // We won the race, do the save
                log::info!("[JIT] Periodic NReady! save (interval: {}s)", 
                           self.config.nready_save_interval_secs);
                
                if let Err(e) = self.save_nready(PersistFormat::All) {
                    log::warn!("[JIT] Periodic NReady! save failed: {:?}", e);
                } else {
                    log::debug!("[JIT] Periodic NReady! save completed");
                }
            }
        }
    }
    
    /// Get statistics
    pub fn stats(&self) -> &JitStats {
        &self.stats
    }
    
    /// Get configuration
    pub fn config(&self) -> &JitConfig {
        &self.config
    }
    
    // ========================================================================
    // Smart Eviction - Tiered Code Cache Management
    // ========================================================================
    
    /// Perform smart eviction to free space in code cache
    /// 
    /// Strategy:
    /// 1. Select coldest blocks using hotness tracker
    /// 2. S2 blocks: Always persist to disk (expensive to recompile)
    /// 3. S1 blocks: Persist if hot enough, otherwise discard
    /// 4. Remove blocks from cache after persistence
    fn perform_smart_eviction(&self, needed: u64) -> JitResult<()> {
        log::debug!("[JIT] Smart eviction triggered: need {} bytes", needed);
        
        self.stats.smart_evictions.fetch_add(1, Ordering::Relaxed);
        
        // S1 preserve threshold: blocks with 1000+ executions worth saving
        const S1_PRESERVE_THRESHOLD: u64 = 1000;
        
        // Use code cache's smart eviction
        let evict_result = self.code_cache.smart_evict(needed, S1_PRESERVE_THRESHOLD)
            .map_err(|_| JitError::CodeCacheFull)?;
        
        log::debug!("[JIT] Smart evict selected: {} to persist, {} to discard",
            evict_result.to_persist.len(),
            evict_result.to_discard.len());
        
        // Persist blocks to disk via NReady!
        if !evict_result.to_persist.is_empty() {
            if let Some(ref nready) = self.nready {
                for block_info in &evict_result.to_persist {
                    let evictable = EvictableBlock {
                        rip: block_info.guest_rip,
                        tier: block_info.tier,
                        native_code: block_info.native_code.clone(),
                        guest_size: block_info.guest_size,
                        guest_instrs: block_info.guest_instrs,
                        guest_checksum: block_info.guest_checksum,
                        exec_count: block_info.exec_count,
                        ir_data: None, // TODO: serialize IR if available
                    };
                    
                    match nready.evict_block(&evictable) {
                        Ok(result) => {
                            // Record in hotness tracker's evicted index
                            self.hotness_tracker.evicted_index.record_eviction(EvictedBlockInfo {
                                rip: block_info.guest_rip,
                                tier: block_info.tier,
                                exec_count: block_info.exec_count,
                                guest_checksum: block_info.guest_checksum,
                                evicted_at: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0),
                                persist_path: result.path,
                                has_native: result.has_native,
                                has_ir: result.has_ir,
                            });
                            self.hotness_tracker.mark_evicted(block_info.guest_rip);
                            self.stats.blocks_evicted_to_disk.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            log::warn!("[JIT] Failed to evict block {:#x} to disk: {:?}", 
                                block_info.guest_rip, e);
                        }
                    }
                }
            }
        }
        
        // Remove all selected blocks from cache
        let mut all_rips: Vec<u64> = evict_result.to_discard.clone();
        all_rips.extend(evict_result.persisted_rips.iter());
        
        let freed = self.code_cache.remove_blocks(&all_rips);
        log::debug!("[JIT] Smart eviction freed {} bytes", freed);
        
        Ok(())
    }
    
    /// Try to restore a block from disk if it was previously evicted
    /// 
    /// Called when a cache miss occurs for a block that might be in the evicted index.
    /// Returns true if restoration was successful.
    fn try_restore_block(&self, rip: u64) -> JitResult<bool> {
        // Check if this block was evicted
        if let Some(_evicted_info) = self.hotness_tracker.can_restore(rip) {
            if let Some(ref nready) = self.nready {
                match nready.restore_block(rip) {
                    Ok(Some(restored)) => {
                        log::debug!("[JIT] Restoring block {:#x} from disk (native={})",
                            rip, restored.can_load_directly());
                        
                        if let Some(native_code) = restored.native_code {
                            // Directly install native code
                            let host_ptr = self.code_cache.allocate_code(&native_code)
                                .ok_or(JitError::CodeCacheFull)?;
                            
                            let block = CompiledBlock {
                                guest_rip: rip,
                                guest_size: restored.guest_size,
                                host_code: host_ptr,
                                host_size: native_code.len() as u32,
                                tier: restored.tier,
                                exec_count: AtomicU64::new(restored.exec_count),
                                last_access: AtomicU64::new(0),
                                guest_instrs: restored.guest_instrs,
                                guest_checksum: restored.guest_checksum,
                                depends_on: Vec::new(),
                                invalidated: false,
                            };
                            
                            self.code_cache.insert(block).map_err(|_| JitError::CodeCacheFull)?;
                        } else {
                            // Native code stale, but we can use IR if available
                            // For now, return false to trigger recompilation
                            log::debug!("[JIT] Restored block {:#x} has stale native code, needs recompile", rip);
                            return Ok(false);
                        }
                        
                        // Update tracker
                        self.hotness_tracker.mark_restored(rip);
                        self.stats.blocks_restored.fetch_add(1, Ordering::Relaxed);
                        
                        return Ok(true);
                    }
                    Ok(None) => {
                        // Block not found on disk, maybe it was deleted
                        log::debug!("[JIT] Block {:#x} not found on disk", rip);
                    }
                    Err(e) => {
                        log::warn!("[JIT] Failed to restore block {:#x}: {:?}", rip, e);
                    }
                }
            }
        }
        
        Ok(false)
    }
    
    /// Get hotness tracker snapshot for monitoring
    pub fn hotness_snapshot(&self) -> HotnessSnapshot {
        self.hotness_tracker.get_stats()
    }
    
    /// Get evicted blocks disk usage
    pub fn evicted_disk_usage(&self) -> u64 {
        self.nready.as_ref()
            .map(|n| n.evicted_disk_usage())
            .unwrap_or(0)
    }

    /// Shutdown the JIT engine
    /// 
    /// This:
    /// 1. Stops async compilation workers
    /// 2. Stops async eviction manager
    /// 3. Stops async restore manager
    /// 4. Saves NReady! cache if auto_save is enabled
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        
        // Shutdown async components first
        if self.async_enabled.load(Ordering::Relaxed) {
            log::info!("[JIT] Shutting down async JIT infrastructure...");
            
            // Shutdown compilation runtime
            if let Some(ref runtime_mutex) = self.async_runtime {
                if let Ok(mut runtime) = runtime_mutex.lock() {
                    runtime.shutdown();
                }
            }
            
            // Shutdown eviction manager
            if let Some(ref eviction_mutex) = self.async_eviction {
                if let Ok(mut eviction) = eviction_mutex.lock() {
                    eviction.shutdown();
                }
            }
            
            // Shutdown restore manager
            if let Some(ref restore_mutex) = self.async_restore {
                if let Ok(mut restore) = restore_mutex.lock() {
                    restore.shutdown();
                }
            }
            
            log::info!("[JIT] Async JIT infrastructure shutdown complete");
        }
        
        if self.config.nready_enabled && self.config.nready_auto_save {
            log::info!("[JIT] Saving NReady! cache on shutdown...");
            if let Err(e) = self.save_nready(PersistFormat::All) {
                log::warn!("[JIT] Failed to save NReady! cache: {:?}", e);
            } else {
                log::info!("[JIT] NReady! cache saved successfully");
            }
        }
    }
    
    /// Check if async JIT is enabled
    pub fn is_async_enabled(&self) -> bool {
        self.async_enabled.load(Ordering::Relaxed)
    }
    
    /// Get async runtime statistics (if enabled)
    pub fn async_stats(&self) -> Option<AsyncStatsSnapshot> {
        if let Some(ref runtime_mutex) = self.async_runtime {
            if let Ok(runtime) = runtime_mutex.lock() {
                return Some(runtime.get_stats());
            }
        }
        None
    }
    
    /// Get async eviction statistics (if enabled)
    pub fn async_eviction_stats(&self) -> Option<EvictionStatsSnapshot> {
        if let Some(ref eviction_mutex) = self.async_eviction {
            if let Ok(eviction) = eviction_mutex.lock() {
                return Some(eviction.get_stats());
            }
        }
        None
    }
    
    /// Get async restore statistics (if enabled)
    pub fn async_restore_stats(&self) -> Option<RestoreStatsSnapshot> {
        if let Some(ref restore_mutex) = self.async_restore {
            if let Ok(restore) = restore_mutex.lock() {
                return Some(restore.get_stats());
            }
        }
        None
    }
}

impl Drop for JitEngine {
    fn drop(&mut self) {
        // Shutdown async infrastructure first
        if self.async_enabled.load(Ordering::Relaxed) {
            if let Some(ref runtime_mutex) = self.async_runtime {
                if let Ok(mut runtime) = runtime_mutex.lock() {
                    runtime.shutdown();
                }
            }
            if let Some(ref eviction_mutex) = self.async_eviction {
                if let Ok(mut eviction) = eviction_mutex.lock() {
                    eviction.shutdown();
                }
            }
            if let Some(ref restore_mutex) = self.async_restore {
                if let Ok(mut restore) = restore_mutex.lock() {
                    restore.shutdown();
                }
            }
        }
        
        // Auto-save NReady! on drop if enabled
        if self.config.nready_enabled && self.config.nready_auto_save {
            log::info!("[JIT] Auto-saving NReady! cache on drop...");
            if let Err(e) = self.save_nready(PersistFormat::All) {
                log::warn!("[JIT] Failed to auto-save NReady! cache: {:?}", e);
            }
        }
    }
}

impl Default for JitEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of block execution
#[derive(Debug, Clone)]
pub enum ExecuteResult {
    /// Continue execution at new RIP
    Continue { next_rip: u64 },
    /// Halt (HLT instruction)
    Halt,
    /// External interrupt pending
    Interrupt { vector: u8 },
    /// Exception occurred
    Exception { vector: u8, error_code: Option<u32> },
    /// I/O instruction needs emulation
    IoNeeded { port: u16, is_write: bool, size: u8 },
    /// MMIO access needs emulation
    MmioNeeded { addr: u64, is_write: bool, size: u8 },
    /// Hypercall (VMCALL)
    Hypercall { nr: u64, args: [u64; 4] },
    /// Reset requested
    Reset,
    /// Shutdown (triple fault)
    Shutdown,
}

impl ExecuteResult {
    fn from_native(code: u64) -> Self {
        // Decode native return value
        let kind = (code >> 56) as u8;
        let value = code & 0x00FF_FFFF_FFFF_FFFF;
        
        match kind {
            0 => Self::Continue { next_rip: value },
            1 => Self::Halt,
            2 => Self::Interrupt { vector: value as u8 },
            3 => Self::Exception { 
                vector: (value >> 32) as u8,
                error_code: if (value & 0x8000_0000) != 0 {
                    Some((value & 0x7FFF_FFFF) as u32)
                } else {
                    None
                },
            },
            4 => Self::IoNeeded {
                port: (value >> 16) as u16,
                is_write: (value & 0x8000) != 0,
                size: (value & 0xFF) as u8,
            },
            5 => Self::Reset,
            6 => Self::Shutdown,
            _ => Self::Continue { next_rip: value },
        }
    }
}

/// Result of single instruction step
#[derive(Debug, Clone)]
pub struct StepResult {
    /// Next RIP after instruction
    pub next_rip: u64,
    /// Instruction mnemonic
    pub mnemonic: String,
    /// Instruction length
    pub length: u8,
    /// Was branch taken?
    pub branch_taken: Option<bool>,
    /// Memory accesses
    pub mem_accesses: Vec<MemAccess>,
}

/// Memory access record
#[derive(Debug, Clone)]
pub struct MemAccess {
    pub addr: u64,
    pub size: u8,
    pub is_write: bool,
    pub value: u64,
}

/// NReady! warmup type based on cache contents
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NReadyWarmupType {
    /// Hot start: >50% native blocks, instant execution
    Hot,
    /// Warm-hot start: >30% native + IR combined, fast codegen
    WarmHot,
    /// Warm start: profile-guided, needs recompilation but optimized
    Warm,
    /// Cold start: no cache or minimal data
    Cold,
}

impl std::fmt::Display for NReadyWarmupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NReadyWarmupType::Hot => write!(f, "Hot"),
            NReadyWarmupType::WarmHot => write!(f, "Warm-Hot"),
            NReadyWarmupType::Warm => write!(f, "Warm"),
            NReadyWarmupType::Cold => write!(f, "Cold"),
        }
    }
}

/// NReady! load statistics
#[derive(Debug, Clone, Default)]
pub struct NReadyStats {
    pub profiles_loaded: usize,
    pub ir_blocks_loaded: usize,
    pub native_blocks_loaded: usize,
    pub native_blocks_rejected: usize,
    pub load_time_ms: u64,
}

impl NReadyStats {
    /// Calculate total blocks loaded (excluding profiles which are metadata)
    pub fn total_code_blocks(&self) -> usize {
        self.native_blocks_loaded + self.ir_blocks_loaded
    }
    
    /// Calculate percentage of native blocks (instant execution)
    pub fn native_ratio(&self) -> f32 {
        let total = self.total_code_blocks();
        if total == 0 { return 0.0; }
        (self.native_blocks_loaded as f32 / total as f32) * 100.0
    }
    
    /// Calculate percentage of IR blocks (fast codegen)
    pub fn ir_ratio(&self) -> f32 {
        let total = self.total_code_blocks();
        if total == 0 { return 0.0; }
        (self.ir_blocks_loaded as f32 / total as f32) * 100.0
    }
    
    /// Determine warmup type based on loaded cache contents
    /// 
    /// Classification:
    /// - Hot: >50% native blocks (instant, zero warmup)
    /// - Warm-Hot: >30% native+IR combined (fast codegen path)
    /// - Warm: has profile data (guided recompilation)
    /// - Cold: nothing useful loaded
    pub fn warmup_type(&self) -> NReadyWarmupType {
        let total = self.total_code_blocks();
        
        if total == 0 {
            // No code blocks, check if we have profile
            if self.profiles_loaded > 0 {
                return NReadyWarmupType::Warm;
            }
            return NReadyWarmupType::Cold;
        }
        
        let native_pct = self.native_ratio();
        let combined_pct = native_pct + self.ir_ratio();
        
        if native_pct >= 50.0 {
            NReadyWarmupType::Hot
        } else if combined_pct >= 30.0 || self.native_blocks_loaded >= 1 {
            NReadyWarmupType::WarmHot
        } else if self.profiles_loaded > 0 {
            NReadyWarmupType::Warm
        } else {
            NReadyWarmupType::Cold
        }
    }
    
    /// Format a summary string for logging
    pub fn summary(&self) -> String {
        let warmup = self.warmup_type();
        let total = self.total_code_blocks();
        
        if total == 0 && self.profiles_loaded == 0 {
            return format!("[{}] no cache data", warmup);
        }
        
        let mut parts = Vec::new();
        
        if self.native_blocks_loaded > 0 {
            parts.push(format!("Native: {} ({:.1}%)", 
                self.native_blocks_loaded, self.native_ratio()));
        }
        if self.ir_blocks_loaded > 0 {
            parts.push(format!("IR: {} ({:.1}%)", 
                self.ir_blocks_loaded, self.ir_ratio()));
        }
        if self.profiles_loaded > 0 {
            parts.push(format!("Profile: {}", self.profiles_loaded));
        }
        
        format!("[{}] {} | {}ms", warmup, parts.join(", "), self.load_time_ms)
    }
}

// ============================================================================
// Async JIT Orchestrator - Zero-STW Unified Management
// ============================================================================

/// Configuration for the async JIT orchestrator
#[derive(Debug, Clone)]
pub struct AsyncJitConfig {
    /// Number of compilation worker threads (0 = auto)
    pub compile_workers: usize,
    /// Number of restoration worker threads (0 = auto)
    pub restore_workers: usize,
    /// Enable prefetch for restoration
    pub enable_prefetch: bool,
    /// High watermark for cache eviction (percentage)
    pub eviction_high_watermark: f64,
    /// Low watermark for cache eviction (percentage)
    pub eviction_low_watermark: f64,
}

impl Default for AsyncJitConfig {
    fn default() -> Self {
        Self {
            compile_workers: 0, // Auto-detect
            restore_workers: 0, // Auto-detect
            enable_prefetch: true,
            eviction_high_watermark: 0.80,
            eviction_low_watermark: 0.60,
        }
    }
}

/// Unified async JIT orchestrator
/// 
/// Coordinates compilation, eviction, and restoration without VM pause.
/// This is the recommended entry point for async JIT operations.
///
/// ## Zero-STW Guarantees
///
/// 1. **Compilation**: Happens in background workers. VM continues interpreting
///    until compiled code is atomically installed.
///
/// 2. **Eviction**: Incremental, batch-based. Blocks are persisted in small
///    batches with yields between, never blocking VM execution.
///
/// 3. **Restoration**: On-demand with prefetch. Cache misses trigger async
///    restoration while VM falls back to interpret/recompile.
///
/// ## Usage
///
/// ```rust,ignore
/// let orchestrator = AsyncJitOrchestrator::new(cache, nready, hotness, config);
/// orchestrator.start();
///
/// // Request compilation (non-blocking)
/// orchestrator.request_compile(rip, guest_size, checksum, exec_count, CompileTier::S1);
///
/// // On cache miss, check evicted index
/// if orchestrator.is_evicted(rip) {
///     orchestrator.request_restore(rip);  // Non-blocking
///     // Fall back to interpreter while restoration proceeds
/// }
///
/// orchestrator.shutdown();  // Graceful shutdown
/// ```
pub struct AsyncJitOrchestrator {
    /// Async compilation runtime
    compile_runtime: async_runtime::AsyncJitRuntime,
    /// Async eviction manager  
    eviction_manager: async_eviction::AsyncEvictionManager,
    /// Async restoration manager
    restore_manager: async_restore::AsyncRestoreManager,
    /// Code cache (shared)
    cache: Arc<CodeCache>,
    /// NReady! cache (shared)
    nready: Arc<NReadyCache>,
    /// Hotness tracker (shared)
    hotness: Arc<HotnessTracker>,
    /// Configuration
    config: AsyncJitConfig,
    /// Is orchestrator running?
    running: AtomicBool,
}

impl AsyncJitOrchestrator {
    /// Create a new async JIT orchestrator
    pub fn new(
        cache: Arc<CodeCache>,
        nready: Arc<NReadyCache>,
        hotness: Arc<HotnessTracker>,
        profile_db: Arc<ProfileDb>,
        config: AsyncJitConfig,
    ) -> Self {
        // Create compile callback
        let compile_callback = Arc::new(async_runtime::CodeCacheInstaller::new(Arc::clone(&cache)));
        
        // Create compiler context for workers
        let compiler_ctx = Arc::new(CompilerContext::new(Arc::clone(&profile_db)));
        
        // Create restore callback
        let restore_callback = Arc::new(async_restore::CacheRestoreInstaller::new(
            Arc::clone(&cache),
            Arc::clone(&hotness),
        ));
        
        // Build components
        let compile_runtime = async_runtime::AsyncJitRuntime::new(
            compile_callback,
            compiler_ctx,
            if config.compile_workers == 0 { None } else { Some(config.compile_workers) },
        );
        
        let eviction_manager = async_eviction::AsyncEvictionManager::new(
            Arc::clone(&cache),
            Arc::clone(&nready),
            Arc::clone(&hotness),
        );
        
        let restore_manager = async_restore::AsyncRestoreManager::new(
            Arc::clone(&nready),
            Arc::clone(&hotness),
            restore_callback,
            if config.restore_workers == 0 { None } else { Some(config.restore_workers) },
        );
        
        Self {
            compile_runtime,
            eviction_manager,
            restore_manager,
            cache,
            nready,
            hotness,
            config,
            running: AtomicBool::new(false),
        }
    }
    
    /// Start all async workers
    pub fn start(&mut self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }
        
        log::info!("[AsyncJIT] Starting orchestrator");
        
        self.compile_runtime.start();
        self.eviction_manager.start();
        self.restore_manager.start();
        
        log::info!("[AsyncJIT] Orchestrator started");
    }
    
    /// Request async compilation (non-blocking)
    /// 
    /// Returns true if request was queued, false if already in-flight.
    pub fn request_compile(
        &self,
        rip: u64,
        guest_size: u32,
        checksum: u64,
        exec_count: u64,
        tier: CompileTier,
        guest_code: Vec<u8>,
    ) -> bool {
        match tier {
            CompileTier::S1 => self.compile_runtime.request_s1(rip, guest_size, checksum, exec_count, guest_code),
            CompileTier::S2 => self.compile_runtime.request_s2(rip, guest_size, checksum, exec_count, None, guest_code),
            CompileTier::Interpreter => false,
        }
    }
    
    /// Request async restoration (non-blocking)
    ///
    /// Returns true if request was queued, false if not evicted or already in-flight.
    pub fn request_restore(&self, rip: u64) -> bool {
        self.restore_manager.request_restore(rip, true)
    }
    
    /// Check if a block was evicted (for fallback decision)
    pub fn is_evicted(&self, rip: u64) -> bool {
        self.hotness.evicted_index.get_evicted(rip).is_some()
    }
    
    /// Check if compilation is in progress
    pub fn is_compiling(&self, _rip: u64) -> bool {
        // TODO: Track in-flight compilations
        false
    }
    
    /// Check if restoration is in progress
    pub fn is_restoring(&self, rip: u64) -> bool {
        self.restore_manager.is_restoring(rip)
    }
    
    /// Trigger eviction check (for proactive cache management)
    pub fn trigger_eviction(&self) {
        self.eviction_manager.trigger();
    }
    
    /// Emergency eviction when cache is critically full
    pub fn emergency_eviction(&self, bytes_needed: u64) {
        self.eviction_manager.emergency_evict(bytes_needed);
    }
    
    /// Pause eviction (for critical operations)
    pub fn pause_eviction(&self) {
        self.eviction_manager.pause();
    }
    
    /// Resume eviction
    pub fn resume_eviction(&self) {
        self.eviction_manager.resume();
    }
    
    /// Get unified statistics
    pub fn get_stats(&self) -> OrchestratorStats {
        OrchestratorStats {
            compile: self.compile_runtime.get_stats(),
            eviction: self.eviction_manager.get_stats(),
            restore: self.restore_manager.get_stats(),
            cache_size: self.cache.total_size(),
            cache_capacity: self.cache.capacity(),
        }
    }
    
    /// Shutdown all async workers gracefully
    pub fn shutdown(&mut self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return; // Already stopped
        }
        
        log::info!("[AsyncJIT] Shutting down orchestrator...");
        
        // Shutdown in reverse order
        self.restore_manager.shutdown();
        self.eviction_manager.shutdown();
        self.compile_runtime.shutdown();
        
        log::info!("[AsyncJIT] Orchestrator shutdown complete");
    }
}

impl Drop for AsyncJitOrchestrator {
    fn drop(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

/// Unified orchestrator statistics
#[derive(Debug, Clone)]
pub struct OrchestratorStats {
    pub compile: AsyncStatsSnapshot,
    pub eviction: async_eviction::EvictionStatsSnapshot,
    pub restore: async_restore::RestoreStatsSnapshot,
    pub cache_size: u64,
    pub cache_capacity: u64,
}

impl OrchestratorStats {
    /// Cache utilization percentage
    pub fn cache_utilization(&self) -> f64 {
        if self.cache_capacity == 0 {
            return 0.0;
        }
        (self.cache_size as f64 / self.cache_capacity as f64) * 100.0
    }
    
    /// Format summary for logging
    pub fn summary(&self) -> String {
        format!(
            "Cache: {:.1}% ({}/{}MB) | Compile: {}/{} | Evict: {}/{} | Restore: {}/{}",
            self.cache_utilization(),
            self.cache_size / (1024 * 1024),
            self.cache_capacity / (1024 * 1024),
            self.compile.requests_completed,
            self.compile.requests_submitted,
            self.eviction.blocks_persisted,
            self.eviction.blocks_discarded,
            self.restore.restorations_success,
            self.restore.requests_total,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_jit_engine_creation() {
        let engine = JitEngine::new();
        assert!(engine.config.tiered_compilation);
    }
    
    #[test]
    fn test_tier_thresholds() {
        let thresholds = TierThresholds::default();
        assert!(thresholds.interpreter_to_s1 < thresholds.s1_to_s2);
    }
    
    #[test]
    fn test_async_jit_config_default() {
        let config = AsyncJitConfig::default();
        assert_eq!(config.compile_workers, 0);
        assert!(config.enable_prefetch);
        assert!(config.eviction_high_watermark > config.eviction_low_watermark);
    }
    
    #[test]
    fn test_orchestrator_stats_summary() {
        let stats = OrchestratorStats {
            compile: AsyncStatsSnapshot {
                requests_submitted: 100,
                requests_completed: 90,
                requests_failed: 5,
                requests_dropped: 5,
                s1_compilations: 80,
                s2_compilations: 10,
                queue_depth: 0,
                peak_queue_depth: 10,
                active_workers: 0,
                avg_compile_time: std::time::Duration::from_micros(500),
            },
            eviction: async_eviction::EvictionStatsSnapshot {
                cycles_started: 5,
                cycles_completed: 5,
                blocks_persisted: 50,
                blocks_discarded: 30,
                bytes_freed: 5 * 1024 * 1024,
                bytes_to_disk: 3 * 1024 * 1024,
                emergency_evictions: 0,
                persist_errors: 0,
                eviction_time_us: 1000,
            },
            restore: async_restore::RestoreStatsSnapshot {
                requests_total: 20,
                restorations_success: 18,
                restorations_failed: 2,
                native_restored: 15,
                ir_restored: 3,
                prefetch_hits: 5,
                prefetch_misses: 2,
                on_demand_count: 10,
                avg_restore_time: std::time::Duration::from_micros(200),
                queue_depth: 0,
            },
            cache_size: 50 * 1024 * 1024,
            cache_capacity: 100 * 1024 * 1024,
        };
        
        let summary = stats.summary();
        assert!(summary.contains("50.0%"));
        assert!(summary.contains("Compile"));
        assert!(summary.contains("Evict"));
        assert!(summary.contains("Restore"));
    }
}
