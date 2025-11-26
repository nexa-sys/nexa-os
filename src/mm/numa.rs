//! NUMA (Non-Uniform Memory Access) Support
//!
//! This module provides NUMA topology detection and management for NexaOS.
//! It parses ACPI SRAT (System Resource Affinity Table) and SLIT (System Locality
//! Information Table) to determine:
//!
//! - Which CPUs belong to which NUMA nodes
//! - Which memory regions belong to which NUMA nodes
//! - The relative distance/latency between nodes
//!
//! # Architecture
//!
//! NUMA systems have multiple memory controllers, each serving a "local" set of
//! CPUs. Access to local memory is faster than access to remote memory. The kernel
//! uses this information to:
//!
//! 1. Prefer allocating memory on the node where a process is running
//! 2. Migrate processes to run closer to their memory
//! 3. Balance load while minimizing cross-node traffic
//!
//! # ACPI Tables
//!
//! - **SRAT**: Static Resource Affinity Table - maps CPUs and memory to nodes
//! - **SLIT**: System Locality Information Table - node-to-node distances

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::safety::static_slice;

// =============================================================================
// Constants
// =============================================================================

/// Maximum number of NUMA nodes supported
pub const MAX_NUMA_NODES: usize = 64;

/// Maximum number of memory affinity entries
const MAX_MEMORY_AFFINITY: usize = 256;

/// SRAT signature
const SRAT_SIGNATURE: &[u8; 4] = b"SRAT";

/// SLIT signature  
const SLIT_SIGNATURE: &[u8; 4] = b"SLIT";

/// NUMA_NO_NODE indicates no preferred NUMA node
pub const NUMA_NO_NODE: u32 = 0xFFFFFFFF;

/// Local distance (same node)
pub const LOCAL_DISTANCE: u8 = 10;

/// Remote distance (default for different nodes)
pub const REMOTE_DISTANCE: u8 = 20;

/// Unreachable distance
pub const UNREACHABLE_DISTANCE: u8 = 255;

// =============================================================================
// ACPI Table Structures
// =============================================================================

/// ACPI SDT Header (common to all ACPI tables)
#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

/// SRAT table header
#[repr(C, packed)]
struct Srat {
    header: SdtHeader,
    reserved1: u32, // Table revision (must be 1)
    reserved2: u64, // Reserved
                    // Followed by affinity structures
}

/// SRAT Local APIC/SAPIC Affinity Structure (Type 0)
#[repr(C, packed)]
struct SratLocalApicAffinity {
    entry_type: u8,        // 0
    length: u8,            // 16
    proximity_domain_lo: u8, // Low byte of proximity domain
    apic_id: u8,
    flags: u32,
    local_sapic_eid: u8,
    proximity_domain_hi: [u8; 3], // High bytes of proximity domain
    clock_domain: u32,
}

/// SRAT Memory Affinity Structure (Type 1)
#[repr(C, packed)]
struct SratMemoryAffinity {
    entry_type: u8,       // 1
    length: u8,           // 40
    proximity_domain: u32,
    reserved1: u16,
    base_address_lo: u32,
    base_address_hi: u32,
    length_lo: u32,
    length_hi: u32,
    reserved2: u32,
    flags: u32,
    reserved3: u64,
}

/// SRAT x2APIC Affinity Structure (Type 2)
#[repr(C, packed)]
struct SratX2ApicAffinity {
    entry_type: u8,  // 2
    length: u8,      // 24
    reserved1: u16,
    proximity_domain: u32,
    x2apic_id: u32,
    flags: u32,
    clock_domain: u32,
    reserved2: u32,
}

/// SLIT table header
#[repr(C, packed)]
struct Slit {
    header: SdtHeader,
    localities: u64,
    // Followed by localities * localities bytes of distance matrix
}

// =============================================================================
// NUMA Data Structures
// =============================================================================

