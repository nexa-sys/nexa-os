//! Core types for the compositor
//!
//! This module defines the fundamental data structures used throughout
//! the compositor system.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::numa;
use crate::smp;

use super::config::MAX_LAYERS;

// =============================================================================
// Work Type
// =============================================================================

/// Work type for parallel operations
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkType {
    /// No work pending
    None = 0,
    /// Scroll memory copy operation
    Scroll = 1,
    /// Fill memory operation
    Fill = 2,
    /// Composition operation
    Compose = 3,
}

impl WorkType {
    /// Convert from u8 to WorkType
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => WorkType::Scroll,
            2 => WorkType::Fill,
            3 => WorkType::Compose,
            _ => WorkType::None,
        }
    }
}

// =============================================================================
// Composition Region
// =============================================================================

/// Defines a rectangular region for composition
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CompositionRegion {
    /// X coordinate of top-left corner
    pub x: u32,
    /// Y coordinate of top-left corner
    pub y: u32,
    /// Width of region
    pub width: u32,
    /// Height of region
    pub height: u32,
}

impl CompositionRegion {
    /// Create a new composition region
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Create a region covering the entire screen
    pub const fn full_screen(width: u32, height: u32) -> Self {
        Self { x: 0, y: 0, width, height }
    }

    /// Check if region is valid (non-zero area)
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Calculate area in pixels
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Check if this region intersects another
    pub fn intersects(&self, other: &Self) -> bool {
        !(self.x + self.width <= other.x
            || other.x + other.width <= self.x
            || self.y + self.height <= other.y
            || other.y + other.height <= self.y)
    }

    /// Calculate intersection with another region
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.intersects(other) {
            return None;
        }

        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);

        Some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

// =============================================================================
// Blend Mode
// =============================================================================

/// Layer blend mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendMode {
    /// Source replaces destination
    Opaque = 0,
    /// Alpha blending: dst = src * alpha + dst * (1 - alpha)
    Alpha = 1,
    /// Additive: dst = src + dst
    Additive = 2,
    /// Multiply: dst = src * dst
    Multiply = 3,
}

impl Default for BlendMode {
    fn default() -> Self {
        Self::Opaque
    }
}

// =============================================================================
// Composition Layer
// =============================================================================

/// A composition layer
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CompositionLayer {
    /// Layer enabled flag
    pub enabled: bool,
    /// Layer visibility
    pub visible: bool,
    /// Z-order (higher = on top)
    pub z_order: u16,
    /// Blend mode
    pub blend_mode: BlendMode,
    /// Global alpha (0-255)
    pub alpha: u8,
    /// Source region in layer buffer
    pub src_region: CompositionRegion,
    /// Destination region on screen
    pub dst_region: CompositionRegion,
    /// Buffer address (physical or virtual depending on context)
    pub buffer_addr: u64,
    /// Buffer pitch (bytes per row)
    pub buffer_pitch: u32,
    /// Bytes per pixel
    pub bpp: u8,
    /// NUMA node hint for memory allocation
    pub numa_node: u32,
}

impl CompositionLayer {
    /// Create an empty/disabled layer
    pub const fn empty() -> Self {
        Self {
            enabled: false,
            visible: false,
            z_order: 0,
            blend_mode: BlendMode::Opaque,
            alpha: 255,
            src_region: CompositionRegion::new(0, 0, 0, 0),
            dst_region: CompositionRegion::new(0, 0, 0, 0),
            buffer_addr: 0,
            buffer_pitch: 0,
            bpp: 0,
            numa_node: numa::NUMA_NO_NODE,
        }
    }

    /// Check if layer should be rendered
    pub fn should_render(&self) -> bool {
        self.enabled && self.visible && self.alpha > 0 && self.dst_region.is_valid()
    }
}

// =============================================================================
// Per-CPU Work State
// =============================================================================

/// Per-CPU compositor work state
#[repr(C, align(64))]  // Cache-line aligned to avoid false sharing
pub struct CpuWorkState {
    /// Current work generation this CPU is working on
    pub current_gen: AtomicU64,
    /// Number of stripes completed by this CPU
    pub stripes_completed: AtomicUsize,
    /// CPU is currently working
    pub working: AtomicBool,
    /// NUMA node this CPU belongs to
    pub numa_node: AtomicU32,
    /// Padding for cache line alignment
    _pad: [u8; 32],
}

impl CpuWorkState {
    /// Create a new CPU work state
    pub const fn new() -> Self {
        Self {
            current_gen: AtomicU64::new(0),
            stripes_completed: AtomicUsize::new(0),
            working: AtomicBool::new(false),
            numa_node: AtomicU32::new(numa::NUMA_NO_NODE),
            _pad: [0; 32],
        }
    }

    /// Reset the CPU work state
    pub fn reset(&self) {
        self.current_gen.store(0, Ordering::Release);
        self.stripes_completed.store(0, Ordering::Release);
        self.working.store(false, Ordering::Release);
    }
}

/// Per-CPU work states (MAX_CPUS from smp module)
pub static mut CPU_WORK_STATES: [CpuWorkState; smp::MAX_CPUS] = {
    const INIT: CpuWorkState = CpuWorkState::new();
    [INIT; smp::MAX_CPUS]
};

// =============================================================================
// Compositor Statistics
// =============================================================================

/// Compositor performance statistics
#[derive(Clone, Copy, Debug, Default)]
pub struct CompositorStats {
    /// Total compositions completed
    pub total_compositions: u64,
    /// Number of workers used in last composition
    pub last_worker_count: usize,
    /// Stripes processed in last composition
    pub last_stripes: usize,
    /// Whether parallel composition was used
    pub parallel_enabled: bool,
}

// =============================================================================
// Layer Storage for Parallel Composition
// =============================================================================

/// Layer storage for parallel composition (fixed-size array for no_std)
/// AP cores read this during composition work
pub static mut COMPOSE_LAYERS: [CompositionLayer; MAX_LAYERS] = {
    const INIT: CompositionLayer = CompositionLayer::empty();
    [INIT; MAX_LAYERS]
};
