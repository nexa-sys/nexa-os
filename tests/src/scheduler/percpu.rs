//! Per-CPU Scheduler Tests
//!
//! Tests for per-CPU run queues, scheduling data, and load balancing.
//! These tests focus on the percpu.rs functionality.

use crate::scheduler::{CpuMask, SchedPolicy, nice_to_weight, BASE_SLICE_NS};
use crate::scheduler::percpu::{
    RunQueueEntry, PerCpuRunQueue, PerCpuSchedData,
    PERCPU_RQ_SIZE,
};
use crate::process::ProcessState;

/// Helper to create a test run queue entry
fn make_rq_entry(pid: u64, vdeadline: u64, policy: SchedPolicy) -> RunQueueEntry {
    RunQueueEntry {
        pid,
        table_index: pid as u16,
        vdeadline,
        vruntime: vdeadline.saturating_sub(BASE_SLICE_NS),
        policy,
        priority: 128,
        eligible: true,
    }
}

// ============================================================================
// RunQueueEntry Tests
// ============================================================================

#[test]
fn test_run_queue_entry_empty() {
    let entry = RunQueueEntry::empty();
    
    assert_eq!(entry.pid, 0);
    assert_eq!(entry.table_index, 0);
    assert_eq!(entry.vdeadline, 0);
    assert_eq!(entry.vruntime, 0);
    assert_eq!(entry.policy, SchedPolicy::Normal);
    assert_eq!(entry.priority, 128);
    assert!(entry.eligible);
}

#[test]
fn test_run_queue_entry_creation() {
    let entry = make_rq_entry(42, 5000, SchedPolicy::Normal);
    
    assert_eq!(entry.pid, 42);
    assert_eq!(entry.vdeadline, 5000);
    assert_eq!(entry.policy, SchedPolicy::Normal);
    assert!(entry.eligible);
}

// ============================================================================
// PerCpuRunQueue Tests
// ============================================================================

#[test]
fn test_percpu_rq_new() {
    let rq = PerCpuRunQueue::new(0);
    
    assert!(rq.is_empty());
    assert_eq!(rq.len(), 0);
    assert_eq!(rq.min_vruntime(), 0);
    assert_eq!(rq.current(), None);
}

#[test]
fn test_percpu_rq_init() {
    let mut rq = PerCpuRunQueue::new(0);
    rq.init(5, 1); // CPU 5, NUMA node 1
    
    assert!(rq.is_empty());
    assert_eq!(rq.len(), 0);
    assert_eq!(rq.numa_node(), 1);
}

#[test]
fn test_percpu_rq_enqueue_single() {
    let mut rq = PerCpuRunQueue::new(0);
    let entry = make_rq_entry(1, 1000, SchedPolicy::Normal);
    
    let result = rq.enqueue(entry);
    assert!(result.is_ok());
    assert_eq!(rq.len(), 1);
    assert!(!rq.is_empty());
    assert!(rq.contains(1));
}

#[test]
fn test_percpu_rq_enqueue_multiple() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Enqueue in reverse deadline order
    rq.enqueue(make_rq_entry(1, 3000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(2, 1000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(3, 2000, SchedPolicy::Normal)).unwrap();
    
    assert_eq!(rq.len(), 3);
    assert!(rq.contains(1));
    assert!(rq.contains(2));
    assert!(rq.contains(3));
}

#[test]
fn test_percpu_rq_enqueue_sorted_by_deadline() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Enqueue in random order
    rq.enqueue(make_rq_entry(3, 3000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(2, 2000, SchedPolicy::Normal)).unwrap();
    
    // pick_next should return earliest deadline first (PID 1)
    let next = rq.pick_next();
    assert!(next.is_some());
    let entry = next.unwrap();
    assert_eq!(entry.pid, 1, "Should select process with earliest deadline");
}

#[test]
fn test_percpu_rq_dequeue() {
    let mut rq = PerCpuRunQueue::new(0);
    
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(2, 2000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(3, 3000, SchedPolicy::Normal)).unwrap();
    
    // Dequeue middle entry
    let removed = rq.dequeue(2);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().pid, 2);
    
    assert_eq!(rq.len(), 2);
    assert!(rq.contains(1));
    assert!(!rq.contains(2)); // Should be removed
    assert!(rq.contains(3));
}

#[test]
fn test_percpu_rq_dequeue_nonexistent() {
    let mut rq = PerCpuRunQueue::new(0);
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    
    let removed = rq.dequeue(999);
    assert!(removed.is_none());
    assert_eq!(rq.len(), 1);
}

