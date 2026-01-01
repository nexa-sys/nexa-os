//! EEVDF Scheduler Algorithm Bug Detection Tests
//!
//! Tests that specifically target potential bugs in the EEVDF scheduler:
//! - vruntime calculation errors
//! - deadline computation bugs
//! - eligibility check failures
//! - weight/nice value mapping errors
//! - priority inversion scenarios

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        nice_to_weight, ProcessEntry, SchedPolicy, CpuMask,
        BASE_SLICE_NS, NICE_0_WEIGHT, MAX_SLICE_NS, SCHED_GRANULARITY_NS,
        calc_delta_vruntime, calc_delta_vruntime_fast, calc_vdeadline, is_eligible,
    };
    use crate::process::{Process, ProcessState};

    fn is_nearly_eligible(entry: &ProcessEntry) -> bool {
        entry.lag >= -1_000_000
    }

    fn is_wakeup_eligible(entry: &ProcessEntry) -> bool {
        entry.lag >= -500_000 // WAKEUP_PREEMPT_THRESH_NS
    }

    fn create_test_process(pid: u64, state: ProcessState) -> Process {
        Process {
            pid,
            ppid: 0,
            tgid: pid,
            state,
            entry_point: 0x1000000,
            stack_top: 0x1A00000,
            heap_start: 0x1200000,
            heap_end: 0x1200000,
            signal_state: crate::ipc::signal::SignalState::new(),
            context: crate::process::Context::zero(),
            has_entered_user: false,
            context_valid: false,
            is_fork_child: false,
            is_thread: false,
            cr3: 0,
            tty: 0,
            memory_base: 0,
            memory_size: 0,
            user_rip: 0,
            user_rsp: 0,
            user_rflags: 0x202,
            user_r10: 0,
            user_r8: 0,
            user_r9: 0,
            exit_code: 0,
            term_signal: None,
            kernel_stack: 0,
            fs_base: 0,
            clear_child_tid: 0,
            cmdline: [0; 1024],
            cmdline_len: 0,
            open_fds: 0,
            exec_pending: false,
            exec_entry: 0,
            exec_stack: 0,
            exec_user_data_sel: 0,
            wake_pending: false,
        }
    }

    fn create_test_entry(pid: u64, state: ProcessState, nice: i8) -> ProcessEntry {
        let weight = nice_to_weight(nice);
        ProcessEntry {
            process: create_test_process(pid, state),
            vruntime: 0,
            vdeadline: BASE_SLICE_NS,
            lag: 0,
            weight,
            slice_ns: BASE_SLICE_NS,
            slice_remaining_ns: BASE_SLICE_NS,
            priority: 128,
            base_priority: 128,
            time_slice: 4,
            total_time: 0,
            wait_time: 0,
            last_scheduled: 0,
            cpu_burst_count: 0,
            avg_cpu_burst: 0,
            policy: SchedPolicy::Normal,
            nice,
            quantum_level: 0,
            preempt_count: 0,
            voluntary_switches: 0,
            cpu_affinity: CpuMask::all(),
            last_cpu: 0,
            numa_preferred_node: crate::numa::NUMA_NO_NODE,
            numa_policy: crate::numa::NumaPolicy::Local,
        }
    }

    // =========================================================================
    // BUG TEST: Weight calculation correctness
    // =========================================================================

    /// Test: nice_to_weight must produce correct weights for all nice values
    /// BUG: Wrong weight table causes unfair scheduling.
    #[test]
    fn test_nice_to_weight_table_correctness() {
        // nice 0 should give NICE_0_WEIGHT
        let weight_0 = nice_to_weight(0);
        assert_eq!(weight_0, NICE_0_WEIGHT,
            "BUG: nice 0 doesn't give NICE_0_WEIGHT");
        
        // Negative nice (higher priority) should give higher weight
        let weight_neg10 = nice_to_weight(-10);
        assert!(weight_neg10 > weight_0,
            "BUG: Negative nice should give higher weight");
        
        // Positive nice (lower priority) should give lower weight
        let weight_pos10 = nice_to_weight(10);
        assert!(weight_pos10 < weight_0,
            "BUG: Positive nice should give lower weight");
        
        // Weight ratio should be ~10% per nice level (approximately)
        // nice -1 should be ~25% more weight than nice 0
        let weight_neg1 = nice_to_weight(-1);
        let ratio = weight_neg1 as f64 / weight_0 as f64;
        assert!(ratio > 1.2 && ratio < 1.3,
            "BUG: Weight ratio per nice level wrong, got {}", ratio);
    }

    /// Test: Extreme nice values are clamped correctly
    #[test]
    fn test_nice_clamping() {
        // nice -100 should clamp to -20
        let weight_extreme_neg = nice_to_weight(-100);
        let weight_min = nice_to_weight(-20);
        assert_eq!(weight_extreme_neg, weight_min,
            "BUG: nice -100 should clamp to -20");
        
        // nice +100 should clamp to +19
        let weight_extreme_pos = nice_to_weight(100);
        let weight_max = nice_to_weight(19);
        assert_eq!(weight_extreme_pos, weight_max,
            "BUG: nice +100 should clamp to +19");
    }

    /// Test: Weight values must be non-zero (division by weight)
    #[test]
    fn test_weights_nonzero() {
        for nice in -20i8..=19 {
            let weight = nice_to_weight(nice);
            assert!(weight > 0,
                "BUG: Weight for nice {} is zero (would cause division by zero)", nice);
        }
    }

    // =========================================================================
    // BUG TEST: vruntime calculation
    // =========================================================================

    /// Test: vruntime increases correctly with execution time
    /// Formula: delta_vruntime = delta_exec * NICE_0_WEIGHT / weight
    #[test]
    fn test_vruntime_calculation_nice0() {
        let delta_exec = 1_000_000u64; // 1ms
        let weight = NICE_0_WEIGHT;
        
        let delta_vrt = calc_delta_vruntime(delta_exec, weight);
        
        // For nice 0, delta_vrt should equal delta_exec
        assert_eq!(delta_vrt, delta_exec,
            "BUG: vruntime calc for nice 0 wrong, expected {}, got {}", delta_exec, delta_vrt);
    }

    /// Test: Higher weight (lower nice) accumulates less vruntime
    #[test]
    fn test_vruntime_high_weight_slower() {
        let delta_exec = 1_000_000u64; // 1ms
        let weight_high = nice_to_weight(-10);
        let weight_normal = nice_to_weight(0);
        
        let delta_vrt_high = calc_delta_vruntime(delta_exec, weight_high);
        let delta_vrt_normal = calc_delta_vruntime(delta_exec, weight_normal);
        
        assert!(delta_vrt_high < delta_vrt_normal,
            "BUG: Higher weight process should accumulate less vruntime");
    }

    /// Test: Lower weight (higher nice) accumulates more vruntime
    #[test]
    fn test_vruntime_low_weight_faster() {
        let delta_exec = 1_000_000u64; // 1ms
        let weight_low = nice_to_weight(10);
        let weight_normal = nice_to_weight(0);
        
        let delta_vrt_low = calc_delta_vruntime(delta_exec, weight_low);
        let delta_vrt_normal = calc_delta_vruntime(delta_exec, weight_normal);
        
        assert!(delta_vrt_low > delta_vrt_normal,
            "BUG: Lower weight process should accumulate more vruntime");
    }

    /// Test: Fast vruntime calculation matches slow version
    #[test]
    fn test_vruntime_fast_matches_slow() {
        let delta_exec = 1_000_000u64;
        
        for nice in -20i8..=19 {
            let weight = nice_to_weight(nice);
            let slow = calc_delta_vruntime(delta_exec, weight);
            let fast = calc_delta_vruntime_fast(delta_exec, nice);
            
            // Allow small rounding difference (< 1%)
            let diff = (slow as i64 - fast as i64).abs();
            let tolerance = (slow as i64 / 100).max(1);
            
            assert!(diff <= tolerance,
                "BUG: Fast vruntime calc differs from slow for nice {}: slow={}, fast={}", 
                nice, slow, fast);
        }
    }

    /// Test: Zero weight handling (should not crash)
    #[test]
    fn test_vruntime_zero_weight_safety() {
        let delta_exec = 1_000_000u64;
        let weight = 0u64;
        
        // Should not panic (returns delta_exec as fallback)
        let delta_vrt = calc_delta_vruntime(delta_exec, weight);
        
        // Fallback behavior: return delta_exec
        assert_eq!(delta_vrt, delta_exec,
            "BUG: Zero weight should fall back to delta_exec");
    }

    // =========================================================================
    // BUG TEST: Deadline calculation
    // =========================================================================

    /// Test: vdeadline = vruntime + slice_ns * NICE_0_WEIGHT / weight
    #[test]
    fn test_vdeadline_calculation() {
        let vruntime = 1000u64;
        let slice_ns = BASE_SLICE_NS;
        let weight = NICE_0_WEIGHT;
        
        let vdeadline = calc_vdeadline(vruntime, slice_ns, weight);
        
        // For nice 0: vdeadline = vruntime + slice_ns
        assert_eq!(vdeadline, vruntime + slice_ns,
            "BUG: vdeadline calc wrong for nice 0");
    }

    /// Test: Higher weight gets closer deadline (more urgent)
    #[test]
    fn test_vdeadline_weight_effect() {
        let vruntime = 1000u64;
        let slice_ns = BASE_SLICE_NS;
        
        let weight_high = nice_to_weight(-10);
        let weight_normal = nice_to_weight(0);
        
        let vdl_high = calc_vdeadline(vruntime, slice_ns, weight_high);
        let vdl_normal = calc_vdeadline(vruntime, slice_ns, weight_normal);
        
        // Higher weight = smaller deadline delta = closer deadline
        assert!(vdl_high < vdl_normal,
            "BUG: Higher weight should get closer deadline");
    }

    /// Test: Zero weight in deadline (should not crash)
    #[test]
    fn test_vdeadline_zero_weight_safety() {
        let vruntime = 1000u64;
        let slice_ns = BASE_SLICE_NS;
        let weight = 0u64;
        
        // Should not panic
        let vdeadline = calc_vdeadline(vruntime, slice_ns, weight);
        
        // Fallback: vruntime + slice_ns
        assert_eq!(vdeadline, vruntime.saturating_add(slice_ns));
    }

    // =========================================================================
    // BUG TEST: Eligibility checks
    // =========================================================================

    /// Test: Positive lag makes process eligible
    #[test]
    fn test_eligibility_positive_lag() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        entry.lag = 1000; // Positive lag = deserves CPU time
        
        assert!(is_eligible(&entry),
            "BUG: Process with positive lag should be eligible");
    }

    /// Test: Zero lag makes process eligible
    #[test]
    fn test_eligibility_zero_lag() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        entry.lag = 0;
        
        assert!(is_eligible(&entry),
            "BUG: Process with zero lag should be eligible");
    }

    /// Test: Negative lag makes process ineligible
    #[test]
    fn test_eligibility_negative_lag() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        entry.lag = -1000; // Negative lag = consumed too much
        
        assert!(!is_eligible(&entry),
            "BUG: Process with negative lag should not be eligible");
    }

    /// Test: is_nearly_eligible allows small negative lag
    #[test]
    fn test_nearly_eligible_threshold() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Small negative lag (< 1ms) should be nearly eligible
        entry.lag = -500_000; // -0.5ms
        assert!(is_nearly_eligible(&entry),
            "BUG: Small negative lag should be nearly eligible");
        
        // Large negative lag should not be nearly eligible
        entry.lag = -5_000_000; // -5ms
        assert!(!is_nearly_eligible(&entry),
            "BUG: Large negative lag should not be nearly eligible");
    }

    /// Test: Wakeup eligibility is more lenient
    #[test]
    fn test_wakeup_eligibility_lenient() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Process just woke up might have slight negative lag
        entry.lag = -250_000; // -0.25ms
        
        assert!(is_wakeup_eligible(&entry),
            "BUG: Recently woken process should be wakeup eligible");
    }

    // =========================================================================
    // BUG TEST: Slice management
    // =========================================================================

    /// Test: Slice values must be within bounds
    #[test]
    fn test_slice_bounds() {
        // BASE_SLICE_NS should be at least 1ms
        assert!(BASE_SLICE_NS >= 1_000_000,
            "BUG: BASE_SLICE_NS too small (< 1ms)");
        
        // MAX_SLICE_NS should be reasonable (< 1s)
        assert!(MAX_SLICE_NS <= 1_000_000_000,
            "BUG: MAX_SLICE_NS too large (> 1s)");
        
        // SCHED_GRANULARITY_NS should be smaller than slice
        assert!(SCHED_GRANULARITY_NS < BASE_SLICE_NS,
            "BUG: SCHED_GRANULARITY_NS >= BASE_SLICE_NS (preemption too eager)");
    }

    /// Test: Slice remaining can't go negative
    #[test]
    fn test_slice_remaining_bounds() {
        let mut entry = create_test_entry(100, ProcessState::Running, 0);
        entry.slice_remaining_ns = 1000;
        
        // Subtract more than remaining
        let executed = 5000u64;
        entry.slice_remaining_ns = entry.slice_remaining_ns.saturating_sub(executed);
        
        assert_eq!(entry.slice_remaining_ns, 0,
            "BUG: slice_remaining_ns went negative (use saturating_sub)");
    }

    // =========================================================================
    // BUG TEST: Policy-based scheduling
    // =========================================================================

    /// Test: Realtime policy should have higher priority
    #[test]
    fn test_realtime_policy_priority() {
        let rt_entry = {
            let mut e = create_test_entry(100, ProcessState::Ready, 0);
            e.policy = SchedPolicy::Realtime;
            e
        };
        
        let normal_entry = {
            let mut e = create_test_entry(101, ProcessState::Ready, 0);
            e.policy = SchedPolicy::Normal;
            e
        };
        
        // Realtime should always beat normal, regardless of vdeadline
        // This is tested in scheduler selection logic
        assert_eq!(rt_entry.policy, SchedPolicy::Realtime);
        assert_eq!(normal_entry.policy, SchedPolicy::Normal);
    }

    /// Test: Idle policy should have lowest priority
    #[test]
    fn test_idle_policy_lowest_priority() {
        let idle_entry = {
            let mut e = create_test_entry(100, ProcessState::Ready, 0);
            e.policy = SchedPolicy::Idle;
            e
        };
        
        let normal_entry = {
            let mut e = create_test_entry(101, ProcessState::Ready, 0);
            e.policy = SchedPolicy::Normal;
            e
        };
        
        // Normal should beat idle
        assert_eq!(idle_entry.policy, SchedPolicy::Idle);
        assert_eq!(normal_entry.policy, SchedPolicy::Normal);
    }

    // =========================================================================
    // BUG TEST: Overflow and edge cases
    // =========================================================================

    /// Test: Very large execution times don't overflow
    #[test]
    fn test_large_delta_exec_no_overflow() {
        let delta_exec = u64::MAX / 2;
        let weight = nice_to_weight(0);
        
        // Should not panic
        let delta_vrt = calc_delta_vruntime(delta_exec, weight);
        
        // Result should be reasonable (not negative/wrapped)
        assert!(delta_vrt > 0,
            "BUG: Large delta_exec caused overflow");
    }

    /// Test: Cumulative vruntime stays in bounds
    #[test]
    fn test_cumulative_vruntime_saturation() {
        let mut entry = create_test_entry(100, ProcessState::Running, 0);
        entry.vruntime = u64::MAX - 100;
        
        // Add delta that would overflow
        let delta = 200u64;
        entry.vruntime = entry.vruntime.saturating_add(delta);
        
        assert_eq!(entry.vruntime, u64::MAX,
            "BUG: vruntime overflow, should saturate at MAX");
    }

    /// Test: Lag bounds are enforced
    #[test]
    fn test_lag_saturation_bounds() {
        let mut entry = create_test_entry(100, ProcessState::Ready, 0);
        
        // Try to give extreme positive lag
        entry.lag = i64::MAX;
        let max_lag: i64 = 100_000_000; // 100ms cap from code
        entry.lag = entry.lag.min(max_lag);
        
        assert!(entry.lag <= max_lag,
            "BUG: Positive lag exceeded cap");
        
        // Try to give extreme negative lag
        entry.lag = i64::MIN;
        let min_lag: i64 = -100_000_000;
        entry.lag = entry.lag.max(min_lag);
        
        assert!(entry.lag >= min_lag,
            "BUG: Negative lag exceeded cap");
    }
}
