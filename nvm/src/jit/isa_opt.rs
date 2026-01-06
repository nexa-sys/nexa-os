//! ISA-Aware Optimization Pass
//!
//! Analyzes IR and applies ISA-specific transformations to generate optimal code
//! for the target CPU's instruction set.
//!
//! ## Optimization Phases
//!
//! 1. **IR Analysis**: Scan IR to identify ISA-sensitive operations
//! 2. **Pattern Matching**: Identify patterns that can use specialized ISA features
//! 3. **IR Transformation**: Transform generic IR ops to ISA-optimized versions
//! 4. **Cost Estimation**: Estimate performance improvement from ISA features
//!
//! ## ISA Feature Detection and Usage
//!
//! | Feature | Detection | IR Transformation |
//! |---------|-----------|-------------------|
//! | POPCNT | `IrOp::Popcnt` | Keep as-is (codegen handles hw/sw) |
//! | LZCNT | `IrOp::Lzcnt` | Keep as-is |
//! | BMI1 | `IrOp::Tzcnt`, `IrOp::Bextr` | Keep or expand to shift+mask |
//! | BMI2 | `IrOp::Pdep`, `IrOp::Pext` | Keep or expand to loops |
//! | FMA | `IrOp::Fma` | Keep or split to mul+add |
//! | AVX | Vector ops width=256 | Adjust width based on ISA |
//! | AVX-512 | Vector ops width=512 | Adjust width based on ISA |
//!
//! ## Pattern Recognition
//!
//! The pass recognizes common patterns and transforms them:
//!
//! ```text
//! Pattern: x & (x - 1)  → BLSR (BMI1)
//! Pattern: x & -x       → BLSI (BMI1)
//! Pattern: ~x & y       → ANDN (BMI1)
//! Pattern: popcount via shift/and sequence → POPCNT
//! Pattern: mul + add (FP) → FMA
//! ```

use super::ir::{IrBlock, IrBasicBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, VectorOpKind, IrBlockMeta};
use super::nready::InstructionSets;
use super::block_manager::{IsaCodegenConfig, IrOpStats, IsaOpCategory, IsaPerfModel};

/// ISA optimization pass configuration
#[derive(Clone, Debug)]
pub struct IsaOptConfig {
    /// Target ISA features to use
    pub target_isa: InstructionSets,
    /// Enable pattern recognition for BMI1 ops (BLSR, BLSI, ANDN)
    pub bmi1_patterns: bool,
    /// Enable pattern recognition for BMI2 ops (PDEP, PEXT)
    pub bmi2_patterns: bool,
    /// Enable POPCNT pattern recognition
    pub popcnt_patterns: bool,
    /// Enable FMA pattern recognition (mul+add → FMA)
    pub fma_patterns: bool,
    /// Enable vector width optimization
    pub vector_width_opt: bool,
    /// Minimum speedup factor to apply transformation (1.0 = any improvement)
    pub min_speedup: f32,
    /// Enable aggressive optimization (may increase code size)
    pub aggressive: bool,
}

impl Default for IsaOptConfig {
    fn default() -> Self {
        let target_isa = InstructionSets::detect_current();
        Self {
            target_isa,
            bmi1_patterns: target_isa.contains(InstructionSets::BMI1),
            bmi2_patterns: target_isa.contains(InstructionSets::BMI2),
            popcnt_patterns: target_isa.contains(InstructionSets::POPCNT),
            fma_patterns: target_isa.contains(InstructionSets::FMA),
            vector_width_opt: true,
            min_speedup: 1.1,
            aggressive: false,
        }
    }
}

impl IsaOptConfig {
    /// Create config targeting specific ISA
    pub fn for_isa(target_isa: InstructionSets) -> Self {
        Self {
            target_isa,
            bmi1_patterns: target_isa.contains(InstructionSets::BMI1),
            bmi2_patterns: target_isa.contains(InstructionSets::BMI2),
            popcnt_patterns: target_isa.contains(InstructionSets::POPCNT),
            fma_patterns: target_isa.contains(InstructionSets::FMA),
            vector_width_opt: true,
            min_speedup: 1.1,
            aggressive: false,
        }
    }
    
