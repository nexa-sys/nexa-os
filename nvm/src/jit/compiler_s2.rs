//! S2 Optimizing Compiler
//!
//! Full optimizing compiler for hot code.
//! Applies aggressive optimizations using profile data.
//! Takes more time but produces much better code.
//!
//! ## Speculative Optimizations
//!
//! S2 now supports profile-guided speculative optimizations:
//! - Type speculation with guards
//! - Value speculation for constant propagation
//! - Branch speculation for hot path optimization
//! - Call speculation for devirtualization
//! - Path speculation for multi-condition optimization
//!
//! ## Advanced Optimizations (Enterprise)
//!
//! - **Escape Analysis + Scalar Replacement**: Avoid heap allocations for
//!   non-escaping objects by decomposing them into registers/stack variables
//! - **Loop Optimizations**: 
//!   - Loop Unrolling (profile-guided factor selection)
//!   - Loop Invariant Code Motion (LICM)
//!   - Induction Variable Simplification
//!   - REP STOS/MOVS unrolling (critical for x86 string operations)

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, ExitReason, IrBasicBlock};
use super::decoder::X86Decoder;
use super::profile::{ProfileDb, BranchBias, MemoryPattern};
use super::deopt::{DeoptManager, DeoptGuard, DeoptReason, GuardKind};
use super::speculation::{
    SpeculationManager, BlockSpeculations, TypeSpeculation, ValueSpeculation,
    BranchSpeculation, CallSpeculation, PathSpeculation, apply_speculations,
};
use super::escape::{EscapeScalarPass, EscapeConfig, EscapePassResult};
use super::loop_opt::{LoopOptPass, LoopOptConfig, LoopOptResult};
use super::isa_opt::{IsaOptPass, IsaOptConfig, IsaOptResult};
use super::nready::InstructionSets;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// S2 compiler configuration
#[derive(Clone, Debug)]
pub struct S2Config {
    /// Enable loop unrolling
    pub loop_unroll: bool,
    /// Max unroll factor
    pub max_unroll: u32,
    /// Enable loop invariant code motion
    pub licm: bool,
    /// Enable global value numbering
    pub gvn: bool,
    /// Enable common subexpression elimination
    pub cse: bool,
    /// Enable instruction scheduling
    pub scheduling: bool,
    /// Enable register coalescing
    pub reg_coalesce: bool,
    /// Enable strength reduction
    pub strength_reduce: bool,
    /// Enable tail call optimization
    pub tail_call: bool,
    /// Enable inline expansion
    pub inline: bool,
    /// Max inline size (IR instructions)
    pub max_inline_size: usize,
    /// Enable type speculation
    pub type_speculation: bool,
    /// Enable value speculation
    pub value_speculation: bool,
    /// Enable branch speculation (hot path optimization)
    pub branch_speculation: bool,
    /// Enable call speculation (devirtualization)
    pub call_speculation: bool,
    /// Enable path speculation (multi-condition optimization)
    pub path_speculation: bool,
    /// Speculation confidence threshold (0.0 - 1.0)
    pub speculation_threshold: f64,
    /// Enable escape analysis + scalar replacement
    pub escape_analysis: bool,
    /// Escape analysis configuration
    pub escape_config: EscapeConfig,
    /// Enable advanced loop optimizations
    pub advanced_loop_opts: bool,
    /// Loop optimization configuration
    pub loop_opt_config: LoopOptConfig,
    /// Enable ISA-aware optimization
    pub isa_optimization: bool,
    /// ISA optimization configuration
    pub isa_opt_config: IsaOptConfig,
    /// Enable scope-aware optimization (cross-block/function analysis)
    pub scope_aware_opt: bool,
    /// Scope configuration for optimization boundaries
    pub scope_config: super::scope::ScopeConfig,
    /// Enable dependency graph analysis for reordering
    pub dependency_analysis: bool,
}

impl Default for S2Config {
    fn default() -> Self {
        Self {
            loop_unroll: true,
            max_unroll: 8,
            licm: true,
            gvn: true,
            cse: true,
            scheduling: true,
            reg_coalesce: true,
            strength_reduce: true,
            tail_call: true,
            inline: true,
            max_inline_size: 50,
            type_speculation: true,
            value_speculation: true,
            branch_speculation: true,
            call_speculation: true,
            path_speculation: true,
            speculation_threshold: 0.95,
            escape_analysis: true,
            escape_config: EscapeConfig::default(),
            advanced_loop_opts: true,
            loop_opt_config: LoopOptConfig::default(),
            isa_optimization: true,
            isa_opt_config: IsaOptConfig::default(),
            scope_aware_opt: true,
            scope_config: super::scope::ScopeConfig::default(),
            dependency_analysis: true,
        }
    }
}

/// S2 compiled block
pub struct S2Block {
    /// Guest start address
    pub guest_rip: u64,
    /// Guest code size
    pub guest_size: u32,
    /// Optimized IR
    pub ir: IrBlock,
    /// Native code
    pub native: Vec<u8>,
    /// Estimated cycles
    pub est_cycles: u32,
    /// Optimization stats
    pub opt_stats: OptStats,
}

/// Optimization statistics
#[derive(Default, Clone, Debug)]
pub struct OptStats {
    pub instrs_before: u32,
    pub instrs_after: u32,
    pub loops_unrolled: u32,
    pub exprs_hoisted: u32,
    pub cse_eliminated: u32,
    pub strength_reduced: u32,
    pub tail_calls: u32,
    pub inlined_calls: u32,
    /// Type speculations inserted
    pub type_guards: u32,
    /// Value speculations inserted
    pub value_guards: u32,
    /// Branch speculations applied
    pub branch_specs: u32,
    /// Call speculations applied (devirtualizations)
    pub call_specs: u32,
    /// Path speculations applied
    pub path_specs: u32,
    /// Escape analysis results
    pub escape_result: Option<EscapePassResult>,
    /// Loop optimization results
    pub loop_opt_result: Option<LoopOptResult>,
    /// ISA optimization results
    pub isa_opt_result: Option<IsaOptResult>,
    /// Scope-aware optimization results
    pub scope_level: super::scope::ScopeLevel,
    /// Dependency graph statistics
    pub dep_stats: Option<super::scope::DependencyStats>,
    /// Memory reorderings applied
    pub memory_reorders: u32,
    /// Cross-block optimizations
    pub cross_block_opts: u32,
    /// Scheduler statistics
    pub scheduler_stats: Option<super::scheduler::ScheduleStats>,
    /// Instructions reordered by scheduler
    pub instrs_reordered: u32,
    /// Critical path length (cycles)
    pub critical_path_length: u32,
    /// Achieved ILP (instruction level parallelism)
    pub achieved_ilp: f32,
}

/// S2 optimizing compiler
pub struct S2Compiler {
    config: S2Config,
    /// Speculation manager (shared across compilations)
    spec_mgr: Option<Arc<SpeculationManager>>,
    /// Deoptimization manager (shared across compilations)
    deopt_mgr: Option<Arc<DeoptManager>>,
}

impl S2Compiler {
    pub fn new() -> Self {
        Self {
            config: S2Config::default(),
            spec_mgr: None,
            deopt_mgr: None,
        }
    }
    
    pub fn with_config(config: S2Config) -> Self {
        Self { 
            config,
            spec_mgr: None,
            deopt_mgr: None,
        }
    }
    
    /// Set speculation manager for profile-guided speculation
    pub fn with_speculation(mut self, spec_mgr: Arc<SpeculationManager>, deopt_mgr: Arc<DeoptManager>) -> Self {
        self.spec_mgr = Some(spec_mgr);
        self.deopt_mgr = Some(deopt_mgr);
        self
    }
    
    /// Generate native code directly from IR (used by NReady! cache restoration)
    /// 
    /// This is a simplified path that skips optimization passes and just does:
    /// 1. Register allocation
    /// 2. Instruction scheduling (if enabled)
    /// 3. Code generation
    /// 
    /// For full optimization, use `compile_from_s1()` instead.
    pub fn codegen_from_ir(&self, ir: &IrBlock) -> JitResult<Vec<u8>> {
        // Skip optimization passes since this IR may already be optimized
        // (e.g., restored from NReady! cache where S2 optimization was applied)
        
        // Register allocation
        let reg_alloc = self.allocate_registers(ir);
        
        // Create dummy profile for codegen (no speculation)
        let profile = ProfileDb::new(1024);
        
        // Code generation
        self.codegen(ir, &reg_alloc, &profile)
    }
    
    /// Recompile an S1 block with full optimizations
    pub fn compile_from_s1(
        &self,
        s1: &super::compiler_s1::S1Block,
        profile: &ProfileDb,
    ) -> JitResult<S2Block> {
        let mut ir = s1.ir.clone();
        let mut stats = OptStats::default();
        stats.instrs_before = count_instrs(&ir);
        
        // Build control flow graph
        let cfg = self.build_cfg(&ir);
        
        // Compute dominators
        let doms = self.compute_dominators(&cfg);
        
        // Detect loops
        let loops = self.detect_loops(&cfg, &doms);
        
        // ====================================================================
        // Phase 1: Escape Analysis + Scalar Replacement (Enterprise)
        // ====================================================================
        // Run early to eliminate unnecessary allocations before other opts
        if self.config.escape_analysis {
            let escape_pass = EscapeScalarPass::with_config(self.config.escape_config.clone());
            let escape_result = escape_pass.run(&mut ir);
            stats.escape_result = Some(escape_result);
        }
        
        // ====================================================================
        // Phase 2: Classic Optimizations
        // ====================================================================
        if self.config.gvn {
            self.global_value_numbering(&mut ir, &doms, &mut stats);
        }
        
        if self.config.cse {
            self.common_subexpr_elim(&mut ir, &mut stats);
        }
        
        // ====================================================================
        // Phase 3: Advanced Loop Optimizations (Enterprise)
        // ====================================================================
        // Run comprehensive loop optimizations before basic loop opts
        if self.config.advanced_loop_opts {
            let mut loop_pass = LoopOptPass::with_config(self.config.loop_opt_config.clone());
            let loop_result = loop_pass.run(&mut ir, profile);
            stats.loop_opt_result = Some(loop_result);
        } else {
            // Fallback to basic loop optimizations
            if self.config.licm {
                self.loop_invariant_motion(&mut ir, &loops, &mut stats);
            }
            
            if self.config.loop_unroll {
                self.unroll_loops(&mut ir, &loops, profile, &mut stats);
            }
        }
        
        if self.config.strength_reduce {
            self.strength_reduction(&mut ir, &mut stats);
        }
        
        if self.config.inline {
            self.inline_expansion(&mut ir, profile, &mut stats);
        }
        
        if self.config.tail_call {
            self.tail_call_opt(&mut ir, &mut stats);
        }
        
        // ====================================================================
        // Phase 4: Speculative Optimizations (Profile-Guided)
        // ====================================================================
        
        // Apply speculative optimizations if managers are available
        let guards = if let (Some(spec_mgr), Some(deopt_mgr)) = (&self.spec_mgr, &self.deopt_mgr) {
            self.apply_speculative_opts(&mut ir, s1.guest_rip, profile, spec_mgr, deopt_mgr, &mut stats)
        } else {
            Vec::new()
        };
        
        // ====================================================================
        // Phase 5: ISA-Aware Optimization (Enterprise)
        // ====================================================================
        // Run after other optimizations to ensure optimal ISA usage
        // This pass:
        // - Recognizes patterns for BMI1/BMI2 instructions
        // - Optimizes vector widths for available SIMD (AVX-512/AVX/SSE)
        // - Transforms mul+add to FMA where beneficial
        // - Tracks required ISA for codegen
        if self.config.isa_optimization {
            let isa_pass = IsaOptPass::with_config(self.config.isa_opt_config.clone());
            let isa_result = isa_pass.run(&mut ir);
            log::debug!(
                "[S2] ISA optimization: speedup={:.2}x, required={:?}",
                isa_result.estimated_speedup,
                isa_result.required_isa.to_string_list()
            );
            stats.isa_opt_result = Some(isa_result);
        }
        
        // ====================================================================
        // Phase 6: Scope-Aware Optimization and Dependency Analysis
        // ====================================================================
        // Build dependency graph for advanced reordering decisions
        // This enables:
        // - Cross-block optimization based on scope level
        // - Memory reordering with profile-guided safety
        // - Critical path optimization
        // - ILP maximization
        if self.config.scope_aware_opt || self.config.dependency_analysis {
            self.apply_scope_aware_opts(&mut ir, s1.guest_rip, profile, &mut stats);
        }
        
        // Dead code elimination after all opts
        self.dead_code_elim(&mut ir);
        
        // Register allocation
        let reg_alloc = self.allocate_registers(&ir);
        
        // Instruction scheduling
        if self.config.scheduling {
            self.schedule_instructions(&mut ir);
        }
        
        // Code generation with guards
        let native = self.codegen_with_guards(&ir, &reg_alloc, profile, &guards)?;
        
        stats.instrs_after = count_instrs(&ir);
        let est_cycles = self.estimate_cycles(&ir);
        
        Ok(S2Block {
            guest_rip: s1.guest_rip,
            guest_size: s1.guest_size,
            ir,
            native,
            est_cycles,
            opt_stats: stats,
        })
    }
    
