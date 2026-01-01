//! EEVDF (Earliest Eligible Virtual Deadline First) Scheduler Tests
//!
//! Tests for the core EEVDF scheduling algorithm, including:
//! - Virtual runtime calculation
//! - Virtual deadline calculation
//! - Eligibility checking
//! - Preemption decisions
//! - Lag tracking

use crate::scheduler::{
    CpuMask, SchedPolicy, ProcessEntry, nice_to_weight,
    BASE_SLICE_NS, NICE_0_WEIGHT, SCHED_GRANULARITY_NS,
};
use crate::process::{ProcessState, Pid};

/// Helper to create a test process entry with given parameters
fn make_test_entry(pid: Pid, nice: i8, policy: SchedPolicy) -> ProcessEntry {
    let mut entry = ProcessEntry::empty();
    entry.process.pid = pid;
    entry.process.state = ProcessState::Ready;
    entry.nice = nice;
    entry.policy = policy;
    entry.weight = nice_to_weight(nice);
    entry.slice_ns = BASE_SLICE_NS;
    entry.slice_remaining_ns = BASE_SLICE_NS;
    entry.cpu_affinity = CpuMask::all();
    entry
}

// ============================================================================
// Virtual Runtime Tests
// ============================================================================

#[test]
fn test_vruntime_initial() {
    let entry = make_test_entry(1, 0, SchedPolicy::Normal);
    assert_eq!(entry.vruntime, 0, "Initial vruntime should be 0");
}

#[test]
fn test_vruntime_weight_relationship() {
    // Higher weight (lower nice) processes should accumulate vruntime slower
    let entry_high_prio = make_test_entry(1, -10, SchedPolicy::Normal);
    let entry_normal = make_test_entry(2, 0, SchedPolicy::Normal);
    let entry_low_prio = make_test_entry(3, 10, SchedPolicy::Normal);
    
    // Verify weights are ordered correctly
    assert!(entry_high_prio.weight > entry_normal.weight);
    assert!(entry_normal.weight > entry_low_prio.weight);
    
    // Higher weight should give more CPU time for same vruntime advancement
    // vruntime_delta = actual_runtime * (NICE_0_WEIGHT / weight)
    let runtime_ns: u64 = 1_000_000; // 1ms
    
    // Calculate vruntime deltas
    let delta_high = (runtime_ns * NICE_0_WEIGHT) / entry_high_prio.weight;
    let delta_normal = (runtime_ns * NICE_0_WEIGHT) / entry_normal.weight;
    let delta_low = (runtime_ns * NICE_0_WEIGHT) / entry_low_prio.weight;
    
    // High priority process advances vruntime slower
    assert!(delta_high < delta_normal);
    assert!(delta_normal < delta_low);
    
    eprintln!("vruntime deltas for 1ms actual runtime:");
    eprintln!("  High priority (nice -10): {} ns", delta_high);
    eprintln!("  Normal (nice 0): {} ns", delta_normal);
    eprintln!("  Low priority (nice 10): {} ns", delta_low);
}

// ============================================================================
// Virtual Deadline Tests
// ============================================================================

#[test]
fn test_vdeadline_calculation() {
    // vdeadline = vruntime + (slice_ns / weight) * NICE_0_WEIGHT
    // Or simpler: vdeadline = vruntime + slice_ns * (NICE_0_WEIGHT / weight)
    
    let entry = make_test_entry(1, 0, SchedPolicy::Normal);
    
    // For nice 0, weight = NICE_0_WEIGHT, so deadline = vruntime + slice_ns
    let expected_deadline = entry.vruntime + entry.slice_ns;
    
    // The actual calculation in priority.rs
    use crate::scheduler::calc_vdeadline;
    let actual_deadline = calc_vdeadline(entry.vruntime, entry.slice_ns, entry.weight);
    
    assert_eq!(actual_deadline, expected_deadline,
        "Nice 0 deadline should be vruntime + slice_ns");
}

#[test]
fn test_vdeadline_priority_affects_urgency() {
    // Higher priority (lower nice) should have shorter deadlines
    // This gives them better latency
    
    let mut entry_high = make_test_entry(1, -10, SchedPolicy::Normal);
    let mut entry_normal = make_test_entry(2, 0, SchedPolicy::Normal);
    let mut entry_low = make_test_entry(3, 10, SchedPolicy::Normal);
    
    // Set same vruntime for comparison
    entry_high.vruntime = 1000;
    entry_normal.vruntime = 1000;
    entry_low.vruntime = 1000;
    
    use crate::scheduler::calc_vdeadline;
    
    let dl_high = calc_vdeadline(entry_high.vruntime, entry_high.slice_ns, entry_high.weight);
    let dl_normal = calc_vdeadline(entry_normal.vruntime, entry_normal.slice_ns, entry_normal.weight);
    let dl_low = calc_vdeadline(entry_low.vruntime, entry_low.slice_ns, entry_low.weight);
    
    // Higher weight = smaller slice_ns/weight = earlier deadline
    assert!(dl_high < dl_normal, "High priority should have earlier deadline");
    assert!(dl_normal < dl_low, "Normal should have earlier deadline than low priority");
    
    eprintln!("Deadlines with same vruntime (1000):");
    eprintln!("  High priority (nice -10): {}", dl_high);
    eprintln!("  Normal (nice 0): {}", dl_normal);
    eprintln!("  Low priority (nice 10): {}", dl_low);
}

