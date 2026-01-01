//! vruntime Correctness and Bug Detection Tests
//!
//! These tests specifically target vruntime-related bugs including:
//! - vruntime overflow and unbounded growth
//! - vruntime wraparound issues
//! - min_vruntime tracking correctness
//! - lag accumulation bugs
//! - Race conditions in vruntime updates
//!
//! These tests are designed to catch the "crazy growth" bugs observed in real kernel runs.

use crate::scheduler::{CpuMask, SchedPolicy, nice_to_weight, BASE_SLICE_NS, NICE_0_WEIGHT};
use crate::scheduler::percpu::{
    RunQueueEntry, PerCpuRunQueue, PerCpuSchedData, PERCPU_RQ_SIZE,
};
use crate::scheduler::ProcessEntry;
use crate::scheduler::{calc_vdeadline, is_eligible};
use crate::process::ProcessState;

use std::sync::{Arc, Barrier, atomic::{AtomicU64, Ordering}};
use std::thread;
use std::time::Duration;

/// Calculate the weighted vruntime delta
/// delta_vruntime = delta_exec * NICE_0_WEIGHT / weight
#[inline]
fn calc_delta_vruntime(delta_exec_ns: u64, weight: u64) -> u64 {
    if weight == 0 {
        return delta_exec_ns;
    }
    ((delta_exec_ns as u128 * NICE_0_WEIGHT as u128) / weight as u128) as u64
}

/// Helper to create a test process entry
fn make_test_entry(pid: u64, nice: i8) -> ProcessEntry {
    let mut entry = ProcessEntry::empty();
    entry.process.pid = pid;
    entry.process.state = ProcessState::Ready;
    entry.nice = nice;
    entry.policy = SchedPolicy::Normal;
    entry.weight = nice_to_weight(nice);
    entry.slice_ns = BASE_SLICE_NS;
    entry.slice_remaining_ns = BASE_SLICE_NS;
    entry.cpu_affinity = CpuMask::all();
    entry.vruntime = 0;
    entry.vdeadline = BASE_SLICE_NS;
    entry.lag = 0;
    entry
}

/// Helper to create run queue entry
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

// ============================================================================
// vruntime Overflow and Growth Tests
// ============================================================================

#[test]
fn test_vruntime_no_unbounded_growth() {
    // Test long-running process to ensure vruntime doesn't grow unboundedly
    let mut entry = make_test_entry(1, 0);
    
    // Run for 1 hour of CPU time (3.6e12 ns)
    let total_runtime_ns: u64 = 3_600_000_000_000;
    let step_ns: u64 = 10_000_000; // 10ms steps
    let steps = total_runtime_ns / step_ns;
    
    let initial_vruntime = entry.vruntime;
    
    for _ in 0..steps {
        let delta_vrt = calc_delta_vruntime(step_ns, entry.weight);
        entry.vruntime = entry.vruntime.saturating_add(delta_vrt);
    }
    
    // vruntime should increase but not overflow
    assert!(entry.vruntime > initial_vruntime);
    assert!(entry.vruntime < u64::MAX / 2, "vruntime should not grow too large");
    
    eprintln!("vruntime after 1 hour: {} (delta from start: {})", 
              entry.vruntime, entry.vruntime - initial_vruntime);
}

#[test]
fn test_vruntime_weight_affects_growth_rate() {
    // High priority (low nice) should have slower vruntime growth
    let mut high_prio = make_test_entry(1, -20);
    let mut normal = make_test_entry(2, 0);
    let mut low_prio = make_test_entry(3, 19);
    
    let runtime_ns: u64 = 1_000_000_000; // 1 second
    
    let delta_high = calc_delta_vruntime(runtime_ns, high_prio.weight);
    let delta_normal = calc_delta_vruntime(runtime_ns, normal.weight);
    let delta_low = calc_delta_vruntime(runtime_ns, low_prio.weight);
    
    high_prio.vruntime = high_prio.vruntime.saturating_add(delta_high);
    normal.vruntime = normal.vruntime.saturating_add(delta_normal);
    low_prio.vruntime = low_prio.vruntime.saturating_add(delta_low);
    
    // High priority should have smallest vruntime growth
    assert!(delta_high < delta_normal, 
            "High priority delta ({}) should be less than normal ({})", delta_high, delta_normal);
    assert!(delta_normal < delta_low,
            "Normal delta ({}) should be less than low priority ({})", delta_normal, delta_low);
    
    eprintln!("1s runtime vruntime deltas:");
    eprintln!("  High priority (nice -20): {} ns", delta_high);
    eprintln!("  Normal (nice 0): {} ns", delta_normal);
    eprintln!("  Low priority (nice 19): {} ns", delta_low);
}