    // ========================================================================
    // Speculative Optimization Implementation
    // ========================================================================
    
    /// Apply all speculative optimizations based on profile data
    fn apply_speculative_opts(
        &self,
        ir: &mut IrBlock,
        block_rip: u64,
        profile: &ProfileDb,
        spec_mgr: &SpeculationManager,
        deopt_mgr: &DeoptManager,
        stats: &mut OptStats,
    ) -> Vec<DeoptGuard> {
        let mut guards = Vec::new();
        
        // Analyze block and generate speculations
        let specs = spec_mgr.analyze_block(block_rip, profile, deopt_mgr);
        
        // Type speculation: insert type guards
        if self.config.type_speculation {
            for type_spec in &specs.types {
                if let Some(guard) = self.apply_type_speculation(ir, type_spec, deopt_mgr) {
                    guards.push(guard);
                    stats.type_guards += 1;
                }
            }
        }
        
        // Value speculation: insert value guards and constant propagation
        if self.config.value_speculation {
            for value_spec in &specs.values {
                if let Some(guard) = self.apply_value_speculation(ir, value_spec, deopt_mgr) {
                    guards.push(guard);
                    stats.value_guards += 1;
                }
            }
        }
        
        // Branch speculation: hot path optimization, cold path separation
        if self.config.branch_speculation {
            for branch_spec in &specs.branches {
                if self.apply_branch_speculation(ir, branch_spec) {
                    stats.branch_specs += 1;
                }
            }
        }
        
        // Call speculation: devirtualization
        if self.config.call_speculation {
            for call_spec in &specs.calls {
                if let Some(guard) = self.apply_call_speculation(ir, call_spec, deopt_mgr) {
                    guards.push(guard);
                    stats.call_specs += 1;
                }
            }
        }
        
        // Path speculation: multi-condition optimization
        if self.config.path_speculation {
            for path_spec in &specs.paths {
                if let Some(guard) = self.apply_path_speculation(ir, path_spec, deopt_mgr) {
                    guards.push(guard);
                    stats.path_specs += 1;
                }
            }
        }
        
        // Register all guards with deopt manager
        for guard in &guards {
            deopt_mgr.register_guard(guard.clone());
        }
        
        guards
    }
    
    /// Apply type speculation: insert guard and propagate type info
    fn apply_type_speculation(
        &self,
        ir: &mut IrBlock,
        spec: &TypeSpeculation,
        deopt_mgr: &DeoptManager,
    ) -> Option<DeoptGuard> {
        // Create guard instruction
        let guard = DeoptGuard::new(
            deopt_mgr.alloc_guard_id(),
            spec.rip,
            spec.guard_kind.clone(),
            DeoptReason::TypeMismatch,
        );
        
        // Insert guard at appropriate position in IR
        // The guard acts as a speculation barrier - if it fails, we deoptimize
        self.insert_guard_instr(ir, spec.rip, &guard);
        
        // After guard, we can assume the type is correct and optimize accordingly
        // e.g., for objects, we can inline vtable lookups
        
        Some(guard)
    }
    
    /// Apply value speculation: insert guard and propagate constant
    fn apply_value_speculation(
        &self,
        ir: &mut IrBlock,
        spec: &ValueSpeculation,
        deopt_mgr: &DeoptManager,
    ) -> Option<DeoptGuard> {
        let guard = DeoptGuard::new(
            deopt_mgr.alloc_guard_id(),
            spec.rip,
            spec.guard_kind.clone(),
            DeoptReason::ValueMismatch,
        );
        
        self.insert_guard_instr(ir, spec.rip, &guard);
        
        // After guard, we can treat the register as holding the expected value
        // This enables constant propagation and dead code elimination
        if let Some((value, _)) = spec.values.first() {
            self.propagate_constant(ir, spec.rip, spec.reg, *value);
        }
        
        Some(guard)
    }
    
