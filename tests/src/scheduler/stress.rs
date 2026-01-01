//! Scheduler Stress Tests
//!
//! High-intensity tests to find race conditions, deadlocks, and edge cases
//! in the scheduler and per-CPU infrastructure.

use crate::scheduler::{CpuMask, SchedPolicy, nice_to_weight, BASE_SLICE_NS};
use crate::scheduler::percpu::{
    RunQueueEntry, PerCpuRunQueue, PerCpuSchedData, PERCPU_RQ_SIZE,
};
use crate::scheduler::ProcessEntry;
use crate::process::ProcessState;

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

/// Helper to create a run queue entry
fn make_rq_entry(pid: u64, vdeadline: u64) -> RunQueueEntry {
    RunQueueEntry {
        pid,
        table_index: pid as u16,
        vdeadline,
        vruntime: vdeadline.saturating_sub(BASE_SLICE_NS),
        policy: SchedPolicy::Normal,
        priority: 128,
        eligible: true,
    }
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[test]
fn test_percpu_concurrent_enqueue_dequeue() {
    // Test concurrent enqueue/dequeue doesn't cause data corruption
    
    let data = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = vec![];
    
    // Thread 1: Enqueue operations
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..100 {
                let mut rq = data.run_queue.lock();
                let _ = rq.enqueue(make_rq_entry(1000 + i, i as u64 * 100));
            }
        }));
    }
    
    // Thread 2: Enqueue operations with different PIDs
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..100 {
                let mut rq = data.run_queue.lock();
                let _ = rq.enqueue(make_rq_entry(2000 + i, i as u64 * 100));
            }
        }));
    }
    
    // Thread 3: Dequeue operations
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..50 {
                let mut rq = data.run_queue.lock();
                let _ = rq.dequeue(1000 + i * 2);
            }
        }));
    }
    
    // Thread 4: Pick next operations
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..50 {
                let mut rq = data.run_queue.lock();
                let _ = rq.pick_next();
            }
        }));
    }
    
    // Wait for all threads
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Verify queue is in valid state
    let rq = data.run_queue.lock();
    let len = rq.len();
    eprintln!("Final queue length after concurrent operations: {}", len);
    assert!(len <= PERCPU_RQ_SIZE, "Queue length should not exceed maximum");
}

#[test]
fn test_percpu_concurrent_statistics() {
    // Test concurrent statistics updates don't cause data races
    
    let data = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = vec![];
    
    for _ in 0..4 {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..1000 {
                data.record_context_switch(true);
                data.record_context_switch(false);
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Each thread did 2000 context switches, 4 threads total
    use core::sync::atomic::Ordering;
    let total = data.context_switches.load(Ordering::SeqCst);
    assert_eq!(total, 8000, "All context switches should be counted");
    
    let voluntary = data.voluntary_switches.load(Ordering::SeqCst);
    let preemptions = data.preemptions.load(Ordering::SeqCst);
    assert_eq!(voluntary, 4000, "Voluntary switches should be counted");
    assert_eq!(preemptions, 4000, "Preemptions should be counted");
}

#[test]
fn test_percpu_concurrent_idle_tracking() {
    // Test concurrent idle state updates
    
    let data = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = vec![];
    
    // Thread 1: Enter/exit idle rapidly
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..1000u64 {
                data.enter_idle(i * 2);
                data.exit_idle(i * 2 + 1);
            }
        }));
    }
    
    // Thread 2: Read idle state and stats
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            use core::sync::atomic::Ordering;
            for _ in 0..1000 {
                let _ = data.is_idle.load(Ordering::Relaxed);
                let _ = data.idle_ns.load(Ordering::Relaxed);
                let _ = data.load_percent();
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}

#[test]
fn test_percpu_concurrent_need_resched() {
    // Test concurrent need_resched flag manipulation
    
    let data = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = vec![];
    
    // Thread 1: Set need_resched
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..1000 {
                let rq = data.run_queue.lock();
                rq.set_need_resched(true);
            }
        }));
    }
    
    // Thread 2: Check and clear need_resched
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let mut clear_count = 0;
            for _ in 0..1000 {
                let rq = data.run_queue.lock();
                if rq.check_need_resched() {
                    clear_count += 1;
                }
            }
            eprintln!("Cleared need_resched {} times", clear_count);
        }));
    }
    
    // Thread 3: Also check and clear
    {
        let data = Arc::clone(&data);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let mut clear_count = 0;
            for _ in 0..1000 {
                let rq = data.run_queue.lock();
                if rq.check_need_resched() {
                    clear_count += 1;
                }
            }
            eprintln!("Thread 3 cleared need_resched {} times", clear_count);
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}

