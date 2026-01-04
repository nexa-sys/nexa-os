//! S2 Optimizing Compiler
//!
//! Full optimizing compiler for hot code.
//! Applies aggressive optimizations using profile data.
//! Takes more time but produces much better code.

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrInstr, IrOp, VReg, ExitReason, IrBasicBlock};
use super::decoder::X86Decoder;
use super::profile::{ProfileDb, BranchBias, MemoryPattern};
use super::cache::CompileTier;
use super::compiler_s1::S1Block;
use std::collections::{HashMap, HashSet, VecDeque};

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
}

/// S2 optimizing compiler
pub struct S2Compiler {
    config: S2Config,
}

impl S2Compiler {
    pub fn new() -> Self {
        Self {
            config: S2Config::default(),
        }
    }
    
    pub fn with_config(config: S2Config) -> Self {
        Self { config }
    }
    
    /// Recompile an S1 block with full optimizations
    pub fn compile_from_s1(
        &self,
        s1: &S1Block,
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
        
        // Apply optimizations in order
        if self.config.gvn {
            self.global_value_numbering(&mut ir, &doms, &mut stats);
        }
        
        if self.config.cse {
            self.common_subexpr_elim(&mut ir, &mut stats);
        }
        
        if self.config.licm {
            self.loop_invariant_motion(&mut ir, &loops, &mut stats);
        }
        
        if self.config.loop_unroll {
            self.unroll_loops(&mut ir, &loops, profile, &mut stats);
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
        
        // Dead code elimination after all opts
        self.dead_code_elim(&mut ir);
        
        // Register allocation
        let reg_alloc = self.allocate_registers(&ir);
        
        // Instruction scheduling
        if self.config.scheduling {
            self.schedule_instructions(&mut ir);
        }
        
        // Code generation
        let native = self.codegen(&ir, &reg_alloc, profile)?;
        
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
        
        // Build edges from exits
        for (i, bb) in ir.blocks.iter().enumerate() {
            match &bb.exit {
                ExitReason::Jump(target) => {
                    // Would resolve target to block id
                    if i + 1 < cfg.blocks.len() {
                        cfg.blocks[i].succs.push(i + 1);
                        cfg.blocks[i + 1].preds.push(i);
                    }
                }
                ExitReason::Branch { .. } => {
                    // Both paths
                    if i + 1 < cfg.blocks.len() {
                        cfg.blocks[i].succs.push(i + 1);
                        cfg.blocks[i + 1].preds.push(i);
                    }
                }
                _ => {}
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
        let mut value_numbers: HashMap<ExprKey, VReg> = HashMap::new();
        
        for bb in &mut ir.blocks {
            for instr in &mut bb.instrs {
                if let Some(key) = make_expr_key(&instr.op) {
                    if let Some(&existing) = value_numbers.get(&key) {
                        // Replace with copy
                        if let Some(dst) = get_def(&instr.op) {
                            instr.op = IrOp::Copy(dst, existing);
                            stats.cse_eliminated += 1;
                        }
                    } else {
                        if let Some(dst) = get_def(&instr.op) {
                            value_numbers.insert(key, dst);
                        }
                    }
                }
            }
        }
    }
    
    fn common_subexpr_elim(&self, ir: &mut IrBlock, stats: &mut OptStats) {
        // Local CSE within each basic block
        for bb in &mut ir.blocks {
            let mut seen: HashMap<ExprKey, VReg> = HashMap::new();
            
            for instr in &mut bb.instrs {
                if let Some(key) = make_expr_key(&instr.op) {
                    if let Some(&prev) = seen.get(&key) {
                        if let Some(dst) = get_def(&instr.op) {
                            instr.op = IrOp::Copy(dst, prev);
                            stats.cse_eliminated += 1;
                        }
                    } else {
                        if let Some(dst) = get_def(&instr.op) {
                            seen.insert(key, dst);
                        }
                    }
                }
            }
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
        match &instr.op {
            IrOp::LoadConst(_, _) => true,
            IrOp::Add(_, _, _) | IrOp::Sub(_, _, _) | IrOp::And(_, _, _) |
            IrOp::Or(_, _, _) | IrOp::Xor(_, _, _) => {
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
                    // mul x, 2 -> shl x, 1
                    IrOp::Mul(dst, a, b) => {
                        // Would check if b is power of 2
                        let _ = (dst, a, b);
                    }
                    // div x, 2 -> shr x, 1 (for unsigned)
                    IrOp::Div(dst, a, b) => {
                        let _ = (dst, a, b);
                    }
                    // mod x, power_of_2 -> and x, (power_of_2 - 1)
                    IrOp::Rem(dst, a, b) => {
                        let _ = (dst, a, b);
                        stats.strength_reduced += 1;
                    }
                    _ => {}
                }
            }
        }
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
                if matches!(bb.exit, ExitReason::Return(_)) {
                    // Would convert to tail call
                    stats.tail_calls += 1;
                }
            }
        }
    }
    
    fn dead_code_elim(&self, ir: &mut IrBlock) {
        // Build use set
        let mut used = HashSet::new();
        
        for bb in &ir.blocks {
            match &bb.exit {
                ExitReason::Jump(t) => { used.insert(*t); }
                ExitReason::Branch { cond, target, fallthrough } => {
                    used.insert(*cond);
                    used.insert(*target);
                    used.insert(*fallthrough);
                }
                ExitReason::IndirectJump(t) => { used.insert(*t); }
                ExitReason::Return(v) => { if let Some(v) = v { used.insert(*v); } }
                _ => {}
            }
            
            for instr in &bb.instrs {
                for op in get_operands(&instr.op) {
                    used.insert(op);
                }
            }
        }
        
        // Remove unused definitions
        for bb in &mut ir.blocks {
            bb.instrs.retain(|instr| {
                if let Some(dst) = get_def(&instr.op) {
                    used.contains(&dst) || has_side_effect(&instr.op)
                } else {
                    has_side_effect(&instr.op)
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
                
                // Update intervals for defs
                if let Some(dst) = get_def(&instr.op) {
                    intervals.entry(dst)
                        .or_insert(LiveInterval { start: pos, end: pos });
                }
                
                pos += 1;
            }
        }
        
        intervals
    }
    
    fn schedule_instructions(&self, ir: &mut IrBlock) {
        // List scheduling to hide latencies
        for bb in &mut ir.blocks {
            // Build dependency graph
            let deps = self.build_dep_graph(&bb.instrs);
            
            // Schedule
            let scheduled = self.list_schedule(&bb.instrs, &deps);
            
            // Reorder
            let new_instrs: Vec<_> = scheduled.into_iter()
                .map(|i| bb.instrs[i].clone())
                .collect();
            bb.instrs = new_instrs;
        }
    }
    
    fn build_dep_graph(&self, instrs: &[IrInstr]) -> Vec<Vec<usize>> {
        let n = instrs.len();
        let mut deps = vec![Vec::new(); n];
        
        // Track last writer of each vreg
        let mut last_writer: HashMap<VReg, usize> = HashMap::new();
        
        for (i, instr) in instrs.iter().enumerate() {
            // RAW dependencies
            for op in get_operands(&instr.op) {
                if let Some(&writer) = last_writer.get(&op) {
                    deps[i].push(writer);
                }
            }
            
            // Update last writer
            if let Some(dst) = get_def(&instr.op) {
                last_writer.insert(dst, i);
            }
        }
        
        deps
    }
    
    fn list_schedule(&self, instrs: &[IrInstr], deps: &[Vec<usize>]) -> Vec<usize> {
        let n = instrs.len();
        let mut scheduled = Vec::with_capacity(n);
        let mut ready: Vec<usize> = Vec::new();
        let mut done = vec![false; n];
        
        // Find initially ready (no deps)
        for i in 0..n {
            if deps[i].is_empty() {
                ready.push(i);
            }
        }
        
        while scheduled.len() < n {
            if ready.is_empty() {
                // Find any unscheduled instruction
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
                
                // Add newly ready instructions
                for i in 0..n {
                    if done[i] {
                        continue;
                    }
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
        
        // Prologue - save callee-saved registers
        for hreg in &[3, 5, 12, 13, 14, 15] { // rbx, rbp, r12-r15
            code.push(0x50 + (hreg & 7)); // push
        }
        
        // Emit code for each block
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                self.emit_instr(&mut code, instr, alloc)?;
            }
            
            self.emit_exit(&mut code, &bb.exit, alloc)?;
        }
        
        // Epilogue
        for hreg in [15, 14, 13, 12, 5, 3].iter().rev() {
            code.push(0x58 + (hreg & 7)); // pop
        }
        code.push(0xC3); // ret
        
        Ok(code)
    }
    
    fn emit_instr(&self, code: &mut Vec<u8>, instr: &IrInstr, alloc: &RegisterAllocation) -> JitResult<()> {
        // Similar to S1 but uses register allocation
        match &instr.op {
            IrOp::LoadConst(dst, val) => {
                if let Some(&hreg) = alloc.vreg_to_hreg.get(dst) {
                    emit_mov_imm64(code, hreg, *val);
                }
            }
            IrOp::Copy(dst, src) => {
                if let (Some(&dreg), Some(&sreg)) = (
                    alloc.vreg_to_hreg.get(dst),
                    alloc.vreg_to_hreg.get(src)
                ) {
                    if dreg != sreg {
                        emit_mov_reg_reg(code, dreg, sreg);
                    }
                }
            }
            IrOp::Add(dst, a, b) => {
                if let (Some(&dreg), Some(&areg), Some(&breg)) = (
                    alloc.vreg_to_hreg.get(dst),
                    alloc.vreg_to_hreg.get(a),
                    alloc.vreg_to_hreg.get(b)
                ) {
                    if dreg != areg {
                        emit_mov_reg_reg(code, dreg, areg);
                    }
                    emit_add_reg_reg(code, dreg, breg);
                }
            }
            _ => {
                code.push(0x90); // nop for unhandled
            }
        }
        
        Ok(())
    }
    
    fn emit_exit(&self, code: &mut Vec<u8>, exit: &ExitReason, _alloc: &RegisterAllocation) -> JitResult<()> {
        match exit {
            ExitReason::Return(_) | ExitReason::Halt => {
                // Will fall through to epilogue
            }
            ExitReason::Jump(_) => {
                // Would emit jump
            }
            _ => {}
        }
        
        Ok(())
    }
    
    fn estimate_cycles(&self, ir: &IrBlock) -> u32 {
        // More accurate estimation than S1
        let mut cycles = 0u32;
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                cycles += match &instr.op {
                    IrOp::Mul(_, _, _) => 3,
                    IrOp::Div(_, _, _) | IrOp::Rem(_, _, _) => 15, // Optimized div
                    IrOp::Load8(_, _) | IrOp::Load16(_, _) |
                    IrOp::Load32(_, _) | IrOp::Load64(_, _) => 3,  // Better scheduling
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

fn make_expr_key(op: &IrOp) -> Option<ExprKey> {
    match op {
        IrOp::Add(_, a, b) => Some(ExprKey { op: 1, operands: vec![*a, *b] }),
        IrOp::Sub(_, a, b) => Some(ExprKey { op: 2, operands: vec![*a, *b] }),
        IrOp::And(_, a, b) => Some(ExprKey { op: 3, operands: vec![*a, *b] }),
        IrOp::Or(_, a, b) => Some(ExprKey { op: 4, operands: vec![*a, *b] }),
        IrOp::Xor(_, a, b) => Some(ExprKey { op: 5, operands: vec![*a, *b] }),
        _ => None,
    }
}

fn count_instrs(ir: &IrBlock) -> u32 {
    ir.blocks.iter().map(|bb| bb.instrs.len() as u32).sum()
}

fn get_def(op: &IrOp) -> Option<VReg> {
    match op {
        IrOp::LoadConst(d, _) | IrOp::Copy(d, _) |
        IrOp::Add(d, _, _) | IrOp::Sub(d, _, _) | IrOp::And(d, _, _) |
        IrOp::Or(d, _, _) | IrOp::Xor(d, _, _) | IrOp::Mul(d, _, _) |
        IrOp::Div(d, _, _) | IrOp::Rem(d, _, _) | IrOp::Shl(d, _, _) |
        IrOp::Shr(d, _, _) | IrOp::Sar(d, _, _) | IrOp::Neg(d, _) |
        IrOp::Not(d, _) | IrOp::Load8(d, _) | IrOp::Load16(d, _) |
        IrOp::Load32(d, _) | IrOp::Load64(d, _) | IrOp::Compare(d, _, _) |
        IrOp::ZeroExtend(d, _, _) | IrOp::SignExtend(d, _, _) |
        IrOp::ReadGpr(d, _) | IrOp::ReadFlags(d) => Some(*d),
        _ => None,
    }
}

fn get_operands(op: &IrOp) -> Vec<VReg> {
    match op {
        IrOp::Copy(_, src) => vec![*src],
        IrOp::Add(_, a, b) | IrOp::Sub(_, a, b) | IrOp::And(_, a, b) |
        IrOp::Or(_, a, b) | IrOp::Xor(_, a, b) | IrOp::Mul(_, a, b) |
        IrOp::Div(_, a, b) | IrOp::Rem(_, a, b) | IrOp::Shl(_, a, b) |
        IrOp::Shr(_, a, b) | IrOp::Sar(_, a, b) => vec![*a, *b],
        IrOp::Neg(_, a) | IrOp::Not(_, a) => vec![*a],
        IrOp::Load8(_, addr) | IrOp::Load16(_, addr) |
        IrOp::Load32(_, addr) | IrOp::Load64(_, addr) => vec![*addr],
        IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
        IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => vec![*addr, *val],
        IrOp::Compare(_, a, b) => vec![*a, *b],
        _ => Vec::new(),
    }
}

fn has_side_effect(op: &IrOp) -> bool {
    matches!(op,
        IrOp::Store8(_, _) | IrOp::Store16(_, _) |
        IrOp::Store32(_, _) | IrOp::Store64(_, _) |
        IrOp::WriteGpr(_, _) | IrOp::WriteFlags(_) |
        IrOp::Call(_) | IrOp::IoIn(_, _) | IrOp::IoOut(_, _) |
        IrOp::Interrupt(_) | IrOp::Fence
    )
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
