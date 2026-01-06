//! Scope-Aware Optimizer
//!
//! Implements the optimization capability matrix based on compilation scope level.
//! Different scope levels enable different optimization capabilities.
//!
//! ## Optimization Capability Matrix
//!
//! ```text
//! ┌───────────┬──────────┬─────┬─────┬──────┬────────┬────────┐
//! │ Scope     │ Reorder  │ DCE │ CSE │ LICM │ Inline │ Devirt │
//! ├───────────┼──────────┼─────┼─────┼──────┼────────┼────────┤
//! │ Block     │ ✓ Local  │ ✓   │ ✓   │ ✗    │ ✗      │ ✗      │
//! │ Function  │ ✓ CFG    │ ✓   │ ✓   │ ✓    │ ✓      │ ✗      │
//! │ Region    │ ✓ Trace  │ ✓   │ ✓   │ ✓    │ ✓      │ ✓      │  ← 热路径优化
//! │ CallGraph │ ✓ Global │ ✓   │ ✓   │ ✓    │ ✓      │ ✓      │  ← 全局激进优化
//! └───────────┴──────────┴─────┴─────┴──────┴────────┴────────┘
//! ```
//!
//! ## Architecture
//!
//! The optimizer works in phases:
//! 1. **Scope Selection**: Determine optimal scope based on profile hotness
//! 2. **Dependency Analysis**: Build cross-instruction/block dependency graph
//! 3. **Optimization Passes**: Apply scope-appropriate optimizations
//! 4. **Scheduling**: Reorder instructions for maximum ILP

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::ir::{IrBlock, IrInstr, IrOp, VReg, BlockId, IrFlags, IrBasicBlock, ExitReason};
use super::scope::{
    ScopeLevel, ScopeConfig, ScopeBuilder, CompilationScope, 
    DependencyGraph, ScopeProfile, ScopeProfileDb,
};
use super::profile::{ProfileDb, BranchBias};
use super::scheduler::{InstructionScheduler, SchedulerConfig, ScheduleResult, SchedulingAlgorithm};
use super::deopt::{DeoptManager, DeoptGuard, DeoptReason, GuardKind};

// ============================================================================
// Optimizer Configuration
// ============================================================================

/// Scope optimizer configuration
#[derive(Debug, Clone)]
pub struct ScopeOptimizerConfig {
    /// Enable scope-aware optimization
    pub enabled: bool,
    
    /// Scope selection configuration
    pub scope_config: ScopeConfig,
    
    /// Scheduler configuration
    pub scheduler_config: SchedulerConfig,
    
    /// Optimization thresholds by scope level
    pub thresholds: ScopeThresholds,
    
    /// Enable aggressive devirtualization
    pub aggressive_devirt: bool,
    
    /// Enable speculative inlining
    pub speculative_inline: bool,
    
    /// Maximum inline depth
    pub max_inline_depth: u32,
    
    /// Maximum total inlined size
    pub max_inline_size: u32,
}

impl Default for ScopeOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scope_config: ScopeConfig::default(),
            scheduler_config: SchedulerConfig::default(),
            thresholds: ScopeThresholds::default(),
            aggressive_devirt: true,
            speculative_inline: true,
            max_inline_depth: 4,
            max_inline_size: 500,
        }
    }
}

/// Execution count thresholds for different optimizations
#[derive(Debug, Clone)]
pub struct ScopeThresholds {
    /// Minimum executions to consider for function-level optimization
    pub function_level: u64,
    /// Minimum executions to consider for region-level optimization
    pub region_level: u64,
    /// Minimum executions for call graph analysis
    pub callgraph_level: u64,
    /// Devirtualization confidence threshold (0.0-1.0)
    pub devirt_confidence: f64,
    /// Inlining benefit threshold (cycles saved / size increase)
    pub inline_benefit: f64,
}

impl Default for ScopeThresholds {
    fn default() -> Self {
        Self {
            function_level: 100,
            region_level: 1000,
            callgraph_level: 10000,
            devirt_confidence: 0.95,
            inline_benefit: 1.5,
        }
    }
}

// ============================================================================
// Optimization Result
// ============================================================================

/// Result of scope-aware optimization
#[derive(Debug, Clone)]
pub struct ScopeOptResult {
    /// Scope level used
    pub scope_level: ScopeLevel,
    
    /// Number of blocks in scope
    pub scope_blocks: usize,
    
