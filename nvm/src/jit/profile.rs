//! JIT Profile Database
//!
//! Collects runtime profiling data for JIT compilation decisions:
//! - Basic block execution counts (hot code detection)
//! - Branch target statistics (speculative optimization)
//! - Call target frequencies (inline decisions)
//! - Type profiles (speculation)
//! - Value profiles (value speculation)
//! - Path profiles (multi-condition optimization)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::sync::RwLock;

/// Complete profile data for a basic block
#[derive(Debug, Clone, Default)]
pub struct BlockProfile {
    pub rip: u64,
    pub execution_count: u64,
    pub branch_taken: u64,
    pub branch_not_taken: u64,
    pub call_target: Option<u64>,
    pub call_mono_ratio: Option<f64>,
}

/// Type tag for value classification (for speculation)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueTypeTag {
    /// Unknown type
    Unknown,
    /// Zero value
    Zero,
    /// Small positive integer (fits in imm8)
    SmallPositive,
    /// Positive integer
    Positive,
    /// Negative integer
    Negative,
    /// Pointer (high bits indicate kernel/user space)
    Pointer,
    /// Looks like an address in low memory
    LowAddress,
    /// Boolean-like (0 or 1)
    Boolean,
    /// Aligned address (8-byte aligned)
    Aligned8,
    /// Aligned address (page aligned)
    PageAligned,
}

impl ValueTypeTag {
    /// Classify a value into a type tag
    pub fn classify(value: u64) -> Self {
        if value == 0 {
            ValueTypeTag::Zero
        } else if value == 1 {
            ValueTypeTag::Boolean
        } else if value <= 127 {
            ValueTypeTag::SmallPositive
        } else if value < 0x8000_0000_0000_0000 {
            if value & 0xFFF == 0 {
                ValueTypeTag::PageAligned
            } else if value & 0x7 == 0 {
                ValueTypeTag::Aligned8
            } else if value < 0x1000_0000 {
                ValueTypeTag::LowAddress
            } else {
                ValueTypeTag::Positive
            }
        } else {
            // High bit set - could be pointer or negative
            if value >= 0xFFFF_8000_0000_0000 {
                ValueTypeTag::Pointer // Kernel address range
            } else {
                ValueTypeTag::Negative
            }
        }
    }
}

/// Type profile for a register at a specific location
#[derive(Default)]
pub struct RegisterTypeProfile {
    /// Observed type tags with counts
    tags: RwLock<HashMap<ValueTypeTag, AtomicU64>>,
    /// Total observations
    total: AtomicU64,
}

impl RegisterTypeProfile {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn record(&self, value: u64) {
        let tag = ValueTypeTag::classify(value);
        self.total.fetch_add(1, Ordering::Relaxed);
        
        let tags = self.tags.read().unwrap();
        if let Some(counter) = tags.get(&tag) {
            counter.fetch_add(1, Ordering::Relaxed);
            return;
        }
        drop(tags);
        
        let mut tags = self.tags.write().unwrap();
        tags.entry(tag)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }
    
    /// Get dominant type if confidence > threshold
    pub fn dominant_type(&self, threshold: f64) -> Option<ValueTypeTag> {
        let total = self.total.load(Ordering::Relaxed);
        if total < 100 {
            return None;
        }
        
        let tags = self.tags.read().unwrap();
        tags.iter()
            .max_by_key(|(_, count)| count.load(Ordering::Relaxed))
            .filter(|(_, count)| {
                let c = count.load(Ordering::Relaxed);
                (c as f64 / total as f64) >= threshold
            })
            .map(|(&tag, _)| tag)
    }
    
    pub fn is_monomorphic(&self) -> bool {
        self.dominant_type(0.99).is_some()
    }
}

/// Value profile for tracking specific values at a location
#[derive(Default)]
pub struct RegisterValueProfile {
    /// Value -> count mapping (limited to top N)
    values: RwLock<Vec<(u64, AtomicU64)>>,
    /// Total observations
    total: AtomicU64,
    /// Max tracked distinct values
    max_values: usize,
}

impl RegisterValueProfile {
    pub fn new() -> Self {
        Self {
            values: RwLock::new(Vec::new()),
            total: AtomicU64::new(0),
            max_values: 8,
        }
    }
    
    pub fn with_max(max: usize) -> Self {
        Self {
            values: RwLock::new(Vec::new()),
            total: AtomicU64::new(0),
            max_values: max,
        }
    }
    
