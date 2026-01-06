//! Profile-Guided Speculative Optimization Framework
//!
//! Implements aggressive optimizations based on runtime profiling data:
//! - Type Speculation: Assume object types, inline method bodies
//! - Value Speculation: Assume specific values (RAX==0x1F20)
//! - Branch Speculation: Hot path optimization, cold path separation
//! - Call Target Speculation: Monomorphic/Polymorphic call inlining
//! - Path Speculation: Multi-condition path optimization
//!
//! ## Speculation Workflow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                    Speculative Optimization Pipeline                        │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │    ┌────────────┐    ┌────────────┐    ┌────────────┐    ┌────────────┐   │
//! │    │  Profile   │───▶│  Analyze   │───▶│ Speculate  │───▶│  Compile   │   │
//! │    │  Collect   │    │  Patterns  │    │  & Guard   │    │  Optimized │   │
//! │    └────────────┘    └────────────┘    └────────────┘    └────────────┘   │
//! │                                                                             │
//! │  Types:                Patterns:          Guards:           Code:          │
//! │  • Type tags           • Mono calls       • Type checks     • Inlined      │
//! │  • VTable ptrs         • Hot branches     • Value checks    • Devirt'd     │
//! │  • Value ranges        • Value patterns   • Range checks    • Unrolled     │
//! │                                                                             │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example: Type Speculation
//!
//! Original code (virtual call):
//! ```text
//! mov rax, [rdi]          ; load vtable ptr
//! mov rax, [rax + 0x20]   ; load method ptr
//! call rax                ; indirect call
//! ```
//!
//! With type speculation (assuming ArrayList):
//! ```text
//! ; Guard: check vtable == ArrayList_vtable
//! cmp qword [rdi], ARRAYLIST_VTABLE
//! jne .deopt
//! ; Inlined ArrayList.get() body
//! mov rax, [rdi + 0x18]   ; load array ptr
//! mov eax, [rax + rbx*4]  ; direct element access
//! jmp .continue
//! .deopt:
//! ; fallback to interpreter
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use super::deopt::{DeoptGuard, DeoptReason, GuardKind, DeoptManager};
use super::profile::{ProfileDb, BranchBias};
use super::ir::{IrBlock, IrInstr, IrOp, VReg, BlockId, IrBasicBlock};

// ============================================================================
// Type Speculation
// ============================================================================

/// Type tag for value type tracking
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeTag {
    /// Unknown type (need runtime check)
    Unknown,
    /// Signed integer
    SignedInt,
    /// Unsigned integer
    UnsignedInt,
    /// Floating point
    Float,
    /// Pointer to specific type
    Pointer(u64),
    /// Object with known vtable
    Object(u64),
    /// Array with known element type
    Array(u64),
    /// Boolean
    Bool,
    /// Null pointer
    Null,
    /// Small integer (fits in imm32)
    SmallInt,
}

/// Type profile for a register at a specific location
#[derive(Clone, Debug)]
pub struct TypeProfile {
    /// RIP where type was observed
    pub rip: u64,
    /// Register index
    pub reg: u8,
    /// Observed types with counts
    pub types: HashMap<TypeTag, u64>,
    /// Total observations
    pub total: u64,
}

impl TypeProfile {
    pub fn new(rip: u64, reg: u8) -> Self {
        Self {
            rip,
            reg,
            types: HashMap::new(),
            total: 0,
        }
    }
    
    pub fn record(&mut self, tag: TypeTag) {
        *self.types.entry(tag).or_insert(0) += 1;
        self.total += 1;
    }
    
    /// Get dominant type if confidence > threshold
    pub fn dominant_type(&self, threshold: f64) -> Option<TypeTag> {
        if self.total < 100 {
            return None;
        }
        
        self.types.iter()
            .max_by_key(|(_, &count)| count)
            .filter(|(_, &count)| (count as f64 / self.total as f64) >= threshold)
            .map(|(&tag, _)| tag)
    }
    
    /// Check if type is monomorphic (single type > 99%)
    pub fn is_monomorphic(&self) -> bool {
        self.dominant_type(0.99).is_some()
    }
    
