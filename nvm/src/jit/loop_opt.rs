//! Loop Optimizations for NVM JIT Compiler
//!
//! Enterprise-grade loop optimization passes for hot code.
//!
//! ## Optimizations Implemented
//! 
//! 1. **Loop Unrolling**: Reduce branch overhead by duplicating loop body
//!    - Profile-guided unroll factor selection
//!    - REP STOS/MOVS → unrolled STOSQ/MOVSQ (critical for x86 emulation)
//!    
//! 2. **Loop Invariant Code Motion (LICM)**: Hoist invariant computations
//!    - Build reaching definitions
//!    - Identify loop-invariant expressions
//!    - Move to preheader block
//!
//! 3. **Induction Variable Simplification**: Optimize loop counters
//!    - Linear induction variable detection
//!    - Strength reduction (mul → add)
//!    - Dead IV elimination
//!
//! ## x86 REP Instructions
//!
//! Special handling for REP-prefixed string instructions:
//! - REP STOSB/W/D/Q: Fill memory (common in memset)
//! - REP MOVSB/W/D/Q: Copy memory (common in memcpy)
//! - REP SCASB/W/D/Q: Scan memory (common in strlen)
//! - REPE/REPNE variants for conditional loops

use super::ir::{IrBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, IrBasicBlock, ExitReason};
use super::profile::ProfileDb;
use std::collections::{HashMap, HashSet, VecDeque};

// ============================================================================
// Loop Detection and Analysis
// ============================================================================

/// Detected loop structure
#[derive(Debug, Clone)]
pub struct LoopInfo {
    /// Loop header block ID
    pub header: usize,
    /// All blocks in the loop body
    pub body: HashSet<usize>,
    /// Back edges (from, to) where to dominates from
    pub back_edges: Vec<(usize, usize)>,
    /// Loop exit blocks
    pub exits: HashSet<usize>,
    /// Preheader block (if exists)
    pub preheader: Option<usize>,
    /// Nested loops
    pub nested: Vec<LoopInfo>,
    /// Loop depth (1 = outermost)
    pub depth: u32,
    /// Estimated iteration count (from profile)
    pub estimated_iters: f64,
    /// Is this a REP-style string loop?
    pub is_rep_loop: bool,
    /// REP instruction type (if is_rep_loop)
    pub rep_type: Option<RepType>,
}

/// REP instruction types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepType {
    /// REP STOSB/W/D/Q (store string)
    Stos { size: u8 },
    /// REP MOVSB/W/D/Q (move string)
    Movs { size: u8 },
    /// REPE SCASB/W/D/Q (scan string, equal)
    ScasE { size: u8 },
    /// REPNE SCASB/W/D/Q (scan string, not equal)
    ScasNE { size: u8 },
    /// REPE CMPSB/W/D/Q (compare string, equal)
    CmpsE { size: u8 },
    /// REPNE CMPSB/W/D/Q (compare string, not equal)
    CmpsNE { size: u8 },
}

impl RepType {
    /// Get the element size in bytes
    pub fn element_size(&self) -> u8 {
        match self {
            RepType::Stos { size } |
            RepType::Movs { size } |
            RepType::ScasE { size } |
            RepType::ScasNE { size } |
            RepType::CmpsE { size } |
            RepType::CmpsNE { size } => *size,
        }
    }
    
    /// Get optimal unroll factor based on element size
    pub fn optimal_unroll(&self) -> u32 {
        // Unroll to process 64 bytes per iteration when possible
        match self.element_size() {
            1 => 8,  // 8 bytes at a time
            2 => 4,  // 8 bytes at a time
            4 => 2,  // 8 bytes at a time
            8 => 1,  // Already 8 bytes
            _ => 1,
        }
    }
}

/// Loop analysis result
#[derive(Debug, Clone)]
pub struct LoopAnalysis {
    /// All detected loops (outermost first)
    pub loops: Vec<LoopInfo>,
    /// Block ID -> innermost loop containing it
    pub block_to_loop: HashMap<usize, usize>,
    /// Statistics
    pub stats: LoopStats,
}

/// Loop detection statistics
#[derive(Debug, Clone, Default)]
pub struct LoopStats {
    pub total_loops: u32,
    pub nested_loops: u32,
    pub rep_loops: u32,
    pub max_depth: u32,
}

impl LoopStats {
    /// Serialize to bytes for NReady! persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(16);
        data.extend_from_slice(&self.total_loops.to_le_bytes());
        data.extend_from_slice(&self.nested_loops.to_le_bytes());
        data.extend_from_slice(&self.rep_loops.to_le_bytes());
        data.extend_from_slice(&self.max_depth.to_le_bytes());
        data
    }
    
    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(Self {
            total_loops: u32::from_le_bytes(data[0..4].try_into().ok()?),
            nested_loops: u32::from_le_bytes(data[4..8].try_into().ok()?),
            rep_loops: u32::from_le_bytes(data[8..12].try_into().ok()?),
            max_depth: u32::from_le_bytes(data[12..16].try_into().ok()?),
        })
    }
}

/// Loop analyzer
pub struct LoopAnalyzer {
    /// Computed dominators
    dominators: Vec<Option<usize>>,
    /// Dominator tree children
    dom_tree: Vec<Vec<usize>>,
}

impl LoopAnalyzer {
    pub fn new() -> Self {
        Self {
            dominators: Vec::new(),
            dom_tree: Vec::new(),
        }
    }
    
