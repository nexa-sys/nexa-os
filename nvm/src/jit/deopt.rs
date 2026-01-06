//! Zero-STW Deoptimization Framework
//!
//! Implements on-stack replacement (OSR) style deoptimization without stop-the-world pauses.
//! When a speculation guard fails, execution seamlessly transfers to interpreter.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                    Zero-STW Deoptimization Flow                             │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  ┌─────────────────┐     Guard Fail     ┌─────────────────┐                │
//! │  │   Optimized     │────────────────────▶│   Interpreter   │                │
//! │  │   Native Code   │                     │   (Fallback)    │                │
//! │  │                 │                     │                 │                │
//! │  │ • Type Guards   │                     │ • Restore state │                │
//! │  │ • Value Guards  │    DeoptState       │ • Continue exec │                │
//! │  │ • Path Guards   │───────────────────▶│ • Re-profile    │                │
//! │  └─────────────────┘                     └─────────────────┘                │
//! │                                                                             │
//! │  ┌─────────────────────────────────────────────────────────────────────┐  │
//! │  │                         Guard Types                                  │  │
//! │  │  • TypeGuard: Check object type (e.g., RAX is ptr to ArrayList)     │  │
//! │  │  • ValueGuard: Check specific value (e.g., RAX == 0x1F20)           │  │
//! │  │  • RangeGuard: Check value in range (e.g., 0 <= RAX < 100)          │  │
//! │  │  • NullGuard: Check non-null pointer                                 │  │
//! │  │  • BranchGuard: Check branch direction matches profile               │  │
//! │  │  • CallTargetGuard: Check indirect call target                       │  │
//! │  └─────────────────────────────────────────────────────────────────────┘  │
//! │                                                                             │
//! │  ┌─────────────────────────────────────────────────────────────────────┐  │
//! │  │                      Recompilation Strategy                          │  │
//! │  │  1. Guard failure counter incremented                                │  │
//! │  │  2. If failures > threshold → mark speculation as invalid           │  │
//! │  │  3. Next S2 compile omits that speculation                          │  │
//! │  │  4. Or: generate polymorphic path (inline both branches)            │  │
//! │  └─────────────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Zero-STW Design
//!
//! Unlike traditional deoptimization that requires stopping all threads:
//! - Guards are inlined as conditional jumps in native code
//! - Failed guards jump to a trampoline that:
//!   1. Saves current register state to DeoptState
//!   2. Computes guest state from native state using metadata
//!   3. Jumps to interpreter entry with restored guest state
//! - No global synchronization required

use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use std::sync::RwLock;

/// Reason for deoptimization
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeoptReason {
    /// Type speculation failed
    TypeMismatch,
    /// Value speculation failed (expected specific value)
    ValueMismatch,
    /// Value out of expected range
    RangeViolation,
    /// Unexpected null pointer
    NullPointer,
    /// Branch went opposite direction
    BranchMispredict,
    /// Indirect call to unexpected target
    CallTargetMismatch,
    /// Division by zero
    DivisionByZero,
    /// Integer overflow
    Overflow,
    /// Memory access fault
    MemoryFault,
    /// Unknown/other reason
    Other,
}

impl DeoptReason {
    /// Whether this deopt should invalidate the speculation permanently
    pub fn should_invalidate(&self) -> bool {
        matches!(self, 
            DeoptReason::TypeMismatch | 
            DeoptReason::ValueMismatch |
            DeoptReason::BranchMispredict |
            DeoptReason::CallTargetMismatch
        )
    }
    
    /// Human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            DeoptReason::TypeMismatch => "type_mismatch",
            DeoptReason::ValueMismatch => "value_mismatch",
            DeoptReason::RangeViolation => "range_violation",
            DeoptReason::NullPointer => "null_pointer",
            DeoptReason::BranchMispredict => "branch_mispredict",
            DeoptReason::CallTargetMismatch => "call_target_mismatch",
            DeoptReason::DivisionByZero => "division_by_zero",
            DeoptReason::Overflow => "overflow",
            DeoptReason::MemoryFault => "memory_fault",
            DeoptReason::Other => "other",
        }
    }
}

