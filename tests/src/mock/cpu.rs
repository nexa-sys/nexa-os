//! Virtual CPU Emulation
//!
//! This module emulates x86-64 CPU state and instructions that the kernel
//! relies on. We don't need to emulate full x86 instruction set - just the
//! special instructions the kernel uses:
//!
//! - Control registers (CR0, CR2, CR3, CR4)
//! - MSRs (Model Specific Registers)
//! - CPUID results
//! - Interrupt state
//! - Special instructions (RDTSC, RDMSR, WRMSR, HLT, PAUSE, etc.)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// x86-64 general purpose registers
#[derive(Debug, Clone, Default)]
pub struct Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

/// CPU control registers
#[derive(Debug, Clone)]
pub struct ControlRegisters {
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
}

impl Default for ControlRegisters {
    fn default() -> Self {
        Self {
            cr0: 0x8000_0011, // PE + ET + PG enabled
            cr2: 0,
            cr3: 0, // Will be set when paging is initialized
            cr4: 0x20, // PAE enabled
        }
    }
}

/// RFLAGS register bits
pub mod rflags {
    pub const CF: u64 = 1 << 0;   // Carry flag
    pub const PF: u64 = 1 << 2;   // Parity flag
    pub const AF: u64 = 1 << 4;   // Auxiliary carry flag
    pub const ZF: u64 = 1 << 6;   // Zero flag
    pub const SF: u64 = 1 << 7;   // Sign flag
    pub const TF: u64 = 1 << 8;   // Trap flag
    pub const IF: u64 = 1 << 9;   // Interrupt enable flag
    pub const DF: u64 = 1 << 10;  // Direction flag
    pub const OF: u64 = 1 << 11;  // Overflow flag
    pub const IOPL: u64 = 3 << 12; // I/O privilege level
    pub const NT: u64 = 1 << 14;   // Nested task flag
    pub const RF: u64 = 1 << 16;   // Resume flag
    pub const VM: u64 = 1 << 17;   // Virtual 8086 mode
    pub const AC: u64 = 1 << 18;   // Alignment check
    pub const VIF: u64 = 1 << 19;  // Virtual interrupt flag
    pub const VIP: u64 = 1 << 20;  // Virtual interrupt pending
    pub const ID: u64 = 1 << 21;   // ID flag (CPUID support)
}

/// MSR addresses used by the kernel
pub mod msr {
    pub const IA32_APIC_BASE: u32 = 0x1B;
    pub const IA32_MTRRCAP: u32 = 0xFE;
    pub const IA32_SYSENTER_CS: u32 = 0x174;
    pub const IA32_SYSENTER_ESP: u32 = 0x175;
    pub const IA32_SYSENTER_EIP: u32 = 0x176;
    pub const IA32_PAT: u32 = 0x277;
    pub const IA32_PERF_STATUS: u32 = 0x198;
    pub const IA32_EFER: u32 = 0xC0000080;
    pub const IA32_STAR: u32 = 0xC0000081;
    pub const IA32_LSTAR: u32 = 0xC0000082;
    pub const IA32_CSTAR: u32 = 0xC0000083;
    pub const IA32_FMASK: u32 = 0xC0000084;
    pub const IA32_FS_BASE: u32 = 0xC0000100;
    pub const IA32_GS_BASE: u32 = 0xC0000101;
    pub const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
    pub const IA32_TSC_AUX: u32 = 0xC0000103;
}

/// CPUID feature flags
#[derive(Debug, Clone)]
pub struct CpuidFeatures {
    pub vendor: [u8; 12],
    pub brand: [u8; 48],
    pub max_basic_leaf: u32,
    pub max_extended_leaf: u32,
    
    // Feature bits (ECX:EDX for leaf 1)
    pub features_ecx: u32,
    pub features_edx: u32,
    
    // Extended features (ECX:EDX for leaf 0x80000001)
    pub ext_features_ecx: u32,
    pub ext_features_edx: u32,
    
    // Structured extended features (EBX:ECX for leaf 7)
    pub struct_ext_ebx: u32,
    pub struct_ext_ecx: u32,
}

impl Default for CpuidFeatures {
    fn default() -> Self {
        // Default to a reasonable modern CPU
        Self {
            vendor: *b"NexaOSEmula\0",  // 12 bytes: CPUID vendor string
            brand: *b"NexaOS Virtual CPU @ 3.0GHz\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            max_basic_leaf: 0x16,
            max_extended_leaf: 0x8000001F,
            // SSE, SSE2, SSE3, SSSE3, SSE4.1, SSE4.2, POPCNT, AVX, XSAVE
            features_ecx: 0x7FFAFBBF,
            // FPU, VME, DE, PSE, TSC, MSR, PAE, MCE, CX8, APIC, SEP, MTRR, PGE, MCA, CMOV, PAT, PSE36, CLFSH, MMX, FXSR, SSE, SSE2
            features_edx: 0xBFEBFBFF,
            // LAHF/SAHF, ABM, SSE4A, 3DNowPrefetch, RDTSCP, LM, 3DNow!+, 3DNow!
            ext_features_ecx: 0x00000001,
            ext_features_edx: 0x2C100800,
            // FSGSBASE, TSC_ADJUST, BMI1, AVX2, SMEP, BMI2, ERMS, INVPCID, AVX512F
            struct_ext_ebx: 0x029C67AF,
            struct_ext_ecx: 0x00000000,
        }
    }
}