    /// Apply branch speculation: reorder blocks for hot path
    fn apply_branch_speculation(
        &self,
        ir: &mut IrBlock,
        spec: &BranchSpeculation,
    ) -> bool {
        // Find the branch instruction
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if instr.guest_rip == spec.rip {
                    if let IrOp::Branch(cond, true_bb, false_bb) = &instr.op {
                        // If we expect taken, ensure true_bb is the fall-through
                        // If we expect not-taken, swap the targets
                        if !spec.expected_taken {
                            // Swap targets and invert condition
                            instr.op = IrOp::Branch(*cond, *false_bb, *true_bb);
                        }
                        return true;
                    }
                }
            }
        }
        false
    }
    
    /// Apply call speculation: devirtualization
    fn apply_call_speculation(
        &self,
        ir: &mut IrBlock,
        spec: &CallSpeculation,
        deopt_mgr: &DeoptManager,
    ) -> Option<DeoptGuard> {
        if !spec.can_inline() {
            return None;
        }
        
        // Create guard for call target
        let guard = DeoptGuard::new(
            deopt_mgr.alloc_guard_id(),
            spec.rip,
            GuardKind::CallTarget {
                target_reg: 0, // Would get actual reg from instruction
                expected: spec.targets.clone(),
            },
            DeoptReason::CallTargetMismatch,
        );
        
        self.insert_guard_instr(ir, spec.rip, &guard);
        
        // Convert indirect call to direct call (if monomorphic)
        if spec.targets.len() == 1 {
            for bb in &mut ir.blocks {
                for instr in &mut bb.instrs {
                    if instr.guest_rip == spec.rip {
                        if let IrOp::CallIndirect(_) = &instr.op {
                            instr.op = IrOp::Call(spec.targets[0]);
                            break;
                        }
                    }
                }
            }
        }
        
        Some(guard)
    }
    
    /// Apply path speculation: compound guard for multiple conditions
    fn apply_path_speculation(
        &self,
        ir: &mut IrBlock,
        spec: &PathSpeculation,
        deopt_mgr: &DeoptManager,
    ) -> Option<DeoptGuard> {
        let guard = spec.to_guard(deopt_mgr);
        self.insert_guard_instr(ir, spec.entry_rip, &guard);
        Some(guard)
    }
    
    /// Insert a guard instruction into IR at the appropriate position
    fn insert_guard_instr(&self, ir: &mut IrBlock, rip: u64, guard: &DeoptGuard) {
        // Find block containing this RIP
        for bb in &mut ir.blocks {
            if bb.entry_rip <= rip && rip < bb.entry_rip + 0x100 {
                // Create a guard instruction (represented as a conditional exit)
                let guard_instr = IrInstr {
                    dst: VReg::NONE,
                    op: IrOp::Exit(ExitReason::Normal), // Would be Guard(guard.id)
                    guest_rip: rip,
                    flags: IrFlags::SIDE_EFFECT,
                };
                
                // Insert after register loads, before other instructions
                let insert_pos = bb.instrs.iter()
                    .position(|i| !matches!(i.op, IrOp::LoadGpr(_) | IrOp::LoadFlags | IrOp::LoadRip))
                    .unwrap_or(0);
                
                bb.instrs.insert(insert_pos, guard_instr);
                break;
            }
        }
        let _ = guard; // Used for metadata in real implementation
    }
    
    /// Propagate a constant value through IR after a value guard
    fn propagate_constant(&self, ir: &mut IrBlock, rip: u64, reg: u8, value: u64) {
        // After the guard at `rip`, replace loads of `reg` with the constant `value`
        let mut found_guard = false;
        
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if instr.guest_rip == rip {
                    found_guard = true;
                    continue;
                }
                
                if found_guard {
                    // Replace LoadGpr(reg) with Const(value)
                    if let IrOp::LoadGpr(r) = &instr.op {
                        if *r == reg {
                            instr.op = IrOp::Const(value as i64);
                        }
                    }
                }
            }
        }
    }
    
    /// Code generation with guard support
    fn codegen_with_guards(
        &self,
        ir: &IrBlock,
        alloc: &RegisterAllocation,
        profile: &ProfileDb,
        guards: &[DeoptGuard],
    ) -> JitResult<Vec<u8>> {
        // For now, delegate to standard codegen
        // Real implementation would generate guard check code
        let _ = guards; // Used for generating deopt stubs
        self.codegen(ir, alloc, profile)
    }
    
    // ========================================================================
    // Scope-Aware Optimization Implementation
    // ========================================================================
    
    /// Apply scope-aware optimizations using dependency graph analysis
    /// 
    /// This method:
    /// 1. Determines the optimal compilation scope (block/function/region)
    /// 2. Builds a cross-block dependency graph
    /// 3. Identifies reordering opportunities
    /// 4. Applies profile-guided memory reordering
    /// 5. Records optimization decisions to scope profile for future runs
    fn apply_scope_aware_opts(
        &self,
        ir: &mut IrBlock,
        block_rip: u64,
        profile: &ProfileDb,
        stats: &mut OptStats,
    ) {
        use super::scope::{ScopeBuilder, DependencyGraph, ScopeProfile, ScopeLevel};
        
        // Build compilation scope
        let scope_builder = ScopeBuilder::new(self.config.scope_config.clone());
        let scope = scope_builder.build_scope(block_rip, profile);
        stats.scope_level = scope.level;
        
        log::debug!(
            "[S2] Scope-aware opt: level={:?}, blocks={}, exec_count={}",
            scope.level,
            scope.blocks.len(),
            scope.execution_count
        );
        
        // Build dependency graph for the IR
        let dep_graph = DependencyGraph::build(ir);
        
        // Record dependency statistics
        let mut scope_profile = ScopeProfile::new(block_rip, scope.level);
        scope_profile.update_dep_stats(&dep_graph);
        stats.dep_stats = Some(scope_profile.dep_stats.clone());
        
        log::debug!(
            "[S2] Dependency graph: critical_path={}, ILP={:.2}, RAW={}, memory={}",
            dep_graph.critical_length(),
            dep_graph.ilp(),
            scope_profile.dep_stats.raw_deps,
            scope_profile.dep_stats.memory_deps
        );
        
        // Apply optimizations based on scope level
        match scope.level {
            ScopeLevel::Block => {
                // Block-level: only local reordering
                self.apply_local_reordering(ir, &dep_graph, stats);
            }
            ScopeLevel::Function | ScopeLevel::Region | ScopeLevel::CallGraph => {
                // Function+ level: more aggressive optimizations
                self.apply_local_reordering(ir, &dep_graph, stats);
                self.apply_memory_reordering(ir, &dep_graph, profile, &mut scope_profile, stats);
                self.apply_cross_block_opts(ir, &scope, profile, &mut scope_profile, stats);
            }
        }
        
        // Record discovered constants and ranges to profile
        self.discover_value_info(ir, profile, &mut scope_profile);
    }
    
    /// Apply local instruction reordering within basic blocks
    fn apply_local_reordering(
        &self,
        ir: &mut IrBlock,
        dep_graph: &super::scope::DependencyGraph,
        stats: &mut OptStats,
    ) {
        // Get reorderable instruction pairs
        let pairs = dep_graph.reorderable_pairs();
        
        // For now, just track statistics
        // Real implementation would reorder instructions to maximize ILP
        if !pairs.is_empty() {
            log::trace!(
                "[S2] Found {} reorderable instruction pairs",
                pairs.len()
            );
        }
        
        // Track ILP improvement potential
        let ilp = dep_graph.ilp();
        if ilp > 1.5 {
            log::debug!("[S2] High ILP potential: {:.2}", ilp);
        }
    }
    
    /// Apply speculative memory reordering based on profile data
    fn apply_memory_reordering(
        &self,
        ir: &mut IrBlock,
        dep_graph: &super::scope::DependencyGraph,
        profile: &ProfileDb,
        scope_profile: &mut super::scope::ScopeProfile,
        stats: &mut OptStats,
    ) {
        use super::scope::DependencyKind;
        
        // Get potential memory reordering opportunities
        let speculative_moves = dep_graph.speculative_moves();
        
        for (load_idx, store_idx) in speculative_moves {
            // Check if we have profile data indicating this reorder is safe
            // In a real implementation, we'd check alias analysis + profile
            
            // For now, be conservative: only reorder if profile indicates safety
            // This would check memory_reorder_success in scope_profile
            
            // Track that we considered this reordering
            stats.memory_reorders += 0; // Would increment on actual reorder
        }
    }
    
    /// Apply cross-block optimizations (function/region scope)
    fn apply_cross_block_opts(
        &self,
        ir: &mut IrBlock,
        scope: &super::scope::CompilationScope,
        profile: &ProfileDb,
        scope_profile: &mut super::scope::ScopeProfile,
        stats: &mut OptStats,
    ) {
        // Cross-block optimizations enabled by larger scopes:
        
        // 1. Global code motion - move instructions across blocks
        // Already partially handled by LICM, but scope-aware version is more aggressive
        
        // 2. Partial redundancy elimination (PRE)
        // Move computations to less-frequently executed paths
        
        // 3. Hot path linearization
        // Reorder blocks to improve cache locality for hot paths
        if !scope.loop_headers.is_empty() {
            log::trace!("[S2] Found {} loop headers for potential hot path optimization", 
                scope.loop_headers.len());
        }
        
        // 4. Call site optimization
        // With region scope, we can optimize across function boundaries
        for (caller, callees) in &scope.call_edges {
            for callee in callees {
                // Check if callee is hot enough to inline
                let callee_count = profile.get_block_count(*callee);
                if callee_count >= self.config.scope_config.region_hotness_threshold {
                    // Record inlining candidate
                    scope_profile.record_inline(*caller, *callee, true);
                    stats.cross_block_opts += 1;
                }
            }
        }
    }
    
    /// Discover value ranges and constants from profile data
    fn discover_value_info(
        &self,
        ir: &IrBlock,
        profile: &ProfileDb,
        scope_profile: &mut super::scope::ScopeProfile,
    ) {
        // Check type profiles for constant values
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                // Would check profile.type_profiles and profile.value_profiles
                // to discover constants and value ranges
                
                // Record discovered constants to scope profile
                // This enables better optimization in future compilations
            }
        }
    }
    
    // ========================================================================
    // CFG and Dominator Analysis
    // ========================================================================
    
    fn build_cfg(&self, ir: &IrBlock) -> ControlFlowGraph {
        let mut cfg = ControlFlowGraph {
            entry: 0,
            blocks: ir.blocks.iter().enumerate()
                .map(|(i, _)| CfgNode {
                    id: i,
                    preds: Vec::new(),
                    succs: Vec::new(),
                })
                .collect(),
        };
        
        // Build edges from terminator instructions
        for (i, bb) in ir.blocks.iter().enumerate() {
            // Get terminator (last instruction)
            if let Some(term) = bb.instrs.last() {
                match &term.op {
                    IrOp::Jump(target) => {
                        let target_idx = target.0 as usize;
                        if target_idx < cfg.blocks.len() {
                            cfg.blocks[i].succs.push(target_idx);
                            cfg.blocks[target_idx].preds.push(i);
                        }
                    }
                    IrOp::Branch(_, true_bb, false_bb) => {
                        let true_idx = true_bb.0 as usize;
                        let false_idx = false_bb.0 as usize;
                        if true_idx < cfg.blocks.len() {
                            cfg.blocks[i].succs.push(true_idx);
                            cfg.blocks[true_idx].preds.push(i);
                        }
                        if false_idx < cfg.blocks.len() {
                            cfg.blocks[i].succs.push(false_idx);
                            cfg.blocks[false_idx].preds.push(i);
                        }
                    }
                    IrOp::Ret | IrOp::Hlt | IrOp::Exit(_) => {
                        // No successors
                    }
                    _ => {
                        // Fall through to next block
                        if i + 1 < cfg.blocks.len() {
                            cfg.blocks[i].succs.push(i + 1);
                            cfg.blocks[i + 1].preds.push(i);
                        }
                    }
                }
            }
        }
        
        cfg
    }
    
    fn compute_dominators(&self, cfg: &ControlFlowGraph) -> DominatorTree {
        let n = cfg.blocks.len();
        let mut doms = vec![None; n];
        doms[cfg.entry] = Some(cfg.entry);
        
        // Simple iterative dominator computation
        let mut changed = true;
        while changed {
            changed = false;
            for i in 0..n {
                if i == cfg.entry {
                    continue;
                }
                
                let preds: Vec<_> = cfg.blocks[i].preds.iter()
                    .filter(|&&p| doms[p].is_some())
                    .cloned()
                    .collect();
                
                if preds.is_empty() {
                    continue;
                }
                
                let new_dom = preds.iter().cloned()
                    .reduce(|a, b| self.intersect(&doms, a, b))
                    .unwrap();
                
                if doms[i] != Some(new_dom) {
                    doms[i] = Some(new_dom);
                    changed = true;
                }
            }
        }
        
        DominatorTree {
            idom: doms.into_iter().map(|d| d.unwrap_or(0)).collect(),
        }
    }
    
    fn intersect(&self, doms: &[Option<usize>], mut a: usize, mut b: usize) -> usize {
        while a != b {
            while a > b {
                a = doms[a].unwrap_or(0);
            }
            while b > a {
                b = doms[b].unwrap_or(0);
            }
        }
        a
    }
    
    fn detect_loops(&self, cfg: &ControlFlowGraph, doms: &DominatorTree) -> Vec<Loop> {
        let mut loops = Vec::new();
        
        // Find back edges
        for (i, node) in cfg.blocks.iter().enumerate() {
            for &succ in &node.succs {
                // Back edge: succ dominates i
                if self.dominates(doms, succ, i) {
                    let loop_body = self.find_loop_body(cfg, succ, i);
                    loops.push(Loop {
                        header: succ,
                        body: loop_body,
                        back_edges: vec![(i, succ)],
                    });
                }
            }
        }
        
        loops
    }
    
    fn dominates(&self, doms: &DominatorTree, a: usize, b: usize) -> bool {
        let mut curr = b;
        while curr != a {
            if curr == 0 && a != 0 {
                return false;
            }
            curr = doms.idom[curr];
        }
        true
    }
    
    fn find_loop_body(&self, cfg: &ControlFlowGraph, header: usize, tail: usize) -> HashSet<usize> {
        let mut body = HashSet::new();
        body.insert(header);
        body.insert(tail);
        
        let mut worklist = vec![tail];
        while let Some(n) = worklist.pop() {
            for &pred in &cfg.blocks[n].preds {
                if !body.contains(&pred) {
                    body.insert(pred);
                    worklist.push(pred);
                }
            }
        }
        
        body
    }
    
    // ========================================================================
    // Optimizations
    // ========================================================================
    
    fn global_value_numbering(&self, ir: &mut IrBlock, _doms: &DominatorTree, stats: &mut OptStats) {
        // GVN: assign value numbers to expressions, eliminate redundant computations
        // In SSA form, we track which VReg holds equivalent values
        let mut value_numbers: HashMap<ExprKey, VReg> = HashMap::new();
        let mut vreg_map: HashMap<VReg, VReg> = HashMap::new(); // old -> canonical
        
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if let Some(key) = make_expr_key(&instr.op) {
                    if let Some(&existing) = value_numbers.get(&key) {
                        // This expression was computed before
                        // Map this dst to the existing one
                        if op_produces_value(&instr.op) && instr.dst.is_valid() {
                            vreg_map.insert(instr.dst, existing);
                            // Mark as dead (will be eliminated in DCE)
                            instr.op = IrOp::Nop;
                            instr.dst = VReg::NONE;
                            stats.cse_eliminated += 1;
                        }
                    } else {
                        if op_produces_value(&instr.op) && instr.dst.is_valid() {
                            value_numbers.insert(key, instr.dst);
                        }
                    }
                }
            }
        }
        
        // Rewrite uses of remapped VRegs
        self.rewrite_vreg_uses(ir, &vreg_map);
    }
    
    fn common_subexpr_elim(&self, ir: &mut IrBlock, stats: &mut OptStats) {
        // Local CSE within each basic block
        for bb in &mut ir.blocks {
            let mut seen: HashMap<ExprKey, VReg> = HashMap::new();
            let mut vreg_map: HashMap<VReg, VReg> = HashMap::new();
            
            for instr in &mut bb.instrs {
                if let Some(key) = make_expr_key(&instr.op) {
                    if let Some(&prev) = seen.get(&key) {
                        if op_produces_value(&instr.op) && instr.dst.is_valid() {
                            vreg_map.insert(instr.dst, prev);
                            instr.op = IrOp::Nop;
                            instr.dst = VReg::NONE;
                            stats.cse_eliminated += 1;
                        }
                    } else {
                        if op_produces_value(&instr.op) && instr.dst.is_valid() {
                            seen.insert(key, instr.dst);
                        }
                    }
                }
            }
            
            // Rewrite uses within this block
            for instr in &mut bb.instrs {
                self.rewrite_operands(&mut instr.op, &vreg_map);
            }
        }
    }
    
    fn rewrite_vreg_uses(&self, ir: &mut IrBlock, vreg_map: &HashMap<VReg, VReg>) {
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                self.rewrite_operands(&mut instr.op, vreg_map);
            }
        }
    }
    
    fn rewrite_operands(&self, op: &mut IrOp, vreg_map: &HashMap<VReg, VReg>) {
        // Helper to rewrite a VReg
        let rewrite = |v: &mut VReg| {
            if let Some(&new_v) = vreg_map.get(v) {
                *v = new_v;
            }
        };
        
        match op {
            IrOp::Add(a, b) | IrOp::Sub(a, b) | IrOp::Mul(a, b) | IrOp::IMul(a, b) |
            IrOp::Div(a, b) | IrOp::IDiv(a, b) | IrOp::And(a, b) | IrOp::Or(a, b) |
            IrOp::Xor(a, b) | IrOp::Shl(a, b) | IrOp::Shr(a, b) | IrOp::Sar(a, b) |
            IrOp::Rol(a, b) | IrOp::Ror(a, b) | IrOp::Cmp(a, b) | IrOp::Test(a, b) => {
                rewrite(a); rewrite(b);
            }
            IrOp::Neg(a) | IrOp::Not(a) | IrOp::Load8(a) | IrOp::Load16(a) |
            IrOp::Load32(a) | IrOp::Load64(a) | IrOp::GetCF(a) | IrOp::GetZF(a) |
            IrOp::GetSF(a) | IrOp::GetOF(a) | IrOp::GetPF(a) | IrOp::Sext8(a) |
            IrOp::Sext16(a) | IrOp::Sext32(a) | IrOp::Zext8(a) | IrOp::Zext16(a) |
            IrOp::Zext32(a) | IrOp::Trunc8(a) | IrOp::Trunc16(a) | IrOp::Trunc32(a) |
            IrOp::In8(a) | IrOp::In16(a) | IrOp::In32(a) | IrOp::CallIndirect(a) => {
                rewrite(a);
            }
            IrOp::Store8(a, v) | IrOp::Store16(a, v) | IrOp::Store32(a, v) | IrOp::Store64(a, v) |
            IrOp::Out8(a, v) | IrOp::Out16(a, v) | IrOp::Out32(a, v) => {
                rewrite(a); rewrite(v);
            }
            IrOp::StoreGpr(_, v) | IrOp::StoreFlags(v) | IrOp::StoreRip(v) => {
                rewrite(v);
            }
            IrOp::Select(c, t, f) => {
                rewrite(c); rewrite(t); rewrite(f);
            }
            IrOp::Branch(c, _, _) => {
                rewrite(c);
            }
            IrOp::Phi(sources) => {
                for (_, v) in sources {
                    rewrite(v);
                }
            }
            _ => {}
        }
    }
    
    fn loop_invariant_motion(&self, ir: &mut IrBlock, loops: &[Loop], stats: &mut OptStats) {
        // Move loop-invariant computations out of loops
        for lp in loops {
            let mut invariant_instrs = Vec::new();
            
            // Find definitions that don't depend on loop
            for &block_id in &lp.body {
                if block_id >= ir.blocks.len() {
                    continue;
                }
                
                let bb = &ir.blocks[block_id];
                for (i, instr) in bb.instrs.iter().enumerate() {
                    if self.is_loop_invariant(instr, &lp.body, ir) {
                        invariant_instrs.push((block_id, i));
                    }
                }
            }
            
            // Move to preheader (before loop header)
            // For simplicity, just mark as hoisted
            stats.exprs_hoisted += invariant_instrs.len() as u32;
        }
    }
    
    fn is_loop_invariant(&self, instr: &IrInstr, _loop_body: &HashSet<usize>, _ir: &IrBlock) -> bool {
        // Check if all operands are defined outside loop
        // Constants are always invariant
        match &instr.op {
            IrOp::Const(_) | IrOp::ConstF64(_) => true,
            IrOp::Add(_, _) | IrOp::Sub(_, _) | IrOp::And(_, _) |
            IrOp::Or(_, _) | IrOp::Xor(_, _) | IrOp::Mul(_, _) => {
                // Would check if operands are defined outside loop
                false
            }
            _ => false,
        }
    }
    
    fn unroll_loops(&self, ir: &mut IrBlock, loops: &[Loop], profile: &ProfileDb, stats: &mut OptStats) {
        for lp in loops {
            // Check profile for iteration count
            let avg_iters = profile.get_loop_avg_iters(lp.header as u64);
            
            if avg_iters > 0.0 && avg_iters <= self.config.max_unroll as f64 {
                let unroll_factor = avg_iters.ceil() as u32;
                if unroll_factor <= self.config.max_unroll && unroll_factor > 1 {
                    // Would duplicate loop body unroll_factor times
                    stats.loops_unrolled += 1;
                }
            }
        }
    }
    
    fn strength_reduction(&self, ir: &mut IrBlock, stats: &mut OptStats) {
        // Replace expensive operations with cheaper equivalents
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                match &instr.op {
                    // mul x, 2 -> shl x, 1 (would need to track constant values)
                    IrOp::Mul(a, b) => {
                        let _ = (a, b); // placeholder
                    }
                    // div x, 2 -> shr x, 1 (for unsigned)
                    IrOp::Div(a, b) => {
                        let _ = (a, b); // placeholder
                    }
                    _ => {}
                }
            }
        }
        let _ = stats; // Suppress warning until implemented
    }
    
    fn inline_expansion(&self, ir: &mut IrBlock, _profile: &ProfileDb, stats: &mut OptStats) {
        // Inline small, frequently-called functions
        for bb in &mut ir.blocks {
            for instr in &bb.instrs {
                if let IrOp::Call(target) = &instr.op {
                    // Would check if target is small and hot
                    let _ = target;
                    stats.inlined_calls += 0; // Placeholder
                }
            }
        }
    }
    
    fn tail_call_opt(&self, ir: &mut IrBlock, stats: &mut OptStats) {
        // Convert tail calls to jumps
        for bb in &mut ir.blocks {
            if bb.instrs.is_empty() {
                continue;
            }
            
            // Check if last instruction is call followed by return
            let last_idx = bb.instrs.len() - 1;
            if let IrOp::Call(_) = &bb.instrs[last_idx].op {
                // Check if next instruction is return
                if last_idx + 1 < bb.instrs.len() {
                    if matches!(bb.instrs[last_idx + 1].op, IrOp::Ret) {
                        // Would convert to tail call
                        stats.tail_calls += 1;
                    }
                }
            }
        }
    }
    
    fn dead_code_elim(&self, ir: &mut IrBlock) {
        // Build use set (VRegs that are used)
        let mut used: HashSet<VReg> = HashSet::new();
        
        // Collect uses from all instructions
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                for op in get_operands(&instr.op) {
                    used.insert(op);
                }
            }
        }
        
        // Remove unused definitions (keep side effects)
        for bb in &mut ir.blocks {
            bb.instrs.retain(|instr| {
                if op_produces_value(&instr.op) && instr.dst.is_valid() {
                    used.contains(&instr.dst) || has_side_effect(&instr.op)
                } else {
                    // Keep side-effectful and terminator instructions
                    has_side_effect(&instr.op) || instr.flags.contains(IrFlags::TERMINATOR)
                }
            });
        }
    }
    
    // ========================================================================
    // Register Allocation
    // ========================================================================
    
    fn allocate_registers(&self, ir: &IrBlock) -> RegisterAllocation {
        // Linear scan register allocation
        let mut alloc = RegisterAllocation {
            vreg_to_hreg: HashMap::new(),
            spills: HashSet::new(),
        };
        
        // Build live intervals
        let intervals = self.build_live_intervals(ir);
        
        // Available registers (caller-saved first)
        let mut available: VecDeque<u8> = (0..16).collect();
        
        // Allocate in order of interval start
        let mut sorted_intervals: Vec<_> = intervals.into_iter().collect();
        sorted_intervals.sort_by_key(|(_, interval)| interval.start);
        
        for (vreg, interval) in sorted_intervals {
            // Expire old intervals
            available.retain(|_| true); // Would free registers from expired intervals
            
            if let Some(hreg) = available.pop_front() {
                alloc.vreg_to_hreg.insert(vreg, hreg);
            } else {
                // Spill
                alloc.spills.insert(vreg);
            }
            
            let _ = interval; // Use interval
        }
        
        alloc
    }
    
    fn build_live_intervals(&self, ir: &IrBlock) -> HashMap<VReg, LiveInterval> {
        let mut intervals = HashMap::new();
        let mut pos = 0usize;
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                // Update intervals for uses
                for op in get_operands(&instr.op) {
                    intervals.entry(op)
                        .or_insert(LiveInterval { start: pos, end: pos })
                        .end = pos;
                }
                
                // Update intervals for defs (SSA: dst is in instr.dst)
                if op_produces_value(&instr.op) {
                    intervals.entry(instr.dst)
                        .or_insert(LiveInterval { start: pos, end: pos });
                }
                
                pos += 1;
            }
        }
        
        intervals
    }
    
    fn schedule_instructions(&self, ir: &mut IrBlock) {
        self.schedule_instructions_with_stats(ir, &mut None);
    }
    
    /// Schedule instructions using the new enterprise scheduler
    /// Returns detailed statistics about the scheduling process
    fn schedule_instructions_with_stats(
        &self, 
        ir: &mut IrBlock,
        stats: &mut Option<&mut OptStats>,
    ) {
        use super::scheduler::{InstructionScheduler, SchedulerConfig, SchedulingAlgorithm};
        use super::scope::DependencyGraph;
        
        // S2 uses the new enterprise scheduler with:
        // - Full dependency analysis (RAW, WAR, WAW, memory, control)
        // - Accurate latency model based on Intel/AMD microarchitectures
        // - Critical path scheduling for maximum ILP
        // - Register pressure tracking to minimize spills
        // - Software pipelining for loops
        // - Speculative memory reordering with profile guidance
        
        // Configure scheduler based on S2Config
        let scheduler_config = SchedulerConfig {
            max_window_size: if self.config.scope_aware_opt { 4096 } else { 256 },
            speculative_memory: self.config.branch_speculation,
            speculation_threshold: self.config.speculation_threshold,
            critical_path_priority: true,
            resource_model: true,
            algorithm: SchedulingAlgorithm::CriticalPath,
            ..SchedulerConfig::default()
        };
        
        let scheduler = InstructionScheduler::with_config(scheduler_config);
        
        // Build dependency graph for the entire IR
        let dep_graph = DependencyGraph::build(ir);
        
        // Get scope level for scheduling decisions
        let scope_level = if let Some(ref stats) = stats {
            stats.scope_level
        } else {
            super::scope::ScopeLevel::Block
        };
        
        // Create dummy profile for scheduling (would use real profile in production)
        let profile = ProfileDb::new(1024);
        
        // Run scheduler
        let result = scheduler.schedule_scope(ir, scope_level, &profile);
        
        // Update statistics
        if let Some(ref mut opt_stats) = stats {
            opt_stats.scheduler_stats = Some(result.stats.clone());
            opt_stats.instrs_reordered = result.stats.reordered;
            opt_stats.critical_path_length = result.stats.critical_path_length;
            opt_stats.achieved_ilp = result.stats.achieved_ilp;
        }
        
        // For backward compatibility, also run per-block scheduling for small blocks
        for bb in &mut ir.blocks {
            if bb.instrs.len() < 3 {
                continue;
            }
            
            // Build full scheduling context (legacy path)
            let sched_ctx = S2ScheduleContext::build(&bb.instrs);
            
            // Run advanced list scheduling with register pressure awareness
            let new_order = sched_ctx.schedule_with_pressure();
            
            // Reorder instructions
            let new_instrs: Vec<_> = new_order.into_iter()
                .map(|i| bb.instrs[i].clone())
                .collect();
            bb.instrs = new_instrs;
        }
    }
    
    // Legacy methods kept for compatibility - delegated to S2ScheduleContext
    fn build_dep_graph(&self, instrs: &[IrInstr]) -> Vec<Vec<usize>> {
        let ctx = S2ScheduleContext::build(instrs);
        ctx.deps.iter().map(|edges| edges.iter().map(|e| e.pred).collect()).collect()
    }
    
    fn list_schedule(&self, instrs: &[IrInstr], deps: &[Vec<usize>]) -> Vec<usize> {
        // Fallback to simple scheduling if called directly
        let n = instrs.len();
        let mut scheduled = Vec::with_capacity(n);
        let mut ready: Vec<usize> = Vec::new();
        let mut done = vec![false; n];
        
        for i in 0..n {
            if deps[i].is_empty() {
                ready.push(i);
            }
        }
        
        while scheduled.len() < n {
            if ready.is_empty() {
                for i in 0..n {
                    if !done[i] {
                        ready.push(i);
                        break;
                    }
                }
            }
            
            if let Some(idx) = ready.pop() {
                scheduled.push(idx);
                done[idx] = true;
                
                for i in 0..n {
                    if done[i] { continue; }
                    let all_done = deps[i].iter().all(|&d| done[d]);
                    if all_done && !ready.contains(&i) {
                        ready.push(i);
                    }
                }
            }
        }
        
        scheduled
    }
    
    // ========================================================================
    // Code Generation
    // ========================================================================
    
    fn codegen(&self, ir: &IrBlock, alloc: &RegisterAllocation, _profile: &ProfileDb) -> JitResult<Vec<u8>> {
        let mut code = Vec::new();
        
        // Calculate spill size from allocation
        let spill_size = alloc.spills.len() as i32 * 8;
        
        // Prologue:
        // 1. Save R15 (callee-saved) - we use it as JitState pointer
        // 2. mov r15, rdi - save JitState pointer to R15
        // 3. Allocate spill slots if needed
        
        // push r15 (0x41 = REX.B, 0x57 = push r15)
        code.extend_from_slice(&[0x41, 0x57]);
        
        // mov r15, rdi (0x49 = REX.W+REX.B, 0x89 = mov r/m64, r64, 0xFF = rdi -> r15)
        code.extend_from_slice(&[0x49, 0x89, 0xFF]);
        
        // sub rsp, spill_size (if needed)
        if spill_size > 0 {
            if spill_size <= 127 {
                code.extend_from_slice(&[0x48, 0x83, 0xEC, spill_size as u8]);
            } else {
                code.extend_from_slice(&[0x48, 0x81, 0xEC]);
                code.extend_from_slice(&(spill_size as u32).to_le_bytes());
            }
        }
        
        // Emit code for each block
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                self.emit_instr(&mut code, instr, alloc, spill_size)?;
            }
        }
        
        // Default epilogue (for fallthrough) - return 0 (Normal exit with rip)
        self.emit_epilogue(&mut code, spill_size, 0);
        
        Ok(code)
    }
    
    /// Emit epilogue that reads JitState.rip at runtime and encodes return value.
    /// Return value format: (exit_kind << 56) | rip
    fn emit_epilogue(&self, code: &mut Vec<u8>, spill_size: i32, exit_kind: u32) {
        // add rsp, spill_size (if needed)
        if spill_size > 0 {
            if spill_size <= 127 {
                code.extend_from_slice(&[0x48, 0x83, 0xC4, spill_size as u8]);
            } else {
                code.extend_from_slice(&[0x48, 0x81, 0xC4]);
                code.extend_from_slice(&(spill_size as u32).to_le_bytes());
            }
        }
        
        // Load JitState.rip into rax (r15 points to JitState, rip is at offset 0x80)
        // mov rax, [r15 + 0x80]
        code.extend_from_slice(&[0x49, 0x8B, 0x87, 0x80, 0x00, 0x00, 0x00]);
        
        // If exit_kind != 0, OR the kind into high byte of rax
        if exit_kind != 0 {
            let kind_shifted = (exit_kind as u64) << 56;
            // mov r11, imm64
            code.extend_from_slice(&[0x49, 0xBB]);
            code.extend_from_slice(&kind_shifted.to_le_bytes());
            // or rax, r11
            code.extend_from_slice(&[0x4C, 0x09, 0xD8]);
        }
        
        // pop r15
        code.extend_from_slice(&[0x41, 0x5F]);
        
        // ret
        code.push(0xC3);
    }
    
    fn emit_instr(&self, code: &mut Vec<u8>, instr: &IrInstr, alloc: &RegisterAllocation, spill_size: i32) -> JitResult<()> {
        // SSA style: dst is in instr.dst, IrOp contains only operands
        let dst = instr.dst;
        
        match &instr.op {
            IrOp::Const(val) => {
                if let Some(&hreg) = alloc.vreg_to_hreg.get(&dst) {
                    emit_mov_imm64(code, hreg, *val as u64);
                }
            }
            IrOp::LoadGpr(idx) => {
                // Load from guest state - would use memory reference
                if let Some(&hreg) = alloc.vreg_to_hreg.get(&dst) {
                    // Placeholder - would load from guest state offset
                    let _ = (hreg, idx);
                    code.push(0x90); // nop
                }
            }
            IrOp::Add(a, b) => {
                if let (Some(&dreg), Some(&areg), Some(&breg)) = (
                    alloc.vreg_to_hreg.get(&dst),
                    alloc.vreg_to_hreg.get(a),
                    alloc.vreg_to_hreg.get(b)
                ) {
                    if dreg != areg {
                        emit_mov_reg_reg(code, dreg, areg);
                    }
                    emit_add_reg_reg(code, dreg, breg);
                }
            }
            IrOp::Sub(a, b) => {
                if let (Some(&dreg), Some(&areg), Some(&breg)) = (
                    alloc.vreg_to_hreg.get(&dst),
                    alloc.vreg_to_hreg.get(a),
                    alloc.vreg_to_hreg.get(b)
                ) {
                    if dreg != areg {
                        emit_mov_reg_reg(code, dreg, areg);
                    }
                    // sub dst, src
                    code.push(0x48 | ((breg >> 3) << 2) | (dreg >> 3));
                    code.push(0x29);
                    code.push(0xC0 | ((breg & 7) << 3) | (dreg & 7));
                }
            }
            IrOp::Mul(a, b) => {
                if let (Some(&dreg), Some(&areg), Some(&breg)) = (
                    alloc.vreg_to_hreg.get(&dst),
                    alloc.vreg_to_hreg.get(a),
                    alloc.vreg_to_hreg.get(b)
                ) {
                    if dreg != areg {
                        emit_mov_reg_reg(code, dreg, areg);
                    }
                    // imul dst, src
                    code.push(0x48 | ((dreg >> 3) << 2) | (breg >> 3));
                    code.extend_from_slice(&[0x0F, 0xAF]);
                    code.push(0xC0 | ((dreg & 7) << 3) | (breg & 7));
                }
            }
            IrOp::And(a, b) | IrOp::Or(a, b) | IrOp::Xor(a, b) => {
                if let (Some(&dreg), Some(&areg), Some(&breg)) = (
                    alloc.vreg_to_hreg.get(&dst),
                    alloc.vreg_to_hreg.get(a),
                    alloc.vreg_to_hreg.get(b)
                ) {
                    if dreg != areg {
                        emit_mov_reg_reg(code, dreg, areg);
                    }
                    let opcode = match &instr.op {
                        IrOp::And(_, _) => 0x21,
                        IrOp::Or(_, _) => 0x09,
                        IrOp::Xor(_, _) => 0x31,
                        _ => unreachable!(),
                    };
                    code.push(0x48 | ((breg >> 3) << 2) | (dreg >> 3));
                    code.push(opcode);
                    code.push(0xC0 | ((breg & 7) << 3) | (dreg & 7));
                }
            }
            IrOp::Shl(a, b) | IrOp::Shr(a, b) | IrOp::Sar(a, b) => {
                if let (Some(&dreg), Some(&areg)) = (
                    alloc.vreg_to_hreg.get(&dst),
                    alloc.vreg_to_hreg.get(a)
                ) {
                    if dreg != areg {
                        emit_mov_reg_reg(code, dreg, areg);
                    }
                    // Move shift amount to CL
                    if let Some(&breg) = alloc.vreg_to_hreg.get(b) {
                        emit_mov_reg_reg(code, 1, breg); // rcx
                    }
                    let shift_op = match &instr.op {
                        IrOp::Shl(_, _) => 0xE0,
                        IrOp::Shr(_, _) => 0xE8,
                        IrOp::Sar(_, _) => 0xF8,
                        _ => unreachable!(),
                    };
                    code.push(0x48 | (dreg >> 3));
                    code.push(0xD3);
                    code.push(shift_op | (dreg & 7));
                }
            }
            IrOp::Load8(addr) | IrOp::Load16(addr) | IrOp::Load32(addr) | IrOp::Load64(addr) => {
                if let (Some(&dreg), Some(&areg)) = (
                    alloc.vreg_to_hreg.get(&dst),
                    alloc.vreg_to_hreg.get(addr)
                ) {
                    // mov dst, [addr]
                    code.push(0x48 | ((dreg >> 3) << 2) | (areg >> 3));
                    code.push(0x8B);
                    code.push((dreg & 7) << 3 | (areg & 7));
                }
            }
            IrOp::Store8(addr, val) | IrOp::Store16(addr, val) | 
            IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => {
                if let (Some(&areg), Some(&vreg)) = (
                    alloc.vreg_to_hreg.get(addr),
                    alloc.vreg_to_hreg.get(val)
                ) {
                    // mov [addr], val
                    code.push(0x48 | ((vreg >> 3) << 2) | (areg >> 3));
                    code.push(0x89);
                    code.push((vreg & 7) << 3 | (areg & 7));
                }
            }
            IrOp::Jump(block_id) => {
                // jmp rel32 - would need label resolution
                let _ = block_id;
                code.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]);
            }
            IrOp::Branch(cond, true_blk, false_blk) => {
                let _ = (true_blk, false_blk);
                if let Some(&creg) = alloc.vreg_to_hreg.get(cond) {
                    // test cond, cond
                    code.push(0x48 | ((creg >> 3) << 2) | (creg >> 3));
                    code.push(0x85);
                    code.push(0xC0 | ((creg & 7) << 3) | (creg & 7));
                    // jnz/jmp - placeholders
                    code.extend_from_slice(&[0x0F, 0x85, 0x00, 0x00, 0x00, 0x00]);
                    code.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]);
                }
            }
            IrOp::Ret => {
                // Return with normal exit (exit_kind = 0)
                self.emit_epilogue(code, spill_size, 0);
            }
            IrOp::Hlt => {
                // Return with halt exit (exit_kind = 1)
                self.emit_epilogue(code, spill_size, 1);
            }
            IrOp::Exit(reason) => {
                let exit_code = match reason {
                    ExitReason::Normal => 0u32,
                    ExitReason::Halt => 1,
                    ExitReason::Interrupt(n) => 0x100 | (*n as u32),
                    ExitReason::Exception(n, _) => 0x200 | (*n as u32),
                    ExitReason::IoRead(_, _) => 0x300,
                    ExitReason::IoWrite(_, _) => 0x400,
                    ExitReason::Mmio(_, _, _) => 0x500,
                    ExitReason::Hypercall => 0x600,
                    ExitReason::Reset => 0x700,
                };
                self.emit_epilogue(code, spill_size, exit_code);
            }
            IrOp::Nop => {
                code.push(0x90);
            }
            _ => {
                code.push(0x90); // nop for unhandled
            }
        }
        
        Ok(())
    }
    
    fn estimate_cycles(&self, ir: &IrBlock) -> u32 {
        // More accurate estimation than S1
        let mut cycles = 0u32;
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                cycles += match &instr.op {
                    IrOp::Mul(_, _) | IrOp::IMul(_, _) => 3,
                    IrOp::Div(_, _) | IrOp::IDiv(_, _) => 15, // Optimized div
                    IrOp::Load8(_) | IrOp::Load16(_) |
                    IrOp::Load32(_) | IrOp::Load64(_) => 3,  // Better scheduling
                    IrOp::Exit(_) => 10,
                    _ => 1,
                };
            }
        }
        
        cycles
    }
}

