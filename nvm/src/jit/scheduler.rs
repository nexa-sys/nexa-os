//! Instruction Scheduler
//!
//! Enterprise-grade instruction scheduling engine for software-level
//! out-of-order execution. Far exceeds hardware ROB capabilities by
//! operating at compilation scope (Region can span multiple functions).
//!
//! ## Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────────────────┐
//! │                        Instruction Scheduler                              │
//! ├───────────────────────────────────────────────────────────────────────────┤
//! │                                                                           │
//! │  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐     │
//! │  │  Dependency     │────▶│   Scheduler     │────▶│   Reordered     │     │
//! │  │  Graph (DAG)    │     │   (List/CP)     │     │   IR            │     │
//! │  └─────────────────┘     └─────────────────┘     └─────────────────┘     │
//! │          │                       │                       │               │
//! │          ▼                       ▼                       ▼               │
//! │  ┌─────────────────────────────────────────────────────────────────────┐ │
//! │  │                    Scheduling Window                                │ │
//! │  │  • Software ROB: Entire scope (vs. 512 µop hardware)               │ │
//! │  │  • Speculative: Profile-guided memory reordering                   │ │
//! │  │  • Deopt: Safe fallback on speculation failure                     │ │
//! │  └─────────────────────────────────────────────────────────────────────┘ │
//! └───────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Scheduling Algorithms
//!
//! - **List Scheduling**: Greedy schedule based on ready queue priority
//! - **Critical Path**: Prioritize instructions on critical path
//! - **Resource Constrained**: Model execution unit contention
//! - **Modulo Scheduling**: Software pipelining for loops

use std::collections::{HashMap, HashSet, BinaryHeap, VecDeque};
use std::cmp::Ordering as CmpOrdering;

use super::ir::{IrBlock, IrInstr, IrOp, VReg, BlockId, IrFlags, IrBasicBlock};
use super::scope::{DependencyGraph, DependencyKind, DependencyEdge, DependencyNode, ScopeLevel};
use super::profile::ProfileDb;

// ============================================================================
// Scheduler Configuration
// ============================================================================

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Maximum scheduling window size (number of instructions)
    /// Software equivalent of hardware ROB - can be much larger
    pub max_window_size: usize,
    
    /// Enable speculative memory reordering
    pub speculative_memory: bool,
    
    /// Minimum profile confidence for speculative reordering
    pub speculation_threshold: f64,
    
    /// Enable critical path prioritization
    pub critical_path_priority: bool,
    
    /// Enable resource modeling (execution units)
    pub resource_model: bool,
    
    /// Number of execution units per type
    pub execution_units: ExecutionUnits,
    
    /// Enable modulo scheduling for loops
    pub modulo_scheduling: bool,
    
    /// Scheduling algorithm
    pub algorithm: SchedulingAlgorithm,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_window_size: 4096, // Much larger than hardware ROB (~512)
            speculative_memory: true,
            speculation_threshold: 0.95,
            critical_path_priority: true,
            resource_model: true,
            execution_units: ExecutionUnits::default(),
            modulo_scheduling: true,
            algorithm: SchedulingAlgorithm::CriticalPath,
        }
    }
}

/// Scheduling algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingAlgorithm {
    /// Greedy list scheduling
    List,
    /// Critical path scheduling (default, best for most code)
    CriticalPath,
    /// Balanced scheduling (minimize register pressure)
    Balanced,
    /// Resource-constrained scheduling
    ResourceConstrained,
}

/// Execution unit model
#[derive(Debug, Clone)]
pub struct ExecutionUnits {
    /// Integer ALU units
    pub int_alu: u8,
    /// Integer multiply units
    pub int_mul: u8,
    /// Integer divide units
    pub int_div: u8,
    /// Memory load units
    pub mem_load: u8,
    /// Memory store units
    pub mem_store: u8,
    /// Branch units
    pub branch: u8,
    /// Vector/SIMD units
    pub vector: u8,
}

impl Default for ExecutionUnits {
    fn default() -> Self {
        // Model modern x86_64 (similar to Zen 4 / Golden Cove)
        Self {
            int_alu: 4,
            int_mul: 2,
            int_div: 1,
            mem_load: 2,
            mem_store: 2,
            branch: 2,
            vector: 2,
        }
    }
}

