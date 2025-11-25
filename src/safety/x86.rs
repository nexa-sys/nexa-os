//! x86-64 specific unsafe operations.
//!
//! This module provides safe wrappers around architecture-specific operations
//! like port I/O, MSR access, and special CPU instructions.

use core::arch::asm;

// ============================================================================
// Port I/O Operations
// ============================================================================

/// Read a byte from an I/O port.
#[inline]
pub fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: Port I/O is always safe at the instruction level,
    // though the effect depends on the port being accessed.
    unsafe {
        asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Write a byte to an I/O port.
#[inline]
pub fn outb(port: u16, value: u8) {
    // SAFETY: Port I/O is always safe at the instruction level.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// Read a 16-bit word from an I/O port.
#[inline]
pub fn inw(port: u16) -> u16 {
    let value: u16;
    unsafe {
        asm!(
            "in ax, dx",
            out("ax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Write a 16-bit word to an I/O port.
#[inline]
pub fn outw(port: u16, value: u16) {
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// Read a 32-bit dword from an I/O port.
#[inline]
pub fn inl(port: u16) -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "in eax, dx",
            out("eax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Write a 32-bit dword to an I/O port.
#[inline]
pub fn outl(port: u16, value: u32) {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

// ============================================================================
// PCI Configuration Space Access
// ============================================================================

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Read from PCI configuration space.
///
/// # Arguments
/// * `bus` - PCI bus number (0-255)
/// * `device` - Device number (0-31)
/// * `function` - Function number (0-7)
/// * `offset` - Register offset (must be 4-byte aligned)
#[inline]
pub fn pci_config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32 & 0x1F) << 11)
        | ((function as u32 & 0x07) << 8)
        | ((offset as u32) & 0xFC);

    outl(PCI_CONFIG_ADDR, address);
    inl(PCI_CONFIG_DATA)
}

/// Write to PCI configuration space.
#[inline]
pub fn pci_config_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32 & 0x1F) << 11)
        | ((function as u32 & 0x07) << 8)
        | ((offset as u32) & 0xFC);

    outl(PCI_CONFIG_ADDR, address);
    outl(PCI_CONFIG_DATA, value);
}

// ============================================================================
// MSR (Model Specific Register) Operations
// ============================================================================

/// Read a Model Specific Register.
///
/// # Safety
/// Reading certain MSRs can have side effects or may not be available on all CPUs.
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack, preserves_flags)
    );
    ((high as u64) << 32) | (low as u64)
}

/// Write a Model Specific Register.
///
/// # Safety
/// Writing to MSRs can have significant system-wide effects.
#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack, preserves_flags)
    );
}

// Common MSR addresses
pub const MSR_IA32_STAR: u32 = 0xC0000081;
pub const MSR_IA32_LSTAR: u32 = 0xC0000082;
pub const MSR_IA32_FMASK: u32 = 0xC0000084;
pub const MSR_IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
pub const MSR_IA32_GS_BASE: u32 = 0xC0000101;
pub const MSR_IA32_FS_BASE: u32 = 0xC0000100;

// ============================================================================
// CPU Instructions
// ============================================================================

/// Read the Time Stamp Counter.
#[inline]
pub fn rdtsc() -> u64 {
    // SAFETY: RDTSC is always safe to execute
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Execute CPUID instruction.
///
/// Returns (eax, ebx, ecx, edx).
#[inline]
pub fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: CPUID is always safe to execute
    // Note: We must save and restore rbx since LLVM uses it internally
    unsafe {
        asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            ebx_out = out(reg) ebx,
            lateout("ecx") ecx,
            lateout("edx") edx,
            options(nomem, nostack, preserves_flags)
        );
    }
    (eax, ebx, ecx, edx)
}

/// Execute CPUID instruction with subleaf.
#[inline]
pub fn cpuid_count(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // Note: We must save and restore rbx since LLVM uses it internally
    unsafe {
        asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            ebx_out = out(reg) ebx,
            inout("ecx") subleaf => ecx,
            lateout("edx") edx,
            options(nomem, nostack, preserves_flags)
        );
    }
    (eax, ebx, ecx, edx)
}

/// Halt the CPU until the next interrupt.
#[inline]
pub fn hlt() {
    // SAFETY: HLT is safe, just waits for interrupt
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// Disable interrupts.
///
/// # Safety
/// Disabling interrupts for too long can cause system hangs.
#[inline]
pub unsafe fn cli() {
    asm!("cli", options(nomem, nostack, preserves_flags));
}

/// Enable interrupts.
///
/// # Safety
/// Must only be called when interrupt handlers are properly set up.
#[inline]
pub unsafe fn sti() {
    asm!("sti", options(nomem, nostack, preserves_flags));
}

/// Check if interrupts are enabled.
#[inline]
pub fn interrupts_enabled() -> bool {
    let flags: u64;
    // SAFETY: Reading flags is always safe
    unsafe {
        asm!(
            "pushfq",
            "pop {0}",
            out(reg) flags,
            options(nomem, preserves_flags)
        );
    }
    (flags & 0x200) != 0 // IF flag
}

/// Execute with interrupts disabled, then restore previous state.
///
/// # Safety
/// The closure must not enable interrupts or cause panics.
#[inline]
pub unsafe fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let enabled = interrupts_enabled();
    cli();
    let result = f();
    if enabled {
        sti();
    }
    result
}

// ============================================================================
// Memory Barriers
// ============================================================================

/// Memory fence (full barrier).
#[inline]
pub fn mfence() {
    // SAFETY: Memory fences are always safe
    unsafe {
        asm!("mfence", options(nostack, preserves_flags));
    }
}

/// Store fence.
#[inline]
pub fn sfence() {
    unsafe {
        asm!("sfence", options(nostack, preserves_flags));
    }
}

/// Load fence.
#[inline]
pub fn lfence() {
    unsafe {
        asm!("lfence", options(nostack, preserves_flags));
    }
}

/// Pause instruction (for spin loops).
#[inline]
pub fn pause() {
    // SAFETY: PAUSE is always safe
    unsafe {
        asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

// ============================================================================
// Control Register Access
// ============================================================================

/// Read CR3 (page table base register).
#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    // SAFETY: Reading CR3 is safe
    unsafe {
        asm!(
            "mov {0}, cr3",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Write CR3 (page table base register).
///
/// # Safety
/// Writing CR3 changes the active page table. The new value must point to
/// a valid page table structure, or the system will triple fault.
#[inline]
pub unsafe fn write_cr3(value: u64) {
    asm!(
        "mov cr3, {0}",
        in(reg) value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Invalidate a TLB entry for the given virtual address.
#[inline]
pub fn invlpg(addr: u64) {
    // SAFETY: INVLPG is safe to execute
    unsafe {
        asm!(
            "invlpg [{}]",
            in(reg) addr,
            options(nostack, preserves_flags)
        );
    }
}