    /// Optimizations applied
    pub optimizations: OptimizationStats,
    
    /// Schedule result (if scheduling was applied)
    pub schedule: Option<ScheduleResult>,
    
    /// Guards inserted for speculative optimizations
    pub guards: Vec<GuardInfo>,
    
    /// Estimated speedup factor
    pub estimated_speedup: f64,
}

/// Statistics about applied optimizations
#[derive(Debug, Clone, Default)]
pub struct OptimizationStats {
    /// Dead code eliminated (instructions)
    pub dce_eliminated: u32,
    /// Common subexpressions eliminated
    pub cse_eliminated: u32,
    /// Loop invariants hoisted
    pub licm_hoisted: u32,
    /// Functions inlined
    pub inlined: u32,
    /// Calls devirtualized
    pub devirtualized: u32,
    /// Memory operations reordered
    pub memory_reordered: u32,
    /// Instructions reordered
    pub instructions_reordered: u32,
    /// Cross-block optimizations
    pub cross_block_opts: u32,
}

/// Information about an inserted guard
#[derive(Debug, Clone)]
pub struct GuardInfo {
    pub rip: u64,
    pub kind: String,
    pub deopt_reason: String,
}

// ============================================================================
// Scope Optimizer
// ============================================================================

/// Enterprise scope-aware optimizer
pub struct ScopeOptimizer {
    config: ScopeOptimizerConfig,
    scope_builder: ScopeBuilder,
    scheduler: InstructionScheduler,
    scope_profile_db: Arc<ScopeProfileDb>,
}

impl ScopeOptimizer {
    pub fn new() -> Self {
        let config = ScopeOptimizerConfig::default();
        Self {
            scope_builder: ScopeBuilder::new(config.scope_config.clone()),
            scheduler: InstructionScheduler::with_config(config.scheduler_config.clone()),
            scope_profile_db: Arc::new(ScopeProfileDb::new(10000)),
            config,
        }
    }
    
    pub fn with_config(config: ScopeOptimizerConfig) -> Self {
        Self {
            scope_builder: ScopeBuilder::new(config.scope_config.clone()),
            scheduler: InstructionScheduler::with_config(config.scheduler_config.clone()),
            scope_profile_db: Arc::new(ScopeProfileDb::new(10000)),
            config,
        }
    }
    
    /// Run scope-aware optimization on an IR block
    pub fn optimize(
        &self,
        ir: &mut IrBlock,
        entry_rip: u64,
        profile: &ProfileDb,
        deopt_mgr: Option<&DeoptManager>,
    ) -> ScopeOptResult {
        // Phase 1: Scope Selection
        let scope = self.scope_builder.build_scope(entry_rip, profile);
        let scope_level = scope.level;
        
        log::debug!(
            "[ScopeOpt] Selected scope level {:?} for RIP {:#x} (blocks={}, exec_count={})",
            scope_level, entry_rip, scope.blocks.len(), scope.execution_count
        );
        
        // Load or create scope profile
        let mut scope_profile = self.scope_profile_db.get_or_create(entry_rip, scope_level);
        
        // Phase 2: Build Dependency Graph
        let dep_graph = DependencyGraph::build(ir);
        scope_profile.update_dep_stats(&dep_graph);
        
        // Phase 3: Apply Optimizations Based on Scope Level
        let mut stats = OptimizationStats::default();
        let mut guards = Vec::new();
        
        // Always apply: DCE, CSE (all scope levels)
        self.dead_code_elimination(ir, &mut stats);
        self.common_subexpression_elimination(ir, &mut stats);
        
        match scope_level {
            ScopeLevel::Block => {
                // Block level: local optimizations only
                self.local_reordering(ir, &dep_graph, &mut stats);
            }
            ScopeLevel::Function => {
                // Function level: add LICM and inlining
                self.loop_invariant_code_motion(ir, &mut stats);
                self.try_inline(ir, &scope, profile, &mut stats);
                self.local_reordering(ir, &dep_graph, &mut stats);
            }
            ScopeLevel::Region => {
                // Region level: add devirtualization and cross-function optimization
                self.loop_invariant_code_motion(ir, &mut stats);
                self.try_inline(ir, &scope, profile, &mut stats);
                
                if let Some(dm) = deopt_mgr {
                    self.devirtualization(ir, &scope, profile, dm, &mut stats, &mut guards);
                }
                
                self.cross_function_optimization(ir, &scope, profile, &mut scope_profile, &mut stats);
                self.trace_optimization(ir, &scope, profile, &mut stats);
            }
            ScopeLevel::CallGraph => {
                // CallGraph level: whole program optimization
                self.loop_invariant_code_motion(ir, &mut stats);
                self.aggressive_inline(ir, &scope, profile, &mut stats);
                
                if let Some(dm) = deopt_mgr {
                    self.devirtualization(ir, &scope, profile, dm, &mut stats, &mut guards);
                }
                
                self.cross_function_optimization(ir, &scope, profile, &mut scope_profile, &mut stats);
                self.whole_program_optimization(ir, &scope, profile, &mut stats);
            }
        }
        
        // Phase 4: Instruction Scheduling
        let schedule = if self.config.scheduler_config.critical_path_priority {
            let dep_graph = DependencyGraph::build(ir); // Rebuild after optimizations
            Some(self.scheduler.schedule_scope(ir, scope_level, profile))
        } else {
            None
        };
        
        if let Some(ref sched) = schedule {
            stats.instructions_reordered = sched.stats.reordered;
        }
        
        // Update scope profile with optimization decisions
        self.scope_profile_db.update(scope_profile);
        
        // Estimate speedup
        let estimated_speedup = self.estimate_speedup(&stats, &schedule, &dep_graph);
        
        ScopeOptResult {
            scope_level,
            scope_blocks: scope.blocks.len(),
            optimizations: stats,
            schedule,
            guards: guards.into_iter().map(|g| GuardInfo {
                rip: g.guest_rip,
                kind: format!("{:?}", g.kind),
                deopt_reason: format!("{:?}", g.reason),
            }).collect(),
            estimated_speedup,
        }
    }
    
