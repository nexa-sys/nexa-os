use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use x86_64::registers::model_specific::Msr;

const IA32_APIC_BASE: u32 = 0x1B;
const APIC_ENABLE: u64 = 1 << 11;
const APIC_BASE_MASK: u64 = 0xFFFFF000;
const DEFAULT_SPURIOUS_VECTOR: u8 = 0xFF;

const REG_ID: u32 = 0x20;
const REG_EOI: u32 = 0x0B0;
const REG_SVR: u32 = 0x0F0;
const REG_ICR_LOW: u32 = 0x300;
const REG_ICR_HIGH: u32 = 0x310;

static LAPIC_BASE: AtomicU64 = AtomicU64::new(0);
static LAPIC_READY: AtomicBool = AtomicBool::new(false);

pub fn init(lapic_base: u64) {
    LAPIC_BASE.store(lapic_base & APIC_BASE_MASK, Ordering::SeqCst);
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

pub fn send_init_ipi(apic_id: u32) {
    send_ipi(apic_id, 0x4500);
}

pub fn send_startup_ipi(apic_id: u32, vector: u8) {
    send_ipi(apic_id, 0x4600 | (vector as u32));
}

pub fn send_eoi() {
    unsafe {
        write_register(REG_EOI, 0);
    }
}

fn send_ipi(apic_id: u32, command: u32) {
    unsafe {
        wait_for_icr();
        write_register(REG_ICR_HIGH, apic_id << 24);
        write_register(REG_ICR_LOW, command);
        wait_for_icr();
    }
}

unsafe fn wait_for_icr() {
    while (read_register(REG_ICR_LOW) & (1 << 12)) != 0 {}
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
    }
}