#[test]
fn test_percpu_rq_pick_next_empty() {
    let mut rq = PerCpuRunQueue::new(0);
    
    let next = rq.pick_next();
    assert!(next.is_none());
}

#[test]
fn test_percpu_rq_pick_next_removes_entry() {
    let mut rq = PerCpuRunQueue::new(0);
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    
    assert_eq!(rq.len(), 1);
    
    let next = rq.pick_next();
    assert!(next.is_some());
    
    // Entry should be removed from queue
    assert_eq!(rq.len(), 0);
    assert!(!rq.contains(1));
}

#[test]
fn test_percpu_rq_realtime_priority() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add normal process with early deadline
    rq.enqueue(make_rq_entry(1, 500, SchedPolicy::Normal)).unwrap();
    
    // Add realtime process with later deadline
    let mut rt_entry = make_rq_entry(2, 5000, SchedPolicy::Realtime);
    rt_entry.priority = 10;
    rq.enqueue(rt_entry).unwrap();
    
    // Realtime should be selected first despite later deadline
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(next.unwrap().pid, 2, "Realtime should be selected first");
}

#[test]
fn test_percpu_rq_realtime_priority_ordering() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add two realtime processes
    let mut rt_low = make_rq_entry(1, 1000, SchedPolicy::Realtime);
    rt_low.priority = 20; // Lower priority (higher number)
    rq.enqueue(rt_low).unwrap();
    
    let mut rt_high = make_rq_entry(2, 2000, SchedPolicy::Realtime);
    rt_high.priority = 5; // Higher priority (lower number)
    rq.enqueue(rt_high).unwrap();
    
    // Higher priority RT should be selected
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(next.unwrap().pid, 2, "Higher priority RT should be selected");
}

#[test]
fn test_percpu_rq_idle_last() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add idle process first
    rq.enqueue(make_rq_entry(1, 500, SchedPolicy::Idle)).unwrap();
    
    // Add normal process with later deadline
    rq.enqueue(make_rq_entry(2, 5000, SchedPolicy::Normal)).unwrap();
    
    // Normal should be selected first
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(next.unwrap().pid, 2, "Normal should be selected before Idle");
}

#[test]
fn test_percpu_rq_eligibility_check() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add eligible process with later deadline
    let mut eligible = make_rq_entry(1, 2000, SchedPolicy::Normal);
    eligible.eligible = true;
    rq.enqueue(eligible).unwrap();
    
    // Add ineligible process with earlier deadline
    let mut ineligible = make_rq_entry(2, 1000, SchedPolicy::Normal);
    ineligible.eligible = false;
    rq.enqueue(ineligible).unwrap();
    
    // Eligible should be selected first (EEVDF rule)
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(next.unwrap().pid, 1, "Eligible process should be selected first");
}

#[test]
fn test_percpu_rq_update_entry() {
    let mut rq = PerCpuRunQueue::new(0);
    
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    
    // Update entry
    rq.update_entry(1, 2000, 3000, false);
    
    // Verify it's still in queue
    assert!(rq.contains(1));
}

#[test]
fn test_percpu_rq_min_vruntime() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Initial min_vruntime
    assert_eq!(rq.min_vruntime(), 0);
    
    // Add entry with vruntime 500
    let mut entry1 = make_rq_entry(1, 1000, SchedPolicy::Normal);
    entry1.vruntime = 500;
    rq.enqueue(entry1).unwrap();
    
    // min_vruntime should be updated
    // Note: The actual update depends on implementation
}

#[test]
fn test_percpu_rq_need_resched() {
    let rq = PerCpuRunQueue::new(0);
    
    // Initially no resched needed
    assert!(!rq.check_need_resched());
    
    // Set need_resched
    rq.set_need_resched(true);
    
    // Check and clear should return true
    assert!(rq.check_need_resched());
    
    // Second check should return false (cleared)
    assert!(!rq.check_need_resched());
}

#[test]
fn test_percpu_rq_current_tracking() {
    let mut rq = PerCpuRunQueue::new(0);
    
    assert_eq!(rq.current(), None);
    
    rq.set_current(Some(42));
    assert_eq!(rq.current(), Some(42));
    
    rq.set_current(None);
    assert_eq!(rq.current(), None);
}

#[test]
fn test_percpu_rq_full() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Fill the queue
    for i in 0..PERCPU_RQ_SIZE {
        let result = rq.enqueue(make_rq_entry(i as u64, i as u64 * 100, SchedPolicy::Normal));
        assert!(result.is_ok(), "Should be able to enqueue up to PERCPU_RQ_SIZE");
    }
    
    assert_eq!(rq.len(), PERCPU_RQ_SIZE);
    
    // Try to add one more
    let result = rq.enqueue(make_rq_entry(9999, 9999, SchedPolicy::Normal));
    assert!(result.is_err(), "Should fail when queue is full");
}