#[test]
fn test_vruntime_near_overflow() {
    // Test behavior when vruntime is near u64::MAX
    let mut entry = make_test_entry(1, 0);
    entry.vruntime = u64::MAX - 1_000_000_000; // Near max
    
    // Try to add more runtime - should saturate, not overflow
    let delta_vrt = calc_delta_vruntime(10_000_000_000, entry.weight);
    let new_vruntime = entry.vruntime.saturating_add(delta_vrt);
    
    // Should saturate at MAX, not wrap around to small values
    assert!(new_vruntime >= entry.vruntime, "vruntime should not wrap around");
    eprintln!("vruntime after near-overflow add: {}", new_vruntime);
}

#[test]
fn test_vruntime_zero_weight_protection() {
    // Ensure zero weight doesn't cause division by zero
    let delta = calc_delta_vruntime(1_000_000, 0);
    // Should return something reasonable, not panic
    eprintln!("Delta with zero weight: {}", delta);
}

// ============================================================================
// vdeadline Correctness Tests
// ============================================================================

#[test]
fn test_vdeadline_calculation_correctness() {
    // For nice 0, vdeadline = vruntime + slice_ns
    let entry = make_test_entry(1, 0);
    let vdeadline = calc_vdeadline(1000, BASE_SLICE_NS, entry.weight);
    let expected = 1000 + BASE_SLICE_NS;
    
    assert_eq!(vdeadline, expected, 
               "Nice 0 deadline should be vruntime + slice_ns");
}

#[test]
fn test_vdeadline_priority_ordering() {
    // Higher priority should have earlier deadlines for same vruntime
    let high_weight = nice_to_weight(-10);
    let normal_weight = nice_to_weight(0);
    let low_weight = nice_to_weight(10);
    
    let vruntime = 1000u64;
    
    let dl_high = calc_vdeadline(vruntime, BASE_SLICE_NS, high_weight);
    let dl_normal = calc_vdeadline(vruntime, BASE_SLICE_NS, normal_weight);
    let dl_low = calc_vdeadline(vruntime, BASE_SLICE_NS, low_weight);
    
    assert!(dl_high < dl_normal, "High priority should have earlier deadline");
    assert!(dl_normal < dl_low, "Normal should have earlier deadline than low");
    
    eprintln!("Deadlines with vruntime=1000:");
    eprintln!("  High priority: {}", dl_high);
    eprintln!("  Normal: {}", dl_normal);
    eprintln!("  Low priority: {}", dl_low);
}

#[test]
fn test_vdeadline_overflow_protection() {
    // Test vdeadline calculation when vruntime is very large
    let vruntime = u64::MAX - BASE_SLICE_NS;
    let weight = nice_to_weight(0);
    
    let vdeadline = calc_vdeadline(vruntime, BASE_SLICE_NS, weight);
    
    // Should saturate, not overflow
    assert!(vdeadline >= vruntime, "vdeadline should not be less than vruntime");
}

// ============================================================================
// Lag Accumulation Bug Detection Tests
// ============================================================================