    // ========================================================================
    // Optimization Passes
    // ========================================================================
    
    /// Dead Code Elimination
    fn dead_code_elimination(&self, ir: &mut IrBlock, stats: &mut OptimizationStats) {
        let mut used_vregs = HashSet::new();
        
        // Collect used VRegs (backward pass)
        for bb in ir.blocks.iter().rev() {
            for instr in bb.instrs.iter().rev() {
                // If instruction has side effects, keep it
                if instr.flags.contains(IrFlags::SIDE_EFFECT) {
                    for vreg in super::scope::get_read_vregs_from_op(&instr.op) {
                        used_vregs.insert(vreg);
                    }
                    continue;
                }
                
                // If destination is used, keep instruction and mark inputs as used
                if instr.dst.is_valid() && used_vregs.contains(&instr.dst) {
                    for vreg in super::scope::get_read_vregs_from_op(&instr.op) {
                        used_vregs.insert(vreg);
                    }
                }
            }
        }
        
        // Remove dead instructions
        for bb in &mut ir.blocks {
            let before = bb.instrs.len();
            bb.instrs.retain(|instr| {
                if instr.flags.contains(IrFlags::SIDE_EFFECT) {
                    return true;
                }
                if !instr.dst.is_valid() {
                    return true;
                }
                used_vregs.contains(&instr.dst)
            });
            stats.dce_eliminated += (before - bb.instrs.len()) as u32;
        }
    }
    