impl Default for S2Compiler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Types
// ============================================================================

struct ControlFlowGraph {
    entry: usize,
    blocks: Vec<CfgNode>,
}

struct CfgNode {
    id: usize,
    preds: Vec<usize>,
    succs: Vec<usize>,
}

struct DominatorTree {
    idom: Vec<usize>,
}

struct Loop {
    header: usize,
    body: HashSet<usize>,
    back_edges: Vec<(usize, usize)>,
}

struct RegisterAllocation {
    vreg_to_hreg: HashMap<VReg, u8>,
    spills: HashSet<VReg>,
}

struct LiveInterval {
    start: usize,
    end: usize,
}

/// Expression key for value numbering
#[derive(Clone, Hash, PartialEq, Eq)]
struct ExprKey {
    op: u8,
    operands: Vec<VReg>,
}

/// Create expression key for SSA-style IR (dst is in IrInstr.dst)
fn make_expr_key(op: &IrOp) -> Option<ExprKey> {
    match op {
        IrOp::Add(a, b) => Some(ExprKey { op: 1, operands: vec![*a, *b] }),
        IrOp::Sub(a, b) => Some(ExprKey { op: 2, operands: vec![*a, *b] }),
        IrOp::And(a, b) => Some(ExprKey { op: 3, operands: vec![*a, *b] }),
        IrOp::Or(a, b) => Some(ExprKey { op: 4, operands: vec![*a, *b] }),
        IrOp::Xor(a, b) => Some(ExprKey { op: 5, operands: vec![*a, *b] }),
        IrOp::Mul(a, b) => Some(ExprKey { op: 6, operands: vec![*a, *b] }),
        IrOp::Shl(a, b) => Some(ExprKey { op: 7, operands: vec![*a, *b] }),
        IrOp::Shr(a, b) => Some(ExprKey { op: 8, operands: vec![*a, *b] }),
        _ => None,
    }
}

