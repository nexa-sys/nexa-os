use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use x86_64::registers::model_specific::Msr;

const IA32_APIC_BASE: u32 = 0x1B;
const APIC_ENABLE: u64 = 1 << 11;
const APIC_BASE_MASK: u64 = 0xFFFFF000;
const DEFAULT_SPURIOUS_VECTOR: u8 = 0xFF;

const REG_ID: u32 = 0x20;
const REG_VERSION: u32 = 0x30;
const REG_TPR: u32 = 0x80;  // Task Priority Register
const REG_EOI: u32 = 0x0B0;
const REG_LDR: u32 = 0x0D0;  // Logical Destination Register
const REG_DFR: u32 = 0x0E0;  // Destination Format Register
const REG_SVR: u32 = 0x0F0;
const REG_ISR_BASE: u32 = 0x100;  // In-Service Register
const REG_TMR_BASE: u32 = 0x180;  // Trigger Mode Register
const REG_IRR_BASE: u32 = 0x200;  // Interrupt Request Register
const REG_ERROR: u32 = 0x280;
const REG_LVT_CMCI: u32 = 0x2F0;
const REG_ICR_LOW: u32 = 0x300;
const REG_ICR_HIGH: u32 = 0x310;
const REG_LVT_TIMER: u32 = 0x320;
const REG_LVT_THERMAL: u32 = 0x330;
const REG_LVT_PERF: u32 = 0x340;
const REG_LVT_LINT0: u32 = 0x350;
const REG_LVT_LINT1: u32 = 0x360;
const REG_LVT_ERROR: u32 = 0x370;
const REG_TIMER_INITIAL: u32 = 0x380;
const REG_TIMER_CURRENT: u32 = 0x390;
const REG_TIMER_DIVIDE: u32 = 0x3E0;

// Timer modes
const TIMER_MODE_ONESHOT: u32 = 0 << 17;
const TIMER_MODE_PERIODIC: u32 = 1 << 17;
const TIMER_MODE_TSC_DEADLINE: u32 = 2 << 17;

// Delivery modes for IPI
const DELIVERY_MODE_FIXED: u32 = 0 << 8;
const DELIVERY_MODE_INIT: u32 = 5 << 8;
const DELIVERY_MODE_STARTUP: u32 = 6 << 8;

static LAPIC_BASE: AtomicU64 = AtomicU64::new(0);
static LAPIC_READY: AtomicBool = AtomicBool::new(false);

pub fn init(lapic_base: u64) {
    LAPIC_BASE.store(lapic_base & APIC_BASE_MASK, Ordering::SeqCst);
    
    // Ensure LAPIC MMIO region is properly mapped
    // The LAPIC is at a fixed physical address and needs to be accessible
    unsafe {
        // Map 4KB for LAPIC registers
        match crate::paging::map_device_region(lapic_base & APIC_BASE_MASK, 4096) {
            Ok(virt_addr) => {
                crate::kinfo!("LAPIC: Mapped MMIO region at physical {:#x} -> virtual {:#x}", 
                    lapic_base & APIC_BASE_MASK, virt_addr as u64);
                // Update base to use the mapped virtual address
                LAPIC_BASE.store(virt_addr as u64, Ordering::SeqCst);
            }
            Err(e) => {
                crate::kwarn!("LAPIC: Failed to map MMIO region: {:?}, using identity mapping", e);
                // Fall back to assuming identity mapping works
            }
        }
    }
    
    enable_apic();
    LAPIC_READY.store(true, Ordering::SeqCst);
    crate::kinfo!(
        "LAPIC: Enabled local APIC at {:#x} (ID {:#x})",
        lapic_base,
        bsp_apic_id()
    );
}

pub fn base() -> Option<u64> {
    if !LAPIC_READY.load(Ordering::SeqCst) {
        return None;
    }
    Some(LAPIC_BASE.load(Ordering::SeqCst))
}

pub fn bsp_apic_id() -> u32 {
    unsafe { read_register(REG_ID) >> 24 }
}