/// NUMA node information
#[derive(Clone, Copy, Debug)]
pub struct NumaNode {
    /// Node ID (proximity domain)
    pub id: u32,
    /// Whether this node is online/valid
    pub online: bool,
    /// Number of CPUs on this node
    pub cpu_count: u32,
    /// Total memory on this node (in bytes)
    pub memory_size: u64,
    /// Base address of memory on this node
    pub memory_base: u64,
}

impl NumaNode {
    const fn empty() -> Self {
        Self {
            id: NUMA_NO_NODE,
            online: false,
            cpu_count: 0,
            memory_size: 0,
            memory_base: 0,
        }
    }
}

/// CPU to NUMA node mapping
#[derive(Clone, Copy, Debug)]
pub struct CpuNumaMapping {
    pub apic_id: u32,
    pub numa_node: u32,
}

impl CpuNumaMapping {
    const fn empty() -> Self {
        Self {
            apic_id: 0,
            numa_node: NUMA_NO_NODE,
        }
    }
}

/// Memory region to NUMA node mapping
#[derive(Clone, Copy, Debug)]
pub struct MemoryNumaMapping {
    pub base: u64,
    pub size: u64,
    pub numa_node: u32,
    pub hotpluggable: bool,
    pub nonvolatile: bool,
}

impl MemoryNumaMapping {
    const fn empty() -> Self {
        Self {
            base: 0,
            size: 0,
            numa_node: NUMA_NO_NODE,
            hotpluggable: false,
            nonvolatile: false,
        }
    }
}

// =============================================================================
// Global State
// =============================================================================

static NUMA_INITIALIZED: AtomicBool = AtomicBool::new(false);
static NUMA_NODE_COUNT: AtomicU32 = AtomicU32::new(0);

/// NUMA nodes
static mut NUMA_NODES: [NumaNode; MAX_NUMA_NODES] = [NumaNode::empty(); MAX_NUMA_NODES];

/// CPU to node mapping (indexed by APIC ID, up to 1024 CPUs)
static mut CPU_TO_NODE: [u32; crate::acpi::MAX_CPUS] = [NUMA_NO_NODE; crate::acpi::MAX_CPUS];

/// Memory affinity entries
static mut MEMORY_AFFINITY: [MemoryNumaMapping; MAX_MEMORY_AFFINITY] =
    [MemoryNumaMapping::empty(); MAX_MEMORY_AFFINITY];
static mut MEMORY_AFFINITY_COUNT: usize = 0;

/// Node distance matrix (SLIT data)
/// Distance from node i to node j is DISTANCE_MATRIX[i * MAX_NUMA_NODES + j]
static mut DISTANCE_MATRIX: [u8; MAX_NUMA_NODES * MAX_NUMA_NODES] =
    [REMOTE_DISTANCE; MAX_NUMA_NODES * MAX_NUMA_NODES];

// =============================================================================
// Public API
// =============================================================================

/// Initialize NUMA subsystem
///
/// This should be called after ACPI initialization.
/// If SRAT is not available, the system is treated as UMA (single node).
pub fn init() -> Result<(), &'static str> {
    if NUMA_INITIALIZED.load(Ordering::SeqCst) {
        return Ok(());
    }

    crate::kinfo!("NUMA: Initializing...");

    // Initialize default distance matrix (local = 10, remote = 20)
    unsafe {
        for i in 0..MAX_NUMA_NODES {
            for j in 0..MAX_NUMA_NODES {
                let idx = i * MAX_NUMA_NODES + j;
                DISTANCE_MATRIX[idx] = if i == j {
                    LOCAL_DISTANCE
                } else {
                    REMOTE_DISTANCE
                };
            }
        }
    }

    // Try to parse SRAT
    match parse_srat() {
        Ok(node_count) => {
            NUMA_NODE_COUNT.store(node_count, Ordering::SeqCst);
            crate::kinfo!("NUMA: Detected {} nodes from SRAT", node_count);
        }
        Err(e) => {
            // No SRAT - treat as single-node system
            crate::kinfo!("NUMA: {} - using UMA mode (single node)", e);
            setup_uma_fallback();
        }
    }

    // Try to parse SLIT for distance information
    if let Err(e) = parse_slit() {
        crate::kdebug!("NUMA: SLIT not available ({}), using default distances", e);
    }

    NUMA_INITIALIZED.store(true, Ordering::SeqCst);

    // Log NUMA topology
    log_topology();

    Ok(())
}