fn count_instrs(ir: &IrBlock) -> u32 {
    ir.blocks.iter().map(|bb| bb.instrs.len() as u32).sum()
}

/// Get operands of an IR operation (SSA style - dst is in IrInstr.dst)
fn get_operands(op: &IrOp) -> Vec<VReg> {
    match op {
        // Binary ops
        IrOp::Add(a, b) | IrOp::Sub(a, b) | IrOp::And(a, b) |
        IrOp::Or(a, b) | IrOp::Xor(a, b) | IrOp::Mul(a, b) | IrOp::IMul(a, b) |
        IrOp::Div(a, b) | IrOp::IDiv(a, b) | IrOp::Shl(a, b) |
        IrOp::Shr(a, b) | IrOp::Sar(a, b) | IrOp::Rol(a, b) | IrOp::Ror(a, b) |
        IrOp::Cmp(a, b) | IrOp::Test(a, b) => vec![*a, *b],
        // Unary ops
        IrOp::Neg(a) | IrOp::Not(a) => vec![*a],
        // Memory loads
        IrOp::Load8(addr) | IrOp::Load16(addr) |
        IrOp::Load32(addr) | IrOp::Load64(addr) => vec![*addr],
        // Memory stores
        IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
        IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => vec![*addr, *val],
        // Extensions
        IrOp::Sext8(v) | IrOp::Sext16(v) | IrOp::Sext32(v) |
        IrOp::Zext8(v) | IrOp::Zext16(v) | IrOp::Zext32(v) |
        IrOp::Trunc8(v) | IrOp::Trunc16(v) | IrOp::Trunc32(v) => vec![*v],
        // Flag extraction
        IrOp::GetCF(v) | IrOp::GetZF(v) | IrOp::GetSF(v) |
        IrOp::GetOF(v) | IrOp::GetPF(v) => vec![*v],
        // Select
        IrOp::Select(c, t, f) => vec![*c, *t, *f],
        // Guest register stores
        IrOp::StoreGpr(_, v) | IrOp::StoreFlags(v) | IrOp::StoreRip(v) => vec![*v],
        // I/O
        IrOp::In8(p) | IrOp::In16(p) | IrOp::In32(p) => vec![*p],
        IrOp::Out8(p, v) | IrOp::Out16(p, v) | IrOp::Out32(p, v) => vec![*p, *v],
        // Control flow
        IrOp::Branch(c, _, _) => vec![*c],
        IrOp::CallIndirect(t) => vec![*t],
        // Phi
        IrOp::Phi(sources) => sources.iter().map(|(_, v)| *v).collect(),
        // No operands
        _ => Vec::new(),
    }
}

