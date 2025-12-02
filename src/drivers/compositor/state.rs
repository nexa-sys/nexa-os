//! Global compositor state
//!
//! This module contains all atomic state variables used for parallel
//! work coordination between CPU cores.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::numa;
use crate::smp;

use super::types::{CompositorStats, CPU_WORK_STATES};

// =============================================================================
// Multi-Core Work Distribution State
// =============================================================================

/// Global work type indicator for AP cores
pub static WORK_TYPE: AtomicU8 = AtomicU8::new(0);

/// Number of workers that have joined current work
pub static WORKERS_JOINED: AtomicUsize = AtomicUsize::new(0);

/// Signal that work is available and AP cores should wake
pub static WORK_AVAILABLE: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Compositor Core State
// =============================================================================

/// Global compositor initialization flag
pub static COMPOSITOR_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Number of workers (online CPUs) available for composition
pub static WORKER_COUNT: AtomicUsize = AtomicUsize::new(1);

/// Composition task counter for statistics
pub static TOTAL_COMPOSITIONS: AtomicU64 = AtomicU64::new(0);

/// Current composition generation (for synchronization)
pub static COMPOSITION_GEN: AtomicU64 = AtomicU64::new(0);

// =============================================================================
// Work Distribution State
// =============================================================================

/// Next stripe to be claimed
pub static WORK_NEXT_STRIPE: AtomicUsize = AtomicUsize::new(0);

/// Total number of stripes for current work
pub static WORK_TOTAL_STRIPES: AtomicUsize = AtomicUsize::new(0);

/// Number of stripes completed
pub static WORK_COMPLETED: AtomicUsize = AtomicUsize::new(0);

/// Work in progress flag
pub static WORK_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Scroll Operation State
// =============================================================================

/// Source address for scroll operation
pub static SCROLL_SRC_ADDR: AtomicU64 = AtomicU64::new(0);

/// Destination address for scroll operation
pub static SCROLL_DST_ADDR: AtomicU64 = AtomicU64::new(0);

/// Bytes per row to copy
pub static SCROLL_ROW_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Pitch (bytes per row including padding)
pub static SCROLL_PITCH: AtomicUsize = AtomicUsize::new(0);

/// Next row to process
pub static SCROLL_NEXT_ROW: AtomicUsize = AtomicUsize::new(0);

/// Total rows to process
pub static SCROLL_TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);

/// Rows completed
pub static SCROLL_ROWS_DONE: AtomicUsize = AtomicUsize::new(0);

/// Scroll distance in rows (for determining safe batch size)
pub static SCROLL_DISTANCE: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Fill Operation State
// =============================================================================

/// Buffer address for fill operation
pub static FILL_BUFFER_ADDR: AtomicU64 = AtomicU64::new(0);

/// Pitch for fill operation
pub static FILL_PITCH: AtomicUsize = AtomicUsize::new(0);

/// Width for fill operation
pub static FILL_WIDTH: AtomicUsize = AtomicUsize::new(0);

/// Bytes per pixel for fill operation
pub static FILL_BPP: AtomicUsize = AtomicUsize::new(0);

/// Color for fill operation
pub static FILL_COLOR: AtomicU32 = AtomicU32::new(0);

/// Next row to fill
pub static FILL_NEXT_ROW: AtomicUsize = AtomicUsize::new(0);

/// Total rows to fill
pub static FILL_TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);

/// Rows filled
pub static FILL_ROWS_DONE: AtomicUsize = AtomicUsize::new(0);

/// Template row for copy-based fill
pub static FILL_TEMPLATE_ROW: AtomicUsize = AtomicUsize::new(0);

/// X offset for fill operation
pub static FILL_X_OFFSET: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Composition Operation State
// =============================================================================

/// Destination buffer for composition
pub static COMPOSE_DST_BUFFER: AtomicU64 = AtomicU64::new(0);

/// Destination pitch for composition
pub static COMPOSE_DST_PITCH: AtomicUsize = AtomicUsize::new(0);

/// Destination BPP for composition
pub static COMPOSE_DST_BPP: AtomicUsize = AtomicUsize::new(0);

/// Screen width for composition
pub static COMPOSE_SCREEN_WIDTH: AtomicUsize = AtomicUsize::new(0);

/// Total rows for composition
pub static COMPOSE_TOTAL_ROWS: AtomicUsize = AtomicUsize::new(0);

/// Stripe height for composition
pub static COMPOSE_STRIPE_HEIGHT: AtomicUsize = AtomicUsize::new(0);

/// Stripes completed for composition
pub static COMPOSE_STRIPES_DONE: AtomicUsize = AtomicUsize::new(0);

/// Layer count for composition
pub static COMPOSE_LAYER_COUNT: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Public State API
// =============================================================================