/// Check if NUMA is initialized
pub fn is_initialized() -> bool {
    NUMA_INITIALIZED.load(Ordering::Acquire)
}

/// Get the number of NUMA nodes
pub fn node_count() -> u32 {
    if !is_initialized() {
        return 1;
    }
    NUMA_NODE_COUNT.load(Ordering::Relaxed).max(1)
}

/// Get information about a NUMA node
pub fn get_node(node_id: u32) -> Option<&'static NumaNode> {
    if !is_initialized() || node_id >= MAX_NUMA_NODES as u32 {
        return None;
    }
    unsafe {
        let node = &NUMA_NODES[node_id as usize];
        if node.online {
            Some(node)
        } else {
            None
        }
    }
}

/// Get the NUMA node for a given CPU (by APIC ID)
pub fn cpu_to_node(apic_id: u32) -> u32 {
    if !is_initialized() {
        return 0;
    }
    if apic_id as usize >= crate::acpi::MAX_CPUS {
        return 0;
    }
    unsafe {
        let node = CPU_TO_NODE[apic_id as usize];
        if node == NUMA_NO_NODE {
            0
        } else {
            node
        }
    }
}

/// Get the NUMA node for a given physical address
pub fn addr_to_node(addr: u64) -> u32 {
    if !is_initialized() {
        return 0;
    }

    unsafe {
        for i in 0..MEMORY_AFFINITY_COUNT {
            let entry = &MEMORY_AFFINITY[i];
            if addr >= entry.base && addr < entry.base + entry.size {
                return entry.numa_node;
            }
        }
    }

    // Default to node 0 if not found
    0
}

/// Get the distance between two NUMA nodes
pub fn node_distance(from: u32, to: u32) -> u8 {
    if !is_initialized() {
        return LOCAL_DISTANCE;
    }
    if from >= MAX_NUMA_NODES as u32 || to >= MAX_NUMA_NODES as u32 {
        return UNREACHABLE_DISTANCE;
    }
    unsafe {
        DISTANCE_MATRIX[from as usize * MAX_NUMA_NODES + to as usize]
    }
}

/// Get all online NUMA nodes
pub fn online_nodes() -> &'static [NumaNode] {
    if !is_initialized() {
        return &[];
    }

    unsafe {
        let count = NUMA_NODE_COUNT.load(Ordering::Relaxed) as usize;
        static_slice(&NUMA_NODES[0], count.min(MAX_NUMA_NODES))
    }
}

/// Get memory affinity entries
pub fn memory_affinity_entries() -> &'static [MemoryNumaMapping] {
    if !is_initialized() {
        return &[];
    }

    unsafe {
        static_slice(&MEMORY_AFFINITY[0], MEMORY_AFFINITY_COUNT)
    }
}

/// Get the preferred NUMA node for the current CPU
pub fn current_node() -> u32 {
    if !is_initialized() {
        return 0;
    }
    let apic_id = crate::lapic::current_apic_id();
    cpu_to_node(apic_id)
}

/// Get list of CPUs on a specific NUMA node
pub fn cpus_on_node(node_id: u32) -> impl Iterator<Item = u32> {
    let cpus = crate::acpi::cpus();
    let node = node_id;
    cpus.iter()
        .filter(move |cpu| cpu.enabled && cpu_to_node(cpu.apic_id as u32) == node)
        .map(|cpu| cpu.apic_id as u32)
}

// =============================================================================
// SRAT Parsing
// =============================================================================

