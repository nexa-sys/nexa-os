//! Basic Scheduler Tests
//!
//! Tests for fundamental scheduler data structures and basic operations.

use crate::scheduler::{CpuMask, SchedPolicy};

// ============================================================================
// CpuMask Tests
// ============================================================================

#[test]
fn test_cpu_mask_empty() {
    let mask = CpuMask::empty();
    assert!(mask.is_empty());
    assert_eq!(mask.count(), 0);
    assert!(!mask.is_set(0));
    assert!(!mask.is_set(63));
    assert!(!mask.is_set(64));
    assert!(!mask.is_set(1023));
}

#[test]
fn test_cpu_mask_all() {
    let mask = CpuMask::all();
    assert!(!mask.is_empty());
    // Test first word (0-63)
    for i in 0..64 {
        assert!(mask.is_set(i), "Bit {} should be set", i);
    }
    // Test second word (64-127)
    for i in 64..128 {
        assert!(mask.is_set(i), "Bit {} should be set", i);
    }
    // Test last valid CPU
    assert!(mask.is_set(1023));
}

#[test]
fn test_cpu_mask_set_clear() {
    let mut mask = CpuMask::empty();

    // Set individual bits
    mask.set(0);
    assert!(mask.is_set(0));
    assert_eq!(mask.count(), 1);

    mask.set(63);
    assert!(mask.is_set(63));
    assert_eq!(mask.count(), 2);

    // Set bit in second word
    mask.set(64);
    assert!(mask.is_set(64));
    assert_eq!(mask.count(), 3);

    // Set bit in last word (assuming MAX_CPUS = 1024)
    mask.set(1000);
    assert!(mask.is_set(1000));
    assert_eq!(mask.count(), 4);

    // Clear bits
    mask.clear(0);
    assert!(!mask.is_set(0));
    assert_eq!(mask.count(), 3);

    mask.clear(64);
    assert!(!mask.is_set(64));
    assert_eq!(mask.count(), 2);
}

#[test]
fn test_cpu_mask_first_set() {
    let mut mask = CpuMask::empty();
    assert_eq!(mask.first_set(), None);

    mask.set(42);
    assert_eq!(mask.first_set(), Some(42));

    mask.set(10);
    assert_eq!(mask.first_set(), Some(10));

    // Test with bit in second word only
    let mut mask2 = CpuMask::empty();
    mask2.set(100);
    assert_eq!(mask2.first_set(), Some(100));
}

#[test]
fn test_cpu_mask_iter_set() {
    let mut mask = CpuMask::empty();
    mask.set(5);
    mask.set(10);
    mask.set(64);  // Second word
    mask.set(128); // Third word

    let set_cpus: Vec<usize> = mask.iter_set().collect();
    assert_eq!(set_cpus, vec![5, 10, 64, 128]);
}

#[test]
fn test_cpu_mask_from_u32() {
    let mask = CpuMask::from_u32(0b1010_1010);
    
    assert!(mask.is_set(1));
    assert!(!mask.is_set(0));
    assert!(mask.is_set(3));
    assert!(!mask.is_set(2));
    assert!(mask.is_set(5));
    assert!(mask.is_set(7));
    assert!(!mask.is_set(32));
    
    assert_eq!(mask.count(), 4);
}

#[test]
fn test_cpu_mask_boundary_conditions() {
    let mut mask = CpuMask::empty();
    
    // Test boundaries between words (every 64 bits)
    mask.set(63);
    mask.set(64);
    assert!(mask.is_set(63));
    assert!(mask.is_set(64));
    assert_eq!(mask.count(), 2);
    
    // Test last valid bit
    mask.set(1023);
    assert!(mask.is_set(1023));
    
    // Test out of bounds (should be safe no-op)
    mask.set(1024); // Should be ignored
    assert!(!mask.is_set(1024)); // Should return false for out of bounds
}

// ============================================================================
// SchedPolicy Tests
// ============================================================================

#[test]
fn test_sched_policy_equality() {
    assert_eq!(SchedPolicy::Normal, SchedPolicy::Normal);
    assert_eq!(SchedPolicy::Realtime, SchedPolicy::Realtime);
    assert_eq!(SchedPolicy::Batch, SchedPolicy::Batch);
    assert_eq!(SchedPolicy::Idle, SchedPolicy::Idle);
    
    assert_ne!(SchedPolicy::Normal, SchedPolicy::Realtime);
    assert_ne!(SchedPolicy::Normal, SchedPolicy::Batch);
    assert_ne!(SchedPolicy::Normal, SchedPolicy::Idle);
    assert_ne!(SchedPolicy::Realtime, SchedPolicy::Batch);
}

#[test]
fn test_sched_policy_copy() {
    let policy = SchedPolicy::Realtime;
    let policy_copy = policy;
    assert_eq!(policy, policy_copy);
}

// ============================================================================
// Scheduler Constants Tests
// ============================================================================

#[test]
fn test_scheduler_constants() {
    use crate::scheduler::{
        BASE_SLICE_NS, MAX_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
    };
    
    // Verify constants are reasonable
    assert!(BASE_SLICE_NS > 0);
    assert!(MAX_SLICE_NS > BASE_SLICE_NS);
    assert!(SCHED_GRANULARITY_NS > 0);
    assert!(NICE_0_WEIGHT > 0);
    
    // Base slice should be at least 1ms
    assert!(BASE_SLICE_NS >= 1_000_000);
    
    // Max slice should be at most 1 second
    assert!(MAX_SLICE_NS <= 1_000_000_000);
}

#[test]
fn test_nice_to_weight() {
    use crate::scheduler::nice_to_weight;
    
    // Nice 0 should have base weight
    let weight_0 = nice_to_weight(0);
    assert_eq!(weight_0, 1024);
    
    // Negative nice (higher priority) should have higher weight
    let weight_neg10 = nice_to_weight(-10);
    assert!(weight_neg10 > weight_0);
    
    // Positive nice (lower priority) should have lower weight
    let weight_pos10 = nice_to_weight(10);
    assert!(weight_pos10 < weight_0);
    
    // Extreme values
    let weight_neg20 = nice_to_weight(-20);
    let weight_pos19 = nice_to_weight(19);
    assert!(weight_neg20 > weight_pos19);
    
    // Weight should strictly decrease as nice increases
    for nice in -19..19i8 {
        let weight_curr = nice_to_weight(nice);
        let weight_next = nice_to_weight(nice + 1);
        assert!(
            weight_curr > weight_next,
            "Weight should decrease: nice {} ({}) -> nice {} ({})",
            nice, weight_curr, nice + 1, weight_next
        );
    }
}

// ============================================================================
// Process Entry Tests
// ============================================================================

#[test]
fn test_process_entry_empty() {
    use crate::scheduler::ProcessEntry;
    use crate::process::ProcessState;
    
    let entry = ProcessEntry::empty();
    
    // Check defaults
    assert_eq!(entry.process.pid, 0);
    assert_eq!(entry.process.state, ProcessState::Ready);
    assert_eq!(entry.vruntime, 0);
    assert_eq!(entry.vdeadline, 0);
    assert_eq!(entry.lag, 0);
    assert_eq!(entry.weight, 1024); // NICE_0_WEIGHT
    assert_eq!(entry.policy, SchedPolicy::Normal);
    assert_eq!(entry.nice, 0);
    assert!(!entry.cpu_affinity.is_empty()); // All CPUs by default
}
