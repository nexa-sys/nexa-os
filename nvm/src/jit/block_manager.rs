//! Unified Block Manager
//!
//! Provides a unified interface for managing compiled blocks across S1/S2 compilers.
//! Handles:
//! - Tier-aware compilation and recompilation
//! - ISA-aware native code generation
//! - Consistent block storage format
//!
//! ## Block Lifecycle
//!
//! ```text
//! Guest Code
//!     │
//!     ▼
//! ┌─────────┐    ┌─────────┐    ┌─────────┐
//! │ Decoder │───▶│   IR    │───▶│ Native  │
//! └─────────┘    └────┬────┘    └─────────┘
//!                     │              │
//!                     ▼              ▼
//!                 Save IR       Save Native
//!               (universal)   (tier + ISA)
//! ```
//!
//! ## Recompilation Flow
//!
//! When native code is stale (JIT version mismatch or ISA unavailable):
//! 1. Load IR from cache
//! 2. Select compiler based on stored tier (S1 or S2)
//! 3. Generate native code with current ISA optimization
//! 4. Save new native code with updated ISA requirements
//!
//! ## ISA-Aware Code Generation
//!
//! The block manager analyzes IR to determine which instruction sets are needed,
//! and generates optimized code paths based on available ISA:
//!
//! | IR Operation | Baseline (SSE2) | With POPCNT | With BMI1/2 | With AVX |
//! |--------------|-----------------|-------------|-------------|----------|
//! | Popcnt       | Lookup table    | POPCNT      | POPCNT      | POPCNT   |
//! | Lzcnt        | BSR + adjust    | LZCNT       | LZCNT       | LZCNT    |
//! | Bextr        | Shift + mask    | Shift+mask  | BEXTR       | BEXTR    |
//! | VectorOp     | 128-bit SSE     | 128-bit SSE | 128-bit SSE | 256-bit  |

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrOp, IrInstr, VReg, IrFlags, BlockId, VectorOpKind};
use super::cache::CompileTier;
use super::nready::{InstructionSets, NReadyCache, NativeBlockHeader, NativeBlockInfo};
use super::compiler_s1::S1Compiler;
use super::compiler_s2::S2Compiler;
use super::profile::ProfileDb;

// ============================================================================
// ISA-Aware Codegen Configuration
// ============================================================================

/// ISA-aware codegen configuration
#[derive(Clone, Debug)]
pub struct IsaCodegenConfig {
    /// Target instruction sets to use (subset of available)
    pub target_isa: InstructionSets,
    /// Enable SIMD optimizations if AVX2+ available
    pub enable_simd: bool,
    /// Enable BMI optimizations if available
    pub enable_bmi: bool,
    /// Enable POPCNT/LZCNT if available
    pub enable_bit_ops: bool,
    /// Enable FMA if available
    pub enable_fma: bool,
    /// Enable AES-NI if available
    pub enable_aes: bool,
}

impl Default for IsaCodegenConfig {
    fn default() -> Self {
        Self {
            target_isa: InstructionSets::SSE2, // Baseline only
            enable_simd: false,
            enable_bmi: false,
            enable_bit_ops: false,
            enable_fma: false,
            enable_aes: false,
        }
    }
}

impl IsaCodegenConfig {
    /// Create config targeting current CPU's instruction sets
    pub fn for_current_cpu() -> Self {
        let available = InstructionSets::detect_current();
        Self {
            target_isa: available,
            enable_simd: available.contains(InstructionSets::AVX2),
            enable_bmi: available.contains(InstructionSets::BMI1),
            enable_bit_ops: available.contains(InstructionSets::POPCNT) 
                         || available.contains(InstructionSets::LZCNT),
            enable_fma: available.contains(InstructionSets::FMA),
            enable_aes: available.contains(InstructionSets::AESNI),
        }
    }
    
    /// Create config with specific target ISA
    pub fn with_target(isa: InstructionSets) -> Self {
        Self {
            target_isa: isa,
            enable_simd: isa.contains(InstructionSets::AVX2),
            enable_bmi: isa.contains(InstructionSets::BMI1),
            enable_bit_ops: isa.contains(InstructionSets::POPCNT) 
                         || isa.contains(InstructionSets::LZCNT),
            enable_fma: isa.contains(InstructionSets::FMA),
            enable_aes: isa.contains(InstructionSets::AESNI),
        }
    }
    
    /// Check if a specific ISA feature is available
    pub fn has_feature(&self, feature: InstructionSets) -> bool {
        self.target_isa.contains(feature)
    }
}

// ============================================================================
// ISA Upgrade Evaluation System
// ============================================================================

/// ISA operation category for performance modeling
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IsaOpCategory {
    /// Population count (POPCNT instruction)
    Popcnt,
    /// Leading/trailing zero count (LZCNT/TZCNT)
    BitCount,
    /// Bit scan (BSF/BSR)
    BitScan,
    /// Bit field extract (BEXTR - BMI1)
    BitExtract,
    /// Parallel bit deposit/extract (PDEP/PEXT - BMI2)
    ParallelBits,
    /// Fused multiply-add (FMA)
    Fma,
    /// AES encryption/decryption (AES-NI)
    Aes,
    /// Carryless multiply (PCLMULQDQ)
    Pclmul,
    /// 128-bit vector operations (SSE)
    Vector128,
    /// 256-bit vector operations (AVX/AVX2)
    Vector256,
    /// 512-bit vector operations (AVX-512)
    Vector512,
    /// Scalar integer operations
    ScalarInt,
    /// Scalar floating point
    ScalarFp,
    /// Memory operations
    Memory,
    /// Control flow
    Control,
}

/// Performance characteristics for an operation under different ISA levels
#[derive(Clone, Copy, Debug)]
pub struct OpPerfProfile {
    /// Latency in cycles (approximate)
    pub latency: u8,
    /// Throughput (ops per cycle, scaled by 10 for precision)
    pub throughput_x10: u8,
    /// Code size in bytes (u16 to handle large software fallbacks like AES)
    pub code_size: u16,
}

impl OpPerfProfile {
    const fn new(latency: u8, throughput_x10: u8, code_size: u16) -> Self {
        Self { latency, throughput_x10, code_size }
    }
}

/// ISA Performance Model
/// 
/// Contains performance profiles for all operation categories across ISA levels.
/// Data sourced from Intel/AMD optimization manuals and Agner Fog's tables.
pub struct IsaPerfModel;