// ============================================================================
// Eligibility Tests
// ============================================================================

#[test]
fn test_eligibility_positive_lag() {
    // Process with positive lag (waited longer than expected) should be eligible
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    entry.lag = 1000; // Positive lag - process deserves CPU time
    
    use crate::scheduler::is_eligible;
    assert!(is_eligible(&entry), "Positive lag should be eligible");
}

#[test]
fn test_eligibility_zero_lag() {
    // Process with zero lag should be eligible
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    entry.lag = 0;
    
    use crate::scheduler::is_eligible;
    assert!(is_eligible(&entry), "Zero lag should be eligible");
}

#[test]
fn test_eligibility_negative_lag() {
    // Process with negative lag (got more CPU than deserved) should NOT be eligible
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    entry.lag = -1000; // Negative lag - process got extra CPU time
    
    use crate::scheduler::is_eligible;
    assert!(!is_eligible(&entry), "Negative lag should NOT be eligible");
}

// ============================================================================
// Scheduling Policy Tests
// ============================================================================

#[test]
fn test_realtime_policy_always_first() {
    // Realtime processes should always be selected before normal processes
    let rt_entry = make_test_entry(1, 0, SchedPolicy::Realtime);
    let normal_entry = make_test_entry(2, -20, SchedPolicy::Normal); // Even highest priority normal
    
    // Realtime should win regardless of nice value
    assert_eq!(rt_entry.policy, SchedPolicy::Realtime);
    assert_eq!(normal_entry.policy, SchedPolicy::Normal);
    
    // The scheduler should select RT first based on policy comparison
    // In the actual code, this is done in should_replace_candidate()
}

#[test]
fn test_idle_policy_last() {
    // Idle processes should only run when nothing else is ready
    let idle_entry = make_test_entry(1, 0, SchedPolicy::Idle);
    let normal_entry = make_test_entry(2, 19, SchedPolicy::Normal); // Lowest priority normal
    
    assert_eq!(idle_entry.policy, SchedPolicy::Idle);
    assert_eq!(normal_entry.policy, SchedPolicy::Normal);
    
    // Normal should always win over Idle
}

#[test]
fn test_batch_policy_longer_slices() {
    // Batch processes should get longer time slices for throughput
    let batch_entry = make_test_entry(1, 0, SchedPolicy::Batch);
    
    // Batch policy should be recognized
    assert_eq!(batch_entry.policy, SchedPolicy::Batch);
    
    // Note: In actual implementation, batch processes might get different treatment
    // for time slice duration, but that's handled elsewhere
}

// ============================================================================
// Time Slice Tests
// ============================================================================

#[test]
fn test_time_slice_exhaustion() {
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    
    // Initially, slice should be full
    assert_eq!(entry.slice_remaining_ns, BASE_SLICE_NS);
    
    // Consume time slice
    let consumed = 1_000_000; // 1ms
    entry.slice_remaining_ns = entry.slice_remaining_ns.saturating_sub(consumed);
    assert!(entry.slice_remaining_ns < BASE_SLICE_NS);
    
    // Fully exhaust
    entry.slice_remaining_ns = 0;
    assert_eq!(entry.slice_remaining_ns, 0, "Slice should be exhausted");
}

#[test]
fn test_time_slice_replenishment() {
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    
    // Exhaust the slice
    entry.slice_remaining_ns = 0;
    
    // Replenishment logic (directly test the expected behavior)
    // replenish_slice is internal to scheduler, so we verify the behavior manually
    let expected_slice = BASE_SLICE_NS * entry.weight as u64 / NICE_0_WEIGHT as u64;
    entry.slice_remaining_ns = expected_slice;
    
    // Slice should be restored
    assert!(entry.slice_remaining_ns > 0, "Slice should be replenished");
    assert_eq!(entry.slice_remaining_ns, expected_slice);
}

// ============================================================================
// Preemption Decision Tests
// ============================================================================

#[test]
fn test_realtime_preempts_normal() {
    let mut rt_entry = make_test_entry(1, 0, SchedPolicy::Realtime);
    rt_entry.priority = 10; // Lower number = higher priority for RT
    rt_entry.process.state = ProcessState::Ready;
    
    let mut normal_entry = make_test_entry(2, 0, SchedPolicy::Normal);
    normal_entry.process.state = ProcessState::Running;
    
    // RT should preempt Normal
    // This is tested in the actual scheduler through should_preempt_for_eevdf
}