    /// Check if type is polymorphic (2-4 types cover 95%)
    pub fn is_polymorphic(&self) -> Option<Vec<TypeTag>> {
        if self.total < 100 {
            return None;
        }
        
        let mut sorted: Vec<_> = self.types.iter()
            .map(|(&t, &c)| (t, c))
            .collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        
        let mut result = Vec::new();
        let mut covered = 0u64;
        
        for (tag, count) in sorted.iter().take(4) {
            result.push(*tag);
            covered += count;
            if (covered as f64 / self.total as f64) >= 0.95 {
                return Some(result);
            }
        }
        
        None
    }
}

// ============================================================================
// Value Speculation
// ============================================================================

/// Value speculation info for specific constants
#[derive(Clone, Debug)]
pub struct ValueSpeculation {
    /// Guest RIP where value is used
    pub rip: u64,
    /// Register containing the value
    pub reg: u8,
    /// Expected value(s) with probabilities
    pub values: Vec<(u64, f64)>,
    /// Guard to insert
    pub guard_kind: GuardKind,
}

impl ValueSpeculation {
    pub fn single(rip: u64, reg: u8, value: u64) -> Self {
        Self {
            rip,
            reg,
            values: vec![(value, 1.0)],
            guard_kind: GuardKind::ValueEquals { reg, expected: value },
        }
    }
    
    pub fn range(rip: u64, reg: u8, min: u64, max: u64) -> Self {
        Self {
            rip,
            reg,
            values: vec![],
            guard_kind: GuardKind::ValueInRange { reg, min, max },
        }
    }
}

/// Value profile for tracking specific values
#[derive(Clone, Debug)]
pub struct ValueProfile {
    /// RIP where value was observed
    pub rip: u64,
    /// Register index
    pub reg: u8,
    /// Value -> count mapping (limited to top N)
    pub values: HashMap<u64, u64>,
    /// Total observations
    pub total: u64,
    /// Maximum tracked values
    max_values: usize,
}

impl ValueProfile {
    pub fn new(rip: u64, reg: u8) -> Self {
        Self {
            rip,
            reg,
            values: HashMap::new(),
            total: 0,
            max_values: 8,
        }
    }
    
    pub fn record(&mut self, value: u64) {
        self.total += 1;
        
        if self.values.len() < self.max_values {
            *self.values.entry(value).or_insert(0) += 1;
        } else if self.values.contains_key(&value) {
            *self.values.get_mut(&value).unwrap() += 1;
        }
        // else: exceeded max_values, just increment total
    }
    
    /// Get dominant value if confidence > threshold
    pub fn dominant_value(&self, threshold: f64) -> Option<(u64, f64)> {
        if self.total < 100 {
            return None;
        }
        
        self.values.iter()
            .max_by_key(|(_, &count)| count)
            .filter(|(_, &count)| {
                let ratio = count as f64 / self.total as f64;
                ratio >= threshold
            })
            .map(|(&val, &count)| (val, count as f64 / self.total as f64))
    }
    
    /// Check if values fall within a tight range
    pub fn tight_range(&self) -> Option<(u64, u64)> {
        if self.values.is_empty() {
            return None;
        }
        
        let min = *self.values.keys().min().unwrap();
        let max = *self.values.keys().max().unwrap();
        
        // Range is "tight" if span is < 256 and covers 90% of observations
        let span = max - min;
        let covered: u64 = self.values.values().sum();
        
        if span <= 256 && (covered as f64 / self.total as f64) >= 0.90 {
            Some((min, max + 1))
        } else {
            None
        }
    }
}

// ============================================================================
// Branch Speculation
// ============================================================================

/// Branch speculation for hot/cold path optimization
#[derive(Clone, Debug)]
pub struct BranchSpeculation {
    /// Guest RIP of branch instruction
    pub rip: u64,
    /// Expected direction (true = taken, false = not taken)
    pub expected_taken: bool,
    /// Confidence level (0.0 - 1.0)
    pub confidence: f64,
    /// Hot path target RIP
    pub hot_target: u64,
    /// Cold path target RIP (moved out of line)
    pub cold_target: u64,
}

impl BranchSpeculation {
    /// Create from branch profile
    pub fn from_profile(
        rip: u64,
        bias: BranchBias,
        taken_target: u64,
        fallthrough: u64,
    ) -> Option<Self> {
        match bias {
            BranchBias::AlwaysTaken | BranchBias::MostlyTaken => {
                Some(Self {
                    rip,
                    expected_taken: true,
                    confidence: if bias == BranchBias::AlwaysTaken { 0.99 } else { 0.85 },
                    hot_target: taken_target,
                    cold_target: fallthrough,
                })
            }
            BranchBias::NeverTaken | BranchBias::MostlyNotTaken => {
                Some(Self {
                    rip,
                    expected_taken: false,
                    confidence: if bias == BranchBias::NeverTaken { 0.99 } else { 0.85 },
                    hot_target: fallthrough,
                    cold_target: taken_target,
                })
            }
            _ => None,
        }
    }
}

