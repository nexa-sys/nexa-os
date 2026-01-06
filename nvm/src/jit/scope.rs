//! Compilation Scope and Dependency Graph
//!
//! This module provides infrastructure for scope-aware optimization.
//! The compiler's optimization/reordering potential is determined by
//! the visible code scope (basic block, function, region, call graph).
//!
//! ## Scope Hierarchy
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │                        CallGraph (Whole Program)                      │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                      Region (Hot Path)                          │  │
//! │  │  ┌──────────────────────────────────────────────────────────┐  │  │
//! │  │  │                 Function (Single Entry)                   │  │  │
//! │  │  │  ┌───────────┐  ┌───────────┐  ┌───────────┐             │  │  │
//! │  │  │  │   Block   │─▶│   Block   │─▶│   Block   │             │  │  │
//! │  │  │  └───────────┘  └───────────┘  └───────────┘             │  │  │
//! │  │  │        │              │              │                    │  │  │
//! │  │  │        └──────────────┴──────────────┘                    │  │  │
//! │  │  │               Dependency Graph                            │  │  │
//! │  │  └──────────────────────────────────────────────────────────┘  │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Optimization Benefits by Scope
//!
//! | Scope       | Reordering | DCE | CSE | LICM | Inline | Devirt |
//! |-------------|------------|-----|-----|------|--------|--------|
//! | Block       | ✓ Local    | ✓   | ✓   | ✗    | ✗      | ✗      |
//! | Function    | ✓ CFG      | ✓   | ✓   | ✓    | ✓      | ✗      |
//! | Region      | ✓ Trace    | ✓   | ✓   | ✓    | ✓      | ✓      |
//! | CallGraph   | ✓ Global   | ✓   | ✓   | ✓    | ✓      | ✓      |

use std::collections::{HashMap, HashSet, VecDeque, BinaryHeap};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::cmp::Ordering as CmpOrdering;

use super::ir::{IrBlock, IrInstr, IrOp, VReg, BlockId, IrFlags};
use super::profile::{ProfileDb, BranchBias};

// ============================================================================
// Compilation Scope Types
// ============================================================================

/// Compilation scope level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeLevel {
    /// Single basic block (local optimization only)
    Block,
    /// Single function (intra-procedural optimization)
    Function,
    /// Hot region spanning multiple functions (trace-based)
    Region,
    /// Entire call graph (whole-program optimization)
    CallGraph,
}

impl ScopeLevel {
    /// Can this scope level perform global value numbering?
    pub fn supports_gvn(&self) -> bool {
        *self >= ScopeLevel::Function
    }
    
    /// Can this scope level perform loop-invariant code motion?
    pub fn supports_licm(&self) -> bool {
        *self >= ScopeLevel::Function
    }
    
    /// Can this scope level inline functions?
    pub fn supports_inlining(&self) -> bool {
        *self >= ScopeLevel::Function
    }
    
    /// Can this scope level perform devirtualization?
    pub fn supports_devirt(&self) -> bool {
        *self >= ScopeLevel::Region
    }
    
    /// Can this scope level perform cross-function optimization?
    pub fn supports_interprocedural(&self) -> bool {
        *self >= ScopeLevel::Region
    }
}

/// Compilation scope configuration
#[derive(Debug, Clone)]
pub struct ScopeConfig {
    /// Maximum number of blocks in a function scope
    pub max_function_blocks: usize,
    /// Maximum number of functions in a region scope
    pub max_region_functions: usize,
    /// Minimum execution count to include in region
    pub region_hotness_threshold: u64,
    /// Maximum call graph depth for analysis
    pub max_call_depth: usize,
    /// Enable speculative scope expansion
    pub speculative_expansion: bool,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            max_function_blocks: 256,
            max_region_functions: 16,
            region_hotness_threshold: 1000,
            max_call_depth: 8,
            speculative_expansion: true,
        }
    }
}

// ============================================================================
// Dependency Types
// ============================================================================

/// Dependency type between instructions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DependencyKind {
    /// Read-After-Write (true dependency)
    RAW,
    /// Write-After-Read (anti-dependency)
    WAR,
    /// Write-After-Write (output dependency)
    WAW,
    /// Memory dependency (load/store ordering)
    Memory,
    /// Control dependency (conditional execution)
    Control,
    /// Call dependency (function call barrier)
    Call,
    /// Side effect dependency (I/O, syscall, etc.)
    SideEffect,
}

impl DependencyKind {
    /// Is this a true data dependency that cannot be eliminated?
    pub fn is_true_dependency(&self) -> bool {
        matches!(self, DependencyKind::RAW)
    }
    
    /// Can this dependency be resolved by register renaming?
    pub fn resolvable_by_renaming(&self) -> bool {
        matches!(self, DependencyKind::WAR | DependencyKind::WAW)
    }
    
    /// Is this a memory ordering constraint?
    pub fn is_memory_order(&self) -> bool {
        matches!(self, DependencyKind::Memory)
    }
}

/// Edge in dependency graph
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    /// Source instruction index
    pub from: usize,
    /// Destination instruction index
    pub to: usize,
    /// Type of dependency
    pub kind: DependencyKind,
    /// Latency in cycles (for scheduling)
    pub latency: u8,
    /// Can this edge be speculated past?
    pub speculative: bool,
}