impl IsaPerfModel {
    /// Get performance profile for an operation category at given ISA level
    /// 
    /// Returns (software_fallback_profile, hardware_profile_if_available)
    pub fn get_profile(category: IsaOpCategory, isa: InstructionSets) -> (OpPerfProfile, Option<OpPerfProfile>) {
        match category {
            // POPCNT: Software = ~15 cycles, Hardware = 3 cycles
            IsaOpCategory::Popcnt => {
                let software = OpPerfProfile::new(15, 5, 45);  // Parallel bit count
                let hardware = if isa.contains(InstructionSets::POPCNT) {
                    Some(OpPerfProfile::new(3, 10, 5))  // Single instruction
                } else {
                    None
                };
                (software, hardware)
            }
            
            // LZCNT/TZCNT: Software BSR/BSF+adjust = ~5 cycles, Hardware = 3 cycles  
            IsaOpCategory::BitCount => {
                let software = OpPerfProfile::new(5, 8, 12);
                let hardware = if isa.contains(InstructionSets::LZCNT) || isa.contains(InstructionSets::BMI1) {
                    Some(OpPerfProfile::new(3, 10, 5))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // BSF/BSR: Always available, but TZCNT/LZCNT are faster on newer CPUs
            IsaOpCategory::BitScan => {
                let baseline = OpPerfProfile::new(3, 10, 4);
                (baseline, Some(baseline))
            }
            
            // BEXTR: Software shift+mask = ~4 cycles, BMI1 = 2 cycles
            IsaOpCategory::BitExtract => {
                let software = OpPerfProfile::new(4, 8, 10);
                let hardware = if isa.contains(InstructionSets::BMI1) {
                    Some(OpPerfProfile::new(2, 10, 6))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // PDEP/PEXT: Software loop = ~30 cycles, BMI2 = 3 cycles
            IsaOpCategory::ParallelBits => {
                let software = OpPerfProfile::new(30, 2, 60);  // Loop-based
                let hardware = if isa.contains(InstructionSets::BMI2) {
                    Some(OpPerfProfile::new(3, 10, 6))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // FMA: Software mul+add = 8 cycles, FMA = 4 cycles (also better precision)
            IsaOpCategory::Fma => {
                let software = OpPerfProfile::new(8, 5, 10);
                let hardware = if isa.contains(InstructionSets::FMA) {
                    Some(OpPerfProfile::new(4, 10, 5))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // AES: Software = ~200 cycles/round, AES-NI = 4 cycles/round
            IsaOpCategory::Aes => {
                let software = OpPerfProfile::new(200, 1, 500);  // Table lookup implementation
                let hardware = if isa.contains(InstructionSets::AESNI) {
                    Some(OpPerfProfile::new(4, 10, 6))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // PCLMUL: Software = ~50 cycles, Hardware = 7 cycles
            IsaOpCategory::Pclmul => {
                let software = OpPerfProfile::new(50, 2, 80);
                let hardware = if isa.contains(InstructionSets::PCLMUL) {
                    Some(OpPerfProfile::new(7, 5, 6))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // Vector 128-bit: SSE2 baseline
            IsaOpCategory::Vector128 => {
                let profile = OpPerfProfile::new(4, 10, 6);
                (profile, Some(profile))
            }
            
            // Vector 256-bit: Need AVX/AVX2, otherwise split into 2x128
            IsaOpCategory::Vector256 => {
                let software = OpPerfProfile::new(8, 5, 12);  // 2x SSE ops
                let hardware = if isa.contains(InstructionSets::AVX) {
                    Some(OpPerfProfile::new(4, 10, 6))  // Single 256-bit op
                } else {
                    None
                };
                (software, hardware)
            }
            
            // Vector 512-bit: Need AVX-512, otherwise split into 2x256 or 4x128
            IsaOpCategory::Vector512 => {
                let software = if isa.contains(InstructionSets::AVX) {
                    OpPerfProfile::new(8, 5, 12)   // 2x AVX ops
                } else {
                    OpPerfProfile::new(16, 3, 24) // 4x SSE ops
                };
                let hardware = if isa.contains(InstructionSets::AVX512F) {
                    Some(OpPerfProfile::new(4, 10, 8))
                } else {
                    None
                };
                (software, hardware)
            }
            
            // Scalar operations - baseline
            IsaOpCategory::ScalarInt | IsaOpCategory::ScalarFp => {
                let profile = OpPerfProfile::new(1, 40, 4);
                (profile, Some(profile))
            }
            
            IsaOpCategory::Memory => {
                let profile = OpPerfProfile::new(4, 20, 5);
                (profile, Some(profile))
            }
            
            IsaOpCategory::Control => {
                let profile = OpPerfProfile::new(1, 20, 5);
                (profile, Some(profile))
            }
        }
    }
    
    /// Calculate speedup factor for upgrading from one ISA to another
    /// 
    /// Returns speedup multiplier (1.0 = no change, 2.0 = 2x faster)
    pub fn calculate_speedup(category: IsaOpCategory, from_isa: InstructionSets, to_isa: InstructionSets) -> f32 {
        let (from_sw, from_hw) = Self::get_profile(category, from_isa);
        let (_, to_hw) = Self::get_profile(category, to_isa);
        
        let from_latency = from_hw.map(|p| p.latency).unwrap_or(from_sw.latency) as f32;
        let to_latency = to_hw.map(|p| p.latency).unwrap_or(from_sw.latency) as f32;
        
        if to_latency > 0.0 {
            from_latency / to_latency
        } else {
            1.0
        }
    }
}

/// IR Operation Statistics
/// 
/// Counts of each operation category in an IR block.
#[derive(Clone, Debug, Default)]
pub struct IrOpStats {
    pub popcnt: u32,
    pub bit_count: u32,
    pub bit_scan: u32,
    pub bit_extract: u32,
    pub parallel_bits: u32,
    pub fma: u32,
    pub aes: u32,
    pub pclmul: u32,
    pub vector_128: u32,
    pub vector_256: u32,
    pub vector_512: u32,
    pub scalar_int: u32,
    pub scalar_fp: u32,
    pub memory: u32,
    pub control: u32,
}

impl IrOpStats {
    /// Analyze an IR block and count operations by category
    pub fn from_ir(ir: &IrBlock) -> Self {
        let mut stats = Self::default();
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                stats.count_op(&instr.op);
            }
        }
        
        stats
    }
    
    fn count_op(&mut self, op: &IrOp) {
        match op {
            IrOp::Popcnt(_) => self.popcnt += 1,
            IrOp::Lzcnt(_) | IrOp::Tzcnt(_) => self.bit_count += 1,
            IrOp::Bsf(_) | IrOp::Bsr(_) => self.bit_scan += 1,
            IrOp::Bextr(_, _, _) => self.bit_extract += 1,
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) => self.parallel_bits += 1,
            IrOp::Fma(_, _, _) => self.fma += 1,
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => self.aes += 1,
            IrOp::Pclmul(_, _, _) => self.pclmul += 1,
            
            IrOp::VectorOp { width, .. } => {
                match width {
                    0..=128 => self.vector_128 += 1,
                    129..=256 => self.vector_256 += 1,
                    _ => self.vector_512 += 1,
                }
            }
            
            // Scalar integer
            IrOp::Add(_, _) | IrOp::Sub(_, _) | IrOp::Mul(_, _) | IrOp::IMul(_, _) |
            IrOp::Div(_, _) | IrOp::IDiv(_, _) | IrOp::And(_, _) | IrOp::Or(_, _) |
            IrOp::Xor(_, _) | IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) |
            IrOp::Rol(_, _) | IrOp::Ror(_, _) | IrOp::Neg(_) | IrOp::Not(_) |
            IrOp::Cmp(_, _) | IrOp::Test(_, _) | IrOp::Select(_, _, _) |
            IrOp::Const(_) | IrOp::LoadGpr(_) | IrOp::StoreGpr(_, _) |
            IrOp::Sext8(_) | IrOp::Sext16(_) | IrOp::Sext32(_) |
            IrOp::Zext8(_) | IrOp::Zext16(_) | IrOp::Zext32(_) |
            IrOp::Trunc8(_) | IrOp::Trunc16(_) | IrOp::Trunc32(_) |
            IrOp::GetCF(_) | IrOp::GetZF(_) | IrOp::GetSF(_) | 
            IrOp::GetOF(_) | IrOp::GetPF(_) => self.scalar_int += 1,
            
            // Scalar FP
            IrOp::ConstF64(_) => self.scalar_fp += 1,
            
            // Memory
            IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_) |
            IrOp::Store8(_, _) | IrOp::Store16(_, _) | IrOp::Store32(_, _) | IrOp::Store64(_, _) => {
                self.memory += 1;
            }
            
            // Control
            IrOp::Jump(_) | IrOp::Branch(_, _, _) | IrOp::Call(_) | IrOp::CallIndirect(_) |
            IrOp::Ret | IrOp::Exit(_) => self.control += 1,
            
            // Misc (no category impact)
            _ => {}
        }
    }
    
    /// Get count for a specific category
    pub fn get(&self, category: IsaOpCategory) -> u32 {
        match category {
            IsaOpCategory::Popcnt => self.popcnt,
            IsaOpCategory::BitCount => self.bit_count,
            IsaOpCategory::BitScan => self.bit_scan,
            IsaOpCategory::BitExtract => self.bit_extract,
            IsaOpCategory::ParallelBits => self.parallel_bits,
            IsaOpCategory::Fma => self.fma,
            IsaOpCategory::Aes => self.aes,
            IsaOpCategory::Pclmul => self.pclmul,
            IsaOpCategory::Vector128 => self.vector_128,
            IsaOpCategory::Vector256 => self.vector_256,
            IsaOpCategory::Vector512 => self.vector_512,
            IsaOpCategory::ScalarInt => self.scalar_int,
            IsaOpCategory::ScalarFp => self.scalar_fp,
            IsaOpCategory::Memory => self.memory,
            IsaOpCategory::Control => self.control,
        }
    }
    
    /// Total operation count
    pub fn total(&self) -> u32 {
        self.popcnt + self.bit_count + self.bit_scan + self.bit_extract +
        self.parallel_bits + self.fma + self.aes + self.pclmul +
        self.vector_128 + self.vector_256 + self.vector_512 +
        self.scalar_int + self.scalar_fp + self.memory + self.control
    }
    
    /// Categories that benefit from ISA upgrades (non-baseline operations)
    pub fn isa_sensitive_ops(&self) -> u32 {
        self.popcnt + self.bit_count + self.bit_extract + self.parallel_bits +
        self.fma + self.aes + self.pclmul + self.vector_256 + self.vector_512
    }
}

/// ISA Upgrade Decision
#[derive(Clone, Debug)]
pub struct UpgradeDecision {
    /// Whether to upgrade
    pub should_upgrade: bool,
    /// Estimated speedup factor (1.0 = no change)
    pub estimated_speedup: f32,
    /// Estimated cycles saved per execution
    pub cycles_saved: u64,
    /// New ISA features that would be utilized
    pub new_features: InstructionSets,
    /// Reason for decision
    pub reason: UpgradeReason,
}

#[derive(Clone, Debug)]
pub enum UpgradeReason {
    /// No ISA-sensitive operations in block
    NoSensitiveOps,
    /// Block is too cold (low execution count)
    TooCold { exec_count: u64, threshold: u64 },
    /// Speedup too small to justify recompilation
    SpeedupTooSmall { speedup: f32, threshold: f32 },
    /// Worth upgrading for performance
    WorthUpgrading { dominant_category: IsaOpCategory },
    /// Critical feature missing (e.g., AES-NI for crypto code)
    CriticalFeature { feature: IsaOpCategory },
}

/// ISA Upgrade Evaluator
/// 
/// Determines whether recompiling a block with newer ISA features is worthwhile.
pub struct IsaUpgradeEvaluator {
    /// Current CPU's ISA capabilities
    available_isa: InstructionSets,
    /// Minimum execution count to consider upgrade
    hot_threshold: u64,
    /// Minimum speedup factor to justify recompilation  
    min_speedup: f32,
    /// Recompilation cost in equivalent cycles
    recompile_cost: u64,
}

impl Default for IsaUpgradeEvaluator {
    fn default() -> Self {
        Self {
            available_isa: InstructionSets::detect_current(),
            hot_threshold: 100,        // Block must execute 100+ times
            min_speedup: 1.1,          // Need 10%+ speedup
            recompile_cost: 10_000,    // ~10k cycles to recompile
        }
    }
}

impl IsaUpgradeEvaluator {
    pub fn new(available_isa: InstructionSets) -> Self {
        Self {
            available_isa,
            ..Default::default()
        }
    }
    
    /// Configure thresholds
    pub fn with_thresholds(mut self, hot_threshold: u64, min_speedup: f32) -> Self {
        self.hot_threshold = hot_threshold;
        self.min_speedup = min_speedup;
        self
    }
    
    /// Evaluate whether a block should be recompiled with better ISA
    pub fn evaluate(&self, block: &UnifiedBlock) -> UpgradeDecision {
        let current_isa = block.required_isa;
        let target_isa = self.available_isa;
        
        // Already at best available ISA
        if current_isa == target_isa || !target_isa.contains(current_isa) {
            return UpgradeDecision {
                should_upgrade: false,
                estimated_speedup: 1.0,
                cycles_saved: 0,
                new_features: InstructionSets::empty(),
                reason: UpgradeReason::SpeedupTooSmall { 
                    speedup: 1.0, 
                    threshold: self.min_speedup 
                },
            };
        }
        
        // Analyze IR operations
        let stats = IrOpStats::from_ir(&block.ir);
        
        // No ISA-sensitive operations
        if stats.isa_sensitive_ops() == 0 {
            return UpgradeDecision {
                should_upgrade: false,
                estimated_speedup: 1.0,
                cycles_saved: 0,
                new_features: InstructionSets::empty(),
                reason: UpgradeReason::NoSensitiveOps,
            };
        }
        
        // Check hotness (but always upgrade if critical feature missing)
        let has_critical = self.has_critical_feature(&stats, current_isa, target_isa);
        
        if block.exec_count < self.hot_threshold && !has_critical {
            return UpgradeDecision {
                should_upgrade: false,
                estimated_speedup: 1.0,
                cycles_saved: 0,
                new_features: target_isa - current_isa,
                reason: UpgradeReason::TooCold { 
                    exec_count: block.exec_count, 
                    threshold: self.hot_threshold 
                },
            };
        }
        
        // Calculate detailed speedup
        let (speedup, cycles_saved, dominant) = self.calculate_benefit(&stats, current_isa, target_isa);
        let new_features = target_isa - current_isa;
        
        // Critical features always upgrade (AES, PCLMUL for crypto)
        if has_critical {
            if let Some(critical_cat) = self.find_critical_category(&stats, current_isa, target_isa) {
                return UpgradeDecision {
                    should_upgrade: true,
                    estimated_speedup: speedup,
                    cycles_saved,
                    new_features,
                    reason: UpgradeReason::CriticalFeature { feature: critical_cat },
                };
            }
        }
        
        // Check if speedup justifies recompilation cost
        let break_even_execs = if cycles_saved > 0 {
            self.recompile_cost / cycles_saved
        } else {
            u64::MAX
        };
        
        // Will this pay off within reasonable future executions?
        let expected_future_execs = block.exec_count * 10;  // Assume 10x more execs
        let worth_it = speedup >= self.min_speedup && break_even_execs < expected_future_execs;
        
        UpgradeDecision {
            should_upgrade: worth_it,
            estimated_speedup: speedup,
            cycles_saved,
            new_features,
            reason: if worth_it {
                UpgradeReason::WorthUpgrading { dominant_category: dominant }
            } else {
                UpgradeReason::SpeedupTooSmall { 
                    speedup, 
                    threshold: self.min_speedup 
                }
            },
        }
    }
    
    /// Check if block uses a "critical" feature that has massive speedup
    fn has_critical_feature(&self, stats: &IrOpStats, current: InstructionSets, target: InstructionSets) -> bool {
        // AES-NI: 50x speedup for crypto
        if stats.aes > 0 && !current.contains(InstructionSets::AESNI) && target.contains(InstructionSets::AESNI) {
            return true;
        }
        
        // PDEP/PEXT: 10x speedup for bit manipulation
        if stats.parallel_bits > 0 && !current.contains(InstructionSets::BMI2) && target.contains(InstructionSets::BMI2) {
            return true;
        }
        
        // PCLMUL: 7x speedup for CRC/GF operations
        if stats.pclmul > 0 && !current.contains(InstructionSets::PCLMUL) && target.contains(InstructionSets::PCLMUL) {
            return true;
        }
        
        false
    }
    
    fn find_critical_category(&self, stats: &IrOpStats, current: InstructionSets, target: InstructionSets) -> Option<IsaOpCategory> {
        if stats.aes > 0 && !current.contains(InstructionSets::AESNI) && target.contains(InstructionSets::AESNI) {
            return Some(IsaOpCategory::Aes);
        }
        if stats.parallel_bits > 0 && !current.contains(InstructionSets::BMI2) && target.contains(InstructionSets::BMI2) {
            return Some(IsaOpCategory::ParallelBits);
        }
        if stats.pclmul > 0 && !current.contains(InstructionSets::PCLMUL) && target.contains(InstructionSets::PCLMUL) {
            return Some(IsaOpCategory::Pclmul);
        }
        None
    }
    
    /// Calculate total benefit of ISA upgrade
    fn calculate_benefit(&self, stats: &IrOpStats, from: InstructionSets, to: InstructionSets) 
        -> (f32, u64, IsaOpCategory) 
    {
        let categories = [
            IsaOpCategory::Popcnt,
            IsaOpCategory::BitCount,
            IsaOpCategory::BitExtract,
            IsaOpCategory::ParallelBits,
            IsaOpCategory::Fma,
            IsaOpCategory::Aes,
            IsaOpCategory::Pclmul,
            IsaOpCategory::Vector256,
            IsaOpCategory::Vector512,
        ];
        
        let mut total_from_cycles = 0u64;
        let mut total_to_cycles = 0u64;
        let mut max_savings = 0u64;
        let mut dominant = IsaOpCategory::ScalarInt;
        
        for cat in categories {
            let count = stats.get(cat) as u64;
            if count == 0 {
                continue;
            }
            
            let (from_sw, from_hw) = IsaPerfModel::get_profile(cat, from);
            let (to_sw, to_hw) = IsaPerfModel::get_profile(cat, to);
            
            let from_latency = from_hw.map(|p| p.latency).unwrap_or(from_sw.latency) as u64;
            let to_latency = to_hw.map(|p| p.latency).unwrap_or(to_sw.latency) as u64;
            
            let from_total = count * from_latency;
            let to_total = count * to_latency;
            
            total_from_cycles += from_total;
            total_to_cycles += to_total;
            
            let savings = from_total.saturating_sub(to_total);
            if savings > max_savings {
                max_savings = savings;
                dominant = cat;
            }
        }
        
        // Add baseline ops (no change)
        let baseline_cycles = (stats.scalar_int + stats.scalar_fp + stats.memory + stats.control + stats.vector_128) as u64;
        total_from_cycles += baseline_cycles;
        total_to_cycles += baseline_cycles;
        
        let speedup = if total_to_cycles > 0 {
            total_from_cycles as f32 / total_to_cycles as f32
        } else {
            1.0
        };
        
        let cycles_saved = total_from_cycles.saturating_sub(total_to_cycles);
        
        (speedup, cycles_saved, dominant)
    }
    
    /// Get recommended ISA level for a block based on its operations
    pub fn recommend_isa(&self, stats: &IrOpStats) -> InstructionSets {
        let mut recommended = InstructionSets::SSE2;
        
        if stats.popcnt > 0 {
            recommended |= InstructionSets::POPCNT;
        }
        
        if stats.bit_count > 0 {
            recommended |= InstructionSets::LZCNT | InstructionSets::BMI1;
        }
        
        if stats.bit_extract > 0 {
            recommended |= InstructionSets::BMI1;
        }
        
        if stats.parallel_bits > 0 {
            recommended |= InstructionSets::BMI2;
        }
        
        if stats.fma > 0 {
            recommended |= InstructionSets::FMA;
        }
        
        if stats.aes > 0 {
            recommended |= InstructionSets::AESNI;
        }
        
        if stats.pclmul > 0 {
            recommended |= InstructionSets::PCLMUL;
        }
        
        if stats.vector_256 > 0 {
            recommended |= InstructionSets::AVX | InstructionSets::AVX2;
        }
        
        if stats.vector_512 > 0 {
            recommended |= InstructionSets::AVX512F;
        }
        
        // Only return features actually available
        recommended & self.available_isa
    }
}

// ============================================================================
// Unified Block Representation
// ============================================================================

/// Unified block representation
/// 
/// Contains all information needed to manage a block across tiers.
#[derive(Clone, Debug)]
pub struct UnifiedBlock {
    /// Guest RIP
    pub guest_rip: u64,
    /// Guest code size
    pub guest_size: u32,
    /// Guest instruction count
    pub guest_instrs: u32,
    /// Guest code checksum
    pub guest_checksum: u64,
    /// IR representation (platform-neutral)
    pub ir: IrBlock,
    /// Native code (platform-specific)
    pub native_code: Vec<u8>,
    /// Compilation tier (S1 or S2)
    pub tier: CompileTier,
    /// Required instruction sets for native code
    pub required_isa: InstructionSets,
    /// Execution count
    pub exec_count: u64,
}

impl UnifiedBlock {
    /// Create from S1 compiled block
    pub fn from_s1(s1: &super::compiler_s1::S1Block, required_isa: InstructionSets) -> Self {
        Self {
            guest_rip: s1.guest_rip,
            guest_size: s1.guest_size,
            guest_instrs: s1.ir.blocks.iter().map(|b| b.instrs.len()).sum::<usize>() as u32,
            guest_checksum: 0, // Will be computed when saving
            ir: s1.ir.clone(),
            native_code: s1.native.clone(),
            tier: CompileTier::S1,
            required_isa,
            exec_count: 0,
        }
    }
    
    /// Create from S2 compiled block
    pub fn from_s2(s2: &super::compiler_s2::S2Block, required_isa: InstructionSets) -> Self {
        Self {
            guest_rip: s2.guest_rip,
            guest_size: s2.guest_size,
            guest_instrs: s2.ir.blocks.iter().map(|b| b.instrs.len()).sum::<usize>() as u32,
            guest_checksum: 0,
            ir: s2.ir.clone(),
            native_code: s2.native.clone(),
            tier: CompileTier::S2,
            required_isa,
            exec_count: 0,
        }
    }
}

// ============================================================================
// ISA-Aware Code Generator
// ============================================================================

/// ISA-aware native code generator
/// 
/// Generates optimized machine code based on available instruction sets.
/// This is separate from S1/S2 compilers - it handles the final codegen
/// with ISA-specific optimizations.
pub struct IsaCodeGen {
    config: IsaCodegenConfig,
}

impl IsaCodeGen {
    pub fn new(config: IsaCodegenConfig) -> Self {
        Self { config }
    }
    
    /// Generate ISA-optimized native code for a POPCNT operation
    /// 
    /// If POPCNT ISA available: emit POPCNT instruction
    /// Otherwise: emit software lookup table implementation
    pub fn emit_popcnt(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8) -> InstructionSets {
        if self.config.has_feature(InstructionSets::POPCNT) {
            // POPCNT r64, r64: F3 REX.W 0F B8 /r
            let rex = 0x48 | ((dst_reg >> 3) << 2) | (src_reg >> 3);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[0xF3, rex, 0x0F, 0xB8, modrm]);
            InstructionSets::POPCNT
        } else {
            // Software implementation using parallel bit count
            // This is the standard "divide and conquer" algorithm
            self.emit_popcnt_software(code, dst_reg, src_reg);
            InstructionSets::SSE2
        }
    }
    
    /// Software POPCNT implementation (baseline SSE2)
    /// 
    /// Uses the parallel counting algorithm:
    /// ```text
    /// x = x - ((x >> 1) & 0x5555555555555555)
    /// x = (x & 0x3333333333333333) + ((x >> 2) & 0x3333333333333333)
    /// x = (x + (x >> 4)) & 0x0F0F0F0F0F0F0F0F
    /// x = (x * 0x0101010101010101) >> 56
    /// ```
    fn emit_popcnt_software(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8) {
        // For simplicity, use RAX as temp register (caller must save if needed)
        // mov rax, src
        if src_reg != 0 {
            let rex = 0x48 | (src_reg >> 3);
            let modrm = 0xC0 | (src_reg & 7);
            code.extend_from_slice(&[rex, 0x8B, modrm]); // mov rax, src
        }
        
        // Step 1: x = x - ((x >> 1) & 0x5555555555555555)
        // mov rdx, rax
        code.extend_from_slice(&[0x48, 0x89, 0xC2]);
        // shr rdx, 1
        code.extend_from_slice(&[0x48, 0xD1, 0xEA]);
        // mov rcx, 0x5555555555555555
        code.extend_from_slice(&[0x48, 0xB9]);
        code.extend_from_slice(&0x5555555555555555u64.to_le_bytes());
        // and rdx, rcx
        code.extend_from_slice(&[0x48, 0x21, 0xCA]);
        // sub rax, rdx
        code.extend_from_slice(&[0x48, 0x29, 0xD0]);
        
        // Step 2: x = (x & 0x3333333333333333) + ((x >> 2) & 0x3333333333333333)
        // mov rdx, rax
        code.extend_from_slice(&[0x48, 0x89, 0xC2]);
        // shr rdx, 2
        code.extend_from_slice(&[0x48, 0xC1, 0xEA, 0x02]);
        // mov rcx, 0x3333333333333333
        code.extend_from_slice(&[0x48, 0xB9]);
        code.extend_from_slice(&0x3333333333333333u64.to_le_bytes());
        // and rax, rcx
        code.extend_from_slice(&[0x48, 0x21, 0xC8]);
        // and rdx, rcx
        code.extend_from_slice(&[0x48, 0x21, 0xCA]);
        // add rax, rdx
        code.extend_from_slice(&[0x48, 0x01, 0xD0]);
        
        // Step 3: x = (x + (x >> 4)) & 0x0F0F0F0F0F0F0F0F
        // mov rdx, rax
        code.extend_from_slice(&[0x48, 0x89, 0xC2]);
        // shr rdx, 4
        code.extend_from_slice(&[0x48, 0xC1, 0xEA, 0x04]);
        // add rax, rdx
        code.extend_from_slice(&[0x48, 0x01, 0xD0]);
        // mov rcx, 0x0F0F0F0F0F0F0F0F
        code.extend_from_slice(&[0x48, 0xB9]);
        code.extend_from_slice(&0x0F0F0F0F0F0F0F0Fu64.to_le_bytes());
        // and rax, rcx
        code.extend_from_slice(&[0x48, 0x21, 0xC8]);
        
        // Step 4: x = (x * 0x0101010101010101) >> 56
        // mov rcx, 0x0101010101010101
        code.extend_from_slice(&[0x48, 0xB9]);
        code.extend_from_slice(&0x0101010101010101u64.to_le_bytes());
        // imul rax, rcx
        code.extend_from_slice(&[0x48, 0x0F, 0xAF, 0xC1]);
        // shr rax, 56
        code.extend_from_slice(&[0x48, 0xC1, 0xE8, 0x38]);
        
        // mov dst, rax (if dst != rax)
        if dst_reg != 0 {
            let rex = 0x48 | ((dst_reg >> 3) << 2);
            let modrm = 0xC0 | ((dst_reg & 7) << 3);
            code.extend_from_slice(&[rex, 0x89, modrm]);
        }
    }
    
    /// Generate ISA-optimized native code for LZCNT operation
    pub fn emit_lzcnt(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8) -> InstructionSets {
        if self.config.has_feature(InstructionSets::LZCNT) {
            // LZCNT r64, r64: F3 REX.W 0F BD /r
            let rex = 0x48 | ((dst_reg >> 3) << 2) | (src_reg >> 3);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[0xF3, rex, 0x0F, 0xBD, modrm]);
            InstructionSets::LZCNT
        } else {
            // Fallback: BSR + XOR 63 (BSR gives position of MSB, we want count of leading zeros)
            // Handle zero case first
            // test src, src
            let rex_test = 0x48 | ((src_reg >> 3) << 2) | (src_reg >> 3);
            let modrm_test = 0xC0 | ((src_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex_test, 0x85, modrm_test]);
            
            // jnz not_zero (skip the mov 64)
            code.extend_from_slice(&[0x75, 0x07]); // jnz +7
            
            // mov dst, 64 (for zero input)
            let rex_mov = 0x48 | (dst_reg >> 3);
            code.extend_from_slice(&[rex_mov, 0xC7, 0xC0 | (dst_reg & 7)]);
            code.extend_from_slice(&64u32.to_le_bytes());
            // jmp end
            code.extend_from_slice(&[0xEB, 0x08]); // jmp +8
            
            // not_zero:
            // BSR r64, r64: REX.W 0F BD /r
            let rex = 0x48 | ((dst_reg >> 3) << 2) | (src_reg >> 3);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex, 0x0F, 0xBD, modrm]);
            
            // XOR dst, 63 to convert BSR result to LZCNT
            let rex_xor = 0x48 | (dst_reg >> 3);
            let modrm_xor = 0xF0 | (dst_reg & 7);
            code.extend_from_slice(&[rex_xor, 0x83, modrm_xor, 0x3F]);
            // end:
            
            InstructionSets::SSE2
        }
    }
    
    /// Generate ISA-optimized native code for TZCNT operation
    pub fn emit_tzcnt(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8) -> InstructionSets {
        if self.config.has_feature(InstructionSets::BMI1) {
            // TZCNT r64, r64: F3 REX.W 0F BC /r
            let rex = 0x48 | ((dst_reg >> 3) << 2) | (src_reg >> 3);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[0xF3, rex, 0x0F, 0xBC, modrm]);
            InstructionSets::BMI1
        } else {
            // Fallback: BSF with zero handling
            // test src, src
            let rex_test = 0x48 | ((src_reg >> 3) << 2) | (src_reg >> 3);
            let modrm_test = 0xC0 | ((src_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex_test, 0x85, modrm_test]);
            
            // jnz not_zero
            code.extend_from_slice(&[0x75, 0x07]);
            
            // mov dst, 64 (for zero input)
            let rex_mov = 0x48 | (dst_reg >> 3);
            code.extend_from_slice(&[rex_mov, 0xC7, 0xC0 | (dst_reg & 7)]);
            code.extend_from_slice(&64u32.to_le_bytes());
            // jmp end
            code.extend_from_slice(&[0xEB, 0x04]);
            
            // not_zero:
            // BSF r64, r64: REX.W 0F BC /r
            let rex = 0x48 | ((dst_reg >> 3) << 2) | (src_reg >> 3);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex, 0x0F, 0xBC, modrm]);
            // end:
            
            InstructionSets::SSE2
        }
    }
    
