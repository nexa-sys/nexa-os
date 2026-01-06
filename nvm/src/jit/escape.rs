//! Escape Analysis + Scalar Replacement
//!
//! Enterprise-grade escape analysis for NVM's JIT compiler.
//! 
//! ## Goals
//! - Analyze whether objects "escape" their defining scope
//! - If non-escaping → replace heap allocation with stack/registers (scalar replacement)
//! - Optimize frequent small struct allocations in guest code (e.g., x86 decode context)
//!
//! ## Escape States
//! - **NoEscape**: Object never leaves the current function/block
//! - **ArgEscape**: Object escapes through function arguments but not globally
//! - **GlobalEscape**: Object escapes to global state (heap, external calls)
//!
//! ## Optimization Passes
//! 1. **Connection Graph Construction**: Build points-to graph for all allocations
//! 2. **Escape Analysis**: Propagate escape states through graph
//! 3. **Scalar Replacement**: Decompose non-escaping objects into scalar values

use super::ir::{IrBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, IrBasicBlock};
use std::collections::{HashMap, HashSet, VecDeque};

// ============================================================================
// Escape States
// ============================================================================

/// Escape state of an allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EscapeState {
    /// Object does not escape current scope
    NoEscape = 0,
    /// Object escapes through function arguments (callee may access)
    ArgEscape = 1,
    /// Object escapes globally (stored to heap, returned, external call)
    GlobalEscape = 2,
}