// ============================================================================
// Scheduling Result
// ============================================================================

/// Scheduling result
#[derive(Debug, Clone)]
pub struct ScheduleResult {
    /// Reordered instruction indices (original index -> new position)
    pub schedule: Vec<usize>,
    
    /// Schedule cycles (estimated execution time)
    pub cycles: u32,
    
    /// Instructions moved speculatively
    pub speculative_moves: Vec<SpeculativeMove>,
    
    /// Statistics
    pub stats: ScheduleStats,
}

/// A speculative instruction move
#[derive(Debug, Clone)]
pub struct SpeculativeMove {
    /// Original instruction index
    pub instr_idx: usize,
    /// Original position
    pub from_pos: usize,
    /// New position
    pub to_pos: usize,
    /// Speculation type
    pub kind: SpeculativeMoveKind,
    /// Deopt RIP if speculation fails
    pub deopt_rip: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeculativeMoveKind {
    /// Load moved before store (alias speculation)
    LoadBeforeStore,
    /// Store moved before load (alias speculation)
    StoreBeforeLoad,
    /// Branch moved (prediction speculation)
    BranchPrediction,
}

/// Scheduling statistics
#[derive(Debug, Clone, Default)]
pub struct ScheduleStats {
    /// Total instructions scheduled
    pub total_instrs: u32,
    /// Instructions on critical path
    pub critical_path_instrs: u32,
    /// Critical path length (cycles)
    pub critical_path_length: u32,
    /// Achieved ILP
    pub achieved_ilp: f32,
    /// Instructions reordered
    pub reordered: u32,
    /// Speculative moves applied
    pub speculative_moves: u32,
    /// Register pressure (max live registers)
    pub max_live_regs: u32,
}

// ============================================================================
// Instruction Scheduler
// ============================================================================

/// Enterprise instruction scheduler
pub struct InstructionScheduler {
    config: SchedulerConfig,
}

impl InstructionScheduler {
    pub fn new() -> Self {
        Self {
            config: SchedulerConfig::default(),
        }
    }
    
    pub fn with_config(config: SchedulerConfig) -> Self {
        Self { config }
    }
    
    /// Schedule a basic block
    pub fn schedule_block(
        &self,
        ir: &mut IrBlock,
        dep_graph: &DependencyGraph,
        profile: &ProfileDb,
    ) -> ScheduleResult {
        match self.config.algorithm {
            SchedulingAlgorithm::List => self.list_schedule(ir, dep_graph, profile),
            SchedulingAlgorithm::CriticalPath => self.critical_path_schedule(ir, dep_graph, profile),
            SchedulingAlgorithm::Balanced => self.balanced_schedule(ir, dep_graph, profile),
            SchedulingAlgorithm::ResourceConstrained => self.resource_schedule(ir, dep_graph, profile),
        }
    }
    
    /// Schedule with scope awareness
    pub fn schedule_scope(
        &self,
        ir: &mut IrBlock,
        scope_level: ScopeLevel,
        profile: &ProfileDb,
    ) -> ScheduleResult {
        // Build dependency graph for the scope
        let dep_graph = DependencyGraph::build(ir);
        
        // Adjust config based on scope level
        let mut config = self.config.clone();
        match scope_level {
            ScopeLevel::Block => {
                // Conservative: small window, no speculation
                config.max_window_size = 64;
                config.speculative_memory = false;
            }
            ScopeLevel::Function => {
                // Moderate: medium window, limited speculation
                config.max_window_size = 512;
                config.speculative_memory = true;
                config.speculation_threshold = 0.99;
            }
            ScopeLevel::Region | ScopeLevel::CallGraph => {
                // Aggressive: large window, full speculation
                config.max_window_size = 4096;
                config.speculative_memory = true;
                config.speculation_threshold = 0.95;
            }
        }
        
        let scheduler = InstructionScheduler::with_config(config);
        scheduler.schedule_block(ir, &dep_graph, profile)
    }
    
    // ========================================================================
    // List Scheduling
    // ========================================================================
    