    /// Generate ISA-optimized native code for BEXTR operation (bit field extract)
    pub fn emit_bextr(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8, start: u8, len: u8) -> InstructionSets {
        if self.config.has_feature(InstructionSets::BMI1) {
            // BEXTR r64, r64, r64: VEX.LZ.0F38.W1 F7 /r
            // Control value: bits[7:0] = start, bits[15:8] = len
            let control = (start as u32) | ((len as u32) << 8);
            
            // Load control into temp register (R11 - caller-saved)
            // mov r11d, control
            code.extend_from_slice(&[0x41, 0xBB]);
            code.extend_from_slice(&control.to_le_bytes());
            
            // BEXTR dst, src, r11
            // VEX.NDS.LZ.0F38.W1 F7 /r
            let vex_r = if dst_reg < 8 { 0x80 } else { 0 };
            let vex_x = 0x40;
            let vex_b = if src_reg < 8 { 0x20 } else { 0 };
            let vex_w = 0x80;
            let vex_vvvv = (!11u8 & 0x0F) << 3; // R11 = 11, inverted
            
            code.push(0xC4);
            code.push(vex_r | vex_x | vex_b | 0x02);
            code.push(vex_w | vex_vvvv);
            code.push(0xF7);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.push(modrm);
            
            InstructionSets::BMI1
        } else {
            // Fallback: shift right by start, then mask by (1 << len) - 1
            // mov dst, src
            let rex = 0x48 | ((dst_reg >> 3) << 2) | (src_reg >> 3);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex, 0x8B, modrm]);
            