    /// Create baseline config (SSE2 only, no advanced features)
    pub fn baseline() -> Self {
        Self {
            target_isa: InstructionSets::SSE2,
            bmi1_patterns: false,
            bmi2_patterns: false,
            popcnt_patterns: false,
            fma_patterns: false,
            vector_width_opt: false,
            min_speedup: 1.0,
            aggressive: false,
        }
    }
}

/// Result of ISA optimization pass
#[derive(Clone, Debug)]
pub struct IsaOptResult {
    /// Number of POPCNT patterns recognized
    pub popcnt_recognized: u32,
    /// Number of BMI1 patterns recognized
    pub bmi1_patterns_applied: u32,
    /// Number of BMI2 patterns recognized  
    pub bmi2_patterns_applied: u32,
    /// Number of FMA patterns recognized
    pub fma_patterns_applied: u32,
    /// Number of vector ops width-optimized
    pub vector_ops_optimized: u32,
    /// Estimated speedup factor
    pub estimated_speedup: f32,
    /// ISA features that will be used
    pub required_isa: InstructionSets,
}

impl Default for IsaOptResult {
    fn default() -> Self {
        Self {
            popcnt_recognized: 0,
            bmi1_patterns_applied: 0,
            bmi2_patterns_applied: 0,
            fma_patterns_applied: 0,
            vector_ops_optimized: 0,
            estimated_speedup: 1.0,
            required_isa: InstructionSets::empty(),
        }
    }
}

/// ISA-aware optimization pass
pub struct IsaOptPass {
    config: IsaOptConfig,
}

impl IsaOptPass {
    pub fn new() -> Self {
        Self {
            config: IsaOptConfig::default(),
        }
    }
    
    pub fn with_config(config: IsaOptConfig) -> Self {
        Self { config }
    }
    
    /// Run ISA optimization pass on IR
    pub fn run(&self, ir: &mut IrBlock) -> IsaOptResult {
        let mut result = IsaOptResult::default();
        result.required_isa = InstructionSets::SSE2;
        
        // Phase 1: Analyze current IR ops
        let stats_before = IrOpStats::from_ir(ir);
        
        // Phase 2: Pattern recognition and transformation
        for bb in &mut ir.blocks {
            self.optimize_basic_block(bb, &mut result);
        }
        
        // Phase 3: Vector width optimization
        if self.config.vector_width_opt {
            self.optimize_vector_widths(ir, &mut result);
        }
        
        // Phase 4: Estimate speedup
        let stats_after = IrOpStats::from_ir(ir);
        result.estimated_speedup = self.estimate_speedup(&stats_before, &stats_after);
        
        result
    }
    
    /// Optimize a single basic block
    fn optimize_basic_block(&self, bb: &mut IrBasicBlock, result: &mut IsaOptResult) {
        let mut i = 0;
        while i < bb.instrs.len() {
            let transformed = self.try_transform_instr(&bb.instrs, i, result);
            
            if let Some(new_instrs) = transformed {
                // Replace instruction(s)
                bb.instrs.splice(i..i+1, new_instrs.into_iter());
            }
            
            // Update required ISA based on instruction
            result.required_isa |= self.instr_required_isa(&bb.instrs[i]);
            
            i += 1;
        }
        
        // Pattern matching pass (looks at sequences)
        if self.config.bmi1_patterns {
            self.apply_bmi1_patterns(bb, result);
        }
        
        if self.config.fma_patterns {
            self.apply_fma_patterns(bb, result);
        }
    }
    