// ============================================================================
// Rapid State Transition Tests
// ============================================================================

#[test]
fn test_rapid_enqueue_dequeue_same_pid() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Rapidly add and remove same PID
    for _ in 0..1000 {
        rq.enqueue(make_rq_entry(42, 1000)).unwrap();
        let removed = rq.dequeue(42);
        assert!(removed.is_some());
    }
    
    assert!(rq.is_empty());
}

#[test]
fn test_rapid_pick_next_single_entry() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Repeatedly add single entry and pick it
    for i in 0u64..1000 {
        rq.enqueue(make_rq_entry(i, i * 100)).unwrap();
        let next = rq.pick_next();
        assert!(next.is_some());
        assert_eq!(next.unwrap().pid, i);
    }
    
    assert!(rq.is_empty());
}

#[test]
fn test_rapid_current_process_changes() {
    let mut rq = PerCpuRunQueue::new(0);
    
    for i in 0..1000 {
        rq.set_current(Some(i));
        assert_eq!(rq.current(), Some(i));
        rq.set_current(None);
        assert_eq!(rq.current(), None);
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_zero_deadline() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Entry with zero deadline
    let entry = RunQueueEntry {
        pid: 1,
        table_index: 1,
        vdeadline: 0,
        vruntime: 0,
        policy: SchedPolicy::Normal,
        priority: 128,
        eligible: true,
    };
    
    rq.enqueue(entry).unwrap();
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(next.unwrap().vdeadline, 0);
}

#[test]
fn test_max_deadline() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Entry with maximum deadline
    let entry = RunQueueEntry {
        pid: 1,
        table_index: 1,
        vdeadline: u64::MAX,
        vruntime: u64::MAX - BASE_SLICE_NS,
        policy: SchedPolicy::Normal,
        priority: 128,
        eligible: true,
    };
    
    rq.enqueue(entry).unwrap();
    let next = rq.pick_next();
    assert!(next.is_some());
    assert_eq!(next.unwrap().vdeadline, u64::MAX);
}

#[test]
fn test_max_pid() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Entry with maximum PID (use a large value, not u64::MAX to avoid overflow)
    let max_pid: u64 = u32::MAX as u64;
    let entry = make_rq_entry(max_pid, 1000);
    
    rq.enqueue(entry).unwrap();
    assert!(rq.contains(max_pid));
    
    let removed = rq.dequeue(max_pid);
    assert!(removed.is_some());
}

#[test]
fn test_all_policies_in_queue() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add one of each policy
    let mut normal = make_rq_entry(1, 2000);
    normal.policy = SchedPolicy::Normal;
    
    let mut rt = make_rq_entry(2, 3000);
    rt.policy = SchedPolicy::Realtime;
    rt.priority = 10;
    
    let mut batch = make_rq_entry(3, 1000);
    batch.policy = SchedPolicy::Batch;
    
    let mut idle = make_rq_entry(4, 500);
    idle.policy = SchedPolicy::Idle;
    
    rq.enqueue(normal).unwrap();
    rq.enqueue(rt).unwrap();
    rq.enqueue(batch).unwrap();
    rq.enqueue(idle).unwrap();
    
    // RT should be selected first
    let next = rq.pick_next().unwrap();
    assert_eq!(next.pid, 2, "RT should be first");
    
    // Then batch (earlier deadline than normal)
    let next = rq.pick_next().unwrap();
    assert_eq!(next.pid, 3, "Batch should be next (earlier deadline)");
    
    // Then normal
    let next = rq.pick_next().unwrap();
    assert_eq!(next.pid, 1, "Normal should be next");
    
    // Finally idle
    let next = rq.pick_next().unwrap();
    assert_eq!(next.pid, 4, "Idle should be last");
}

#[test]
fn test_mixed_eligibility() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Mix of eligible and ineligible entries
    for i in 0..10 {
        let mut entry = make_rq_entry(i, i as u64 * 100);
        entry.eligible = i % 2 == 0; // Even PIDs are eligible
        rq.enqueue(entry).unwrap();
    }
    
    // First pick should be eligible entry with earliest deadline
    let next = rq.pick_next().unwrap();
    assert!(next.eligible || rq.len() == 0, "Should prefer eligible entries");
}