/// Dependency graph node info
#[derive(Debug, Clone, Default)]
pub struct DependencyNode {
    /// Incoming dependency edges
    pub predecessors: Vec<DependencyEdge>,
    /// Outgoing dependency edges
    pub successors: Vec<DependencyEdge>,
    /// Earliest start cycle (for scheduling)
    pub earliest_start: u32,
    /// Latest start cycle (for critical path)
    pub latest_start: u32,
    /// Is this on the critical path?
    pub on_critical_path: bool,
}

// ============================================================================
// Compilation Scope
// ============================================================================

/// A compilation scope containing multiple blocks
#[derive(Debug, Clone)]
pub struct CompilationScope {
    /// Scope identifier
    pub id: u64,
    /// Scope level
    pub level: ScopeLevel,
    /// Entry block RIP
    pub entry_rip: u64,
    /// Block RIPs in this scope (ordered by execution frequency)
    pub blocks: Vec<u64>,
    /// Call edges within scope: caller_rip -> callee_rips
    pub call_edges: HashMap<u64, HashSet<u64>>,
    /// Loop headers in scope
    pub loop_headers: HashSet<u64>,
    /// Estimated execution count
    pub execution_count: u64,
    /// Total IR instruction count
    pub ir_instr_count: u32,
}

impl CompilationScope {
    /// Create a single-block scope
    pub fn single_block(rip: u64) -> Self {
        Self {
            id: rip,
            level: ScopeLevel::Block,
            entry_rip: rip,
            blocks: vec![rip],
            call_edges: HashMap::new(),
            loop_headers: HashSet::new(),
            execution_count: 0,
            ir_instr_count: 0,
        }
    }
    
    /// Create a function scope from entry point
    pub fn function(entry_rip: u64, blocks: Vec<u64>) -> Self {
        Self {
            id: entry_rip,
            level: ScopeLevel::Function,
            entry_rip,
            blocks,
            call_edges: HashMap::new(),
            loop_headers: HashSet::new(),
            execution_count: 0,
            ir_instr_count: 0,
        }
    }
    
    /// Check if a block is in this scope
    pub fn contains_block(&self, rip: u64) -> bool {
        self.blocks.contains(&rip)
    }
    
    /// Add a call edge
    pub fn add_call_edge(&mut self, caller: u64, callee: u64) {
        self.call_edges.entry(caller).or_default().insert(callee);
    }
    
    /// Mark a block as loop header
    pub fn mark_loop_header(&mut self, rip: u64) {
        self.loop_headers.insert(rip);
    }
    
    /// Get all callees from this scope
    pub fn all_callees(&self) -> HashSet<u64> {
        self.call_edges.values().flatten().cloned().collect()
    }
    
    /// Expand scope to include callees (upgrade to Region level)
    pub fn expand_to_region(&mut self, callees: &[u64]) {
        self.level = ScopeLevel::Region;
        for &callee in callees {
            if !self.blocks.contains(&callee) {
                self.blocks.push(callee);
            }
        }
    }
}

// ============================================================================
// Dependency Graph
// ============================================================================

/// Cross-block dependency graph for optimization
pub struct DependencyGraph {
    /// Nodes indexed by instruction position
    nodes: Vec<DependencyNode>,
    /// Total instruction count
    instr_count: usize,
    /// Critical path length
    critical_path_length: u32,
    /// Available ILP (instructions schedulable in parallel)
    available_ilp: f32,
}

impl DependencyGraph {
    /// Build dependency graph from IR block
    pub fn build(ir: &IrBlock) -> Self {
        let mut all_instrs: Vec<&IrInstr> = Vec::new();
        let mut block_offsets: Vec<usize> = Vec::new();
        
        // Flatten all instructions with block offsets
        for bb in &ir.blocks {
            block_offsets.push(all_instrs.len());
            for instr in &bb.instrs {
                all_instrs.push(instr);
            }
        }
        
        let n = all_instrs.len();
        let mut nodes: Vec<DependencyNode> = (0..n).map(|_| DependencyNode::default()).collect();
        
        // Build data dependencies (RAW, WAR, WAW)
        let mut last_write: HashMap<VReg, usize> = HashMap::new();
        let mut last_read: HashMap<VReg, Vec<usize>> = HashMap::new();
        
        for (i, instr) in all_instrs.iter().enumerate() {
            // Check reads
            let reads = get_read_vregs(&instr.op);
            for vreg in &reads {
                // RAW: reader depends on last writer
                if let Some(&writer) = last_write.get(vreg) {
                    let edge = DependencyEdge {
                        from: writer,
                        to: i,
                        kind: DependencyKind::RAW,
                        latency: estimate_latency(&all_instrs[writer].op),
                        speculative: false,
                    };
                    nodes[writer].successors.push(edge.clone());
                    nodes[i].predecessors.push(edge);
                }
                // Track read
                last_read.entry(*vreg).or_default().push(i);
            }
            
            // Check writes
            if instr.dst.is_valid() {
                let dst = instr.dst;
                
                // WAW: this write depends on last write
                if let Some(&prev_writer) = last_write.get(&dst) {
                    let edge = DependencyEdge {
                        from: prev_writer,
                        to: i,
                        kind: DependencyKind::WAW,
                        latency: 0, // No data latency, just ordering
                        speculative: false,
                    };
                    nodes[prev_writer].successors.push(edge.clone());
                    nodes[i].predecessors.push(edge);
                }
                
                // WAR: this write depends on all previous readers
                if let Some(readers) = last_read.remove(&dst) {
                    for reader in readers {
                        if reader != i {
                            let edge = DependencyEdge {
                                from: reader,
                                to: i,
                                kind: DependencyKind::WAR,
                                latency: 0,
                                speculative: false,
                            };
                            nodes[reader].successors.push(edge.clone());
                            nodes[i].predecessors.push(edge);
                        }
                    }
                }
                
                // Update last write
                last_write.insert(dst, i);
            }
        }
        
        // Build memory dependencies
        Self::add_memory_deps(&mut nodes, &all_instrs);
        
        // Build control dependencies
        Self::add_control_deps(&mut nodes, &all_instrs, &block_offsets);
        
        // Compute critical path
        let critical_path_length = Self::compute_critical_path(&mut nodes);
        
        // Compute available ILP
        let available_ilp = Self::compute_ilp(&nodes, n);
        
        Self {
            nodes,
            instr_count: n,
            critical_path_length,
            available_ilp,
        }
    }
    