    pub fn record(&self, value: u64) {
        self.total.fetch_add(1, Ordering::Relaxed);
        
        // Fast path: check if value exists
        {
            let values = self.values.read().unwrap();
            for (v, count) in values.iter() {
                if *v == value {
                    count.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }
        
        // Slow path: add new value
        let mut values = self.values.write().unwrap();
        // Double-check after acquiring write lock
        for (v, count) in values.iter() {
            if *v == value {
                count.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        
        if values.len() < self.max_values {
            values.push((value, AtomicU64::new(1)));
        }
    }
    
    /// Get dominant value if confidence > threshold
    pub fn dominant_value(&self, threshold: f64) -> Option<(u64, f64)> {
        let total = self.total.load(Ordering::Relaxed);
        if total < 100 {
            return None;
        }
        
        let values = self.values.read().unwrap();
        values.iter()
            .max_by_key(|(_, count)| count.load(Ordering::Relaxed))
            .filter(|(_, count)| {
                let c = count.load(Ordering::Relaxed);
                (c as f64 / total as f64) >= threshold
            })
            .map(|(val, count)| {
                let c = count.load(Ordering::Relaxed);
                (*val, c as f64 / total as f64)
            })
    }
    
    /// Check if values fall within a tight range
    pub fn value_range(&self) -> Option<(u64, u64)> {
        let values = self.values.read().unwrap();
        if values.is_empty() {
            return None;
        }
        
        let min = values.iter().map(|(v, _)| *v).min().unwrap();
        let max = values.iter().map(|(v, _)| *v).max().unwrap();
        
        // Check if range is tight and covers most observations
        let span = max.saturating_sub(min);
        let total = self.total.load(Ordering::Relaxed);
        let covered: u64 = values.iter()
            .map(|(_, c)| c.load(Ordering::Relaxed))
            .sum();
        
        if span <= 256 && (covered as f64 / total as f64) >= 0.90 {
            Some((min, max.saturating_add(1)))
        } else {
            None
        }
    }
    
    /// Check if values appear to be aligned
    pub fn common_alignment(&self) -> Option<u64> {
        let values = self.values.read().unwrap();
        if values.is_empty() {
            return None;
        }
        
        // Find common alignment (8, 16, 4096, etc.)
        let alignments = [4096u64, 64, 16, 8, 4];
        for align in alignments {
            if values.iter().all(|(v, _)| v % align == 0) {
                return Some(align);
            }
        }
        None
    }
}

/// Path profile entry for multi-condition speculation
#[derive(Debug)]
pub struct PathProfileEntry {
    /// Condition values: (reg_index, observed_value)
    pub conditions: Vec<(u8, u64)>,
    /// Times this exact path was taken
    pub count: AtomicU64,
    /// Target RIP after conditions
    pub target_rip: u64,
}

impl Clone for PathProfileEntry {
    fn clone(&self) -> Self {
        Self {
            conditions: self.conditions.clone(),
            count: AtomicU64::new(self.count.load(Ordering::Relaxed)),
            target_rip: self.target_rip,
        }
    }
}

/// Path profile for tracking execution paths
#[derive(Default)]
pub struct PathProfile {
    /// Observed paths
    paths: RwLock<Vec<PathProfileEntry>>,
    /// Total path observations
    total: AtomicU64,
    /// Max tracked paths
    max_paths: usize,
}

impl PathProfile {
    pub fn new() -> Self {
        Self {
            paths: RwLock::new(Vec::new()),
            total: AtomicU64::new(0),
            max_paths: 8,
        }
    }
    
    /// Record a path observation
    pub fn record(&self, conditions: &[(u8, u64)], target_rip: u64) {
        self.total.fetch_add(1, Ordering::Relaxed);
        
        // Check if path exists
        {
            let paths = self.paths.read().unwrap();
            for path in paths.iter() {
                if path.target_rip == target_rip && path.conditions == conditions {
                    path.count.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }
        
        // Add new path
        let mut paths = self.paths.write().unwrap();
        // Double-check
        for path in paths.iter() {
            if path.target_rip == target_rip && path.conditions == conditions {
                path.count.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        
        if paths.len() < self.max_paths {
            paths.push(PathProfileEntry {
                conditions: conditions.to_vec(),
                count: AtomicU64::new(1),
                target_rip,
            });
        }
    }
    
    /// Get dominant path if confidence > threshold
    pub fn dominant_path(&self, threshold: f64) -> Option<(Vec<(u8, u64)>, u64, f64)> {
        let total = self.total.load(Ordering::Relaxed);
        if total < 100 {
            return None;
        }
        
        let paths = self.paths.read().unwrap();
        paths.iter()
            .max_by_key(|p| p.count.load(Ordering::Relaxed))
            .filter(|p| {
                let c = p.count.load(Ordering::Relaxed);
                (c as f64 / total as f64) >= threshold
            })
            .map(|p| {
                let c = p.count.load(Ordering::Relaxed);
                (p.conditions.clone(), p.target_rip, c as f64 / total as f64)
            })
    }
}

/// Profile database for JIT compilation decisions
pub struct ProfileDb {
    /// Block execution counts: rip -> count
    block_counts: RwLock<HashMap<u64, AtomicU64>>,
    
    /// Branch profiles: rip -> (taken_count, not_taken_count)
    branch_profiles: RwLock<HashMap<u64, BranchProfile>>,
    
    /// Call target profiles: call_site_rip -> targets
    call_profiles: RwLock<HashMap<u64, CallProfile>>,
    
    /// Loop iteration profiles: loop_header_rip -> stats
    loop_profiles: RwLock<HashMap<u64, LoopProfile>>,
    
    /// Memory access profiles: rip -> pattern
    memory_profiles: RwLock<HashMap<u64, MemoryProfile>>,
    
    /// Type profiles: (rip, reg) -> type profile
    type_profiles: RwLock<HashMap<(u64, u8), RegisterTypeProfile>>,
    
    /// Value profiles: (rip, reg) -> value profile
    value_profiles: RwLock<HashMap<(u64, u8), RegisterValueProfile>>,
    
    /// Path profiles: entry_rip -> path profile
    path_profiles: RwLock<HashMap<u64, PathProfile>>,
    
    /// Maximum entries per category (prevent unbounded growth)
    max_entries: usize,
}

/// Branch execution profile
#[derive(Default)]
pub struct BranchProfile {
    pub taken: AtomicU64,
    pub not_taken: AtomicU64,
}

impl BranchProfile {
    pub fn bias(&self) -> BranchBias {
        let t = self.taken.load(Ordering::Relaxed);
        let nt = self.not_taken.load(Ordering::Relaxed);
        let total = t + nt;
        
        if total < 100 {
            return BranchBias::Unknown;
        }
        
        let ratio = (t as f64) / (total as f64);
        if ratio > 0.99 {
            BranchBias::AlwaysTaken
        } else if ratio < 0.01 {
            BranchBias::NeverTaken
        } else if ratio > 0.80 {
            BranchBias::MostlyTaken
        } else if ratio < 0.20 {
            BranchBias::MostlyNotTaken
        } else {
            BranchBias::Mixed
        }
    }
    
    pub fn total(&self) -> u64 {
        self.taken.load(Ordering::Relaxed) + self.not_taken.load(Ordering::Relaxed)
    }
}

/// Branch bias classification
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchBias {
    Unknown,
    AlwaysTaken,
    NeverTaken,
    MostlyTaken,
    MostlyNotTaken,
    Mixed,
}

/// Call target profile (for devirtualization)
#[derive(Default)]
pub struct CallProfile {
    /// Target address -> call count
    targets: RwLock<Vec<(u64, AtomicU32)>>,
    max_targets: usize,
}

impl CallProfile {
    pub fn new() -> Self {
        Self {
            targets: RwLock::new(Vec::new()),
            max_targets: 8,
        }
    }
    
    pub fn record(&self, target: u64) {
        let mut targets = self.targets.write().unwrap();
        
        // Find existing entry
        for (t, count) in targets.iter() {
            if *t == target {
                count.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        
        // Add new entry if space
        if targets.len() < self.max_targets {
            targets.push((target, AtomicU32::new(1)));
        }
    }
    
    pub fn dominant_target(&self) -> Option<(u64, f64)> {
        let targets = self.targets.read().unwrap();
        if targets.is_empty() {
            return None;
        }
        
        let total: u32 = targets.iter()
            .map(|(_, c)| c.load(Ordering::Relaxed))
            .sum();
        
        if total < 100 {
            return None;
        }
        
        let mut best = (0u64, 0u32);
        for (t, c) in targets.iter() {
            let count = c.load(Ordering::Relaxed);
            if count > best.1 {
                best = (*t, count);
            }
        }
        
        let ratio = (best.1 as f64) / (total as f64);
        if ratio > 0.90 {
            Some((best.0, ratio))
        } else {
            None
        }
    }
    
    pub fn is_monomorphic(&self) -> bool {
        let targets = self.targets.read().unwrap();
        targets.len() == 1
    }
    
    pub fn is_polymorphic(&self) -> bool {
        let targets = self.targets.read().unwrap();
        targets.len() > 1 && targets.len() <= 4
    }
    
    pub fn is_megamorphic(&self) -> bool {
        let targets = self.targets.read().unwrap();
        targets.len() > 4
    }
}

/// Loop iteration profile
#[derive(Default)]
pub struct LoopProfile {
    /// Total iterations
    pub iterations: AtomicU64,
    /// Number of times loop was entered
    pub entries: AtomicU64,
    /// Trip counts histogram (small counts)
    pub small_trips: [AtomicU32; 16],
}

impl LoopProfile {
    pub fn avg_iterations(&self) -> f64 {
        let iters = self.iterations.load(Ordering::Relaxed) as f64;
        let entries = self.entries.load(Ordering::Relaxed) as f64;
        if entries > 0.0 { iters / entries } else { 0.0 }
    }
    
    pub fn record_iteration(&self) {
        self.iterations.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_entry(&self) {
        self.entries.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_trip_count(&self, count: u64) {
        if count < 16 {
            self.small_trips[count as usize].fetch_add(1, Ordering::Relaxed);
        }
    }
    
    pub fn is_short_loop(&self) -> bool {
        self.avg_iterations() < 16.0
    }
    
    pub fn common_trip_count(&self) -> Option<u64> {
        let mut best = (0usize, 0u32);
        for (i, c) in self.small_trips.iter().enumerate() {
            let count = c.load(Ordering::Relaxed);
            if count > best.1 {
                best = (i, count);
            }
        }
        
        let total: u32 = self.small_trips.iter()
            .map(|c| c.load(Ordering::Relaxed))
            .sum();
        
        if total > 100 && (best.1 as f64) / (total as f64) > 0.80 {
            Some(best.0 as u64)
        } else {
            None
        }
    }
}

/// Memory access pattern profile
#[derive(Default)]
pub struct MemoryProfile {
    /// Access count
    pub count: AtomicU64,
    /// Sequential accesses
    pub sequential: AtomicU64,
    /// Last address accessed
    pub last_addr: AtomicU64,
    /// Stride between accesses
    pub stride: AtomicU64,
    /// Consistent stride count
    pub stride_hits: AtomicU64,
}

impl MemoryProfile {
    pub fn record(&self, addr: u64) {
        let last = self.last_addr.swap(addr, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        
        if last != 0 {
            let diff = addr.wrapping_sub(last);
            let expected_stride = self.stride.load(Ordering::Relaxed);
            
            if diff == expected_stride {
                self.stride_hits.fetch_add(1, Ordering::Relaxed);
            } else {
                self.stride.store(diff, Ordering::Relaxed);
                self.stride_hits.store(0, Ordering::Relaxed);
            }
            
            // Check sequential access
            if diff == 1 || diff == 2 || diff == 4 || diff == 8 {
                self.sequential.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    
    pub fn access_pattern(&self) -> MemoryPattern {
        let count = self.count.load(Ordering::Relaxed);
        let stride_hits = self.stride_hits.load(Ordering::Relaxed);
        let sequential = self.sequential.load(Ordering::Relaxed);
        
        if count < 100 {
            return MemoryPattern::Unknown;
        }
        
        let stride_ratio = (stride_hits as f64) / (count as f64);
        let seq_ratio = (sequential as f64) / (count as f64);
        
        if stride_ratio > 0.90 {
            let stride = self.stride.load(Ordering::Relaxed);
            MemoryPattern::Strided(stride as i64)
        } else if seq_ratio > 0.90 {
            MemoryPattern::Sequential
        } else {
            MemoryPattern::Random
        }
    }
}

/// Memory access pattern classification
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryPattern {
    Unknown,
    Sequential,
    Strided(i64),
    Random,
}

impl ProfileDb {
    pub fn new(max_entries: usize) -> Self {
        Self {
            block_counts: RwLock::new(HashMap::new()),
            branch_profiles: RwLock::new(HashMap::new()),
            call_profiles: RwLock::new(HashMap::new()),
            loop_profiles: RwLock::new(HashMap::new()),
            memory_profiles: RwLock::new(HashMap::new()),
            type_profiles: RwLock::new(HashMap::new()),
            value_profiles: RwLock::new(HashMap::new()),
            path_profiles: RwLock::new(HashMap::new()),
            max_entries,
        }
    }
    
    // ========================================================================
    // Block execution counting
    // ========================================================================
    
    pub fn record_block(&self, rip: u64) {
        let counts = self.block_counts.read().unwrap();
        if let Some(counter) = counts.get(&rip) {
            counter.fetch_add(1, Ordering::Relaxed);
            return;
        }
        drop(counts);
        
        let mut counts = self.block_counts.write().unwrap();
        if counts.len() >= self.max_entries {
            return; // Don't grow unboundedly
        }
        counts.entry(rip).or_insert_with(|| AtomicU64::new(1));
    }
    
    pub fn get_block_count(&self, rip: u64) -> u64 {
        let counts = self.block_counts.read().unwrap();
        counts.get(&rip)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }
    
    pub fn is_hot(&self, rip: u64, threshold: u64) -> bool {
        self.get_block_count(rip) >= threshold
    }
    
    /// Get complete profile data for a block
    pub fn get_block_profile(&self, rip: u64) -> BlockProfile {
        let count = self.get_block_count(rip);
        let branch_stats = self.get_branch_stats(rip);
        let call_target = self.get_call_target(rip);
        
        BlockProfile {
            rip,
            execution_count: count,
            branch_taken: branch_stats.map(|(t, _)| t).unwrap_or(0),
            branch_not_taken: branch_stats.map(|(_, n)| n).unwrap_or(0),
            call_target: call_target.map(|(t, _)| t),
            call_mono_ratio: call_target.map(|(_, r)| r),
        }
    }

    /// Get top N hottest blocks
    pub fn hot_blocks(&self, n: usize) -> Vec<(u64, u64)> {
        let counts = self.block_counts.read().unwrap();
        let mut blocks: Vec<_> = counts.iter()
            .map(|(rip, c)| (*rip, c.load(Ordering::Relaxed)))
            .collect();
        blocks.sort_by(|a, b| b.1.cmp(&a.1));
        blocks.truncate(n);
        blocks
    }
    
    // ========================================================================
    // Branch profiling
    // ========================================================================
    
    pub fn record_branch(&self, rip: u64, taken: bool) {
        let profiles = self.branch_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&rip) {
            if taken {
                profile.taken.fetch_add(1, Ordering::Relaxed);
            } else {
                profile.not_taken.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
        drop(profiles);
        
        let mut profiles = self.branch_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(rip).or_insert_with(BranchProfile::default);
        if taken {
            profile.taken.fetch_add(1, Ordering::Relaxed);
        } else {
            profile.not_taken.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    pub fn get_branch_bias(&self, rip: u64) -> BranchBias {
        let profiles = self.branch_profiles.read().unwrap();
        profiles.get(&rip)
            .map(|p| p.bias())
            .unwrap_or(BranchBias::Unknown)
    }
    
    pub fn get_branch_stats(&self, rip: u64) -> Option<(u64, u64)> {
        let profiles = self.branch_profiles.read().unwrap();
        profiles.get(&rip).map(|p| {
            (p.taken.load(Ordering::Relaxed), p.not_taken.load(Ordering::Relaxed))
        })
    }
    
    // ========================================================================
    // Call profiling
    // ========================================================================
    
    pub fn record_call(&self, call_site: u64, target: u64) {
        let profiles = self.call_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&call_site) {
            profile.record(target);
            return;
        }
        drop(profiles);
        
        let mut profiles = self.call_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(call_site).or_insert_with(CallProfile::new);
        profile.record(target);
    }
    
    pub fn get_call_target(&self, call_site: u64) -> Option<(u64, f64)> {
        let profiles = self.call_profiles.read().unwrap();
        profiles.get(&call_site).and_then(|p| p.dominant_target())
    }
    
    pub fn is_call_monomorphic(&self, call_site: u64) -> bool {
        let profiles = self.call_profiles.read().unwrap();
        profiles.get(&call_site).map(|p| p.is_monomorphic()).unwrap_or(false)
    }
    
    // ========================================================================
    // Loop profiling
    // ========================================================================
    
    pub fn record_loop_entry(&self, header: u64) {
        let profiles = self.loop_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&header) {
            profile.record_entry();
            return;
        }
        drop(profiles);
        
        let mut profiles = self.loop_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(header).or_insert_with(LoopProfile::default);
        profile.record_entry();
    }
    
    pub fn record_loop_iteration(&self, header: u64) {
        let profiles = self.loop_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&header) {
            profile.record_iteration();
        }
    }
    
    pub fn get_loop_avg_iters(&self, header: u64) -> f64 {
        let profiles = self.loop_profiles.read().unwrap();
        profiles.get(&header)
            .map(|p| p.avg_iterations())
            .unwrap_or(0.0)
    }
    
    // ========================================================================
    // Memory profiling
    // ========================================================================
    
    pub fn record_memory_access(&self, rip: u64, addr: u64) {
        let profiles = self.memory_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&rip) {
            profile.record(addr);
            return;
        }
        drop(profiles);
        
        let mut profiles = self.memory_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(rip).or_insert_with(MemoryProfile::default);
        profile.record(addr);
    }
    
    pub fn get_memory_pattern(&self, rip: u64) -> MemoryPattern {
        let profiles = self.memory_profiles.read().unwrap();
        profiles.get(&rip)
            .map(|p| p.access_pattern())
            .unwrap_or(MemoryPattern::Unknown)
    }
    
    // ========================================================================
    // Serialization for NReady! (Enterprise-Grade)
    // ========================================================================
    //
    // Format: Section-based with forward/backward compatibility
    //
    // Header (16 bytes):
    //   [0..4]   Magic: "NVMP"
    //   [4..8]   Version: u32 (current: 2)
    //   [8..12]  Total sections: u32
    //   [12..16] Reserved: u32
    //
    // Each Section:
    //   [0..2]   Section type: u16
    //   [2..6]   Section length: u32 (excluding header)
    //   [6..]    Section data
    //
    // Section types:
    //   0x0001 - Block counts
    //   0x0002 - Branch profiles
    //   0x0010 - Type profiles (V2)
    //   0x0011 - Value profiles (V2)
    //   0x0012 - Path profiles (V2)
    //   0x0013 - Call profiles (V2)
    //   0x00FF - Extension (future)
    //
    // Unknown sections are skipped (forward compatibility)
    // Missing sections use defaults (backward compatibility)
    
    const SECTION_BLOCK_COUNTS: u16 = 0x0001;
    const SECTION_BRANCH_PROFILES: u16 = 0x0002;
    const SECTION_TYPE_PROFILES: u16 = 0x0010;
    const SECTION_VALUE_PROFILES: u16 = 0x0011;
    const SECTION_PATH_PROFILES: u16 = 0x0012;
    const SECTION_CALL_PROFILES: u16 = 0x0013;
    
    /// Serialize profile data for NReady! persistence
    /// 
    /// Uses section-based format for forward/backward compatibility:
    /// - Old readers skip unknown sections
    /// - New readers handle missing sections with defaults
    /// - Full distribution data preserved (not just dominant values)
    pub fn serialize(&self) -> Vec<u8> {
        let mut sections: Vec<(u16, Vec<u8>)> = Vec::new();
        
        // Section 0x0001: Block counts
        sections.push((Self::SECTION_BLOCK_COUNTS, self.serialize_block_counts()));
        
        // Section 0x0002: Branch profiles
        sections.push((Self::SECTION_BRANCH_PROFILES, self.serialize_branch_profiles()));
        
        // Section 0x0010: Type profiles (full distribution)
        sections.push((Self::SECTION_TYPE_PROFILES, self.serialize_type_profiles()));
        
        // Section 0x0011: Value profiles (full distribution)
        sections.push((Self::SECTION_VALUE_PROFILES, self.serialize_value_profiles()));
        
        // Section 0x0012: Path profiles
        sections.push((Self::SECTION_PATH_PROFILES, self.serialize_path_profiles()));
        
        // Section 0x0013: Call profiles (full target list)
        sections.push((Self::SECTION_CALL_PROFILES, self.serialize_call_profiles()));
        
        // Build final buffer
        let mut data = Vec::new();
        
        // Header
        data.extend_from_slice(b"NVMP");
        data.extend_from_slice(&2u32.to_le_bytes()); // Version
        data.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // Reserved
        
        // Sections
        for (section_type, section_data) in sections {
            data.extend_from_slice(&section_type.to_le_bytes());
            data.extend_from_slice(&(section_data.len() as u32).to_le_bytes());
            data.extend_from_slice(&section_data);
        }
        
        data
    }
    
    fn serialize_block_counts(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let counts = self.block_counts.read().unwrap();
        
        data.extend_from_slice(&(counts.len() as u32).to_le_bytes());
        for (rip, count) in counts.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.extend_from_slice(&count.load(Ordering::Relaxed).to_le_bytes());
        }
        
        data
    }
    
    fn serialize_branch_profiles(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let branches = self.branch_profiles.read().unwrap();
        
        data.extend_from_slice(&(branches.len() as u32).to_le_bytes());
        for (rip, profile) in branches.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.extend_from_slice(&profile.taken.load(Ordering::Relaxed).to_le_bytes());
            data.extend_from_slice(&profile.not_taken.load(Ordering::Relaxed).to_le_bytes());
        }
        
        data
    }
    
    fn serialize_type_profiles(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let types = self.type_profiles.read().unwrap();
        
        data.extend_from_slice(&(types.len() as u32).to_le_bytes());
        for ((rip, reg), profile) in types.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.push(*reg);
            data.extend_from_slice(&profile.total.load(Ordering::Relaxed).to_le_bytes());
            
            // Full type distribution (not just dominant)
            let tags = profile.tags.read().unwrap();
            data.extend_from_slice(&(tags.len() as u16).to_le_bytes());
            for (tag, count) in tags.iter() {
                data.push(*tag as u8);
                data.extend_from_slice(&count.load(Ordering::Relaxed).to_le_bytes());
            }
        }
        
        data
    }
    
    fn serialize_value_profiles(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let values = self.value_profiles.read().unwrap();
        
        data.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for ((rip, reg), profile) in values.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.push(*reg);
            data.extend_from_slice(&profile.total.load(Ordering::Relaxed).to_le_bytes());
            
            // Full value distribution
            let vals = profile.values.read().unwrap();
            data.extend_from_slice(&(vals.len() as u16).to_le_bytes());
            for (val, count) in vals.iter() {
                data.extend_from_slice(&val.to_le_bytes());
                data.extend_from_slice(&count.load(Ordering::Relaxed).to_le_bytes());
            }
        }
        
        data
    }
    
    fn serialize_path_profiles(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let paths = self.path_profiles.read().unwrap();
        
        data.extend_from_slice(&(paths.len() as u32).to_le_bytes());
        for (rip, profile) in paths.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.extend_from_slice(&profile.total.load(Ordering::Relaxed).to_le_bytes());
            
            // Full path entries
            let entries = profile.paths.read().unwrap();
            data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
            for entry in entries.iter() {
                // Conditions
                data.extend_from_slice(&(entry.conditions.len() as u16).to_le_bytes());
                for (reg, val) in &entry.conditions {
                    data.push(*reg);
                    data.extend_from_slice(&val.to_le_bytes());
                }
                // Target and count
                data.extend_from_slice(&entry.target_rip.to_le_bytes());
                data.extend_from_slice(&entry.count.load(Ordering::Relaxed).to_le_bytes());
            }
        }
        
        data
    }
    
    fn serialize_call_profiles(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let calls = self.call_profiles.read().unwrap();
        
        data.extend_from_slice(&(calls.len() as u32).to_le_bytes());
        for (rip, profile) in calls.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            
            // Full target list (not just dominant)
            let targets = profile.targets.read().unwrap();
            data.extend_from_slice(&(targets.len() as u16).to_le_bytes());
            for (target, count) in targets.iter() {
                data.extend_from_slice(&target.to_le_bytes());
                data.extend_from_slice(&count.load(Ordering::Relaxed).to_le_bytes());
            }
        }
        
        data
    }
    
    /// Deserialize profile data from NReady! persistence
    /// 
    /// Supports:
    /// - V1 (legacy sequential format) for backward compatibility
    /// - V2 (section-based format) with forward compatibility
    /// 
    /// Unknown sections are skipped, missing sections use defaults.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 16 || &data[0..4] != b"NVMP" {
            return None;
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
        
        match version {
            1 => Self::deserialize_v1(data),
            2 => Self::deserialize_v2(data),
            _ => {
                // Future version: try V2 format (forward compatible)
                log::warn!("[Profile] Unknown version {}, attempting V2 parse", version);
                Self::deserialize_v2(data)
            }
        }
    }
    
    /// Deserialize V1 legacy format (sequential)
    fn deserialize_v1(data: &[u8]) -> Option<Self> {
        let db = Self::new(100000);
        let mut offset = 8;
        
        // Block counts
        if offset + 4 > data.len() { return Some(db); }
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        {
            let mut blocks = db.block_counts.write().unwrap();
            for _ in 0..count {
                if offset + 16 > data.len() { break; }
                let rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
                let cnt = u64::from_le_bytes(data[offset+8..offset+16].try_into().ok()?);
                blocks.insert(rip, AtomicU64::new(cnt));
                offset += 16;
            }
        }
        
        // Branch profiles
        if offset + 4 > data.len() { return Some(db); }
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        {
            let mut branches = db.branch_profiles.write().unwrap();
            for _ in 0..count {
                if offset + 24 > data.len() { break; }
                let rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
                let taken = u64::from_le_bytes(data[offset+8..offset+16].try_into().ok()?);
                let not_taken = u64::from_le_bytes(data[offset+16..offset+24].try_into().ok()?);
                branches.insert(rip, BranchProfile {
                    taken: AtomicU64::new(taken),
                    not_taken: AtomicU64::new(not_taken),
                });
                offset += 24;
            }
        }
        
        Some(db)
    }
    
    /// Deserialize V2 section-based format
    fn deserialize_v2(data: &[u8]) -> Option<Self> {
        if data.len() < 16 { return None; }
        
        let section_count = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
        let db = Self::new(100000);
        let mut offset = 16;
        
        for _ in 0..section_count {
            if offset + 6 > data.len() { break; }
            
            let section_type = u16::from_le_bytes(data[offset..offset+2].try_into().ok()?);
            let section_len = u32::from_le_bytes(data[offset+2..offset+6].try_into().ok()?) as usize;
            offset += 6;
            
            if offset + section_len > data.len() { break; }
            let section_data = &data[offset..offset+section_len];
            
            match section_type {
                Self::SECTION_BLOCK_COUNTS => {
                    db.deserialize_block_counts(section_data);
                }
                Self::SECTION_BRANCH_PROFILES => {
                    db.deserialize_branch_profiles(section_data);
                }
                Self::SECTION_TYPE_PROFILES => {
                    db.deserialize_type_profiles(section_data);
                }
                Self::SECTION_VALUE_PROFILES => {
                    db.deserialize_value_profiles(section_data);
                }
                Self::SECTION_PATH_PROFILES => {
                    db.deserialize_path_profiles(section_data);
                }
                Self::SECTION_CALL_PROFILES => {
                    db.deserialize_call_profiles(section_data);
                }
                _ => {
                    // Unknown section: skip (forward compatibility)
                    log::debug!("[Profile] Skipping unknown section type {:#x}", section_type);
                }
            }
            
            offset += section_len;
        }
        
        Some(db)
    }
    
    fn deserialize_block_counts(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        
        let mut blocks = self.block_counts.write().unwrap();
        for _ in 0..count {
            if offset + 16 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let cnt = u64::from_le_bytes(data[offset+8..offset+16].try_into().unwrap());
            blocks.insert(rip, AtomicU64::new(cnt));
            offset += 16;
        }
    }
    
    fn deserialize_branch_profiles(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        
        let mut branches = self.branch_profiles.write().unwrap();
        for _ in 0..count {
            if offset + 24 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let taken = u64::from_le_bytes(data[offset+8..offset+16].try_into().unwrap());
            let not_taken = u64::from_le_bytes(data[offset+16..offset+24].try_into().unwrap());
            branches.insert(rip, BranchProfile {
                taken: AtomicU64::new(taken),
                not_taken: AtomicU64::new(not_taken),
            });
            offset += 24;
        }
    }
    
    fn deserialize_type_profiles(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        
        let mut types = self.type_profiles.write().unwrap();
        for _ in 0..count {
            if offset + 17 > data.len() { break; } // rip(8) + reg(1) + total(8)
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let reg = data[offset + 8];
            let total = u64::from_le_bytes(data[offset+9..offset+17].try_into().unwrap());
            offset += 17;
            
            let profile = RegisterTypeProfile::new();
            profile.total.store(total, Ordering::Relaxed);
            
            // Tag distribution
            if offset + 2 > data.len() { break; }
            let tag_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;
            
            {
                let mut tags = profile.tags.write().unwrap();
                for _ in 0..tag_count {
                    if offset + 9 > data.len() { break; }
                    let tag_byte = data[offset];
                    let tag_cnt = u64::from_le_bytes(data[offset+1..offset+9].try_into().unwrap());
                    offset += 9;
                    
                    if let Some(tag) = Self::tag_from_u8(tag_byte) {
                        tags.insert(tag, AtomicU64::new(tag_cnt));
                    }
                }
            }
            
            types.insert((rip, reg), profile);
        }
    }
    
    fn tag_from_u8(v: u8) -> Option<ValueTypeTag> {
        match v {
            0 => Some(ValueTypeTag::Unknown),
            1 => Some(ValueTypeTag::Zero),
            2 => Some(ValueTypeTag::SmallPositive),
            3 => Some(ValueTypeTag::Positive),
            4 => Some(ValueTypeTag::Negative),
            5 => Some(ValueTypeTag::Pointer),
            6 => Some(ValueTypeTag::LowAddress),
            7 => Some(ValueTypeTag::Boolean),
            8 => Some(ValueTypeTag::Aligned8),
            9 => Some(ValueTypeTag::PageAligned),
            _ => None,
        }
    }
    
    fn deserialize_value_profiles(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        
        let mut values = self.value_profiles.write().unwrap();
        for _ in 0..count {
            if offset + 17 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let reg = data[offset + 8];
            let total = u64::from_le_bytes(data[offset+9..offset+17].try_into().unwrap());
            offset += 17;
            
            let profile = RegisterValueProfile::new();
            profile.total.store(total, Ordering::Relaxed);
            
            // Value distribution
            if offset + 2 > data.len() { break; }
            let val_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;
            
            {
                let mut vals = profile.values.write().unwrap();
                for _ in 0..val_count {
                    if offset + 16 > data.len() { break; }
                    let val = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                    let cnt = u64::from_le_bytes(data[offset+8..offset+16].try_into().unwrap());
                    offset += 16;
                    vals.push((val, AtomicU64::new(cnt)));
                }
            }
            
            values.insert((rip, reg), profile);
        }
    }
    
    fn deserialize_path_profiles(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        
        let mut paths = self.path_profiles.write().unwrap();
        for _ in 0..count {
            if offset + 16 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let total = u64::from_le_bytes(data[offset+8..offset+16].try_into().unwrap());
            offset += 16;
            
            let profile = PathProfile::new();
            profile.total.store(total, Ordering::Relaxed);
            
            // Path entries
            if offset + 2 > data.len() { break; }
            let entry_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;
            
            {
                let mut entries = profile.paths.write().unwrap();
                for _ in 0..entry_count {
                    if offset + 2 > data.len() { break; }
                    let cond_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
                    offset += 2;
                    
                    let mut conditions = Vec::with_capacity(cond_count);
                    for _ in 0..cond_count {
                        if offset + 9 > data.len() { break; }
                        let reg = data[offset];
                        let val = u64::from_le_bytes(data[offset+1..offset+9].try_into().unwrap());
                        offset += 9;
                        conditions.push((reg, val));
                    }
                    
                    if offset + 16 > data.len() { break; }
                    let target_rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                    let cnt = u64::from_le_bytes(data[offset+8..offset+16].try_into().unwrap());
                    offset += 16;
                    
                    entries.push(PathProfileEntry {
                        conditions,
                        count: AtomicU64::new(cnt),
                        target_rip,
                    });
                }
            }
            
            paths.insert(rip, profile);
        }
    }
    
    fn deserialize_call_profiles(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        
        let mut calls = self.call_profiles.write().unwrap();
        for _ in 0..count {
            if offset + 8 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            offset += 8;
            
            let profile = CallProfile::new();
            
            // Target distribution
            if offset + 2 > data.len() { break; }
            let target_count = u16::from_le_bytes(data[offset..offset+2].try_into().unwrap()) as usize;
            offset += 2;
            
            {
                let mut targets = profile.targets.write().unwrap();
                for _ in 0..target_count {
                    if offset + 12 > data.len() { break; }
                    let target = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
                    let cnt = u32::from_le_bytes(data[offset+8..offset+12].try_into().unwrap());
                    offset += 12;
                    targets.push((target, AtomicU32::new(cnt)));
                }
            }
            
            calls.insert(rip, profile);
        }
    }
    
    /// Clear all profile data
    pub fn clear(&self) {
        self.block_counts.write().unwrap().clear();
        self.branch_profiles.write().unwrap().clear();
        self.call_profiles.write().unwrap().clear();
        self.loop_profiles.write().unwrap().clear();
        self.memory_profiles.write().unwrap().clear();
        self.type_profiles.write().unwrap().clear();
        self.value_profiles.write().unwrap().clear();
        self.path_profiles.write().unwrap().clear();
    }
    
    /// Get the number of profiled blocks
    pub fn block_count(&self) -> usize {
        self.block_counts.read().unwrap().len()
    }
    
    /// Get RIPs of hot blocks (execution count >= threshold)
    /// 
    /// Returns a list of block RIPs sorted by execution count (hottest first).
    pub fn hot_block_rips(&self, threshold: u64) -> Vec<u64> {
        let counts = self.block_counts.read().unwrap();
        let mut hot_blocks: Vec<(u64, u64)> = counts.iter()
            .map(|(rip, count)| (*rip, count.load(Ordering::Relaxed)))
            .filter(|(_, count)| *count >= threshold)
            .collect();
        
        // Sort by count descending (hottest first)
        hot_blocks.sort_by(|a, b| b.1.cmp(&a.1));
        
        hot_blocks.into_iter().map(|(rip, _)| rip).collect()
    }
    
    // ========================================================================
    // Type Profiling (for speculation)
    // ========================================================================
    
    /// Record a type observation for a register at a specific RIP
    pub fn record_type(&self, rip: u64, reg: u8, value: u64) {
        let key = (rip, reg);
        
        let profiles = self.type_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&key) {
            profile.record(value);
            return;
        }
        drop(profiles);
        
        let mut profiles = self.type_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(key).or_insert_with(RegisterTypeProfile::new);
        profile.record(value);
    }
    
    /// Get dominant type for a register at a specific RIP
    pub fn get_dominant_type(&self, rip: u64, reg: u8, threshold: f64) -> Option<ValueTypeTag> {
        let profiles = self.type_profiles.read().unwrap();
        profiles.get(&(rip, reg))
            .and_then(|p| p.dominant_type(threshold))
    }
    
    /// Check if register type is monomorphic
    pub fn is_type_monomorphic(&self, rip: u64, reg: u8) -> bool {
        let profiles = self.type_profiles.read().unwrap();
        profiles.get(&(rip, reg))
            .map(|p| p.is_monomorphic())
            .unwrap_or(false)
    }
    
    // ========================================================================
    // Value Profiling (for value speculation)
    // ========================================================================
    
    /// Record a value observation for a register at a specific RIP
    pub fn record_value(&self, rip: u64, reg: u8, value: u64) {
        let key = (rip, reg);
        
        let profiles = self.value_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&key) {
            profile.record(value);
            return;
        }
        drop(profiles);
        
        let mut profiles = self.value_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(key).or_insert_with(RegisterValueProfile::new);
        profile.record(value);
    }
    
    /// Get dominant value for a register at a specific RIP
    pub fn get_dominant_value(&self, rip: u64, reg: u8, threshold: f64) -> Option<(u64, f64)> {
        let profiles = self.value_profiles.read().unwrap();
        profiles.get(&(rip, reg))
            .and_then(|p| p.dominant_value(threshold))
    }
    
    /// Get value range for a register at a specific RIP
    pub fn get_value_range(&self, rip: u64, reg: u8) -> Option<(u64, u64)> {
        let profiles = self.value_profiles.read().unwrap();
        profiles.get(&(rip, reg))
            .and_then(|p| p.value_range())
    }
    
    /// Get common alignment for values at a specific RIP
    pub fn get_value_alignment(&self, rip: u64, reg: u8) -> Option<u64> {
        let profiles = self.value_profiles.read().unwrap();
        profiles.get(&(rip, reg))
            .and_then(|p| p.common_alignment())
    }
    
    // ========================================================================
    // Path Profiling (for multi-condition speculation)
    // ========================================================================
    
    /// Record a path observation
    pub fn record_path(&self, entry_rip: u64, conditions: &[(u8, u64)], target_rip: u64) {
        let profiles = self.path_profiles.read().unwrap();
        if let Some(profile) = profiles.get(&entry_rip) {
            profile.record(conditions, target_rip);
            return;
        }
        drop(profiles);
        
        let mut profiles = self.path_profiles.write().unwrap();
        if profiles.len() >= self.max_entries {
            return;
        }
        
        let profile = profiles.entry(entry_rip).or_insert_with(PathProfile::new);
        profile.record(conditions, target_rip);
    }
    
    /// Get dominant path for an entry RIP
    pub fn get_dominant_path(&self, entry_rip: u64, threshold: f64) -> Option<(Vec<(u8, u64)>, u64, f64)> {
        let profiles = self.path_profiles.read().unwrap();
        profiles.get(&entry_rip)
            .and_then(|p| p.dominant_path(threshold))
    }
    
    /// Merge another profile database into this one
    /// 
    /// This is used by NReady! to combine persisted profile data
    /// with runtime data. Values are added together for counters.
    pub fn merge(&self, other: &ProfileDb) {
        // Merge block counts
        let other_blocks = other.block_counts.read().unwrap();
        let mut our_blocks = self.block_counts.write().unwrap();
        for (rip, count) in other_blocks.iter() {
            let other_count = count.load(Ordering::Relaxed);
            our_blocks.entry(*rip)
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(other_count, Ordering::Relaxed);
        }
        drop(other_blocks);
        drop(our_blocks);
        
        // Merge branch profiles
        let other_branches = other.branch_profiles.read().unwrap();
        let mut our_branches = self.branch_profiles.write().unwrap();
        for (rip, profile) in other_branches.iter() {
            let entry = our_branches.entry(*rip)
                .or_insert_with(BranchProfile::default);
            entry.taken.fetch_add(profile.taken.load(Ordering::Relaxed), Ordering::Relaxed);
            entry.not_taken.fetch_add(profile.not_taken.load(Ordering::Relaxed), Ordering::Relaxed);
        }
    }
    
    /// Statistics
    pub fn stats(&self) -> ProfileStats {
        ProfileStats {
            block_count: self.block_counts.read().unwrap().len(),
            branch_count: self.branch_profiles.read().unwrap().len(),
            call_count: self.call_profiles.read().unwrap().len(),
            loop_count: self.loop_profiles.read().unwrap().len(),
            memory_count: self.memory_profiles.read().unwrap().len(),
            type_count: self.type_profiles.read().unwrap().len(),
            value_count: self.value_profiles.read().unwrap().len(),
            path_count: self.path_profiles.read().unwrap().len(),
        }
    }
}

/// Profile database statistics
#[derive(Debug, Clone)]
pub struct ProfileStats {
    pub block_count: usize,
    pub branch_count: usize,
    pub call_count: usize,
    pub loop_count: usize,
    pub memory_count: usize,
    pub type_count: usize,
    pub value_count: usize,
    pub path_count: usize,
}

impl ProfileStats {
    pub fn total_entries(&self) -> usize {
        self.block_count + self.branch_count + self.call_count + 
        self.loop_count + self.memory_count + self.type_count +
        self.value_count + self.path_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_branch_bias() {
        let profile = BranchProfile::default();
        
        // Record mostly taken (99.1% > 99% threshold)
        for _ in 0..991 {
            profile.taken.fetch_add(1, Ordering::Relaxed);
        }
        for _ in 0..9 {
            profile.not_taken.fetch_add(1, Ordering::Relaxed);
        }
        
        assert_eq!(profile.bias(), BranchBias::AlwaysTaken);
    }
    
    #[test]
    fn test_call_profile() {
        let profile = CallProfile::new();
        
        // Record monomorphic calls
        for _ in 0..1000 {
            profile.record(0x1000);
        }
        
        assert!(profile.is_monomorphic());
        let (target, ratio) = profile.dominant_target().unwrap();
        assert_eq!(target, 0x1000);
        assert!(ratio > 0.99);
    }
    
    #[test]
    fn test_profile_serialization() {
        let db = ProfileDb::new(1000);
        
        db.record_block(0x1000);
        db.record_block(0x1000);
        db.record_branch(0x1010, true);
        db.record_branch(0x1010, false);
        
        let data = db.serialize();
        let restored = ProfileDb::deserialize(&data).unwrap();
        
        assert_eq!(restored.get_block_count(0x1000), 2);
        assert_eq!(restored.get_branch_stats(0x1010), Some((1, 1)));
    }
}