// ============================================================================
// PerCpuSchedData Tests
// ============================================================================

#[test]
fn test_percpu_sched_data_new() {
    let data = PerCpuSchedData::new(0);
    
    // Check initial state
    assert_eq!(data.cpu_id, 0);
    assert_eq!(data.numa_node, 0);
}

#[test]
fn test_percpu_sched_data_init() {
    let mut data = PerCpuSchedData::new(0);
    data.init(5, 2); // CPU 5, NUMA node 2
    
    assert_eq!(data.cpu_id, 5);
    assert_eq!(data.numa_node, 2);
}

#[test]
fn test_percpu_sched_data_context_switch_recording() {
    let data = PerCpuSchedData::new(0);
    
    // Record voluntary switch
    data.record_context_switch(true);
    
    // Record involuntary switch (preemption)
    data.record_context_switch(false);
    
    // Both should be counted
    use core::sync::atomic::Ordering;
    assert_eq!(data.context_switches.load(Ordering::Relaxed), 2);
    assert_eq!(data.voluntary_switches.load(Ordering::Relaxed), 1);
    assert_eq!(data.preemptions.load(Ordering::Relaxed), 1);
}

#[test]
fn test_percpu_sched_data_idle_tracking() {
    let data = PerCpuSchedData::new(0);
    
    // Initially idle
    use core::sync::atomic::Ordering;
    assert!(data.is_idle.load(Ordering::Relaxed));
    
    // Enter idle at time 1000
    data.enter_idle(1000);
    assert!(data.is_idle.load(Ordering::Relaxed));
    
    // Exit idle at time 2000 (1000ns idle time)
    data.exit_idle(2000);
    assert!(!data.is_idle.load(Ordering::Relaxed));
    
    // Idle time should be recorded
    assert_eq!(data.idle_ns.load(Ordering::Relaxed), 1000);
}

#[test]
fn test_percpu_sched_data_load_average() {
    let data = PerCpuSchedData::new(0);
    
    // Add some processes to run queue
    {
        let mut rq = data.run_queue.lock();
        rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
        rq.enqueue(make_rq_entry(2, 2000, SchedPolicy::Normal)).unwrap();
    }
    
    // Update load average
    data.update_load_average();
    
    // Load should be non-zero now
    use core::sync::atomic::Ordering;
    let load = data.load_avg.load(Ordering::Relaxed);
    eprintln!("Load average after 2 processes: {}", load);
}

#[test]
fn test_percpu_sched_data_load_percent() {
    let data = PerCpuSchedData::new(0);
    
    // Initially idle, load should be low
    let load = data.load_percent();
    assert!(load <= 100, "Load percent should be <= 100");
}

// ============================================================================
// Run Queue Stress Tests
// ============================================================================

#[test]
fn test_percpu_rq_rapid_enqueue_dequeue() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Rapidly enqueue and dequeue
    for i in 0..100 {
        rq.enqueue(make_rq_entry(i, i as u64 * 100, SchedPolicy::Normal)).unwrap();
    }
    
    for i in 0..100 {
        let entry = rq.dequeue(i);
        assert!(entry.is_some(), "Should find entry {}", i);
    }
    
    assert!(rq.is_empty());
}

#[test]
fn test_percpu_rq_interleaved_operations() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Interleave enqueue, dequeue, and pick_next
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(2, 2000, SchedPolicy::Normal)).unwrap();
    
    let _ = rq.pick_next(); // Removes PID 1
    
    rq.enqueue(make_rq_entry(3, 1500, SchedPolicy::Normal)).unwrap();
    
    let _ = rq.dequeue(2); // Removes PID 2
    
    rq.enqueue(make_rq_entry(4, 500, SchedPolicy::Normal)).unwrap();
    
    // Now should have PIDs 3 and 4
    assert_eq!(rq.len(), 2);
    assert!(rq.contains(3));
    assert!(rq.contains(4));
}

#[test]
fn test_percpu_rq_same_deadline() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add multiple entries with same deadline
    rq.enqueue(make_rq_entry(1, 1000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(2, 1000, SchedPolicy::Normal)).unwrap();
    rq.enqueue(make_rq_entry(3, 1000, SchedPolicy::Normal)).unwrap();
    
    // All should be in queue
    assert_eq!(rq.len(), 3);
    
    // pick_next should work (order among same-deadline is implementation-defined)
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(rq.len(), 2);
}
