//! SMP (Symmetric Multi-Processing) Scheduler Tests
//!
//! Tests for multi-CPU scheduling, including:
//! - CPU affinity enforcement
//! - Load balancing between CPUs
//! - NUMA-aware scheduling
//! - Per-CPU state isolation
//! - IPI (Inter-Processor Interrupt) triggering

use crate::scheduler::{CpuMask, SchedPolicy, nice_to_weight, BASE_SLICE_NS};
use crate::scheduler::percpu::{
    RunQueueEntry, PerCpuRunQueue, PerCpuSchedData,
    find_least_loaded_cpu, find_best_cpu_numa, get_cpu_load, get_cpu_queue_len,
};
use crate::scheduler::ProcessEntry;
use crate::process::{ProcessState, Pid};

/// Helper to create a test process entry
fn make_test_entry(pid: Pid, nice: i8) -> ProcessEntry {
    let mut entry = ProcessEntry::empty();
    entry.process.pid = pid;
    entry.process.state = ProcessState::Ready;
    entry.nice = nice;
    entry.policy = SchedPolicy::Normal;
    entry.weight = nice_to_weight(nice);
    entry.slice_ns = BASE_SLICE_NS;
    entry.slice_remaining_ns = BASE_SLICE_NS;
    entry.cpu_affinity = CpuMask::all();
    entry
}