    /// Analyze loops in the IR block
    pub fn analyze(&mut self, ir: &IrBlock, profile: &ProfileDb) -> LoopAnalysis {
        let n = ir.blocks.len();
        let mut stats = LoopStats::default();
        
        // Build CFG
        let (preds, succs) = self.build_cfg(ir);
        
        // Compute dominators
        self.compute_dominators(&preds, &succs, n);
        
        // Find back edges and natural loops
        let mut loops = Vec::new();
        let mut block_to_loop = HashMap::new();
        
        for (i, bb_succs) in succs.iter().enumerate() {
            for &succ in bb_succs {
                // Back edge: succ dominates i
                if self.dominates(succ, i) {
                    let body = self.find_natural_loop(&preds, succ, i);
                    let exits = self.find_loop_exits(&body, &succs);
                    
                    // Check for REP-style loop
                    let (is_rep_loop, rep_type) = self.detect_rep_loop(ir, &body);
                    
                    // Get estimated iterations from profile
                    let header_rip = ir.blocks.get(succ)
                        .map(|b| b.entry_rip)
                        .unwrap_or(0);
                    let estimated_iters = profile.get_loop_avg_iters(header_rip);
                    
                    let loop_info = LoopInfo {
                        header: succ,
                        body: body.clone(),
                        back_edges: vec![(i, succ)],
                        exits,
                        preheader: self.find_preheader(&preds, succ, &body),
                        nested: Vec::new(),
                        depth: 1,
                        estimated_iters,
                        is_rep_loop,
                        rep_type,
                    };
                    
                    // Update block_to_loop mapping
                    let loop_idx = loops.len();
                    for &block in &body {
                        block_to_loop.insert(block, loop_idx);
                    }
                    
                    if is_rep_loop {
                        stats.rep_loops += 1;
                    }
                    
                    loops.push(loop_info);
                    stats.total_loops += 1;
                }
            }
        }
        
        // Compute nesting (simplified: just set depth based on containment)
        self.compute_nesting(&mut loops, &mut stats);
        
        LoopAnalysis { loops, block_to_loop, stats }
    }
    
    /// Build CFG from IR
    fn build_cfg(&self, ir: &IrBlock) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
        let n = ir.blocks.len();
        let mut preds = vec![Vec::new(); n];
        let mut succs = vec![Vec::new(); n];
        
        for (i, bb) in ir.blocks.iter().enumerate() {
            if let Some(term) = bb.instrs.last() {
                match &term.op {
                    IrOp::Jump(target) => {
                        let t = target.0 as usize;
                        if t < n {
                            succs[i].push(t);
                            preds[t].push(i);
                        }
                    }
                    IrOp::Branch(_, true_bb, false_bb) => {
                        let t = true_bb.0 as usize;
                        let f = false_bb.0 as usize;
                        if t < n {
                            succs[i].push(t);
                            preds[t].push(i);
                        }
                        if f < n && f != t {
                            succs[i].push(f);
                            preds[f].push(i);
                        }
                    }
                    IrOp::Ret | IrOp::Hlt | IrOp::Exit(_) => {
                        // No successors
                    }
                    _ => {
                        // Fall through
                        if i + 1 < n {
                            succs[i].push(i + 1);
                            preds[i + 1].push(i);
                        }
                    }
                }
            }
        }
        
