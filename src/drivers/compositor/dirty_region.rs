//! Dirty region tracking for incremental updates
//!
//! This module provides efficient tracking of screen regions that need
//! to be redrawn, enabling partial screen updates for better performance.

use super::config::MAX_DIRTY_REGIONS;
use super::types::CompositionRegion;

// =============================================================================
// Dirty Region Tracker
// =============================================================================

/// Dirty region tracker for incremental updates
pub struct DirtyRegionTracker {
    /// Array of dirty regions
    regions: [CompositionRegion; MAX_DIRTY_REGIONS],
    /// Number of active dirty regions
    count: usize,
    /// Flag indicating full repaint needed
    full_repaint: bool,
}

impl DirtyRegionTracker {
    /// Create a new dirty region tracker
    pub const fn new() -> Self {
        Self {
            regions: [CompositionRegion::new(0, 0, 0, 0); MAX_DIRTY_REGIONS],
            count: 0,
            full_repaint: false,
        }
    }
    
    /// Mark a region as dirty
    pub fn mark_dirty(&mut self, region: CompositionRegion) {
        if self.full_repaint || !region.is_valid() {
            return;
        }
        
        // Try to merge with existing region
        for i in 0..self.count {
            if self.regions[i].intersects(&region) {
                // Merge: expand existing region to include new one
                let existing = &mut self.regions[i];
                let new_x = existing.x.min(region.x);
                let new_y = existing.y.min(region.y);
                let new_right = (existing.x + existing.width).max(region.x + region.width);
                let new_bottom = (existing.y + existing.height).max(region.y + region.height);
                existing.x = new_x;
                existing.y = new_y;
                existing.width = new_right - new_x;
                existing.height = new_bottom - new_y;
                return;
            }
        }
        
        // Add new region if space available
        if self.count < MAX_DIRTY_REGIONS {
            self.regions[self.count] = region;
            self.count += 1;
        } else {
            // Too many regions - fall back to full repaint
            self.full_repaint = true;
        }
    }
    
    /// Mark entire screen as dirty
    pub fn mark_full_repaint(&mut self) {
        self.full_repaint = true;
    }
    
    /// Check if full repaint is needed
    pub fn needs_full_repaint(&self) -> bool {
        self.full_repaint
    }
    
    /// Get dirty regions for rendering
    pub fn get_dirty_regions(&self) -> &[CompositionRegion] {
        if self.full_repaint {
            &[] // Caller should handle full repaint separately
        } else {
            &self.regions[..self.count]
        }
    }
    
    /// Clear all dirty regions after rendering
    pub fn clear(&mut self) {
        self.count = 0;
        self.full_repaint = false;
    }
    
    /// Check if any regions are dirty
    pub fn is_dirty(&self) -> bool {
        self.full_repaint || self.count > 0
    }
    
    /// Get the number of tracked dirty regions
    pub fn region_count(&self) -> usize {
        self.count
    }
    
    /// Get the total area of all dirty regions (for statistics)
    pub fn total_dirty_area(&self) -> u64 {
        if self.full_repaint {
            return u64::MAX; // Indicate full repaint
        }
        
        let mut total = 0u64;
        for i in 0..self.count {
            total += self.regions[i].area();
        }
        total
    }
}

impl Default for DirtyRegionTracker {
    fn default() -> Self {
        Self::new()
    }
}