            // shr dst, start
            if start > 0 {
                let rex_shr = 0x48 | (dst_reg >> 3);
                let modrm_shr = 0xE8 | (dst_reg & 7);
                code.extend_from_slice(&[rex_shr, 0xC1, modrm_shr, start]);
            }
            
            // and dst, mask
            if len < 64 {
                let mask = (1u64 << len) - 1;
                if mask <= 0x7FFFFFFF {
                    // and r64, imm32 (sign-extended)
                    let rex_and = 0x48 | (dst_reg >> 3);
                    let modrm_and = 0xE0 | (dst_reg & 7);
                    code.extend_from_slice(&[rex_and, 0x81, modrm_and]);
                    code.extend_from_slice(&(mask as u32).to_le_bytes());
                } else {
                    // Use R11 for 64-bit mask
                    // mov r11, mask
                    code.extend_from_slice(&[0x49, 0xBB]);
                    code.extend_from_slice(&mask.to_le_bytes());
                    // and dst, r11
                    let rex_and = 0x4C | (dst_reg >> 3);
                    let modrm_and = 0xC0 | (3 << 3) | (dst_reg & 7); // r11 = reg 11
                    code.extend_from_slice(&[rex_and, 0x21, modrm_and]);
                }
            }
            
            InstructionSets::SSE2
        }
    }
    
    /// Generate ISA-optimized native code for PDEP operation (parallel bits deposit)
    pub fn emit_pdep(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8, mask_reg: u8) -> InstructionSets {
        if self.config.has_feature(InstructionSets::BMI2) {
            // PDEP r64, r64, r64: VEX.NDS.LZ.F2.0F38.W1 F5 /r
            let vex_r = if dst_reg < 8 { 0x80 } else { 0 };
            let vex_x = 0x40;
            let vex_b = if mask_reg < 8 { 0x20 } else { 0 };
            let vex_w = 0x80;
            let vex_vvvv = (!(src_reg) & 0x0F) << 3;
            
            code.push(0xC4);
            code.push(vex_r | vex_x | vex_b | 0x02);
            code.push(vex_w | vex_vvvv | 0x03); // pp=11 (F2)
            code.push(0xF5);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (mask_reg & 7);
            code.push(modrm);
            
            InstructionSets::BMI2
        } else {
            self.emit_pdep_software(code, dst_reg, src_reg, mask_reg);
            InstructionSets::SSE2
        }
    }
    
    /// Software PDEP implementation
    /// 
    /// Algorithm:
    /// ```text
    /// result = 0
    /// src_bit = 1
    /// for mask_bit from 1 to 2^63:
    ///   if (mask & mask_bit):
    ///     if (src & src_bit): result |= mask_bit
    ///     src_bit <<= 1
    /// ```
    fn emit_pdep_software(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8, mask_reg: u8) {
        // Use caller-saved registers: RAX=result, RCX=mask_copy, RDX=src_copy
        // R8=mask_bit, R9=src_bit, R10=temp
        
        // xor rax, rax (result = 0)
        code.extend_from_slice(&[0x48, 0x31, 0xC0]);
        
        // mov rcx, mask
        if mask_reg != 1 {
            let rex = 0x48 | (mask_reg >> 3);
            let modrm = 0xC0 | (1 << 3) | (mask_reg & 7);
            code.extend_from_slice(&[rex, 0x8B, modrm]);
        }
        
        // mov rdx, src
        if src_reg != 2 {
            let rex = 0x48 | (src_reg >> 3);
            let modrm = 0xC0 | (2 << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex, 0x8B, modrm]);
        }
        
        // mov r9, 1 (src_bit = 1)
        code.extend_from_slice(&[0x49, 0xC7, 0xC1, 0x01, 0x00, 0x00, 0x00]);
        
        let loop_start = code.len();
        
        // test rcx, rcx (check if mask is zero)
        code.extend_from_slice(&[0x48, 0x85, 0xC9]);
        // jz end (will patch)
        code.push(0x74);
        let jz_offset = code.len();
        code.push(0x00);
        
        // Extract lowest set bit: r8 = rcx & (-rcx) using blsi or software
        // mov r8, rcx
        code.extend_from_slice(&[0x49, 0x89, 0xC8]);
        // neg r10, rcx (r10 = -rcx)
        code.extend_from_slice(&[0x49, 0x89, 0xCA]); // mov r10, rcx
        code.extend_from_slice(&[0x49, 0xF7, 0xDA]); // neg r10
        // and r8, r10
        code.extend_from_slice(&[0x4D, 0x21, 0xD0]);
        
        // test rdx, r9 (check if (src & src_bit))
        code.extend_from_slice(&[0x4C, 0x85, 0xCA]);
        // jz skip_set
        code.extend_from_slice(&[0x74, 0x03]);
        // or rax, r8 (result |= mask_bit)
        code.extend_from_slice(&[0x4C, 0x09, 0xC0]);
        // skip_set:
        // shl r9, 1 (src_bit <<= 1)
        code.extend_from_slice(&[0x49, 0xD1, 0xE1]);
        
        // Clear lowest set bit: rcx = rcx & (rcx - 1) using blsr or software
        // lea r10, [rcx - 1]
        code.extend_from_slice(&[0x4C, 0x8D, 0x51, 0xFF]);
        // and rcx, r10
        code.extend_from_slice(&[0x4C, 0x21, 0xD1]);
        
        // jmp loop_start
        let rel = (loop_start as i32) - (code.len() as i32) - 2;
        code.push(0xEB);
        code.push(rel as u8);
        
        // Patch jz offset
        let end_offset = code.len();
        code[jz_offset] = (end_offset - jz_offset - 1) as u8;
        
        // mov dst, rax
        if dst_reg != 0 {
            let rex = 0x48 | ((dst_reg >> 3) << 2);
            let modrm = 0xC0 | ((dst_reg & 7) << 3);
            code.extend_from_slice(&[rex, 0x89, modrm]);
        }
    }
    
    /// Generate ISA-optimized native code for PEXT operation (parallel bits extract)
    pub fn emit_pext(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8, mask_reg: u8) -> InstructionSets {
        if self.config.has_feature(InstructionSets::BMI2) {
            // PEXT r64, r64, r64: VEX.NDS.LZ.F3.0F38.W1 F5 /r
            let vex_r = if dst_reg < 8 { 0x80 } else { 0 };
            let vex_x = 0x40;
            let vex_b = if mask_reg < 8 { 0x20 } else { 0 };
            let vex_w = 0x80;
            let vex_vvvv = (!(src_reg) & 0x0F) << 3;
            
            code.push(0xC4);
            code.push(vex_r | vex_x | vex_b | 0x02);
            code.push(vex_w | vex_vvvv | 0x02); // pp=10 (F3)
            code.push(0xF5);
            let modrm = 0xC0 | ((dst_reg & 7) << 3) | (mask_reg & 7);
            code.push(modrm);
            
            InstructionSets::BMI2
        } else {
            self.emit_pext_software(code, dst_reg, src_reg, mask_reg);
            InstructionSets::SSE2
        }
    }
    
    /// Software PEXT implementation
    /// 
    /// Algorithm:
    /// ```text
    /// result = 0
    /// result_bit = 1
    /// for mask_bit from 1 to 2^63:
    ///   if (mask & mask_bit):
    ///     if (src & mask_bit): result |= result_bit
    ///     result_bit <<= 1
    /// ```
    fn emit_pext_software(&self, code: &mut Vec<u8>, dst_reg: u8, src_reg: u8, mask_reg: u8) {
        // Use: RAX=result, RCX=mask_copy, RDX=src, R8=mask_bit, R9=result_bit, R10=temp
        
        // xor rax, rax (result = 0)
        code.extend_from_slice(&[0x48, 0x31, 0xC0]);
        
        // mov r9, 1 (result_bit = 1)
        code.extend_from_slice(&[0x49, 0xC7, 0xC1, 0x01, 0x00, 0x00, 0x00]);
        
        // mov rcx, mask
        if mask_reg != 1 {
            let rex = 0x48 | (mask_reg >> 3);
            let modrm = 0xC0 | (1 << 3) | (mask_reg & 7);
            code.extend_from_slice(&[rex, 0x8B, modrm]);
        }
        
        // mov rdx, src
        if src_reg != 2 {
            let rex = 0x48 | (src_reg >> 3);
            let modrm = 0xC0 | (2 << 3) | (src_reg & 7);
            code.extend_from_slice(&[rex, 0x8B, modrm]);
        }
        
        let loop_start = code.len();
        
        // test rcx, rcx
        code.extend_from_slice(&[0x48, 0x85, 0xC9]);
        // jz end
        code.push(0x74);
        let jz_offset = code.len();
        code.push(0x00);
        
        // Extract lowest set bit: r8 = rcx & (-rcx)
        code.extend_from_slice(&[0x49, 0x89, 0xC8]); // mov r8, rcx
        code.extend_from_slice(&[0x49, 0x89, 0xCA]); // mov r10, rcx
        code.extend_from_slice(&[0x49, 0xF7, 0xDA]); // neg r10
        code.extend_from_slice(&[0x4D, 0x21, 0xD0]); // and r8, r10
        
        // test rdx, r8 (check if (src & mask_bit))
        code.extend_from_slice(&[0x4C, 0x85, 0xC2]);
        // jz skip_set
        code.extend_from_slice(&[0x74, 0x03]);
        // or rax, r9 (result |= result_bit)
        code.extend_from_slice(&[0x4C, 0x09, 0xC8]);
        // skip_set:
        // shl r9, 1 (result_bit <<= 1)
        code.extend_from_slice(&[0x49, 0xD1, 0xE1]);
        
        // Clear lowest set bit: rcx = rcx & (rcx - 1)
        code.extend_from_slice(&[0x4C, 0x8D, 0x51, 0xFF]); // lea r10, [rcx - 1]
        code.extend_from_slice(&[0x4C, 0x21, 0xD1]);       // and rcx, r10
        
        // jmp loop_start
        let rel = (loop_start as i32) - (code.len() as i32) - 2;
        code.push(0xEB);
        code.push(rel as u8);
        
        // Patch jz offset
        let end_offset = code.len();
        code[jz_offset] = (end_offset - jz_offset - 1) as u8;
        
        // mov dst, rax
        if dst_reg != 0 {
            let rex = 0x48 | ((dst_reg >> 3) << 2);
            let modrm = 0xC0 | ((dst_reg & 7) << 3);
            code.extend_from_slice(&[rex, 0x89, modrm]);
        }
    }
}