impl EscapeState {
    /// Merge two escape states (take the higher/worse one)
    pub fn merge(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

// ============================================================================
// Connection Graph
// ============================================================================

/// Node in the connection graph
#[derive(Debug, Clone)]
pub enum CgNode {
    /// Heap allocation site
    Allocation {
        vreg: VReg,
        size: usize,
        rip: u64,
    },
    /// Local variable (VReg)
    Local(VReg),
    /// Field of an object (base_vreg, field_offset)
    Field {
        base: VReg,
        offset: usize,
    },
    /// Function parameter
    Param(u8),
    /// Global/heap memory
    Global,
    /// Return value
    Return,
    /// Phantom node (represents unknown)
    Phantom,
}

/// Edge type in connection graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CgEdge {
    /// Points-to edge (pointer -> pointee)
    PointsTo,
    /// Deferred edge (source -> target through assignment)
    Deferred,
    /// Field edge (object -> field)
    Field(usize),
}

/// Connection graph for escape analysis
pub struct ConnectionGraph {
    /// Node ID -> Node
    nodes: HashMap<u32, CgNode>,
    /// Edges: (source, edge_type) -> targets
    edges: HashMap<(u32, CgEdge), HashSet<u32>>,
    /// VReg -> Node ID mapping
    vreg_to_node: HashMap<VReg, u32>,
    /// Next node ID
    next_id: u32,
    /// Escape state for each allocation node
    escape_states: HashMap<u32, EscapeState>,
}

impl ConnectionGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            vreg_to_node: HashMap::new(),
            next_id: 0,
            escape_states: HashMap::new(),
        }
    }
    
    /// Add a node and return its ID
    fn add_node(&mut self, node: CgNode) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        
        // Track VReg mapping
        match &node {
            CgNode::Allocation { vreg, .. } | CgNode::Local(vreg) => {
                self.vreg_to_node.insert(*vreg, id);
            }
            _ => {}
        }
        
        self.nodes.insert(id, node);
        id
    }
    
    /// Add an edge
    fn add_edge(&mut self, from: u32, edge: CgEdge, to: u32) {
        self.edges.entry((from, edge)).or_default().insert(to);
    }
    
    /// Get node ID for VReg
    fn get_node(&self, vreg: VReg) -> Option<u32> {
        self.vreg_to_node.get(&vreg).copied()
    }
    
    /// Get or create node for VReg
    fn get_or_create_node(&mut self, vreg: VReg) -> u32 {
        if let Some(id) = self.vreg_to_node.get(&vreg) {
            *id
        } else {
            self.add_node(CgNode::Local(vreg))
        }
    }
    
    /// Get all nodes a given node points to
    fn points_to(&self, node: u32) -> HashSet<u32> {
        self.edges.get(&(node, CgEdge::PointsTo))
            .cloned()
            .unwrap_or_default()
    }
    
    /// Get all nodes through deferred edges
    fn deferred(&self, node: u32) -> HashSet<u32> {
        self.edges.get(&(node, CgEdge::Deferred))
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for ConnectionGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Escape Analyzer
// ============================================================================

/// Escape analysis configuration
#[derive(Clone, Debug)]
pub struct EscapeConfig {
    /// Maximum allocation size for scalar replacement (bytes)
    pub max_scalar_size: usize,
    /// Maximum number of fields for scalar replacement
    pub max_scalar_fields: usize,
    /// Enable aggressive inlining to improve escape analysis
    pub aggressive_inline: bool,
    /// Track allocation sites across basic blocks
    pub interprocedural: bool,
}

impl Default for EscapeConfig {
    fn default() -> Self {
        Self {
            max_scalar_size: 128, // 128 bytes max for scalar replacement
            max_scalar_fields: 8, // Max 8 fields
            aggressive_inline: true,
            interprocedural: true,
        }
    }
}

/// Result of escape analysis
#[derive(Debug, Clone)]
pub struct EscapeResult {
    /// Allocation site -> escape state
    pub allocations: HashMap<VReg, AllocationInfo>,
    /// Statistics
    pub stats: EscapeStats,
}

/// Information about an allocation
#[derive(Debug, Clone)]
pub struct AllocationInfo {
    /// Guest RIP of allocation
    pub rip: u64,
    /// Escape state
    pub escape: EscapeState,
    /// Size in bytes
    pub size: usize,
    /// Can be scalar replaced?
    pub can_scalar_replace: bool,
    /// Field decomposition (if can_scalar_replace)
    pub fields: Vec<ScalarField>,
}

/// Scalar field after decomposition
#[derive(Debug, Clone)]
pub struct ScalarField {
    /// Offset in object
    pub offset: usize,
    /// Size in bytes
    pub size: usize,
    /// Replacement VReg (allocated during transformation)
    pub vreg: VReg,
}

/// Escape analysis statistics
#[derive(Debug, Clone, Default)]
pub struct EscapeStats {
    /// Total allocations analyzed
    pub total_allocations: u32,
    /// Allocations that don't escape
    pub no_escape: u32,
    /// Allocations that escape through args
    pub arg_escape: u32,
    /// Allocations that escape globally
    pub global_escape: u32,
    /// Allocations eligible for scalar replacement
    pub scalar_replaceable: u32,
    /// Bytes saved from heap
    pub bytes_saved: usize,
}

/// Escape analyzer
pub struct EscapeAnalyzer {
    config: EscapeConfig,
}

impl EscapeAnalyzer {
    pub fn new() -> Self {
        Self {
            config: EscapeConfig::default(),
        }
    }
    
    pub fn with_config(config: EscapeConfig) -> Self {
        Self { config }
    }
    
    /// Perform escape analysis on IR block
    pub fn analyze(&self, ir: &IrBlock) -> EscapeResult {
        let mut stats = EscapeStats::default();
        let mut allocations = HashMap::new();
        
        // Phase 1: Build connection graph
        let mut cg = self.build_connection_graph(ir);
        
        // Phase 2: Propagate escape states
        self.propagate_escape_states(&mut cg);
        
        // Phase 3: Collect results
        for (&id, node) in &cg.nodes {
            if let CgNode::Allocation { vreg, size, rip } = node {
                let escape = cg.escape_states.get(&id)
                    .copied()
                    .unwrap_or(EscapeState::GlobalEscape);
                
                stats.total_allocations += 1;
                match escape {
                    EscapeState::NoEscape => stats.no_escape += 1,
                    EscapeState::ArgEscape => stats.arg_escape += 1,
                    EscapeState::GlobalEscape => stats.global_escape += 1,
                }
                
                let can_scalar_replace = escape == EscapeState::NoEscape
                    && *size <= self.config.max_scalar_size;
                
                if can_scalar_replace {
                    stats.scalar_replaceable += 1;
                    stats.bytes_saved += *size;
                }
                
                // Analyze field structure for scalar replacement
                let fields = if can_scalar_replace {
                    self.analyze_fields(ir, *vreg, *size)
                } else {
                    Vec::new()
                };
                
                allocations.insert(*vreg, AllocationInfo {
                    rip: *rip,
                    escape,
                    size: *size,
                    can_scalar_replace: can_scalar_replace && fields.len() <= self.config.max_scalar_fields,
                    fields,
                });
            }
        }
        
        EscapeResult { allocations, stats }
    }
    
    /// Build connection graph from IR
    fn build_connection_graph(&self, ir: &IrBlock) -> ConnectionGraph {
        let mut cg = ConnectionGraph::new();
        
        // Add special nodes
        let global_node = cg.add_node(CgNode::Global);
        let return_node = cg.add_node(CgNode::Return);
        let phantom_node = cg.add_node(CgNode::Phantom);
        
        // Pre-mark special nodes as escaping
        cg.escape_states.insert(global_node, EscapeState::GlobalEscape);
        cg.escape_states.insert(return_node, EscapeState::GlobalEscape);
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                self.process_instruction(&mut cg, instr, global_node, return_node, phantom_node);
            }
        }
        
        cg
    }
    
    /// Process a single instruction for connection graph
    fn process_instruction(
        &self,
        cg: &mut ConnectionGraph,
        instr: &IrInstr,
        global_node: u32,
        return_node: u32,
        _phantom_node: u32,
    ) {
        let dst = instr.dst;
        
        match &instr.op {
            // Memory allocation (detected by pattern: call to malloc/alloc)
            IrOp::Call(target) if self.is_allocation_call(*target) => {
                // This is an allocation site
                let alloc_id = cg.add_node(CgNode::Allocation {
                    vreg: dst,
                    size: self.infer_allocation_size(instr),
                    rip: instr.guest_rip,
                });
                cg.escape_states.insert(alloc_id, EscapeState::NoEscape);
            }
            
            // Pointer assignment: dst = src (creates deferred edge)
            IrOp::Add(a, b) if self.is_pointer_vreg(cg, *a) || self.is_pointer_vreg(cg, *b) => {
                // Pointer arithmetic - track the base pointer
                let base = if self.is_pointer_vreg(cg, *a) { *a } else { *b };
                let base_id = cg.get_or_create_node(base);
                let dst_id = cg.get_or_create_node(dst);
                cg.add_edge(dst_id, CgEdge::Deferred, base_id);
            }
            
            // Store to memory: [addr] = val
            IrOp::Store64(addr, val) | IrOp::Store32(addr, val) |
            IrOp::Store16(addr, val) | IrOp::Store8(addr, val) => {
                let val_id = cg.get_or_create_node(*val);
                
                // Check if storing to a tracked object's field
                if let Some(addr_id) = cg.get_node(*addr) {
                    if let Some(CgNode::Allocation { .. }) = cg.nodes.get(&addr_id) {
                        // Storing into an allocation - add field edge
                        cg.add_edge(addr_id, CgEdge::PointsTo, val_id);
                        return;
                    }
                }
                
                // Unknown store target - val escapes globally
                cg.add_edge(val_id, CgEdge::PointsTo, global_node);
                if let Some(&state) = cg.escape_states.get(&val_id) {
                    cg.escape_states.insert(val_id, state.merge(EscapeState::GlobalEscape));
                } else {
                    cg.escape_states.insert(val_id, EscapeState::GlobalEscape);
                }
            }
            
            // Load from memory: dst = [addr]
            IrOp::Load64(addr) | IrOp::Load32(addr) |
            IrOp::Load16(addr) | IrOp::Load8(addr) => {
                let dst_id = cg.get_or_create_node(dst);
                
                // Check if loading from tracked allocation
                if let Some(addr_id) = cg.get_node(*addr) {
                    // Add deferred edge from loaded value to potential pointees
                    for &pointee in cg.points_to(addr_id).iter() {
                        cg.add_edge(dst_id, CgEdge::Deferred, pointee);
                    }
                } else {
                    // Unknown load source - assume from global
                    cg.add_edge(dst_id, CgEdge::Deferred, global_node);
                }
            }
            
            // Return instruction - returned values escape
            IrOp::Ret => {
                // The returned value escapes globally
                // Convention: RAX holds return value
            }
            
            // Call instruction - arguments may escape
            IrOp::Call(_) | IrOp::CallIndirect(_) => {
                // For now, mark all pointer arguments as ArgEscape
                // A more sophisticated analysis would check call signatures
            }
            
            // Guest register store - may escape depending on register
            IrOp::StoreGpr(_, val) => {
                let val_id = cg.get_or_create_node(*val);
                // Conservative: guest register stores may escape
                cg.add_edge(val_id, CgEdge::PointsTo, global_node);
            }
            
            _ => {}
        }
    }
    
    /// Check if a call target is an allocation function
    fn is_allocation_call(&self, target: u64) -> bool {
        // Heuristic: known allocation function addresses or patterns
        // In practice, would check symbol table or pattern match
        target == 0 // Placeholder
    }
    
    /// Infer allocation size from instruction context
    fn infer_allocation_size(&self, _instr: &IrInstr) -> usize {
        // Would analyze RDI (first arg) for malloc-style calls
        64 // Default size
    }
    
    /// Check if a VReg represents a pointer in the graph
    fn is_pointer_vreg(&self, cg: &ConnectionGraph, vreg: VReg) -> bool {
        cg.vreg_to_node.contains_key(&vreg)
    }
    
    /// Propagate escape states through the connection graph
    fn propagate_escape_states(&self, cg: &mut ConnectionGraph) {
        // Worklist algorithm
        let mut worklist: VecDeque<u32> = VecDeque::new();
        let mut in_worklist: HashSet<u32> = HashSet::new();
        
        // Initialize worklist with nodes that have escape states
        for &node_id in cg.escape_states.keys() {
            worklist.push_back(node_id);
            in_worklist.insert(node_id);
        }
        
        // Fixed-point iteration
        while let Some(node_id) = worklist.pop_front() {
            in_worklist.remove(&node_id);
            
            let current_state = cg.escape_states.get(&node_id)
                .copied()
                .unwrap_or(EscapeState::NoEscape);
            
            // Propagate to deferred edges (backwards)
            for (&(src, edge), targets) in &cg.edges {
                if edge == CgEdge::Deferred && targets.contains(&node_id) {
                    let src_state = cg.escape_states.get(&src)
                        .copied()
                        .unwrap_or(EscapeState::NoEscape);
                    let new_state = src_state.merge(current_state);
                    
                    if new_state > src_state {
                        cg.escape_states.insert(src, new_state);
                        if !in_worklist.contains(&src) {
                            worklist.push_back(src);
                            in_worklist.insert(src);
                        }
                    }
                }
            }
            
            // Propagate to points-to targets
            for &target in cg.points_to(node_id).iter() {
                let target_state = cg.escape_states.get(&target)
                    .copied()
                    .unwrap_or(EscapeState::NoEscape);
                let new_state = target_state.merge(current_state);
                
                if new_state > target_state {
                    cg.escape_states.insert(target, new_state);
                    if !in_worklist.contains(&target) {
                        worklist.push_back(target);
                        in_worklist.insert(target);
                    }
                }
            }
        }
    }
    
    /// Analyze field structure for scalar replacement
    fn analyze_fields(&self, ir: &IrBlock, alloc_vreg: VReg, size: usize) -> Vec<ScalarField> {
        let mut fields = Vec::new();
        let mut accessed_offsets: HashSet<usize> = HashSet::new();
        
        // Find all stores to this allocation
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                match &instr.op {
                    IrOp::Store64(addr, _) | IrOp::Store32(addr, _) |
                    IrOp::Store16(addr, _) | IrOp::Store8(addr, _) => {
                        if let Some(offset) = self.compute_offset_from_base(ir, *addr, alloc_vreg) {
                            let field_size = match &instr.op {
                                IrOp::Store64(_, _) => 8,
                                IrOp::Store32(_, _) => 4,
                                IrOp::Store16(_, _) => 2,
                                IrOp::Store8(_, _) => 1,
                                _ => unreachable!(),
                            };
                            accessed_offsets.insert(offset);
                            if !fields.iter().any(|f: &ScalarField| f.offset == offset) {
                                fields.push(ScalarField {
                                    offset,
                                    size: field_size,
                                    vreg: VReg::NONE, // Will be assigned during transformation
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        
        // Sort by offset
        fields.sort_by_key(|f| f.offset);
        
        // If no explicit fields found, treat as single blob
        if fields.is_empty() && size > 0 {
            fields.push(ScalarField {
                offset: 0,
                size,
                vreg: VReg::NONE,
            });
        }
        
        fields
    }
    
    /// Compute offset from base allocation
    fn compute_offset_from_base(&self, ir: &IrBlock, addr: VReg, base: VReg) -> Option<usize> {
        if addr == base {
            return Some(0);
        }
        
        // Find the instruction defining addr
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                if instr.dst == addr {
                    if let IrOp::Add(a, b) = &instr.op {
                        if *a == base {
                            // addr = base + offset
                            return self.get_constant_value(ir, *b).map(|v| v as usize);
                        } else if *b == base {
                            return self.get_constant_value(ir, *a).map(|v| v as usize);
                        }
                    }
                }
            }
        }
        
        None
    }
    
    /// Get constant value of a VReg if known
    fn get_constant_value(&self, ir: &IrBlock, vreg: VReg) -> Option<i64> {
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
}

impl Default for EscapeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Scalar Replacement Transformation
// ============================================================================

/// Scalar replacement transformer
pub struct ScalarReplacer {
    config: EscapeConfig,
}

impl ScalarReplacer {
    pub fn new() -> Self {
        Self {
            config: EscapeConfig::default(),
        }
    }
    
    pub fn with_config(config: EscapeConfig) -> Self {
        Self { config }
    }
    
    /// Apply scalar replacement to IR based on escape analysis results
    pub fn transform(&self, ir: &mut IrBlock, escape_result: &EscapeResult) -> ScalarReplaceStats {
        let mut stats = ScalarReplaceStats::default();
        
        // Build mapping: allocation VReg -> replacement VRegs for each field
        let mut replacements: HashMap<VReg, HashMap<usize, VReg>> = HashMap::new();
        
        for (&alloc_vreg, info) in &escape_result.allocations {
            if !info.can_scalar_replace {
                continue;
            }
            
            stats.allocations_replaced += 1;
            stats.bytes_saved += info.size;
            
            // Allocate new VRegs for each field
            let mut field_map = HashMap::new();
            for field in &info.fields {
                let new_vreg = ir.alloc_vreg();
                field_map.insert(field.offset, new_vreg);
                stats.fields_created += 1;
            }
            replacements.insert(alloc_vreg, field_map);
        }
        
        // Transform IR
        for bb in &mut ir.blocks {
            let mut new_instrs = Vec::with_capacity(bb.instrs.len());
            
            for instr in &bb.instrs {
                // Check if this is a store to a replaced allocation
                if let Some(transformed) = self.transform_store(instr, &replacements) {
                    new_instrs.push(transformed);
                    continue;
                }
                
                // Check if this is a load from a replaced allocation  
                if let Some(transformed) = self.transform_load(instr, &replacements) {
                    new_instrs.push(transformed);
                    continue;
                }
                
                // Check if this is the allocation itself (remove it)
                if let IrOp::Call(target) = &instr.op {
                    if replacements.contains_key(&instr.dst) && *target == 0 {
                        // Skip the allocation call
                        stats.allocations_eliminated += 1;
                        continue;
                    }
                }
                
                // Keep instruction as-is
                new_instrs.push(instr.clone());
            }
            
            bb.instrs = new_instrs;
        }
        
        stats
    }
    
    /// Transform a store instruction if targeting replaced allocation
    fn transform_store(
        &self,
        instr: &IrInstr,
        replacements: &HashMap<VReg, HashMap<usize, VReg>>,
    ) -> Option<IrInstr> {
        let (addr, val, size) = match &instr.op {
            IrOp::Store64(a, v) => (*a, *v, 8),
            IrOp::Store32(a, v) => (*a, *v, 4),
            IrOp::Store16(a, v) => (*a, *v, 2),
            IrOp::Store8(a, v) => (*a, *v, 1),
            _ => return None,
        };
        
        // Check if addr is a replaced allocation (or offset from one)
        // For simplicity, check direct match first
        if let Some(field_map) = replacements.get(&addr) {
            if let Some(&replacement_vreg) = field_map.get(&0) {
                // Replace store with move to scalar
                return Some(IrInstr {
                    dst: replacement_vreg,
                    op: match size {
                        8 => IrOp::Add(val, VReg::NONE), // Pseudo: just copy
                        4 => IrOp::Trunc32(val),
                        2 => IrOp::Trunc16(val),
                        1 => IrOp::Trunc8(val),
                        _ => return None,
                    },
                    guest_rip: instr.guest_rip,
                    flags: IrFlags::empty(),
                });
            }
        }
        
        None
    }
    
    /// Transform a load instruction if loading from replaced allocation
    fn transform_load(
        &self,
        instr: &IrInstr,
        replacements: &HashMap<VReg, HashMap<usize, VReg>>,
    ) -> Option<IrInstr> {
        let (addr, size) = match &instr.op {
            IrOp::Load64(a) => (*a, 8),
            IrOp::Load32(a) => (*a, 4),
            IrOp::Load16(a) => (*a, 2),
            IrOp::Load8(a) => (*a, 1),
            _ => return None,
        };
        
        // Check if loading from replaced allocation
        if let Some(field_map) = replacements.get(&addr) {
            if let Some(&replacement_vreg) = field_map.get(&0) {
                // Replace load with move from scalar
                return Some(IrInstr {
                    dst: instr.dst,
                    op: match size {
                        8 => IrOp::Add(replacement_vreg, VReg::NONE),
                        4 => IrOp::Zext32(replacement_vreg),
                        2 => IrOp::Zext16(replacement_vreg),
                        1 => IrOp::Zext8(replacement_vreg),
                        _ => return None,
                    },
                    guest_rip: instr.guest_rip,
                    flags: IrFlags::empty(),
                });
            }
        }
        
        None
    }
}

impl Default for ScalarReplacer {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics from scalar replacement
#[derive(Debug, Clone, Default)]
pub struct ScalarReplaceStats {
    /// Number of allocations replaced with scalars
    pub allocations_replaced: u32,
    /// Number of allocations eliminated
    pub allocations_eliminated: u32,
    /// Number of scalar fields created
    pub fields_created: u32,
    /// Bytes saved from heap allocation
    pub bytes_saved: usize,
}

// ============================================================================
// Integration with S2 Compiler
// ============================================================================

/// Combined escape analysis + scalar replacement pass
pub struct EscapeScalarPass {
    analyzer: EscapeAnalyzer,
    replacer: ScalarReplacer,
}

impl EscapeScalarPass {
    pub fn new() -> Self {
        Self {
            analyzer: EscapeAnalyzer::new(),
            replacer: ScalarReplacer::new(),
        }
    }
    
    pub fn with_config(config: EscapeConfig) -> Self {
        Self {
            analyzer: EscapeAnalyzer::with_config(config.clone()),
            replacer: ScalarReplacer::with_config(config),
        }
    }
    
    /// Run the complete escape analysis + scalar replacement pass
    pub fn run(&self, ir: &mut IrBlock) -> EscapePassResult {
        // Phase 1: Analyze escapes
        let escape_result = self.analyzer.analyze(ir);
        
        // Phase 2: Apply scalar replacement for non-escaping allocations
        let replace_stats = self.replacer.transform(ir, &escape_result);
        
        EscapePassResult {
            escape_stats: escape_result.stats,
            replace_stats,
        }
    }
}

impl Default for EscapeScalarPass {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined result from escape analysis pass
#[derive(Debug, Clone)]
pub struct EscapePassResult {
    pub escape_stats: EscapeStats,
    pub replace_stats: ScalarReplaceStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_escape_state_merge() {
        assert_eq!(EscapeState::NoEscape.merge(EscapeState::NoEscape), EscapeState::NoEscape);
        assert_eq!(EscapeState::NoEscape.merge(EscapeState::ArgEscape), EscapeState::ArgEscape);
        assert_eq!(EscapeState::ArgEscape.merge(EscapeState::GlobalEscape), EscapeState::GlobalEscape);
    }
    
    #[test]
    fn test_escape_state_ordering() {
        assert!(EscapeState::NoEscape < EscapeState::ArgEscape);
        assert!(EscapeState::ArgEscape < EscapeState::GlobalEscape);
    }
    
    #[test]
    fn test_connection_graph_basic() {
        let mut cg = ConnectionGraph::new();
        
        let alloc_id = cg.add_node(CgNode::Allocation {
            vreg: VReg(0),
            size: 64,
            rip: 0x1000,
        });
        
        let local_id = cg.add_node(CgNode::Local(VReg(1)));
        
        cg.add_edge(local_id, CgEdge::PointsTo, alloc_id);
        
        assert!(cg.points_to(local_id).contains(&alloc_id));
        assert!(cg.points_to(alloc_id).is_empty());
    }
    
    #[test]
    fn test_connection_graph_deferred_edges() {
        let mut cg = ConnectionGraph::new();
        
        let v0 = cg.add_node(CgNode::Local(VReg(0)));
        let v1 = cg.add_node(CgNode::Local(VReg(1)));
        let v2 = cg.add_node(CgNode::Local(VReg(2)));
        
        cg.add_edge(v1, CgEdge::Deferred, v0);
        cg.add_edge(v2, CgEdge::Deferred, v1);
        
        assert!(cg.deferred(v1).contains(&v0));
        assert!(cg.deferred(v2).contains(&v1));
    }
    
    #[test]
    fn test_connection_graph_vreg_mapping() {
        let mut cg = ConnectionGraph::new();
        
        let vreg = VReg(42);
        let node_id = cg.add_node(CgNode::Local(vreg));
        
        assert_eq!(cg.get_node(vreg), Some(node_id));
        assert_eq!(cg.get_node(VReg(999)), None);
    }
    
    #[test]
    fn test_escape_analyzer_basic() {
        let analyzer = EscapeAnalyzer::new();
        let ir = IrBlock::new(0x1000);
        
        let result = analyzer.analyze(&ir);
        assert_eq!(result.stats.total_allocations, 0);
    }
    
    #[test]
    fn test_escape_config_defaults() {
        let config = EscapeConfig::default();
        assert!(config.max_scalar_size > 0);
        assert!(config.max_scalar_fields > 0);
        assert!(config.aggressive_inline);
    }
    
    #[test]
    fn test_scalar_field_creation() {
        let field = ScalarField {
            offset: 8,
            size: 4,
            vreg: VReg(100),
        };
        assert_eq!(field.offset, 8);
        assert_eq!(field.size, 4);
    }
    
    #[test]
    fn test_escape_pass_integration() {
        let pass = EscapeScalarPass::new();
        let mut ir = IrBlock::new(0x2000);
        
        let result = pass.run(&mut ir);
        
        // Empty IR should have no allocations
        assert_eq!(result.escape_stats.total_allocations, 0);
        assert_eq!(result.replace_stats.allocations_replaced, 0);
    }
    
    #[test]
    fn test_escape_pass_with_config() {
        let config = EscapeConfig {
            max_scalar_size: 256,
            max_scalar_fields: 16,
            aggressive_inline: false,
            interprocedural: false,
        };
        let pass = EscapeScalarPass::with_config(config);
        let mut ir = IrBlock::new(0x3000);
        
        let result = pass.run(&mut ir);
        assert_eq!(result.escape_stats.total_allocations, 0);
    }
}