/// Guard specification for speculation
#[derive(Debug)]
pub struct DeoptGuard {
    /// Unique ID for this guard
    pub id: u32,
    /// Guest RIP where speculation applies
    pub guest_rip: u64,
    /// Type of speculation
    pub kind: GuardKind,
    /// Reason code if guard fails
    pub reason: DeoptReason,
    /// Number of times this guard has failed
    pub failures: AtomicU64,
}

impl Clone for DeoptGuard {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            guest_rip: self.guest_rip,
            kind: self.kind.clone(),
            reason: self.reason.clone(),
            failures: AtomicU64::new(self.failures.load(Ordering::Relaxed)),
        }
    }
}

/// Kind of guard check
#[derive(Clone, Debug)]
pub enum GuardKind {
    /// Check that value equals expected constant
    /// (register_index, expected_value)
    ValueEquals { reg: u8, expected: u64 },
    
    /// Check that value is in range [min, max)
    /// (register_index, min, max)
    ValueInRange { reg: u8, min: u64, max: u64 },
    
    /// Check that pointer is non-null
    /// (register_index)
    NonNull { reg: u8 },
    
    /// Check that memory at address has expected value
    /// Used for type checks (vtable pointer, type tag, etc.)
    MemoryEquals { addr_reg: u8, offset: i32, expected: u64, size: u8 },
    
    /// Check branch direction (from profile)
    /// (expected_taken: true = branch should be taken)
    BranchDirection { expected_taken: bool },
    
    /// Check indirect call target
    /// (target_reg, expected_targets: list of acceptable targets)
    CallTarget { target_reg: u8, expected: Vec<u64> },
    
    /// Check value has specific low bits (tag check for NaN-boxing)
    /// (register_index, mask, expected_bits)
    TagCheck { reg: u8, mask: u64, expected: u64 },
    
    /// Compound guard: all sub-guards must pass
    All(Vec<GuardKind>),
    
    /// Compound guard: any sub-guard passing is sufficient
    Any(Vec<GuardKind>),
}

impl DeoptGuard {
    pub fn new(id: u32, guest_rip: u64, kind: GuardKind, reason: DeoptReason) -> Self {
        Self {
            id,
            guest_rip,
            kind,
            reason,
            failures: AtomicU64::new(0),
        }
    }
    
    pub fn record_failure(&self) -> u64 {
        self.failures.fetch_add(1, Ordering::Relaxed) + 1
    }
    
    pub fn failure_count(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }
    
    /// Check if this guard should be disabled due to too many failures
    pub fn should_disable(&self, threshold: u64) -> bool {
        self.failure_count() >= threshold
    }
}

/// State captured at deoptimization point for interpreter resume
#[derive(Clone, Debug)]
pub struct DeoptState {
    /// Guest RIP to resume at
    pub guest_rip: u64,
    /// Guest general-purpose registers
    pub gprs: [u64; 16],
    /// Guest RFLAGS
    pub rflags: u64,
    /// Stack pointer
    pub rsp: u64,
    /// Deopt reason
    pub reason: DeoptReason,
    /// Guard ID that failed
    pub guard_id: u32,
}

impl DeoptState {
    pub fn new(guest_rip: u64, reason: DeoptReason, guard_id: u32) -> Self {
        Self {
            guest_rip,
            gprs: [0; 16],
            rflags: 0,
            rsp: 0,
            reason,
            guard_id,
        }
    }
}

/// Mapping from native code offset to DeoptState for reconstruction
/// 
/// At each guard location, we need enough metadata to reconstruct
/// the guest state from native register values.
#[derive(Clone, Debug)]
pub struct DeoptMetadata {
    /// Native code offset where this deopt point is
    pub native_offset: u32,
    /// Guest RIP at this point
    pub guest_rip: u64,
    /// Mapping: guest_reg -> native_reg or spill_slot
    pub reg_map: Vec<RegMapping>,
    /// Guard associated with this deopt point
    pub guard_id: u32,
    /// Reason for potential deopt
    pub reason: DeoptReason,
}