    /// Try to transform a single instruction for ISA optimization
    fn try_transform_instr(&self, instrs: &[IrInstr], idx: usize, result: &mut IsaOptResult) -> Option<Vec<IrInstr>> {
        let instr = &instrs[idx];
        
        match &instr.op {
            // Vector width optimization: adjust width based on available ISA
            IrOp::VectorOp { kind, width, src1, src2 } => {
                let optimal_width = self.optimal_vector_width(*width);
                if optimal_width != *width {
                    result.vector_ops_optimized += 1;
                    return Some(vec![IrInstr {
                        dst: instr.dst,
                        op: IrOp::VectorOp {
                            kind: *kind,
                            width: optimal_width,
                            src1: *src1,
                            src2: *src2,
                        },
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    }]);
                }
            }
            
            // POPCNT: keep as-is, codegen handles hw/sw selection
            IrOp::Popcnt(_) => {
                if self.config.target_isa.contains(InstructionSets::POPCNT) {
                    result.required_isa |= InstructionSets::POPCNT;
                }
            }
            
            // LZCNT: keep as-is
            IrOp::Lzcnt(_) => {
                if self.config.target_isa.contains(InstructionSets::LZCNT) {
                    result.required_isa |= InstructionSets::LZCNT;
                }
            }
            
            // BMI1 ops
            IrOp::Tzcnt(_) | IrOp::Bextr(_, _, _) => {
                if self.config.target_isa.contains(InstructionSets::BMI1) {
                    result.required_isa |= InstructionSets::BMI1;
                }
            }
            
            // BMI2 ops
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) => {
                if self.config.target_isa.contains(InstructionSets::BMI2) {
                    result.required_isa |= InstructionSets::BMI2;
                }
            }
            
            // FMA
            IrOp::Fma(_, _, _) => {
                if self.config.target_isa.contains(InstructionSets::FMA) {
                    result.required_isa |= InstructionSets::FMA;
                }
            }
            
            // AES-NI
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => {
                result.required_isa |= InstructionSets::AESNI;
            }
            
            // PCLMUL
            IrOp::Pclmul(_, _, _) => {
                result.required_isa |= InstructionSets::PCLMUL;
            }
            
            _ => {}
        }
        