/// Full CPU state
#[derive(Debug, Clone)]
pub struct CpuState {
    pub regs: Registers,
    pub cr: ControlRegisters,
    pub msrs: HashMap<u32, u64>,
    pub interrupts_enabled: bool,
    pub nmi_pending: bool,
    pub halted: bool,
    pub cpuid: CpuidFeatures,
}

impl Default for CpuState {
    fn default() -> Self {
        let mut msrs = HashMap::new();
        // Initialize important MSRs
        msrs.insert(msr::IA32_EFER, 0x501); // LME + LMA + SCE
        msrs.insert(msr::IA32_APIC_BASE, 0xFEE0_0900); // APIC enabled, BSP
        msrs.insert(msr::IA32_PAT, 0x0007040600070406); // Default PAT
        
        Self {
            regs: Registers::default(),
            cr: ControlRegisters::default(),
            msrs,
            interrupts_enabled: false,
            nmi_pending: false,
            halted: false,
            cpuid: CpuidFeatures::default(),
        }
    }
}

/// Virtual CPU emulation
pub struct VirtualCpu {
    /// CPU ID (for SMP support)
    pub id: u32,
    /// CPU state (protected by lock for thread safety)
    state: RwLock<CpuState>,
    /// Time stamp counter (atomic for performance)
    tsc: AtomicU64,
    /// Cycle counter for TSC advancement
    cycle_count: AtomicU64,
}

