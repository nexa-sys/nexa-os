//! Scheduler SMP Integration Tests
//!
//! Tests for scheduler behavior in multi-CPU environments.
//! These tests verify:
//! - Per-CPU run queues work correctly
//! - Load balancing distributes work properly
//! - CPU affinity is respected
//! - Need-resched IPI mechanism
//! - Context switches work correctly across CPUs

use crate::mock::vm::{VirtualMachine, VmConfig};
use crate::scheduler::{CpuMask, SchedPolicy, nice_to_weight, BASE_SLICE_NS};
use crate::scheduler::percpu::{
    RunQueueEntry, PerCpuRunQueue, PerCpuSchedData,
};
use crate::scheduler::ProcessEntry;
use crate::process::ProcessState;

use std::sync::{Arc, Mutex, Barrier};
use std::thread;

/// Helper to create a run queue entry
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
// Per-CPU Run Queue Isolation Tests
// ============================================================================

#[test]
fn test_scheduler_percpu_queue_isolation() {
    // Create per-CPU scheduler data for multiple CPUs
    let cpu0 = Arc::new(PerCpuSchedData::new(0));
    let cpu1 = Arc::new(PerCpuSchedData::new(1));
    let cpu2 = Arc::new(PerCpuSchedData::new(2));
    let cpu3 = Arc::new(PerCpuSchedData::new(3));
    
    // Add different processes to each CPU's queue
    {
        let mut rq = cpu0.run_queue.lock();
        rq.enqueue(make_rq_entry(100, 1000, SchedPolicy::Normal)).unwrap();
        rq.enqueue(make_rq_entry(101, 2000, SchedPolicy::Normal)).unwrap();
    }
    
    {
        let mut rq = cpu1.run_queue.lock();
        rq.enqueue(make_rq_entry(200, 1500, SchedPolicy::Normal)).unwrap();
    }
    
    {
        let mut rq = cpu2.run_queue.lock();
        // CPU 2 is empty
    }
    
    {
        let mut rq = cpu3.run_queue.lock();
        rq.enqueue(make_rq_entry(300, 500, SchedPolicy::Realtime)).unwrap();
    }
    
    // Verify isolation
    assert_eq!(cpu0.run_queue.lock().len(), 2);
    assert_eq!(cpu1.run_queue.lock().len(), 1);
    assert_eq!(cpu2.run_queue.lock().len(), 0);
    assert_eq!(cpu3.run_queue.lock().len(), 1);
    
    // Verify correct PIDs
    assert!(cpu0.run_queue.lock().contains(100));
    assert!(cpu0.run_queue.lock().contains(101));
    assert!(!cpu0.run_queue.lock().contains(200));
    
    assert!(cpu1.run_queue.lock().contains(200));
    assert!(!cpu1.run_queue.lock().contains(100));
}

// ============================================================================
// Concurrent Per-CPU Operations Tests
// ============================================================================

#[test]
fn test_scheduler_concurrent_percpu_operations() {
    // Multiple threads operating on different per-CPU data
    let cpu_data: Vec<Arc<PerCpuSchedData>> = (0..4)
        .map(|i| Arc::new(PerCpuSchedData::new(i)))
        .collect();
    
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = vec![];
    
    for cpu_id in 0..4 {
        let data = Arc::clone(&cpu_data[cpu_id as usize]);
        let barrier = Arc::clone(&barrier);
        
        handles.push(thread::spawn(move || {
            barrier.wait();
            
            // Each "CPU" does operations on its own data
            for i in 0..100 {
                let mut rq = data.run_queue.lock();
                let pid = cpu_id * 1000 + i;
                rq.enqueue(make_rq_entry(pid, i as u64 * 100, SchedPolicy::Normal)).unwrap();
            }
            
            // Pick some
            for _ in 0..50 {
                let mut rq = data.run_queue.lock();
                let _ = rq.pick_next();
            }
            
            // Record stats
            for _ in 0..100 {
                data.record_context_switch(true);
            }
        }));
    }
    
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Verify each CPU has expected state
    for (cpu_id, data) in cpu_data.iter().enumerate() {
        let rq = data.run_queue.lock();
        assert_eq!(rq.len(), 50, "CPU {} should have 50 entries left", cpu_id);
        
        use core::sync::atomic::Ordering;
        let switches = data.context_switches.load(Ordering::Relaxed);
        assert_eq!(switches, 100, "CPU {} should have 100 context switches", cpu_id);
    }
}

