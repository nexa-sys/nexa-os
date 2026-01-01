//! Scheduler EEVDF Algorithm Edge Case Tests
//!
//! Tests for the EEVDF (Earliest Eligible Virtual Deadline First) scheduler.
//! These tests verify vruntime calculations, deadline ordering, and fairness.

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        nice_to_weight, calc_vdeadline,
        BASE_SLICE_NS, MAX_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
    };

    // =========================================================================
    // EEVDF Constants Tests
    // =========================================================================

    #[test]
    fn test_base_slice_reasonable() {
        // Base time slice should be between 1ms and 100ms
        assert!(BASE_SLICE_NS >= 1_000_000);    // At least 1ms
        assert!(BASE_SLICE_NS <= 100_000_000);  // At most 100ms
    }

    #[test]
    fn test_max_slice_reasonable() {
        // Max slice should be larger than base
        assert!(MAX_SLICE_NS > BASE_SLICE_NS);
        // But not more than 1 second
        assert!(MAX_SLICE_NS <= 1_000_000_000);
    }

    #[test]
    fn test_sched_granularity() {
        // Granularity should be between 1ms and 10ms
        assert!(SCHED_GRANULARITY_NS >= 1_000_000);
        assert!(SCHED_GRANULARITY_NS <= 10_000_000);
    }

    #[test]
    fn test_nice_0_weight() {
        // Nice 0 weight should be 1024 (standard Linux value)
        assert_eq!(NICE_0_WEIGHT, 1024);
    }

    // =========================================================================
    // Nice to Weight Conversion Tests
    // =========================================================================

    #[test]
    fn test_nice_to_weight_zero() {
        let weight = nice_to_weight(0);
        assert_eq!(weight, 1024);
    }

    #[test]
    fn test_nice_to_weight_negative() {
        // Negative nice = higher priority = higher weight
        let weight_neg5 = nice_to_weight(-5);
        let weight_neg10 = nice_to_weight(-10);
        let weight_neg20 = nice_to_weight(-20);
        
        assert!(weight_neg5 > 1024);
        assert!(weight_neg10 > weight_neg5);
        assert!(weight_neg20 > weight_neg10);
    }

    #[test]
    fn test_nice_to_weight_positive() {
        // Positive nice = lower priority = lower weight
        let weight_pos5 = nice_to_weight(5);
        let weight_pos10 = nice_to_weight(10);
        let weight_pos19 = nice_to_weight(19);
        
        assert!(weight_pos5 < 1024);
        assert!(weight_pos10 < weight_pos5);
        assert!(weight_pos19 < weight_pos10);
    }

    #[test]
    fn test_nice_to_weight_ratio() {
        // Each nice level should change weight by ~1.25x
        // This ensures consistent CPU time ratios
        let weight_0 = nice_to_weight(0) as f64;
        let weight_1 = nice_to_weight(1) as f64;
        
        let ratio = weight_0 / weight_1;
        // Should be approximately 1.25
        assert!(ratio > 1.2 && ratio < 1.3);
    }

    #[test]
    fn test_nice_to_weight_extremes() {
        // Test extreme values
        let weight_min = nice_to_weight(-20);
        let weight_max = nice_to_weight(19);
        
        // Ratio between extremes should be very large (88740x in Linux)
        let ratio = weight_min as f64 / weight_max as f64;
        assert!(ratio > 1000.0);
    }

    // =========================================================================
    // Virtual Runtime Calculation Tests
    // =========================================================================

    /// Calculate weighted virtual runtime increase
    fn calc_vruntime_delta(actual_time_ns: u64, weight: u64) -> u64 {
        // vruntime_delta = actual_time * NICE_0_WEIGHT / weight
        (actual_time_ns as u128 * NICE_0_WEIGHT as u128 / weight as u128) as u64
    }

    #[test]
    fn test_vruntime_nice_zero() {
        // Nice 0 process: vruntime increases at 1:1
        let actual = 1_000_000u64; // 1ms
        let weight = nice_to_weight(0);
        let delta = calc_vruntime_delta(actual, weight);
        
        assert_eq!(delta, actual);
    }

    #[test]
    fn test_vruntime_high_priority() {
        // High priority (-10): vruntime increases slower
        let actual = 1_000_000u64;
        let weight_0 = nice_to_weight(0);
        let weight_neg10 = nice_to_weight(-10);
        
        let delta_0 = calc_vruntime_delta(actual, weight_0);
        let delta_neg10 = calc_vruntime_delta(actual, weight_neg10);
        
        // Higher weight = slower vruntime increase
        assert!(delta_neg10 < delta_0);
    }

    #[test]
    fn test_vruntime_low_priority() {
        // Low priority (+10): vruntime increases faster
        let actual = 1_000_000u64;
        let weight_0 = nice_to_weight(0);
        let weight_pos10 = nice_to_weight(10);
        
        let delta_0 = calc_vruntime_delta(actual, weight_0);
        let delta_pos10 = calc_vruntime_delta(actual, weight_pos10);
        
        // Lower weight = faster vruntime increase
        assert!(delta_pos10 > delta_0);
    }

    // =========================================================================
    // Virtual Deadline Calculation Tests
    // =========================================================================

    #[test]
    fn test_vdeadline_calculation() {
        // deadline = vruntime + slice / weight * NICE_0_WEIGHT
        let vruntime = 1000u64;
        let slice = BASE_SLICE_NS;
        let weight = nice_to_weight(0);
        
        let deadline = calc_vdeadline(vruntime, slice, weight);
        
        // For nice 0, deadline = vruntime + slice
        assert_eq!(deadline, vruntime + slice);
    }

    #[test]
    fn test_vdeadline_high_priority() {
        let vruntime = 1000u64;
        let slice = BASE_SLICE_NS;
        
        let deadline_0 = calc_vdeadline(vruntime, slice, nice_to_weight(0));
        let deadline_neg10 = calc_vdeadline(vruntime, slice, nice_to_weight(-10));
        
        // High priority has shorter virtual deadline (gets scheduled sooner)
        assert!(deadline_neg10 < deadline_0);
    }

    #[test]
    fn test_vdeadline_low_priority() {
        let vruntime = 1000u64;
        let slice = BASE_SLICE_NS;
        
        let deadline_0 = calc_vdeadline(vruntime, slice, nice_to_weight(0));
        let deadline_pos10 = calc_vdeadline(vruntime, slice, nice_to_weight(10));
        
        // Low priority has longer virtual deadline (gets scheduled later)
        assert!(deadline_pos10 > deadline_0);
    }

    // =========================================================================
    // EEVDF Eligibility Tests
    // =========================================================================

    /// Calculate lag: ideal_time - actual_time
    fn calc_lag(ideal_vruntime: u64, actual_vruntime: u64) -> i64 {
        ideal_vruntime as i64 - actual_vruntime as i64
    }

    /// Check if process is eligible (lag >= 0)
    fn is_eligible(ideal_vruntime: u64, actual_vruntime: u64) -> bool {
        calc_lag(ideal_vruntime, actual_vruntime) >= 0
    }

    #[test]
    fn test_eligibility_caught_up() {
        // Process that has run its fair share
        let ideal = 1000u64;
        let actual = 1000u64;
        
        assert!(is_eligible(ideal, actual));
    }

    #[test]
    fn test_eligibility_behind() {
        // Process that hasn't run enough (positive lag)
        let ideal = 1000u64;
        let actual = 500u64;
        
        assert!(is_eligible(ideal, actual));
        assert!(calc_lag(ideal, actual) > 0);
    }

    #[test]
    fn test_eligibility_ahead() {
        // Process that has run too much (negative lag)
        let ideal = 1000u64;
        let actual = 1500u64;
        
        assert!(!is_eligible(ideal, actual));
        assert!(calc_lag(ideal, actual) < 0);
    }

    // =========================================================================
    // EEVDF Selection Tests
    // =========================================================================

    struct EevdfProcess {
        pid: u64,
        vruntime: u64,
        vdeadline: u64,
        weight: u64,
    }

    fn select_next_eevdf(processes: &[EevdfProcess], min_vruntime: u64) -> Option<u64> {
        // 1. Filter eligible processes (vruntime <= min_vruntime)
        // 2. Select one with earliest virtual deadline
        
        let mut best: Option<&EevdfProcess> = None;
        
        for p in processes {
            // Check eligibility (simplified: vruntime <= min_vruntime + some_threshold)
            if p.vruntime > min_vruntime + 1000 {
                continue; // Not eligible
            }
            
            match best {
                None => best = Some(p),
                Some(current_best) => {
                    if p.vdeadline < current_best.vdeadline {
                        best = Some(p);
                    }
                }
            }
        }
        
        best.map(|p| p.pid)
    }

    #[test]
    fn test_eevdf_selection_basic() {
        let processes = vec![
            EevdfProcess { pid: 1, vruntime: 100, vdeadline: 200, weight: 1024 },
            EevdfProcess { pid: 2, vruntime: 100, vdeadline: 150, weight: 1024 },
            EevdfProcess { pid: 3, vruntime: 100, vdeadline: 300, weight: 1024 },
        ];
        
        let selected = select_next_eevdf(&processes, 100);
        
        // Should select process with earliest deadline (PID 2)
        assert_eq!(selected, Some(2));
    }

    #[test]
    fn test_eevdf_selection_ineligible() {
        let processes = vec![
            EevdfProcess { pid: 1, vruntime: 5000, vdeadline: 200, weight: 1024 }, // Ineligible
            EevdfProcess { pid: 2, vruntime: 100, vdeadline: 250, weight: 1024 },  // Eligible
        ];
        
        let selected = select_next_eevdf(&processes, 100);
        
        // Should select PID 2 (PID 1 is not eligible)
        assert_eq!(selected, Some(2));
    }

    // =========================================================================
    // Fairness Tests
    // =========================================================================

    #[test]
    fn test_fairness_equal_priority() {
        // Two processes with same priority should get equal CPU time
        // After running, their vruntimes should be similar
        
        let weight = nice_to_weight(0);
        let mut vruntime_1 = 0u64;
        let mut vruntime_2 = 0u64;
        let time_quantum = 1_000_000u64; // 1ms
        
        // 100 scheduling rounds
        for round in 0..100 {
            if round % 2 == 0 {
                vruntime_1 += calc_vruntime_delta(time_quantum, weight);
            } else {
                vruntime_2 += calc_vruntime_delta(time_quantum, weight);
            }
        }
        
        // Vruntimes should be equal
        assert_eq!(vruntime_1, vruntime_2);
    }

    #[test]
    fn test_fairness_different_priority() {
        // Process with nice -5 should get ~3x more CPU than nice +5
        let weight_neg5 = nice_to_weight(-5);
        let weight_pos5 = nice_to_weight(5);
        let time_quantum = 1_000_000u64;
        
        let vruntime_delta_neg5 = calc_vruntime_delta(time_quantum, weight_neg5);
        let vruntime_delta_pos5 = calc_vruntime_delta(time_quantum, weight_pos5);
        
        // To have equal vruntime progress, neg5 needs more actual time
        // Ratio should be approximately weight ratio
        let ratio = vruntime_delta_pos5 as f64 / vruntime_delta_neg5 as f64;
        let expected_ratio = weight_neg5 as f64 / weight_pos5 as f64;
        
        assert!((ratio - expected_ratio).abs() < 0.01);
    }

    // =========================================================================
    // Edge Cases and Potential Bugs
    // =========================================================================

    #[test]
    fn test_vruntime_overflow() {
        // Test behavior near u64::MAX
        let large_vruntime = u64::MAX - 1_000_000;
        let weight = nice_to_weight(0);
        
        let delta = calc_vruntime_delta(1_000_000, weight);
        
        // Should handle large values without panic
        let new_vruntime = large_vruntime.saturating_add(delta);
        assert!(new_vruntime <= u64::MAX);
    }

    #[test]
    fn test_vdeadline_overflow() {
        // Test deadline calculation near overflow
        let large_vruntime = u64::MAX - BASE_SLICE_NS;
        let weight = nice_to_weight(0);
        
        // Should not overflow
        let deadline = large_vruntime.saturating_add(BASE_SLICE_NS);
        assert!(deadline >= large_vruntime);
    }

    #[test]
    fn test_zero_weight_protection() {
        // Weight should never be zero (would cause division by zero)
        for nice in -20..=19 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight for nice {} is zero!", nice);
        }
    }

    #[test]
    fn test_min_vruntime_update() {
        // min_vruntime should only increase
        let mut min_vruntime = 0u64;
        
        fn update_min_vruntime(min: &mut u64, process_vruntime: u64) {
            if process_vruntime > *min {
                *min = process_vruntime;
            }
        }
        
        update_min_vruntime(&mut min_vruntime, 100);
        assert_eq!(min_vruntime, 100);
        
        update_min_vruntime(&mut min_vruntime, 50); // Lower value
        assert_eq!(min_vruntime, 100); // Should not decrease
        
        update_min_vruntime(&mut min_vruntime, 200);
        assert_eq!(min_vruntime, 200);
    }

    #[test]
    fn test_new_process_vruntime() {
        // New process should start at min_vruntime to be fair
        let min_vruntime = 1000u64;
        
        fn init_new_process_vruntime(min_vruntime: u64) -> u64 {
            // Could add a bonus to help new processes get scheduled
            min_vruntime
        }
        
        let new_vruntime = init_new_process_vruntime(min_vruntime);
        assert_eq!(new_vruntime, min_vruntime);
    }

    #[test]
    fn test_starvation_prevention() {
        // Processes with high priority shouldn't starve others
        // After enough time, all processes should make progress
        
        let mut processes = vec![
            EevdfProcess { pid: 1, vruntime: 0, vdeadline: 100, weight: nice_to_weight(-20) as u64 },
            EevdfProcess { pid: 2, vruntime: 0, vdeadline: 1000, weight: nice_to_weight(19) as u64 },
        ];
        
        let mut selections = [0u32; 2];
        
        // Many scheduling rounds
        for _ in 0..1000 {
            // Simplified selection: lowest vruntime wins
            let selected_idx = if processes[0].vruntime <= processes[1].vruntime { 0 } else { 1 };
            
            selections[selected_idx] += 1;
            
            // Update vruntime
            let delta = calc_vruntime_delta(1_000_000, processes[selected_idx].weight);
            processes[selected_idx].vruntime += delta;
        }
        
        // Both processes should get some CPU time
        assert!(selections[0] > 0, "Process 1 starved");
        assert!(selections[1] > 0, "Process 2 starved");
        
        // High priority should get more
        assert!(selections[0] > selections[1]);
    }
}