/// How to recover a guest register value
#[derive(Clone, Copy, Debug)]
pub enum RegMapping {
    /// Guest reg is in native reg
    InRegister(u8),
    /// Guest reg is on stack at offset from RSP
    OnStack(i32),
    /// Guest reg has constant value
    Constant(u64),
    /// Guest reg value is computed: base + index * scale + offset
    Computed { base: u8, index: Option<u8>, scale: u8, offset: i32 },
}

/// Manager for deoptimization handling
pub struct DeoptManager {
    /// Guard definitions: guard_id -> guard
    guards: RwLock<HashMap<u32, DeoptGuard>>,
    /// Deopt metadata: block_rip -> Vec<DeoptMetadata>
    metadata: RwLock<HashMap<u64, Vec<DeoptMetadata>>>,
    /// Next guard ID
    next_guard_id: AtomicU64,
    /// Statistics
    stats: DeoptStats,
    /// Threshold for disabling speculation
    failure_threshold: u64,
    /// Disabled speculations: (guest_rip, guard_kind_hash) -> disabled
    disabled: RwLock<HashMap<(u64, u64), bool>>,
}

/// Deoptimization statistics
pub struct DeoptStats {
    pub total_deopts: AtomicU64,
    pub type_mismatches: AtomicU64,
    pub value_mismatches: AtomicU64,
    pub range_violations: AtomicU64,
    pub null_pointers: AtomicU64,
    pub branch_mispredicts: AtomicU64,
    pub call_target_mismatches: AtomicU64,
    pub guards_disabled: AtomicU64,
    pub recompilations_triggered: AtomicU64,
}

impl Default for DeoptStats {
    fn default() -> Self {
        Self {
            total_deopts: AtomicU64::new(0),
            type_mismatches: AtomicU64::new(0),
            value_mismatches: AtomicU64::new(0),
            range_violations: AtomicU64::new(0),
            null_pointers: AtomicU64::new(0),
            branch_mispredicts: AtomicU64::new(0),
            call_target_mismatches: AtomicU64::new(0),
            guards_disabled: AtomicU64::new(0),
            recompilations_triggered: AtomicU64::new(0),
        }
    }
}

impl DeoptManager {
    pub fn new(failure_threshold: u64) -> Self {
        Self {
            guards: RwLock::new(HashMap::new()),
            metadata: RwLock::new(HashMap::new()),
            next_guard_id: AtomicU64::new(1),
            stats: DeoptStats::default(),
            failure_threshold,
            disabled: RwLock::new(HashMap::new()),
        }
    }
    
    /// Allocate a new guard ID
    pub fn alloc_guard_id(&self) -> u32 {
        self.next_guard_id.fetch_add(1, Ordering::Relaxed) as u32
    }
    
    /// Register a guard
    pub fn register_guard(&self, guard: DeoptGuard) {
        let id = guard.id;
        self.guards.write().unwrap().insert(id, guard);
    }
    
    /// Register deopt metadata for a compiled block
    pub fn register_metadata(&self, block_rip: u64, metadata: Vec<DeoptMetadata>) {
        self.metadata.write().unwrap().insert(block_rip, metadata);
    }
    