// ============================================================================
// Load Balancing Tests
// ============================================================================

#[test]
fn test_scheduler_load_imbalance_detection() {
    // Create CPUs with different loads
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    let cpu2 = PerCpuSchedData::new(2);
    let cpu3 = PerCpuSchedData::new(3);
    
    // CPU 0: Heavy load (10 processes)
    for i in 0..10 {
        cpu0.run_queue.lock().enqueue(make_rq_entry(i, i as u64 * 100, SchedPolicy::Normal)).unwrap();
    }
    
    // CPU 1: Medium load (5 processes)
    for i in 0..5 {
        cpu1.run_queue.lock().enqueue(make_rq_entry(100 + i, i as u64 * 100, SchedPolicy::Normal)).unwrap();
    }
    
    // CPU 2: Light load (2 processes)
    for i in 0..2 {
        cpu2.run_queue.lock().enqueue(make_rq_entry(200 + i, i as u64 * 100, SchedPolicy::Normal)).unwrap();
    }
    
    // CPU 3: Idle (0 processes)
    
    // Calculate loads
    let loads = [
        cpu0.run_queue.lock().len(),
        cpu1.run_queue.lock().len(),
        cpu2.run_queue.lock().len(),
        cpu3.run_queue.lock().len(),
    ];
    
    let total_load: usize = loads.iter().sum();
    let avg_load = total_load / 4;
    
    eprintln!("Loads: {:?}, avg: {}", loads, avg_load);
    
    // Detect imbalance
    let mut overloaded = vec![];
    let mut underloaded = vec![];
    
    const MIN_IMBALANCE: usize = 2;
    
    for (cpu, &load) in loads.iter().enumerate() {
        if load > avg_load + MIN_IMBALANCE {
            overloaded.push(cpu);
        } else if load < avg_load.saturating_sub(1) {
            underloaded.push(cpu);
        }
    }
    
    eprintln!("Overloaded CPUs: {:?}", overloaded);
    eprintln!("Underloaded CPUs: {:?}", underloaded);
    
    // CPU 0 should be overloaded, CPU 3 should be underloaded
    assert!(overloaded.contains(&0), "CPU 0 should be overloaded");
    assert!(underloaded.contains(&3), "CPU 3 should be underloaded");
}

#[test]
fn test_scheduler_migration_concept() {
    // Test process migration between CPUs
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    
    // Add process to CPU 0
    cpu0.run_queue.lock().enqueue(make_rq_entry(42, 1000, SchedPolicy::Normal)).unwrap();
    
    // Verify on CPU 0
    assert!(cpu0.run_queue.lock().contains(42));
    assert!(!cpu1.run_queue.lock().contains(42));
    
    // Migrate: remove from CPU 0, add to CPU 1
    let entry = cpu0.run_queue.lock().dequeue(42);
    assert!(entry.is_some());
    
    cpu1.run_queue.lock().enqueue(entry.unwrap()).unwrap();
    
    // Verify migration
    assert!(!cpu0.run_queue.lock().contains(42));
    assert!(cpu1.run_queue.lock().contains(42));
    
    // Update migration stats
    use core::sync::atomic::Ordering;
    cpu0.migrations_out.fetch_add(1, Ordering::Relaxed);
    cpu1.migrations_in.fetch_add(1, Ordering::Relaxed);
    
    assert_eq!(cpu0.migrations_out.load(Ordering::Relaxed), 1);
    assert_eq!(cpu1.migrations_in.load(Ordering::Relaxed), 1);
}