    fn add_memory_deps(nodes: &mut [DependencyNode], instrs: &[&IrInstr]) {
        let mut last_store: Option<usize> = None;
        let mut loads_since_store: Vec<usize> = Vec::new();
        
        for (i, instr) in instrs.iter().enumerate() {
            let is_load = matches!(
                &instr.op,
                IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_)
            );
            let is_store = matches!(
                &instr.op,
                IrOp::Store8(_, _) | IrOp::Store16(_, _) | IrOp::Store32(_, _) | IrOp::Store64(_, _)
            );
            
            if is_load {
                // Load depends on last store (conservative)
                if let Some(store_idx) = last_store {
                    let edge = DependencyEdge {
                        from: store_idx,
                        to: i,
                        kind: DependencyKind::Memory,
                        latency: 4, // Memory latency
                        speculative: true, // Can speculate if addresses differ
                    };
                    nodes[store_idx].successors.push(edge.clone());
                    nodes[i].predecessors.push(edge);
                }
                loads_since_store.push(i);
            }
            
            if is_store {
                // Store depends on last store
                if let Some(prev_store) = last_store {
                    let edge = DependencyEdge {
                        from: prev_store,
                        to: i,
                        kind: DependencyKind::Memory,
                        latency: 0,
                        speculative: false,
                    };
                    nodes[prev_store].successors.push(edge.clone());
                    nodes[i].predecessors.push(edge);
                }
                // Store depends on all loads (WAR)
                for &load_idx in &loads_since_store {
                    let edge = DependencyEdge {
                        from: load_idx,
                        to: i,
                        kind: DependencyKind::Memory,
                        latency: 0,
                        speculative: true,
                    };
                    nodes[load_idx].successors.push(edge.clone());
                    nodes[i].predecessors.push(edge);
                }
                
                last_store = Some(i);
                loads_since_store.clear();
            }
        }
    }
    
    fn add_control_deps(nodes: &mut [DependencyNode], instrs: &[&IrInstr], block_offsets: &[usize]) {
        // Add control dependencies at block boundaries
        for (block_idx, &offset) in block_offsets.iter().enumerate() {
            // Find the terminator of previous block
            if block_idx > 0 && offset > 0 {
                let prev_term = offset - 1;
                let term_op = &instrs[prev_term].op;
                
                // If previous block ends with branch, add control dep
                if matches!(term_op, IrOp::Branch(_, _, _) | IrOp::Jump(_)) {
                    // First instruction of this block depends on the branch
                    let edge = DependencyEdge {
                        from: prev_term,
                        to: offset,
                        kind: DependencyKind::Control,
                        latency: 1, // Branch misprediction penalty
                        speculative: true,
                    };
                    nodes[prev_term].successors.push(edge.clone());
                    nodes[offset].predecessors.push(edge);
                }
            }
        }
        
        // Call/syscall/sideeffect barriers
        let mut last_barrier: Option<usize> = None;
        for (i, instr) in instrs.iter().enumerate() {
            let is_barrier = matches!(
                &instr.op,
                IrOp::Call(_) | IrOp::CallIndirect(_) | IrOp::Syscall | 
                IrOp::Hlt | IrOp::Exit(_)
            ) || instr.flags.contains(IrFlags::SIDE_EFFECT);
            
            if is_barrier {
                if let Some(prev) = last_barrier {
                    let edge = DependencyEdge {
                        from: prev,
                        to: i,
                        kind: DependencyKind::SideEffect,
                        latency: 0,
                        speculative: false,
                    };
                    nodes[prev].successors.push(edge.clone());
                    nodes[i].predecessors.push(edge);
                }
                last_barrier = Some(i);
            }
        }
    }
    
    fn compute_critical_path(nodes: &mut [DependencyNode]) -> u32 {
        let n = nodes.len();
        if n == 0 {
            return 0;
        }
        
        // Forward pass: compute earliest start times
        for i in 0..n {
            let mut earliest = 0u32;
            for pred in &nodes[i].predecessors {
                let pred_finish = nodes[pred.from].earliest_start + pred.latency as u32;
                earliest = earliest.max(pred_finish);
            }
            nodes[i].earliest_start = earliest;
        }
        
        // Find critical path length
        let max_finish = nodes.iter()
            .map(|n| n.earliest_start + 1)
            .max()
            .unwrap_or(0);
        
        // Backward pass: compute latest start times and mark critical path
        for i in (0..n).rev() {
            let mut latest = max_finish.saturating_sub(1);
            for succ in &nodes[i].successors {
                let required = nodes[succ.to].latest_start.saturating_sub(succ.latency as u32);
                latest = latest.min(required);
            }
            nodes[i].latest_start = latest;
            
            // On critical path if earliest == latest
            nodes[i].on_critical_path = nodes[i].earliest_start == nodes[i].latest_start;
        }
        
        max_finish
    }
    
    fn compute_ilp(nodes: &[DependencyNode], n: usize) -> f32 {
        if n == 0 {
            return 0.0;
        }
        
        // Count instructions at each time slot
        let mut time_slots: HashMap<u32, u32> = HashMap::new();
        for node in nodes {
            *time_slots.entry(node.earliest_start).or_default() += 1;
        }
        
        // Average ILP
        let total_width: u32 = time_slots.values().sum();
        let total_cycles = time_slots.len() as f32;
        
        if total_cycles > 0.0 {
            total_width as f32 / total_cycles
        } else {
            1.0
        }
    }
    
    /// Get instructions on the critical path
    pub fn critical_path(&self) -> Vec<usize> {
        self.nodes.iter()
            .enumerate()
            .filter(|(_, n)| n.on_critical_path)
            .map(|(i, _)| i)
            .collect()
    }
    
    /// Get available instruction-level parallelism
    pub fn ilp(&self) -> f32 {
        self.available_ilp
    }
    
    /// Get critical path length in cycles
    pub fn critical_length(&self) -> u32 {
        self.critical_path_length
    }
    
    /// Get reorderable instruction pairs (no true dependency)
    pub fn reorderable_pairs(&self) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        
        for i in 0..self.instr_count {
            for j in (i + 1)..self.instr_count {
                // Check if i and j have any dependency
                let has_dep = self.nodes[i].successors.iter()
                    .any(|e| e.to == j && e.kind.is_true_dependency());
                let has_rev_dep = self.nodes[j].successors.iter()
                    .any(|e| e.to == i && e.kind.is_true_dependency());
                
                if !has_dep && !has_rev_dep {
                    pairs.push((i, j));
                }
            }
        }
        
        pairs
    }
    
    /// Get instructions that can be speculatively reordered
    pub fn speculative_moves(&self) -> Vec<(usize, usize)> {
        let mut moves = Vec::new();
        
        for (i, node) in self.nodes.iter().enumerate() {
            for edge in &node.successors {
                if edge.speculative && edge.kind == DependencyKind::Memory {
                    moves.push((i, edge.to));
                }
            }
        }
        
        moves
    }
}