    /// Handle a deoptimization event
    pub fn handle_deopt(&self, guard_id: u32, native_rip: u64) -> Option<DeoptState> {
        self.stats.total_deopts.fetch_add(1, Ordering::Relaxed);
        
        // Find the guard
        let guards = self.guards.read().unwrap();
        let guard = guards.get(&guard_id)?;
        
        // Record failure
        let failure_count = guard.record_failure();
        
        // Update reason-specific stats
        match guard.reason {
            DeoptReason::TypeMismatch => {
                self.stats.type_mismatches.fetch_add(1, Ordering::Relaxed);
            }
            DeoptReason::ValueMismatch => {
                self.stats.value_mismatches.fetch_add(1, Ordering::Relaxed);
            }
            DeoptReason::RangeViolation => {
                self.stats.range_violations.fetch_add(1, Ordering::Relaxed);
            }
            DeoptReason::NullPointer => {
                self.stats.null_pointers.fetch_add(1, Ordering::Relaxed);
            }
            DeoptReason::BranchMispredict => {
                self.stats.branch_mispredicts.fetch_add(1, Ordering::Relaxed);
            }
            DeoptReason::CallTargetMismatch => {
                self.stats.call_target_mismatches.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        
        // Check if should disable this speculation
        if guard.should_disable(self.failure_threshold) {
            self.disable_speculation(guard.guest_rip, &guard.kind);
            self.stats.guards_disabled.fetch_add(1, Ordering::Relaxed);
        }
        
        // Find metadata for state reconstruction
        // (would look up by native_rip in real implementation)
        let _ = native_rip;
        
        // Create deopt state (simplified - real impl reconstructs from metadata)
        Some(DeoptState::new(guard.guest_rip, guard.reason, guard_id))
    }
    
    /// Mark a speculation as disabled (won't be used in future compiles)
    fn disable_speculation(&self, guest_rip: u64, kind: &GuardKind) {
        let kind_hash = self.hash_guard_kind(kind);
        self.disabled.write().unwrap().insert((guest_rip, kind_hash), true);
        log::debug!("[Deopt] Disabled speculation at {:#x} (hash={:#x})", guest_rip, kind_hash);
    }
    
    /// Check if a speculation is disabled
    pub fn is_speculation_disabled(&self, guest_rip: u64, kind: &GuardKind) -> bool {
        let kind_hash = self.hash_guard_kind(kind);
        *self.disabled.read().unwrap().get(&(guest_rip, kind_hash)).unwrap_or(&false)
    }
    
    /// Simple hash for guard kind (for quick lookup)
    fn hash_guard_kind(&self, kind: &GuardKind) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        // Hash the discriminant and key fields
        match kind {
            GuardKind::ValueEquals { reg, expected } => {
                0u8.hash(&mut hasher);
                reg.hash(&mut hasher);
                expected.hash(&mut hasher);
            }
            GuardKind::ValueInRange { reg, min, max } => {
                1u8.hash(&mut hasher);
                reg.hash(&mut hasher);
                min.hash(&mut hasher);
                max.hash(&mut hasher);
            }
            GuardKind::NonNull { reg } => {
                2u8.hash(&mut hasher);
                reg.hash(&mut hasher);
            }
            GuardKind::MemoryEquals { addr_reg, offset, expected, size } => {
                3u8.hash(&mut hasher);
                addr_reg.hash(&mut hasher);
                offset.hash(&mut hasher);
                expected.hash(&mut hasher);
                size.hash(&mut hasher);
            }
            GuardKind::BranchDirection { expected_taken } => {
                4u8.hash(&mut hasher);
                expected_taken.hash(&mut hasher);
            }
            GuardKind::CallTarget { target_reg, expected } => {
                5u8.hash(&mut hasher);
                target_reg.hash(&mut hasher);
                expected.hash(&mut hasher);
            }
            GuardKind::TagCheck { reg, mask, expected } => {
                6u8.hash(&mut hasher);
                reg.hash(&mut hasher);
                mask.hash(&mut hasher);
                expected.hash(&mut hasher);
            }
            GuardKind::All(guards) | GuardKind::Any(guards) => {
                7u8.hash(&mut hasher);
                guards.len().hash(&mut hasher);
            }
        }
        hasher.finish()
    }
    
    /// Get statistics snapshot
    pub fn stats_snapshot(&self) -> DeoptStatsSnapshot {
        DeoptStatsSnapshot {
            total_deopts: self.stats.total_deopts.load(Ordering::Relaxed),
            type_mismatches: self.stats.type_mismatches.load(Ordering::Relaxed),
            value_mismatches: self.stats.value_mismatches.load(Ordering::Relaxed),
            range_violations: self.stats.range_violations.load(Ordering::Relaxed),
            null_pointers: self.stats.null_pointers.load(Ordering::Relaxed),
            branch_mispredicts: self.stats.branch_mispredicts.load(Ordering::Relaxed),
            call_target_mismatches: self.stats.call_target_mismatches.load(Ordering::Relaxed),
            guards_disabled: self.stats.guards_disabled.load(Ordering::Relaxed),
            recompilations_triggered: self.stats.recompilations_triggered.load(Ordering::Relaxed),
        }
    }
    
    /// Clear all guards and metadata for a block (called on recompilation)
    pub fn clear_block(&self, block_rip: u64) {
        // Remove metadata
        self.metadata.write().unwrap().remove(&block_rip);
        
        // Remove guards for this block
        let mut guards = self.guards.write().unwrap();
        guards.retain(|_, g| g.guest_rip != block_rip);
    }
    
    // ========================================================================
    // NReady! Persistence Integration
    // ========================================================================
    
    /// Serialize deoptimization state for NReady! persistence
    /// 
    /// Persists:
    /// - Disabled speculations (to avoid re-speculating on things that fail)
    /// - Guard failure counts (for smarter speculation decisions on next run)
    /// - Stats (for debugging/monitoring)
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Magic + version
        data.extend_from_slice(b"NVMD"); // NVM Deopt
        data.extend_from_slice(&1u32.to_le_bytes());
        
        // Stats
        data.extend_from_slice(&self.stats.total_deopts.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.type_mismatches.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.value_mismatches.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.range_violations.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.null_pointers.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.branch_mispredicts.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.call_target_mismatches.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.guards_disabled.load(Ordering::Relaxed).to_le_bytes());
        data.extend_from_slice(&self.stats.recompilations_triggered.load(Ordering::Relaxed).to_le_bytes());
        
        // Disabled speculations (critical for avoiding repeated failures)
        let disabled = self.disabled.read().unwrap();
        data.extend_from_slice(&(disabled.len() as u32).to_le_bytes());
        for ((rip, kind_hash), &is_disabled) in disabled.iter() {
            if is_disabled {
                data.extend_from_slice(&rip.to_le_bytes());
                data.extend_from_slice(&kind_hash.to_le_bytes());
            }
        }
        drop(disabled);
        
        // Guard failure counts (top N most-failed guards for learning)
        let guards = self.guards.read().unwrap();
        let mut failure_counts: Vec<_> = guards.iter()
            .map(|(id, g)| (*id, g.guest_rip, g.failures.load(Ordering::Relaxed)))
            .filter(|(_, _, f)| *f > 0)
            .collect();
        failure_counts.sort_by(|a, b| b.2.cmp(&a.2)); // Sort by failure count desc
        
        let top_n = failure_counts.iter().take(1000).collect::<Vec<_>>();
        data.extend_from_slice(&(top_n.len() as u32).to_le_bytes());
        for (id, rip, failures) in top_n {
            data.extend_from_slice(&id.to_le_bytes());
            data.extend_from_slice(&rip.to_le_bytes());
            data.extend_from_slice(&failures.to_le_bytes());
        }
        
        data
    }
    