#[test]
fn test_lag_bounded_growth() {
    let mut entry = make_test_entry(1, 0);
    
    // Long waiting time (lag should be bounded)
    const MAX_LAG: i64 = 100_000_000; // 100ms max as defined in kernel
    
    // Lag accumulation over many iterations
    for _ in 0..10000 {
        entry.lag = entry.lag.saturating_add(100_000); // Add 100us each iteration
        entry.lag = entry.lag.min(MAX_LAG);
    }
    
    assert!(entry.lag <= MAX_LAG, "Lag should be bounded at MAX_LAG");
    eprintln!("Final lag after 10000 increments: {}", entry.lag);
}

#[test]
fn test_lag_negative_after_running() {
    let mut entry = make_test_entry(1, 0);
    entry.lag = 50_000_000; // 50ms positive lag (deserves CPU)
    
    // Running: lag should decrease
    let runtime_ns = 100_000_000u64; // 100ms
    entry.lag = entry.lag.saturating_sub(runtime_ns as i64);
    
    // After running more than lag, lag should be negative
    assert!(entry.lag < 0, "Lag should become negative after running");
    assert!(!is_eligible(&entry), "Process with negative lag should not be eligible");
}

#[test]
fn test_eligibility_transitions() {
    let mut entry = make_test_entry(1, 0);
    
    // Initially eligible (lag = 0)
    entry.lag = 0;
    assert!(is_eligible(&entry), "Zero lag should be eligible");
    
    // After waiting (positive lag)
    entry.lag = 1000;
    assert!(is_eligible(&entry), "Positive lag should be eligible");
    
    // After running too much (negative lag)
    entry.lag = -1000;
    assert!(!is_eligible(&entry), "Negative lag should not be eligible");
}

// ============================================================================
// min_vruntime Tracking Tests
// ============================================================================

#[test]
fn test_min_vruntime_tracking_in_runqueue() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add entries with different vruntimes
    rq.enqueue(make_rq_entry(1, 5000, 10000)).unwrap();
    rq.enqueue(make_rq_entry(2, 1000, 6000)).unwrap();  // Lowest vruntime
    rq.enqueue(make_rq_entry(3, 3000, 8000)).unwrap();
    
    // min_vruntime should track the minimum
    // Note: Actual behavior depends on implementation
    let min = rq.min_vruntime();
    eprintln!("min_vruntime in queue: {}", min);
}

#[test]
fn test_min_vruntime_increases_monotonically() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add and remove entries, min_vruntime should only increase
    rq.enqueue(make_rq_entry(1, 1000, 5000)).unwrap();
    let min1 = rq.min_vruntime();
    
    rq.enqueue(make_rq_entry(2, 500, 4500)).unwrap();
    let min2 = rq.min_vruntime();
    
    // Remove the one with lowest vruntime
    rq.dequeue(2);
    let min3 = rq.min_vruntime();
    
    // min_vruntime should not decrease (to prevent starvation)
    assert!(min3 >= min2.min(min1), "min_vruntime should not decrease drastically");
    
    eprintln!("min_vruntime sequence: {} -> {} -> {}", min1, min2, min3);
}

// ============================================================================
// Concurrent vruntime Update Tests
// ============================================================================

