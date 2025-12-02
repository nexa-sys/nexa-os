//! Double buffering support
//!
//! Provides tear-free rendering through double buffering techniques.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use super::config::PARALLEL_SCROLL_THRESHOLD;
use super::state::*;
use super::types::WorkType;
use super::workers::{dispatch_to_ap_cores, scroll_worker};

// =============================================================================
// Double Buffer Structure
// =============================================================================

/// Double buffer state for tear-free rendering
///
/// In no_std environment, we track buffer state but actual buffer
/// allocation must be done by the caller using frame allocator.
pub struct DoubleBuffer {
    /// Front buffer (currently displayed)
    front_buffer: AtomicU64,
    /// Back buffer (being rendered to)
    back_buffer: AtomicU64,
    /// Buffer dimensions
    width: AtomicUsize,
    height: AtomicUsize,
    pitch: AtomicUsize,
    bpp: AtomicUsize,
    /// Swap pending flag
    swap_pending: AtomicBool,
    /// Generation counter for synchronization
    generation: AtomicU64,
}

impl DoubleBuffer {
    /// Create a new uninitialized double buffer state
    pub const fn new() -> Self {
        Self {
            front_buffer: AtomicU64::new(0),
            back_buffer: AtomicU64::new(0),
            width: AtomicUsize::new(0),
            height: AtomicUsize::new(0),
            pitch: AtomicUsize::new(0),
            bpp: AtomicUsize::new(0),
            swap_pending: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    /// Initialize double buffer with provided buffer addresses
    ///
    /// # Safety
    ///
    /// The caller must ensure both buffer addresses point to valid
    /// framebuffer memory of sufficient size.
    pub unsafe fn init(
        &self,
        front: u64,
        back: u64,
        width: usize,
        height: usize,
        pitch: usize,
        bpp: usize,
    ) {
        self.front_buffer.store(front, Ordering::Release);
        self.back_buffer.store(back, Ordering::Release);
        self.width.store(width, Ordering::Release);
        self.height.store(height, Ordering::Release);
        self.pitch.store(pitch, Ordering::Release);
        self.bpp.store(bpp, Ordering::Release);
        self.swap_pending.store(false, Ordering::Release);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Get the back buffer for rendering
    #[inline]
    pub fn back_buffer(&self) -> *mut u8 {
        self.back_buffer.load(Ordering::Acquire) as *mut u8
    }

    /// Get the front buffer (displayed)
    #[inline]
    pub fn front_buffer(&self) -> *const u8 {
        self.front_buffer.load(Ordering::Acquire) as *const u8
    }

    /// Get buffer dimensions
    #[inline]
    pub fn dimensions(&self) -> (usize, usize, usize, usize) {
        (
            self.width.load(Ordering::Acquire),
            self.height.load(Ordering::Acquire),
            self.pitch.load(Ordering::Acquire),
            self.bpp.load(Ordering::Acquire),
        )
    }

    /// Mark back buffer as ready to swap
    ///
    /// The actual swap should happen during vsync or at a safe point.
    pub fn request_swap(&self) {
        self.swap_pending.store(true, Ordering::Release);
    }

    /// Check if swap is pending
    #[inline]
    pub fn is_swap_pending(&self) -> bool {
        self.swap_pending.load(Ordering::Acquire)
    }

    /// Perform the buffer swap
    ///
    /// Returns true if swap was performed, false if no swap was pending.
    /// This should be called during vsync or when it's safe to switch buffers.
    pub fn swap(&self) -> bool {
        if !self
            .swap_pending
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return false;
        }

        // Swap the buffer pointers
        let front = self.front_buffer.load(Ordering::Acquire);
        let back = self.back_buffer.load(Ordering::Acquire);

        self.front_buffer.store(back, Ordering::Release);
        self.back_buffer.store(front, Ordering::Release);

        self.generation.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// Get current generation (increments on each swap)
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Check if double buffering is available
    #[inline]
    pub fn is_available(&self) -> bool {
        self.front_buffer.load(Ordering::Acquire) != 0
            && self.back_buffer.load(Ordering::Acquire) != 0
            && self.front_buffer.load(Ordering::Acquire) != self.back_buffer.load(Ordering::Acquire)
    }
}

// =============================================================================
// Global Instance
// =============================================================================

/// Global double buffer state
pub static DOUBLE_BUFFER: DoubleBuffer = DoubleBuffer::new();

// =============================================================================
// Operations
// =============================================================================

/// Copy back buffer to front buffer (for single-buffer fallback)
///
/// When true double buffering isn't available, this copies the
/// rendered content to the display buffer.
pub fn copy_to_front() {
    if !DOUBLE_BUFFER.is_available() {
        return;
    }

    let (width, height, pitch, bpp) = DOUBLE_BUFFER.dimensions();
    let total_bytes = height * pitch;

    if total_bytes == 0 {
        return;
    }

    let src = DOUBLE_BUFFER.back_buffer();
    let dst = DOUBLE_BUFFER.front_buffer() as *mut u8;

    // Use parallel copy for large framebuffers
    let workers = worker_count();
    if workers > 1 && total_bytes >= PARALLEL_SCROLL_THRESHOLD {
        // Setup parameters for parallel copy
        SCROLL_SRC_ADDR.store(src as u64, Ordering::Release);
        SCROLL_DST_ADDR.store(dst as u64, Ordering::Release);
        SCROLL_ROW_BYTES.store(width * bpp, Ordering::Release);
        SCROLL_PITCH.store(pitch, Ordering::Release);
        SCROLL_NEXT_ROW.store(0, Ordering::Release);
        SCROLL_TOTAL_ROWS.store(height, Ordering::Release);
        SCROLL_ROWS_DONE.store(0, Ordering::Release);
        SCROLL_DISTANCE.store(height, Ordering::Release); // Large distance = safe

        WORK_TYPE.store(WorkType::Scroll as u8, Ordering::Release);
        core::sync::atomic::fence(Ordering::SeqCst);

        dispatch_to_ap_cores();
        scroll_worker();

        // Wait for completion
        let mut backoff = 1u32;
        while SCROLL_ROWS_DONE.load(Ordering::Acquire) < height {
            for _ in 0..backoff {
                core::hint::spin_loop();
            }
            backoff = (backoff * 2).min(1024);
        }

        WORK_AVAILABLE.store(false, Ordering::Release);
        WORK_TYPE.store(WorkType::None as u8, Ordering::Release);
    } else {
        // Single-core copy
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, total_bytes);
        }
    }
}