    /// Deserialize deoptimization state from NReady! persistence
    pub fn deserialize(&self, data: &[u8]) -> bool {
        if data.len() < 8 || &data[0..4] != b"NVMD" {
            return false;
        }
        
        let version = u32::from_le_bytes(match data[4..8].try_into() {
            Ok(b) => b,
            Err(_) => return false,
        });
        if version != 1 {
            return false;
        }
        
        let mut offset = 8;
        
        // Stats (72 bytes = 9 * 8)
        if offset + 72 > data.len() { return false; }
        self.stats.total_deopts.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.type_mismatches.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.value_mismatches.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.range_violations.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.null_pointers.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.branch_mispredicts.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.call_target_mismatches.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.guards_disabled.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        self.stats.recompilations_triggered.store(
            u64::from_le_bytes(data[offset..offset+8].try_into().unwrap()), Ordering::Relaxed);
        offset += 8;
        
        // Disabled speculations
        if offset + 4 > data.len() { return false; }
        let disabled_count = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        
        let mut disabled = self.disabled.write().unwrap();
        for _ in 0..disabled_count {
            if offset + 16 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let kind_hash = u64::from_le_bytes(data[offset+8..offset+16].try_into().unwrap());
            disabled.insert((rip, kind_hash), true);
            offset += 16;
        }
        drop(disabled);
        
        // Guard failure counts (informational, don't reconstruct guards)
        // Just log for monitoring
        if offset + 4 <= data.len() {
            let failure_count = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            let mut total_historical_failures = 0u64;
            for _ in 0..failure_count {
                if offset + 20 > data.len() { break; }
                let _id = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap());
                let _rip = u64::from_le_bytes(data[offset+4..offset+12].try_into().unwrap());
                let failures = u64::from_le_bytes(data[offset+12..offset+20].try_into().unwrap());
                total_historical_failures += failures;
                offset += 20;
            }
            
            log::info!("[NReady!] Restored {} disabled speculations, {} historical guard failures",
                disabled_count, total_historical_failures);
        }
        