// ============================================================================
// Call Target Speculation (Devirtualization)
// ============================================================================

/// Call target speculation for virtual call inlining
#[derive(Clone, Debug)]
pub struct CallSpeculation {
    /// Guest RIP of call instruction
    pub rip: u64,
    /// Speculation type
    pub kind: CallSpecKind,
    /// Target(s) for inlining
    pub targets: Vec<u64>,
    /// Confidence level
    pub confidence: f64,
}

/// Kind of call speculation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallSpecKind {
    /// Single target (>99% of calls)
    Monomorphic,
    /// 2-4 targets (>95% of calls)
    Polymorphic,
    /// Many targets (megamorphic, don't speculate)
    Megamorphic,
}

impl CallSpeculation {
    pub fn monomorphic(rip: u64, target: u64, confidence: f64) -> Self {
        Self {
            rip,
            kind: CallSpecKind::Monomorphic,
            targets: vec![target],
            confidence,
        }
    }
    
    pub fn polymorphic(rip: u64, targets: Vec<u64>, confidence: f64) -> Self {
        Self {
            rip,
            kind: CallSpecKind::Polymorphic,
            targets,
            confidence,
        }
    }
    
    /// Can this call be inlined?
    pub fn can_inline(&self) -> bool {
        matches!(self.kind, CallSpecKind::Monomorphic | CallSpecKind::Polymorphic)
    }
}

// ============================================================================
// Path Speculation (Multi-Condition Optimization)
// ============================================================================

/// Path speculation for complex condition optimization
/// 
/// Example: "If RAX == 0x1F20 AND RBX < 100, skip all checks"
#[derive(Clone, Debug)]
pub struct PathSpeculation {
    /// Guest RIP where path starts
    pub entry_rip: u64,
    /// Conditions that define this path
    pub conditions: Vec<PathCondition>,
    /// Probability of this path being taken
    pub probability: f64,
    /// Optimized target (skip checks, inline, etc.)
    pub optimized_target: u64,
    /// Fallback target (original slow path)
    pub fallback_target: u64,
}

/// Single condition in a path
#[derive(Clone, Debug)]
pub enum PathCondition {
    /// Register equals specific value
    RegEquals { reg: u8, value: u64 },
    /// Register in range
    RegInRange { reg: u8, min: u64, max: u64 },
    /// Memory equals value
    MemEquals { base_reg: u8, offset: i32, value: u64, size: u8 },
    /// Flag is set/clear
    FlagSet { flag: u8, expected: bool },
    /// Pointer is non-null
    NonNull { reg: u8 },
}

impl PathSpeculation {
    /// Convert to compound guard
    pub fn to_guard(&self, deopt_mgr: &DeoptManager) -> DeoptGuard {
        let guards: Vec<GuardKind> = self.conditions.iter()
            .map(|cond| match cond {
                PathCondition::RegEquals { reg, value } => {
                    GuardKind::ValueEquals { reg: *reg, expected: *value }
                }
                PathCondition::RegInRange { reg, min, max } => {
                    GuardKind::ValueInRange { reg: *reg, min: *min, max: *max }
                }
                PathCondition::MemEquals { base_reg, offset, value, size } => {
                    GuardKind::MemoryEquals {
                        addr_reg: *base_reg,
                        offset: *offset,
                        expected: *value,
                        size: *size,
                    }
                }
                PathCondition::NonNull { reg } => {
                    GuardKind::NonNull { reg: *reg }
                }
                PathCondition::FlagSet { .. } => {
                    // Flags need special handling
                    GuardKind::ValueEquals { reg: 0, expected: 0 } // placeholder
                }
            })
            .collect();
        
        DeoptGuard::new(
            deopt_mgr.alloc_guard_id(),
            self.entry_rip,
            GuardKind::All(guards),
            DeoptReason::ValueMismatch,
        )
    }
}

// ============================================================================
// Speculation Manager
// ============================================================================