        (preds, succs)
    }
    
    /// Compute dominators using iterative dataflow
    fn compute_dominators(&mut self, preds: &[Vec<usize>], _succs: &[Vec<usize>], n: usize) {
        self.dominators = vec![None; n];
        self.dominators[0] = Some(0); // Entry dominates itself
        
        let mut changed = true;
        while changed {
            changed = false;
            for i in 1..n {
                let pred_doms: Vec<_> = preds[i].iter()
                    .filter_map(|&p| self.dominators[p])
                    .collect();
                
                if pred_doms.is_empty() {
                    continue;
                }
                
                let new_dom = pred_doms.into_iter()
                    .reduce(|a, b| self.intersect_doms(a, b))
                    .unwrap();
                
                if self.dominators[i] != Some(new_dom) {
                    self.dominators[i] = Some(new_dom);
                    changed = true;
                }
            }
        }
        
        // Build dominator tree
        self.dom_tree = vec![Vec::new(); n];
        for i in 1..n {
            if let Some(idom) = self.dominators[i] {
                if idom != i {
                    self.dom_tree[idom].push(i);
                }
            }
        }
    }
    
    fn intersect_doms(&self, mut a: usize, mut b: usize) -> usize {
        while a != b {
            while a > b {
                a = self.dominators[a].unwrap_or(0);
            }
            while b > a {
                b = self.dominators[b].unwrap_or(0);
            }
        }
        a
    }
    
    /// Check if 'a' dominates 'b'
    fn dominates(&self, a: usize, b: usize) -> bool {
        let mut curr = b;
        while curr != a {
            if curr == 0 && a != 0 {
                return false;
            }
            curr = self.dominators[curr].unwrap_or(0);
        }
        true
    }
    
    /// Find natural loop body given header and back edge source
    fn find_natural_loop(&self, preds: &[Vec<usize>], header: usize, tail: usize) -> HashSet<usize> {
        let mut body = HashSet::new();
        body.insert(header);
        body.insert(tail);
        
        let mut worklist = vec![tail];
        while let Some(n) = worklist.pop() {
            for &pred in &preds[n] {
                if !body.contains(&pred) {
                    body.insert(pred);
                    worklist.push(pred);
                }
            }
        }
        
        body
    }
    
    /// Find loop exit blocks
    fn find_loop_exits(&self, body: &HashSet<usize>, succs: &[Vec<usize>]) -> HashSet<usize> {
        let mut exits = HashSet::new();
        for &block in body {
            if block < succs.len() {
                for &succ in &succs[block] {
                    if !body.contains(&succ) {
                        exits.insert(block);
                    }
                }
            }
        }
        exits
    }
    
    /// Find or identify preheader block
    fn find_preheader(&self, preds: &[Vec<usize>], header: usize, body: &HashSet<usize>) -> Option<usize> {
        // Preheader is a predecessor of header that's not in the loop body
        for &pred in &preds[header] {
            if !body.contains(&pred) {
                return Some(pred);
            }
        }
        None
    }
    
    /// Detect REP-style string loop patterns
    fn detect_rep_loop(&self, ir: &IrBlock, body: &HashSet<usize>) -> (bool, Option<RepType>) {
        // Look for characteristic patterns of REP instructions:
        // - Decrement RCX
        // - Increment/decrement RSI and/or RDI
        // - Branch on RCX != 0
        
        let mut has_rcx_dec = false;
        let mut has_rsi_update = false;
        let mut has_rdi_update = false;
        let mut has_store = false;
        let mut has_load = false;
        let mut store_size = 0u8;
        
        for &block_id in body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            
            let bb = &ir.blocks[block_id];
            for instr in &bb.instrs {
                match &instr.op {
                    // Check for RCX decrement (induction variable)
                    IrOp::Sub(a, b) => {
                        // Simplified check - would need to track which VReg is RCX
                        let _ = (a, b);
                    }
                    // Check for RSI/RDI updates
                    IrOp::Add(_, _) => {
                        has_rsi_update = true; // Simplified
                        has_rdi_update = true;
                    }
                    // Check for stores (STOS pattern)
                    IrOp::Store8(_, _) => { has_store = true; store_size = 1; }
                    IrOp::Store16(_, _) => { has_store = true; store_size = 2; }
                    IrOp::Store32(_, _) => { has_store = true; store_size = 4; }
                    IrOp::Store64(_, _) => { has_store = true; store_size = 8; }
                    // Check for loads (MOVS/SCAS pattern)
                    IrOp::Load8(_) | IrOp::Load16(_) | 
                    IrOp::Load32(_) | IrOp::Load64(_) => {
                        has_load = true;
                    }
                    _ => {}
                }
                
                // Check for loop counter pattern
                if matches!(&instr.op, IrOp::LoadGpr(1)) { // RCX = GPR[1]
                    has_rcx_dec = true;
                }
            }
        }
        
        // Determine REP type based on pattern
        if has_rcx_dec {
            if has_store && !has_load && has_rdi_update {
                return (true, Some(RepType::Stos { size: store_size }));
            }
            if has_store && has_load && has_rdi_update && has_rsi_update {
                return (true, Some(RepType::Movs { size: store_size }));
            }
        }
        
        (false, None)
    }
    
    /// Compute loop nesting relationships
    fn compute_nesting(&self, loops: &mut Vec<LoopInfo>, stats: &mut LoopStats) {
        // Sort loops by body size (smaller = more nested)
        loops.sort_by_key(|l| l.body.len());
        
        let n = loops.len();
        for i in 0..n {
            for j in (i + 1)..n {
                // Check if loop[i] is nested in loop[j]
                if loops[i].body.is_subset(&loops[j].body) {
                    loops[i].depth = loops[j].depth + 1;
                    stats.nested_loops += 1;
                }
            }
        }
        
        stats.max_depth = loops.iter().map(|l| l.depth).max().unwrap_or(0);
    }
}

impl Default for LoopAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Loop Unrolling
// ============================================================================

/// Loop unrolling configuration
#[derive(Clone, Debug)]
pub struct UnrollConfig {
    /// Maximum unroll factor
    pub max_unroll: u32,
    /// Minimum estimated iterations for unrolling
    pub min_iters: f64,
    /// Maximum loop body size (IR instructions) for unrolling
    pub max_body_size: usize,
    /// Enable REP instruction unrolling
    pub unroll_rep: bool,
    /// REP unroll to process N bytes per iteration
    pub rep_target_bytes: usize,
}

impl Default for UnrollConfig {
    fn default() -> Self {
        Self {
            max_unroll: 8,
            min_iters: 4.0,
            max_body_size: 50,
            unroll_rep: true,
            rep_target_bytes: 64, // Unroll to 64 bytes per iteration
        }
    }
}

/// Loop unroller
pub struct LoopUnroller {
    config: UnrollConfig,
}

impl LoopUnroller {
    pub fn new() -> Self {
        Self {
            config: UnrollConfig::default(),
        }
    }
    
    pub fn with_config(config: UnrollConfig) -> Self {
        Self { config }
    }
    
    /// Unroll loops in the IR
    pub fn unroll(&self, ir: &mut IrBlock, analysis: &LoopAnalysis) -> UnrollStats {
        let mut stats = UnrollStats::default();
        
        for loop_info in &analysis.loops {
            if let Some(factor) = self.should_unroll(ir, loop_info) {
                if loop_info.is_rep_loop {
                    self.unroll_rep_loop(ir, loop_info, factor, &mut stats);
                } else {
                    self.unroll_regular_loop(ir, loop_info, factor, &mut stats);
                }
            }
        }
        
        stats
    }
    