// ============================================================================
// Unified Block Manager
// ============================================================================

/// Unified Block Manager
/// 
/// Provides consistent interface for:
/// - Compiling blocks with appropriate tier
/// - Saving/loading blocks with tier information
/// - Recompiling from IR when native code is stale
/// - ISA-aware code generation
pub struct BlockManager {
    /// S1 compiler instance
    s1_compiler: S1Compiler,
    /// S2 compiler instance
    s2_compiler: S2Compiler,
    /// Current ISA configuration
    isa_config: IsaCodegenConfig,
    /// ISA-aware code generator
    isa_codegen: IsaCodeGen,
    /// Profile database reference
    profile: ProfileDb,
}

impl BlockManager {
    /// Create new block manager
    pub fn new() -> Self {
        let isa_config = IsaCodegenConfig::for_current_cpu();
        log::info!("[BlockManager] Initialized with ISA: {:?}", 
            isa_config.target_isa.to_string_list());
        
        let isa_codegen = IsaCodeGen::new(isa_config.clone());
        
        Self {
            s1_compiler: S1Compiler::new(),
            s2_compiler: S2Compiler::new(),
            isa_config,
            isa_codegen,
            profile: ProfileDb::new(4096),
        }
    }
    
    /// Create with custom configuration
    pub fn with_config(
        isa_config: IsaCodegenConfig,
        s2_config: super::compiler_s2::S2Config,
    ) -> Self {
        let isa_codegen = IsaCodeGen::new(isa_config.clone());
        
        Self {
            s1_compiler: S1Compiler::new(),
            s2_compiler: S2Compiler::with_config(s2_config),
            isa_config,
            isa_codegen,
            profile: ProfileDb::new(4096),
        }
    }
    