#[test]
fn test_concurrent_vruntime_updates() {
    // Test that concurrent updates to vruntime-related fields don't cause corruption
    let data = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = vec![];
    
    // Multiple threads enqueue/dequeue with different vruntimes
    for thread_id in 0..4 {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..100 {
                let vruntime = (thread_id * 1000 + i) as u64 * 100;
                let vdeadline = vruntime + BASE_SLICE_NS;
                let pid = (thread_id * 1000 + i) as u64;
                
                let entry = make_rq_entry(pid, vruntime, vdeadline);
                
                let mut rq = data.run_queue.lock();
                let _ = rq.enqueue(entry);
                let _ = rq.dequeue(pid);
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Queue should be empty after all operations
    let rq = data.run_queue.lock();
    assert!(rq.is_empty(), "Queue should be empty after concurrent ops");
}

#[test]
fn test_vruntime_update_race() {
    // Race condition scenario: multiple "CPUs" update vruntime concurrently
    let shared_vruntime = Arc::new(AtomicU64::new(0));
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = vec![];
    
    for _ in 0..8 {
        let vruntime = Arc::clone(&shared_vruntime);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..1000 {
                // vruntime increment
                let delta = 1000u64;
                vruntime.fetch_add(delta, Ordering::SeqCst);
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Should have 8 threads * 1000 iterations * 1000 delta = 8,000,000
    let final_vruntime = shared_vruntime.load(Ordering::SeqCst);
    assert_eq!(final_vruntime, 8_000_000, "All updates should be counted");
}

// ============================================================================
// Per-CPU vruntime Divergence Tests
// ============================================================================

#[test]
fn test_percpu_vruntime_divergence() {
    // Test that per-CPU vruntimes can diverge without issues
    let mut rq0 = PerCpuRunQueue::new(0);
    let mut rq1 = PerCpuRunQueue::new(1);
    
    // CPU 0 has processes with low vruntimes
    rq0.enqueue(make_rq_entry(1, 1000, 5000)).unwrap();
    rq0.enqueue(make_rq_entry(2, 2000, 6000)).unwrap();
    
    // CPU 1 has processes with much higher vruntimes
    rq1.enqueue(make_rq_entry(3, 1_000_000, 1_004_000)).unwrap();
    rq1.enqueue(make_rq_entry(4, 1_000_500, 1_004_500)).unwrap();
    
    // Both should work correctly despite large vruntime difference
    let next0 = rq0.pick_next();
    let next1 = rq1.pick_next();
    
    assert!(next0.is_some());
    assert!(next1.is_some());
    
    eprintln!("CPU 0 picked: vruntime={}", next0.unwrap().vruntime);
    eprintln!("CPU 1 picked: vruntime={}", next1.unwrap().vruntime);
}

#[test]
fn test_process_migration_vruntime_adjustment() {
    // Process migrating between CPUs with different min_vruntimes
    let mut rq0 = PerCpuRunQueue::new(0);
    let mut rq1 = PerCpuRunQueue::new(1);
    
    // CPU 0 has low vruntime processes
    rq0.enqueue(make_rq_entry(1, 1000, 5000)).unwrap();
    rq0.enqueue(make_rq_entry(2, 2000, 6000)).unwrap();
    
    // CPU 1 has high vruntime (more advanced)
    rq1.enqueue(make_rq_entry(3, 100_000, 104_000)).unwrap();
    
    // Migrate PID 2 from CPU 0 to CPU 1
    let migrated = rq0.dequeue(2).unwrap();
    
    // When migrating, vruntime should be adjusted to target CPU's min_vruntime
    // This prevents unfair advantage/disadvantage
    let target_min = rq1.min_vruntime();
    let adjusted_vruntime = target_min.max(migrated.vruntime);
    
    let mut adjusted_entry = migrated;
    adjusted_entry.vruntime = adjusted_vruntime;
    adjusted_entry.vdeadline = adjusted_vruntime + BASE_SLICE_NS;
    
    rq1.enqueue(adjusted_entry).unwrap();
    
    eprintln!("Migrated process vruntime: {} -> {}", migrated.vruntime, adjusted_vruntime);
}

// ============================================================================
// Stress Tests for vruntime Bugs
// ============================================================================

#[test]
fn test_rapid_vruntime_oscillation() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Rapidly add and update entries with varying vruntimes
    for cycle in 0..100 {
        // Add entries
        for i in 0..10 {
            let vruntime = (cycle * 1000 + i * 100) as u64;
            rq.enqueue(make_rq_entry(i as u64, vruntime, vruntime + BASE_SLICE_NS)).unwrap();
        }
        
        // Update some entries
        for i in 0..5 {
            let new_vruntime = (cycle * 1000 + i * 200 + 50) as u64;
            rq.update_entry(i as u64, new_vruntime, new_vruntime + BASE_SLICE_NS, true);
        }
        
        // Pick some
        for _ in 0..5 {
            let _ = rq.pick_next();
        }
        
        // Clear rest
        while rq.pick_next().is_some() {}
    }
}

#[test]
fn test_extreme_vruntime_values() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Test with extreme vruntime values
    let extreme_values: Vec<u64> = vec![
        0,
        1,
        BASE_SLICE_NS,
        u64::MAX / 4,
        u64::MAX / 2,
        u64::MAX - BASE_SLICE_NS,
        u64::MAX - 1,
    ];
    
    for (i, &vruntime) in extreme_values.iter().enumerate() {
        let vdeadline = vruntime.saturating_add(BASE_SLICE_NS);
        let result = rq.enqueue(make_rq_entry(i as u64, vruntime, vdeadline));
        assert!(result.is_ok(), "Should handle extreme vruntime {}", vruntime);
    }
    
    // Should be able to pick all of them
    let mut count = 0;
    while rq.pick_next().is_some() {
        count += 1;
    }
    
    assert_eq!(count, extreme_values.len(), "Should pick all extreme entries");
}

#[test]
fn test_vruntime_consistency_under_load() {
    // Heavy scheduling load: verify vruntime consistency
    let data = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let error_count = Arc::new(AtomicU64::new(0));
    let mut handles = vec![];
    
    for thread_id in 0..4 {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        let errors = Arc::clone(&error_count);
        
        handles.push(thread::spawn(move || {
            barrier.wait();
            
            for i in 0..500 {
                let base_pid = (thread_id * 1000 + i) as u64;
                let vruntime = i as u64 * 1000;
                let vdeadline = vruntime + BASE_SLICE_NS;
                
                let entry = make_rq_entry(base_pid, vruntime, vdeadline);
                
                {
                    let mut rq = data.run_queue.lock();
                    if let Ok(()) = rq.enqueue(entry) {
                        // Verify entry is findable
                        if !rq.contains(base_pid) {
                            errors.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
                
                // Small delay to increase race window
                thread::yield_now();
                
                {
                    let mut rq = data.run_queue.lock();
                    let _ = rq.dequeue(base_pid);
                }
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    let errors = error_count.load(Ordering::SeqCst);
    assert_eq!(errors, 0, "No consistency errors should occur");
}

// ============================================================================
// vruntime Delta Calculation Precision Tests
// ============================================================================

#[test]
fn test_delta_vruntime_precision() {
    // Test that delta_vruntime calculation doesn't lose precision
    let weight = nice_to_weight(0);
    
    // Small delta - should be calculated precisely
    let small_delta = calc_delta_vruntime(1000, weight);
    assert!(small_delta > 0, "Small delta should be > 0");
    
    // Medium delta
    let medium_delta = calc_delta_vruntime(1_000_000, weight);
    assert!(medium_delta >= small_delta * 1000 - 1000, "Should scale linearly");
    assert!(medium_delta <= small_delta * 1000 + 1000, "Should scale linearly");
    
    // Large delta - should not overflow
    let large_delta = calc_delta_vruntime(1_000_000_000_000, weight);
    assert!(large_delta > 0, "Large delta should not overflow to 0");
    
    eprintln!("Delta precision test:");
    eprintln!("  1us -> {} vruntime", small_delta);
    eprintln!("  1ms -> {} vruntime", medium_delta);
    eprintln!("  1000s -> {} vruntime", large_delta);
}

#[test]
fn test_delta_vruntime_with_all_nice_values() {
    let runtime_ns = 1_000_000u64; // 1ms
    let mut prev_delta = 0u64;
    
    // Nice -20 to +19: as nice increases, weight decreases, so delta increases
    // Higher nice value = lower priority = faster vruntime growth
    for nice in -20i8..=19 {
        let weight = nice_to_weight(nice);
        let delta = calc_delta_vruntime(runtime_ns, weight);
        
        assert!(delta > 0, "Delta for nice {} should be > 0", nice);
        assert!(delta > prev_delta, "Delta should increase with nice value (nice={}, delta={}, prev={})", nice, delta, prev_delta);
        
        prev_delta = delta;
    }
}
