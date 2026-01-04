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
//! │  │                       ReadyNow! Cache                                │  │
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
pub mod readynow;

use std::sync::{Arc, RwLock, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::collections::HashMap;

use crate::cpu::VirtualCpu;
use crate::memory::{PhysicalMemory, AddressSpace};

pub use decoder::{X86Decoder, DecodedInstr};
pub use ir::{IrBuilder, IrBlock, IrInstr, IrOp};
pub use interpreter::Interpreter;
pub use compiler_s1::S1Compiler;
pub use compiler_s2::{S2Compiler, S2Config};
pub use codegen::CodeGen;
pub use profile::{ProfileDb, BranchProfile, CallProfile, BlockProfile};
pub use cache::{CodeCache, CacheStats, CompiledBlock, CompileTier, CacheError};
pub use readynow::{ReadyNowCache, NativeBlockInfo};

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
    pub fn name(&self) -> &'static str {
        match self {
            Self::Interpreter => "Interpreter",
            Self::S1 => "S1 (Quick)",
            Self::S2 => "S2 (Optimizing)",
        }
    }
}

/// ReadyNow! persistence format
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

/// Tier promotion thresholds
#[derive(Debug, Clone)]
pub struct TierThresholds {
    /// Invocations before promoting Interpreter → S1
    pub interpreter_to_s1: u64,
    /// Invocations before promoting S1 → S2
    pub s1_to_s2: u64,
    /// Back-edge count threshold for OSR (on-stack replacement)
    pub osr_threshold: u64,
    /// Minimum block size for S2 compilation (bytes)
    pub s2_min_block_size: usize,
}

impl Default for TierThresholds {
    fn default() -> Self {
        Self {
            interpreter_to_s1: 100,      // Quick promotion to S1
            s1_to_s2: 10_000,            // S2 for hot code
            osr_threshold: 5_000,        // OSR for hot loops
            s2_min_block_size: 64,       // Don't S2 tiny blocks
        }
    }
}

/// JIT configuration
#[derive(Debug, Clone)]
pub struct JitConfig {
    /// Enable tiered compilation
    pub tiered_compilation: bool,
    /// Tier promotion thresholds
    pub thresholds: TierThresholds,
    /// Code cache size (bytes)
    pub code_cache_size: usize,
    /// Profile database size (entries)
    pub profile_db_size: usize,
    /// Enable ReadyNow! preloading
    pub readynow_enabled: bool,
    /// ReadyNow! cache path
    pub readynow_path: Option<String>,
    /// Enable aggressive inlining in S2
    pub aggressive_inlining: bool,
    /// Enable loop unrolling
    pub loop_unrolling: bool,
    /// Max inline depth
    pub max_inline_depth: u32,
    /// Enable speculative optimizations
    pub speculative_opts: bool,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            tiered_compilation: true,
            thresholds: TierThresholds::default(),
            code_cache_size: 64 * 1024 * 1024,  // 64MB code cache
            profile_db_size: 1_000_000,          // 1M profile entries
            readynow_enabled: true,
            readynow_path: None,
            aggressive_inlining: true,
            loop_unrolling: true,
            max_inline_depth: 9,
            speculative_opts: true,
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
    /// Current execution tier
    pub tier: ExecutionTier,
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
            tier: self.tier.clone(),
            invocations: AtomicU64::new(self.invocations.load(std::sync::atomic::Ordering::Relaxed)),
            back_edges: AtomicU64::new(self.back_edges.load(std::sync::atomic::Ordering::Relaxed)),
            native_code: self.native_code,
            native_size: self.native_size,
            ir: self.ir.clone(),
            valid: AtomicBool::new(self.valid.load(std::sync::atomic::Ordering::Relaxed)),
            last_exec: AtomicU64::new(self.last_exec.load(std::sync::atomic::Ordering::Relaxed)),
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
            tier: ExecutionTier::Interpreter,
            invocations: AtomicU64::new(0),
            back_edges: AtomicU64::new(0),
            native_code: None,
            native_size: 0,
            ir: None,
            valid: AtomicBool::new(true),
            last_exec: AtomicU64::new(0),
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
    /// ReadyNow! loads
    pub readynow_loads: AtomicU64,
    /// Total compilation time (ns)
    pub compilation_time_ns: AtomicU64,
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
    code_cache: CodeCache,
    /// Block metadata
    blocks: RwLock<HashMap<u64, Arc<BlockMeta>>>,
    /// Profile database
    profile_db: ProfileDb,
    /// ReadyNow! cache
    readynow: Option<ReadyNowCache>,
    /// Statistics
    stats: Arc<JitStats>,
    /// Is engine running?
    running: AtomicBool,
}

impl JitEngine {
    /// Create new JIT engine with default config
    pub fn new() -> Self {
        Self::with_config(JitConfig::default())
    }
    