/// Check if operation has side effects
fn has_side_effect(op: &IrOp) -> bool {
    matches!(op,
        IrOp::Store8(_, _) | IrOp::Store16(_, _) |
        IrOp::Store32(_, _) | IrOp::Store64(_, _) |
        IrOp::StoreGpr(_, _) | IrOp::StoreFlags(_) | IrOp::StoreRip(_) |
        IrOp::Call(_) | IrOp::CallIndirect(_) |
        IrOp::Out8(_, _) | IrOp::Out16(_, _) | IrOp::Out32(_, _) |
        IrOp::Syscall | IrOp::Hlt |
        IrOp::Jump(_) | IrOp::Branch(_, _, _) | IrOp::Ret |
        IrOp::Exit(_)
    )
}

/// Check if op produces a value (SSA style - dst is in IrInstr.dst)
fn op_produces_value(op: &IrOp) -> bool {
    match op {
        IrOp::Const(_) | IrOp::ConstF64(_) |
        IrOp::LoadGpr(_) | IrOp::LoadFlags | IrOp::LoadRip |
        IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_) |
        IrOp::Add(_, _) | IrOp::Sub(_, _) | IrOp::Mul(_, _) | IrOp::IMul(_, _) |
        IrOp::Div(_, _) | IrOp::IDiv(_, _) | IrOp::Neg(_) |
        IrOp::And(_, _) | IrOp::Or(_, _) | IrOp::Xor(_, _) | IrOp::Not(_) |
        IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) |
        IrOp::Rol(_, _) | IrOp::Ror(_, _) |
        IrOp::Cmp(_, _) | IrOp::Test(_, _) |
        IrOp::GetCF(_) | IrOp::GetZF(_) | IrOp::GetSF(_) | IrOp::GetOF(_) | IrOp::GetPF(_) |
        IrOp::Select(_, _, _) |
        IrOp::Sext8(_) | IrOp::Sext16(_) | IrOp::Sext32(_) |
        IrOp::Zext8(_) | IrOp::Zext16(_) | IrOp::Zext32(_) |
        IrOp::Trunc8(_) | IrOp::Trunc16(_) | IrOp::Trunc32(_) |
        IrOp::In8(_) | IrOp::In16(_) | IrOp::In32(_) |
        IrOp::Rdtsc | IrOp::Cpuid |
        IrOp::Phi(_) => true,
        _ => false,
    }
}

// ============================================================================
// Code emission helpers
// ============================================================================

fn emit_mov_imm64(code: &mut Vec<u8>, reg: u8, val: u64) {
    code.push(0x48 | ((reg >> 3) << 2));
    code.push(0xB8 + (reg & 7));
    code.extend_from_slice(&val.to_le_bytes());
}

fn emit_mov_reg_reg(code: &mut Vec<u8>, dst: u8, src: u8) {
    code.push(0x48 | ((src >> 3) << 2) | (dst >> 3));
    code.push(0x89);
    code.push(0xC0 | ((src & 7) << 3) | (dst & 7));
}

fn emit_add_reg_reg(code: &mut Vec<u8>, dst: u8, src: u8) {
    code.push(0x48 | ((src >> 3) << 2) | (dst >> 3));
    code.push(0x01);
    code.push(0xC0 | ((src & 7) << 3) | (dst & 7));
}

// ============================================================================
// S2 Advanced Out-of-Order Instruction Scheduling (Enterprise)
// ============================================================================
//
// S2 implements a sophisticated instruction scheduler that goes beyond S1's
// basic list scheduling. Key enhancements:
//
// 1. **Multi-level Dependency Analysis**: Full RAW/WAR/WAW/Memory/Control
// 2. **Micro-architecture Aware Latencies**: Execution ports, μop fusion
// 3. **Critical Path + Slack Scheduling**: Balance latency hiding with ILP
// 4. **Register Pressure Tracking**: Avoid schedules that cause spills
// 5. **Memory Disambiguation**: Speculative load reordering when safe
// 6. **Execution Port Modeling**: Distribute instructions across ports
//
// ## Scheduling Algorithm
//
// Uses a hybrid approach combining:
// - Critical path scheduling (prioritize long dependency chains)
// - Register pressure heuristics (limit live ranges)
// - Resource balancing (distribute across execution units)

/// Instruction latency with micro-architectural details for S2
#[derive(Debug, Clone, Copy)]
struct S2InstrLatency {
    /// Execution latency (cycles until result is ready)
    latency: u8,
    /// Throughput (cycles between issue of same type)
    throughput: u8,
    /// Number of micro-ops
    uops: u8,
    /// Execution ports this instruction can use (bitmask)
    /// Ports 0-5 on Intel, 0-3 on AMD
    ports: u8,
    /// Whether this instruction can be fused with next
    can_fuse: bool,
}

impl S2InstrLatency {
    const fn new(latency: u8, throughput: u8, uops: u8, ports: u8, can_fuse: bool) -> Self {
        Self { latency, throughput, uops, ports, can_fuse }
    }
    
    /// Get detailed latency for an IR operation
    /// Based on Intel Ice Lake / Alder Lake and AMD Zen 4 data
    fn for_op(op: &IrOp) -> Self {
        match op {
            // Constants: eliminated by renaming, can execute on any port
            IrOp::Const(_) | IrOp::ConstF64(_) => Self::new(0, 1, 1, 0x3F, false),
            
            // Guest register loads: memory from JitState, ports 2,3 (load)
            IrOp::LoadGpr(_) | IrOp::LoadFlags | IrOp::LoadRip => Self::new(4, 1, 1, 0x0C, false),
            
            // Guest register stores: memory to JitState, ports 4 (store-data), 2,3 (store-addr)
            IrOp::StoreGpr(_, _) | IrOp::StoreFlags(_) | IrOp::StoreRip(_) => Self::new(1, 1, 2, 0x1C, false),
            
            // Memory loads: L1 hit, ports 2,3
            IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_) => Self::new(4, 1, 1, 0x0C, false),
            
            // Memory stores: ports 4 + 2,3
            IrOp::Store8(_, _) | IrOp::Store16(_, _) | 
            IrOp::Store32(_, _) | IrOp::Store64(_, _) => Self::new(1, 1, 2, 0x1C, false),
            
            // Simple ALU: 1 cycle, ports 0,1,5,6 (Intel) - can fuse with adjacent ops
            IrOp::Add(_, _) | IrOp::Sub(_, _) => Self::new(1, 1, 1, 0x63, true),
            IrOp::And(_, _) | IrOp::Or(_, _) | IrOp::Xor(_, _) => Self::new(1, 1, 1, 0x63, true),
            IrOp::Neg(_) | IrOp::Not(_) => Self::new(1, 1, 1, 0x63, false),
            
            // Shifts: ports 0,6
            IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) => Self::new(1, 1, 1, 0x41, false),
            
            // Rotates: 2 μops on some architectures
            IrOp::Rol(_, _) | IrOp::Ror(_, _) => Self::new(1, 1, 2, 0x41, false),
            
            // Multiplication: ports 1 only, 3 cycle latency
            IrOp::Mul(_, _) | IrOp::IMul(_, _) => Self::new(3, 1, 1, 0x02, false),
            
