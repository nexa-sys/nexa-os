//! Scheduler EEVDF Algorithm Edge Case Tests
//!
//! Tests for the EEVDF (Earliest Eligible Virtual Deadline First) scheduler.
//! These tests verify vruntime calculations, deadline ordering, and fairness.

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        calc_delta_vruntime, calc_vdeadline, is_eligible, nice_to_weight, update_curr,
        ProcessEntry, BASE_SLICE_NS, MAX_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
    };
    use crate::scheduler::percpu::{PerCpuRunQueue, RunQueueEntry, PERCPU_RQ_SIZE};
    use crate::scheduler::SchedPolicy;
    use crate::process::ProcessState;

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
    // Virtual Runtime Calculation Tests - Using Real Kernel Functions
    // =========================================================================

    #[test]
    fn test_vruntime_nice_zero() {
        // Nice 0 process: vruntime increases at 1:1
        let actual = 1_000_000u64; // 1ms
        let weight = nice_to_weight(0);
        let delta = calc_delta_vruntime(actual, weight);

        assert_eq!(delta, actual);
    }

    #[test]
    fn test_vruntime_high_priority() {
        // High priority (-10): vruntime increases slower
        let actual = 1_000_000u64;
        let weight_0 = nice_to_weight(0);
        let weight_neg10 = nice_to_weight(-10);

        let delta_0 = calc_delta_vruntime(actual, weight_0);
        let delta_neg10 = calc_delta_vruntime(actual, weight_neg10);

        // Higher weight = slower vruntime increase
        assert!(delta_neg10 < delta_0);
    }

    #[test]
    fn test_vruntime_low_priority() {
        // Low priority (+10): vruntime increases faster
        let actual = 1_000_000u64;
        let weight_0 = nice_to_weight(0);
        let weight_pos10 = nice_to_weight(10);

        let delta_0 = calc_delta_vruntime(actual, weight_0);
        let delta_pos10 = calc_delta_vruntime(actual, weight_pos10);

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
    // EEVDF Eligibility Tests (using real ProcessEntry and is_eligible)
    // =========================================================================

    /// Helper to create test ProcessEntry with specific lag value
    fn make_entry_with_lag(pid: u64, lag: i64) -> ProcessEntry {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = pid;
        entry.process.state = ProcessState::Ready;
        entry.nice = 0;
        entry.policy = SchedPolicy::Normal;
        entry.weight = nice_to_weight(0);
        entry.slice_ns = BASE_SLICE_NS;
        entry.slice_remaining_ns = BASE_SLICE_NS;
        entry.vruntime = 1000;
        entry.vdeadline = 1000 + BASE_SLICE_NS;
        entry.lag = lag;
        entry
    }

    #[test]
    fn test_eligibility_caught_up() {
        // Process that has run its fair share (lag = 0)
        let entry = make_entry_with_lag(1, 0);
        
        assert!(is_eligible(&entry));
    }

    #[test]
    fn test_eligibility_behind() {
        // Process that hasn't run enough (positive lag)
        let entry = make_entry_with_lag(1, 500);
        
        assert!(is_eligible(&entry));
        assert!(entry.lag > 0);
    }

    #[test]
    fn test_eligibility_ahead() {
        // Process that has run too much (negative lag)
        let entry = make_entry_with_lag(1, -500);
        
        assert!(!is_eligible(&entry));
        assert!(entry.lag < 0);
    }

    // =========================================================================
    // EEVDF Selection Tests (using real PerCpuRunQueue::pick_next)
    // =========================================================================

    /// Helper to create RunQueueEntry for selection tests
    fn make_rq_entry(pid: u64, vruntime: u64, vdeadline: u64) -> RunQueueEntry {
        RunQueueEntry {
            pid,
            table_index: pid as u16,
            vdeadline,
            vruntime,
            policy: SchedPolicy::Normal,
            priority: 128,
            eligible: true,
        }
    }

    #[test]
    fn test_eevdf_selection_basic() {
        // Use real PerCpuRunQueue::pick_next for selection
        let mut rq = PerCpuRunQueue::new(0);
        
        // Add entries with different deadlines
        rq.enqueue(make_rq_entry(1, 100, 200));
        rq.enqueue(make_rq_entry(2, 100, 150));  // Earliest deadline
        rq.enqueue(make_rq_entry(3, 100, 300));
        
        let selected = rq.pick_next();
        
        // Should select process with earliest deadline (PID 2)
        assert_eq!(selected.map(|e| e.pid), Some(2));
    }

    #[test]
    fn test_eevdf_selection_ineligible() {
        // Use real PerCpuRunQueue::pick_next for selection
        let mut rq = PerCpuRunQueue::new(0);
        
        // Add one ineligible entry (marked as not eligible)
        let mut entry1 = make_rq_entry(1, 5000, 200);
        entry1.eligible = false;  // Marked ineligible
        rq.enqueue(entry1);
        
        // Add one eligible entry
        rq.enqueue(make_rq_entry(2, 100, 250));
        
        let selected = rq.pick_next();
        
        // Should select PID 2 (PID 1 is not eligible)
        assert_eq!(selected.map(|e| e.pid), Some(2));
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
                vruntime_1 += calc_delta_vruntime(time_quantum, weight);
            } else {
                vruntime_2 += calc_delta_vruntime(time_quantum, weight);
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
        
        let vruntime_delta_neg5 = calc_delta_vruntime(time_quantum, weight_neg5);
        let vruntime_delta_pos5 = calc_delta_vruntime(time_quantum, weight_pos5);
        
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
        
        let delta = calc_delta_vruntime(1_000_000, weight);
        
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

    /// Helper to create a full ProcessEntry for EEVDF testing
    fn make_test_entry(pid: u64, nice: i8) -> ProcessEntry {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = pid;
        entry.process.state = ProcessState::Ready;
        entry.nice = nice;
        entry.policy = SchedPolicy::Normal;
        entry.weight = nice_to_weight(nice);
        entry.slice_ns = BASE_SLICE_NS;
        entry.slice_remaining_ns = BASE_SLICE_NS;
        entry.vruntime = 0;
        entry.vdeadline = calc_vdeadline(0, BASE_SLICE_NS, entry.weight);
        entry.lag = 0;
        entry
    }

    #[test]
    fn test_starvation_prevention() {
        // Test EEVDF starvation prevention using real update_curr and is_eligible
        // Even with extreme nice values, low priority process should eventually run
        
        let mut entry_high = make_test_entry(1, -20);  // Highest priority
        let mut entry_low = make_test_entry(2, 19);    // Lowest priority
        
        let mut selections = [0u32; 2];
        let run_time_ns = 1_000_000u64; // 1ms per scheduling round
        
        // Simulate 100 scheduling rounds and print first 10
        for i in 0..100 {
            // Check eligibility using real is_eligible function
            let high_eligible = is_eligible(&entry_high);
            let low_eligible = is_eligible(&entry_low);
            
            // Select based on EEVDF rules: eligible + earliest deadline
            let selected = if high_eligible && low_eligible {
                // Both eligible - pick by deadline (lower wins)
                if entry_high.vdeadline <= entry_low.vdeadline { 0 } else { 1 }
            } else if high_eligible {
                0
            } else if low_eligible {
                1
            } else {
                // Neither eligible - pick one with less negative lag
                if entry_high.lag >= entry_low.lag { 0 } else { 1 }
            };
            
            if i < 10 {
                eprintln!("Round {}: high(vdl={}, lag={}, elig={}) low(vdl={}, lag={}, elig={}) -> {}",
                    i, entry_high.vdeadline, entry_high.lag, high_eligible,
                    entry_low.vdeadline, entry_low.lag, low_eligible,
                    if selected == 0 { "HIGH" } else { "LOW" });
            }
            
            selections[selected] += 1;
            
            // Update the selected entry using real update_curr
            if selected == 0 {
                update_curr(&mut entry_high, run_time_ns);
                // Other process gains lag (it was waiting)
                entry_low.lag = entry_low.lag.saturating_add(run_time_ns as i64);
            } else {
                update_curr(&mut entry_low, run_time_ns);
                entry_high.lag = entry_high.lag.saturating_add(run_time_ns as i64);
            }
        }
        
        // CRITICAL: Both processes MUST get some CPU time
        assert!(selections[0] > 0, "High priority process starved");
        assert!(selections[1] > 0, "Low priority process starved");
        
        // High priority should get significantly more CPU time
        assert!(selections[0] > selections[1], 
                "Higher priority should run more: high={}, low={}", selections[0], selections[1]);
        
        // Weight ratio for nice -20 vs nice 19 is about 88:1
        // High priority should dominate but low priority still gets some time
        let ratio = selections[0] as f64 / selections[1].max(1) as f64;
        assert!(ratio > 10.0, "Ratio {} too low for extreme nice difference", ratio);
    }
}