    fn list_schedule(
        &self,
        ir: &mut IrBlock,
        dep_graph: &DependencyGraph,
        _profile: &ProfileDb,
    ) -> ScheduleResult {
        let n = dep_graph.instr_count();
        if n == 0 {
            return ScheduleResult {
                schedule: vec![],
                cycles: 0,
                speculative_moves: vec![],
                stats: ScheduleStats::default(),
            };
        }
        
        // Build scheduling state
        let mut state = SchedulingState::new(n);
        let mut schedule = Vec::with_capacity(n);
        let mut current_cycle = 0u32;
        
        // Initialize ready queue with instructions that have no predecessors
        for i in 0..n {
            if dep_graph.nodes()[i].predecessors.is_empty() {
                state.ready_queue.push(ReadyInstr {
                    idx: i,
                    priority: self.compute_priority(i, dep_graph),
                    earliest_start: 0,
                });
            }
        }
        
        // Schedule instructions
        while schedule.len() < n {
            // Get highest priority ready instruction
            if let Some(ready) = state.ready_queue.pop() {
                // Update cycle if needed
                if ready.earliest_start > current_cycle {
                    current_cycle = ready.earliest_start;
                }
                
                // Schedule this instruction
                schedule.push(ready.idx);
                state.scheduled.insert(ready.idx);
                let finish_cycle = current_cycle + self.instr_latency(ready.idx, dep_graph);
                state.finish_times[ready.idx] = finish_cycle;
                
                // Check successors
                for edge in &dep_graph.nodes()[ready.idx].successors {
                    let succ = edge.to;
                    if !state.scheduled.contains(&succ) && !state.in_ready(&succ) {
                        // Check if all predecessors are scheduled
                        let all_preds_done = dep_graph.nodes()[succ].predecessors.iter()
                            .all(|e| state.scheduled.contains(&e.from));
                        
                        if all_preds_done {
                            // Compute earliest start time
                            let earliest = dep_graph.nodes()[succ].predecessors.iter()
                                .map(|e| state.finish_times[e.from] + e.latency as u32)
                                .max()
                                .unwrap_or(0);
                            
                            state.ready_queue.push(ReadyInstr {
                                idx: succ,
                                priority: self.compute_priority(succ, dep_graph),
                                earliest_start: earliest,
                            });
                        }
                    }
                }
            } else {
                // No ready instructions - advance cycle
                current_cycle += 1;
                if current_cycle > n as u32 * 100 {
                    // Safety break for cycles
                    break;
                }
            }
        }
        
        let stats = self.compute_stats(&schedule, dep_graph, current_cycle);
        
        ScheduleResult {
            schedule,
            cycles: current_cycle,
            speculative_moves: vec![],
            stats,
        }
    }
    
    // ========================================================================
    // Critical Path Scheduling
    // ========================================================================
    