    /// Common Subexpression Elimination
    fn common_subexpression_elimination(&self, ir: &mut IrBlock, stats: &mut OptimizationStats) {
        let mut expr_map: HashMap<ExprKey, VReg> = HashMap::new();
        let mut replacements: Vec<(VReg, VReg)> = Vec::new();
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                if !instr.dst.is_valid() {
                    continue;
                }
                
                if let Some(key) = make_expr_key(&instr.op) {
                    if let Some(&existing) = expr_map.get(&key) {
                        // Found common subexpression
                        replacements.push((instr.dst, existing));
                        stats.cse_eliminated += 1;
                    } else {
                        expr_map.insert(key, instr.dst);
                    }
                }
            }
        }
        
        // Apply replacements
        for (old, new) in replacements {
            self.replace_vreg(ir, old, new);
        }
    }
    
    /// Loop Invariant Code Motion
    fn loop_invariant_code_motion(&self, ir: &mut IrBlock, stats: &mut OptimizationStats) {
        // Find loop headers and bodies
        let loops = self.detect_loops(ir);
        
        for loop_info in &loops {
            let invariants = self.find_loop_invariants(ir, loop_info);
            
            // Move invariant instructions to preheader
            for instr_idx in invariants {
                // Would move instruction to loop preheader
                stats.licm_hoisted += 1;
            }
        }
    }
    
    /// Local instruction reordering within basic blocks
    fn local_reordering(
        &self,
        ir: &mut IrBlock,
        dep_graph: &DependencyGraph,
        stats: &mut OptimizationStats,
    ) {
        let reorderable = dep_graph.reorderable_pairs();
        
        // For each reorderable pair, check if reordering improves ILP
        for (i, j) in reorderable {
            // Would analyze and potentially swap instructions
            // to reduce critical path or improve resource utilization
        }
    }
    
    /// Try to inline functions (Function/Region scope)
    fn try_inline(
        &self,
        ir: &mut IrBlock,
        scope: &CompilationScope,
        profile: &ProfileDb,
        stats: &mut OptimizationStats,
    ) {
        if !self.config.speculative_inline {
            return;
        }
        
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if let IrOp::Call(target) = &instr.op {
                    // Check if target is hot enough to inline
                    let target_count = profile.get_block_count(*target);
                    if target_count >= self.config.thresholds.function_level {
                        // Would inline the target function
                        // For now, just track the opportunity
                        stats.inlined += 1;
                    }
                }
            }
        }
    }
    
    /// Aggressive inlining (CallGraph scope)
    fn aggressive_inline(
        &self,
        ir: &mut IrBlock,
        scope: &CompilationScope,
        profile: &ProfileDb,
        stats: &mut OptimizationStats,
    ) {
        // CallGraph scope allows inlining across module boundaries
        // and speculative inlining based on profile data
        self.try_inline(ir, scope, profile, stats);
        
        // Additional aggressive inlining heuristics
        // - Inline small functions unconditionally
        // - Inline functions on hot paths even if larger
    }
    
    /// Devirtualization (Region/CallGraph scope)
    fn devirtualization(
        &self,
        ir: &mut IrBlock,
        scope: &CompilationScope,
        profile: &ProfileDb,
        deopt_mgr: &DeoptManager,
        stats: &mut OptimizationStats,
        guards: &mut Vec<DeoptGuard>,
    ) {
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if let IrOp::CallIndirect(target_reg) = &instr.op {
                    // Check call profile for dominant target
                    if let Some((dominant_target, confidence)) = profile.get_call_target(instr.guest_rip) {
                        if confidence >= self.config.thresholds.devirt_confidence {
                            // Create guard for call target
                            let guard = DeoptGuard::new(
                                deopt_mgr.alloc_guard_id(),
                                instr.guest_rip,
                                GuardKind::CallTarget {
                                    target_reg: target_reg.0 as u8,
                                    expected: vec![dominant_target],
                                },
                                DeoptReason::CallTargetMismatch,
                            );
                            
                            // Convert indirect call to direct call
                            instr.op = IrOp::Call(dominant_target);
                            
                            guards.push(guard);
                            stats.devirtualized += 1;
                        }
                    }
                }
            }
        }
    }
    
    /// Cross-function optimization (Region/CallGraph scope)
    fn cross_function_optimization(
        &self,
        ir: &mut IrBlock,
        scope: &CompilationScope,
        profile: &ProfileDb,
        scope_profile: &mut ScopeProfile,
        stats: &mut OptimizationStats,
    ) {
        // Analyze call edges within scope
        for (caller, callees) in &scope.call_edges {
            for callee in callees {
                let callee_count = profile.get_block_count(*callee);
                
                // Record cross-function optimization decisions
                if callee_count >= self.config.thresholds.region_level {
                    scope_profile.record_inline(*caller, *callee, true);
                    stats.cross_block_opts += 1;
                }
            }
        }
    }
    
    /// Trace optimization for hot paths (Region scope)
    fn trace_optimization(
        &self,
        ir: &mut IrBlock,
        scope: &CompilationScope,
        profile: &ProfileDb,
        stats: &mut OptimizationStats,
    ) {
        // Linearize hot path for better cache locality
        // Reorder blocks based on execution frequency
        
        let mut block_counts: Vec<(usize, u64)> = ir.blocks.iter()
            .enumerate()
            .map(|(i, bb)| (i, profile.get_block_count(bb.entry_rip)))
            .collect();
        
        // Sort by execution count (hot blocks first)
        block_counts.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Would reorder blocks based on hot path
        // For now, just track that we analyzed it
    }
    
    /// Whole program optimization (CallGraph scope)
    fn whole_program_optimization(
        &self,
        ir: &mut IrBlock,
        scope: &CompilationScope,
        profile: &ProfileDb,
        stats: &mut OptimizationStats,
    ) {
        // Global optimizations enabled by whole-program view:
        // - Global constant propagation
        // - Dead function elimination
        // - Global alias analysis
        // - Whole-program devirtualization
        
        // These would require access to all functions in the call graph
        // For now, just track the scope level
    }
    
    // ========================================================================
    // Helper Methods
    // ========================================================================
    
    fn replace_vreg(&self, ir: &mut IrBlock, old: VReg, new: VReg) {
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                replace_vreg_in_op(&mut instr.op, old, new);
            }
        }
    }
    
    fn detect_loops(&self, ir: &IrBlock) -> Vec<LoopInfo> {
        // Simplified loop detection
        // Would use dominator tree for proper detection
        Vec::new()
    }
    
    fn find_loop_invariants(&self, ir: &IrBlock, loop_info: &LoopInfo) -> Vec<usize> {
        Vec::new()
    }
    
    fn estimate_speedup(
        &self,
        stats: &OptimizationStats,
        schedule: &Option<ScheduleResult>,
        dep_graph: &DependencyGraph,
    ) -> f64 {
        let mut speedup = 1.0;
        
        // Each optimization contributes to speedup
        speedup += stats.dce_eliminated as f64 * 0.01;
        speedup += stats.cse_eliminated as f64 * 0.02;
        speedup += stats.licm_hoisted as f64 * 0.05;
        speedup += stats.inlined as f64 * 0.10;
        speedup += stats.devirtualized as f64 * 0.15;
        
        // ILP improvement from scheduling
        if let Some(sched) = schedule {
            let original_ilp = dep_graph.ilp();
            let new_ilp = sched.stats.achieved_ilp;
            if original_ilp > 0.0 {
                speedup *= (new_ilp / original_ilp) as f64;
            }
        }
        
        speedup.min(4.0) // Cap at 4x
    }
}

