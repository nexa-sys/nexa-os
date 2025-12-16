//! Memory allocator tests

use crate::mm::allocator::BuddyStats;

#[test]
fn test_buddy_stats_fields() {
    let stats = BuddyStats {
        pages_allocated: 10,
        pages_free: 90,
        allocations: 5,
        frees: 3,
        splits: 2,
        merges: 1,
    };
    assert_eq!(stats.pages_allocated, 10);
    assert_eq!(stats.pages_free, 90);
    assert_eq!(stats.allocations, 5);
}