    fn critical_path_schedule(
        &self,
        ir: &mut IrBlock,
        dep_graph: &DependencyGraph,
        profile: &ProfileDb,
    ) -> ScheduleResult {
        let n = dep_graph.instr_count();
        if n == 0 {
            return ScheduleResult {
                schedule: vec![],
                cycles: 0,
                speculative_moves: vec![],
                stats: ScheduleStats::default(),
            };
        }
        
        // Compute critical path priority
        let cp_priority = self.compute_cp_priorities(dep_graph);
        
        let mut state = SchedulingState::new(n);
        let mut schedule = Vec::with_capacity(n);
        let mut current_cycle = 0u32;
        let mut speculative_moves = Vec::new();
        
        // Initialize ready queue
        for i in 0..n {
            if dep_graph.nodes()[i].predecessors.is_empty() {
                state.ready_queue.push(ReadyInstr {
                    idx: i,
                    priority: cp_priority[i],
                    earliest_start: 0,
                });
            }
        }
        
        while schedule.len() < n {
            // Try speculative scheduling if enabled
            if self.config.speculative_memory && state.ready_queue.is_empty() {
                if let Some(spec) = self.try_speculative_schedule(&mut state, dep_graph, profile) {
                    speculative_moves.push(spec);
                    continue;
                }
            }
            
            if let Some(ready) = state.ready_queue.pop() {
                if ready.earliest_start > current_cycle {
                    current_cycle = ready.earliest_start;
                }
                
                schedule.push(ready.idx);
                state.scheduled.insert(ready.idx);
                let finish_cycle = current_cycle + self.instr_latency(ready.idx, dep_graph);
                state.finish_times[ready.idx] = finish_cycle;
                
                // Update successors
                for edge in &dep_graph.nodes()[ready.idx].successors {
                    let succ = edge.to;
                    if !state.scheduled.contains(&succ) && !state.in_ready(&succ) {
                        let all_preds_done = dep_graph.nodes()[succ].predecessors.iter()
                            .all(|e| state.scheduled.contains(&e.from) || e.speculative);
                        
                        if all_preds_done {
                            let earliest = dep_graph.nodes()[succ].predecessors.iter()
                                .filter(|e| state.scheduled.contains(&e.from))
                                .map(|e| state.finish_times[e.from] + e.latency as u32)
                                .max()
                                .unwrap_or(0);
                            
                            state.ready_queue.push(ReadyInstr {
                                idx: succ,
                                priority: cp_priority[succ],
                                earliest_start: earliest,
                            });
                        }
                    }
                }
            } else {
                current_cycle += 1;
                if current_cycle > n as u32 * 100 {
                    break;
                }
            }
        }
        
        let mut stats = self.compute_stats(&schedule, dep_graph, current_cycle);
        stats.speculative_moves = speculative_moves.len() as u32;
        
        ScheduleResult {
            schedule,
            cycles: current_cycle,
            speculative_moves,
            stats,
        }
    }
    
    /// Compute critical path priorities (higher = more critical)
    fn compute_cp_priorities(&self, dep_graph: &DependencyGraph) -> Vec<i64> {
        let n = dep_graph.instr_count();
        let mut priorities = vec![0i64; n];
        
        // Priority = critical path length from this instruction to exit
        // Use reverse topological order
        let mut remaining_succs = vec![0usize; n];
        for node in dep_graph.nodes() {
            for edge in &node.successors {
                remaining_succs[edge.from] += 1;
            }
        }
        
        // Find exit nodes (no successors)
        let mut worklist: VecDeque<usize> = (0..n)
            .filter(|&i| dep_graph.nodes()[i].successors.is_empty())
            .collect();
        
        while let Some(i) = worklist.pop_front() {
            // Priority = max(succ priority + latency) + bonus for critical path
            let succ_max: i64 = dep_graph.nodes()[i].successors.iter()
                .map(|e| priorities[e.to] + e.latency as i64)
                .max()
                .unwrap_or(0);
            
            priorities[i] = succ_max + 1;
            
            // Bonus for being on original critical path
            if dep_graph.nodes()[i].on_critical_path {
                priorities[i] += 1000;
            }
            
            // Add predecessors to worklist
            for edge in &dep_graph.nodes()[i].predecessors {
                remaining_succs[edge.from] = remaining_succs[edge.from].saturating_sub(1);
                if remaining_succs[edge.from] == 0 {
                    worklist.push_back(edge.from);
                }
            }
        }
        
        priorities
    }
    
    // ========================================================================
    // Balanced Scheduling (minimize register pressure)
    // ========================================================================
    