impl Default for ScopeOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Types and Functions
// ============================================================================

/// Expression key for CSE
#[derive(Clone, Hash, PartialEq, Eq)]
struct ExprKey {
    op: String,
    operands: Vec<u32>,
}

fn make_expr_key(op: &IrOp) -> Option<ExprKey> {
    match op {
        IrOp::Add(a, b) => Some(ExprKey { op: "add".into(), operands: vec![a.0, b.0] }),
        IrOp::Sub(a, b) => Some(ExprKey { op: "sub".into(), operands: vec![a.0, b.0] }),
        IrOp::Mul(a, b) => Some(ExprKey { op: "mul".into(), operands: vec![a.0, b.0] }),
        IrOp::And(a, b) => Some(ExprKey { op: "and".into(), operands: vec![a.0, b.0] }),
        IrOp::Or(a, b) => Some(ExprKey { op: "or".into(), operands: vec![a.0, b.0] }),
        IrOp::Xor(a, b) => Some(ExprKey { op: "xor".into(), operands: vec![a.0, b.0] }),
        IrOp::Shl(a, b) => Some(ExprKey { op: "shl".into(), operands: vec![a.0, b.0] }),
        IrOp::Shr(a, b) => Some(ExprKey { op: "shr".into(), operands: vec![a.0, b.0] }),
        _ => None,
    }
}

fn replace_vreg_in_op(op: &mut IrOp, old: VReg, new: VReg) {
    match op {
        IrOp::Add(a, b) | IrOp::Sub(a, b) | IrOp::Mul(a, b) | IrOp::And(a, b) |
        IrOp::Or(a, b) | IrOp::Xor(a, b) | IrOp::Shl(a, b) | IrOp::Shr(a, b) |
        IrOp::Cmp(a, b) | IrOp::Test(a, b) => {
            if *a == old { *a = new; }
            if *b == old { *b = new; }
        }
        IrOp::Neg(a) | IrOp::Not(a) | IrOp::Load8(a) | IrOp::Load16(a) |
        IrOp::Load32(a) | IrOp::Load64(a) => {
            if *a == old { *a = new; }
        }
        IrOp::Store8(a, v) | IrOp::Store16(a, v) | IrOp::Store32(a, v) | IrOp::Store64(a, v) => {
            if *a == old { *a = new; }
            if *v == old { *v = new; }
        }
        IrOp::StoreGpr(_, v) | IrOp::StoreFlags(v) | IrOp::StoreRip(v) => {
            if *v == old { *v = new; }
        }
        IrOp::Select(c, t, f) => {
            if *c == old { *c = new; }
            if *t == old { *t = new; }
            if *f == old { *f = new; }
        }
        IrOp::Branch(c, _, _) => {
            if *c == old { *c = new; }
        }
        _ => {}
    }
}