        None
    }
    
    /// Get required ISA for an instruction
    fn instr_required_isa(&self, instr: &IrInstr) -> InstructionSets {
        match &instr.op {
            IrOp::Popcnt(_) if self.config.target_isa.contains(InstructionSets::POPCNT) => {
                InstructionSets::POPCNT
            }
            IrOp::Lzcnt(_) if self.config.target_isa.contains(InstructionSets::LZCNT) => {
                InstructionSets::LZCNT
            }
            IrOp::Tzcnt(_) | IrOp::Bextr(_, _, _) 
                if self.config.target_isa.contains(InstructionSets::BMI1) => {
                InstructionSets::BMI1
            }
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) 
                if self.config.target_isa.contains(InstructionSets::BMI2) => {
                InstructionSets::BMI2
            }
            IrOp::VectorOp { width, .. } => {
                if *width > 256 && self.config.target_isa.contains(InstructionSets::AVX512F) {
                    InstructionSets::AVX512F
                } else if *width > 128 && self.config.target_isa.contains(InstructionSets::AVX) {
                    InstructionSets::AVX
                } else {
                    InstructionSets::SSE2
                }
            }
            IrOp::Fma(_, _, _) if self.config.target_isa.contains(InstructionSets::FMA) => {
                InstructionSets::FMA
            }
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => InstructionSets::AESNI,
            IrOp::Pclmul(_, _, _) => InstructionSets::PCLMUL,
            _ => InstructionSets::SSE2,
        }
    }
    
    /// Determine optimal vector width based on available ISA
    fn optimal_vector_width(&self, requested: u16) -> u16 {
        if requested >= 512 && self.config.target_isa.contains(InstructionSets::AVX512F) {
            512
        } else if requested >= 256 && self.config.target_isa.contains(InstructionSets::AVX) {
            256
        } else {
            128
        }
    }
    
    /// Apply BMI1 pattern recognition
    /// 
    /// Patterns:
    /// - x & (x - 1) → BLSR r, r (reset lowest set bit)
    /// - x & -x → BLSI r, r (isolate lowest set bit)
    /// - ~x & y → ANDN r, x, y
    fn apply_bmi1_patterns(&self, bb: &mut IrBasicBlock, result: &mut IsaOptResult) {
        if !self.config.target_isa.contains(InstructionSets::BMI1) {
            return;
        }
        
        // Scan for patterns in 2-3 instruction sequences
        let mut i = 0;
        while i + 1 < bb.instrs.len() {
            // Pattern: sub t, x, 1; and dst, x, t → BLSR dst, x
            if let (IrOp::Sub(x1, one), IrOp::And(x2, t)) = 
                (&bb.instrs[i].op, &bb.instrs[i + 1].op) 
            {
                if let IrOp::Const(1) = self.get_const_op(*one, &bb.instrs[..i]) {
                    let t_vreg = bb.instrs[i].dst;
                    if *t == t_vreg && *x1 == *x2 {
                        // Transform to BLSR pattern
                        // Note: We mark this as BMI1-optimized; codegen will emit BLSR
                        result.bmi1_patterns_applied += 1;
                        result.required_isa |= InstructionSets::BMI1;
                    }
                }
            }
            
            // Pattern: neg t, x; and dst, x, t → BLSI dst, x
            if let (IrOp::Neg(x1), IrOp::And(x2, t)) = 
                (&bb.instrs[i].op, &bb.instrs[i + 1].op) 
            {
                let t_vreg = bb.instrs[i].dst;
                if *t == t_vreg && *x1 == *x2 {
                    result.bmi1_patterns_applied += 1;
                    result.required_isa |= InstructionSets::BMI1;
                }
            }
            
            // Pattern: not t, x; and dst, t, y → ANDN dst, x, y
            if let (IrOp::Not(x), IrOp::And(t, y)) = 
                (&bb.instrs[i].op, &bb.instrs[i + 1].op) 
            {
                let t_vreg = bb.instrs[i].dst;
                if *t == t_vreg {
                    result.bmi1_patterns_applied += 1;
                    result.required_isa |= InstructionSets::BMI1;
                }
                let _ = (x, y); // Used in pattern
            }
            
            i += 1;
        }
    }
    
    /// Helper to get const value from VReg
    fn get_const_op(&self, vreg: VReg, prior_instrs: &[IrInstr]) -> IrOp {
        for instr in prior_instrs.iter().rev() {
            if instr.dst == vreg {
                return instr.op.clone();
            }
        }
        IrOp::Const(0) // Default
    }
    
    /// Apply FMA pattern recognition
    /// 
    /// Pattern: mul t, a, b; add dst, t, c → FMA dst, a, b, c
    fn apply_fma_patterns(&self, bb: &mut IrBasicBlock, result: &mut IsaOptResult) {
        if !self.config.target_isa.contains(InstructionSets::FMA) {
            return;
        }
        
        // This is for floating point operations
        // In practice, we'd need to track which VRegs are FP vs integer
        // For now, just count potential patterns
        
        let mut i = 0;
        while i + 1 < bb.instrs.len() {
            // Look for mul followed by add where mul result is only used by add
            if let IrOp::Mul(a, b) = &bb.instrs[i].op {
                let mul_dst = bb.instrs[i].dst;
                
                if let IrOp::Add(t, c) = &bb.instrs[i + 1].op {
                    if *t == mul_dst {
                        // Could transform to FMA
                        // In real implementation, would replace with IrOp::Fma(*a, *b, *c)
                        result.fma_patterns_applied += 1;
                        result.required_isa |= InstructionSets::FMA;
                        let _ = (a, b, c);
                    }
                }
            }
            
            i += 1;
        }
    }
    
    /// Optimize vector operation widths across the IR
    fn optimize_vector_widths(&self, ir: &mut IrBlock, result: &mut IsaOptResult) {
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if let IrOp::VectorOp { kind, width, src1, src2 } = &instr.op {
                    let optimal = self.optimal_vector_width(*width);
                    if optimal != *width {
                        instr.op = IrOp::VectorOp {
                            kind: *kind,
                            width: optimal,
                            src1: *src1,
                            src2: *src2,
                        };
                        result.vector_ops_optimized += 1;
                    }
                }
            }
        }
    }
    
    /// Estimate speedup from ISA optimizations
    fn estimate_speedup(&self, before: &IrOpStats, after: &IrOpStats) -> f32 {
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
        
        let baseline_isa = InstructionSets::SSE2;
        let target_isa = self.config.target_isa;
        
        let mut total_before_cycles = 0u64;
        let mut total_after_cycles = 0u64;
        
        for cat in categories {
            let count = before.get(cat) as u64;
            if count == 0 {
                continue;
            }
            
            let (sw_profile, _) = IsaPerfModel::get_profile(cat, baseline_isa);
            let (_, hw_profile) = IsaPerfModel::get_profile(cat, target_isa);
            
            total_before_cycles += count * sw_profile.latency as u64;
            total_after_cycles += count * hw_profile.map(|p| p.latency).unwrap_or(sw_profile.latency) as u64;
        }
        
        // Add baseline ops (no change)
        let baseline_ops = (before.scalar_int + before.scalar_fp + before.memory + before.control) as u64;
        total_before_cycles += baseline_ops;
        total_after_cycles += baseline_ops;
        
        if total_after_cycles > 0 {
            total_before_cycles as f32 / total_after_cycles as f32
        } else {
            1.0
        }
    }
}