/// Parse SRAT table from ACPI
fn parse_srat() -> Result<u32, &'static str> {
    // Get RSDP from bootinfo or scan
    let rsdp_addr = crate::bootinfo::acpi_rsdp_addr().ok_or("No ACPI RSDP available")?;

    unsafe {
        let srat = find_acpi_table(rsdp_addr, SRAT_SIGNATURE).ok_or("SRAT not found")?;
        let srat = &*(srat as *const Srat);

        let entries_start = (srat as *const Srat as *const u8).add(core::mem::size_of::<Srat>());
        let entries_len = (srat.header.length as usize).saturating_sub(core::mem::size_of::<Srat>());

        let mut offset = 0usize;
        let mut max_node = 0u32;

        while offset + 2 <= entries_len {
            let entry_ptr = entries_start.add(offset);
            let entry_type = entry_ptr.read();
            let entry_len = entry_ptr.add(1).read() as usize;

            if entry_len < 2 || offset + entry_len > entries_len {
                break;
            }

            match entry_type {
                0 => {
                    // Local APIC Affinity
                    if entry_len >= core::mem::size_of::<SratLocalApicAffinity>() {
                        let apic_aff = &*(entry_ptr as *const SratLocalApicAffinity);
                        let flags = apic_aff.flags;

                        // Check if enabled
                        if flags & 1 != 0 {
                            let proximity = (apic_aff.proximity_domain_lo as u32)
                                | ((apic_aff.proximity_domain_hi[0] as u32) << 8)
                                | ((apic_aff.proximity_domain_hi[1] as u32) << 16)
                                | ((apic_aff.proximity_domain_hi[2] as u32) << 24);

                            let apic_id = apic_aff.apic_id as u32;
                            register_cpu_affinity(apic_id, proximity);

                            if proximity > max_node {
                                max_node = proximity;
                            }
                        }
                    }
                }
                1 => {
                    // Memory Affinity
                    if entry_len >= core::mem::size_of::<SratMemoryAffinity>() {
                        let mem_aff = &*(entry_ptr as *const SratMemoryAffinity);
                        let flags = mem_aff.flags;

                        // Check if enabled
                        if flags & 1 != 0 {
                            let base = (mem_aff.base_address_lo as u64)
                                | ((mem_aff.base_address_hi as u64) << 32);
                            let length =
                                (mem_aff.length_lo as u64) | ((mem_aff.length_hi as u64) << 32);
                            let proximity = mem_aff.proximity_domain;
                            let hotpluggable = (flags & 2) != 0;
                            let nonvolatile = (flags & 4) != 0;

                            register_memory_affinity(
                                base,
                                length,
                                proximity,
                                hotpluggable,
                                nonvolatile,
                            );

                            if proximity > max_node {
                                max_node = proximity;
                            }
                        }
                    }
                }
                2 => {
                    // x2APIC Affinity
                    if entry_len >= core::mem::size_of::<SratX2ApicAffinity>() {
                        let x2apic_aff = &*(entry_ptr as *const SratX2ApicAffinity);
                        let flags = x2apic_aff.flags;

                        // Check if enabled
                        if flags & 1 != 0 {
                            let apic_id = x2apic_aff.x2apic_id;
                            let proximity = x2apic_aff.proximity_domain;

                            register_cpu_affinity(apic_id, proximity);

                            if proximity > max_node {
                                max_node = proximity;
                            }
                        }
                    }
                }
                _ => {
                    // Unknown entry type, skip
                }
            }

            offset += entry_len;
        }

        // Mark discovered nodes as online
        for i in 0..=max_node.min(MAX_NUMA_NODES as u32 - 1) {
            NUMA_NODES[i as usize].id = i;
            NUMA_NODES[i as usize].online = true;
        }

        Ok(max_node + 1)
    }
}

/// Register CPU affinity from SRAT
unsafe fn register_cpu_affinity(apic_id: u32, proximity_domain: u32) {
    if apic_id as usize >= crate::acpi::MAX_CPUS {
        return;
    }
    if proximity_domain >= MAX_NUMA_NODES as u32 {
        return;
    }

    CPU_TO_NODE[apic_id as usize] = proximity_domain;
    NUMA_NODES[proximity_domain as usize].cpu_count += 1;

    crate::kdebug!(
        "NUMA: CPU APIC {} -> Node {}",
        apic_id,
        proximity_domain
    );
}