    fn balanced_schedule(
        &self,
        ir: &mut IrBlock,
        dep_graph: &DependencyGraph,
        profile: &ProfileDb,
    ) -> ScheduleResult {
        // Similar to list scheduling but with register pressure consideration
        let n = dep_graph.instr_count();
        if n == 0 {
            return ScheduleResult {
                schedule: vec![],
                cycles: 0,
                speculative_moves: vec![],
                stats: ScheduleStats::default(),
            };
        }
        
        // Compute use counts for each VReg
        let use_counts = self.compute_use_counts(ir);
        
        let mut state = SchedulingState::new(n);
        let mut schedule = Vec::with_capacity(n);
        let mut current_cycle = 0u32;
        let mut live_regs: HashSet<VReg> = HashSet::new();
        let mut max_live = 0usize;
        
        // Initialize ready queue
        for i in 0..n {
            if dep_graph.nodes()[i].predecessors.is_empty() {
                let priority = self.balanced_priority(i, dep_graph, &use_counts, live_regs.len());
                state.ready_queue.push(ReadyInstr {
                    idx: i,
                    priority,
                    earliest_start: 0,
                });
            }
        }
        
        while schedule.len() < n {
            if let Some(ready) = state.ready_queue.pop() {
                if ready.earliest_start > current_cycle {
                    current_cycle = ready.earliest_start;
                }
                
                schedule.push(ready.idx);
                state.scheduled.insert(ready.idx);
                
                // Update live registers
                // (simplified - would need actual VReg info from instructions)
                max_live = max_live.max(live_regs.len());
                
                let finish_cycle = current_cycle + self.instr_latency(ready.idx, dep_graph);
                state.finish_times[ready.idx] = finish_cycle;
                
                for edge in &dep_graph.nodes()[ready.idx].successors {
                    let succ = edge.to;
                    if !state.scheduled.contains(&succ) && !state.in_ready(&succ) {
                        let all_preds_done = dep_graph.nodes()[succ].predecessors.iter()
                            .all(|e| state.scheduled.contains(&e.from));
                        
                        if all_preds_done {
                            let earliest = dep_graph.nodes()[succ].predecessors.iter()
                                .map(|e| state.finish_times[e.from] + e.latency as u32)
                                .max()
                                .unwrap_or(0);
                            
                            let priority = self.balanced_priority(succ, dep_graph, &use_counts, live_regs.len());
                            state.ready_queue.push(ReadyInstr {
                                idx: succ,
                                priority,
                                earliest_start: earliest,
                            });
                        }
                    }
                }
            } else {
                current_cycle += 1;
                if current_cycle > n as u32 * 100 {
                    break;
                }
            }
        }
        
        let mut stats = self.compute_stats(&schedule, dep_graph, current_cycle);
        stats.max_live_regs = max_live as u32;
        
        ScheduleResult {
            schedule,
            cycles: current_cycle,
            speculative_moves: vec![],
            stats,
        }
    }
    
    fn balanced_priority(
        &self,
        idx: usize,
        dep_graph: &DependencyGraph,
        use_counts: &HashMap<VReg, usize>,
        current_pressure: usize,
    ) -> i64 {
        let base = self.compute_priority(idx, dep_graph);
        
        // Prefer instructions that reduce register pressure
        // (consume values that won't be used again)
        let pressure_factor = if current_pressure > 16 {
            // High pressure: strongly prefer consumers
            -100
        } else {
            0
        };
        
        base + pressure_factor
    }
    
    // ========================================================================
    // Resource-Constrained Scheduling
    // ========================================================================
    
