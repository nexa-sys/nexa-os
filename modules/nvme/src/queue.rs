//! NVMe Queue Management
//!
//! Implements Submission Queue (SQ) and Completion Queue (CQ) handling.

use crate::cmd::{NvmeCmd, NvmeCqe};
use crate::regs::*;
use crate::{kmod_zalloc, kmod_dealloc, kmod_virt_to_phys, kmod_fence};
use crate::kmod_mmio_write32;
use core::ptr;
use core::sync::atomic::{AtomicU16, Ordering};

// =============================================================================
// Queue Pair
// =============================================================================

/// NVMe Queue Pair (Submission + Completion queues)
#[repr(C)]
pub struct NvmeQueuePair {
    /// Queue ID (0 = Admin, 1+ = I/O)
    pub qid: u16,
    /// Queue depth (number of entries)
    pub depth: u16,
    /// Doorbell stride (4 << dstrd)
    pub dstrd: u8,

    /// Submission Queue base (virtual)
    pub sq_base: *mut NvmeCmd,
    /// Submission Queue physical address
    pub sq_phys: u64,
    /// Submission Queue tail (where to write next command)
    pub sq_tail: AtomicU16,
    /// Submission Queue doorbell register address
    pub sq_db: u64,

    /// Completion Queue base (virtual)
    pub cq_base: *mut NvmeCqe,
    /// Completion Queue physical address
    pub cq_phys: u64,
    /// Completion Queue head (where to read next completion)
    pub cq_head: AtomicU16,
    /// Completion Queue doorbell register address
    pub cq_db: u64,
    /// Current phase tag (toggles on wrap)
    pub cq_phase: bool,

    /// Command ID counter
    pub cid_counter: AtomicU16,

    /// Lock for queue access
    pub lock: u64,
}

impl NvmeQueuePair {
    /// Allocate and initialize a new queue pair
    pub fn new(qid: u16, depth: u16, bar0: u64, dstrd: u8) -> Option<Self> {
        // Allocate Submission Queue (64 bytes per entry, 4KB aligned)
        let sq_size = (depth as usize) * SQE_SIZE;
        let sq_base = unsafe { kmod_zalloc(sq_size, 4096) } as *mut NvmeCmd;
        if sq_base.is_null() {
            return None;
        }
        let sq_phys = unsafe { kmod_virt_to_phys(sq_base as u64) };

        // Allocate Completion Queue (16 bytes per entry, 4KB aligned)
        let cq_size = (depth as usize) * CQE_SIZE;
        let cq_base = unsafe { kmod_zalloc(cq_size, 4096) } as *mut NvmeCqe;
        if cq_base.is_null() {
            unsafe { kmod_dealloc(sq_base as *mut u8, sq_size, 4096); }
            return None;
        }
        let cq_phys = unsafe { kmod_virt_to_phys(cq_base as u64) };

        // Calculate doorbell addresses
        let sq_db = sq_tail_doorbell(qid, dstrd) + bar0;
        let cq_db = cq_head_doorbell(qid, dstrd) + bar0;

        Some(Self {
            qid,
            depth,
            dstrd,
            sq_base,
            sq_phys,
            sq_tail: AtomicU16::new(0),
            sq_db,
            cq_base,
            cq_phys,
            cq_head: AtomicU16::new(0),
            cq_db,
            cq_phase: true,
            cid_counter: AtomicU16::new(0),
            lock: 0,
        })
    }

    /// Allocate the next command ID
    #[inline]
    pub fn alloc_cid(&self) -> u16 {
        let cid = self.cid_counter.fetch_add(1, Ordering::Relaxed);
        cid % self.depth
    }

    /// Get number of free slots in submission queue
    #[inline]
    pub fn sq_free_slots(&self) -> u16 {
        // SQ is empty when tail == head
        // SQ is full when (tail + 1) % depth == head
        // We need to track head via CQ completions
        // For simplicity, we just use depth - 1 as max in-flight
        self.depth - 1
    }