impl VirtualCpu {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            state: RwLock::new(CpuState::default()),
            tsc: AtomicU64::new(0),
            cycle_count: AtomicU64::new(0),
        }
    }
    
    /// Create bootstrap processor (BSP) 
    pub fn new_bsp() -> Self {
        Self::new(0)
    }
    
    /// Create application processor (AP)
    pub fn new_ap(id: u32) -> Self {
        assert!(id > 0, "AP ID must be > 0");
        let vcpu = Self::new(id);
        // AP starts halted
        vcpu.state.write().unwrap().halted = true;
        vcpu
    }
    
    // ========================================================================
    // Control Register Operations
    // ========================================================================
    
    pub fn read_cr0(&self) -> u64 {
        self.state.read().unwrap().cr.cr0
    }
    
    pub fn write_cr0(&self, value: u64) {
        self.state.write().unwrap().cr.cr0 = value;
    }
    
    pub fn read_cr2(&self) -> u64 {
        self.state.read().unwrap().cr.cr2
    }
    
    pub fn write_cr2(&self, value: u64) {
        self.state.write().unwrap().cr.cr2 = value;
    }
    
    pub fn read_cr3(&self) -> u64 {
        self.state.read().unwrap().cr.cr3
    }
    
    pub fn write_cr3(&self, value: u64) {
        self.state.write().unwrap().cr.cr3 = value;
        // CR3 write invalidates TLB - could track this if needed
    }
    
    pub fn read_cr4(&self) -> u64 {
        self.state.read().unwrap().cr.cr4
    }
    
    pub fn write_cr4(&self, value: u64) {
        self.state.write().unwrap().cr.cr4 = value;
    }
    
    // ========================================================================
    // MSR Operations
    // ========================================================================
    
    pub fn read_msr(&self, msr: u32) -> u64 {
        self.state.read().unwrap().msrs.get(&msr).copied().unwrap_or(0)
    }
    
    pub fn write_msr(&self, msr: u32, value: u64) {
        self.state.write().unwrap().msrs.insert(msr, value);
    }
    
    // ========================================================================
    // CPUID
    // ========================================================================
    
    pub fn cpuid(&self, leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
        let state = self.state.read().unwrap();
        let cpuid = &state.cpuid;
        
        match leaf {
            0 => {
                // Vendor string
                let vendor = &cpuid.vendor;
                (
                    cpuid.max_basic_leaf,
                    u32::from_le_bytes([vendor[0], vendor[1], vendor[2], vendor[3]]),
                    u32::from_le_bytes([vendor[8], vendor[9], vendor[10], vendor[11]]),
                    u32::from_le_bytes([vendor[4], vendor[5], vendor[6], vendor[7]]),
                )
            }
            1 => {
                // Feature information
                let family = 6u32;
                let model = 15u32;
                let stepping = 1u32;
                let signature = (family << 8) | (model << 4) | stepping;
                (
                    signature,
                    (self.id << 24) | 0x00100800, // EBX: APIC ID, CLFLUSH size, max logical CPUs
                    cpuid.features_ecx,
                    cpuid.features_edx,
                )
            }
            7 if subleaf == 0 => {
                // Structured extended features
                (0, cpuid.struct_ext_ebx, cpuid.struct_ext_ecx, 0)
            }
            0x80000000 => {
                // Extended CPUID max leaf
                (cpuid.max_extended_leaf, 0, 0, 0)
            }
            0x80000001 => {
                // Extended features
                (0, 0, cpuid.ext_features_ecx, cpuid.ext_features_edx)
            }
            0x80000002..=0x80000004 => {
                // Processor brand string
                let offset = ((leaf - 0x80000002) * 16) as usize;
                let brand = &cpuid.brand[offset..offset + 16];
                (
                    u32::from_le_bytes([brand[0], brand[1], brand[2], brand[3]]),
                    u32::from_le_bytes([brand[4], brand[5], brand[6], brand[7]]),
                    u32::from_le_bytes([brand[8], brand[9], brand[10], brand[11]]),
                    u32::from_le_bytes([brand[12], brand[13], brand[14], brand[15]]),
                )
            }
            _ => (0, 0, 0, 0),
        }
    }
    
    // ========================================================================
    // Time Stamp Counter
    // ========================================================================
    
    pub fn rdtsc(&self) -> u64 {
        // Advance TSC on each read to simulate time passing
        self.advance_cycles(100);
        self.tsc.load(Ordering::SeqCst)
    }
    
    pub fn advance_cycles(&self, cycles: u64) {
        self.cycle_count.fetch_add(cycles, Ordering::SeqCst);
        self.tsc.fetch_add(cycles, Ordering::SeqCst);
    }
    
    pub fn set_tsc(&self, value: u64) {
        self.tsc.store(value, Ordering::SeqCst);
    }
    
    // ========================================================================
    // Interrupt State
    // ========================================================================
    
    pub fn interrupts_enabled(&self) -> bool {
        self.state.read().unwrap().interrupts_enabled
    }
    
    pub fn enable_interrupts(&self) {
        let mut state = self.state.write().unwrap();
        state.interrupts_enabled = true;
        state.regs.rflags |= rflags::IF;
    }
    
    pub fn disable_interrupts(&self) {
        let mut state = self.state.write().unwrap();
        state.interrupts_enabled = false;
        state.regs.rflags &= !rflags::IF;
    }
    
    pub fn is_halted(&self) -> bool {
        self.state.read().unwrap().halted
    }
    
    pub fn halt(&self) {
        self.state.write().unwrap().halted = true;
    }
    
    pub fn wake(&self) {
        self.state.write().unwrap().halted = false;
    }
    
    // ========================================================================
    // Stack Operations (for RSP tracking)
    // ========================================================================
    
    pub fn read_rsp(&self) -> u64 {
        self.state.read().unwrap().regs.rsp
    }
    
    pub fn write_rsp(&self, value: u64) {
        self.state.write().unwrap().regs.rsp = value;
    }
    
    // ========================================================================
    // Full State Access (for debugging/assertions)
    // ========================================================================
    
    pub fn get_state(&self) -> CpuState {
        self.state.read().unwrap().clone()
    }
    
    pub fn set_state(&self, state: CpuState) {
        *self.state.write().unwrap() = state;
    }
}

impl Default for VirtualCpu {
    fn default() -> Self {
        Self::new_bsp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vcpu_basic() {
        let vcpu = VirtualCpu::new_bsp();
        assert_eq!(vcpu.id, 0);
        assert!(!vcpu.is_halted());
    }
    
    #[test]
    fn test_vcpu_cr3() {
        let vcpu = VirtualCpu::new_bsp();
        vcpu.write_cr3(0x1000);
        assert_eq!(vcpu.read_cr3(), 0x1000);
    }
    
    #[test]
    fn test_vcpu_msr() {
        let vcpu = VirtualCpu::new_bsp();
        vcpu.write_msr(msr::IA32_LSTAR, 0xFFFF_8000_0000_1000);
        assert_eq!(vcpu.read_msr(msr::IA32_LSTAR), 0xFFFF_8000_0000_1000);
    }
    
    #[test]
    fn test_vcpu_cpuid_vendor() {
        let vcpu = VirtualCpu::new_bsp();
        let (eax, ebx, ecx, edx) = vcpu.cpuid(0, 0);
        assert!(eax >= 0x16); // Max basic leaf
    }
    
    #[test]
    fn test_vcpu_tsc_advances() {
        let vcpu = VirtualCpu::new_bsp();
        let tsc1 = vcpu.rdtsc();
        let tsc2 = vcpu.rdtsc();
        assert!(tsc2 > tsc1);
    }
    
    #[test]
    fn test_vcpu_interrupts() {
        let vcpu = VirtualCpu::new_bsp();
        assert!(!vcpu.interrupts_enabled());
        vcpu.enable_interrupts();
        assert!(vcpu.interrupts_enabled());
        vcpu.disable_interrupts();
        assert!(!vcpu.interrupts_enabled());
    }
}