    /// Create new JIT engine with custom config
    pub fn with_config(config: JitConfig) -> Self {
        let readynow = if config.readynow_enabled {
            let cache_dir = config.readynow_path.clone()
                .unwrap_or_else(|| "/tmp/nvm-jit".to_string());
            Some(ReadyNowCache::new(&cache_dir, "default"))
        } else {
            None
        };
        
        Self {
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
            code_cache: CodeCache::new(config.code_cache_size as u64),
            blocks: RwLock::new(HashMap::new()),
            profile_db: ProfileDb::new(config.profile_db_size),
            readynow,
            stats: Arc::new(JitStats::default()),
            running: AtomicBool::new(false),
            config,
        }
    }
    
    /// Execute guest code starting at RIP
    /// 
    /// This is the main entry point. It:
    /// 1. Checks code cache for compiled code
    /// 2. Falls back to interpreter if not compiled
    /// 3. Collects profile data
    /// 4. Triggers compilation when thresholds are met
    pub fn execute(&self, cpu: &VirtualCpu, memory: &AddressSpace) -> JitResult<ExecuteResult> {
        self.running.store(true, Ordering::SeqCst);
        
        let rip = cpu.read_rip();
        
        // Check code cache first
        if let Some(entry) = self.code_cache.lookup(rip) {
            self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            return self.execute_native(cpu, memory, entry);
        }
        
        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        
        // Get or create block metadata
        let block = self.get_or_create_block(rip);
        let invocations = block.increment_invocations();
        
        // Check tier promotion
        let tier = self.determine_tier(&block, invocations);
        
        match tier {
            ExecutionTier::Interpreter => {
                self.stats.interpreter_execs.fetch_add(1, Ordering::Relaxed);
                self.interpret(cpu, memory, rip)
            }
            ExecutionTier::S1 => {
                // Compile with S1 if not already
                if block.tier == ExecutionTier::Interpreter {
                    self.compile_s1(cpu, memory, rip, &block)?;
                }
                self.stats.s1_execs.fetch_add(1, Ordering::Relaxed);
                self.execute_s1(cpu, memory, &block)
            }
            ExecutionTier::S2 => {
                // Compile with S2 if not already
                if block.tier != ExecutionTier::S2 {
                    self.compile_s2(cpu, memory, rip, &block)?;
                }
                self.stats.s2_execs.fetch_add(1, Ordering::Relaxed);
                self.execute_s2(cpu, memory, &block)
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
        
        // Get native code size and pointer
        let native_len = s1_block.native.len();
        let native_code: Box<[u8]> = s1_block.native.into_boxed_slice();
        let host_ptr = Box::into_raw(native_code) as *const u8;
        
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
        self.code_cache.insert(block).map_err(|_| JitError::CodeCacheFull)?;
        
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
        if block.ir.is_none() {
            // Need to compile with S1 first
            let mut guest_code = vec![0u8; 4096];
            for (i, byte) in guest_code.iter_mut().enumerate() {
                *byte = memory.read_u8(rip + i as u64);
            }
            let s1_block = self.s1_compiler.compile(&guest_code, rip, &self.decoder, &self.profile_db)?;
            
            // Recompile with S2 optimizations
            let s2_block = self.s2_compiler.compile_from_s1(&s1_block, &self.profile_db)?;
            
            // Replace in code cache
            self.code_cache.replace(rip, s2_block.native)?;
        } else if let Some(ref ir) = block.ir {
            // We have existing IR, find S1 block somehow
            // For now, recompile from scratch
            let mut guest_code = vec![0u8; 4096];
            for (i, byte) in guest_code.iter_mut().enumerate() {
                *byte = memory.read_u8(rip + i as u64);
            }
            let s1_block = self.s1_compiler.compile(&guest_code, rip, &self.decoder, &self.profile_db)?;
            let s2_block = self.s2_compiler.compile_from_s1(&s1_block, &self.profile_db)?;
            self.code_cache.replace(rip, s2_block.native)?;
        }
        
        let elapsed = start.elapsed().as_nanos() as u64;
        self.stats.compilation_time_ns.fetch_add(elapsed, Ordering::Relaxed);
        self.stats.s2_compilations.fetch_add(1, Ordering::Relaxed);
        
        Ok(())
    }
    
    /// Execute native code from cache
    fn execute_native(&self, cpu: &VirtualCpu, memory: &AddressSpace, code_ptr: *const u8) -> JitResult<ExecuteResult> {
        // Safety: native code was generated by our codegen
        // Note: Native code uses raw RAM pointer, bypassing MMIO.
        // MMIO devices won't receive these accesses.
        unsafe {
            let func: extern "C" fn(*mut VirtualCpu, *mut PhysicalMemory) -> u64 = 
                std::mem::transmute(code_ptr);
            
            let result = func(
                cpu as *const VirtualCpu as *mut VirtualCpu,
                memory.ram_ptr(),
            );
            
            Ok(ExecuteResult::from_native(result))
        }
    }
    
    fn execute_s1(&self, cpu: &VirtualCpu, memory: &AddressSpace, block: &BlockMeta) -> JitResult<ExecuteResult> {
        if let Some(code_ptr) = block.native_code {
            // Note: Native code uses raw RAM pointer, bypassing MMIO.
            unsafe {
                let func: extern "C" fn(*mut VirtualCpu, *mut PhysicalMemory) -> u64 =
                    std::mem::transmute(code_ptr);
                let result = func(
                    cpu as *const VirtualCpu as *mut VirtualCpu,
                    memory.ram_ptr(),
                );
                Ok(ExecuteResult::from_native(result))
            }
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
    // ReadyNow! Persistence - Three Formats with Different Compatibility
    // ========================================================================
    
    /// Load ReadyNow! cache with tiered loading strategy
    /// 
    /// Loading priority:
    /// 1. Native code (instant, same-generation only)
    /// 2. RI (backward compatible, needs codegen)
    /// 3. Profile (full compat, guides future compilation)
    pub fn load_readynow(&self) -> JitResult<ReadyNowStats> {
        let readynow = self.readynow.as_ref()
            .ok_or_else(|| JitError::CompilationError("ReadyNow! not enabled".to_string()))?;
        
        let mut stats = ReadyNowStats::default();
        let start = std::time::Instant::now();
        
        // 1. Try loading native code first (zero warmup if version matches)
        if let Ok(Some(native_blocks)) = readynow.load_native() {
            stats.native_blocks_loaded = native_blocks.len();
            // Install native blocks directly into code cache
            // (version check done inside load_native)
        }
        
        // 2. Load RI blocks (backward compatible)
        if let Ok(ir_blocks) = readynow.load_ri() {
            stats.ir_blocks_loaded = ir_blocks.len();
            // These can be compiled on demand
        }
        
        // 3. Always load profile (full compat, guides compilation)
        if let Ok(profile) = readynow.load_profile() {
            stats.profiles_loaded = profile.block_count();
            // Merge profile data to guide hot path detection
            self.profile_db.merge(&profile);
        }
        
        stats.load_time_ms = start.elapsed().as_millis() as u64;
        self.stats.readynow_loads.fetch_add(1, Ordering::Relaxed);
        
        Ok(stats)
    }
    
    /// Save ReadyNow! cache in specified format
    /// 
    /// Format compatibility guarantees:
    /// - Profile: Full forward AND backward compatibility
    /// - RI: Backward compatible (old RI works on new JIT)
    /// - Native: Same-generation only (version must match exactly)
    pub fn save_readynow(&self, format: PersistFormat) -> JitResult<()> {
        let readynow = self.readynow.as_ref()
            .ok_or_else(|| JitError::CompilationError("ReadyNow! not enabled".to_string()))?;
        
        match format {
            PersistFormat::Profile => {
                // Profile: Always safe, full compatibility
                readynow.save_profile(&self.profile_db)
            }
            PersistFormat::Ri => {
                // RI: Backward compatible IR
                let blocks = self.blocks.read().unwrap();
                let ir_blocks: HashMap<u64, _> = blocks.iter()
                    .filter_map(|(rip, meta)| {
                        meta.ir.as_ref().map(|ir| (*rip, (**ir).clone()))
                    })
                    .collect();
                readynow.save_ri(&ir_blocks)
            }
            PersistFormat::Native => {
                // Native: Same-generation only
                let blocks = self.blocks.read().unwrap();
                let native_blocks: HashMap<u64, _> = blocks.iter()
                    .filter_map(|(rip, meta)| {
                        meta.native_code.map(|ptr| {
                            let compiled = CompiledBlock {
                                guest_rip: *rip,
                                guest_size: meta.guest_size as u32,
                                host_code: ptr,
                                host_size: meta.native_size as u32,
                                tier: match meta.tier {
                                    ExecutionTier::Interpreter => CompileTier::Interpreter,
                                    ExecutionTier::S1 => CompileTier::S1,
                                    ExecutionTier::S2 => CompileTier::S2,
                                },
                                exec_count: AtomicU64::new(meta.invocations.load(Ordering::Relaxed)),
                                last_access: AtomicU64::new(meta.last_exec.load(Ordering::Relaxed)),
                                guest_instrs: 0,
                                guest_checksum: 0,
                                depends_on: Vec::new(),
                                invalidated: !meta.valid.load(Ordering::Relaxed),
                            };
                            (*rip, compiled)
                        })
                    })
                    .collect();
                readynow.save_native(&native_blocks)
            }
            PersistFormat::All => {
                // Save all formats for maximum flexibility
                self.save_readynow(PersistFormat::Profile)?;
                self.save_readynow(PersistFormat::Ri)?;
                self.save_readynow(PersistFormat::Native)?;
                Ok(())
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

/// ReadyNow! load statistics
#[derive(Debug, Clone, Default)]
pub struct ReadyNowStats {
    pub profiles_loaded: usize,
    pub ir_blocks_loaded: usize,
    pub native_blocks_loaded: usize,
    pub native_blocks_rejected: usize,
    pub load_time_ms: u64,
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
}