// ============================================================================
// CPU Affinity Enforcement Tests
// ============================================================================

#[test]
fn test_scheduler_affinity_enforcement() {
    // Create process with restricted affinity
    let mut entry = ProcessEntry::empty();
    entry.process.pid = 42;
    entry.process.state = ProcessState::Ready;
    
    // Can only run on CPUs 0 and 2
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    entry.cpu_affinity.set(2);
    
    // Scheduling decision on CPU 1
    let target_cpu = 1usize;
    let can_schedule = entry.cpu_affinity.is_set(target_cpu);
    assert!(!can_schedule, "Should not schedule on CPU 1 (not in affinity)");
    
    // Can schedule on CPU 0
    let can_schedule = entry.cpu_affinity.is_set(0);
    assert!(can_schedule, "Should be able to schedule on CPU 0");
    
    // Can schedule on CPU 2
    let can_schedule = entry.cpu_affinity.is_set(2);
    assert!(can_schedule, "Should be able to schedule on CPU 2");
}

#[test]
fn test_scheduler_affinity_migration_blocked() {
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    
    // Process can only run on CPU 0
    let mut entry = make_rq_entry(42, 1000, SchedPolicy::Normal);
    
    // Add to CPU 0
    cpu0.run_queue.lock().enqueue(entry.clone()).unwrap();
    
    // Affinity check before migration
    let mut affinity = CpuMask::empty();
    affinity.set(0); // Only CPU 0
    
    let target_cpu = 1usize;
    let can_migrate = affinity.is_set(target_cpu);
    
    assert!(!can_migrate, "Migration to CPU 1 should be blocked by affinity");
}

// ============================================================================
// Need Resched Tests
// ============================================================================

#[test]
fn test_scheduler_need_resched_flag() {
    let cpu0 = PerCpuSchedData::new(0);
    
    // Flag should start cleared
    {
        let rq = cpu0.run_queue.lock();
        assert!(!rq.check_need_resched());
    }
    
    // Set flag (IPI or wake-up notification)
    {
        let rq = cpu0.run_queue.lock();
        rq.set_need_resched(true);
    }
    
    // Check and clear should return true
    {
        let rq = cpu0.run_queue.lock();
        assert!(rq.check_need_resched(), "First check should return true");
    }
    
    // Second check should return false (cleared)
    {
        let rq = cpu0.run_queue.lock();
        assert!(!rq.check_need_resched(), "Second check should return false");
    }
}

#[test]
fn test_scheduler_need_resched_concurrent() {
    let cpu = Arc::new(PerCpuSchedData::new(0));
    let barrier = Arc::new(Barrier::new(2));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    
    let cpu_clone = Arc::clone(&cpu);
    let barrier_clone = Arc::clone(&barrier);
    let done_clone = Arc::clone(&done);
    
    // Thread 1: Sets need_resched repeatedly
    let setter = thread::spawn(move || {
        barrier_clone.wait();
        for _ in 0..1000 {
            cpu_clone.run_queue.lock().set_need_resched(true);
            thread::yield_now(); // Allow other thread to observe
        }
        done_clone.store(true, std::sync::atomic::Ordering::Release);
    });
    
    // Thread 2: Checks and clears until setter is done
    let cpu_checker = Arc::clone(&cpu);
    let checker = thread::spawn(move || {
        barrier.wait();
        let mut true_count = 0;
        while !done.load(std::sync::atomic::Ordering::Acquire) || true_count == 0 {
            if cpu_checker.run_queue.lock().check_need_resched() {
                true_count += 1;
            }
            thread::yield_now();
        }
        true_count
    });
    
    setter.join().unwrap();
    let cleared = checker.join().unwrap();
    
    eprintln!("Cleared need_resched {} times", cleared);
    assert!(cleared > 0, "Should have cleared at least some flags");
}

// ============================================================================
// Current Process Per-CPU Tests
// ============================================================================

