//! Compositor configuration constants
//!
//! Optimized for 2.5K (2560x1440) and higher resolution displays.

/// Maximum number of composition layers supported
pub const MAX_LAYERS: usize = 16;

/// Maximum number of pending composition tasks per CPU
pub const MAX_TASKS_PER_CPU: usize = 32;

/// Minimum rows per worker for effective parallelization
/// Reduced to enable parallelism on smaller regions
pub const MIN_ROWS_PER_WORKER: usize = 8;

/// Default stripe height for parallel composition
/// Optimized for L2 cache (256KB typical):
/// For 2560px width @ 4bpp = 10240 bytes/row
/// 16 rows = 164KB, leaves room for source data in L2
/// Smaller stripes = better load balancing on many-core systems
pub const DEFAULT_STRIPE_HEIGHT: usize = 16;

/// Threshold for using fast memset-style fill (in pixels)
pub(crate) const FAST_FILL_THRESHOLD: usize = 4;

/// Threshold for using batch pixel processing
/// Set to 8 to ensure enough pixels for efficient 64-bit operations
/// (8 pixels = 32 bytes, half a cache line)
pub(crate) const BATCH_BLEND_THRESHOLD: usize = 8;

/// SIMD batch size - process 16 pixels at a time for better instruction pipelining
/// 16 pixels = 64 bytes = 1 full cache line (optimal for streaming)
pub(crate) const SIMD_BATCH_SIZE: usize = 16;

/// Large SIMD batch for aligned bulk operations (32 pixels = 128 bytes)
#[allow(dead_code)]
pub(crate) const SIMD_LARGE_BATCH: usize = 32;

/// Use 64-bit writes for double-pixel operations (2 pixels = 8 bytes)
#[allow(dead_code)]
pub(crate) const PIXEL_PAIR_SIZE: usize = 2;

/// Cache line size for memory alignment (64 bytes on x86-64)
#[allow(dead_code)]
pub(crate) const CACHE_LINE_SIZE: usize = 64;

/// Prefetch distance in cache lines (optimized for DDR4 latency)
/// ~8 cache lines = 512 bytes ahead for streaming access
#[allow(dead_code)]
pub(crate) const PREFETCH_DISTANCE_LINES: usize = 8;

/// Tile size for tile-based rendering (GPU-inspired)
/// 64x64 = 4096 pixels × 4 bytes = 16KB, fits in L1 cache
pub const TILE_SIZE: usize = 64;

/// Threshold for parallel scroll (in total bytes to move)
/// For 2.5K (2560x1440x4bpp ≈ 14MB), always use parallel
/// Reduced to 64KB for aggressive parallelization
pub(crate) const PARALLEL_SCROLL_THRESHOLD: usize = 64 * 1024; // 64KB

/// Threshold for parallel fill (in total bytes)
pub(crate) const PARALLEL_FILL_THRESHOLD: usize = 32 * 1024; // 32KB

/// Write combining buffer size hint (typically 4 writes)
#[allow(dead_code)]
pub(crate) const WRITE_COMBINE_THRESHOLD: usize = 4;

/// Maximum number of dirty regions to track before full repaint
pub(crate) const MAX_DIRTY_REGIONS: usize = 16;