    /// Get current ISA config
    pub fn isa_config(&self) -> &IsaCodegenConfig {
        &self.isa_config
    }
    
    /// Get ISA-aware code generator
    pub fn isa_codegen(&self) -> &IsaCodeGen {
        &self.isa_codegen
    }
    
    /// Get profile database
    pub fn profile(&self) -> &ProfileDb {
        &self.profile
    }
    
    /// Get mutable profile database
    pub fn profile_mut(&mut self) -> &mut ProfileDb {
        &mut self.profile
    }
    
    // ========================================================================
    // Compilation Methods
    // ========================================================================
    
    /// Compile with S1 (quick baseline)
    pub fn compile_s1(
        &self,
        guest_code: &[u8],
        start_rip: u64,
        decoder: &super::decoder::X86Decoder,
    ) -> JitResult<UnifiedBlock> {
        let s1_block = self.s1_compiler.compile(guest_code, start_rip, decoder, &self.profile)?;
        // Use the required_isa computed by S1's ISA pass
        let required_isa = s1_block.required_isa;
        Ok(UnifiedBlock::from_s1(&s1_block, required_isa))
    }
    
    /// Compile with S2 (optimizing) from S1 block
    pub fn compile_s2(
        &self,
        s1_block: &super::compiler_s1::S1Block,
    ) -> JitResult<UnifiedBlock> {
        let s2_block = self.s2_compiler.compile_from_s1(s1_block, &self.profile)?;
        // Use the required_isa from S2's ISA optimization pass
        let required_isa = s2_block.opt_stats.isa_opt_result
            .as_ref()
            .map(|r| r.required_isa)
            .unwrap_or_else(|| self.analyze_required_isa(&s2_block.ir));
        Ok(UnifiedBlock::from_s2(&s2_block, required_isa))
    }
    
    /// Recompile from IR with specified tier
    pub fn recompile_from_ir(
        &self,
        ir: &IrBlock,
        tier: CompileTier,
    ) -> JitResult<(Vec<u8>, InstructionSets)> {
        let native_code = match tier {
            CompileTier::Interpreter => {
                return Err(JitError::InvalidTier);
            }
            CompileTier::S1 => {
                self.s1_compiler.codegen_from_ir(ir)?
            }
            CompileTier::S2 => {
                self.s2_compiler.codegen_from_ir(ir)?
            }
        };
        
        let required_isa = self.analyze_required_isa(ir);
        Ok((native_code, required_isa))
    }
    
    /// Recompile with ISA-targeted optimization
    /// 
    /// Generates code optimized for specific instruction sets.
    pub fn recompile_with_isa(
        &self,
        ir: &IrBlock,
        tier: CompileTier,
        target_isa: InstructionSets,
    ) -> JitResult<(Vec<u8>, InstructionSets)> {
        // Transform IR to use ISA-specific operations where beneficial
        let optimized_ir = self.optimize_ir_for_isa(ir, target_isa);
        
        let native_code = match tier {
            CompileTier::Interpreter => {
                return Err(JitError::InvalidTier);
            }
            CompileTier::S1 => {
                self.s1_compiler.codegen_from_ir(&optimized_ir)?
            }
            CompileTier::S2 => {
                self.s2_compiler.codegen_from_ir(&optimized_ir)?
            }
        };
        
        let required_isa = self.analyze_required_isa(&optimized_ir);
        
        debug_assert!(
            target_isa.contains(required_isa),
            "Generated code requires ISA not in target: required={:?}, target={:?}",
            required_isa.to_string_list(),
            target_isa.to_string_list()
        );
        
        Ok((native_code, required_isa))
    }
    
    /// Optimize IR for target ISA
    fn optimize_ir_for_isa(&self, ir: &IrBlock, target_isa: InstructionSets) -> IrBlock {
        let mut optimized = ir.clone();
        
        for bb in &mut optimized.blocks {
            for instr in &mut bb.instrs {
                self.optimize_instr_for_isa(instr, target_isa);
            }
        }
        
        optimized
    }
    
    /// Optimize a single instruction for target ISA
    fn optimize_instr_for_isa(&self, instr: &mut IrInstr, target_isa: InstructionSets) {
        match &instr.op {
            // Vector width selection based on ISA
            IrOp::VectorOp { kind, width, src1, src2 } => {
                let new_width = if *width >= 512 && target_isa.contains(InstructionSets::AVX512F) {
                    512
                } else if *width >= 256 && target_isa.contains(InstructionSets::AVX) {
                    256
                } else {
                    128
                };
                
                if new_width != *width {
                    instr.op = IrOp::VectorOp {
                        kind: *kind,
                        width: new_width,
                        src1: *src1,
                        src2: *src2,
                    };
                }
            }
            _ => {}
        }
    }
    
    // ========================================================================
    // ISA Analysis
    // ========================================================================
    