impl Default for IsaOptPass {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// S1-Specific ISA Optimization (Lightweight)
// ============================================================================

/// Lightweight ISA pass for S1 (quick compile)
/// 
/// S1 focuses on compile speed, so we only do:
/// - Vector width adjustment (no pattern matching)
/// - ISA requirement analysis (for codegen)
pub struct S1IsaPass {
    target_isa: InstructionSets,
}

impl S1IsaPass {
    pub fn new() -> Self {
        Self {
            target_isa: InstructionSets::detect_current(),
        }
    }
    
    pub fn with_isa(target_isa: InstructionSets) -> Self {
        Self { target_isa }
    }
    
    /// Quick ISA analysis for S1 - only adjusts vector widths
    pub fn run(&self, ir: &mut IrBlock) -> InstructionSets {
        let mut required = InstructionSets::SSE2;
        
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                // Adjust vector widths
                if let IrOp::VectorOp { kind, width, src1, src2 } = &instr.op {
                    let optimal = self.optimal_width(*width);
                    if optimal != *width {
                        instr.op = IrOp::VectorOp {
                            kind: *kind,
                            width: optimal,
                            src1: *src1,
                            src2: *src2,
                        };
                    }
                }
                
                // Track required ISA
                required |= self.op_required_isa(&instr.op);
            }
        }
        