// ============================================================================
// Scope Profile - Persisted optimization info
// ============================================================================

/// Profile data for a compilation scope
#[derive(Debug, Clone, Default)]
pub struct ScopeProfile {
    /// Scope identifier
    pub scope_id: u64,
    /// Scope level
    pub level: ScopeLevel,
    /// Inlining decisions: (call_site, callee) -> should_inline
    pub inline_decisions: HashMap<(u64, u64), bool>,
    /// Loop unroll factors: loop_header -> unroll_factor
    pub unroll_factors: HashMap<u64, u32>,
    /// Speculative memory reordering success: (load_rip, store_rip) -> success_rate
    pub memory_reorder_success: HashMap<(u64, u64), f64>,
    /// Value range info: (rip, reg) -> (min, max)
    pub value_ranges: HashMap<(u64, u8), (u64, u64)>,
    /// Discovered constants: (rip, reg) -> value
    pub discovered_constants: HashMap<(u64, u8), u64>,
    /// Hot paths: entry_rip -> (path_blocks, execution_count)
    pub hot_paths: HashMap<u64, (Vec<u64>, u64)>,
    /// Devirtualization targets: call_site -> (target, confidence)
    pub devirt_targets: HashMap<u64, (u64, f64)>,
    /// Escape analysis results: alloc_site -> escapes
    pub escape_info: HashMap<u64, bool>,
    /// Dependency graph statistics
    pub dep_stats: DependencyStats,
}

/// Dependency graph statistics for profiling
#[derive(Debug, Clone, Default)]
pub struct DependencyStats {
    /// Critical path length
    pub critical_path_length: u32,
    /// Available ILP
    pub available_ilp: f32,
    /// Number of RAW dependencies
    pub raw_deps: u32,
    /// Number of memory dependencies
    pub memory_deps: u32,
    /// Number of speculative opportunities
    pub speculative_ops: u32,
}

impl ScopeProfile {
    /// Create new profile for scope
    pub fn new(scope_id: u64, level: ScopeLevel) -> Self {
        Self {
            scope_id,
            level,
            ..Default::default()
        }
    }
    