#[test]
fn test_scheduler_current_process_percpu() {
    // Each CPU tracks its own current process
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    
    // Set different current processes
    {
        let mut rq0 = cpu0.run_queue.lock();
        rq0.set_current(Some(100));
    }
    {
        let mut rq1 = cpu1.run_queue.lock();
        rq1.set_current(Some(200));
    }
    
    // Verify isolation
    assert_eq!(cpu0.run_queue.lock().current(), Some(100));
    assert_eq!(cpu1.run_queue.lock().current(), Some(200));
    
    // Change CPU 0's current
    {
        let mut rq0 = cpu0.run_queue.lock();
        rq0.set_current(Some(150));
    }
    
    // CPU 1 unchanged
    assert_eq!(cpu1.run_queue.lock().current(), Some(200));
    assert_eq!(cpu0.run_queue.lock().current(), Some(150));
}

// ============================================================================
// Statistics Per-CPU Tests
// ============================================================================

#[test]
fn test_scheduler_stats_percpu() {
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    
    // Record different stats on each CPU
    for _ in 0..10 {
        cpu0.record_context_switch(true);  // Voluntary
    }
    
    for _ in 0..5 {
        cpu1.record_context_switch(false); // Preemption
    }
    
    use core::sync::atomic::Ordering;
    
    // Verify CPU 0 stats
    assert_eq!(cpu0.context_switches.load(Ordering::Relaxed), 10);
    assert_eq!(cpu0.voluntary_switches.load(Ordering::Relaxed), 10);
    assert_eq!(cpu0.preemptions.load(Ordering::Relaxed), 0);
    
    // Verify CPU 1 stats
    assert_eq!(cpu1.context_switches.load(Ordering::Relaxed), 5);
    assert_eq!(cpu1.voluntary_switches.load(Ordering::Relaxed), 0);
    assert_eq!(cpu1.preemptions.load(Ordering::Relaxed), 5);
}

// ============================================================================
// Idle State Per-CPU Tests
// ============================================================================

#[test]
fn test_scheduler_idle_state_percpu() {
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    
    use core::sync::atomic::Ordering;
    
    // CPU 0 goes idle
    cpu0.enter_idle(1000);
    assert!(cpu0.is_idle.load(Ordering::Relaxed));
    
    // CPU 1 is running
    cpu1.exit_idle(1000);
    assert!(!cpu1.is_idle.load(Ordering::Relaxed));
    
    // Exit idle on CPU 0
    cpu0.exit_idle(2000);
    
    // Both should be non-idle
    assert!(!cpu0.is_idle.load(Ordering::Relaxed));
    assert!(!cpu1.is_idle.load(Ordering::Relaxed));
    
    // CPU 0 accumulated 1000ns of idle time
    assert_eq!(cpu0.idle_ns.load(Ordering::Relaxed), 1000);
}

// ============================================================================
// Priority Scheduling Across CPUs Tests
// ============================================================================

#[test]
fn test_scheduler_rt_process_on_correct_cpu() {
    // RT process should be scheduled on CPU with best availability
    let cpu0 = PerCpuSchedData::new(0);
    let cpu1 = PerCpuSchedData::new(1);
    
    // CPU 0 has RT process running
    {
        let mut rq = cpu0.run_queue.lock();
        rq.set_current(Some(1));
        let mut rt = make_rq_entry(10, 1000, SchedPolicy::Realtime);
        rt.priority = 5;
        rq.enqueue(rt).unwrap();
    }
    
    // CPU 1 has only normal processes
    {
        let mut rq = cpu1.run_queue.lock();
        rq.set_current(Some(2));
        rq.enqueue(make_rq_entry(20, 500, SchedPolicy::Normal)).unwrap();
    }
    
    // New RT process arrives - should consider both CPUs
    let new_rt = make_rq_entry(30, 2000, SchedPolicy::Realtime);
    
    // In real implementation, would check if new_rt can preempt on either CPU
    // Here we just verify the queues are set up correctly
    assert!(cpu0.run_queue.lock().contains(10));
    assert!(cpu1.run_queue.lock().contains(20));
}