    /// Submit a command to the submission queue
    pub fn submit(&self, cmd: &NvmeCmd) -> u16 {
        let tail = self.sq_tail.load(Ordering::Acquire);
        
        // Write command to SQ
        unsafe {
            ptr::write_volatile(self.sq_base.add(tail as usize), *cmd);
            kmod_fence();
        }

        // Update tail
        let new_tail = (tail + 1) % self.depth;
        self.sq_tail.store(new_tail, Ordering::Release);

        // Ring doorbell
        unsafe {
            kmod_mmio_write32(self.sq_db, new_tail as u32);
        }

        // Return command ID (from cdw0)
        (cmd.cdw0 >> 16) as u16
    }

    /// Check for a completion entry (non-blocking)
    /// Returns None if no completion is ready
    pub fn poll_completion(&mut self) -> Option<NvmeCqe> {
        let head = self.cq_head.load(Ordering::Acquire);
        
        // Read completion entry
        let cqe = unsafe { ptr::read_volatile(self.cq_base.add(head as usize)) };
        
        // Check phase tag
        if cqe.phase() != self.cq_phase {
            return None;
        }

        // Advance head
        let new_head = (head + 1) % self.depth;
        if new_head == 0 {
            // Wrapped around, toggle phase
            self.cq_phase = !self.cq_phase;
        }
        self.cq_head.store(new_head, Ordering::Release);

        // Ring completion queue doorbell
        unsafe {
            kmod_mmio_write32(self.cq_db, new_head as u32);
        }

        Some(cqe)
    }

    /// Wait for a specific command to complete
    /// Returns the completion status or error
    pub fn wait_for_completion(&mut self, cid: u16) -> Result<NvmeCqe, i32> {
        for _ in 0..TIMEOUT_LOOPS {
            if let Some(cqe) = self.poll_completion() {
                if cqe.cid == cid {
                    if cqe.is_success() {
                        return Ok(cqe);
                    } else {
                        return Err(-(((cqe.sct() as i32) << 8) | (cqe.sc() as i32)));
                    }
                }
                // Not our command, but we need to handle it
                // In a real driver, we'd queue these for later processing
            }
            core::hint::spin_loop();
        }
        Err(-1) // Timeout
    }

    /// Submit a command and wait for completion
    pub fn submit_and_wait(&mut self, cmd: &NvmeCmd) -> Result<NvmeCqe, i32> {
        let cid = (cmd.cdw0 >> 16) as u16;
        self.submit(cmd);
        self.wait_for_completion(cid)
    }

    /// Cleanup queue pair memory
    pub fn cleanup(&mut self) {
        let sq_size = (self.depth as usize) * SQE_SIZE;
        let cq_size = (self.depth as usize) * CQE_SIZE;
        
        if !self.sq_base.is_null() {
            unsafe { kmod_dealloc(self.sq_base as *mut u8, sq_size, 4096); }
            self.sq_base = ptr::null_mut();
        }
        if !self.cq_base.is_null() {
            unsafe { kmod_dealloc(self.cq_base as *mut u8, cq_size, 4096); }
            self.cq_base = ptr::null_mut();
        }
    }
}

impl Drop for NvmeQueuePair {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// =============================================================================
// Admin Queue (Special handling for queue 0)
// =============================================================================

/// Create admin queue pair - uses fixed addresses in ASQ/ACQ registers
pub fn create_admin_queue(depth: u16, bar0: u64, dstrd: u8) -> Option<NvmeQueuePair> {
    NvmeQueuePair::new(0, depth, bar0, dstrd)
}

/// Setup admin queue registers (call before enabling controller)
pub fn setup_admin_queue_regs(qp: &NvmeQueuePair, bar0: u64) {
    // Set Admin Queue Attributes
    let aqa = aqa_value(qp.depth, qp.depth);
    unsafe {
        kmod_mmio_write32(bar0 + REG_AQA, aqa);
        
        // Set Admin Submission Queue Base Address (64-bit)
        kmod_mmio_write32(bar0 + REG_ASQ, qp.sq_phys as u32);
        kmod_mmio_write32(bar0 + REG_ASQ + 4, (qp.sq_phys >> 32) as u32);
        
        // Set Admin Completion Queue Base Address (64-bit)
        kmod_mmio_write32(bar0 + REG_ACQ, qp.cq_phys as u32);
        kmod_mmio_write32(bar0 + REG_ACQ + 4, (qp.cq_phys >> 32) as u32);
    }
}