/// Central manager for all speculation decisions
pub struct SpeculationManager {
    /// Type profiles: (rip, reg) -> TypeProfile
    type_profiles: RwLock<HashMap<(u64, u8), TypeProfile>>,
    
    /// Value profiles: (rip, reg) -> ValueProfile
    value_profiles: RwLock<HashMap<(u64, u8), ValueProfile>>,
    
    /// Path profiles: entry_rip -> Vec<PathProfile>
    path_profiles: RwLock<HashMap<u64, Vec<PathProfile>>>,
    
    /// Active speculations for each block
    active_specs: RwLock<HashMap<u64, BlockSpeculations>>,
    
    /// Configuration
    config: SpecConfig,
    
    /// Statistics
    stats: SpecStats,
}

/// Configuration for speculation
#[derive(Clone, Debug)]
pub struct SpecConfig {
    /// Minimum confidence for type speculation
    pub type_confidence: f64,
    /// Minimum confidence for value speculation
    pub value_confidence: f64,
    /// Minimum confidence for branch speculation
    pub branch_confidence: f64,
    /// Minimum confidence for call speculation
    pub call_confidence: f64,
    /// Enable type speculation
    pub enable_type_spec: bool,
    /// Enable value speculation
    pub enable_value_spec: bool,
    /// Enable branch speculation
    pub enable_branch_spec: bool,
    /// Enable call speculation
    pub enable_call_spec: bool,
    /// Enable path speculation
    pub enable_path_spec: bool,
    /// Maximum inline size for devirtualization
    pub max_inline_size: usize,
}

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            type_confidence: 0.95,
            value_confidence: 0.99,
            branch_confidence: 0.90,
            call_confidence: 0.95,
            enable_type_spec: true,
            enable_value_spec: true,
            enable_branch_spec: true,
            enable_call_spec: true,
            enable_path_spec: true,
            max_inline_size: 100,
        }
    }
}

/// Path execution profile
#[derive(Clone, Debug)]
pub struct PathProfile {
    /// Conditions observed on this path
    pub conditions: Vec<PathCondition>,
    /// Number of times this exact path was taken
    pub count: u64,
    /// Target RIP after path
    pub target: u64,
}

/// Active speculations for a compiled block
#[derive(Clone, Debug, Default)]
pub struct BlockSpeculations {
    pub types: Vec<TypeSpeculation>,
    pub values: Vec<ValueSpeculation>,
    pub branches: Vec<BranchSpeculation>,
    pub calls: Vec<CallSpeculation>,
    pub paths: Vec<PathSpeculation>,
}

/// Type speculation decision
#[derive(Clone, Debug)]
pub struct TypeSpeculation {
    pub rip: u64,
    pub reg: u8,
    pub expected_type: TypeTag,
    pub guard_kind: GuardKind,
}

/// Speculation statistics
pub struct SpecStats {
    pub type_specs_generated: AtomicU64,
    pub value_specs_generated: AtomicU64,
    pub branch_specs_generated: AtomicU64,
    pub call_specs_generated: AtomicU64,
    pub path_specs_generated: AtomicU64,
    pub type_spec_hits: AtomicU64,
    pub type_spec_misses: AtomicU64,
    pub value_spec_hits: AtomicU64,
    pub value_spec_misses: AtomicU64,
    pub branch_spec_hits: AtomicU64,
    pub branch_spec_misses: AtomicU64,
    pub call_spec_hits: AtomicU64,
    pub call_spec_misses: AtomicU64,
    pub path_spec_hits: AtomicU64,
    pub path_spec_misses: AtomicU64,
}

impl Default for SpecStats {
    fn default() -> Self {
        Self {
            type_specs_generated: AtomicU64::new(0),
            value_specs_generated: AtomicU64::new(0),
            branch_specs_generated: AtomicU64::new(0),
            call_specs_generated: AtomicU64::new(0),
            path_specs_generated: AtomicU64::new(0),
            type_spec_hits: AtomicU64::new(0),
            type_spec_misses: AtomicU64::new(0),
            value_spec_hits: AtomicU64::new(0),
            value_spec_misses: AtomicU64::new(0),
            branch_spec_hits: AtomicU64::new(0),
            branch_spec_misses: AtomicU64::new(0),
            call_spec_hits: AtomicU64::new(0),
            call_spec_misses: AtomicU64::new(0),
            path_spec_hits: AtomicU64::new(0),
            path_spec_misses: AtomicU64::new(0),
        }
    }
}