    /// Record an inlining decision
    pub fn record_inline(&mut self, call_site: u64, callee: u64, should_inline: bool) {
        self.inline_decisions.insert((call_site, callee), should_inline);
    }
    
    /// Record loop unroll factor
    pub fn record_unroll(&mut self, header: u64, factor: u32) {
        self.unroll_factors.insert(header, factor);
    }
    
    /// Record speculative memory reorder result
    pub fn record_memory_reorder(&mut self, load: u64, store: u64, success: bool) {
        let key = (load, store);
        let entry = self.memory_reorder_success.entry(key).or_insert(0.5);
        // Exponential moving average
        *entry = *entry * 0.9 + if success { 0.1 } else { 0.0 };
    }
    
    /// Record discovered value range
    pub fn record_value_range(&mut self, rip: u64, reg: u8, min: u64, max: u64) {
        self.value_ranges.insert((rip, reg), (min, max));
    }
    
    /// Record discovered constant
    pub fn record_constant(&mut self, rip: u64, reg: u8, value: u64) {
        self.discovered_constants.insert((rip, reg), value);
    }
    
    /// Record hot path
    pub fn record_hot_path(&mut self, entry: u64, path: Vec<u64>, count: u64) {
        self.hot_paths.insert(entry, (path, count));
    }
    
    /// Record devirtualization target
    pub fn record_devirt(&mut self, call_site: u64, target: u64, confidence: f64) {
        self.devirt_targets.insert(call_site, (target, confidence));
    }
    
    /// Record escape analysis result
    pub fn record_escape(&mut self, alloc_site: u64, escapes: bool) {
        self.escape_info.insert(alloc_site, escapes);
    }
    
    /// Update dependency stats from graph
    pub fn update_dep_stats(&mut self, graph: &DependencyGraph) {
        self.dep_stats.critical_path_length = graph.critical_length();
        self.dep_stats.available_ilp = graph.ilp();
        
        // Count dependency types
        let mut raw = 0u32;
        let mut memory = 0u32;
        let mut speculative = 0u32;
        
        for node in &graph.nodes {
            for edge in &node.successors {
                match edge.kind {
                    DependencyKind::RAW => raw += 1,
                    DependencyKind::Memory => memory += 1,
                    _ => {}
                }
                if edge.speculative {
                    speculative += 1;
                }
            }
        }
        
        self.dep_stats.raw_deps = raw;
        self.dep_stats.memory_deps = memory;
        self.dep_stats.speculative_ops = speculative;
    }
    
    /// Get recommended inline decision
    pub fn should_inline(&self, call_site: u64, callee: u64) -> Option<bool> {
        self.inline_decisions.get(&(call_site, callee)).copied()
    }
    
    /// Get recommended unroll factor
    pub fn get_unroll_factor(&self, header: u64) -> Option<u32> {
        self.unroll_factors.get(&header).copied()
    }
    
    /// Check if memory reorder is safe
    pub fn is_reorder_safe(&self, load: u64, store: u64, threshold: f64) -> bool {
        self.memory_reorder_success
            .get(&(load, store))
            .map(|&rate| rate >= threshold)
            .unwrap_or(false)
    }
    
    /// Serialize to bytes for NReady! cache
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Header
        data.extend_from_slice(b"NVMS"); // Scope profile magic
        data.extend_from_slice(&1u32.to_le_bytes()); // Version
        data.extend_from_slice(&self.scope_id.to_le_bytes());
        data.push(self.level as u8);
        
        // Inline decisions
        data.extend_from_slice(&(self.inline_decisions.len() as u32).to_le_bytes());
        for ((call_site, callee), should) in &self.inline_decisions {
            data.extend_from_slice(&call_site.to_le_bytes());
            data.extend_from_slice(&callee.to_le_bytes());
            data.push(if *should { 1 } else { 0 });
        }
        
        // Unroll factors
        data.extend_from_slice(&(self.unroll_factors.len() as u32).to_le_bytes());
        for (header, factor) in &self.unroll_factors {
            data.extend_from_slice(&header.to_le_bytes());
            data.extend_from_slice(&factor.to_le_bytes());
        }
        
        // Devirt targets
        data.extend_from_slice(&(self.devirt_targets.len() as u32).to_le_bytes());
        for (call_site, (target, conf)) in &self.devirt_targets {
            data.extend_from_slice(&call_site.to_le_bytes());
            data.extend_from_slice(&target.to_le_bytes());
            data.extend_from_slice(&conf.to_le_bytes());
        }
        
        // Discovered constants
        data.extend_from_slice(&(self.discovered_constants.len() as u32).to_le_bytes());
        for ((rip, reg), value) in &self.discovered_constants {
            data.extend_from_slice(&rip.to_le_bytes());
            data.push(*reg);
            data.extend_from_slice(&value.to_le_bytes());
        }
        
        // Dependency stats
        data.extend_from_slice(&self.dep_stats.critical_path_length.to_le_bytes());
        data.extend_from_slice(&self.dep_stats.available_ilp.to_le_bytes());
        data.extend_from_slice(&self.dep_stats.raw_deps.to_le_bytes());
        data.extend_from_slice(&self.dep_stats.memory_deps.to_le_bytes());
        data.extend_from_slice(&self.dep_stats.speculative_ops.to_le_bytes());
        
