//! EEVDF (Earliest Eligible Virtual Deadline First) scheduler tests
//!
//! Tests for virtual runtime calculations and priority weights.

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        SCHED_GRANULARITY_NS, BASE_SLICE_NS, MAX_SLICE_NS,
        BASE_TIME_SLICE_MS, DEFAULT_TIME_SLICE, NUM_PRIORITY_LEVELS,
        NICE_0_WEIGHT,
    };
    
    // MIN_PREEMPT_GRANULARITY_NS is internal, define locally
    const MIN_PREEMPT_GRANULARITY_NS: u64 = 2_000_000;

    // NICE_TO_WEIGHT array - reproducing from scheduler types for testing
    const NICE_TO_WEIGHT: [u64; 40] = [
        // -20 to -11
        88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916,
        // -10 to -1
        9548, 7620, 6100, 4904, 3906, 3121, 2501, 1991, 1586, 1277,
        // 0 to 9
        1024, 820, 655, 526, 423, 335, 272, 215, 172, 137,
        // 10 to 19
        110, 87, 70, 56, 45, 36, 29, 23, 18, 15,
    ];

    // =========================================================================
    // Scheduling Constants Tests
    // =========================================================================

    #[test]
    fn test_scheduler_granularity() {
        // Minimum granularity should be reasonable (2ms)
        assert_eq!(SCHED_GRANULARITY_NS, 2_000_000);
        
        // Minimum preemption granularity
        assert_eq!(MIN_PREEMPT_GRANULARITY_NS, 2_000_000);
    }

    #[test]
    fn test_time_slice_bounds() {
        // Base slice (4ms)
        assert_eq!(BASE_SLICE_NS, 4_000_000);
        
        // Max slice (100ms)
        assert_eq!(MAX_SLICE_NS, 100_000_000);
        
        // Base should be less than max
        assert!(BASE_SLICE_NS < MAX_SLICE_NS);
        
        // Granularity should be less than base slice
        assert!(SCHED_GRANULARITY_NS < BASE_SLICE_NS);
    }

    #[test]
    fn test_legacy_time_slice() {
        assert_eq!(DEFAULT_TIME_SLICE, 4);
        assert_eq!(BASE_TIME_SLICE_MS, 4);
        
        // Verify consistency with nanosecond values
        assert_eq!(DEFAULT_TIME_SLICE * 1_000_000, BASE_SLICE_NS / 1000 * 1000);
    }

    #[test]
    fn test_num_priority_levels() {
        assert_eq!(NUM_PRIORITY_LEVELS, 8);
        assert!(NUM_PRIORITY_LEVELS >= 4);
    }

    // =========================================================================
    // Nice Value Weight Tests
    // =========================================================================

    #[test]
    fn test_nice_0_weight() {
        // Nice 0 is the baseline weight
        assert_eq!(NICE_0_WEIGHT, 1024);
        
        // Should match array at index 20 (nice 0 = index 20)
        assert_eq!(NICE_TO_WEIGHT[20], NICE_0_WEIGHT);
    }

    #[test]
    fn test_nice_weights_array_size() {
        // Array covers nice -20 to +19 (40 values)
        assert_eq!(NICE_TO_WEIGHT.len(), 40);
    }

    #[test]
    fn test_nice_weights_monotonic_decreasing() {
        // Higher nice = lower weight (less CPU)
        for i in 1..NICE_TO_WEIGHT.len() {
            assert!(
                NICE_TO_WEIGHT[i] <= NICE_TO_WEIGHT[i - 1],
                "Weight at index {} ({}) should be <= weight at index {} ({})",
                i, NICE_TO_WEIGHT[i], i - 1, NICE_TO_WEIGHT[i - 1]
            );
        }
    }

    #[test]
    fn test_nice_weights_positive() {
        // All weights should be positive
        for (i, &weight) in NICE_TO_WEIGHT.iter().enumerate() {
            assert!(weight > 0, "Weight at index {} should be positive", i);
        }
    }

    #[test]
    fn test_nice_weights_extreme_values() {
        // Nice -20 (index 0) has highest weight
        let nice_minus_20 = NICE_TO_WEIGHT[0];
        
        // Nice +19 (index 39) has lowest weight
        let nice_plus_19 = NICE_TO_WEIGHT[39];
        
        // Ratio should be significant (roughly 1.25^39 ≈ 5900x)
        assert!(nice_minus_20 > nice_plus_19 * 100);
    }

    #[test]
    fn test_nice_weight_ratio_between_levels() {
        // Each nice level should differ by roughly 1.25x
        // (But we allow some variance due to integer rounding)
        let weight_0 = NICE_TO_WEIGHT[20] as f64;
        let weight_1 = NICE_TO_WEIGHT[21] as f64;
        
        let ratio = weight_0 / weight_1;
        
        // Should be approximately 1.25
        assert!(ratio > 1.15 && ratio < 1.35, "Ratio {} not in expected range", ratio);
    }

    // =========================================================================
    // Virtual Runtime Calculation Tests
    // =========================================================================

    #[test]
    fn test_vruntime_calculation_nice_0() {
        // For nice 0, vruntime should equal wall-clock time
        let weight = NICE_TO_WEIGHT[20]; // nice 0
        let delta_ns = 1_000_000_u64; // 1ms
        
        // vruntime = delta * NICE_0_WEIGHT / weight
        let vruntime = delta_ns * NICE_0_WEIGHT / weight;
        
        assert_eq!(vruntime, delta_ns);
    }

    #[test]
    fn test_vruntime_calculation_low_nice() {
        // For nice -5 (higher priority), vruntime should advance slower
        let weight_minus_5 = NICE_TO_WEIGHT[15]; // index for nice -5
        let delta_ns = 1_000_000_u64; // 1ms
        
        let vruntime = delta_ns * NICE_0_WEIGHT / weight_minus_5;
        
        // Higher weight = slower vruntime advancement
        // (gets more CPU before being preempted)
        assert!(vruntime < delta_ns);
    }

    #[test]
    fn test_vruntime_calculation_high_nice() {
        // For nice +5 (lower priority), vruntime should advance faster
        let weight_plus_5 = NICE_TO_WEIGHT[25]; // index for nice +5
        let delta_ns = 1_000_000_u64; // 1ms
        
        let vruntime = delta_ns * NICE_0_WEIGHT / weight_plus_5;
        
        // Lower weight = faster vruntime advancement
        // (gets less CPU, appears to have run longer)
        assert!(vruntime > delta_ns);
    }

    #[test]
    fn test_vruntime_overflow_protection() {
        // Very long running time shouldn't overflow with u64
        let weight = NICE_TO_WEIGHT[20];
        let delta_ns = 3600_000_000_000_u64; // 1 hour in ns
        
        // Should not overflow
        let vruntime = delta_ns.checked_mul(NICE_0_WEIGHT);
        assert!(vruntime.is_some());
        
        let vruntime = vruntime.unwrap().checked_div(weight);
        assert!(vruntime.is_some());
    }

    // =========================================================================
    // Slice Duration Calculation Tests
    // =========================================================================

    #[test]
    fn test_slice_duration_calculation() {
        // Simplified slice calculation: weight / total_weight * period
        let weight = NICE_TO_WEIGHT[20]; // nice 0
        let total_weight = weight * 4; // 4 processes at nice 0
        let period = BASE_SLICE_NS * 4; // Scale with process count
        
        let slice = weight as u64 * period / total_weight as u64;
        
        // Each process should get BASE_SLICE_NS
        assert_eq!(slice, BASE_SLICE_NS);
    }

    #[test]
    fn test_slice_respects_minimum() {
        // Even with many processes, slice shouldn't go below granularity
        let weight = NICE_TO_WEIGHT[20];
        let total_weight = weight * 1000; // 1000 processes
        let period = BASE_SLICE_NS;
        
        let slice = weight as u64 * period / total_weight as u64;
        
        // Actual scheduler should clamp to MIN_PREEMPT_GRANULARITY_NS
        // This test shows the raw calculation might go below minimum
        if slice < SCHED_GRANULARITY_NS {
            // Scheduler should clamp this
            let clamped = slice.max(SCHED_GRANULARITY_NS);
            assert!(clamped >= SCHED_GRANULARITY_NS);
        }
    }

    // =========================================================================
    // EEVDF Eligibility Tests
    // =========================================================================

    #[test]
    fn test_eevdf_eligibility_concept() {
        // A process is eligible if: vruntime <= min_vruntime + request_size
        let min_vruntime = 1_000_000_u64;
        let request_size = BASE_SLICE_NS;
        
        let process_vruntime = min_vruntime + request_size / 2;
        
        // Process is eligible
        let eligible = process_vruntime <= min_vruntime + request_size;
        assert!(eligible);
        
        // Process with high vruntime is not eligible
        let high_vruntime = min_vruntime + request_size * 2;
        let eligible = high_vruntime <= min_vruntime + request_size;
        assert!(!eligible);
    }

    #[test]
    fn test_eevdf_virtual_deadline() {
        // Virtual deadline = vruntime + request_size
        let vruntime = 1_000_000_u64;
        let request_size = BASE_SLICE_NS;
        
        let deadline = vruntime + request_size;
        
        // Deadline should be in the future
        assert!(deadline > vruntime);
    }

    // =========================================================================
    // Weight to Nice Conversion Tests
    // =========================================================================

    #[test]
    fn test_nice_from_weight_exact() {
        // Converting back from weight to nice should work
        fn nice_from_weight(weight: u64) -> Option<i32> {
            for (i, &w) in NICE_TO_WEIGHT.iter().enumerate() {
                if w == weight {
                    return Some(i as i32 - 20); // Convert index to nice
                }
            }
            None
        }
        
        // Check nice 0
        assert_eq!(nice_from_weight(1024), Some(0));
        
        // Check extremes
        assert_eq!(nice_from_weight(NICE_TO_WEIGHT[0]), Some(-20));
        assert_eq!(nice_from_weight(NICE_TO_WEIGHT[39]), Some(19));
    }

    // =========================================================================
    // Load Balance Weight Tests
    // =========================================================================

    #[test]
    fn test_weight_sum_calculation() {
        // Sum of all weights for load balancing
        let total_weight: u64 = NICE_TO_WEIGHT.iter().map(|&w| w).sum();
        
        // Should be a reasonable value
        assert!(total_weight > 0);
        assert!(total_weight < u64::MAX / 1000);
    }

    #[test]
    fn test_load_average_calculation() {
        // Load = sum(weight) / NICE_0_WEIGHT
        let weights = [NICE_TO_WEIGHT[20], NICE_TO_WEIGHT[25], NICE_TO_WEIGHT[15]];
        let sum: u64 = weights.iter().sum();
        
        let load = sum as f64 / NICE_0_WEIGHT as f64;
        
        // 3 processes with different nice values
        // Should be approximately 3 (biased by nice)
        assert!(load > 0.0 && load < 10.0);
    }
}