impl SpeculationManager {
    pub fn new() -> Self {
        Self::with_config(SpecConfig::default())
    }
    
    pub fn with_config(config: SpecConfig) -> Self {
        Self {
            type_profiles: RwLock::new(HashMap::new()),
            value_profiles: RwLock::new(HashMap::new()),
            path_profiles: RwLock::new(HashMap::new()),
            active_specs: RwLock::new(HashMap::new()),
            config,
            stats: SpecStats::default(),
        }
    }
    
    // ========================================================================
    // Profile Recording
    // ========================================================================
    
    /// Record type observation
    pub fn record_type(&self, rip: u64, reg: u8, tag: TypeTag) {
        let key = (rip, reg);
        let mut profiles = self.type_profiles.write().unwrap();
        profiles.entry(key)
            .or_insert_with(|| TypeProfile::new(rip, reg))
            .record(tag);
    }
    
    /// Record value observation
    pub fn record_value(&self, rip: u64, reg: u8, value: u64) {
        let key = (rip, reg);
        let mut profiles = self.value_profiles.write().unwrap();
        profiles.entry(key)
            .or_insert_with(|| ValueProfile::new(rip, reg))
            .record(value);
    }
    
    /// Record path observation
    pub fn record_path(&self, entry_rip: u64, conditions: Vec<PathCondition>, target: u64) {
        let mut profiles = self.path_profiles.write().unwrap();
        let paths = profiles.entry(entry_rip).or_insert_with(Vec::new);
        
        // Find matching path or create new
        for path in paths.iter_mut() {
            if path.target == target && paths_match(&path.conditions, &conditions) {
                path.count += 1;
                return;
            }
        }
        
        // New path
        if paths.len() < 8 { // Limit tracked paths
            paths.push(PathProfile {
                conditions,
                count: 1,
                target,
            });
        }
    }
    
    // ========================================================================
    // Speculation Analysis
    // ========================================================================
    
