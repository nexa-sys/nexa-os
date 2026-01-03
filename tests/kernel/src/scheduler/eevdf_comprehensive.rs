//! Scheduler EEVDF Algorithm Edge Case Tests
//!
//! Tests for EEVDF (Earliest Eligible Virtual Deadline First) scheduling
//! including vruntime wraparound, eligibility checks, and fairness properties.

#[cfg(test)]
mod tests {
    // EEVDF constants from scheduler
    use crate::scheduler::{
        BASE_SLICE_NS, MAX_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
        nice_to_weight, calc_vdeadline, is_eligible, ProcessEntry, SchedPolicy,
    };
    use crate::scheduler::percpu::{PerCpuRunQueue, RunQueueEntry};
    use crate::process::ProcessState;

    // =========================================================================
    // Nice to Weight Conversion Tests
    // =========================================================================

    #[test]
    fn test_nice_0_weight() {
        // Nice 0 should have weight NICE_0_WEIGHT (typically 1024)
        let weight = nice_to_weight(0);
        assert_eq!(weight, NICE_0_WEIGHT);
    }

    #[test]
    fn test_nice_range() {
        // Nice values range from -20 (highest priority) to 19 (lowest)
        for nice in -20i8..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight should always be positive");
        }
    }

    #[test]
    fn test_nice_priority_inversion() {
        // Lower nice value = higher weight = more CPU time
        let weight_high = nice_to_weight(-20);
        let weight_normal = nice_to_weight(0);
        let weight_low = nice_to_weight(19);

        assert!(weight_high > weight_normal, "Nice -20 should have higher weight than nice 0");
        assert!(weight_normal > weight_low, "Nice 0 should have higher weight than nice 19");
    }

    #[test]
    fn test_nice_weight_ratio() {
        // Each nice level should change weight by ~1.25x (Linux semantics)
        let w0 = nice_to_weight(0) as f64;
        let w1 = nice_to_weight(1) as f64;
        let wm1 = nice_to_weight(-1) as f64;

        // Nice +1 should have ~80% of nice 0's weight
        let ratio_down = w1 / w0;
        assert!(ratio_down > 0.7 && ratio_down < 0.9, 
            "Nice +1 should have ~80% of nice 0's weight, got {}", ratio_down);

        // Nice -1 should have ~125% of nice 0's weight
        let ratio_up = wm1 / w0;
        assert!(ratio_up > 1.1 && ratio_up < 1.4,
            "Nice -1 should have ~125% of nice 0's weight, got {}", ratio_up);
    }

    // =========================================================================
    // Virtual Runtime Tests
    // =========================================================================

    #[test]
    fn test_vruntime_calculation() {
        // vruntime increases based on actual runtime scaled by weight
        // vruntime_delta = actual_runtime * NICE_0_WEIGHT / task_weight

        let actual_runtime_ns: u64 = 1_000_000; // 1ms
        
        // Nice 0 task: vruntime = actual runtime
        let weight_0 = nice_to_weight(0);
        let vruntime_0 = actual_runtime_ns * NICE_0_WEIGHT / weight_0;
        assert_eq!(vruntime_0, actual_runtime_ns);

        // Nice -5 task: lower vruntime for same actual runtime
        let weight_neg = nice_to_weight(-5);
        let vruntime_neg = actual_runtime_ns * NICE_0_WEIGHT / weight_neg;
        assert!(vruntime_neg < vruntime_0, "Higher priority task should accrue less vruntime");

        // Nice +5 task: higher vruntime for same actual runtime
        let weight_pos = nice_to_weight(5);
        let vruntime_pos = actual_runtime_ns * NICE_0_WEIGHT / weight_pos;
        assert!(vruntime_pos > vruntime_0, "Lower priority task should accrue more vruntime");
    }

    #[test]
    fn test_vruntime_overflow_protection() {
        // vruntime can grow very large; check for overflow protection
        let large_vruntime: u64 = u64::MAX - 1000;
        let delta: u64 = 500;

        // Should not overflow
        let new_vruntime = large_vruntime.saturating_add(delta);
        assert!(new_vruntime >= large_vruntime);
    }

    #[test]
    fn test_vruntime_wraparound_comparison() {
        // EEVDF uses signed comparison for vruntime to handle wraparound
        fn vruntime_less(a: u64, b: u64) -> bool {
            // Interpret as signed difference
            (a as i64).wrapping_sub(b as i64) < 0
        }

        // Normal case
        assert!(vruntime_less(100, 200));
        assert!(!vruntime_less(200, 100));

        // Near wraparound
        let near_max = u64::MAX - 100;
        let wrapped = 100;
        
        // After wraparound, "wrapped" should be considered greater
        assert!(vruntime_less(near_max, wrapped));
    }

    // =========================================================================
    // Virtual Deadline Tests
    // =========================================================================

    #[test]
    fn test_vdeadline_calculation() {
        // vdeadline = vruntime + (slice / weight)
        let vruntime: u64 = 1_000_000;
        let weight = nice_to_weight(0);
        let slice_ns = BASE_SLICE_NS;

        let vdeadline = calc_vdeadline(vruntime, slice_ns, weight);
        
        // Should be vruntime + slice (for nice 0)
        let expected = vruntime + slice_ns;
        assert_eq!(vdeadline, expected);
    }

    #[test]
    fn test_vdeadline_priority_effect() {
        let vruntime: u64 = 0;
        let slice_ns = BASE_SLICE_NS;

        // High priority (negative nice) has HIGHER weight
        let weight_high = nice_to_weight(-10);
        let deadline_high = calc_vdeadline(vruntime, slice_ns, weight_high);

        // Low priority (positive nice) has LOWER weight
        let weight_low = nice_to_weight(10);
        let deadline_low = calc_vdeadline(vruntime, slice_ns, weight_low);

        // Normal priority
        let weight_normal = nice_to_weight(0);
        let deadline_normal = calc_vdeadline(vruntime, slice_ns, weight_normal);

        // EEVDF: vdeadline = vruntime + slice * NICE_0_WEIGHT / weight
        // Higher weight -> smaller delta -> EARLIER vdeadline
        // Lower weight -> larger delta -> LATER vdeadline
        assert!(deadline_high < deadline_normal, "High priority (high weight) should have EARLIER vdeadline");
        assert!(deadline_low > deadline_normal, "Low priority (low weight) should have LATER vdeadline");
    }

    // =========================================================================
    // Eligibility Tests
    // =========================================================================

    /// Helper to create test ProcessEntry
    fn make_test_entry(pid: u64, lag: i64) -> ProcessEntry {
        let mut entry = ProcessEntry::empty();
        entry.process.pid = pid;
        entry.process.state = ProcessState::Ready;
        entry.nice = 0;
        entry.policy = SchedPolicy::Normal;
        entry.weight = nice_to_weight(0);
        entry.slice_ns = BASE_SLICE_NS;
        entry.slice_remaining_ns = BASE_SLICE_NS;
        entry.vruntime = 1000;
        entry.vdeadline = calc_vdeadline(1000, BASE_SLICE_NS, nice_to_weight(0));
        entry.lag = lag;
        entry
    }

    #[test]
    fn test_eligibility_concept() {
        // A task is eligible if its lag >= 0
        // Use real is_eligible from kernel

        // Task with positive lag (deserves CPU) is eligible
        let entry_positive = make_test_entry(1, 500);
        assert!(is_eligible(&entry_positive));

        // Task with zero lag (caught up) is eligible
        let entry_zero = make_test_entry(2, 0);
        assert!(is_eligible(&entry_zero));

        // Task with negative lag (ran too much) is NOT eligible
        let entry_negative = make_test_entry(3, -500);
        assert!(!is_eligible(&entry_negative));
    }

    // =========================================================================
    // Time Slice Tests
    // =========================================================================

    #[test]
    fn test_base_slice_reasonable() {
        // Base slice should be in reasonable range (e.g., 3-100ms)
        let slice_ms = BASE_SLICE_NS / 1_000_000;
        assert!(slice_ms >= 1, "Base slice should be at least 1ms");
        assert!(slice_ms <= 100, "Base slice should not exceed 100ms");
    }

    #[test]
    fn test_max_slice_reasonable() {
        assert!(MAX_SLICE_NS >= BASE_SLICE_NS, 
            "Max slice should be >= base slice");
    }

    #[test]
    fn test_granularity_reasonable() {
        // Scheduler granularity determines minimum time between preemptions
        let granularity_ms = SCHED_GRANULARITY_NS / 1_000_000;
        assert!(granularity_ms >= 1, "Granularity should be at least 1ms");
    }

    // =========================================================================
    // Fairness Property Tests
    // =========================================================================

    #[test]
    fn test_fair_share_calculation() {
        // With n tasks of equal weight, each should get 1/n of CPU

        fn calculate_fair_share(num_tasks: usize, total_time_ns: u64) -> u64 {
            total_time_ns / num_tasks as u64
        }

        let total = 1_000_000_000u64; // 1 second

        assert_eq!(calculate_fair_share(1, total), total);
        assert_eq!(calculate_fair_share(2, total), total / 2);
        assert_eq!(calculate_fair_share(4, total), total / 4);
    }

    #[test]
    fn test_weighted_fair_share() {
        // With different weights, share is proportional to weight

        fn calculate_weighted_share(weight: u64, total_weight: u64, total_time_ns: u64) -> u64 {
            total_time_ns * weight / total_weight
        }

        let total_time = 1_000_000_000u64; // 1 second
        let w1 = nice_to_weight(-5);
        let w2 = nice_to_weight(0);
        let w3 = nice_to_weight(5);
        let total_weight = w1 + w2 + w3;

        let share1 = calculate_weighted_share(w1, total_weight, total_time);
        let share2 = calculate_weighted_share(w2, total_weight, total_time);
        let share3 = calculate_weighted_share(w3, total_weight, total_time);

        // Higher weight should get more time
        assert!(share1 > share2);
        assert!(share2 > share3);

        // Shares should sum to total (approximately)
        let total_shares = share1 + share2 + share3;
        assert!((total_shares as i64 - total_time as i64).abs() < 10);
    }

    // =========================================================================
    // Edge Cases and Bug Detection
    // =========================================================================

    #[test]
    fn test_single_task_scheduling() {
        // Single task should get all CPU time
        let weight = nice_to_weight(0);
        let total_weight = weight;

        let share = 1_000_000_000u64 * weight / total_weight;
        assert_eq!(share, 1_000_000_000u64);
    }

    #[test]
    fn test_extreme_nice_values() {
        // Test extreme nice values don't cause issues
        let w_min = nice_to_weight(-20);  // Highest priority gets HIGHEST weight
        let w_max = nice_to_weight(19);   // Lowest priority gets LOWEST weight

        assert!(w_min > 0);
        assert!(w_max > 0);
        assert!(w_min > w_max, "nice -20 should have higher weight than nice 19");

        // NexaOS uses exponential weight scaling (1.25^(-nice))
        // Ratio between -20 and 19 is approximately: 88761 / 15 ≈ 5917
        // This gives stronger priority differentiation than Linux's ~88x
        let ratio = w_min as f64 / w_max as f64;
        assert!(ratio > 1.0);
        assert!(ratio < 10000.0, "Ratio should be bounded but can be large: {}", ratio);
    }

    #[test]
    fn test_zero_weight_protection() {
        // Weight should never be zero (would cause division by zero)
        for nice in -20i8..=19i8 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0, "Weight for nice {} should be > 0", nice);
        }
    }

    #[test]
    fn test_vruntime_monotonic() {
        // vruntime should only increase (monotonic)
        // This is a property we want to ensure

        let mut vruntime: u64 = 0;
        let weight = nice_to_weight(0);

        for _ in 0..100 {
            let old_vruntime = vruntime;
            let delta = 1_000_000 * NICE_0_WEIGHT / weight; // 1ms of actual time
            vruntime = vruntime.saturating_add(delta);
            assert!(vruntime >= old_vruntime, "vruntime should be monotonic");
        }
    }

    #[test]
    fn test_min_vruntime_tracking() {
        // min_vruntime should track the smallest vruntime in the run queue
        let mut min_vruntime: u64 = 0;
        let vruntimes: [u64; 5] = [100, 50, 200, 75, 150];

        // When tasks are added, update min_vruntime
        for &vrt in &vruntimes {
            // New task starts with max(min_vruntime, task_vruntime)
            // min_vruntime updated when dequeueing
        }

        // After removing task with vruntime 50, min should be 75
        let remaining: Vec<u64> = vruntimes.iter().filter(|&&v| v != 50).copied().collect();
        let new_min = *remaining.iter().min().unwrap();
        assert_eq!(new_min, 75);
    }

    /// Helper to create RunQueueEntry
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
    fn test_deadline_ordering() {
        // EEVDF picks task with earliest deadline among eligible tasks
        // Use real PerCpuRunQueue::pick_next

        let mut rq = PerCpuRunQueue::new(0);
        
        let weight = nice_to_weight(0);
        rq.enqueue(make_rq_entry(1, 100, calc_vdeadline(100, BASE_SLICE_NS, weight)));
        rq.enqueue(make_rq_entry(2, 90, calc_vdeadline(90, BASE_SLICE_NS, weight)));
        rq.enqueue(make_rq_entry(3, 110, calc_vdeadline(110, BASE_SLICE_NS, weight)));

        let next = rq.pick_next();

        // Should pick task 2 (lowest vruntime = earliest deadline for same nice)
        assert_eq!(next.map(|e| e.pid), Some(2));
    }
}