/// Register memory affinity from SRAT
unsafe fn register_memory_affinity(
    base: u64,
    size: u64,
    proximity_domain: u32,
    hotpluggable: bool,
    nonvolatile: bool,
) {
    if MEMORY_AFFINITY_COUNT >= MAX_MEMORY_AFFINITY {
        return;
    }
    if proximity_domain >= MAX_NUMA_NODES as u32 {
        return;
    }

    let idx = MEMORY_AFFINITY_COUNT;
    MEMORY_AFFINITY[idx] = MemoryNumaMapping {
        base,
        size,
        numa_node: proximity_domain,
        hotpluggable,
        nonvolatile,
    };
    MEMORY_AFFINITY_COUNT += 1;

    // Update node memory info
    let node = &mut NUMA_NODES[proximity_domain as usize];
    if node.memory_size == 0 || base < node.memory_base {
        node.memory_base = base;
    }
    node.memory_size += size;

    crate::kdebug!(
        "NUMA: Memory {:#x}-{:#x} ({} MB) -> Node {}{}{}",
        base,
        base + size,
        size / (1024 * 1024),
        proximity_domain,
        if hotpluggable { " [hotplug]" } else { "" },
        if nonvolatile { " [nvdimm]" } else { "" }
    );
}

// =============================================================================
// SLIT Parsing
// =============================================================================

/// Parse SLIT table from ACPI
fn parse_slit() -> Result<(), &'static str> {
    let rsdp_addr = crate::bootinfo::acpi_rsdp_addr().ok_or("No ACPI RSDP available")?;

    unsafe {
        let slit = find_acpi_table(rsdp_addr, SLIT_SIGNATURE).ok_or("SLIT not found")?;
        let slit = &*(slit as *const Slit);

        let localities = slit.localities as usize;
        if localities == 0 || localities > MAX_NUMA_NODES {
            return Err("Invalid SLIT localities count");
        }

        let matrix_ptr = (slit as *const Slit as *const u8).add(core::mem::size_of::<Slit>());

        for i in 0..localities {
            for j in 0..localities {
                let distance = matrix_ptr.add(i * localities + j).read();
                if i < MAX_NUMA_NODES && j < MAX_NUMA_NODES {
                    DISTANCE_MATRIX[i * MAX_NUMA_NODES + j] = distance;
                }
            }
        }

        crate::kinfo!("NUMA: Parsed SLIT with {} localities", localities);
        Ok(())
    }
}

// =============================================================================
// ACPI Table Lookup
// =============================================================================

/// Find an ACPI table by signature
unsafe fn find_acpi_table(rsdp_addr: u64, signature: &[u8; 4]) -> Option<*const u8> {
    #[repr(C, packed)]
    struct RsdpV2 {
        signature: [u8; 8],
        checksum: u8,
        oem_id: [u8; 6],
        revision: u8,
        rsdt_address: u32,
        length: u32,
        xsdt_address: u64,
        extended_checksum: u8,
        reserved: [u8; 3],
    }

    let rsdp = &*(rsdp_addr as *const RsdpV2);

    // Try XSDT first (64-bit pointers)
    if rsdp.revision >= 2 && rsdp.xsdt_address != 0 {
        if let Some(table) = scan_sdt_for_table(rsdp.xsdt_address, true, signature) {
            return Some(table);
        }
    }

    // Fall back to RSDT (32-bit pointers)
    if rsdp.rsdt_address != 0 {
        if let Some(table) = scan_sdt_for_table(rsdp.rsdt_address as u64, false, signature) {
            return Some(table);
        }
    }

    None
}

