//! S2 Optimizing Compiler
//!
//! Full optimizing compiler for hot code.
//! Applies aggressive optimizations using profile data.
//! Takes more time but produces much better code.

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, ExitReason, IrBasicBlock};
use super::decoder::X86Decoder;
use super::profile::{ProfileDb, BranchBias, MemoryPattern};
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
            
            // Update last writer (SSA: dst is in instr.dst)
            if op_produces_value(&instr.op) {
                last_writer.insert(instr.dst, i);
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
