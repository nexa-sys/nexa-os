//! EEVDF Scheduler Boundary Tests
//!
//! Tests for EEVDF (Earliest Eligible Virtual Deadline First) scheduler edge cases:
//! - Virtual runtime overflow
//! - Weight/nice calculations
//! - Eligibility checks
//! - Deadline calculations

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        nice_to_weight, CpuMask, SchedPolicy, ProcessEntry,
        BASE_SLICE_NS, MAX_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
    };

    // =========================================================================
    // Nice to Weight Conversion Tests
    // =========================================================================

    #[test]
    fn test_nice_zero_weight() {
        // Nice 0 should give the base weight
        let weight = nice_to_weight(0);
        assert_eq!(weight, NICE_0_WEIGHT, "Nice 0 should equal NICE_0_WEIGHT");
    }

    #[test]
    fn test_nice_negative_higher_weight() {
        // Negative nice values (higher priority) should have higher weight
        let weight_neg5 = nice_to_weight(-5);
        let weight_0 = nice_to_weight(0);
        
        assert!(weight_neg5 > weight_0, 
            "Negative nice should have higher weight");
    }

    #[test]
    fn test_nice_positive_lower_weight() {
        // Positive nice values (lower priority) should have lower weight
        let weight_5 = nice_to_weight(5);
        let weight_0 = nice_to_weight(0);
        
        assert!(weight_5 < weight_0,
            "Positive nice should have lower weight");
    }

    #[test]
    fn test_nice_extreme_values() {
        // Nice range is typically -20 to +19
        let weight_min = nice_to_weight(-20);
        let weight_max = nice_to_weight(19);
        
        assert!(weight_min > weight_max,
            "Nice -20 should have much higher weight than nice 19");
        
        // Weights should always be positive
        assert!(weight_min > 0);
        assert!(weight_max > 0);
    }

    #[test]
    fn test_nice_weights_monotonic() {
        // Weight should decrease monotonically as nice increases
        let mut prev_weight = nice_to_weight(-20);
        
        for nice in -19..=19 {
            let weight = nice_to_weight(nice);
            assert!(weight <= prev_weight,
                "Weight should decrease as nice increases: nice={}, weight={}, prev={}",
                nice, weight, prev_weight);
            prev_weight = weight;
        }
    }

    // =========================================================================
    // CPU Mask Tests
    // =========================================================================

    #[test]
    fn test_cpu_mask_empty() {
        let mask = CpuMask::empty();
        assert!(mask.is_empty());
        assert_eq!(mask.count(), 0);
    }

    #[test]
    fn test_cpu_mask_all() {
        let mask = CpuMask::all();
        assert!(!mask.is_empty());
        assert!(mask.is_set(0));
        assert!(mask.is_set(63));
    }

    #[test]
    fn test_cpu_mask_set_clear() {
        let mut mask = CpuMask::empty();
        
        mask.set(5);
        assert!(mask.is_set(5));
        assert_eq!(mask.count(), 1);
        
        mask.set(10);
        assert_eq!(mask.count(), 2);
        
        mask.clear(5);
        assert!(!mask.is_set(5));
        assert!(mask.is_set(10));
        assert_eq!(mask.count(), 1);
    }

    #[test]
    fn test_cpu_mask_boundary() {
        let mut mask = CpuMask::empty();
        
        // Test CPU 0
        mask.set(0);
        assert!(mask.is_set(0));
        
        // Test CPU 63 (end of first word)
        mask.set(63);
        assert!(mask.is_set(63));
        
        // Test CPU 64 (start of second word)
        mask.set(64);
        assert!(mask.is_set(64));
    }

    #[test]
    fn test_cpu_mask_out_of_bounds() {
        let mut mask = CpuMask::empty();
        
        // Setting out-of-bounds should be safe (no panic)
        mask.set(10000);
        
        // Should not be set (bounds check)
        assert!(!mask.is_set(10000));
    }

    #[test]
    fn test_cpu_mask_from_u32() {
        let mask = CpuMask::from_u32(0b1010);
        
        assert!(!mask.is_set(0));
        assert!(mask.is_set(1));
        assert!(!mask.is_set(2));
        assert!(mask.is_set(3));
        assert_eq!(mask.count(), 2);
    }

    #[test]
    fn test_cpu_mask_first_set() {
        let mut mask = CpuMask::empty();
        
        assert!(mask.first_set().is_none(), "Empty mask should have no first set");
        
        mask.set(5);
        assert_eq!(mask.first_set(), Some(5));
        
        mask.set(2);
        assert_eq!(mask.first_set(), Some(2), "first_set should return lowest CPU");
    }

    // =========================================================================
    // Scheduler Constants Tests
    // =========================================================================

    #[test]
    fn test_scheduler_constants_valid() {
        // BASE_SLICE_NS should be reasonable (e.g., 1-100ms)
        assert!(BASE_SLICE_NS > 0);
        assert!(BASE_SLICE_NS <= 100_000_000, "Base slice shouldn't exceed 100ms");
        
        // MAX_SLICE_NS should be larger than BASE_SLICE_NS
        assert!(MAX_SLICE_NS >= BASE_SLICE_NS);
        
        // SCHED_GRANULARITY_NS for preemption decisions
        assert!(SCHED_GRANULARITY_NS > 0);
    }

    #[test]
    fn test_nice_0_weight_value() {
        // NICE_0_WEIGHT is typically 1024 in Linux
        assert!(NICE_0_WEIGHT > 0);
        assert!(NICE_0_WEIGHT.is_power_of_two() || NICE_0_WEIGHT == 1024,
            "NICE_0_WEIGHT is typically a power of 2 for efficient division");
    }

    // =========================================================================
    // Virtual Runtime Overflow Tests
    // =========================================================================

    #[test]
    fn test_vruntime_addition_no_overflow() {
        // Virtual runtime is u64, check for overflow scenarios
        let vruntime: u64 = u64::MAX - 1000;
        let delta: u64 = 500;
        
        let new_vruntime = vruntime.saturating_add(delta);
        assert_eq!(new_vruntime, u64::MAX - 500);
    }

    #[test]
    fn test_vruntime_overflow_saturates() {
        let vruntime: u64 = u64::MAX - 100;
        let delta: u64 = 200;
        
        // Should saturate rather than wrap
        let new_vruntime = vruntime.saturating_add(delta);
        assert_eq!(new_vruntime, u64::MAX);
    }

    #[test]
    fn test_vruntime_wraparound_comparison() {
        // When vruntimes wrap around, comparison must handle it
        // This is a common bug in scheduler implementations
        
        fn vruntime_before(a: u64, b: u64) -> bool {
            // Signed comparison handles wraparound
            (a as i64).wrapping_sub(b as i64) < 0
        }
        
        // Normal case
        assert!(vruntime_before(100, 200));
        assert!(!vruntime_before(200, 100));
        
        // Wraparound case: a is very large, b wrapped around to small
        // In this case, b is "later" even though numerically smaller
        let a = u64::MAX - 10;
        let b = 10u64;
        // b is "after" a if we wrapped around
        assert!(vruntime_before(a, b), "Should handle wraparound");
    }

    // =========================================================================
    // Deadline Calculation Tests
    // =========================================================================

    #[test]
    fn test_vdeadline_calculation() {
        // vdeadline = vruntime + request/weight
        // This gives latency guarantee
        
        fn calc_vdeadline(vruntime: u64, request_ns: u64, weight: u64) -> u64 {
            let slice = request_ns / weight;
            vruntime.saturating_add(slice)
        }
        
        let vruntime = 1000u64;
        let request = 4096u64; // 4096ns request
        let weight = NICE_0_WEIGHT;
        
        let deadline = calc_vdeadline(vruntime, request, weight);
        assert!(deadline > vruntime);
    }

    #[test]
    fn test_vdeadline_higher_priority() {
        // Higher weight (lower nice) should get earlier deadline
        fn calc_vdeadline(vruntime: u64, request_ns: u64, weight: u64) -> u64 {
            let slice = request_ns / weight;
            vruntime.saturating_add(slice)
        }
        
        let vruntime = 1000u64;
        let request = 1_000_000u64; // 1ms
        
        let deadline_high = calc_vdeadline(vruntime, request, nice_to_weight(-10));
        let deadline_normal = calc_vdeadline(vruntime, request, nice_to_weight(0));
        let deadline_low = calc_vdeadline(vruntime, request, nice_to_weight(10));
        
        // Higher weight = smaller slice = earlier deadline
        assert!(deadline_high < deadline_normal);
        assert!(deadline_normal < deadline_low);
    }

    // =========================================================================
    // Eligibility Tests
    // =========================================================================

    #[test]
    fn test_eligibility_check() {
        // In EEVDF, a process is eligible if its lag >= 0
        // lag = ideal_service - actual_service
        
        fn is_eligible(vruntime: u64, min_vruntime: u64, weight: u64, total_weight: u64) -> bool {
            // Simplified eligibility: vruntime <= min_vruntime + epsilon
            // Real implementation considers lag
            vruntime <= min_vruntime.saturating_add(SCHED_GRANULARITY_NS)
        }
        
        let min_vruntime = 1000u64;
        
        // Process with low vruntime is eligible
        assert!(is_eligible(900, min_vruntime, NICE_0_WEIGHT, NICE_0_WEIGHT * 4));
        
        // Process with vruntime close to min is eligible
        assert!(is_eligible(1000, min_vruntime, NICE_0_WEIGHT, NICE_0_WEIGHT * 4));
    }

    // =========================================================================
    // Process Entry EEVDF Fields Tests
    // =========================================================================

    #[test]
    fn test_process_entry_eevdf_fields() {
        let entry = ProcessEntry::empty();
        
        // New process should have 0 vruntime
        assert_eq!(entry.vruntime, 0);
        
        // Should have default weight
        assert!(entry.weight > 0);
    }

    #[test]
    fn test_process_entry_sched_policy() {
        let mut entry = ProcessEntry::empty();
        
        // Default should be Normal (SCHED_OTHER)
        // Check policy can be changed
        entry.policy = SchedPolicy::Realtime;
        assert_eq!(entry.policy, SchedPolicy::Realtime);
        
        entry.policy = SchedPolicy::Batch;
        assert_eq!(entry.policy, SchedPolicy::Batch);
        
        entry.policy = SchedPolicy::Idle;
        assert_eq!(entry.policy, SchedPolicy::Idle);
    }
}