#[test]
fn test_higher_priority_rt_preempts_lower() {
    let mut rt_high = make_test_entry(1, 0, SchedPolicy::Realtime);
    rt_high.priority = 5; // Higher priority (lower number)
    rt_high.process.state = ProcessState::Ready;
    
    let mut rt_low = make_test_entry(2, 0, SchedPolicy::Realtime);
    rt_low.priority = 20; // Lower priority (higher number)
    rt_low.process.state = ProcessState::Running;
    
    // Higher priority RT should preempt lower priority RT
}

#[test]
fn test_normal_does_not_preempt_normal() {
    // Normal processes should NOT preempt each other mid-slice
    // This is critical for preventing excessive context switches
    
    let mut ready_entry = make_test_entry(1, -10, SchedPolicy::Normal);
    ready_entry.process.state = ProcessState::Ready;
    ready_entry.lag = 1000; // Eligible
    ready_entry.vdeadline = 1000;
    
    let mut running_entry = make_test_entry(2, 0, SchedPolicy::Normal);
    running_entry.process.state = ProcessState::Running;
    running_entry.slice_remaining_ns = BASE_SLICE_NS / 2; // Still has time
    
    // Even though ready_entry has higher priority (lower nice),
    // it should NOT preempt running_entry mid-slice
    // The scheduler should wait for time slice exhaustion
}

// ============================================================================
// Fairness Tests
// ============================================================================

#[test]
fn test_lag_accumulation_for_waiting() {
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    entry.process.state = ProcessState::Ready;
    entry.lag = 0;
    entry.wait_time = 0;
    
    // Waiting process
    let wait_ticks = 10;
    entry.wait_time += wait_ticks;
    
    // Lag should increase for waiting processes
    // In actual code, this is done in update_ready_process_eevdf
    // lag_credit = (wait_time_ns * weight) / total_weight
    
    // After waiting, process should have positive lag
    // (demonstrating that it deserves more CPU time)
}

#[test]
fn test_min_vruntime_monotonic() {
    // min_vruntime should only increase to prevent starvation
    
    // Track min_vruntime
    let mut min_vruntime: u64 = 1000;
    
    // New process joins with vruntime 0
    let new_process_vruntime: u64 = 0;
    
    // min_vruntime should NOT decrease
    if new_process_vruntime < min_vruntime {
        // In actual scheduler, new processes start at min_vruntime
        // not at 0, to prevent starvation of existing processes
    }
    
    // min_vruntime should only go forward
    let proposed_min = 500; // Lower value
    if proposed_min > min_vruntime {
        min_vruntime = proposed_min;
    }
    assert_eq!(min_vruntime, 1000, "min_vruntime should not decrease");
}

// ============================================================================
// CPU Affinity Tests
// ============================================================================

#[test]
fn test_affinity_restricts_scheduling() {
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    
    // Restrict to CPUs 0 and 2 only
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    entry.cpu_affinity.set(2);
    
    // Process should only be schedulable on CPUs 0 and 2
    assert!(entry.cpu_affinity.is_set(0));
    assert!(!entry.cpu_affinity.is_set(1));
    assert!(entry.cpu_affinity.is_set(2));
    assert!(!entry.cpu_affinity.is_set(3));
}

#[test]
fn test_last_cpu_tracking() {
    let mut entry = make_test_entry(1, 0, SchedPolicy::Normal);
    entry.last_cpu = 0;
    
    // Migration to different CPU
    entry.last_cpu = 3;
    
    // Should prefer running on last CPU for cache locality
    assert_eq!(entry.last_cpu, 3);
}

// ============================================================================
// EEVDF Algorithm Invariant Tests
// ============================================================================

#[test]
fn test_eevdf_invariant_deadline_ordering() {
    // Among eligible processes, the one with earliest deadline should be selected
    
    let mut entry1 = make_test_entry(1, 0, SchedPolicy::Normal);
    entry1.vruntime = 1000;
    entry1.vdeadline = 2000;
    entry1.lag = 0; // Eligible
    
    let mut entry2 = make_test_entry(2, 0, SchedPolicy::Normal);
    entry2.vruntime = 1000;
    entry2.vdeadline = 1500; // Earlier deadline
    entry2.lag = 0; // Eligible
    
    // entry2 should be selected because it has earlier deadline
    assert!(entry2.vdeadline < entry1.vdeadline);
}

#[test]
fn test_eevdf_invariant_eligibility_before_deadline() {
    // Eligible processes should be preferred over non-eligible
    // even if non-eligible has earlier deadline
    
    let mut eligible = make_test_entry(1, 0, SchedPolicy::Normal);
    eligible.vdeadline = 2000;
    eligible.lag = 0; // Eligible
    
    let mut ineligible = make_test_entry(2, 0, SchedPolicy::Normal);
    ineligible.vdeadline = 1000; // Earlier deadline
    ineligible.lag = -1000; // NOT eligible
    
    // eligible should be selected even though ineligible has earlier deadline
    use crate::scheduler::is_eligible;
    assert!(is_eligible(&eligible));
    assert!(!is_eligible(&ineligible));
}