        true
    }
}

/// Snapshot of deopt statistics
#[derive(Clone, Debug)]
pub struct DeoptStatsSnapshot {
    pub total_deopts: u64,
    pub type_mismatches: u64,
    pub value_mismatches: u64,
    pub range_violations: u64,
    pub null_pointers: u64,
    pub branch_mispredicts: u64,
    pub call_target_mismatches: u64,
    pub guards_disabled: u64,
    pub recompilations_triggered: u64,
}

// ============================================================================
// Native Code Generation for Guards
// ============================================================================

/// Generate native code for a guard check
/// 
/// Returns (guard_code, deopt_stub_offset)
pub fn generate_guard_code(guard: &DeoptGuard, deopt_handler_addr: u64) -> (Vec<u8>, u32) {
    let mut code = Vec::new();
    
    match &guard.kind {
        GuardKind::ValueEquals { reg, expected } => {
            // cmp reg, imm32/64
            // jne deopt_stub
            code.extend_from_slice(&generate_cmp_imm(*reg, *expected));
            let stub_offset = code.len() as u32;
            code.extend_from_slice(&generate_jne_to_deopt(deopt_handler_addr, guard.id));
            (code, stub_offset)
        }
        
        GuardKind::NonNull { reg } => {
            // test reg, reg
            // jz deopt_stub
            code.extend_from_slice(&generate_test_reg(*reg));
            let stub_offset = code.len() as u32;
            code.extend_from_slice(&generate_jz_to_deopt(deopt_handler_addr, guard.id));
            (code, stub_offset)
        }
        
        GuardKind::ValueInRange { reg, min, max } => {
            // cmp reg, min
            // jb deopt_stub
            // cmp reg, max
            // jae deopt_stub
            code.extend_from_slice(&generate_cmp_imm(*reg, *min));
            code.extend_from_slice(&generate_jb_to_deopt(deopt_handler_addr, guard.id));
            code.extend_from_slice(&generate_cmp_imm(*reg, *max));
            let stub_offset = code.len() as u32;
            code.extend_from_slice(&generate_jae_to_deopt(deopt_handler_addr, guard.id));
            (code, stub_offset)
        }
        
        GuardKind::BranchDirection { expected_taken } => {
            // The actual branch comparison happens before; this generates the fallback
            let stub_offset = code.len() as u32;
            if *expected_taken {
                // If we expected taken but not taken, deopt
                code.extend_from_slice(&generate_jmp_to_deopt(deopt_handler_addr, guard.id));
            } else {
                // If we expected not taken but taken, deopt
                code.extend_from_slice(&generate_jmp_to_deopt(deopt_handler_addr, guard.id));
            }
            (code, stub_offset)
        }
        
        GuardKind::CallTarget { target_reg, expected } => {
            // For monomorphic call: cmp target_reg, expected[0]; jne deopt
            // For polymorphic: cmp/je chain
            if expected.len() == 1 {
                code.extend_from_slice(&generate_cmp_imm(*target_reg, expected[0]));
                let stub_offset = code.len() as u32;
                code.extend_from_slice(&generate_jne_to_deopt(deopt_handler_addr, guard.id));
                (code, stub_offset)
            } else {
                // Polymorphic: check each target
                let mut last_offset = 0u32;
                for target in expected {
                    code.extend_from_slice(&generate_cmp_imm(*target_reg, *target));
                    code.extend_from_slice(&[0x74, 0x0A]); // je skip (skip the deopt)
                }
                last_offset = code.len() as u32;
                code.extend_from_slice(&generate_jmp_to_deopt(deopt_handler_addr, guard.id));
                (code, last_offset)
            }
        }
        
        GuardKind::MemoryEquals { addr_reg, offset, expected, size } => {
            // mov tmp, [addr_reg + offset]
            // cmp tmp, expected
            // jne deopt
            code.extend_from_slice(&generate_mem_load(*addr_reg, *offset, *size));
            code.extend_from_slice(&generate_cmp_imm(0 /* RAX as temp */, *expected));
            let stub_offset = code.len() as u32;
            code.extend_from_slice(&generate_jne_to_deopt(deopt_handler_addr, guard.id));
            (code, stub_offset)
        }
        
        GuardKind::TagCheck { reg, mask, expected } => {
            // mov tmp, reg
            // and tmp, mask
            // cmp tmp, expected
            // jne deopt
            code.extend_from_slice(&generate_mov_reg(0, *reg)); // mov rax, reg
            code.extend_from_slice(&generate_and_imm(0, *mask));
            code.extend_from_slice(&generate_cmp_imm(0, *expected));
            let stub_offset = code.len() as u32;
            code.extend_from_slice(&generate_jne_to_deopt(deopt_handler_addr, guard.id));
            (code, stub_offset)
        }
        
        GuardKind::All(guards) | GuardKind::Any(guards) => {
            // Generate code for each sub-guard
            let mut stub_offset = 0u32;
            for sub in guards {
                let sub_guard = DeoptGuard::new(guard.id, guard.guest_rip, sub.clone(), guard.reason);
                let (sub_code, sub_offset) = generate_guard_code(&sub_guard, deopt_handler_addr);
                stub_offset = code.len() as u32 + sub_offset;
                code.extend_from_slice(&sub_code);
            }
            (code, stub_offset)
        }
    }
}