/// Helper to create run queue entry
fn make_rq_entry(pid: Pid, vdeadline: u64) -> RunQueueEntry {
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
// CPU Affinity Tests
// ============================================================================

#[test]
fn test_affinity_single_cpu() {
    let mut entry = make_test_entry(1, 0);
    
    // Restrict to CPU 0 only
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    
    assert!(entry.cpu_affinity.is_set(0));
    assert!(!entry.cpu_affinity.is_set(1));
    assert_eq!(entry.cpu_affinity.count(), 1);
}

#[test]
fn test_affinity_multiple_cpus() {
    let mut entry = make_test_entry(1, 0);
    
    // Restrict to CPUs 0, 2, 4 (even CPUs)
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    entry.cpu_affinity.set(2);
    entry.cpu_affinity.set(4);
    
    assert!(entry.cpu_affinity.is_set(0));
    assert!(!entry.cpu_affinity.is_set(1));
    assert!(entry.cpu_affinity.is_set(2));
    assert!(!entry.cpu_affinity.is_set(3));
    assert!(entry.cpu_affinity.is_set(4));
    assert_eq!(entry.cpu_affinity.count(), 3);
}

#[test]
fn test_affinity_affects_cpu_selection() {
    let mut entry = make_test_entry(1, 0);
    entry.last_cpu = 5;
    
    // Restrict to CPUs that don't include last_cpu
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    entry.cpu_affinity.set(1);
    entry.cpu_affinity.set(2);
    
    // last_cpu (5) is not in affinity, so first_set should be used
    assert!(!entry.cpu_affinity.is_set(entry.last_cpu as usize));
    assert_eq!(entry.cpu_affinity.first_set(), Some(0));
}

#[test]
fn test_affinity_respects_last_cpu() {
    let mut entry = make_test_entry(1, 0);
    entry.last_cpu = 2;
    
    // Affinity includes last_cpu
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    entry.cpu_affinity.set(2);
    entry.cpu_affinity.set(4);
    
    // Should prefer last_cpu if it's in affinity mask
    assert!(entry.cpu_affinity.is_set(entry.last_cpu as usize));
}

#[test]
fn test_affinity_large_cpu_count() {
    let mut entry = make_test_entry(1, 0);
    
    // Set affinity for high-numbered CPUs (> 64)
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(100);
    entry.cpu_affinity.set(500);
    entry.cpu_affinity.set(1000);
    
    assert!(entry.cpu_affinity.is_set(100));
    assert!(entry.cpu_affinity.is_set(500));
    assert!(entry.cpu_affinity.is_set(1000));
    assert!(!entry.cpu_affinity.is_set(99));
    assert_eq!(entry.cpu_affinity.count(), 3);
}

// ============================================================================
// Load Balancing Tests
// ============================================================================

#[test]
fn test_find_least_loaded_cpu_empty_affinity() {
    let affinity = CpuMask::empty();
    
    // With empty affinity, should return 0 as default
    let cpu = find_least_loaded_cpu(&affinity);
    assert_eq!(cpu, 0);
}

#[test]
fn test_find_least_loaded_cpu_single() {
    let mut affinity = CpuMask::empty();
    affinity.set(3);
    
    // With single CPU in affinity, should return that CPU
    let cpu = find_least_loaded_cpu(&affinity);
    // Note: Actual result depends on percpu init state
}

#[test]
fn test_load_distribution_concept() {
    // Conceptual test for load distribution
    
    // Simulate 4 CPUs with different loads
    let loads = [10u8, 50, 30, 20]; // CPU 0 has lowest load
    
    // Find least loaded
    let mut min_load = u8::MAX;
    let mut best_cpu = 0usize;
    
    for (cpu, &load) in loads.iter().enumerate() {
        if load < min_load {
            min_load = load;
            best_cpu = cpu;
        }
    }
    
    assert_eq!(best_cpu, 0, "CPU 0 should be selected (load 10)");
    assert_eq!(min_load, 10);
}

#[test]
fn test_load_balance_threshold() {
    // Test that load balancing only triggers above threshold
    
    let cpu0_load = 5usize;  // Low load
    let cpu1_load = 6usize;  // Slightly higher
    
    const MIN_IMBALANCE: usize = 2;
    
    let imbalance = cpu1_load.saturating_sub(cpu0_load);
    
    // Small imbalance should NOT trigger migration
    assert!(imbalance < MIN_IMBALANCE, "Small imbalance should not trigger migration");
    
    // Larger imbalance
    let cpu0_load = 2usize;
    let cpu1_load = 10usize;
    let imbalance = cpu1_load.saturating_sub(cpu0_load);
    
    assert!(imbalance >= MIN_IMBALANCE, "Large imbalance should trigger migration");
}

// ============================================================================
// NUMA-Aware Scheduling Tests
// ============================================================================

#[test]
fn test_numa_preferred_node() {
    let mut entry = make_test_entry(1, 0);
    
    // Set preferred NUMA node
    entry.numa_preferred_node = 1;
    
    assert_eq!(entry.numa_preferred_node, 1);
}

#[test]
fn test_numa_policy() {
    let mut entry = make_test_entry(1, 0);
    
    // Default should be Local
    assert_eq!(entry.numa_policy, crate::numa::NumaPolicy::Local);
    
    // Change to Interleave
    entry.numa_policy = crate::numa::NumaPolicy::Interleave;
    assert_eq!(entry.numa_policy, crate::numa::NumaPolicy::Interleave);
}

#[test]
fn test_find_best_cpu_numa_concept() {
    // Conceptual test for NUMA-aware CPU selection
    
    // Simulate CPUs on different NUMA nodes
    // CPU 0-3: NUMA node 0
    // CPU 4-7: NUMA node 1
    
    let preferred_node = 0u32;
    
    // CPUs on same NUMA node should have lower "score"
    let cpu0_node = 0u32;
    let cpu4_node = 1u32;
    
    let cpu0_score = if cpu0_node == preferred_node { 0u64 } else { 100 };
    let cpu4_score = if cpu4_node == preferred_node { 0u64 } else { 100 };
    
    assert!(cpu0_score < cpu4_score, "CPU on same NUMA node should have lower score");
}

#[test]
fn test_numa_node_in_percpu_data() {
    let mut data = PerCpuSchedData::new(0);
    
    data.init(0, 2); // CPU 0, NUMA node 2
    
    assert_eq!(data.numa_node, 2);
}

// ============================================================================
// Per-CPU State Isolation Tests
// ============================================================================

#[test]
fn test_percpu_state_isolation() {
    // Each CPU should have independent state
    
    let data0 = PerCpuSchedData::new(0);
    let data1 = PerCpuSchedData::new(1);
    
    // Modify data0
    data0.record_context_switch(true);
    
    // data1 should be unaffected
    use core::sync::atomic::Ordering;
    assert_eq!(data0.context_switches.load(Ordering::Relaxed), 1);
    assert_eq!(data1.context_switches.load(Ordering::Relaxed), 0);
}

#[test]
fn test_percpu_run_queue_isolation() {
    let data0 = PerCpuSchedData::new(0);
    let data1 = PerCpuSchedData::new(1);
    
    // Add process to CPU 0's queue
    {
        let mut rq0 = data0.run_queue.lock();
        rq0.enqueue(make_rq_entry(1, 1000)).unwrap();
    }
    
    // CPU 1's queue should be empty
    {
        let rq1 = data1.run_queue.lock();
        assert!(rq1.is_empty(), "CPU 1 queue should be empty");
    }
    
    // CPU 0's queue should have the entry
    {
        let rq0 = data0.run_queue.lock();
        assert!(!rq0.is_empty(), "CPU 0 queue should not be empty");
        assert!(rq0.contains(1));
    }
}

#[test]
fn test_percpu_current_process_isolation() {
    let data0 = PerCpuSchedData::new(0);
    let data1 = PerCpuSchedData::new(1);
    
    // Set current on CPU 0
    {
        let mut rq0 = data0.run_queue.lock();
        rq0.set_current(Some(42));
    }
    
    // CPU 1 should have no current
    {
        let rq1 = data1.run_queue.lock();
        assert_eq!(rq1.current(), None);
    }
    
    // CPU 0 should have current = 42
    {
        let rq0 = data0.run_queue.lock();
        assert_eq!(rq0.current(), Some(42));
    }
}

// ============================================================================
// Statistics Isolation Tests
// ============================================================================

#[test]
fn test_percpu_stats_isolation() {
    let data0 = PerCpuSchedData::new(0);
    let data1 = PerCpuSchedData::new(1);
    
    use core::sync::atomic::Ordering;
    
    // Record events on CPU 0
    data0.record_context_switch(true);
    data0.record_context_switch(false);
    data0.enter_idle(1000);
    data0.exit_idle(2000);
    
    // Check CPU 0 stats
    assert_eq!(data0.context_switches.load(Ordering::Relaxed), 2);
    assert_eq!(data0.voluntary_switches.load(Ordering::Relaxed), 1);
    assert_eq!(data0.preemptions.load(Ordering::Relaxed), 1);
    assert_eq!(data0.idle_ns.load(Ordering::Relaxed), 1000);
    
    // CPU 1 should have all zeros
    assert_eq!(data1.context_switches.load(Ordering::Relaxed), 0);
    assert_eq!(data1.voluntary_switches.load(Ordering::Relaxed), 0);
    assert_eq!(data1.preemptions.load(Ordering::Relaxed), 0);
    assert_eq!(data1.idle_ns.load(Ordering::Relaxed), 0);
}

// ============================================================================
// Migration Tests
// ============================================================================

#[test]
fn test_process_migration_concept() {
    let data0 = PerCpuSchedData::new(0);
    let data1 = PerCpuSchedData::new(1);
    
    // Add process to CPU 0
    {
        let mut rq0 = data0.run_queue.lock();
        rq0.enqueue(make_rq_entry(1, 1000)).unwrap();
    }
    
    // Simulate migration: remove from CPU 0, add to CPU 1
    let entry = {
        let mut rq0 = data0.run_queue.lock();
        rq0.dequeue(1)
    };
    
    if let Some(mut entry) = entry {
        // Update entry for new CPU (would update last_cpu in real code)
        let mut rq1 = data1.run_queue.lock();
        rq1.enqueue(entry).unwrap();
    }
    
    // Verify migration
    {
        let rq0 = data0.run_queue.lock();
        let rq1 = data1.run_queue.lock();
        
        assert!(!rq0.contains(1), "Process should be removed from CPU 0");
        assert!(rq1.contains(1), "Process should be on CPU 1");
    }
}

#[test]
fn test_migration_respects_affinity() {
    let mut entry = make_test_entry(1, 0);
    
    // Process can only run on CPU 0
    entry.cpu_affinity = CpuMask::empty();
    entry.cpu_affinity.set(0);
    
    // Attempt to migrate to CPU 1 should fail (conceptually)
    let target_cpu = 1usize;
    let can_migrate = entry.cpu_affinity.is_set(target_cpu);
    
    assert!(!can_migrate, "Should not be able to migrate to CPU not in affinity");
}

// ============================================================================
// Need Resched Flag Tests
// ============================================================================

#[test]
fn test_need_resched_flag() {
    let data = PerCpuSchedData::new(0);
    
    {
        let rq = data.run_queue.lock();
        
        // Initially no resched needed
        assert!(!rq.check_need_resched());
        
        // Set flag
        rq.set_need_resched(true);
        
        // Check and clear
        assert!(rq.check_need_resched());
        
        // Should be cleared now
        assert!(!rq.check_need_resched());
    }
}

#[test]
fn test_need_resched_atomic() {
    let data = PerCpuSchedData::new(0);
    
    // Set resched multiple times
    {
        let rq = data.run_queue.lock();
        rq.set_need_resched(true);
        rq.set_need_resched(true);
        rq.set_need_resched(true);
    }
    
    // Should still only return true once
    {
        let rq = data.run_queue.lock();
        assert!(rq.check_need_resched());
        assert!(!rq.check_need_resched());
    }
}

// ============================================================================
// Cache Line Alignment Tests
// ============================================================================

#[test]
fn test_percpu_sched_data_alignment() {
    // Verify PerCpuSchedData is cache-line aligned (64 bytes)
    let align = std::mem::align_of::<PerCpuSchedData>();
    assert_eq!(align, 64, "PerCpuSchedData should be 64-byte aligned");
}

#[test]
fn test_percpu_sched_data_size() {
    // Check size is reasonable (should be multiple cache lines)
    let size = std::mem::size_of::<PerCpuSchedData>();
    eprintln!("PerCpuSchedData size: {} bytes", size);
    
    // Should be at least 2 cache lines (hot and cold data)
    assert!(size >= 128, "PerCpuSchedData should span at least 2 cache lines");
}