        data
    }
    
    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 17 || &data[0..4] != b"NVMS" {
            return None;
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
        if version != 1 {
            return None; // Unknown version
        }
        
        let scope_id = u64::from_le_bytes(data[8..16].try_into().ok()?);
        let level = match data[16] {
            0 => ScopeLevel::Block,
            1 => ScopeLevel::Function,
            2 => ScopeLevel::Region,
            3 => ScopeLevel::CallGraph,
            _ => return None,
        };
        
        let mut profile = ScopeProfile::new(scope_id, level);
        let mut offset = 17;
        
        // Inline decisions
        if offset + 4 > data.len() { return Some(profile); }
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        for _ in 0..count {
            if offset + 17 > data.len() { break; }
            let call_site = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
            let callee = u64::from_le_bytes(data[offset+8..offset+16].try_into().ok()?);
            let should = data[offset+16] != 0;
            profile.inline_decisions.insert((call_site, callee), should);
            offset += 17;
        }
        
        // Unroll factors
        if offset + 4 > data.len() { return Some(profile); }
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        for _ in 0..count {
            if offset + 12 > data.len() { break; }
            let header = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
            let factor = u32::from_le_bytes(data[offset+8..offset+12].try_into().ok()?);
            profile.unroll_factors.insert(header, factor);
            offset += 12;
        }
        
        // Devirt targets
        if offset + 4 > data.len() { return Some(profile); }
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        for _ in 0..count {
            if offset + 24 > data.len() { break; }
            let call_site = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
            let target = u64::from_le_bytes(data[offset+8..offset+16].try_into().ok()?);
            let conf = f64::from_le_bytes(data[offset+16..offset+24].try_into().ok()?);
            profile.devirt_targets.insert(call_site, (target, conf));
            offset += 24;
        }
        
        // Discovered constants
        if offset + 4 > data.len() { return Some(profile); }
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        for _ in 0..count {
            if offset + 17 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
            let reg = data[offset+8];
            let value = u64::from_le_bytes(data[offset+9..offset+17].try_into().ok()?);
            profile.discovered_constants.insert((rip, reg), value);
            offset += 17;
        }
        
        // Dependency stats
        if offset + 20 <= data.len() {
            profile.dep_stats.critical_path_length = 
                u32::from_le_bytes(data[offset..offset+4].try_into().ok()?);
            profile.dep_stats.available_ilp = 
                f32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?);
            profile.dep_stats.raw_deps = 
                u32::from_le_bytes(data[offset+8..offset+12].try_into().ok()?);
            profile.dep_stats.memory_deps = 
                u32::from_le_bytes(data[offset+12..offset+16].try_into().ok()?);
            profile.dep_stats.speculative_ops = 
                u32::from_le_bytes(data[offset+16..offset+20].try_into().ok()?);
        }
        
        Some(profile)
    }
}

impl Default for ScopeLevel {
    fn default() -> Self {
        ScopeLevel::Block
    }
}

// ============================================================================
// Scope Profile Database
// ============================================================================

/// Database of scope profiles
pub struct ScopeProfileDb {
    /// Profiles by scope ID
    profiles: RwLock<HashMap<u64, ScopeProfile>>,
    /// Maximum profiles to keep
    max_profiles: usize,
}

impl ScopeProfileDb {
    /// Create new scope profile database
    pub fn new(max_profiles: usize) -> Self {
        Self {
            profiles: RwLock::new(HashMap::new()),
            max_profiles,
        }
    }
    
    /// Get or create profile for scope
    pub fn get_or_create(&self, scope_id: u64, level: ScopeLevel) -> ScopeProfile {
        let profiles = self.profiles.read().unwrap();
        if let Some(profile) = profiles.get(&scope_id) {
            return profile.clone();
        }
        drop(profiles);
        
        ScopeProfile::new(scope_id, level)
    }
    
    /// Update profile
    pub fn update(&self, profile: ScopeProfile) {
        let mut profiles = self.profiles.write().unwrap();
        if profiles.len() >= self.max_profiles && !profiles.contains_key(&profile.scope_id) {
            // Evict least useful profile (simple LRU for now)
            // In production, would use access timestamps
            if let Some(&first_key) = profiles.keys().next() {
                profiles.remove(&first_key);
            }
        }
        profiles.insert(profile.scope_id, profile);
    }
    
    /// Get profile
    pub fn get(&self, scope_id: u64) -> Option<ScopeProfile> {
        self.profiles.read().unwrap().get(&scope_id).cloned()
    }
    
    /// Serialize all profiles
    pub fn serialize(&self) -> Vec<u8> {
        let profiles = self.profiles.read().unwrap();
        let mut data = Vec::new();
        
        data.extend_from_slice(b"NVSD"); // Scope DB magic
        data.extend_from_slice(&1u32.to_le_bytes()); // Version
        data.extend_from_slice(&(profiles.len() as u32).to_le_bytes());
        
        for profile in profiles.values() {
            let serialized = profile.serialize();
            data.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
            data.extend_from_slice(&serialized);
        }
        
        data
    }
    