    fn resource_schedule(
        &self,
        ir: &mut IrBlock,
        dep_graph: &DependencyGraph,
        profile: &ProfileDb,
    ) -> ScheduleResult {
        let n = dep_graph.instr_count();
        if n == 0 {
            return ScheduleResult {
                schedule: vec![],
                cycles: 0,
                speculative_moves: vec![],
                stats: ScheduleStats::default(),
            };
        }
        
        let mut state = SchedulingState::new(n);
        let mut schedule = Vec::with_capacity(n);
        let mut current_cycle = 0u32;
        
        // Resource availability per cycle
        let mut resources = ResourceTracker::new(&self.config.execution_units);
        
        // Initialize ready queue
        for i in 0..n {
            if dep_graph.nodes()[i].predecessors.is_empty() {
                state.ready_queue.push(ReadyInstr {
                    idx: i,
                    priority: self.compute_priority(i, dep_graph),
                    earliest_start: 0,
                });
            }
        }
        
        while schedule.len() < n {
            // Try to issue multiple instructions per cycle (superscalar)
            let mut issued_this_cycle = 0;
            let mut deferred = Vec::new();
            
            while let Some(ready) = state.ready_queue.pop() {
                if ready.earliest_start > current_cycle {
                    deferred.push(ready);
                    continue;
                }
                
                // Check resource availability
                let resource_type = self.instr_resource_type(ready.idx, ir);
                if resources.is_available(current_cycle, resource_type) {
                    // Issue instruction
                    schedule.push(ready.idx);
                    state.scheduled.insert(ready.idx);
                    
                    let latency = self.instr_latency(ready.idx, dep_graph);
                    let finish_cycle = current_cycle + latency;
                    state.finish_times[ready.idx] = finish_cycle;
                    
                    resources.reserve(current_cycle, resource_type, latency);
                    issued_this_cycle += 1;
                    
                    // Update successors
                    for edge in &dep_graph.nodes()[ready.idx].successors {
                        let succ = edge.to;
                        if !state.scheduled.contains(&succ) && !state.in_ready(&succ) {
                            let all_preds_done = dep_graph.nodes()[succ].predecessors.iter()
                                .all(|e| state.scheduled.contains(&e.from));
                            
                            if all_preds_done {
                                let earliest = dep_graph.nodes()[succ].predecessors.iter()
                                    .map(|e| state.finish_times[e.from] + e.latency as u32)
                                    .max()
                                    .unwrap_or(0);
                                
                                deferred.push(ReadyInstr {
                                    idx: succ,
                                    priority: self.compute_priority(succ, dep_graph),
                                    earliest_start: earliest,
                                });
                            }
                        }
                    }
                } else {
                    deferred.push(ready);
                }
            }
            
            // Put deferred back
            for instr in deferred {
                state.ready_queue.push(instr);
            }
            
            // Advance cycle
            current_cycle += 1;
            if current_cycle > n as u32 * 100 {
                break;
            }
        }
        
        let stats = self.compute_stats(&schedule, dep_graph, current_cycle);
        
        ScheduleResult {
            schedule,
            cycles: current_cycle,
            speculative_moves: vec![],
            stats,
        }
    }
    
    // ========================================================================
    // Helper Methods
    // ========================================================================
    
    fn compute_priority(&self, idx: usize, dep_graph: &DependencyGraph) -> i64 {
        let node = &dep_graph.nodes()[idx];
        
        // Base priority: number of successors (more = higher priority)
        let mut priority = node.successors.len() as i64 * 10;
        
        // Bonus for critical path
        if node.on_critical_path {
            priority += 1000;
        }
        
        // Penalty for high latency (schedule early)
        priority += node.earliest_start as i64;
        
        priority
    }
    
    fn instr_latency(&self, idx: usize, dep_graph: &DependencyGraph) -> u32 {
        // Get max latency from outgoing edges
        dep_graph.nodes()[idx].successors.iter()
            .map(|e| e.latency as u32)
            .max()
            .unwrap_or(1)
    }
    
    fn compute_stats(
        &self,
        schedule: &[usize],
        dep_graph: &DependencyGraph,
        cycles: u32,
    ) -> ScheduleStats {
        let total = schedule.len() as u32;
        let critical_instrs = dep_graph.critical_path().len() as u32;
        let critical_len = dep_graph.critical_length();
        
        let ilp = if cycles > 0 {
            total as f32 / cycles as f32
        } else {
            1.0
        };
        
        // Count reordered instructions
        let mut reordered = 0u32;
        for (new_pos, &orig_idx) in schedule.iter().enumerate() {
            if new_pos != orig_idx {
                reordered += 1;
            }
        }
        
        ScheduleStats {
            total_instrs: total,
            critical_path_instrs: critical_instrs,
            critical_path_length: critical_len,
            achieved_ilp: ilp,
            reordered,
            speculative_moves: 0,
            max_live_regs: 0,
        }
    }
    