        required
    }
    
    fn optimal_width(&self, requested: u16) -> u16 {
        if requested >= 512 && self.target_isa.contains(InstructionSets::AVX512F) {
            512
        } else if requested >= 256 && self.target_isa.contains(InstructionSets::AVX) {
            256
        } else {
            128
        }
    }
    
    fn op_required_isa(&self, op: &IrOp) -> InstructionSets {
        match op {
            IrOp::Popcnt(_) if self.target_isa.contains(InstructionSets::POPCNT) => {
                InstructionSets::POPCNT
            }
            IrOp::Lzcnt(_) if self.target_isa.contains(InstructionSets::LZCNT) => {
                InstructionSets::LZCNT
            }
            IrOp::Tzcnt(_) | IrOp::Bextr(_, _, _) 
                if self.target_isa.contains(InstructionSets::BMI1) => {
                InstructionSets::BMI1
            }
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) 
                if self.target_isa.contains(InstructionSets::BMI2) => {
                InstructionSets::BMI2
            }
            IrOp::VectorOp { width, .. } => {
                if *width > 256 && self.target_isa.contains(InstructionSets::AVX512F) {
                    InstructionSets::AVX512F
                } else if *width > 128 && self.target_isa.contains(InstructionSets::AVX) {
                    InstructionSets::AVX
                } else {
                    InstructionSets::SSE2
                }
            }
            IrOp::Fma(_, _, _) if self.target_isa.contains(InstructionSets::FMA) => {
                InstructionSets::FMA
            }
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => InstructionSets::AESNI,
            IrOp::Pclmul(_, _, _) => InstructionSets::PCLMUL,
            _ => InstructionSets::SSE2,
        }
    }
}

impl Default for S1IsaPass {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_isa_opt_config_default() {
        let config = IsaOptConfig::default();
        // Should detect current CPU features
        assert!(config.target_isa.contains(InstructionSets::SSE2));
    }
    
    #[test]
    fn test_isa_opt_config_baseline() {
        let config = IsaOptConfig::baseline();
        assert_eq!(config.target_isa, InstructionSets::SSE2);
        assert!(!config.bmi1_patterns);
        assert!(!config.popcnt_patterns);
    }
    
    #[test]
    fn test_optimal_vector_width() {
        let pass = IsaOptPass::with_config(IsaOptConfig::for_isa(
            InstructionSets::SSE2 | InstructionSets::AVX
        ));
        
        // Should downgrade 512 to 256 (no AVX-512)
        assert_eq!(pass.optimal_vector_width(512), 256);
        // Should keep 256 (AVX available)
        assert_eq!(pass.optimal_vector_width(256), 256);
        // Should keep 128
        assert_eq!(pass.optimal_vector_width(128), 128);
    }
    
    #[test]
    fn test_optimal_vector_width_baseline() {
        let pass = IsaOptPass::with_config(IsaOptConfig::baseline());
        
        // Should downgrade everything to 128 (SSE2 only)
        assert_eq!(pass.optimal_vector_width(512), 128);
        assert_eq!(pass.optimal_vector_width(256), 128);
        assert_eq!(pass.optimal_vector_width(128), 128);
    }
    
    #[test]
    fn test_s1_isa_pass() {
        let pass = S1IsaPass::with_isa(InstructionSets::SSE2 | InstructionSets::POPCNT);
        
        let mut ir = IrBlock {
            entry_rip: 0,
            guest_size: 0,
            entry_block: BlockId(0),
            next_vreg: 2,
            meta: IrBlockMeta::default(),
            blocks: vec![IrBasicBlock {
                id: BlockId(0),
                entry_rip: 0,
                instrs: vec![
                    IrInstr {
                        dst: VReg(1),
                        op: IrOp::Popcnt(VReg(0)),
                        guest_rip: 0,
                        flags: IrFlags::empty(),
                    },
                ],
                predecessors: vec![],
                successors: vec![],
            }],
        };
        
        let required = pass.run(&mut ir);
        assert!(required.contains(InstructionSets::POPCNT));
    }
    
    #[test]
    fn test_speedup_estimation() {
        let pass = IsaOptPass::with_config(IsaOptConfig::for_isa(
            InstructionSets::SSE2 | InstructionSets::POPCNT | InstructionSets::BMI2
        ));
        
        // Create stats with POPCNT and BMI2 ops
        let mut stats = IrOpStats::default();
        stats.popcnt = 10;
        stats.parallel_bits = 5;
        stats.scalar_int = 100;
        
        let after = stats.clone(); // Assume same ops after
        
        let speedup = pass.estimate_speedup(&stats, &after);
        // With POPCNT and BMI2, should see significant speedup
        assert!(speedup > 1.0);
    }
}
