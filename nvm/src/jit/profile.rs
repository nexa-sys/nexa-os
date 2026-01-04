//! JIT Profile Database
//!
//! Collects runtime profiling data for JIT compilation decisions:
//! - Basic block execution counts (hot code detection)
//! - Branch target statistics (speculative optimization)
//! - Call target frequencies (inline decisions)
//! - Type profiles (speculation)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::sync::RwLock;

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
    // Serialization for ReadyNow!
    // ========================================================================
    
    /// Serialize profile data for persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Magic number
        data.extend_from_slice(b"NVMP");
        
        // Version
        data.extend_from_slice(&1u32.to_le_bytes());
        
        // Block counts
        let counts = self.block_counts.read().unwrap();
        data.extend_from_slice(&(counts.len() as u32).to_le_bytes());
        for (rip, count) in counts.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.extend_from_slice(&count.load(Ordering::Relaxed).to_le_bytes());
        }
        drop(counts);
        
        // Branch profiles
        let branches = self.branch_profiles.read().unwrap();
        data.extend_from_slice(&(branches.len() as u32).to_le_bytes());
        for (rip, profile) in branches.iter() {
            data.extend_from_slice(&rip.to_le_bytes());
            data.extend_from_slice(&profile.taken.load(Ordering::Relaxed).to_le_bytes());
            data.extend_from_slice(&profile.not_taken.load(Ordering::Relaxed).to_le_bytes());
        }
        
        data
    }
    
    /// Deserialize profile data from persistence
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 8 || &data[0..4] != b"NVMP" {
            return None;
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
        if version != 1 {
            return None;
        }
        
        let db = Self::new(100000);
        let mut offset = 8;
        
        // Block counts
        let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let mut blocks = db.block_counts.write().unwrap();
        for _ in 0..count {
            if offset + 16 > data.len() { break; }
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
            let cnt = u64::from_le_bytes(data[offset+8..offset+16].try_into().ok()?);
            blocks.insert(rip, AtomicU64::new(cnt));
            offset += 16;
        }
        drop(blocks);
        
        // Branch profiles
        if offset + 4 <= data.len() {
            let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
            offset += 4;
            
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
    
    /// Clear all profile data
    pub fn clear(&self) {
        self.block_counts.write().unwrap().clear();
        self.branch_profiles.write().unwrap().clear();
        self.call_profiles.write().unwrap().clear();
        self.loop_profiles.write().unwrap().clear();
        self.memory_profiles.write().unwrap().clear();
    }
    
    /// Statistics
    pub fn stats(&self) -> ProfileStats {
        ProfileStats {
            block_count: self.block_counts.read().unwrap().len(),
            branch_count: self.branch_profiles.read().unwrap().len(),
            call_count: self.call_profiles.read().unwrap().len(),
            loop_count: self.loop_profiles.read().unwrap().len(),
            memory_count: self.memory_profiles.read().unwrap().len(),
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
}

impl ProfileStats {
    pub fn total_entries(&self) -> usize {
        self.block_count + self.branch_count + self.call_count + 
        self.loop_count + self.memory_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_branch_bias() {
        let profile = BranchProfile::default();
        
        // Record mostly taken
        for _ in 0..990 {
            profile.taken.fetch_add(1, Ordering::Relaxed);
        }
        for _ in 0..10 {
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