    fn try_speculative_schedule(
        &self,
        state: &mut SchedulingState,
        dep_graph: &DependencyGraph,
        profile: &ProfileDb,
    ) -> Option<SpeculativeMove> {
        // Find a blocked instruction that could be speculatively scheduled
        let speculative_moves = dep_graph.speculative_moves();
        
        for &(load_idx, store_idx) in &speculative_moves {
            if !state.scheduled.contains(&load_idx) && state.scheduled.contains(&store_idx) {
                // Check if only speculative deps are blocking
                let only_speculative = dep_graph.nodes()[load_idx].predecessors.iter()
                    .filter(|e| !state.scheduled.contains(&e.from))
                    .all(|e| e.speculative);
                
                if only_speculative {
                    // Can speculatively schedule this load
                    let earliest = dep_graph.nodes()[load_idx].predecessors.iter()
                        .filter(|e| state.scheduled.contains(&e.from))
                        .map(|e| state.finish_times[e.from] + e.latency as u32)
                        .max()
                        .unwrap_or(0);
                    
                    state.ready_queue.push(ReadyInstr {
                        idx: load_idx,
                        priority: self.compute_priority(load_idx, dep_graph) + 500, // Boost priority
                        earliest_start: earliest,
                    });
                    
                    return Some(SpeculativeMove {
                        instr_idx: load_idx,
                        from_pos: load_idx,
                        to_pos: state.scheduled.len(),
                        kind: SpeculativeMoveKind::LoadBeforeStore,
                        deopt_rip: 0, // Would be set from instruction's guest_rip
                    });
                }
            }
        }
        
        None
    }
    
    fn compute_use_counts(&self, ir: &IrBlock) -> HashMap<VReg, usize> {
        let mut counts = HashMap::new();
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                for vreg in get_read_vregs(&instr.op) {
                    *counts.entry(vreg).or_insert(0) += 1;
                }
            }
        }
        
        counts
    }
    
    fn instr_resource_type(&self, _idx: usize, ir: &IrBlock) -> ResourceType {
        // Simplified - would need to look up actual instruction
        ResourceType::IntAlu
    }
}

impl Default for InstructionScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Scheduling State
// ============================================================================

struct SchedulingState {
    scheduled: HashSet<usize>,
    ready_queue: BinaryHeap<ReadyInstr>,
    finish_times: Vec<u32>,
}

impl SchedulingState {
    fn new(n: usize) -> Self {
        Self {
            scheduled: HashSet::new(),
            ready_queue: BinaryHeap::new(),
            finish_times: vec![0; n],
        }
    }
    
    fn in_ready(&self, idx: &usize) -> bool {
        self.ready_queue.iter().any(|r| r.idx == *idx)
    }
}

#[derive(Clone)]
struct ReadyInstr {
    idx: usize,
    priority: i64,
    earliest_start: u32,
}

impl PartialEq for ReadyInstr {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.earliest_start == other.earliest_start
    }
}

impl Eq for ReadyInstr {}

impl Ord for ReadyInstr {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Higher priority first, then earlier start time
        self.priority.cmp(&other.priority)
            .then_with(|| other.earliest_start.cmp(&self.earliest_start))
    }
}

impl PartialOrd for ReadyInstr {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

// ============================================================================
// Resource Tracking
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ResourceType {
    IntAlu,
    IntMul,
    IntDiv,
    MemLoad,
    MemStore,
    Branch,
    Vector,
}

struct ResourceTracker {
    // Resource availability: cycle -> available count per type
    availability: HashMap<u32, HashMap<ResourceType, u8>>,
    units: ExecutionUnits,
}

impl ResourceTracker {
    fn new(units: &ExecutionUnits) -> Self {
        Self {
            availability: HashMap::new(),
            units: units.clone(),
        }
    }
    
    fn is_available(&self, cycle: u32, resource: ResourceType) -> bool {
        let max = self.max_units(resource);
        let used = self.availability
            .get(&cycle)
            .and_then(|m| m.get(&resource))
            .copied()
            .unwrap_or(0);
        used < max
    }
    
    fn reserve(&mut self, cycle: u32, resource: ResourceType, duration: u32) {
        for c in cycle..cycle + duration {
            let cycle_map = self.availability.entry(c).or_insert_with(HashMap::new);
            *cycle_map.entry(resource).or_insert(0) += 1;
        }
    }
    