// ============================================================================
// Native Code Generation Helpers
// ============================================================================

/// Generate CMP reg, imm64 (or shorter if possible)
fn generate_cmp_imm(reg: u8, value: u64) -> Vec<u8> {
    let mut code = Vec::new();
    
    if value <= 0x7FFFFFFF {
        // CMP reg, imm32
        if reg >= 8 {
            code.push(0x49); // REX.B
        } else if reg >= 4 {
            code.push(0x48); // REX.W
        }
        code.push(0x81);
        code.push(0xF8 + (reg & 7)); // ModR/M: reg
        code.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        // MOV R11, imm64; CMP reg, R11
        code.push(0x49); code.push(0xBB);
        code.extend_from_slice(&value.to_le_bytes());
        code.push(0x4C); code.push(0x39);
        code.push(0xD8 + (reg & 7));
    }
    
    code
}

/// Generate TEST reg, reg
fn generate_test_reg(reg: u8) -> Vec<u8> {
    let mut code = Vec::new();
    if reg >= 8 {
        code.push(0x4D); // REX.WRB
    } else {
        code.push(0x48); // REX.W
    }
    code.push(0x85);
    code.push(0xC0 + (reg & 7) * 9); // ModR/M for test reg, reg
    code
}

/// Generate JNE to deopt handler
fn generate_jne_to_deopt(handler_addr: u64, guard_id: u32) -> Vec<u8> {
    let mut code = Vec::new();
    // JNE rel32 (will be patched or use direct jump)
    // For now, generate a call to the handler with guard_id
    // 0F 85 rel32
    code.extend_from_slice(&[0x0F, 0x85]);
    // Placeholder offset (will need relocation)
    code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    
    // In real impl, we'd generate:
    // jne .deopt_stub
    // ...
    // .deopt_stub:
    // mov edi, guard_id
    // jmp handler_addr
    let _ = (handler_addr, guard_id);
    code
}

/// Generate JZ to deopt handler
fn generate_jz_to_deopt(handler_addr: u64, guard_id: u32) -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0x0F, 0x84]);
    code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    let _ = (handler_addr, guard_id);
    code
}

/// Generate JB (below, unsigned) to deopt handler
fn generate_jb_to_deopt(handler_addr: u64, guard_id: u32) -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0x0F, 0x82]);
    code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    let _ = (handler_addr, guard_id);
    code
}