    /// Deserialize profiles
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 12 || &data[0..4] != b"NVSD" {
            return None;
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
        if version != 1 {
            return None;
        }
        
        let count = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
        let db = ScopeProfileDb::new(count.max(1000));
        let mut offset = 12;
        
        for _ in 0..count {
            if offset + 4 > data.len() { break; }
            let len = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
            offset += 4;
            
            if offset + len > data.len() { break; }
            if let Some(profile) = ScopeProfile::deserialize(&data[offset..offset+len]) {
                db.update(profile);
            }
            offset += len;
        }
        
        Some(db)
    }
}

// ============================================================================
// Scope Builder
// ============================================================================

/// Builds compilation scopes from profile data
pub struct ScopeBuilder {
    config: ScopeConfig,
}

impl ScopeBuilder {
    pub fn new(config: ScopeConfig) -> Self {
        Self { config }
    }
    
    /// Build optimal scope for a hot block
    pub fn build_scope(&self, entry_rip: u64, profile: &ProfileDb) -> CompilationScope {
        let exec_count = profile.get_block_count(entry_rip);
        
        // Start with single block
        let mut scope = CompilationScope::single_block(entry_rip);
        scope.execution_count = exec_count;
        
        // Check if hot enough for function-level
        if exec_count >= self.config.region_hotness_threshold {
            // Try to expand to function scope
            if let Some(func_scope) = self.try_build_function_scope(entry_rip, profile) {
                scope = func_scope;
                
                // Check if hot enough for region-level
                if exec_count >= self.config.region_hotness_threshold * 10 {
                    if let Some(region_scope) = self.try_expand_to_region(&scope, profile) {
                        scope = region_scope;
                    }
                }
            }
        }
        
        scope
    }
    
    fn try_build_function_scope(&self, entry: u64, profile: &ProfileDb) -> Option<CompilationScope> {
        // Get hot blocks reachable from entry
        let mut blocks = Vec::new();
        let mut visited = HashSet::new();
        let mut worklist = VecDeque::new();
        
        worklist.push_back(entry);
        visited.insert(entry);
        
        while let Some(rip) = worklist.pop_front() {
            if blocks.len() >= self.config.max_function_blocks {
                break;
            }
            
            let count = profile.get_block_count(rip);
            if count < self.config.region_hotness_threshold / 10 {
                continue; // Skip cold blocks
            }
            
            blocks.push(rip);
            
            // Add successors (would need CFG info in profile)
            // For now, just use the block itself
        }
        
        if blocks.len() >= 2 {
            Some(CompilationScope::function(entry, blocks))
        } else {
            None
        }
    }
    