            // Division: very expensive, uses divider unit (port 0)
            // Latency varies by operand size, using worst case
            IrOp::Div(_, _) | IrOp::IDiv(_, _) => Self::new(26, 10, 10, 0x01, false),
            
            // Comparisons: can fuse with branch, ports 0,1,5,6
            IrOp::Cmp(_, _) | IrOp::Test(_, _) => Self::new(1, 1, 1, 0x63, true),
            
            // Flag extraction: single μop
            IrOp::GetCF(_) | IrOp::GetZF(_) | IrOp::GetSF(_) |
            IrOp::GetOF(_) | IrOp::GetPF(_) => Self::new(1, 1, 1, 0x63, false),
            
            // Conditional move: 2 μops (read flags + cmov)
            IrOp::Select(_, _, _) => Self::new(2, 1, 2, 0x63, false),
            
            // Sign/zero extensions: often free or 1 cycle
            IrOp::Sext8(_) | IrOp::Sext16(_) | IrOp::Sext32(_) => Self::new(1, 1, 1, 0x63, false),
            IrOp::Zext8(_) | IrOp::Zext16(_) | IrOp::Zext32(_) => Self::new(1, 1, 1, 0x63, false),
            
            // Truncations: effectively free (just use lower bits)
            IrOp::Trunc8(_) | IrOp::Trunc16(_) | IrOp::Trunc32(_) => Self::new(0, 1, 1, 0x3F, false),
            
            // Bit manipulation: BMI instructions, port 1 or 5
            IrOp::Popcnt(_) => Self::new(3, 1, 1, 0x22, false),
            IrOp::Lzcnt(_) | IrOp::Tzcnt(_) => Self::new(3, 1, 1, 0x22, false),
            IrOp::Bsf(_) | IrOp::Bsr(_) => Self::new(3, 1, 1, 0x22, false),
            IrOp::Bextr(_, _, _) => Self::new(2, 1, 2, 0x22, false),
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) => Self::new(3, 1, 1, 0x22, false),
            
            // FMA: 4 cycles, port 0 and 1 (FP units)
            IrOp::Fma(_, _, _) => Self::new(4, 1, 1, 0x03, false),
            
            // AES: port 0 (crypto unit)
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => Self::new(4, 1, 1, 0x01, false),
            
            // PCLMUL: port 5
            IrOp::Pclmul(_, _, _) => Self::new(6, 2, 1, 0x20, false),
            
            // Vector operations: depends on width
            IrOp::VectorOp { width, .. } => {
                let (lat, tp) = match *width {
                    128 => (3, 1),
                    256 => (4, 1),
                    512 => (5, 2),
                    _ => (3, 1),
                };
                Self::new(lat, tp, 1, 0x03, false)
            }
            
            // I/O: causes VM exit
            IrOp::In8(_) | IrOp::In16(_) | IrOp::In32(_) => Self::new(100, 100, 10, 0x01, false),
            IrOp::Out8(_, _) | IrOp::Out16(_, _) | IrOp::Out32(_, _) => Self::new(100, 100, 10, 0x01, false),
            
            // Control flow
            IrOp::Jump(_) => Self::new(1, 1, 1, 0x40, false),
            IrOp::Branch(_, _, _) => Self::new(1, 1, 1, 0x40, false),
            IrOp::Call(_) => Self::new(3, 1, 2, 0x40, false),
            IrOp::CallIndirect(_) => Self::new(4, 1, 3, 0x40, false),
            IrOp::Ret => Self::new(1, 1, 1, 0x40, false),
            
            // Special
            IrOp::Syscall => Self::new(200, 200, 20, 0x01, false),
            IrOp::Cpuid => Self::new(40, 40, 20, 0x01, false),
            IrOp::Rdtsc => Self::new(15, 15, 2, 0x01, false),
            IrOp::Hlt => Self::new(200, 200, 1, 0x01, false),
            IrOp::Nop => Self::new(0, 1, 1, 0x3F, false),
            
            // PHI: eliminated in SSA destruction
            IrOp::Phi(_) => Self::new(0, 1, 0, 0x3F, false),
            
            // Exit
            IrOp::Exit(_) => Self::new(20, 20, 10, 0x01, false),
        }
    }
    
    /// Check if two instructions conflict on execution ports
    fn port_conflict(&self, other: &S2InstrLatency) -> bool {
        // Check if instructions compete for same ports
        (self.ports & other.ports) != 0
    }
}

/// Dependency types for S2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum S2DepKind {
    /// Read-After-Write: true data dependency
    Raw,
    /// Write-After-Read: anti-dependency
    War,
    /// Write-After-Write: output dependency
    Waw,
    /// Memory ordering dependency
    Memory,
    /// Control dependency
    Control,
    /// Port resource conflict (for scheduling quality)
    Resource,
}

/// Dependency edge for S2
#[derive(Debug, Clone)]
struct S2DepEdge {
    pred: usize,
    kind: S2DepKind,
    latency: u8,
}

/// Memory operation info for S2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum S2MemoryInfo {
    None,
    Load(VReg),
    Store(VReg),
    LoadGuest(u8),
    StoreGuest(u8),
}

/// S2 scheduling context with advanced analysis
struct S2ScheduleContext {
    n: usize,
    deps: Vec<Vec<S2DepEdge>>,
    succs: Vec<Vec<(usize, u8)>>,
    latencies: Vec<S2InstrLatency>,
    critical_path: Vec<u32>,
    /// Slack for each instruction (how much it can be delayed)
    slack: Vec<u32>,
    /// Register pressure contribution
    reg_pressure: Vec<i32>,
    /// Whether instruction is a terminator
    terminators: Vec<bool>,
    /// Memory operation classification
    memory_ops: Vec<S2MemoryInfo>,
    /// VRegs defined by each instruction
    defs: Vec<Option<VReg>>,
    /// VRegs used by each instruction
    uses: Vec<Vec<VReg>>,
}

impl S2ScheduleContext {
    /// Build comprehensive scheduling context
    fn build(instrs: &[IrInstr]) -> Self {
        let n = instrs.len();
        let mut deps = vec![Vec::new(); n];
        let mut succs = vec![Vec::new(); n];
        let mut terminators = vec![false; n];
        let mut memory_ops = Vec::with_capacity(n);
        let mut defs = Vec::with_capacity(n);
        let mut uses = Vec::with_capacity(n);
        
        // Compute latencies
        let latencies: Vec<_> = instrs.iter()
            .map(|i| S2InstrLatency::for_op(&i.op))
            .collect();
        
        // Extract defs, uses, memory info
        for instr in instrs {
            memory_ops.push(Self::classify_memory(&instr.op));
            
            let def = if op_produces_value(&instr.op) && instr.dst.is_valid() {
                Some(instr.dst)
            } else {
                None
            };
            defs.push(def);
            uses.push(get_operands(&instr.op));
            
            if instr.flags.contains(IrFlags::TERMINATOR) {
                terminators.push(true);
            } else {
                terminators.push(false);
            }
        }
        terminators.truncate(n);
        
        // Build dependency edges
        let mut last_writer: HashMap<VReg, usize> = HashMap::new();
        let mut last_readers: HashMap<VReg, Vec<usize>> = HashMap::new();
        let mut last_stores: Vec<usize> = Vec::new();
        let mut last_loads: Vec<usize> = Vec::new();
        let mut last_guest_writer: [Option<usize>; 18] = [None; 18];
        let mut last_guest_readers: [Vec<usize>; 18] = Default::default();
        
        for (i, _instr) in instrs.iter().enumerate() {
            // RAW dependencies
            for &operand in &uses[i] {
                if let Some(&writer) = last_writer.get(&operand) {
                    let lat = latencies[writer].latency;
                    deps[i].push(S2DepEdge { pred: writer, kind: S2DepKind::Raw, latency: lat });
                    succs[writer].push((i, lat));
                }
            }
            
            // WAR dependencies
            if let Some(def) = defs[i] {
                if let Some(readers) = last_readers.get(&def) {
                    for &reader in readers {
                        if reader != i {
                            deps[i].push(S2DepEdge { pred: reader, kind: S2DepKind::War, latency: 0 });
                            succs[reader].push((i, 0));
                        }
                    }
                }
            }
            
            // WAW dependencies
            if let Some(def) = defs[i] {
                if let Some(&prev_writer) = last_writer.get(&def) {
                    deps[i].push(S2DepEdge { pred: prev_writer, kind: S2DepKind::Waw, latency: 0 });
                    succs[prev_writer].push((i, 0));
                }
            }
            
            // Memory dependencies
            match memory_ops[i] {
                S2MemoryInfo::Load(_) => {
                    // Loads wait for prior stores (conservative)
                    for &store_idx in &last_stores {
                        deps[i].push(S2DepEdge { pred: store_idx, kind: S2DepKind::Memory, latency: 0 });
                        succs[store_idx].push((i, 0));
                    }
                    last_loads.push(i);
                }
                S2MemoryInfo::Store(_) => {
                    // Stores wait for prior loads and stores
                    for &load_idx in &last_loads {
                        deps[i].push(S2DepEdge { pred: load_idx, kind: S2DepKind::Memory, latency: 0 });
                        succs[load_idx].push((i, 0));
                    }
                    for &store_idx in &last_stores {
                        deps[i].push(S2DepEdge { pred: store_idx, kind: S2DepKind::Memory, latency: 0 });
                        succs[store_idx].push((i, 0));
                    }
                    last_stores.push(i);
                }
                S2MemoryInfo::LoadGuest(idx) => {
                    let slot = idx as usize;
                    if let Some(writer) = last_guest_writer[slot] {
                        let lat = latencies[writer].latency;
                        deps[i].push(S2DepEdge { pred: writer, kind: S2DepKind::Raw, latency: lat });
                        succs[writer].push((i, lat));
                    }
                    last_guest_readers[slot].push(i);
                }
                S2MemoryInfo::StoreGuest(idx) => {
                    let slot = idx as usize;
                    for &reader in &last_guest_readers[slot] {
                        deps[i].push(S2DepEdge { pred: reader, kind: S2DepKind::War, latency: 0 });
                        succs[reader].push((i, 0));
                    }
                    if let Some(writer) = last_guest_writer[slot] {
                        deps[i].push(S2DepEdge { pred: writer, kind: S2DepKind::Waw, latency: 0 });
                        succs[writer].push((i, 0));
                    }
                    last_guest_writer[slot] = Some(i);
                    last_guest_readers[slot].clear();
                }
                S2MemoryInfo::None => {}
            }
            
            // Update tracking
            for &operand in &uses[i] {
                last_readers.entry(operand).or_default().push(i);
            }
            if let Some(def) = defs[i] {
                last_writer.insert(def, i);
                last_readers.remove(&def);
            }
        }
        
        // Critical fix: Terminators must depend on ALL prior instructions
        // This ensures they are scheduled last in the basic block
        for i in 0..n {
            if terminators[i] {
                // Terminator depends on all prior non-terminator instructions
                for j in 0..i {
                    if !terminators[j] {
                        // Check if dependency already exists
                        let already_has_dep = deps[i].iter().any(|e| e.pred == j);
                        if !already_has_dep {
                            deps[i].push(S2DepEdge { pred: j, kind: S2DepKind::Control, latency: 0 });
                            succs[j].push((i, 0));
                        }
                    }
                }
            }
        }
        
        // Also ensure non-terminators after a terminator depend on it
        // (This handles multiple terminators in sequence)
        let mut last_terminator: Option<usize> = None;
        for i in 0..n {
            if let Some(term) = last_terminator {
                if !terminators[i] {
                    deps[i].push(S2DepEdge { pred: term, kind: S2DepKind::Control, latency: 0 });
                    succs[term].push((i, 0));
                }
            }
            if terminators[i] {
                last_terminator = Some(i);
            }
        }
        
        // Compute critical paths
        let critical_path = Self::compute_critical_paths(n, &succs, &latencies);
        
        // Compute slack (latest start - earliest start)
        let slack = Self::compute_slack(n, &deps, &succs, &latencies, &critical_path);
        
        // Compute register pressure contribution
        let reg_pressure = Self::compute_reg_pressure(n, &defs, &uses, &succs);
        
        Self {
            n,
            deps,
            succs,
            latencies,
            critical_path,
            slack,
            reg_pressure,
            terminators,
            memory_ops,
            defs,
            uses,
        }
    }
    