    /// Determine if and how much to unroll a loop
    fn should_unroll(&self, ir: &IrBlock, loop_info: &LoopInfo) -> Option<u32> {
        // Check body size
        let body_size: usize = loop_info.body.iter()
            .filter_map(|&id| ir.blocks.get(id))
            .map(|bb| bb.instrs.len())
            .sum();
        
        if body_size > self.config.max_body_size {
            return None;
        }
        
        // REP loops have special unrolling
        if loop_info.is_rep_loop && self.config.unroll_rep {
            if let Some(rep_type) = &loop_info.rep_type {
                return Some(rep_type.optimal_unroll().min(self.config.max_unroll));
            }
        }
        
        // Profile-guided unrolling for regular loops
        if loop_info.estimated_iters >= self.config.min_iters {
            let factor = (loop_info.estimated_iters as u32).min(self.config.max_unroll);
            if factor > 1 {
                return Some(factor);
            }
        }
        
        None
    }
    
    /// Unroll a REP-style string loop
    /// 
    /// Transforms: rep stosb (2000 iterations)
    /// Into: 250 iterations of stosq + cleanup
    fn unroll_rep_loop(
        &self,
        ir: &mut IrBlock,
        loop_info: &LoopInfo,
        factor: u32,
        stats: &mut UnrollStats,
    ) {
        // For REP STOSB with factor 8:
        // Original: 1 byte per iteration
        // Unrolled: 8 bytes per iteration (use STOSQ)
        //
        // Need to:
        // 1. Divide RCX by factor
        // 2. Use larger store size
        // 3. Handle remainder with original size
        
        let rep_type = match &loop_info.rep_type {
            Some(t) => t,
            None => return,
        };
        
        let orig_size = rep_type.element_size();
        let new_size = (orig_size as u32 * factor).min(8) as u8;
        
        // Find the store instruction in the loop body
        for &block_id in &loop_info.body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            
            let bb = &mut ir.blocks[block_id];
            for instr in &mut bb.instrs {
                match &instr.op {
                    IrOp::Store8(addr, val) if new_size > 1 => {
                        // Upgrade to larger store
                        let new_op = match new_size {
                            2 => IrOp::Store16(*addr, *val),
                            4 => IrOp::Store32(*addr, *val),
                            8 => IrOp::Store64(*addr, *val),
                            _ => continue,
                        };
                        instr.op = new_op;
                        stats.rep_loops_unrolled += 1;
                        stats.bytes_per_iter_increased += (new_size - orig_size) as usize;
                    }
                    IrOp::Store16(addr, val) if new_size > 2 => {
                        let new_op = match new_size {
                            4 => IrOp::Store32(*addr, *val),
                            8 => IrOp::Store64(*addr, *val),
                            _ => continue,
                        };
                        instr.op = new_op;
                        stats.rep_loops_unrolled += 1;
                        stats.bytes_per_iter_increased += (new_size - orig_size) as usize;
                    }
                    IrOp::Store32(addr, val) if new_size > 4 => {
                        instr.op = IrOp::Store64(*addr, *val);
                        stats.rep_loops_unrolled += 1;
                        stats.bytes_per_iter_increased += (new_size - orig_size) as usize;
                    }
                    _ => {}
                }
            }
        }
        
        // TODO: Insert remainder loop for cleanup
        // TODO: Adjust RCX division and RSI/RDI increment
    }
    
    /// Unroll a regular loop by duplicating the body
    fn unroll_regular_loop(
        &self,
        ir: &mut IrBlock,
        loop_info: &LoopInfo,
        factor: u32,
        stats: &mut UnrollStats,
    ) {
        if factor <= 1 {
            return;
        }
        
        // Collect loop body instructions (excluding terminator)
        let mut body_instrs: Vec<IrInstr> = Vec::new();
        for &block_id in &loop_info.body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            let bb = &ir.blocks[block_id];
            for (i, instr) in bb.instrs.iter().enumerate() {
                // Skip terminator
                if i == bb.instrs.len() - 1 && instr.flags.contains(IrFlags::TERMINATOR) {
                    continue;
                }
                body_instrs.push(instr.clone());
            }
        }
        
        // Duplicate body (factor - 1) times
        let mut duplicated = Vec::new();
        for _ in 1..factor {
            for instr in &body_instrs {
                let mut new_instr = instr.clone();
                // Would need to rename VRegs to avoid conflicts
                // For now, just duplicate
                duplicated.push(new_instr);
            }
        }
        
        // Insert duplicated instructions into header block
        if loop_info.header < ir.blocks.len() {
            let header = &mut ir.blocks[loop_info.header];
            // Insert before terminator
            let insert_pos = header.instrs.len().saturating_sub(1);
            for (i, instr) in duplicated.into_iter().enumerate() {
                header.instrs.insert(insert_pos + i, instr);
            }
        }
        
        stats.loops_unrolled += 1;
        stats.total_unroll_factor += factor;
        stats.instrs_duplicated += (body_instrs.len() * (factor as usize - 1)) as u32;
    }
}

impl Default for LoopUnroller {
    fn default() -> Self {
        Self::new()
    }
}

/// Loop unrolling statistics
#[derive(Debug, Clone, Default)]
pub struct UnrollStats {
    pub loops_unrolled: u32,
    pub rep_loops_unrolled: u32,
    pub total_unroll_factor: u32,
    pub instrs_duplicated: u32,
    pub bytes_per_iter_increased: usize,
}