    fn try_expand_to_region(&self, func_scope: &CompilationScope, profile: &ProfileDb) -> Option<CompilationScope> {
        let mut scope = func_scope.clone();
        
        // Find hot callees
        let callees = scope.all_callees();
        let mut hot_callees: Vec<u64> = callees.into_iter()
            .filter(|&rip| profile.get_block_count(rip) >= self.config.region_hotness_threshold)
            .take(self.config.max_region_functions)
            .collect();
        
        if !hot_callees.is_empty() {
            scope.expand_to_region(&hot_callees);
            Some(scope)
        } else {
            None
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get VRegs read by an operation
fn get_read_vregs(op: &IrOp) -> Vec<VReg> {
    match op {
        IrOp::Add(a, b) | IrOp::Sub(a, b) | IrOp::Mul(a, b) | IrOp::IMul(a, b) |
        IrOp::Div(a, b) | IrOp::IDiv(a, b) | IrOp::And(a, b) | IrOp::Or(a, b) |
        IrOp::Xor(a, b) | IrOp::Shl(a, b) | IrOp::Shr(a, b) | IrOp::Sar(a, b) |
        IrOp::Rol(a, b) | IrOp::Ror(a, b) | IrOp::Cmp(a, b) | IrOp::Test(a, b) => {
            vec![*a, *b]
        }
        IrOp::Neg(a) | IrOp::Not(a) | IrOp::Load8(a) | IrOp::Load16(a) |
        IrOp::Load32(a) | IrOp::Load64(a) | IrOp::GetCF(a) | IrOp::GetZF(a) |
        IrOp::GetSF(a) | IrOp::GetOF(a) | IrOp::GetPF(a) | IrOp::Sext8(a) |
        IrOp::Sext16(a) | IrOp::Sext32(a) | IrOp::Zext8(a) | IrOp::Zext16(a) |
        IrOp::Zext32(a) | IrOp::Trunc8(a) | IrOp::Trunc16(a) | IrOp::Trunc32(a) |
        IrOp::In8(a) | IrOp::In16(a) | IrOp::In32(a) | IrOp::CallIndirect(a) |
        IrOp::Popcnt(a) | IrOp::Lzcnt(a) | IrOp::Tzcnt(a) | IrOp::Bsf(a) | IrOp::Bsr(a) => {
            vec![*a]
        }
        IrOp::Store8(a, v) | IrOp::Store16(a, v) | IrOp::Store32(a, v) | IrOp::Store64(a, v) |
        IrOp::Out8(a, v) | IrOp::Out16(a, v) | IrOp::Out32(a, v) |
        IrOp::Aesenc(a, v) | IrOp::Aesdec(a, v) | IrOp::Pdep(a, v) | IrOp::Pext(a, v) => {
            vec![*a, *v]
        }
        IrOp::StoreGpr(_, v) | IrOp::StoreFlags(v) | IrOp::StoreRip(v) => {
            vec![*v]
        }
        IrOp::Select(c, t, f) | IrOp::Bextr(c, t, f) | IrOp::Fma(c, t, f) => {
            vec![*c, *t, *f]
        }
        IrOp::Branch(c, _, _) => {
            vec![*c]
        }
        IrOp::Phi(sources) => {
            sources.iter().map(|(_, v)| *v).collect()
        }
        IrOp::Pclmul(a, b, _) => {
            vec![*a, *b]
        }
        IrOp::VectorOp { src1, src2, .. } => {
            vec![*src1, *src2]
        }
        _ => vec![],
    }
}

/// Estimate latency for an operation
fn estimate_latency(op: &IrOp) -> u8 {
    match op {
        // Simple ALU: 1 cycle
        IrOp::Add(_, _) | IrOp::Sub(_, _) | IrOp::And(_, _) | IrOp::Or(_, _) |
        IrOp::Xor(_, _) | IrOp::Not(_) | IrOp::Neg(_) | IrOp::Const(_) => 1,
        
        // Shifts: 1 cycle
        IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) => 1,
        
        // Multiply: 3-4 cycles
        IrOp::Mul(_, _) | IrOp::IMul(_, _) => 3,
        
        // Division: 20-80 cycles
        IrOp::Div(_, _) | IrOp::IDiv(_, _) => 25,
        
        // Memory: 4+ cycles
        IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_) => 4,
        IrOp::Store8(_, _) | IrOp::Store16(_, _) | IrOp::Store32(_, _) | IrOp::Store64(_, _) => 4,
        
        // Bit manipulation
        IrOp::Popcnt(_) | IrOp::Lzcnt(_) | IrOp::Tzcnt(_) => 3,
        IrOp::Bsf(_) | IrOp::Bsr(_) => 3,
        IrOp::Bextr(_, _, _) | IrOp::Pdep(_, _) | IrOp::Pext(_, _) => 3,
        
        // FMA
        IrOp::Fma(_, _, _) => 4,
        
        // AES
        IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => 4,
        
        // Call/syscall
        IrOp::Call(_) | IrOp::CallIndirect(_) | IrOp::Syscall => 100,
        
        // Default
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ir::{IrBasicBlock, IrBuilder};
    
    #[test]
    fn test_scope_levels() {
        assert!(ScopeLevel::Function > ScopeLevel::Block);
        assert!(ScopeLevel::Region > ScopeLevel::Function);
        assert!(ScopeLevel::CallGraph > ScopeLevel::Region);
        
        assert!(ScopeLevel::Block.supports_gvn() == false);
        assert!(ScopeLevel::Function.supports_gvn() == true);
        assert!(ScopeLevel::Region.supports_devirt() == true);
    }
    
    #[test]
    fn test_dependency_graph_simple() {
        // Create a simple IR block
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
        
        ir.blocks.push(bb);
        
        let graph = DependencyGraph::build(&ir);
        
        // Check that v2 depends on v0 and v1
        assert!(graph.nodes[2].predecessors.len() == 2);
        assert!(graph.nodes[2].predecessors.iter().all(|e| e.kind == DependencyKind::RAW));
        
        // v0 and v1 are independent
        let pairs = graph.reorderable_pairs();
        assert!(pairs.contains(&(0, 1)));
    }
    
    #[test]
    fn test_scope_profile_serialize() {
        let mut profile = ScopeProfile::new(0x1000, ScopeLevel::Function);
        profile.record_inline(0x1000, 0x2000, true);
        profile.record_unroll(0x1010, 4);
        profile.record_devirt(0x1020, 0x3000, 0.95);
        profile.record_constant(0x1030, 0, 42);
        
        let data = profile.serialize();
        let restored = ScopeProfile::deserialize(&data).unwrap();
        
        assert_eq!(restored.scope_id, 0x1000);
        assert_eq!(restored.level, ScopeLevel::Function);
        assert_eq!(restored.should_inline(0x1000, 0x2000), Some(true));
        assert_eq!(restored.get_unroll_factor(0x1010), Some(4));
        assert_eq!(restored.devirt_targets.get(&0x1020).map(|(t, _)| *t), Some(0x3000));
        assert_eq!(restored.discovered_constants.get(&(0x1030, 0)), Some(&42));
    }
    
    #[test]
    fn test_compilation_scope() {
        let mut scope = CompilationScope::single_block(0x1000);
        assert_eq!(scope.level, ScopeLevel::Block);
        
        scope.add_call_edge(0x1000, 0x2000);
        scope.add_call_edge(0x1000, 0x3000);
        
        let callees = scope.all_callees();
        assert!(callees.contains(&0x2000));
        assert!(callees.contains(&0x3000));
        
        scope.expand_to_region(&[0x2000, 0x3000]);
        assert_eq!(scope.level, ScopeLevel::Region);
        assert!(scope.contains_block(0x2000));
    }
}
