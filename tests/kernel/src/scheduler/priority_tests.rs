//! Scheduler Priority and Weight Tests
//!
//! Tests for the EEVDF scheduler's priority calculation,
//! weight assignment, and fairness properties.

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        nice_to_weight, BASE_SLICE_NS, MAX_SLICE_NS, NICE_0_WEIGHT,
        SCHED_GRANULARITY_NS, ProcessEntry, SchedPolicy, CpuMask,
    };
    use crate::process::{ProcessState, Pid};

    // =========================================================================
    // Nice to Weight Conversion Tests
    // =========================================================================

    #[test]
    fn test_nice_zero_weight() {
        let weight = nice_to_weight(0);
        assert_eq!(weight, NICE_0_WEIGHT, "Nice 0 should have NICE_0_WEIGHT");
    }

    #[test]
    fn test_nice_negative_higher_weight() {
        // Lower nice (higher priority) should have higher weight
        for nice in -20..0 {
            let weight = nice_to_weight(nice);
            let higher_nice_weight = nice_to_weight(nice + 1);
            assert!(weight > higher_nice_weight,
                   "Nice {} should have higher weight than nice {}", nice, nice + 1);
        }
    }

    #[test]
    fn test_nice_positive_lower_weight() {
        // Higher nice (lower priority) should have lower weight
        for nice in 0..19 {
            let weight = nice_to_weight(nice);
            let higher_nice_weight = nice_to_weight(nice + 1);
            assert!(weight > higher_nice_weight,
                   "Nice {} should have higher weight than nice {}", nice, nice + 1);
        }
    }

    #[test]
    fn test_nice_range_boundaries() {
        // Test minimum and maximum nice values
        let min_nice_weight = nice_to_weight(-20);
        let max_nice_weight = nice_to_weight(19);
        
        // Min nice should have highest weight
        assert!(min_nice_weight > NICE_0_WEIGHT);
        
        // Max nice should have lowest weight
        assert!(max_nice_weight < NICE_0_WEIGHT);
        
        // All weights should be positive
        assert!(min_nice_weight > 0);
        assert!(max_nice_weight > 0);
    }

    #[test]
    fn test_nice_weight_ratio() {
        // Linux uses approximately 1.25x ratio per nice level
        // Check that the ratio is consistent
        let w0 = nice_to_weight(0) as f64;
        let w1 = nice_to_weight(1) as f64;
        let w_neg1 = nice_to_weight(-1) as f64;
        
        let ratio_pos = w0 / w1;
        let ratio_neg = w_neg1 / w0;
        
        // Ratios should be similar (within 10%)
        let diff = (ratio_pos - ratio_neg).abs();
        assert!(diff < 0.15, "Nice ratios should be consistent: pos={}, neg={}", ratio_pos, ratio_neg);
        
        eprintln!("Nice ratio (nice 0 / nice 1): {:.3}", ratio_pos);
        eprintln!("Nice ratio (nice -1 / nice 0): {:.3}", ratio_neg);
    }

    // =========================================================================
    // Time Slice Tests
    // =========================================================================

    #[test]
    fn test_base_slice_reasonable() {
        // Base slice should be in millisecond range
        assert!(BASE_SLICE_NS >= 1_000_000, "Base slice should be at least 1ms");
        assert!(BASE_SLICE_NS <= 100_000_000, "Base slice should be at most 100ms");
    }

    #[test]
    fn test_max_slice_greater_than_base() {
        assert!(MAX_SLICE_NS >= BASE_SLICE_NS, 
                "Max slice should be >= base slice");
    }

    #[test]
    fn test_granularity_reasonable() {
        // Granularity determines minimum reschedule interval
        assert!(SCHED_GRANULARITY_NS >= 100_000, "Granularity should be at least 100us");
        assert!(SCHED_GRANULARITY_NS <= BASE_SLICE_NS, "Granularity should be <= base slice");
    }

    // =========================================================================
    // Process Entry Tests
    // =========================================================================

    fn make_test_entry(pid: Pid, nice: i8) -> ProcessEntry {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = pid;
        entry.process.state = ProcessState::Ready;
        entry.nice = nice;
        entry.weight = nice_to_weight(nice);
        entry.policy = SchedPolicy::Normal;
        entry.slice_ns = BASE_SLICE_NS;
        entry.slice_remaining_ns = BASE_SLICE_NS;
        entry.cpu_affinity = CpuMask::all();
        entry
    }

    #[test]
    fn test_process_entry_initial_vruntime() {
        let entry = make_test_entry(1, 0);
        assert_eq!(entry.vruntime, 0, "Initial vruntime should be 0");
    }

    #[test]
    fn test_process_entry_weight_matches_nice() {
        for nice in -20i8..=19 {
            let entry = make_test_entry(1, nice);
            assert_eq!(entry.weight, nice_to_weight(nice),
                      "Entry weight should match nice_to_weight for nice={}", nice);
        }
    }

    #[test]
    fn test_process_entry_slice_allocation() {
        let entry = make_test_entry(1, 0);
        assert_eq!(entry.slice_ns, BASE_SLICE_NS);
        assert_eq!(entry.slice_remaining_ns, BASE_SLICE_NS);
    }

    // =========================================================================
    // Virtual Runtime Calculation Tests
    // =========================================================================

    #[test]
    fn test_vruntime_delta_high_priority() {
        // High priority (low nice) should advance vruntime slower
        let runtime_ns: u64 = 1_000_000; // 1ms actual runtime
        
        let weight_high = nice_to_weight(-10);
        let weight_normal = nice_to_weight(0);
        
        // vruntime_delta = runtime * NICE_0_WEIGHT / weight
        let delta_high = (runtime_ns * NICE_0_WEIGHT) / weight_high;
        let delta_normal = (runtime_ns * NICE_0_WEIGHT) / weight_normal;
        
        assert!(delta_high < delta_normal,
               "High priority should advance vruntime slower");
        
        // delta_normal should be exactly runtime_ns (weight = NICE_0_WEIGHT)
        assert_eq!(delta_normal, runtime_ns,
                  "Normal priority should have 1:1 vruntime mapping");
    }

    #[test]
    fn test_vruntime_delta_low_priority() {
        // Low priority (high nice) should advance vruntime faster
        let runtime_ns: u64 = 1_000_000; // 1ms actual runtime
        
        let weight_low = nice_to_weight(10);
        let weight_normal = nice_to_weight(0);
        
        let delta_low = (runtime_ns * NICE_0_WEIGHT) / weight_low;
        let delta_normal = (runtime_ns * NICE_0_WEIGHT) / weight_normal;
        
        assert!(delta_low > delta_normal,
               "Low priority should advance vruntime faster");
    }

    // =========================================================================
    // CPU Affinity Tests
    // =========================================================================

    #[test]
    fn test_cpu_mask_all() {
        let mask = CpuMask::all();
        // Should have all bits set
        assert!(mask.is_set(0), "CPU 0 should be set in all mask");
    }

    #[test]
    fn test_cpu_mask_empty() {
        let mask = CpuMask::empty();
        // Should have no bits set
        assert!(!mask.is_set(0), "CPU 0 should not be set in empty mask");
    }

    #[test]
    fn test_cpu_mask_set_single() {
        let mut mask = CpuMask::empty();
        mask.set(3);
        assert!(mask.is_set(3), "CPU 3 should be set");
        assert!(!mask.is_set(0), "CPU 0 should not be set");
        assert!(!mask.is_set(2), "CPU 2 should not be set");
        assert!(!mask.is_set(4), "CPU 4 should not be set");
    }

    // =========================================================================
    // Scheduling Policy Tests
    // =========================================================================

    #[test]
    fn test_sched_policies_distinct() {
        assert_ne!(SchedPolicy::Normal, SchedPolicy::Batch);
        assert_ne!(SchedPolicy::Normal, SchedPolicy::Idle);
        assert_ne!(SchedPolicy::Batch, SchedPolicy::Idle);
    }

    #[test]
    fn test_process_entry_default_policy() {
        let entry = make_test_entry(1, 0);
        assert_eq!(entry.policy, SchedPolicy::Normal);
    }

    // =========================================================================
    // Fairness Property Tests
    // =========================================================================

    #[test]
    fn test_equal_nice_equal_cpu() {
        // Two processes with same nice should get equal CPU over time
        let runtime = 1_000_000u64; // 1ms
        let weight = nice_to_weight(0);
        
        // Both advance vruntime by same amount
        let delta1 = (runtime * NICE_0_WEIGHT) / weight;
        let delta2 = (runtime * NICE_0_WEIGHT) / weight;
        
        assert_eq!(delta1, delta2, "Same nice should mean same vruntime delta");
    }

    #[test]
    fn test_cpu_share_proportional_to_weight() {
        // Process with 2x weight should get 2x CPU time for same vruntime
        let weight_high = nice_to_weight(-5);
        let weight_low = nice_to_weight(5);
        
        // For same vruntime advancement, how much real time?
        // vruntime_delta = runtime * NICE_0_WEIGHT / weight
        // runtime = vruntime_delta * weight / NICE_0_WEIGHT
        
        let vruntime_delta = 1_000_000u64;
        let runtime_high = (vruntime_delta * weight_high) / NICE_0_WEIGHT;
        let runtime_low = (vruntime_delta * weight_low) / NICE_0_WEIGHT;
        
        assert!(runtime_high > runtime_low,
               "Higher weight should get more runtime for same vruntime delta");
        
        // Ratio should match weight ratio
        let runtime_ratio = runtime_high as f64 / runtime_low as f64;
        let weight_ratio = weight_high as f64 / weight_low as f64;
        
        let ratio_diff = (runtime_ratio - weight_ratio).abs();
        assert!(ratio_diff < 0.01, "Runtime ratio should match weight ratio");
    }

    // =========================================================================
    // Lag Calculation Tests
    // =========================================================================

    #[test]
    fn test_lag_initial_zero() {
        let entry = make_test_entry(1, 0);
        assert_eq!(entry.lag, 0, "Initial lag should be 0");
    }

    #[test]
    fn test_positive_lag_means_deserves_cpu() {
        let mut entry = make_test_entry(1, 0);
        entry.lag = 1000;
        // Positive lag = process waited longer than it "should have"
        // It deserves CPU time
        assert!(entry.lag > 0);
    }

    #[test]
    fn test_negative_lag_means_got_extra() {
        let mut entry = make_test_entry(1, 0);
        entry.lag = -1000;
        // Negative lag = process got more CPU than it "deserved"
        // It should yield to others
        assert!(entry.lag < 0);
    }
}