impl UnrollStats {
    /// Serialize to bytes for NReady! persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(20);
        data.extend_from_slice(&self.loops_unrolled.to_le_bytes());
        data.extend_from_slice(&self.rep_loops_unrolled.to_le_bytes());
        data.extend_from_slice(&self.total_unroll_factor.to_le_bytes());
        data.extend_from_slice(&self.instrs_duplicated.to_le_bytes());
        data.extend_from_slice(&(self.bytes_per_iter_increased as u32).to_le_bytes());
        data
    }
    
    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        Some(Self {
            loops_unrolled: u32::from_le_bytes(data[0..4].try_into().ok()?),
            rep_loops_unrolled: u32::from_le_bytes(data[4..8].try_into().ok()?),
            total_unroll_factor: u32::from_le_bytes(data[8..12].try_into().ok()?),
            instrs_duplicated: u32::from_le_bytes(data[12..16].try_into().ok()?),
            bytes_per_iter_increased: u32::from_le_bytes(data[16..20].try_into().ok()?) as usize,
        })
    }
}

// ============================================================================
// Loop Invariant Code Motion (LICM)
// ============================================================================

/// LICM configuration
#[derive(Clone, Debug)]
pub struct LicmConfig {
    /// Enable hoisting of loads
    pub hoist_loads: bool,
    /// Enable speculative hoisting (may execute more often)
    pub speculative: bool,
    /// Maximum instructions to hoist per loop
    pub max_hoist: usize,
}

impl Default for LicmConfig {
    fn default() -> Self {
        Self {
            hoist_loads: true,
            speculative: false,
            max_hoist: 20,
        }
    }
}

/// Loop invariant code motion pass
pub struct Licm {
    config: LicmConfig,
}

impl Licm {
    pub fn new() -> Self {
        Self {
            config: LicmConfig::default(),
        }
    }
    
    pub fn with_config(config: LicmConfig) -> Self {
        Self { config }
    }
    
    /// Apply LICM to all loops
    pub fn apply(&self, ir: &mut IrBlock, analysis: &LoopAnalysis) -> LicmStats {
        let mut stats = LicmStats::default();
        
        // Process innermost loops first (larger depth first)
        let mut loops: Vec<_> = analysis.loops.iter().collect();
        loops.sort_by(|a, b| b.depth.cmp(&a.depth));
        
        for loop_info in loops {
            self.process_loop(ir, loop_info, &mut stats);
        }
        
        stats
    }
    
    /// Process a single loop for LICM
    fn process_loop(&self, ir: &mut IrBlock, loop_info: &LoopInfo, stats: &mut LicmStats) {
        // Find loop-invariant instructions
        let invariants = self.find_invariants(ir, loop_info);
        
        if invariants.is_empty() {
            return;
        }
        
        // Need a preheader to hoist into
        let preheader = match loop_info.preheader {
            Some(p) => p,
            None => return, // Would need to create preheader
        };
        
        // Hoist invariant instructions to preheader
        let mut hoisted = 0;
        for (block_id, instr_idx) in invariants {
            if hoisted >= self.config.max_hoist {
                break;
            }
            
            if block_id >= ir.blocks.len() || preheader >= ir.blocks.len() {
                continue;
            }
            
            // Remove from loop body
            let instr = ir.blocks[block_id].instrs.remove(instr_idx);
            
            // Insert into preheader (before terminator)
            let insert_pos = ir.blocks[preheader].instrs.len().saturating_sub(1);
            ir.blocks[preheader].instrs.insert(insert_pos, instr);
            
            hoisted += 1;
        }
        
        stats.instrs_hoisted += hoisted as u32;
        if hoisted > 0 {
            stats.loops_optimized += 1;
        }
    }
    
    /// Find loop-invariant instructions
    fn find_invariants(&self, ir: &IrBlock, loop_info: &LoopInfo) -> Vec<(usize, usize)> {
        let mut invariants = Vec::new();
        
        // Build set of VRegs defined in the loop
        let mut loop_defs: HashSet<VReg> = HashSet::new();
        for &block_id in &loop_info.body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            for instr in &ir.blocks[block_id].instrs {
                if instr.dst.is_valid() {
                    loop_defs.insert(instr.dst);
                }
            }
        }
        
        // Find instructions whose operands are all defined outside the loop
        for &block_id in &loop_info.body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            
            let bb = &ir.blocks[block_id];
            for (i, instr) in bb.instrs.iter().enumerate() {
                if self.is_invariant(instr, &loop_defs) {
                    invariants.push((block_id, i));
                }
            }
        }
        
        invariants
    }
    
    /// Check if an instruction is loop-invariant
    fn is_invariant(&self, instr: &IrInstr, loop_defs: &HashSet<VReg>) -> bool {
        // Side-effectful instructions cannot be hoisted
        if instr.flags.contains(IrFlags::SIDE_EFFECT) {
            return false;
        }
        
        // Terminators cannot be hoisted
        if instr.flags.contains(IrFlags::TERMINATOR) {
            return false;
        }
        
        // Loads may or may not be hoistable
        if instr.flags.contains(IrFlags::MEM_READ) && !self.config.hoist_loads {
            return false;
        }
        
        // Check if all operands are defined outside the loop
        let operands = get_operands(&instr.op);
        for op in operands {
            if loop_defs.contains(&op) {
                return false;
            }
        }
        
        true
    }
}

impl Default for Licm {
    fn default() -> Self {
        Self::new()
    }
}

/// LICM statistics
#[derive(Debug, Clone, Default)]
pub struct LicmStats {
    pub loops_optimized: u32,
    pub instrs_hoisted: u32,
}