    fn analyze_required_isa(&self, ir: &IrBlock) -> InstructionSets {
        let mut required = InstructionSets::SSE2;
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                required |= self.ir_op_required_isa(&instr.op);
            }
        }
        
        required
    }
    
    fn ir_op_required_isa(&self, op: &IrOp) -> InstructionSets {
        match op {
            IrOp::Popcnt(_) => {
                if self.isa_config.target_isa.contains(InstructionSets::POPCNT) {
                    InstructionSets::POPCNT
                } else {
                    InstructionSets::SSE2
                }
            }
            
            IrOp::Lzcnt(_) | IrOp::Bsr(_) => {
                if self.isa_config.target_isa.contains(InstructionSets::LZCNT) {
                    InstructionSets::LZCNT
                } else {
                    InstructionSets::SSE2
                }
            }
            
            IrOp::Tzcnt(_) | IrOp::Bsf(_) => {
                if self.isa_config.target_isa.contains(InstructionSets::BMI1) {
                    InstructionSets::BMI1
                } else {
                    InstructionSets::SSE2
                }
            }
            
            IrOp::Bextr(_, _, _) => {
                if self.isa_config.target_isa.contains(InstructionSets::BMI1) {
                    InstructionSets::BMI1
                } else {
                    InstructionSets::SSE2
                }
            }
            
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) => {
                if self.isa_config.target_isa.contains(InstructionSets::BMI2) {
                    InstructionSets::BMI2
                } else {
                    InstructionSets::SSE2
                }
            }
            
            IrOp::VectorOp { width, .. } => {
                match width {
                    512 if self.isa_config.target_isa.contains(InstructionSets::AVX512F) => {
                        InstructionSets::AVX512F
                    }
                    256 if self.isa_config.target_isa.contains(InstructionSets::AVX) => {
                        InstructionSets::AVX
                    }
                    _ => InstructionSets::SSE2
                }
            }
            
            IrOp::Fma(_, _, _) => {
                if self.isa_config.target_isa.contains(InstructionSets::FMA) {
                    InstructionSets::FMA
                } else {
                    InstructionSets::SSE2
                }
            }
            
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => InstructionSets::AESNI,
            IrOp::Pclmul(_, _, _) => InstructionSets::PCLMUL,
            
            _ => InstructionSets::SSE2,
        }
    }
    
    // ========================================================================
    // Block Persistence
    // ========================================================================
    
    /// Save a unified block to NReady! cache
    pub fn save_block(&self, cache: &NReadyCache, block: &UnifiedBlock) -> JitResult<()> {
        cache.save_ir_block(block.guest_rip, &block.ir)?;
        
        cache.save_native_block(
            block.guest_rip,
            &block.native_code,
            block.required_isa,
            block.tier,
            block.guest_size,
            block.guest_instrs,
            block.guest_checksum,
            block.exec_count,
        )?;
        
        Ok(())
    }
    
    /// Load a unified block from NReady! cache
    /// 
    /// Uses the **original tier** stored in the cache for recompilation.
    pub fn load_block(&self, cache: &NReadyCache, rip: u64) -> JitResult<Option<UnifiedBlock>> {
        if let Some(native_info) = cache.load_native_block(rip)? {
            if let Some(ir) = cache.load_ir_block(rip)? {
                return Ok(Some(UnifiedBlock {
                    guest_rip: rip,
                    guest_size: native_info.guest_size,
                    guest_instrs: native_info.guest_instrs,
                    guest_checksum: native_info.guest_checksum,
                    ir,
                    native_code: native_info.native_code,
                    tier: native_info.tier,
                    required_isa: native_info.required_isa,
                    exec_count: native_info.exec_count,
                }));
            }
        }
        
        // Native code missing or stale, try to recompile from IR
        let original_tier = self.get_stale_block_tier(cache, rip);
        
        if let Some(ir) = cache.load_ir_block(rip)? {
            let tier = original_tier.unwrap_or(CompileTier::S1);
            log::debug!("[BlockManager] Block {:#x}: recompiling from IR with tier {:?}", rip, tier);
            
            let (native_code, required_isa) = self.recompile_from_ir(&ir, tier)?;
            
            let block = UnifiedBlock {
                guest_rip: rip,
                guest_size: 0,
                guest_instrs: ir.blocks.iter().map(|b| b.instrs.len()).sum::<usize>() as u32,
                guest_checksum: 0,
                ir,
                native_code,
                tier,
                required_isa,
                exec_count: 0,
            };
            
            cache.save_native_block(
                block.guest_rip,
                &block.native_code,
                block.required_isa,
                block.tier,
                block.guest_size,
                block.guest_instrs,
                block.guest_checksum,
                block.exec_count,
            )?;
            
            return Ok(Some(block));
        }
        
        Ok(None)
    }
    
    /// Read tier from stale native block header
    fn get_stale_block_tier(&self, cache: &NReadyCache, rip: u64) -> Option<CompileTier> {
        let path = format!("{}/native/{:02x}/{:016x}.bin", 
            cache.vm_dir_path(), (rip >> 56) as u8, rip);
        
        let data = std::fs::read(&path).ok()?;
        if data.len() < NativeBlockHeader::SIZE {
            return None;
        }
        
        let tier = match data[16] {
            0 => CompileTier::Interpreter,
            1 => CompileTier::S1,
            _ => CompileTier::S2,
        };
        
        Some(tier)
    }
    
    /// Load block with ISA-specific optimizations
    pub fn load_block_optimized(
        &self, 
        cache: &NReadyCache, 
        rip: u64,
        target_isa: InstructionSets,
    ) -> JitResult<Option<UnifiedBlock>> {
        if let Some(native_info) = cache.load_native_block(rip)? {
            if target_isa.contains(native_info.required_isa) {
                if let Some(ir) = cache.load_ir_block(rip)? {
                    return Ok(Some(UnifiedBlock {
                        guest_rip: rip,
                        guest_size: native_info.guest_size,
                        guest_instrs: native_info.guest_instrs,
                        guest_checksum: native_info.guest_checksum,
                        ir,
                        native_code: native_info.native_code,
                        tier: native_info.tier,
                        required_isa: native_info.required_isa,
                        exec_count: native_info.exec_count,
                    }));
                }
            }
        }
        
        let original_tier = self.get_stale_block_tier(cache, rip).unwrap_or(CompileTier::S1);
        
        if let Some(ir) = cache.load_ir_block(rip)? {
            log::info!("[BlockManager] Block {:#x}: recompiling with tier {:?}, ISA {:?}", 
                rip, original_tier, target_isa.to_string_list());
            
            let (native_code, required_isa) = self.recompile_with_isa(&ir, original_tier, target_isa)?;
            
            let block = UnifiedBlock {
                guest_rip: rip,
                guest_size: 0,
                guest_instrs: ir.blocks.iter().map(|b| b.instrs.len()).sum::<usize>() as u32,
                guest_checksum: 0,
                ir,
                native_code,
                tier: original_tier,
                required_isa,
                exec_count: 0,
            };
            
            cache.save_native_block(
                block.guest_rip,
                &block.native_code,
                block.required_isa,
                block.tier,
                block.guest_size,
                block.guest_instrs,
                block.guest_checksum,
                block.exec_count,
            )?;
            
            return Ok(Some(block));
        }
        
        Ok(None)
    }
    
    // ========================================================================
    // ISA Upgrade Evaluation and Smart Loading
    // ========================================================================
    
    /// Load block with automatic ISA upgrade evaluation
    /// 
    /// This is the **recommended** method for loading cached blocks.
    /// It automatically evaluates whether recompiling with better ISA is worthwhile.
    pub fn load_block_smart(
        &self,
        cache: &NReadyCache, 
        rip: u64,
    ) -> JitResult<Option<(UnifiedBlock, Option<UpgradeDecision>)>> {
        let evaluator = IsaUpgradeEvaluator::new(self.isa_config.target_isa);
        
        // Try to load existing block
        if let Some(block) = self.load_block(cache, rip)? {
            // Evaluate if upgrade is worthwhile
            let decision = evaluator.evaluate(&block);
            
            if decision.should_upgrade {
                log::info!(
                    "[BlockManager] Block {:#x}: ISA upgrade recommended (speedup: {:.2}x, reason: {:?})",
                    rip, decision.estimated_speedup, decision.reason
                );
                
                // Recompile with better ISA
                let (native_code, required_isa) = self.recompile_with_isa(
                    &block.ir, 
                    block.tier, 
                    self.isa_config.target_isa
                )?;
                
                let upgraded_block = UnifiedBlock {
                    native_code,
                    required_isa,
                    ..block
                };
                
                // Save upgraded block
                cache.save_native_block(
                    upgraded_block.guest_rip,
                    &upgraded_block.native_code,
                    upgraded_block.required_isa,
                    upgraded_block.tier,
                    upgraded_block.guest_size,
                    upgraded_block.guest_instrs,
                    upgraded_block.guest_checksum,
                    upgraded_block.exec_count,
                )?;
                
                return Ok(Some((upgraded_block, Some(decision))));
            }
            
            return Ok(Some((block, Some(decision))));
        }
        
        Ok(None)
    }
    
    /// Batch evaluate blocks for potential ISA upgrades
    /// 
    /// Returns list of blocks worth upgrading, sorted by estimated benefit.
    pub fn find_upgrade_candidates(
        &self,
        cache: &NReadyCache,
        block_rips: &[u64],
    ) -> Vec<(u64, UpgradeDecision)> {
        let evaluator = IsaUpgradeEvaluator::new(self.isa_config.target_isa);
        let mut candidates = Vec::new();
        
        for &rip in block_rips {
            if let Ok(Some(block)) = self.load_block(cache, rip) {
                let decision = evaluator.evaluate(&block);
                if decision.should_upgrade {
                    candidates.push((rip, decision));
                }
            }
        }
        
        // Sort by estimated benefit (cycles_saved * speedup)
        candidates.sort_by(|a, b| {
            let benefit_a = a.1.cycles_saved as f32 * a.1.estimated_speedup;
            let benefit_b = b.1.cycles_saved as f32 * b.1.estimated_speedup;
            benefit_b.partial_cmp(&benefit_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        candidates
    }
    
    /// Get ISA upgrade statistics for diagnostics
    pub fn get_isa_stats(&self, block: &UnifiedBlock) -> IsaUpgradeStats {
        let stats = IrOpStats::from_ir(&block.ir);
        let evaluator = IsaUpgradeEvaluator::new(self.isa_config.target_isa);
        let decision = evaluator.evaluate(block);
        let recommended_isa = evaluator.recommend_isa(&stats);
        
        IsaUpgradeStats {
            current_isa: block.required_isa,
            available_isa: self.isa_config.target_isa,
            recommended_isa,
            op_stats: stats,
            upgrade_decision: decision,
        }
    }
}

/// ISA upgrade statistics for diagnostics
#[derive(Clone, Debug)]
pub struct IsaUpgradeStats {
    /// Current ISA used by native code
    pub current_isa: InstructionSets,
    /// ISA available on this CPU
    pub available_isa: InstructionSets,
    /// Recommended ISA based on IR operations
    pub recommended_isa: InstructionSets,
    /// Operation statistics
    pub op_stats: IrOpStats,
    /// Upgrade decision
    pub upgrade_decision: UpgradeDecision,
}

impl Default for BlockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_isa_config_detection() {
        let config = IsaCodegenConfig::for_current_cpu();
        assert!(config.target_isa.contains(InstructionSets::SSE2));
        println!("Detected ISA: {:?}", config.target_isa.to_string_list());
    }
    
    #[test]
    fn test_isa_baseline() {
        let config = IsaCodegenConfig::default();
        assert_eq!(config.target_isa, InstructionSets::SSE2);
        assert!(!config.enable_simd);
        assert!(!config.enable_bmi);
    }
    
    #[test]
    fn test_popcnt_codegen() {
        let config = IsaCodegenConfig::with_target(InstructionSets::SSE2 | InstructionSets::POPCNT);
        let codegen = IsaCodeGen::new(config);
        
        let mut code = Vec::new();
        let isa = codegen.emit_popcnt(&mut code, 0, 1);
        
        if InstructionSets::detect_current().contains(InstructionSets::POPCNT) {
            assert_eq!(isa, InstructionSets::POPCNT);
            assert_eq!(&code[0..2], &[0xF3, 0x48]);
        }
    }
    
    #[test]
    fn test_popcnt_software_fallback() {
        let config = IsaCodegenConfig::default();
        let codegen = IsaCodeGen::new(config);
        
        let mut code = Vec::new();
        let isa = codegen.emit_popcnt(&mut code, 0, 0);
        
        assert_eq!(isa, InstructionSets::SSE2);
        assert!(code.len() > 10);
    }
    
    #[test]
    fn test_lzcnt_with_zero_handling() {
        let config = IsaCodegenConfig::default();
        let codegen = IsaCodeGen::new(config);
        
        let mut code = Vec::new();
        let isa = codegen.emit_lzcnt(&mut code, 0, 1);
        
        assert_eq!(isa, InstructionSets::SSE2);
        // Should include zero check (test + jnz)
        assert!(code.len() > 10);
    }
    
    #[test]
    fn test_bextr_software_fallback() {
        let config = IsaCodegenConfig::default();
        let codegen = IsaCodeGen::new(config);
        
        let mut code = Vec::new();
        let isa = codegen.emit_bextr(&mut code, 0, 1, 8, 16);
        
        assert_eq!(isa, InstructionSets::SSE2);
        assert!(code.len() > 4);
    }
    
    #[test]
    fn test_pdep_software() {
        let config = IsaCodegenConfig::default();
        let codegen = IsaCodeGen::new(config);
        
        let mut code = Vec::new();
        let isa = codegen.emit_pdep(&mut code, 0, 1, 2);
        
        assert_eq!(isa, InstructionSets::SSE2);
        // Software implementation has loop
        assert!(code.len() > 20);
    }
    
    #[test]
    fn test_pext_software() {
        let config = IsaCodegenConfig::default();
        let codegen = IsaCodeGen::new(config);
        
        let mut code = Vec::new();
        let isa = codegen.emit_pext(&mut code, 0, 1, 2);
        
        assert_eq!(isa, InstructionSets::SSE2);
        assert!(code.len() > 20);
    }
    
    // ========================================================================
    // ISA Performance Model Tests
    // ========================================================================
    
    #[test]
    fn test_perf_model_popcnt() {
        // POPCNT: software = 15 cycles, hardware = 3 cycles
        let ssa2_only = InstructionSets::SSE2;
        let with_popcnt = InstructionSets::SSE2 | InstructionSets::POPCNT;
        
        let speedup = IsaPerfModel::calculate_speedup(IsaOpCategory::Popcnt, ssa2_only, with_popcnt);
        assert!(speedup >= 4.0, "POPCNT speedup should be ~5x, got {}", speedup);
    }
    
    #[test]
    fn test_perf_model_aes() {
        // AES: software = 200 cycles, hardware = 4 cycles (50x speedup)
        let ssa2_only = InstructionSets::SSE2;
        let with_aes = InstructionSets::SSE2 | InstructionSets::AESNI;
        
        let speedup = IsaPerfModel::calculate_speedup(IsaOpCategory::Aes, ssa2_only, with_aes);
        assert!(speedup >= 40.0, "AES speedup should be ~50x, got {}", speedup);
    }
    
    #[test]
    fn test_perf_model_parallel_bits() {
        // PDEP/PEXT: software = 30 cycles, hardware = 3 cycles (10x speedup)
        let ssa2_only = InstructionSets::SSE2;
        let with_bmi2 = InstructionSets::SSE2 | InstructionSets::BMI2;
        
        let speedup = IsaPerfModel::calculate_speedup(IsaOpCategory::ParallelBits, ssa2_only, with_bmi2);
        assert!(speedup >= 8.0, "PDEP/PEXT speedup should be ~10x, got {}", speedup);
    }
    
    #[test]
    fn test_perf_model_vector_upgrade() {
        // Vector 256: SSE2 = 2x128bit ops, AVX = 1x256bit op
        let sse2_only = InstructionSets::SSE2;
        let with_avx = InstructionSets::SSE2 | InstructionSets::AVX;
        
        let speedup = IsaPerfModel::calculate_speedup(IsaOpCategory::Vector256, sse2_only, with_avx);
        assert!(speedup >= 1.5, "AVX 256-bit speedup should be ~2x, got {}", speedup);
    }
    
    // ========================================================================
    // ISA Upgrade Evaluator Tests
    // ========================================================================
    
    // Helper function to create test IrBlock
    fn make_test_ir(instrs: Vec<IrInstr>, entry_rip: u64) -> IrBlock {
        use super::super::ir::IrBlockMeta;
        IrBlock {
            entry_rip,
            guest_size: 100,
            blocks: vec![super::super::ir::IrBasicBlock {
                id: BlockId(0),
                instrs,
                predecessors: vec![],
                successors: vec![],
                entry_rip,
            }],
            entry_block: BlockId(0),
            next_vreg: 0,
            meta: IrBlockMeta::default(),
        }
    }
    
    #[test]
    fn test_evaluator_no_sensitive_ops() {
        let evaluator = IsaUpgradeEvaluator::new(
            InstructionSets::SSE2 | InstructionSets::POPCNT | InstructionSets::AVX2
        );
        
        // Block with only scalar ops
        let block = UnifiedBlock {
            guest_rip: 0x1000,
            guest_size: 100,
            guest_instrs: 10,
            guest_checksum: 0,
            ir: make_test_ir(vec![
                IrInstr { dst: VReg(0), op: IrOp::Const(42), guest_rip: 0, flags: IrFlags::empty() },
                IrInstr { dst: VReg(1), op: IrOp::Add(VReg(0), VReg(0)), guest_rip: 0, flags: IrFlags::empty() },
            ], 0x1000),
            native_code: vec![0x90],
            tier: CompileTier::S1,
            required_isa: InstructionSets::SSE2,
            exec_count: 1000,
        };
        
        let decision = evaluator.evaluate(&block);
        assert!(!decision.should_upgrade);
        assert!(matches!(decision.reason, UpgradeReason::NoSensitiveOps));
    }
    
    #[test]
    fn test_evaluator_cold_block() {
        let evaluator = IsaUpgradeEvaluator::new(
            InstructionSets::SSE2 | InstructionSets::POPCNT
        ).with_thresholds(100, 1.1);
        
        // Block with POPCNT but low exec count
        let block = UnifiedBlock {
            guest_rip: 0x1000,
            guest_size: 100,
            guest_instrs: 10,
            guest_checksum: 0,
            ir: make_test_ir(vec![
                IrInstr { dst: VReg(0), op: IrOp::Const(42), guest_rip: 0, flags: IrFlags::empty() },
                IrInstr { dst: VReg(1), op: IrOp::Popcnt(VReg(0)), guest_rip: 0, flags: IrFlags::empty() },
            ], 0x1000),
            native_code: vec![0x90],
            tier: CompileTier::S1,
            required_isa: InstructionSets::SSE2,
            exec_count: 10, // Too cold
        };
        
        let decision = evaluator.evaluate(&block);
        assert!(!decision.should_upgrade);
        assert!(matches!(decision.reason, UpgradeReason::TooCold { .. }));
    }
    
    #[test]
    fn test_evaluator_critical_aes() {
        let evaluator = IsaUpgradeEvaluator::new(
            InstructionSets::SSE2 | InstructionSets::AESNI
        );
        
        // Block with AES operations - should always upgrade
        let block = UnifiedBlock {
            guest_rip: 0x1000,
            guest_size: 100,
            guest_instrs: 10,
            guest_checksum: 0,
            ir: make_test_ir(vec![
                IrInstr { dst: VReg(0), op: IrOp::Const(42), guest_rip: 0, flags: IrFlags::empty() },
                IrInstr { dst: VReg(1), op: IrOp::Aesenc(VReg(0), VReg(0)), guest_rip: 0, flags: IrFlags::empty() },
            ], 0x1000),
            native_code: vec![0x90],
            tier: CompileTier::S1,
            required_isa: InstructionSets::SSE2,
            exec_count: 1, // Even cold blocks with AES should upgrade
        };
        
        let decision = evaluator.evaluate(&block);
        assert!(decision.should_upgrade, "AES blocks should always upgrade");
        assert!(matches!(decision.reason, UpgradeReason::CriticalFeature { feature: IsaOpCategory::Aes }));
        assert!(decision.estimated_speedup >= 10.0, "AES speedup should be very high");
    }
    
    #[test]
    fn test_evaluator_hot_popcnt_block() {
        let evaluator = IsaUpgradeEvaluator::new(
            InstructionSets::SSE2 | InstructionSets::POPCNT | InstructionSets::LZCNT
        );
        
        // Hot block with multiple POPCNTs
        let mut instrs = vec![
            IrInstr { dst: VReg(0), op: IrOp::Const(42), guest_rip: 0, flags: IrFlags::empty() },
        ];
        for i in 1..=20 {
            instrs.push(IrInstr { 
                dst: VReg(i), 
                op: IrOp::Popcnt(VReg(i-1)), 
                guest_rip: 0, 
                flags: IrFlags::empty() 
            });
        }
        
        let block = UnifiedBlock {
            guest_rip: 0x1000,
            guest_size: 100,
            guest_instrs: 21,
            guest_checksum: 0,
            ir: make_test_ir(instrs, 0x1000),
            native_code: vec![0x90],
            tier: CompileTier::S1,
            required_isa: InstructionSets::SSE2,
            exec_count: 10000,
        };
        
        let decision = evaluator.evaluate(&block);
        assert!(decision.should_upgrade, "Hot POPCNT blocks should upgrade");
        assert!(decision.estimated_speedup >= 2.0, "20 POPCNTs should give good speedup");
    }
    
    #[test]
    fn test_op_stats_counting() {
        let ir = make_test_ir(vec![
            IrInstr { dst: VReg(0), op: IrOp::Const(1), guest_rip: 0, flags: IrFlags::empty() },
            IrInstr { dst: VReg(1), op: IrOp::Popcnt(VReg(0)), guest_rip: 0, flags: IrFlags::empty() },
            IrInstr { dst: VReg(2), op: IrOp::Popcnt(VReg(1)), guest_rip: 0, flags: IrFlags::empty() },
            IrInstr { dst: VReg(3), op: IrOp::Lzcnt(VReg(2)), guest_rip: 0, flags: IrFlags::empty() },
            IrInstr { dst: VReg(4), op: IrOp::Aesenc(VReg(3), VReg(0)), guest_rip: 0, flags: IrFlags::empty() },
            IrInstr { dst: VReg(5), op: IrOp::Add(VReg(4), VReg(0)), guest_rip: 0, flags: IrFlags::empty() },
        ], 0x1000);
        
        let stats = IrOpStats::from_ir(&ir);
        assert_eq!(stats.popcnt, 2);
        assert_eq!(stats.bit_count, 1);
        assert_eq!(stats.aes, 1);
        assert_eq!(stats.scalar_int, 2);  // Const + Add
        assert_eq!(stats.isa_sensitive_ops(), 4);  // 2 popcnt + 1 lzcnt + 1 aes
    }
    
    #[test]
    fn test_recommend_isa() {
        let evaluator = IsaUpgradeEvaluator::new(
            InstructionSets::SSE2 | InstructionSets::POPCNT | InstructionSets::LZCNT | 
            InstructionSets::BMI1 | InstructionSets::BMI2 | InstructionSets::AVX | InstructionSets::AVX2
        );
        
        let mut stats = IrOpStats::default();
        stats.popcnt = 5;
        stats.parallel_bits = 3;
        stats.vector_256 = 10;
        
        let recommended = evaluator.recommend_isa(&stats);
        
        assert!(recommended.contains(InstructionSets::POPCNT));
        assert!(recommended.contains(InstructionSets::BMI2));
        assert!(recommended.contains(InstructionSets::AVX));
    }
}