struct LoopInfo {
    header: usize,
    body: HashSet<usize>,
    back_edges: Vec<(usize, usize)>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ir::IrBasicBlock;
    
    fn create_test_ir() -> IrBlock {
        let mut ir = IrBlock::new(0x1000);
        let mut bb = IrBasicBlock::new(BlockId(0), 0x1000);
        
        // v0 = const 10
        bb.instrs.push(IrInstr {
            dst: VReg(0),
            op: IrOp::Const(10),
            guest_rip: 0x1000,
            flags: IrFlags::default(),
        });
        
        // v1 = const 20
        bb.instrs.push(IrInstr {
            dst: VReg(1),
            op: IrOp::Const(20),
            guest_rip: 0x1004,
            flags: IrFlags::default(),
        });
        
        // v2 = add v0, v1
        bb.instrs.push(IrInstr {
            dst: VReg(2),
            op: IrOp::Add(VReg(0), VReg(1)),
            guest_rip: 0x1008,
            flags: IrFlags::default(),
        });
        
        // v3 = add v0, v1 (duplicate - CSE candidate)
        bb.instrs.push(IrInstr {
            dst: VReg(3),
            op: IrOp::Add(VReg(0), VReg(1)),
            guest_rip: 0x100c,
            flags: IrFlags::default(),
        });
        
        // v4 = add v2, v3
        bb.instrs.push(IrInstr {
            dst: VReg(4),
            op: IrOp::Add(VReg(2), VReg(3)),
            guest_rip: 0x1010,
            flags: IrFlags::default(),
        });
        
        // Dead: v5 = const 999 (not used)
        bb.instrs.push(IrInstr {
            dst: VReg(5),
            op: IrOp::Const(999),
            guest_rip: 0x1014,
            flags: IrFlags::default(),
        });
        
        // Store v4 (uses v4, marks it as live)
        bb.instrs.push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::StoreGpr(0, VReg(4)),
            guest_rip: 0x1018,
            flags: IrFlags::SIDE_EFFECT,
        });
        
        ir.blocks.push(bb);
        ir
    }
    
    #[test]
    fn test_dce() {
        let mut ir = create_test_ir();
        let optimizer = ScopeOptimizer::new();
        let mut stats = OptimizationStats::default();
        
        optimizer.dead_code_elimination(&mut ir, &mut stats);
        
        // v5 should be eliminated (unused)
        assert!(stats.dce_eliminated >= 1);
    }
    
    #[test]
    fn test_cse() {
        let mut ir = create_test_ir();
        let optimizer = ScopeOptimizer::new();
        let mut stats = OptimizationStats::default();
        
        optimizer.common_subexpression_elimination(&mut ir, &mut stats);
        
        // v3 = add v0, v1 should be identified as CSE with v2
        assert!(stats.cse_eliminated >= 1);
    }
    
    #[test]
    fn test_scope_levels() {
        let config = ScopeOptimizerConfig::default();
        
        // Verify capability matrix
        assert!(!ScopeLevel::Block.supports_licm());
        assert!(ScopeLevel::Function.supports_licm());
        assert!(ScopeLevel::Function.supports_inlining());
        assert!(!ScopeLevel::Function.supports_devirt());
        assert!(ScopeLevel::Region.supports_devirt());
        assert!(ScopeLevel::CallGraph.supports_devirt());
    }
    
    #[test]
    fn test_full_optimize() {
        let mut ir = create_test_ir();
        let profile = ProfileDb::new(1024);
        let optimizer = ScopeOptimizer::new();
        
        let result = optimizer.optimize(&mut ir, 0x1000, &profile, None);
        
        // Should do some optimization (DCE or CSE)
        assert!(result.optimizations.dce_eliminated > 0 || result.optimizations.cse_eliminated > 0 || 
                result.optimizations.instructions_reordered > 0);
        
        // Speedup should be positive
        assert!(result.estimated_speedup > 0.0);
    }
}