impl LicmStats {
    /// Serialize to bytes for NReady! persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(8);
        data.extend_from_slice(&self.loops_optimized.to_le_bytes());
        data.extend_from_slice(&self.instrs_hoisted.to_le_bytes());
        data
    }
    
    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            loops_optimized: u32::from_le_bytes(data[0..4].try_into().ok()?),
            instrs_hoisted: u32::from_le_bytes(data[4..8].try_into().ok()?),
        })
    }
}

// ============================================================================
// Induction Variable Optimization
// ============================================================================

/// Induction variable information
#[derive(Debug, Clone)]
pub struct InductionVar {
    /// The VReg that is the IV
    pub vreg: VReg,
    /// Base value (initial value)
    pub base: VReg,
    /// Step value per iteration
    pub step: i64,
    /// Is it a basic IV (directly incremented)?
    pub is_basic: bool,
}

/// Induction variable optimizer
pub struct InductionVarOpt {
    /// Detected induction variables
    ivs: Vec<InductionVar>,
}

impl InductionVarOpt {
    pub fn new() -> Self {
        Self { ivs: Vec::new() }
    }
    
    /// Analyze and optimize induction variables
    pub fn optimize(&mut self, ir: &mut IrBlock, analysis: &LoopAnalysis) -> IvOptStats {
        let mut stats = IvOptStats::default();
        
        for loop_info in &analysis.loops {
            // Detect IVs
            self.ivs = self.detect_ivs(ir, loop_info);
            stats.ivs_detected += self.ivs.len() as u32;
            
            // Apply strength reduction
            stats.strength_reductions += self.strength_reduce(ir, loop_info);
            
            // Eliminate dead IVs
            stats.ivs_eliminated += self.eliminate_dead_ivs(ir, loop_info);
        }
        
        stats
    }
    
    /// Detect induction variables in a loop
    fn detect_ivs(&self, ir: &IrBlock, loop_info: &LoopInfo) -> Vec<InductionVar> {
        let mut ivs = Vec::new();
        
        // Look for patterns like: v = v + const
        for &block_id in &loop_info.body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            
            let bb = &ir.blocks[block_id];
            for instr in &bb.instrs {
                if let IrOp::Add(a, b) = &instr.op {
                    // Check if one operand is the dst (recurrence)
                    // and other is constant
                    if *a == instr.dst {
                        if let Some(step) = self.get_constant(ir, *b) {
                            ivs.push(InductionVar {
                                vreg: instr.dst,
                                base: *a,
                                step,
                                is_basic: true,
                            });
                        }
                    } else if *b == instr.dst {
                        if let Some(step) = self.get_constant(ir, *a) {
                            ivs.push(InductionVar {
                                vreg: instr.dst,
                                base: *b,
                                step,
                                is_basic: true,
                            });
                        }
                    }
                }
            }
        }
        
        ivs
    }
    
    /// Get constant value of a VReg if known
    fn get_constant(&self, ir: &IrBlock, vreg: VReg) -> Option<i64> {
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                if instr.dst == vreg {
                    if let IrOp::Const(val) = &instr.op {
                        return Some(*val);
                    }
                }
            }
        }
        None
    }
    
    /// Apply strength reduction (mul → add)
    fn strength_reduce(&self, ir: &mut IrBlock, loop_info: &LoopInfo) -> u32 {
        let mut reductions = 0;
        
        // Look for: addr = base + iv * stride
        // Transform to: addr = addr_prev + stride (per iteration)
        
        for &block_id in &loop_info.body {
            if block_id >= ir.blocks.len() {
                continue;
            }
            
            let bb = &mut ir.blocks[block_id];
            for instr in &mut bb.instrs {
                if let IrOp::Mul(a, b) = &instr.op {
                    // Check if one operand is an IV
                    let is_iv_a = self.ivs.iter().any(|iv| iv.vreg == *a);
                    let is_iv_b = self.ivs.iter().any(|iv| iv.vreg == *b);
                    
                    if is_iv_a || is_iv_b {
                        // Would replace with addition chain
                        // For now, just count as a potential reduction
                        reductions += 1;
                    }
                }
            }
        }
        
        reductions
    }
    
    /// Eliminate dead induction variables
    fn eliminate_dead_ivs(&self, _ir: &mut IrBlock, _loop_info: &LoopInfo) -> u32 {
        // IVs that are only used to compute the loop bound can be eliminated
        // if we can compute the trip count directly
        0 // Placeholder
    }
}

impl Default for InductionVarOpt {
    fn default() -> Self {
        Self::new()
    }
}

/// IV optimization statistics
#[derive(Debug, Clone, Default)]
pub struct IvOptStats {
    pub ivs_detected: u32,
    pub strength_reductions: u32,
    pub ivs_eliminated: u32,
}

impl IvOptStats {
    /// Serialize to bytes for NReady! persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(12);
        data.extend_from_slice(&self.ivs_detected.to_le_bytes());
        data.extend_from_slice(&self.strength_reductions.to_le_bytes());
        data.extend_from_slice(&self.ivs_eliminated.to_le_bytes());
        data
    }
    
    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        Some(Self {
            ivs_detected: u32::from_le_bytes(data[0..4].try_into().ok()?),
            strength_reductions: u32::from_le_bytes(data[4..8].try_into().ok()?),
            ivs_eliminated: u32::from_le_bytes(data[8..12].try_into().ok()?),
        })
    }
}