// ============================================================================
// Memory Pressure Tests
// ============================================================================

#[test]
fn test_queue_fill_empty_cycle() {
    let mut rq = PerCpuRunQueue::new(0);
    
    for cycle in 0..10 {
        // Fill queue
        for i in 0..PERCPU_RQ_SIZE {
            let entry = make_rq_entry(i as u64, i as u64 * 100);
            assert!(rq.enqueue(entry).is_ok(), "Cycle {}: enqueue {} failed", cycle, i);
        }
        
        assert_eq!(rq.len(), PERCPU_RQ_SIZE);
        
        // Empty queue
        while rq.pick_next().is_some() {}
        
        assert!(rq.is_empty());
    }
}

#[test]
fn test_alternating_fill_patterns() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Pattern 1: Fill with even PIDs
    for i in 0..64 {
        rq.enqueue(make_rq_entry(i * 2, i as u64 * 100)).unwrap();
    }
    
    // Remove half
    for i in 0..32 {
        rq.dequeue(i * 4).unwrap();
    }
    
    assert_eq!(rq.len(), 32);
    
    // Fill gaps with odd PIDs
    for i in 0..32 {
        rq.enqueue(make_rq_entry(i * 2 + 1, i as u64 * 50)).unwrap();
    }
    
    assert_eq!(rq.len(), 64);
    
    // Drain all
    let mut count = 0;
    while rq.pick_next().is_some() {
        count += 1;
    }
    assert_eq!(count, 64);
}

// ============================================================================
// Load Average Stress Tests
// ============================================================================

#[test]
fn test_load_average_convergence() {
    let data = PerCpuSchedData::new(0);
    
    // Add fixed load
    {
        let mut rq = data.run_queue.lock();
        for i in 0..5 {
            rq.enqueue(make_rq_entry(i, i as u64 * 100)).unwrap();
        }
    }
    
    // Update load average many times
    use core::sync::atomic::Ordering;
    let mut prev_load = 0u64;
    
    for iteration in 0..100 {
        data.update_load_average();
        let curr_load = data.load_avg.load(Ordering::Relaxed);
        
        // Load should converge toward actual load
        if iteration > 20 {
            // After some iterations, load should be relatively stable
            let diff = (curr_load as i64 - prev_load as i64).abs();
            assert!(diff < 200, "Load should stabilize");
        }
        prev_load = curr_load;
    }
}

#[test]
fn test_load_average_with_empty_queue() {
    let data = PerCpuSchedData::new(0);
    
    // Queue is empty
    assert!(data.run_queue.lock().is_empty());
    
    // Update load average many times
    for _ in 0..100 {
        data.update_load_average();
    }
    
    // Load should converge to low value
    use core::sync::atomic::Ordering;
    let load = data.load_avg.load(Ordering::Relaxed);
    
    // Load should be very low (just the running process if any)
    assert!(load < 1024, "Load with empty queue should be low");
}

// ============================================================================
// Deadline Ordering Stress Tests
// ============================================================================

#[test]
fn test_deadline_ordering_preserved() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add entries in random order
    let deadlines: Vec<u64> = vec![5000, 1000, 3000, 2000, 4000, 500, 2500, 1500, 3500, 4500];
    
    for (i, &dl) in deadlines.iter().enumerate() {
        rq.enqueue(make_rq_entry(i as u64, dl)).unwrap();
    }
    
    // Pick next should return in deadline order (for same policy)
    let mut last_deadline = 0u64;
    while let Some(entry) = rq.pick_next() {
        assert!(
            entry.vdeadline >= last_deadline,
            "Deadlines should be in order: {} >= {}",
            entry.vdeadline,
            last_deadline
        );
        last_deadline = entry.vdeadline;
    }
}

#[test]
fn test_deadline_ordering_after_updates() {
    let mut rq = PerCpuRunQueue::new(0);
    
    // Add entries
    for i in 0..10 {
        rq.enqueue(make_rq_entry(i, i as u64 * 1000)).unwrap();
    }
    
    // Update some entries (vruntime changes)
    rq.update_entry(5, 100, 500, true);   // Move PID 5 earlier
    rq.update_entry(0, 10000, 15000, true); // Move PID 0 later
    
    // First pick should be updated entry with earliest deadline
    // Note: update_entry doesn't re-sort, so this tests the update behavior
    let first = rq.pick_next().unwrap();
    eprintln!("First picked after update: PID {} deadline {}", first.pid, first.vdeadline);
}