/// Check if compositor is initialized
#[inline]
pub fn is_initialized() -> bool {
    COMPOSITOR_INITIALIZED.load(Ordering::Acquire)
}

/// Get number of available workers
#[inline]
pub fn worker_count() -> usize {
    WORKER_COUNT.load(Ordering::Acquire)
}

/// Get compositor statistics
pub fn stats() -> CompositorStats {
    CompositorStats {
        total_compositions: TOTAL_COMPOSITIONS.load(Ordering::Relaxed),
        last_worker_count: WORKER_COUNT.load(Ordering::Relaxed),
        last_stripes: WORK_TOTAL_STRIPES.load(Ordering::Relaxed),
        parallel_enabled: worker_count() > 1,
    }
}

/// Reset compositor statistics for benchmarking
///
/// Clears all per-CPU stripe counters and operation counters.
/// Useful for measuring performance of specific rendering operations.
pub fn reset_stats() {
    TOTAL_COMPOSITIONS.store(0, Ordering::Relaxed);
    COMPOSE_STRIPES_DONE.store(0, Ordering::Relaxed);
    SCROLL_ROWS_DONE.store(0, Ordering::Relaxed);
    FILL_ROWS_DONE.store(0, Ordering::Relaxed);

    // Reset per-CPU stats
    let online = smp::online_cpus();
    unsafe {
        for i in 0..online.min(smp::MAX_CPUS) {
            CPU_WORK_STATES[i]
                .stripes_completed
                .store(0, Ordering::Relaxed);
        }
    }
}

/// Initialize the parallel compositor
///
/// This should be called after SMP and NUMA initialization completes.
/// The compositor will detect available CPUs and NUMA topology to
/// optimize work distribution.
pub fn init() {
    if COMPOSITOR_INITIALIZED.load(Ordering::SeqCst) {
        return;
    }

    // Wait for SMP to be ready
    let online_cpus = smp::online_cpus();
    WORKER_COUNT.store(online_cpus, Ordering::SeqCst);

    // Initialize per-CPU work states with NUMA affinity
    unsafe {
        for i in 0..online_cpus.min(smp::MAX_CPUS) {
            CPU_WORK_STATES[i].reset();

            // Get NUMA node for this CPU
            if numa::is_initialized() {
                let node_id = numa::cpu_to_node(i as u32);
                CPU_WORK_STATES[i]
                    .numa_node
                    .store(node_id, Ordering::Release);
            }
        }
    }

    COMPOSITOR_INITIALIZED.store(true, Ordering::SeqCst);

    crate::kinfo!(
        "Compositor: Initialized with {} worker(s), NUMA-aware: {}",
        online_cpus,
        numa::is_initialized()
    );
}

/// Print compositor debug information
pub fn debug_info() {
    if !is_initialized() {
        crate::kinfo!("Compositor: Not initialized");
        return;
    }

    let stats = stats();
    crate::kinfo!("Compositor Status:");
    crate::kinfo!("  Workers: {}", stats.last_worker_count);
    crate::kinfo!("  Total compositions: {}", stats.total_compositions);
    crate::kinfo!("  Parallel enabled: {}", stats.parallel_enabled);
    crate::kinfo!("  NUMA nodes: {}", numa::node_count());
    crate::kinfo!(
        "  Default stripe height: {} rows",
        super::config::DEFAULT_STRIPE_HEIGHT
    );

    // Last operation stats
    let compose_stripes = COMPOSE_STRIPES_DONE.load(Ordering::Relaxed);
    let scroll_rows = SCROLL_ROWS_DONE.load(Ordering::Relaxed);
    let fill_rows = FILL_ROWS_DONE.load(Ordering::Relaxed);
    crate::kinfo!("  Last compose stripes: {}", compose_stripes);
    crate::kinfo!("  Last scroll rows: {}", scroll_rows);
    crate::kinfo!("  Last fill rows: {}", fill_rows);

    // Per-CPU stats
    let online = smp::online_cpus();
    let mut total_stripes = 0usize;
    for i in 0..online.min(smp::MAX_CPUS) {
        unsafe {
            let state = &CPU_WORK_STATES[i];
            let node = state.numa_node.load(Ordering::Relaxed);
            let stripes = state.stripes_completed.load(Ordering::Relaxed);
            total_stripes += stripes;
            let node_str = if node == numa::NUMA_NO_NODE {
                "N/A"
            } else {
                // Can't format u32 in no_std easily, just show if assigned
                "assigned"
            };
            crate::kinfo!("  CPU {}: {} stripes, NUMA node {}", i, stripes, node_str);
        }
    }

    // Work distribution summary
    if online > 1 && total_stripes > 0 {
        let avg_stripes = total_stripes / online;
        crate::kinfo!("  Avg stripes/CPU: {}", avg_stripes);
    }
}