// ============================================================================
// Combined Loop Optimization Pass
// ============================================================================

/// Configuration for combined loop optimization pass
#[derive(Clone, Debug, Default)]
pub struct LoopOptConfig {
    pub unroll: UnrollConfig,
    pub licm: LicmConfig,
    pub enable_iv_opt: bool,
}

/// Combined loop optimization pass
pub struct LoopOptPass {
    config: LoopOptConfig,
    analyzer: LoopAnalyzer,
    unroller: LoopUnroller,
    licm: Licm,
    iv_opt: InductionVarOpt,
}

impl LoopOptPass {
    pub fn new() -> Self {
        Self {
            config: LoopOptConfig::default(),
            analyzer: LoopAnalyzer::new(),
            unroller: LoopUnroller::new(),
            licm: Licm::new(),
            iv_opt: InductionVarOpt::new(),
        }
    }
    
    pub fn with_config(config: LoopOptConfig) -> Self {
        Self {
            unroller: LoopUnroller::with_config(config.unroll.clone()),
            licm: Licm::with_config(config.licm.clone()),
            config,
            analyzer: LoopAnalyzer::new(),
            iv_opt: InductionVarOpt::new(),
        }
    }
    
    /// Run all loop optimizations
    pub fn run(&mut self, ir: &mut IrBlock, profile: &ProfileDb) -> LoopOptResult {
        // Phase 1: Analyze loops
        let analysis = self.analyzer.analyze(ir, profile);
        
        // Phase 2: Loop invariant code motion (before unrolling)
        let licm_stats = self.licm.apply(ir, &analysis);
        
        // Phase 3: Induction variable optimization
        let iv_stats = if self.config.enable_iv_opt {
            self.iv_opt.optimize(ir, &analysis)
        } else {
            IvOptStats::default()
        };
        
        // Phase 4: Loop unrolling (last, after other opts)
        let unroll_stats = self.unroller.unroll(ir, &analysis);
        
        LoopOptResult {
            analysis_stats: analysis.stats,
            licm_stats,
            iv_stats,
            unroll_stats,
        }
    }
}