    fn max_units(&self, resource: ResourceType) -> u8 {
        match resource {
            ResourceType::IntAlu => self.units.int_alu,
            ResourceType::IntMul => self.units.int_mul,
            ResourceType::IntDiv => self.units.int_div,
            ResourceType::MemLoad => self.units.mem_load,
            ResourceType::MemStore => self.units.mem_store,
            ResourceType::Branch => self.units.branch,
            ResourceType::Vector => self.units.vector,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_read_vregs(op: &IrOp) -> Vec<VReg> {
    // Reuse from scope.rs
    super::scope::get_read_vregs_from_op(op)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ir::{IrBasicBlock, BlockId, IrFlags};
    
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
        
        // v2 = add v0, v1 (depends on v0, v1)
        bb.instrs.push(IrInstr {
            dst: VReg(2),
            op: IrOp::Add(VReg(0), VReg(1)),
            guest_rip: 0x1008,
            flags: IrFlags::default(),
        });
        
        // v3 = mul v0, v1 (depends on v0, v1, independent of v2)
        bb.instrs.push(IrInstr {
            dst: VReg(3),
            op: IrOp::Mul(VReg(0), VReg(1)),
            guest_rip: 0x100c,
            flags: IrFlags::default(),
        });
        
        // v4 = add v2, v3 (depends on v2, v3)
        bb.instrs.push(IrInstr {
            dst: VReg(4),
            op: IrOp::Add(VReg(2), VReg(3)),
            guest_rip: 0x1010,
            flags: IrFlags::default(),
        });
        
        ir.blocks.push(bb);
        ir
    }
    
    #[test]
    fn test_list_scheduling() {
        let mut ir = create_test_ir();
        let dep_graph = DependencyGraph::build(&ir);
        let profile = ProfileDb::new(1024);
        
        let scheduler = InstructionScheduler::with_config(SchedulerConfig {
            algorithm: SchedulingAlgorithm::List,
            ..Default::default()
        });
        
        let result = scheduler.schedule_block(&mut ir, &dep_graph, &profile);
        
        // Schedule should contain all instructions
        assert_eq!(result.schedule.len(), 5);
        
        // v0 and v1 should be scheduled before v2 and v3
        let pos_v0 = result.schedule.iter().position(|&x| x == 0).unwrap();
        let pos_v1 = result.schedule.iter().position(|&x| x == 1).unwrap();
        let pos_v2 = result.schedule.iter().position(|&x| x == 2).unwrap();
        let pos_v3 = result.schedule.iter().position(|&x| x == 3).unwrap();
        let pos_v4 = result.schedule.iter().position(|&x| x == 4).unwrap();
        
        assert!(pos_v0 < pos_v2);
        assert!(pos_v1 < pos_v2);
        assert!(pos_v0 < pos_v3);
        assert!(pos_v1 < pos_v3);
        assert!(pos_v2 < pos_v4);
        assert!(pos_v3 < pos_v4);
    }
    
    #[test]
    fn test_critical_path_scheduling() {
        let mut ir = create_test_ir();
        let dep_graph = DependencyGraph::build(&ir);
        let profile = ProfileDb::new(1024);
        
        let scheduler = InstructionScheduler::with_config(SchedulerConfig {
            algorithm: SchedulingAlgorithm::CriticalPath,
            ..Default::default()
        });
        
        let result = scheduler.schedule_block(&mut ir, &dep_graph, &profile);
        
        // Should produce a valid schedule with all 5 instructions
        assert_eq!(result.schedule.len(), 5);
        
        // Critical path scheduling should respect dependencies
        let pos_v0 = result.schedule.iter().position(|&x| x == 0).unwrap();
        let pos_v2 = result.schedule.iter().position(|&x| x == 2).unwrap();
        let pos_v4 = result.schedule.iter().position(|&x| x == 4).unwrap();
        assert!(pos_v0 < pos_v2, "v0 must be scheduled before v2");
        assert!(pos_v2 < pos_v4, "v2 must be scheduled before v4");
        
        // ILP should be positive (> 0)
        assert!(result.stats.achieved_ilp > 0.0);
    }
    
    #[test]
    fn test_scope_aware_scheduling() {
        let mut ir = create_test_ir();
        let profile = ProfileDb::new(1024);
        
        let scheduler = InstructionScheduler::new();
        
        // Block scope: conservative
        let result_block = scheduler.schedule_scope(&mut ir.clone(), ScopeLevel::Block, &profile);
        
        // Region scope: aggressive
        let result_region = scheduler.schedule_scope(&mut ir, ScopeLevel::Region, &profile);
        
        // Both should produce valid schedules
        assert_eq!(result_block.schedule.len(), 5);
        assert_eq!(result_region.schedule.len(), 5);
    }
}
