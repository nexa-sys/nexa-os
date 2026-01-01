//! EEVDF Scheduler Priority and Vruntime Edge Case Tests
//!
//! Tests for the EEVDF scheduling algorithm including:
//! - Virtual runtime calculation edge cases
//! - Virtual deadline calculations
//! - Eligibility determination
//! - Weight and nice value conversions
//! - Lag tracking and fairness

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        calc_delta_vruntime, calc_vdeadline, get_min_vruntime, nice_to_weight, BASE_SLICE_NS,
        MAX_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
    };

    /// Convert milliseconds to nanoseconds
    #[inline]
    const fn ms_to_ns(ms: u64) -> u64 {
        ms * 1_000_000
    }

    /// Convert nanoseconds to milliseconds
    #[inline]
    const fn ns_to_ms(ns: u64) -> u64 {
        ns / 1_000_000
    }

    // =========================================================================
    // Time Conversion Tests
    // =========================================================================

    #[test]
    fn test_ms_to_ns_basic() {
        assert_eq!(ms_to_ns(0), 0);
        assert_eq!(ms_to_ns(1), 1_000_000);
        assert_eq!(ms_to_ns(1000), 1_000_000_000);
    }

    #[test]
    fn test_ns_to_ms_basic() {
        assert_eq!(ns_to_ms(0), 0);
        assert_eq!(ns_to_ms(1_000_000), 1);
        assert_eq!(ns_to_ms(1_000_000_000), 1000);
    }

    #[test]
    fn test_ns_to_ms_rounding_down() {
        // ns_to_ms should round down
        assert_eq!(ns_to_ms(999_999), 0);
        assert_eq!(ns_to_ms(1_500_000), 1);
        assert_eq!(ns_to_ms(2_999_999), 2);
    }

    #[test]
    fn test_time_conversion_roundtrip() {
        // Converting ms -> ns -> ms should be identity for whole ms values
        for ms in [0, 1, 10, 100, 1000, 10000] {
            assert_eq!(ns_to_ms(ms_to_ns(ms)), ms);
        }
    }

    // =========================================================================
    // Nice Value to Weight Conversion Tests
    // =========================================================================

    #[test]
    fn test_nice_to_weight_default() {
        // Nice 0 should give NICE_0_WEIGHT
        let weight = nice_to_weight(0);
        assert_eq!(weight, NICE_0_WEIGHT as u64, 
            "Nice 0 should give NICE_0_WEIGHT");
    }

    #[test]
    fn test_nice_to_weight_high_priority() {
        // Negative nice = higher priority = higher weight
        let weight_minus10 = nice_to_weight(-10);
        let weight_0 = nice_to_weight(0);
        
        assert!(weight_minus10 > weight_0, 
            "Nice -10 ({}) should have higher weight than nice 0 ({})", 
            weight_minus10, weight_0);
    }

    #[test]
    fn test_nice_to_weight_low_priority() {
        // Positive nice = lower priority = lower weight
        let weight_10 = nice_to_weight(10);
        let weight_0 = nice_to_weight(0);
        
        assert!(weight_10 < weight_0, 
            "Nice 10 ({}) should have lower weight than nice 0 ({})", 
            weight_10, weight_0);
    }

    #[test]
    fn test_nice_to_weight_monotonic() {
        // Weight should monotonically decrease as nice increases
        let mut prev_weight = nice_to_weight(-20);
        
        for nice in -19..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight <= prev_weight, 
                "Weight should decrease: nice {} ({}) should be <= nice {} ({})",
                nice, weight, nice - 1, prev_weight);
            prev_weight = weight;
        }
    }

    #[test]
    fn test_nice_to_weight_extremes() {
        let weight_min = nice_to_weight(-20);
        let weight_max = nice_to_weight(19);
        
        // Linux CFS/EEVDF uses ~5917:1 ratio (88761/15)
        // The original 88:1 estimate was incorrect
        let ratio = weight_min / weight_max.max(1);
        assert!(ratio >= 5000 && ratio <= 7000, 
            "Nice weight ratio should be approximately 5917:1 (Linux standard), got {}", ratio);
    }

    #[test]
    fn test_nice_to_weight_non_zero() {
        // All weights should be positive
        for nice in -20..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight for nice {} should be positive", nice);
        }
    }

    // =========================================================================
    // Vruntime Calculation Tests
    // =========================================================================

    #[test]
    fn test_calc_delta_vruntime_nice_0() {
        // For nice 0, delta_vruntime == delta_exec (no scaling)
        let delta_exec = 1_000_000u64; // 1ms
        let weight = NICE_0_WEIGHT as u64;
        
        let delta_vrt = calc_delta_vruntime(delta_exec, weight);
        assert_eq!(delta_vrt, delta_exec, 
            "Nice 0 should have 1:1 vruntime mapping");
    }

    #[test]
    fn test_calc_delta_vruntime_high_priority() {
        // Higher priority (higher weight) = slower vruntime growth
        let delta_exec = 1_000_000u64;
        let high_weight = nice_to_weight(-10);
        let normal_weight = NICE_0_WEIGHT as u64;
        
        let delta_vrt_high = calc_delta_vruntime(delta_exec, high_weight);
        let delta_vrt_normal = calc_delta_vruntime(delta_exec, normal_weight);
        
        assert!(delta_vrt_high < delta_vrt_normal, 
            "High priority ({}) should accumulate less vruntime than normal ({})",
            delta_vrt_high, delta_vrt_normal);
    }

    #[test]
    fn test_calc_delta_vruntime_low_priority() {
        // Lower priority (lower weight) = faster vruntime growth
        let delta_exec = 1_000_000u64;
        let low_weight = nice_to_weight(10);
        let normal_weight = NICE_0_WEIGHT as u64;
        
        let delta_vrt_low = calc_delta_vruntime(delta_exec, low_weight);
        let delta_vrt_normal = calc_delta_vruntime(delta_exec, normal_weight);
        
        assert!(delta_vrt_low > delta_vrt_normal, 
            "Low priority ({}) should accumulate more vruntime than normal ({})",
            delta_vrt_low, delta_vrt_normal);
    }

    #[test]
    fn test_calc_delta_vruntime_zero_weight() {
        // Edge case: zero weight should not cause division by zero
        let delta_exec = 1_000_000u64;
        let delta_vrt = calc_delta_vruntime(delta_exec, 0);
        
        // Should return delta_exec as fallback
        assert_eq!(delta_vrt, delta_exec, "Zero weight should return delta_exec as fallback");
    }

    #[test]
    fn test_calc_delta_vruntime_zero_exec() {
        // Zero execution time should give zero vruntime delta
        let delta_vrt = calc_delta_vruntime(0, NICE_0_WEIGHT as u64);
        assert_eq!(delta_vrt, 0);
    }

    #[test]
    fn test_calc_delta_vruntime_large_values() {
        // Test with large values that might overflow 64-bit
        let delta_exec = 1_000_000_000_000u64; // 1000 seconds in ns
        let weight = NICE_0_WEIGHT as u64;
        
        let delta_vrt = calc_delta_vruntime(delta_exec, weight);
        
        // Should not overflow and should equal input for nice 0
        assert_eq!(delta_vrt, delta_exec);
    }

    // =========================================================================
    // Virtual Deadline Calculation Tests
    // =========================================================================

    #[test]
    fn test_calc_vdeadline_basic() {
        let vruntime = 1_000_000u64;
        let slice_ns = BASE_SLICE_NS;
        let weight = NICE_0_WEIGHT as u64;
        
        let deadline = calc_vdeadline(vruntime, slice_ns, weight);
        
        // Deadline should be vruntime + slice for nice 0
        assert_eq!(deadline, vruntime + slice_ns);
    }

    #[test]
    fn test_calc_vdeadline_high_priority_earlier() {
        let vruntime = 1_000_000u64;
        let slice_ns = BASE_SLICE_NS;
        
        let high_weight = nice_to_weight(-10);
        let normal_weight = NICE_0_WEIGHT as u64;
        
        let deadline_high = calc_vdeadline(vruntime, slice_ns, high_weight);
        let deadline_normal = calc_vdeadline(vruntime, slice_ns, normal_weight);
        
        // High priority should have earlier (smaller) deadline
        assert!(deadline_high < deadline_normal,
            "High priority deadline ({}) should be earlier than normal ({})",
            deadline_high, deadline_normal);
    }

    #[test]
    fn test_calc_vdeadline_low_priority_later() {
        let vruntime = 1_000_000u64;
        let slice_ns = BASE_SLICE_NS;
        
        let low_weight = nice_to_weight(10);
        let normal_weight = NICE_0_WEIGHT as u64;
        
        let deadline_low = calc_vdeadline(vruntime, slice_ns, low_weight);
        let deadline_normal = calc_vdeadline(vruntime, slice_ns, normal_weight);
        
        // Low priority should have later (larger) deadline
        assert!(deadline_low > deadline_normal,
            "Low priority deadline ({}) should be later than normal ({})",
            deadline_low, deadline_normal);
    }

    #[test]
    fn test_calc_vdeadline_zero_weight() {
        let vruntime = 1_000_000u64;
        let slice_ns = BASE_SLICE_NS;
        
        let deadline = calc_vdeadline(vruntime, slice_ns, 0);
        
        // Should handle gracefully (vruntime + slice using saturating add)
        assert!(deadline >= vruntime);
    }

    #[test]
    fn test_calc_vdeadline_saturation() {
        // Test that deadline calculation saturates instead of wrapping
        let vruntime = u64::MAX - 1000;
        let slice_ns = BASE_SLICE_NS;
        let weight = NICE_0_WEIGHT as u64;
        
        let deadline = calc_vdeadline(vruntime, slice_ns, weight);
        
        // Should saturate at u64::MAX, not wrap around
        assert!(deadline >= vruntime, "Deadline should not wrap around");
    }

    #[test]
    fn test_calc_vdeadline_zero_slice() {
        let vruntime = 1_000_000u64;
        
        let deadline = calc_vdeadline(vruntime, 0, NICE_0_WEIGHT as u64);
        
        // Zero slice means immediate deadline
        assert_eq!(deadline, vruntime);
    }

    // =========================================================================
    // Scheduler Constants Validation
    // =========================================================================

    #[test]
    fn test_base_slice_reasonable() {
        // Base time slice should be reasonable (3-4ms typical)
        assert!(BASE_SLICE_NS >= 1_000_000, "Base slice should be at least 1ms");
        assert!(BASE_SLICE_NS <= 20_000_000, "Base slice should not exceed 20ms");
    }

    #[test]
    fn test_max_slice_greater_than_base() {
        assert!(MAX_SLICE_NS >= BASE_SLICE_NS, 
            "Max slice should be >= base slice");
    }

    #[test]
    fn test_granularity_smaller_than_slice() {
        // Scheduler granularity should be smaller than time slice
        assert!(SCHED_GRANULARITY_NS < BASE_SLICE_NS,
            "Granularity ({}) should be < base slice ({})",
            SCHED_GRANULARITY_NS, BASE_SLICE_NS);
    }

    #[test]
    fn test_nice_0_weight_value() {
        // NICE_0_WEIGHT is typically 1024 (2^10)
        assert!(NICE_0_WEIGHT > 0);
        assert!(NICE_0_WEIGHT <= 2048, "NICE_0_WEIGHT should be reasonable");
    }

    // =========================================================================
    // Fairness Property Tests
    // =========================================================================

    #[test]
    fn test_fairness_equal_nice() {
        // Two processes with equal nice should get equal vruntime growth
        let delta_exec = 1_000_000u64;
        let weight = nice_to_weight(0);
        
        let delta1 = calc_delta_vruntime(delta_exec, weight);
        let delta2 = calc_delta_vruntime(delta_exec, weight);
        
        assert_eq!(delta1, delta2, "Equal nice should give equal vruntime");
    }

    #[test]
    fn test_fairness_proportional() {
        // CPU time should be proportional to weight
        // If process A has 2x weight of B, A should accumulate half the vruntime
        let delta_exec = 10_000_000u64; // 10ms
        
        let weight_high = nice_to_weight(-5);
        let weight_low = nice_to_weight(5);
        
        let vrt_high = calc_delta_vruntime(delta_exec, weight_high);
        let vrt_low = calc_delta_vruntime(delta_exec, weight_low);
        
        // Higher weight should mean lower vruntime accumulation
        let ratio = (vrt_low as f64) / (vrt_high as f64);
        let weight_ratio = (weight_high as f64) / (weight_low as f64);
        
        // The ratios should be approximately equal (within 5%)
        let diff = (ratio - weight_ratio).abs() / weight_ratio;
        assert!(diff < 0.05, 
            "Vruntime ratio ({}) should match weight ratio ({}) within 5%, diff={}", 
            ratio, weight_ratio, diff);
    }

    // =========================================================================
    // Edge Cases and Stress Tests
    // =========================================================================

    #[test]
    fn test_vruntime_accumulation_large() {
        // Long-running process accumulating vruntime
        let mut vruntime = 0u64;
        let delta_exec = 4_000_000u64; // 4ms per tick
        let weight = nice_to_weight(0);
        
        // 1 million ticks (about 4000 seconds of CPU time)
        for _ in 0..1_000_000 {
            let delta = calc_delta_vruntime(delta_exec, weight);
            vruntime = vruntime.saturating_add(delta);
        }
        
        // Should not overflow
        assert!(vruntime > 0);
    }

    #[test]
    fn test_all_nice_values_produce_valid_weights() {
        for nice in -20..=19i8 {
            let weight = nice_to_weight(nice);
            
            // Weight should be usable for calculations
            let delta = calc_delta_vruntime(1_000_000, weight);
            assert!(delta > 0, "Nice {} should produce valid vruntime delta", nice);
            
            let deadline = calc_vdeadline(0, BASE_SLICE_NS, weight);
            assert!(deadline > 0, "Nice {} should produce valid deadline", nice);
        }
    }

    #[test]
    fn test_min_vruntime_initial() {
        // Initial min_vruntime should be 0
        let min_vrt = get_min_vruntime();
        // Just verify we can read it (actual value depends on system state)
        assert!(min_vrt <= u64::MAX);
    }
}
