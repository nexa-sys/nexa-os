//! Parallel Display Compositor
//!
//! This module provides NUMA-aware parallel display composition that leverages
//! SMP to accelerate framebuffer rendering operations. It distributes rendering
//! work across multiple CPU cores after SMP and NUMA initialization completes.
//!
//! # Architecture
//!
//! ```text
//! +------------------+     +------------------+
//! |  CPU 0 (BSP)     |     |  CPU 1 (AP)      |
//! | Row 0-31         | <-> | Row 32-63        |
//! +------------------+     +------------------+
//!           |                      |
//!           v                      v
//!       +-------------------------------+
//!       |       Framebuffer Memory      |
//!       +-------------------------------+
//! ```
//!
//! # Features
//!
//! - NUMA-aware memory allocation for composition buffers
//! - Lock-free work distribution using atomic operations
//! - Per-CPU rendering queues to minimize contention
//! - Stripe-based parallelization for cache efficiency
//!
//! # Module Structure
//!
//! - `config` - Configuration constants
//! - `types` - Core data types (CompositionRegion, CompositionLayer, etc.)
//! - `state` - Global atomic state variables
//! - `blend` - Pixel blending algorithms
//! - `workers` - Parallel worker functions
//! - `ops` - High-level operations (compose, fill, scroll)
//! - `memory` - High-performance memory operations
//! - `double_buffer` - Double buffering support
//! - `dirty_region` - Dirty region tracking

pub mod config;
pub mod types;
pub mod state;
pub mod blend;
pub mod workers;
pub mod ops;
pub mod memory;
pub mod double_buffer;
pub mod dirty_region;

// =============================================================================
// Re-exports for backward compatibility
// =============================================================================

// Configuration constants
pub use config::{
    MAX_LAYERS,
    MAX_TASKS_PER_CPU,
    MIN_ROWS_PER_WORKER,
    DEFAULT_STRIPE_HEIGHT,
    TILE_SIZE,
};

// Core types
pub use types::{
    WorkType,
    CompositionRegion,
    BlendMode,
    CompositionLayer,
    CpuWorkState,
    CompositorStats,
};

// State and initialization
pub use state::{
    init,
    is_initialized,
    worker_count,
    stats,
    reset_stats,
    debug_info,
};

// High-level operations
pub use ops::{
    compose,
    fill_rect,
    copy_rect,
    scroll_up_fast,
    parallel_fill,
    clear_rows_fast,
    optimal_stripe_height,
};

// Worker functions (for direct use if needed)
pub use workers::{
    ap_work_entry,
    worker_compose,
};

// Memory operations
pub use memory::{
    fast_fill_u64,
    fast_copy_prefetch,
    streaming_fill_32,
};

// Double buffering
pub use double_buffer::{
    DoubleBuffer,
    DOUBLE_BUFFER,
    copy_to_front,
};

// Dirty region tracking
pub use dirty_region::DirtyRegionTracker;