/// Scan SDT (RSDT or XSDT) for a table with the given signature
unsafe fn scan_sdt_for_table(
    sdt_addr: u64,
    is_xsdt: bool,
    signature: &[u8; 4],
) -> Option<*const u8> {
    let header = &*(sdt_addr as *const SdtHeader);
    let entries_len = (header.length as usize).saturating_sub(core::mem::size_of::<SdtHeader>());
    let entry_size = if is_xsdt { 8 } else { 4 };
    let entry_count = entries_len / entry_size;
    let entries_ptr = (sdt_addr as *const u8).add(core::mem::size_of::<SdtHeader>());

    for idx in 0..entry_count {
        let entry_addr = if is_xsdt {
            ptr::read_unaligned(entries_ptr.add(idx * entry_size) as *const u64)
        } else {
            ptr::read_unaligned(entries_ptr.add(idx * entry_size) as *const u32) as u64
        };

        if entry_addr == 0 {
            continue;
        }

        let candidate = &*(entry_addr as *const SdtHeader);
        if &candidate.signature == signature {
            return Some(entry_addr as *const u8);
        }
    }

    None
}

// =============================================================================
// UMA Fallback
// =============================================================================

/// Setup single-node (UMA) fallback when SRAT is not available
fn setup_uma_fallback() {
    unsafe {
        // Single node containing all CPUs
        NUMA_NODES[0] = NumaNode {
            id: 0,
            online: true,
            cpu_count: 0,
            memory_size: 0,
            memory_base: 0,
        };

        // Map all CPUs to node 0
        for cpu in crate::acpi::cpus() {
            if cpu.enabled {
                CPU_TO_NODE[cpu.apic_id as usize] = 0;
                NUMA_NODES[0].cpu_count += 1;
            }
        }

        NUMA_NODE_COUNT.store(1, Ordering::SeqCst);
    }
}

// =============================================================================
// Diagnostics
// =============================================================================

/// Log NUMA topology information
fn log_topology() {
    let count = node_count();
    crate::kinfo!("NUMA: {} node(s) configured", count);

    for i in 0..count {
        if let Some(node) = get_node(i) {
            crate::kinfo!(
                "  Node {}: {} CPUs, {} MB memory @ {:#x}",
                node.id,
                node.cpu_count,
                node.memory_size / (1024 * 1024),
                node.memory_base
            );
        }
    }

    // Log distance matrix if more than one node
    if count > 1 {
        crate::kinfo!("NUMA: Distance matrix:");
        for i in 0..count {
            let mut distances = [0u8; MAX_NUMA_NODES];
            for j in 0..count {
                distances[j as usize] = node_distance(i, j);
            }
            let dist_str: &[u8] = &distances[..count as usize];
            crate::kdebug!("  Node {}: {:?}", i, dist_str);
        }
    }
}

// =============================================================================
// NUMA-Aware Allocation Hints
// =============================================================================

/// Hint for NUMA-aware memory allocation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumaPolicy {
    /// Allocate from the local node (default)
    Local,
    /// Allocate from the specified node
    Bind(u32),
    /// Interleave allocations across all nodes
    Interleave,
    /// Prefer local node but fall back to others
    Preferred(u32),
}

impl Default for NumaPolicy {
    fn default() -> Self {
        NumaPolicy::Local
    }
}

/// Get the best node for allocation based on policy
pub fn best_node_for_policy(policy: NumaPolicy) -> u32 {
    match policy {
        NumaPolicy::Local => current_node(),
        NumaPolicy::Bind(node) => node.min(node_count().saturating_sub(1)),
        NumaPolicy::Interleave => {
            // Simple round-robin for interleave
            static INTERLEAVE_COUNTER: AtomicU32 = AtomicU32::new(0);
            let count = node_count();
            if count <= 1 {
                0
            } else {
                INTERLEAVE_COUNTER.fetch_add(1, Ordering::Relaxed) % count
            }
        }
        NumaPolicy::Preferred(node) => {
            if node < node_count() {
                node
            } else {
                current_node()
            }
        }
    }
}