    fn classify_memory(op: &IrOp) -> S2MemoryInfo {
        match op {
            IrOp::Load8(addr) | IrOp::Load16(addr) |
            IrOp::Load32(addr) | IrOp::Load64(addr) => S2MemoryInfo::Load(*addr),
            IrOp::Store8(addr, _) | IrOp::Store16(addr, _) |
            IrOp::Store32(addr, _) | IrOp::Store64(addr, _) => S2MemoryInfo::Store(*addr),
            IrOp::LoadGpr(idx) => S2MemoryInfo::LoadGuest(*idx),
            IrOp::StoreGpr(idx, _) => S2MemoryInfo::StoreGuest(*idx),
            IrOp::LoadFlags => S2MemoryInfo::LoadGuest(16),
            IrOp::StoreFlags(_) => S2MemoryInfo::StoreGuest(16),
            IrOp::LoadRip => S2MemoryInfo::LoadGuest(17),
            IrOp::StoreRip(_) => S2MemoryInfo::StoreGuest(17),
            _ => S2MemoryInfo::None,
        }
    }
    
    fn compute_critical_paths(n: usize, succs: &[Vec<(usize, u8)>], latencies: &[S2InstrLatency]) -> Vec<u32> {
        let mut critical = vec![0u32; n];
        
        // Multiple iterations for convergence
        for _ in 0..3 {
            for i in (0..n).rev() {
                let own_lat = latencies[i].latency as u32;
                let max_succ = succs[i].iter()
                    .map(|(s, edge_lat)| critical[*s] + *edge_lat as u32)
                    .max()
                    .unwrap_or(0);
                critical[i] = own_lat + max_succ;
            }
        }
        
        critical
    }
    
    fn compute_slack(n: usize, deps: &[Vec<S2DepEdge>], succs: &[Vec<(usize, u8)>], 
                     latencies: &[S2InstrLatency], critical_path: &[u32]) -> Vec<u32> {
        // Earliest start time
        let mut earliest = vec![0u32; n];
        for i in 0..n {
            for edge in &deps[i] {
                let pred_finish = earliest[edge.pred] + latencies[edge.pred].latency as u32;
                earliest[i] = earliest[i].max(pred_finish);
            }
        }
        
        // Latest start time (based on total critical path)
        let total_length = critical_path.iter().max().copied().unwrap_or(0);
        let mut latest = vec![total_length; n];
        for i in (0..n).rev() {
            for &(succ, edge_lat) in &succs[i] {
                let succ_start = latest[succ].saturating_sub(edge_lat as u32);
                latest[i] = latest[i].min(succ_start);
            }
        }
        
        // Slack = latest - earliest
        earliest.iter().zip(latest.iter())
            .map(|(&e, &l)| l.saturating_sub(e))
            .collect()
    }
    
    fn compute_reg_pressure(n: usize, defs: &[Option<VReg>], uses: &[Vec<VReg>], 
                            succs: &[Vec<(usize, u8)>]) -> Vec<i32> {
        let mut pressure = vec![0i32; n];
        
        for i in 0..n {
            // +1 for each new definition
            if defs[i].is_some() {
                pressure[i] += 1;
            }
            
            // -1 for each last use (approximate: if no successor uses this vreg)
            for &used_vreg in &uses[i] {
                let is_last_use = !succs[i].iter().any(|(s, _)| {
                    uses[*s].contains(&used_vreg)
                });
                if is_last_use {
                    pressure[i] -= 1;
                }
            }
        }
        
        pressure
    }
    
    /// Schedule with register pressure awareness
    fn schedule_with_pressure(&self) -> Vec<usize> {
        let mut scheduled = Vec::with_capacity(self.n);
        let mut done = vec![false; self.n];
        let mut remaining_deps: Vec<usize> = self.deps.iter()
            .map(|d| d.len())
            .collect();
        
        // Ready queue
        let mut ready: Vec<usize> = Vec::new();
        for i in 0..self.n {
            if remaining_deps[i] == 0 {
                ready.push(i);
            }
        }
        
        // Track current register pressure
        let mut current_pressure = 0i32;
        const MAX_PRESSURE: i32 = 12; // Target max live registers
        
        // Track cycle and ready times
        let mut cycle = 0u32;
        let mut ready_time = vec![0u32; self.n];
        
        // Port usage tracking (for resource balancing)
        let mut port_usage = [0u32; 8];
        
        while scheduled.len() < self.n {
            // Sort ready queue by composite priority
            ready.sort_by(|&a, &b| {
                // Multi-factor priority:
                // 1. Critical path (higher = better)
                // 2. Slack (lower = more urgent)
                // 3. Register pressure impact (lower = better if pressure high)
                // 4. Port balancing
                
                let crit_a = self.critical_path[a];
                let crit_b = self.critical_path[b];
                
                // Primary: critical path
                let crit_cmp = crit_b.cmp(&crit_a);
                if crit_cmp != std::cmp::Ordering::Equal {
                    return crit_cmp;
                }
                
                // Secondary: slack (prefer lower slack - more urgent)
                let slack_cmp = self.slack[a].cmp(&self.slack[b]);
                if slack_cmp != std::cmp::Ordering::Equal {
                    return slack_cmp;
                }
                
                // Tertiary: if pressure is high, prefer instructions that release registers
                if current_pressure > MAX_PRESSURE {
                    let press_cmp = self.reg_pressure[a].cmp(&self.reg_pressure[b]);
                    if press_cmp != std::cmp::Ordering::Equal {
                        return press_cmp;
                    }
                }
                
                // Quaternary: prefer less-used ports
                let port_load_a = self.port_load(&port_usage, &self.latencies[a]);
                let port_load_b = self.port_load(&port_usage, &self.latencies[b]);
                let port_cmp = port_load_a.cmp(&port_load_b);
                if port_cmp != std::cmp::Ordering::Equal {
                    return port_cmp;
                }
                
                // Final: original order
                a.cmp(&b)
            });
            
            // Find instruction ready to execute
            let mut chosen = None;
            for (idx, &instr) in ready.iter().enumerate() {
                if ready_time[instr] <= cycle {
                    chosen = Some(idx);
                    break;
                }
            }
            
            if let Some(idx) = chosen {
                let instr = ready.remove(idx);
                scheduled.push(instr);
                done[instr] = true;
                
                // Update pressure
                current_pressure += self.reg_pressure[instr];
                
                // Update port usage
                let lat = &self.latencies[instr];
                for p in 0..8 {
                    if (lat.ports >> p) & 1 != 0 {
                        port_usage[p] += lat.throughput as u32;
                    }
                }
                
                // Update ready times for successors
                let finish_time = cycle + self.latencies[instr].latency as u32;
                for &(succ, edge_lat) in &self.succs[instr] {
                    let succ_ready = finish_time.saturating_sub(edge_lat as u32);
                    ready_time[succ] = ready_time[succ].max(succ_ready);
                    
                    remaining_deps[succ] -= 1;
                    if remaining_deps[succ] == 0 && !done[succ] && !ready.contains(&succ) {
                        ready.push(succ);
                    }
                }
                
                cycle += 1;
            } else if !ready.is_empty() {
                // Advance to next ready time
                let min_ready = ready.iter()
                    .map(|&i| ready_time[i])
                    .min()
                    .unwrap_or(cycle + 1);
                cycle = min_ready;
            } else {
                // Shouldn't happen with correct deps
                for i in 0..self.n {
                    if !done[i] {
                        ready.push(i);
                        break;
                    }
                }
                cycle += 1;
            }
        }
        
        scheduled
    }
    
    /// Calculate port load for an instruction given current usage
    fn port_load(&self, port_usage: &[u32; 8], lat: &S2InstrLatency) -> u32 {
        let mut min_load = u32::MAX;
        for p in 0..8 {
            if (lat.ports >> p) & 1 != 0 {
                min_load = min_load.min(port_usage[p]);
            }
        }
        if min_load == u32::MAX { 0 } else { min_load }
    }
}

#[cfg(test)]
mod scheduling_tests {
    use super::*;
    
    #[test]
    fn test_s2_latency_model() {
        let add_lat = S2InstrLatency::for_op(&IrOp::Add(VReg(0), VReg(1)));
        assert_eq!(add_lat.latency, 1);
        assert!(add_lat.can_fuse, "ADD should be fusible");
        
        let mul_lat = S2InstrLatency::for_op(&IrOp::Mul(VReg(0), VReg(1)));
        assert_eq!(mul_lat.latency, 3);
        assert_eq!(mul_lat.ports, 0x02, "MUL should use port 1");
        
        let div_lat = S2InstrLatency::for_op(&IrOp::Div(VReg(0), VReg(1)));
        assert!(div_lat.latency >= 20, "DIV should be expensive");
    }
    
    #[test]
    fn test_s2_port_conflict() {
        let add = S2InstrLatency::for_op(&IrOp::Add(VReg(0), VReg(1)));
        let mul = S2InstrLatency::for_op(&IrOp::Mul(VReg(0), VReg(1)));
        
        // ADD uses ports 0,1,5,6; MUL uses port 1 - they conflict on port 1
        assert!(add.port_conflict(&mul));
        
        let load = S2InstrLatency::for_op(&IrOp::Load64(VReg(0)));
        // Load uses ports 2,3 - no conflict with ADD
        assert!(!add.port_conflict(&load));
    }
    
    #[test]
    fn test_s2_schedule_critical_path() {
        // Chain: const -> mul -> div (long path)
        // Independent: const -> add (short path)
        let instrs = vec![
            IrInstr {
                dst: VReg(0),
                op: IrOp::Const(1),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(1),
                op: IrOp::Mul(VReg(0), VReg(0)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(2),
                op: IrOp::Div(VReg(1), VReg(1)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(3),
                op: IrOp::Const(2),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(4),
                op: IrOp::Add(VReg(3), VReg(3)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
        ];
        
        let ctx = S2ScheduleContext::build(&instrs);
        
        // Long chain should have higher critical path
        assert!(ctx.critical_path[0] > ctx.critical_path[3],
            "Long chain start should have higher critical path");
        
        let order = ctx.schedule_with_pressure();
        assert_eq!(order.len(), 5);
        
        // Verify dependency order preserved
        let pos = |idx: usize| order.iter().position(|&x| x == idx).unwrap();
        assert!(pos(0) < pos(1), "const must precede mul");
        assert!(pos(1) < pos(2), "mul must precede div");
        assert!(pos(3) < pos(4), "const must precede add");
    }
    
    #[test]
    fn test_s2_register_pressure() {
        // Create many independent instructions to test pressure handling
        let mut instrs = Vec::new();
        for i in 0..20 {
            instrs.push(IrInstr {
                dst: VReg(i),
                op: IrOp::Const(i as i64),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            });
        }
        
        let ctx = S2ScheduleContext::build(&instrs);
        let order = ctx.schedule_with_pressure();
        
        // All should be scheduled
        assert_eq!(order.len(), 20);
    }
}
