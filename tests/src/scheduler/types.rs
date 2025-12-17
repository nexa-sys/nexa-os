//! Scheduler tests

use crate::scheduler::{CpuMask, SchedPolicy};

#[test]
fn test_cpu_mask_empty() {
    let mask = CpuMask::empty();
    assert!(mask.is_empty());
    assert_eq!(mask.count(), 0);
}

#[test]
fn test_cpu_mask_operations() {
    let mut mask = CpuMask::empty();

    mask.set(5);
    assert!(mask.is_set(5));
    assert!(!mask.is_set(4));
    assert_eq!(mask.count(), 1);

    mask.set(10);
    assert_eq!(mask.count(), 2);

    mask.clear(5);
    assert!(!mask.is_set(5));
    assert_eq!(mask.count(), 1);
}

#[test]
fn test_cpu_mask_all() {
    let mask = CpuMask::all();
    assert!(!mask.is_empty());
    for i in 0..64 {
        assert!(mask.is_set(i));
    }
}

#[test]
fn test_sched_policy_equality() {
    assert_ne!(SchedPolicy::Normal, SchedPolicy::Batch);
    assert_ne!(SchedPolicy::Batch, SchedPolicy::Idle);
    assert_eq!(SchedPolicy::Normal, SchedPolicy::Normal);
}