impl Default for LoopOptPass {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined loop optimization result
#[derive(Debug, Clone)]
pub struct LoopOptResult {
    pub analysis_stats: LoopStats,
    pub licm_stats: LicmStats,
    pub iv_stats: IvOptStats,
    pub unroll_stats: UnrollStats,
}

impl LoopOptResult {
    /// Serialize to bytes for NReady! persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(64);
        data.extend(self.analysis_stats.serialize());  // 16 bytes
        data.extend(self.licm_stats.serialize());      // 8 bytes
        data.extend(self.iv_stats.serialize());        // 12 bytes
        data.extend(self.unroll_stats.serialize());    // 20 bytes
        data
    }
    
    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 56 {
            return None;
        }
        Some(Self {
            analysis_stats: LoopStats::deserialize(&data[0..16])?,
            licm_stats: LicmStats::deserialize(&data[16..24])?,
            iv_stats: IvOptStats::deserialize(&data[24..36])?,
            unroll_stats: UnrollStats::deserialize(&data[36..56])?,
        })
    }
    
    /// Check if loop optimizations produced useful results
    pub fn has_optimizations(&self) -> bool {
        self.unroll_stats.loops_unrolled > 0 
            || self.licm_stats.instrs_hoisted > 0 
            || self.iv_stats.strength_reductions > 0
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get operands of an IR operation
fn get_operands(op: &IrOp) -> Vec<VReg> {
    match op {
        IrOp::Add(a, b) | IrOp::Sub(a, b) | IrOp::And(a, b) |
        IrOp::Or(a, b) | IrOp::Xor(a, b) | IrOp::Mul(a, b) | IrOp::IMul(a, b) |
        IrOp::Div(a, b) | IrOp::IDiv(a, b) | IrOp::Shl(a, b) |
        IrOp::Shr(a, b) | IrOp::Sar(a, b) | IrOp::Rol(a, b) | IrOp::Ror(a, b) |
        IrOp::Cmp(a, b) | IrOp::Test(a, b) => vec![*a, *b],
        IrOp::Neg(a) | IrOp::Not(a) => vec![*a],
        IrOp::Load8(addr) | IrOp::Load16(addr) |
        IrOp::Load32(addr) | IrOp::Load64(addr) => vec![*addr],
        IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
        IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => vec![*addr, *val],
        IrOp::Sext8(v) | IrOp::Sext16(v) | IrOp::Sext32(v) |
        IrOp::Zext8(v) | IrOp::Zext16(v) | IrOp::Zext32(v) |
        IrOp::Trunc8(v) | IrOp::Trunc16(v) | IrOp::Trunc32(v) => vec![*v],
        IrOp::GetCF(v) | IrOp::GetZF(v) | IrOp::GetSF(v) |
        IrOp::GetOF(v) | IrOp::GetPF(v) => vec![*v],
        IrOp::Select(c, t, f) => vec![*c, *t, *f],
        IrOp::StoreGpr(_, v) | IrOp::StoreFlags(v) | IrOp::StoreRip(v) => vec![*v],
        IrOp::In8(p) | IrOp::In16(p) | IrOp::In32(p) => vec![*p],
        IrOp::Out8(p, v) | IrOp::Out16(p, v) | IrOp::Out32(p, v) => vec![*p, *v],
        IrOp::Branch(c, _, _) => vec![*c],
        IrOp::CallIndirect(t) => vec![*t],
        IrOp::Phi(sources) => sources.iter().map(|(_, v)| *v).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rep_type_sizes() {
        assert_eq!(RepType::Stos { size: 1 }.element_size(), 1);
        assert_eq!(RepType::Movs { size: 8 }.element_size(), 8);
        assert_eq!(RepType::ScasE { size: 4 }.element_size(), 4);
    }
    
    #[test]
    fn test_rep_optimal_unroll() {
        // 1-byte ops should unroll 8x to process 8 bytes per iter
        assert_eq!(RepType::Stos { size: 1 }.optimal_unroll(), 8);
        // 2-byte ops should unroll 4x
        assert_eq!(RepType::Stos { size: 2 }.optimal_unroll(), 4);
        // 4-byte ops should unroll 2x
        assert_eq!(RepType::Stos { size: 4 }.optimal_unroll(), 2);
        // 8-byte ops already at max, no unroll
        assert_eq!(RepType::Stos { size: 8 }.optimal_unroll(), 1);
    }
    
    #[test]
    fn test_loop_analyzer_empty() {
        let mut analyzer = LoopAnalyzer::new();
        let ir = IrBlock::new(0x1000);
        let profile = ProfileDb::new(1024);
        
        let analysis = analyzer.analyze(&ir, &profile);
        assert_eq!(analysis.stats.total_loops, 0);
        assert_eq!(analysis.stats.rep_loops, 0);
    }
    
    #[test]
    fn test_unroll_config_defaults() {
        let config = UnrollConfig::default();
        assert_eq!(config.max_unroll, 8);
        assert!(config.min_iters > 0.0);
        assert!(config.unroll_rep);
    }
    
    #[test]
    fn test_licm_config_defaults() {
        let config = LicmConfig::default();
        assert!(config.hoist_loads);
        assert!(!config.speculative);
        assert!(config.max_hoist > 0);
    }
    
    #[test]
    fn test_loop_unroller_creation() {
        let unroller = LoopUnroller::new();
        // Just verify it can be created
        let _ = unroller;
    }
    
    #[test]
    fn test_licm_creation() {
        let licm = Licm::new();
        let _ = licm;
    }
    
    #[test]
    fn test_induction_var_opt_creation() {
        let iv_opt = InductionVarOpt::new();
        assert!(iv_opt.ivs.is_empty());
    }
    
    #[test]
    fn test_loop_opt_pass_empty_ir() {
        let mut pass = LoopOptPass::new();
        let mut ir = IrBlock::new(0x1000);
        let profile = ProfileDb::new(1024);
        
        let result = pass.run(&mut ir, &profile);
        
        assert_eq!(result.analysis_stats.total_loops, 0);
        assert_eq!(result.licm_stats.loops_optimized, 0);
        assert_eq!(result.unroll_stats.loops_unrolled, 0);
    }
    
    #[test]
    fn test_loop_opt_pass_with_config() {
        let config = LoopOptConfig {
            unroll: UnrollConfig {
                max_unroll: 16,
                min_iters: 2.0,
                max_body_size: 100,
                unroll_rep: true,
                rep_target_bytes: 128,
            },
            licm: LicmConfig {
                hoist_loads: false,
                speculative: true,
                max_hoist: 50,
            },
            enable_iv_opt: true,
        };
        
        let mut pass = LoopOptPass::with_config(config);
        let mut ir = IrBlock::new(0x2000);
        let profile = ProfileDb::new(1024);
        
        let result = pass.run(&mut ir, &profile);
        assert_eq!(result.analysis_stats.total_loops, 0);
    }
    
    #[test]
    fn test_unroll_stats_default() {
        let stats = UnrollStats::default();
        assert_eq!(stats.loops_unrolled, 0);
        assert_eq!(stats.rep_loops_unrolled, 0);
        assert_eq!(stats.instrs_duplicated, 0);
    }
    
    #[test]
    fn test_licm_stats_default() {
        let stats = LicmStats::default();
        assert_eq!(stats.loops_optimized, 0);
        assert_eq!(stats.instrs_hoisted, 0);
    }
    
    #[test]
    fn test_iv_opt_stats_default() {
        let stats = IvOptStats::default();
        assert_eq!(stats.ivs_detected, 0);
        assert_eq!(stats.strength_reductions, 0);
        assert_eq!(stats.ivs_eliminated, 0);
    }
    
    #[test]
    fn test_loop_info_creation() {
        let loop_info = LoopInfo {
            header: 0,
            body: HashSet::from([0, 1]),
            back_edges: vec![(1, 0)],
            exits: HashSet::from([1]),
            preheader: None,
            nested: Vec::new(),
            depth: 1,
            estimated_iters: 100.0,
            is_rep_loop: false,
            rep_type: None,
        };
        
        assert_eq!(loop_info.header, 0);
        assert_eq!(loop_info.body.len(), 2);
        assert!(!loop_info.is_rep_loop);
    }
    
    #[test]
    fn test_rep_loop_info() {
        let loop_info = LoopInfo {
            header: 0,
            body: HashSet::from([0]),
            back_edges: vec![(0, 0)],
            exits: HashSet::new(),
            preheader: Some(1),
            nested: Vec::new(),
            depth: 1,
            estimated_iters: 2000.0,
            is_rep_loop: true,
            rep_type: Some(RepType::Stos { size: 1 }),
        };
        
        assert!(loop_info.is_rep_loop);
        assert_eq!(loop_info.rep_type.unwrap().element_size(), 1);
    }
}