/// Get current CPU's APIC ID
pub fn current_apic_id() -> u32 {
    if !LAPIC_READY.load(Ordering::Acquire) {
        return 0;
    }
    unsafe { read_register(REG_ID) >> 24 }
}

/// Send IPI with custom vector (public for SMP use)
pub fn send_ipi(apic_id: u32, vector: u8) {
    send_ipi_ex(apic_id, DELIVERY_MODE_FIXED | (vector as u32));
}

pub fn send_init_ipi(apic_id: u32) {
    send_ipi_ex(apic_id, DELIVERY_MODE_INIT | 0x4000);  // Level triggered
}

pub fn send_startup_ipi(apic_id: u32, vector: u8) {
    send_ipi_ex(apic_id, DELIVERY_MODE_STARTUP | (vector as u32));
}

pub fn send_eoi() {
    unsafe {
        write_register(REG_EOI, 0);
    }
}

fn send_ipi_ex(apic_id: u32, command: u32) {
    unsafe {
        // Simplified: don't wait, just send
        // This might be unsafe but lets us test if the IPI send itself works
        
        // Write destination APIC ID to ICR high register
        write_register(REG_ICR_HIGH, apic_id << 24);
        
        // Write command to ICR low register (triggers send)
        write_register(REG_ICR_LOW, command);
        
        // Small busy wait instead of register polling
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}

unsafe fn read_register(offset: u32) -> u32 {
    let base = LAPIC_BASE.load(Ordering::SeqCst);
    let ptr = (base + offset as u64) as *const u32;
    read_volatile(ptr)
}

unsafe fn write_register(offset: u32, value: u32) {
    let base = LAPIC_BASE.load(Ordering::SeqCst);
    let ptr = (base + offset as u64) as *mut u32;
    write_volatile(ptr, value);
}

fn enable_apic() {
    unsafe {
        let mut msr = Msr::new(IA32_APIC_BASE);
        let mut value = msr.read();
        
        // Ensure we're in xAPIC mode, not x2APIC
        // Bit 10 = x2APIC enable (should be 0 for xAPIC mode)
        value &= !(1 << 10);  // Clear x2APIC bit
        
        let base = LAPIC_BASE.load(Ordering::SeqCst);
        value &= !APIC_BASE_MASK;
        value |= base & APIC_BASE_MASK;
        value |= APIC_ENABLE;
        msr.write(value);

        let mut svr = read_register(REG_SVR);
        svr &= !0xFF;
        svr |= DEFAULT_SPURIOUS_VECTOR as u32;
        svr |= 1 << 8; // APIC software enable
        write_register(REG_SVR, svr);
        
        // Clear error status
        write_register(REG_ERROR, 0);
        write_register(REG_ERROR, 0);
        
        // Set task priority to accept all interrupts
        write_register(REG_TPR, 0);
        
        crate::kinfo!("LAPIC: xAPIC mode enabled, IA32_APIC_BASE={:#x}", value);
    }
}

/// Initialize LAPIC timer for periodic interrupts
pub fn init_timer(vector: u8, frequency_hz: u32) {
    unsafe {
        // Set divide value to 16
        write_register(REG_TIMER_DIVIDE, 0x3);
        
        // Set timer mode to periodic and vector
        let lvt = TIMER_MODE_PERIODIC | (vector as u32);
        write_register(REG_LVT_TIMER, lvt);
        
        // Set initial count (calibrated value based on frequency)
        // This is a simplified version - production should calibrate against known timer
        let initial_count = 10_000_000 / frequency_hz;  // Rough estimate
        write_register(REG_TIMER_INITIAL, initial_count);
    }
}

/// Stop LAPIC timer
pub fn stop_timer() {
    unsafe {
        write_register(REG_LVT_TIMER, 1 << 16);  // Mask timer
        write_register(REG_TIMER_INITIAL, 0);
    }
}

/// Read LAPIC error status
pub fn read_error() -> u32 {
    unsafe {
        write_register(REG_ERROR, 0);
        read_register(REG_ERROR)
    }
}