/// Generate JAE (above or equal, unsigned) to deopt handler
fn generate_jae_to_deopt(handler_addr: u64, guard_id: u32) -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0x0F, 0x83]);
    code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    let _ = (handler_addr, guard_id);
    code
}

/// Generate unconditional JMP to deopt handler
fn generate_jmp_to_deopt(handler_addr: u64, guard_id: u32) -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(&[0xE9]);
    code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    let _ = (handler_addr, guard_id);
    code
}

/// Generate MOV rax, [addr_reg + offset] with size
fn generate_mem_load(addr_reg: u8, offset: i32, size: u8) -> Vec<u8> {
    let mut code = Vec::new();
    
    // REX prefix
    let rex = 0x48 | if addr_reg >= 8 { 0x01 } else { 0 };
    code.push(rex);
    
    // Opcode based on size
    match size {
        1 => code.push(0x8A), // MOV r8, m8
        2 => { code.insert(0, 0x66); code.push(0x8B); } // MOV r16, m16
        4 => code.push(0x8B), // MOV r32, m32
        8 => code.push(0x8B), // MOV r64, m64
        _ => code.push(0x8B),
    }
    
    // ModR/M + SIB + disp
    if offset == 0 && (addr_reg & 7) != 5 {
        code.push(0x00 + (addr_reg & 7)); // [reg]
    } else if offset >= -128 && offset <= 127 {
        code.push(0x40 + (addr_reg & 7)); // [reg + disp8]
        code.push(offset as i8 as u8);
    } else {
        code.push(0x80 + (addr_reg & 7)); // [reg + disp32]
        code.extend_from_slice(&offset.to_le_bytes());
    }
    
    code
}

/// Generate MOV dst_reg, src_reg
fn generate_mov_reg(dst: u8, src: u8) -> Vec<u8> {
    let mut code = Vec::new();
    let mut rex = 0x48;
    if dst >= 8 { rex |= 0x04; }
    if src >= 8 { rex |= 0x01; }
    code.push(rex);
    code.push(0x89);
    code.push(0xC0 + (src & 7) * 8 + (dst & 7));
    code
}

/// Generate AND reg, imm64
fn generate_and_imm(reg: u8, value: u64) -> Vec<u8> {
    let mut code = Vec::new();
    if value <= 0x7FFFFFFF {
        code.push(0x48);
        code.push(0x81);
        code.push(0xE0 + (reg & 7));
        code.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        // MOV R11, imm64; AND reg, R11
        code.push(0x49); code.push(0xBB);
        code.extend_from_slice(&value.to_le_bytes());
        code.push(0x4C); code.push(0x21);
        code.push(0xD8 + (reg & 7));
    }
    code
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_guard_creation() {
        let guard = DeoptGuard::new(
            1,
            0x1000,
            GuardKind::ValueEquals { reg: 0, expected: 42 },
            DeoptReason::TypeMismatch,
        );
        
        assert_eq!(guard.id, 1);
        assert_eq!(guard.failure_count(), 0);
        
        guard.record_failure();
        assert_eq!(guard.failure_count(), 1);
    }
    
    #[test]
    fn test_deopt_manager() {
        let manager = DeoptManager::new(3);
        
        let guard = DeoptGuard::new(
            manager.alloc_guard_id(),
            0x1000,
            GuardKind::BranchDirection { expected_taken: true },
            DeoptReason::BranchMispredict,
        );
        let guard_id = guard.id;
        
        manager.register_guard(guard);
        
        // Simulate failures
        for _ in 0..3 {
            manager.handle_deopt(guard_id, 0x100);
        }
        
        let stats = manager.stats_snapshot();
        assert_eq!(stats.total_deopts, 3);
        assert_eq!(stats.branch_mispredicts, 3);
        assert_eq!(stats.guards_disabled, 1);
    }
    
    #[test]
    fn test_guard_code_generation() {
        let guard = DeoptGuard::new(
            1,
            0x1000,
            GuardKind::NonNull { reg: 0 },
            DeoptReason::NullPointer,
        );
        
        let (code, _offset) = generate_guard_code(&guard, 0xDEADBEEF);
        assert!(!code.is_empty());
    }
}
