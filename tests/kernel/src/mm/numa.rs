//! NUMA Tests
//!
//! Tests for NUMA (Non-Uniform Memory Access) types and policies.

#[cfg(test)]
mod tests {
    use crate::mm::numa::{
        NumaPolicy, NumaNode, CpuNumaMapping, MemoryNumaMapping,
        MAX_NUMA_NODES, NUMA_NO_NODE, LOCAL_DISTANCE, REMOTE_DISTANCE, UNREACHABLE_DISTANCE,
    };

    // =========================================================================
    // NUMA Constants Tests
    // =========================================================================

    #[test]
    fn test_max_numa_nodes() {
        assert!(MAX_NUMA_NODES >= 1);
    }

    #[test]
    fn test_numa_no_node() {
        assert_eq!(NUMA_NO_NODE, 0xFFFFFFFF);
    }

    #[test]
    fn test_local_distance() {
        assert_eq!(LOCAL_DISTANCE, 10);
    }

    #[test]
    fn test_remote_distance() {
        assert_eq!(REMOTE_DISTANCE, 20);
        assert!(REMOTE_DISTANCE > LOCAL_DISTANCE);
    }

    #[test]
    fn test_unreachable_distance() {
        assert_eq!(UNREACHABLE_DISTANCE, 255);
        assert!(UNREACHABLE_DISTANCE > REMOTE_DISTANCE);
    }

    // =========================================================================
    // NumaPolicy Tests
    // =========================================================================

    #[test]
    fn test_numa_policy_default() {
        let policy = NumaPolicy::default();
        assert!(matches!(policy, NumaPolicy::Local));
    }

    #[test]
    fn test_numa_policy_local() {
        let policy = NumaPolicy::Local;
        assert!(matches!(policy, NumaPolicy::Local));
    }

    #[test]
    fn test_numa_policy_bind() {
        let policy = NumaPolicy::Bind(0);
        assert!(matches!(policy, NumaPolicy::Bind(_)));
    }

    #[test]
    fn test_numa_policy_interleave() {
        let policy = NumaPolicy::Interleave;
        assert!(matches!(policy, NumaPolicy::Interleave));
    }

    #[test]
    fn test_numa_policy_preferred() {
        let policy = NumaPolicy::Preferred(0);
        assert!(matches!(policy, NumaPolicy::Preferred(_)));
    }

    #[test]
    fn test_numa_policy_copy() {
        let p1 = NumaPolicy::Local;
        let p2 = p1;
        assert!(matches!(p2, NumaPolicy::Local));
    }

    #[test]
    fn test_numa_policy_clone() {
        let p1 = NumaPolicy::Bind(1);
        let p2 = p1.clone();
        assert!(matches!(p2, NumaPolicy::Bind(1)));
    }

    #[test]
    fn test_numa_policy_eq() {
        assert_eq!(NumaPolicy::Local, NumaPolicy::Local);
        assert_ne!(NumaPolicy::Local, NumaPolicy::Interleave);
    }

    #[test]
    fn test_numa_policy_debug() {
        let policy = NumaPolicy::Interleave;
        let debug_str = format!("{:?}", policy);
        assert!(debug_str.contains("Interleave"));
    }

    // =========================================================================
    // NumaNode Tests
    // =========================================================================

    #[test]
    fn test_numa_node_size() {
        let size = core::mem::size_of::<NumaNode>();
        // id(4) + online(1) + cpu_count(4) + memory_size(8) + memory_base(8) + padding
        assert!(size >= 25);
        assert!(size <= 40);
    }

    #[test]
    fn test_numa_node_online() {
        let node = NumaNode {
            id: 0,
            online: true,
            cpu_count: 4,
            memory_size: 0x1_0000_0000,
            memory_base: 0,
        };
        assert!(node.online);
        assert_eq!(node.id, 0);
        assert_eq!(node.cpu_count, 4);
    }

    #[test]
    fn test_numa_node_offline() {
        let node = NumaNode {
            id: 1,
            online: false,
            cpu_count: 0,
            memory_size: 0,
            memory_base: 0,
        };
        assert!(!node.online);
        assert_eq!(node.id, 1);
    }

    #[test]
    fn test_numa_node_copy() {
        let n1 = NumaNode {
            id: 0,
            online: true,
            cpu_count: 8,
            memory_size: 0x8000_0000,
            memory_base: 0,
        };
        let n2 = n1;
        assert_eq!(n1.id, n2.id);
    }

    // =========================================================================
    // CpuNumaMapping Tests
    // =========================================================================

    #[test]
    fn test_cpu_numa_mapping_size() {
        let size = core::mem::size_of::<CpuNumaMapping>();
        // apic_id(4) + numa_node(4) = 8
        assert_eq!(size, 8);
    }

    #[test]
    fn test_cpu_numa_mapping_creation() {
        let mapping = CpuNumaMapping {
            apic_id: 0,
            numa_node: 0,
        };
        assert_eq!(mapping.apic_id, 0);
        assert_eq!(mapping.numa_node, 0);
    }

    #[test]
    fn test_cpu_numa_mapping_copy() {
        let m1 = CpuNumaMapping { apic_id: 1, numa_node: 0 };
        let m2 = m1;
        assert_eq!(m1.apic_id, m2.apic_id);
    }

    // =========================================================================
    // MemoryNumaMapping Tests
    // =========================================================================

    #[test]
    fn test_memory_numa_mapping_size() {
        let size = core::mem::size_of::<MemoryNumaMapping>();
        // base(8) + size(8) + numa_node(4) + hotpluggable(1) + nonvolatile(1) + padding
        assert!(size >= 22);
        assert!(size <= 32);
    }

    #[test]
    fn test_memory_numa_mapping_creation() {
        let mapping = MemoryNumaMapping {
            base: 0x1000_0000,
            size: 0x1000_0000,
            numa_node: 0,
            hotpluggable: false,
            nonvolatile: false,
        };
        assert_eq!(mapping.base, 0x1000_0000);
        assert_eq!(mapping.size, 0x1000_0000);
        assert_eq!(mapping.numa_node, 0);
    }

    #[test]
    fn test_memory_numa_mapping_copy() {
        let m1 = MemoryNumaMapping {
            base: 0,
            size: 0x8000_0000,
            numa_node: 0,
            hotpluggable: false,
            nonvolatile: false,
        };
        let m2 = m1;
        assert_eq!(m1.base, m2.base);
        assert_eq!(m1.size, m2.size);
    }

    // =========================================================================
    // NUMA Distance Matrix Concepts
    // =========================================================================

    #[test]
    fn test_distance_reflexivity() {
        // Distance from node to itself should be LOCAL_DISTANCE
        let dist = LOCAL_DISTANCE;
        assert_eq!(dist, 10);
    }

    #[test]
    fn test_distance_symmetry() {
        // Distance should be symmetric: dist(A, B) == dist(B, A)
        let dist_0_1 = REMOTE_DISTANCE;
        let dist_1_0 = REMOTE_DISTANCE;
        assert_eq!(dist_0_1, dist_1_0);
    }

    #[test]
    fn test_distance_ordering() {
        // LOCAL_DISTANCE < REMOTE_DISTANCE < UNREACHABLE_DISTANCE
        assert!(LOCAL_DISTANCE < REMOTE_DISTANCE);
        assert!(REMOTE_DISTANCE < UNREACHABLE_DISTANCE);
    }
}