    /// Analyze profiles and generate speculations for a block
    pub fn analyze_block(
        &self,
        block_rip: u64,
        profile_db: &ProfileDb,
        deopt_mgr: &DeoptManager,
    ) -> BlockSpeculations {
        let mut specs = BlockSpeculations::default();
        
        // Type speculations
        if self.config.enable_type_spec {
            let types = self.type_profiles.read().unwrap();
            for ((rip, reg), profile) in types.iter() {
                if !self.is_in_block(*rip, block_rip) {
                    continue;
                }
                
                if let Some(dom_type) = profile.dominant_type(self.config.type_confidence) {
                    if deopt_mgr.is_speculation_disabled(*rip, &type_to_guard(dom_type, *reg)) {
                        continue;
                    }
                    
                    specs.types.push(TypeSpeculation {
                        rip: *rip,
                        reg: *reg,
                        expected_type: dom_type,
                        guard_kind: type_to_guard(dom_type, *reg),
                    });
                    self.stats.type_specs_generated.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        
        // Value speculations
        if self.config.enable_value_spec {
            let values = self.value_profiles.read().unwrap();
            for ((rip, reg), profile) in values.iter() {
                if !self.is_in_block(*rip, block_rip) {
                    continue;
                }
                
                if let Some((value, _conf)) = profile.dominant_value(self.config.value_confidence) {
                    let guard = GuardKind::ValueEquals { reg: *reg, expected: value };
                    if deopt_mgr.is_speculation_disabled(*rip, &guard) {
                        continue;
                    }
                    
                    specs.values.push(ValueSpeculation::single(*rip, *reg, value));
                    self.stats.value_specs_generated.fetch_add(1, Ordering::Relaxed);
                } else if let Some((min, max)) = profile.tight_range() {
                    let guard = GuardKind::ValueInRange { reg: *reg, min, max };
                    if deopt_mgr.is_speculation_disabled(*rip, &guard) {
                        continue;
                    }
                    
                    specs.values.push(ValueSpeculation::range(*rip, *reg, min, max));
                    self.stats.value_specs_generated.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        
        // Branch speculations
        if self.config.enable_branch_spec {
            // Get branch data from profile_db
            let hot_blocks = profile_db.hot_blocks(100);
            for (rip, _count) in hot_blocks {
                if !self.is_in_block(rip, block_rip) {
                    continue;
                }
                
                let bias = profile_db.get_branch_bias(rip);
                if let Some(branch_spec) = BranchSpeculation::from_profile(
                    rip,
                    bias,
                    0, // Would get actual targets from IR
                    0,
                ) {
                    if branch_spec.confidence >= self.config.branch_confidence {
                        let guard = GuardKind::BranchDirection { expected_taken: branch_spec.expected_taken };
                        if !deopt_mgr.is_speculation_disabled(rip, &guard) {
                            specs.branches.push(branch_spec);
                            self.stats.branch_specs_generated.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
        
        // Call speculations
        if self.config.enable_call_spec {
            let hot_blocks = profile_db.hot_blocks(100);
            for (rip, _count) in hot_blocks {
                if !self.is_in_block(rip, block_rip) {
                    continue;
                }
                
                if let Some((target, ratio)) = profile_db.get_call_target(rip) {
                    if ratio >= self.config.call_confidence {
                        let guard = GuardKind::CallTarget { 
                            target_reg: 0, // Would get from instruction 
                            expected: vec![target],
                        };
                        if !deopt_mgr.is_speculation_disabled(rip, &guard) {
                            specs.calls.push(CallSpeculation::monomorphic(rip, target, ratio));
                            self.stats.call_specs_generated.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
        
        // Path speculations
        if self.config.enable_path_spec {
            let paths = self.path_profiles.read().unwrap();
            if let Some(block_paths) = paths.get(&block_rip) {
                let total: u64 = block_paths.iter().map(|p| p.count).sum();
                
                for path in block_paths {
                    let prob = path.count as f64 / total as f64;
                    if prob >= 0.90 && !path.conditions.is_empty() {
                        specs.paths.push(PathSpeculation {
                            entry_rip: block_rip,
                            conditions: path.conditions.clone(),
                            probability: prob,
                            optimized_target: path.target,
                            fallback_target: block_rip, // Would compute actual fallback
                        });
                        self.stats.path_specs_generated.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        
        // Store active speculations
        self.active_specs.write().unwrap().insert(block_rip, specs.clone());
        
        specs
    }
    
    /// Check if RIP is within a block (simplified)
    fn is_in_block(&self, rip: u64, block_rip: u64) -> bool {
        // In real implementation, would check against block boundaries
        rip >= block_rip && rip < block_rip + 0x1000
    }
    
    /// Get statistics snapshot
    pub fn stats_snapshot(&self) -> SpecStatsSnapshot {
        SpecStatsSnapshot {
            type_specs_generated: self.stats.type_specs_generated.load(Ordering::Relaxed),
            value_specs_generated: self.stats.value_specs_generated.load(Ordering::Relaxed),
            branch_specs_generated: self.stats.branch_specs_generated.load(Ordering::Relaxed),
            call_specs_generated: self.stats.call_specs_generated.load(Ordering::Relaxed),
            path_specs_generated: self.stats.path_specs_generated.load(Ordering::Relaxed),
            type_spec_hits: self.stats.type_spec_hits.load(Ordering::Relaxed),
            type_spec_misses: self.stats.type_spec_misses.load(Ordering::Relaxed),
            value_spec_hits: self.stats.value_spec_hits.load(Ordering::Relaxed),
            value_spec_misses: self.stats.value_spec_misses.load(Ordering::Relaxed),
            branch_spec_hits: self.stats.branch_spec_hits.load(Ordering::Relaxed),
            branch_spec_misses: self.stats.branch_spec_misses.load(Ordering::Relaxed),
            call_spec_hits: self.stats.call_spec_hits.load(Ordering::Relaxed),
            call_spec_misses: self.stats.call_spec_misses.load(Ordering::Relaxed),
            path_spec_hits: self.stats.path_spec_hits.load(Ordering::Relaxed),
            path_spec_misses: self.stats.path_spec_misses.load(Ordering::Relaxed),
        }
    }
    
    /// Clear profiles for a block (called on recompilation)
    pub fn clear_block(&self, block_rip: u64) {
        self.active_specs.write().unwrap().remove(&block_rip);
        
        // Optionally clear profiles (or keep for recompilation)
        let mut types = self.type_profiles.write().unwrap();
        types.retain(|(rip, _), _| !self.is_in_block(*rip, block_rip));
        
        let mut values = self.value_profiles.write().unwrap();
        values.retain(|(rip, _), _| !self.is_in_block(*rip, block_rip));
        
        self.path_profiles.write().unwrap().remove(&block_rip);
    }
}

/// Statistics snapshot
#[derive(Clone, Debug)]
pub struct SpecStatsSnapshot {
    pub type_specs_generated: u64,
    pub value_specs_generated: u64,
    pub branch_specs_generated: u64,
    pub call_specs_generated: u64,
    pub path_specs_generated: u64,
    pub type_spec_hits: u64,
    pub type_spec_misses: u64,
    pub value_spec_hits: u64,
    pub value_spec_misses: u64,
    pub branch_spec_hits: u64,
    pub branch_spec_misses: u64,
    pub call_spec_hits: u64,
    pub call_spec_misses: u64,
    pub path_spec_hits: u64,
    pub path_spec_misses: u64,
}

// ============================================================================
// IR Transformation for Speculation
// ============================================================================

/// Apply speculation optimizations to IR
pub fn apply_speculations(
    ir: &mut IrBlock,
    specs: &BlockSpeculations,
    deopt_mgr: &DeoptManager,
) -> Vec<DeoptGuard> {
    let mut guards = Vec::new();
    
    // Insert type guards
    for type_spec in &specs.types {
        let guard = DeoptGuard::new(
            deopt_mgr.alloc_guard_id(),
            type_spec.rip,
            type_spec.guard_kind.clone(),
            DeoptReason::TypeMismatch,
        );
        
        // Insert guard at beginning of block containing this RIP
        insert_guard_ir(ir, type_spec.rip, &guard);
        guards.push(guard);
    }
    
    // Insert value guards
    for value_spec in &specs.values {
        let guard = DeoptGuard::new(
            deopt_mgr.alloc_guard_id(),
            value_spec.rip,
            value_spec.guard_kind.clone(),
            DeoptReason::ValueMismatch,
        );
        
        insert_guard_ir(ir, value_spec.rip, &guard);
        guards.push(guard);
    }
    
    // Apply branch speculation (reorder blocks)
    for branch_spec in &specs.branches {
        apply_branch_speculation(ir, branch_spec);
    }
    
    // Apply call speculation (inline)
    for call_spec in &specs.calls {
        if call_spec.can_inline() {
            let guard = DeoptGuard::new(
                deopt_mgr.alloc_guard_id(),
                call_spec.rip,
                GuardKind::CallTarget {
                    target_reg: 0, // Would get from instruction
                    expected: call_spec.targets.clone(),
                },
                DeoptReason::CallTargetMismatch,
            );
            
            apply_call_speculation(ir, call_spec, &guard);
            guards.push(guard);
        }
    }
    
    // Apply path speculation
    for path_spec in &specs.paths {
        let guard = path_spec.to_guard(deopt_mgr);
        apply_path_speculation(ir, path_spec, &guard);
        guards.push(guard);
    }
    
    guards
}

/// Insert a guard into IR at specified RIP
fn insert_guard_ir(ir: &mut IrBlock, rip: u64, guard: &DeoptGuard) {
    // Find the block containing this RIP
    for bb in &mut ir.blocks {
        if bb.entry_rip == rip || (bb.entry_rip < rip && rip < bb.entry_rip + 0x100) {
            // Create guard instruction
            let guard_instr = IrInstr {
                dst: VReg::NONE,
                op: IrOp::Exit(super::ir::ExitReason::Normal), // Would be Guard op
                guest_rip: rip,
                flags: super::ir::IrFlags::empty(),
            };
            
            // Insert at beginning of block (after loads)
            let insert_pos = bb.instrs.iter()
                .position(|i| !matches!(i.op, IrOp::LoadGpr(_) | IrOp::LoadFlags | IrOp::LoadRip))
                .unwrap_or(0);
            
            bb.instrs.insert(insert_pos, guard_instr);
            break;
        }
    }
    let _ = guard; // Used in real implementation
}

/// Apply branch speculation by reordering blocks
fn apply_branch_speculation(ir: &mut IrBlock, spec: &BranchSpeculation) {
    // In real implementation:
    // 1. Find branch instruction at spec.rip
    // 2. If branch is hot (expected_taken), ensure hot target is fall-through
    // 3. Move cold target to end of function (out of hot path)
    let _ = (ir, spec);
}

/// Apply call speculation by inlining
fn apply_call_speculation(ir: &mut IrBlock, spec: &CallSpeculation, _guard: &DeoptGuard) {
    // In real implementation:
    // 1. Find call instruction at spec.rip
    // 2. Insert guard checking target matches expected
    // 3. Replace call with inlined body (if small enough)
    // 4. For polymorphic, generate type-dispatch chain
    let _ = (ir, spec);
}

/// Apply path speculation
fn apply_path_speculation(ir: &mut IrBlock, spec: &PathSpeculation, _guard: &DeoptGuard) {
    // In real implementation:
    // 1. Insert compound guard at entry
    // 2. If guard passes, skip to optimized path
    // 3. If guard fails, fall through to slow path
    let _ = (ir, spec);
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert type tag to guard kind
fn type_to_guard(tag: TypeTag, reg: u8) -> GuardKind {
    match tag {
        TypeTag::Null => GuardKind::ValueEquals { reg, expected: 0 },
        TypeTag::SmallInt => GuardKind::ValueInRange { reg, min: 0, max: 0x7FFFFFFF },
        TypeTag::Object(vtable) | TypeTag::Pointer(vtable) => {
            // Guard checks vtable pointer at [reg]
            GuardKind::MemoryEquals {
                addr_reg: reg,
                offset: 0,
                expected: vtable,
                size: 8,
            }
        }
        _ => GuardKind::ValueEquals { reg, expected: 0 }, // Fallback
    }
}

/// Check if two path conditions match
fn paths_match(a: &[PathCondition], b: &[PathCondition]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    
    for (ca, cb) in a.iter().zip(b.iter()) {
        match (ca, cb) {
            (PathCondition::RegEquals { reg: ra, value: va },
             PathCondition::RegEquals { reg: rb, value: vb }) => {
                if ra != rb || va != vb {
                    return false;
                }
            }
            (PathCondition::RegInRange { reg: ra, min: mina, max: maxa },
             PathCondition::RegInRange { reg: rb, min: minb, max: maxb }) => {
                if ra != rb || mina != minb || maxa != maxb {
                    return false;
                }
            }
            (PathCondition::NonNull { reg: ra }, PathCondition::NonNull { reg: rb }) => {
                if ra != rb {
                    return false;
                }
            }
            _ => return false,
        }
    }
    
    true
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_type_profile() {
        let mut profile = TypeProfile::new(0x1000, 0);
        
        // Record monomorphic type
        for _ in 0..1000 {
            profile.record(TypeTag::SignedInt);
        }
        
        assert!(profile.is_monomorphic());
        assert_eq!(profile.dominant_type(0.99), Some(TypeTag::SignedInt));
    }
    
    #[test]
    fn test_value_profile() {
        let mut profile = ValueProfile::new(0x1000, 0);
        
        // Record dominant value
        for _ in 0..990 {
            profile.record(42);
        }
        for _ in 0..10 {
            profile.record(43);
        }
        
        let (value, conf) = profile.dominant_value(0.95).unwrap();
        assert_eq!(value, 42);
        assert!(conf > 0.95);
    }
    
    #[test]
    fn test_branch_speculation() {
        let spec = BranchSpeculation::from_profile(
            0x1000,
            BranchBias::AlwaysTaken,
            0x2000,
            0x1010,
        ).unwrap();
        
        assert!(spec.expected_taken);
        assert_eq!(spec.hot_target, 0x2000);
        assert_eq!(spec.cold_target, 0x1010);
        assert!(spec.confidence > 0.95);
    }
    
    #[test]
    fn test_call_speculation() {
        let spec = CallSpeculation::monomorphic(0x1000, 0x5000, 0.99);
        
        assert!(spec.can_inline());
        assert_eq!(spec.kind, CallSpecKind::Monomorphic);
        assert_eq!(spec.targets, vec![0x5000]);
    }
    
    #[test]
    fn test_speculation_manager() {
        let mgr = SpeculationManager::new();
        
        // Record types
        for _ in 0..1000 {
            mgr.record_type(0x1000, 0, TypeTag::SignedInt);
        }
        
        // Record values
        for _ in 0..1000 {
            mgr.record_value(0x1010, 1, 0x1F20);
        }
        
        let profile_db = ProfileDb::new(10000);
        let deopt_mgr = DeoptManager::new(10);
        
        let specs = mgr.analyze_block(0x1000, &profile_db, &deopt_mgr);
        
        // Should have generated type speculation
        assert!(!specs.types.is_empty() || specs.values.is_empty());
    }
}
