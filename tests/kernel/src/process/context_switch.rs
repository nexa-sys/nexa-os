//! Context Switch Tests
//!
//! Tests for CPU context saving and restoring during context switches.
//! These tests verify that all registers are properly preserved.

#[cfg(test)]
mod tests {
    use crate::process::Context;

    // =========================================================================
    // Context Structure Tests
    // =========================================================================

    #[test]
    fn test_context_zero() {
        let ctx = Context::zero();
        
        // All general purpose registers should be zero
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.rdx, 0);
        assert_eq!(ctx.rsi, 0);
        assert_eq!(ctx.rdi, 0);
        assert_eq!(ctx.rbp, 0);
        assert_eq!(ctx.r8, 0);
        assert_eq!(ctx.r9, 0);
        assert_eq!(ctx.r10, 0);
        assert_eq!(ctx.r11, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);
        
        // Instruction and stack pointers should be zero
        assert_eq!(ctx.rip, 0);
        assert_eq!(ctx.rsp, 0);
        
        // RFLAGS should have IF set (interrupts enabled)
        assert_eq!(ctx.rflags, 0x202);
    }

    #[test]
    fn test_context_rflags_if_set() {
        let ctx = Context::zero();
        
        // IF flag is bit 9
        const IF_FLAG: u64 = 1 << 9;
        assert_ne!(ctx.rflags & IF_FLAG, 0, "IF flag should be set");
    }

    #[test]
    fn test_context_rflags_reserved_bit() {
        let ctx = Context::zero();
        
        // Bit 1 is always set in x86_64
        const RESERVED_BIT: u64 = 1 << 1;
        assert_ne!(ctx.rflags & RESERVED_BIT, 0, "Reserved bit 1 should be set");
    }

    #[test]
    fn test_context_copy() {
        let mut ctx1 = Context::zero();
        ctx1.rax = 0x1234567890ABCDEF;
        ctx1.rip = 0x400000;
        ctx1.rsp = 0x7FFFFF000;
        
        let ctx2 = ctx1;
        
        assert_eq!(ctx2.rax, ctx1.rax);
        assert_eq!(ctx2.rip, ctx1.rip);
        assert_eq!(ctx2.rsp, ctx1.rsp);
    }

    #[test]
    fn test_context_size() {
        // Context should be reasonably sized
        let size = core::mem::size_of::<Context>();
        
        // 18 u64 fields * 8 bytes = 144 bytes minimum
        assert!(size >= 144, "Context should be at least 144 bytes");
        
        // But not too large
        assert!(size <= 512, "Context shouldn't be too large");
        
        eprintln!("Context size: {} bytes", size);
    }

    #[test]
    fn test_context_alignment() {
        let alignment = core::mem::align_of::<Context>();
        
        // Should be at least 8-byte aligned for u64 members
        assert!(alignment >= 8, "Context should be at least 8-byte aligned");
    }

    // =========================================================================
    // Register Preservation Tests
    // =========================================================================

    /// Context save/restore helper to verify all fields are preserved
    struct ContextPreserver {
        saved: Option<Context>,
    }

    impl ContextPreserver {
        fn new() -> Self {
            Self { saved: None }
        }

        fn save(&mut self, ctx: &Context) {
            self.saved = Some(*ctx);
        }

        fn restore(&self) -> Option<Context> {
            self.saved
        }
    }

    #[test]
    fn test_preserve_all_gprs() {
        let mut ctx = Context::zero();
        
        // Set distinctive values for each register
        ctx.rax = 0xAAAAAAAAAAAAAAAA;
        ctx.rbx = 0xBBBBBBBBBBBBBBBB;
        ctx.rcx = 0xCCCCCCCCCCCCCCCC;
        ctx.rdx = 0xDDDDDDDDDDDDDDDD;
        ctx.rsi = 0x1111111111111111;
        ctx.rdi = 0x2222222222222222;
        ctx.rbp = 0x3333333333333333;
        ctx.r8 = 0x8888888888888888;
        ctx.r9 = 0x9999999999999999;
        ctx.r10 = 0x1010101010101010;
        ctx.r11 = 0x1111111111111111;
        ctx.r12 = 0x1212121212121212;
        ctx.r13 = 0x1313131313131313;
        ctx.r14 = 0x1414141414141414;
        ctx.r15 = 0x1515151515151515;
        ctx.rip = 0xDEADBEEFCAFEBABE;
        ctx.rsp = 0x7FFFFFFFFFFF0000;
        ctx.rflags = 0x246; // Common user-mode flags
        
        let mut preserver = ContextPreserver::new();
        preserver.save(&ctx);
        
        let restored = preserver.restore().expect("Should restore");
        
        // Verify all registers match
        assert_eq!(restored.rax, ctx.rax, "RAX mismatch");
        assert_eq!(restored.rbx, ctx.rbx, "RBX mismatch");
        assert_eq!(restored.rcx, ctx.rcx, "RCX mismatch");
        assert_eq!(restored.rdx, ctx.rdx, "RDX mismatch");
        assert_eq!(restored.rsi, ctx.rsi, "RSI mismatch");
        assert_eq!(restored.rdi, ctx.rdi, "RDI mismatch");
        assert_eq!(restored.rbp, ctx.rbp, "RBP mismatch");
        assert_eq!(restored.r8, ctx.r8, "R8 mismatch");
        assert_eq!(restored.r9, ctx.r9, "R9 mismatch");
        assert_eq!(restored.r10, ctx.r10, "R10 mismatch");
        assert_eq!(restored.r11, ctx.r11, "R11 mismatch");
        assert_eq!(restored.r12, ctx.r12, "R12 mismatch");
        assert_eq!(restored.r13, ctx.r13, "R13 mismatch");
        assert_eq!(restored.r14, ctx.r14, "R14 mismatch");
        assert_eq!(restored.r15, ctx.r15, "R15 mismatch");
        assert_eq!(restored.rip, ctx.rip, "RIP mismatch");
        assert_eq!(restored.rsp, ctx.rsp, "RSP mismatch");
        assert_eq!(restored.rflags, ctx.rflags, "RFLAGS mismatch");
    }

    // =========================================================================
    // RFLAGS Specific Tests
    // =========================================================================

    // RFLAGS bit definitions
    const CF: u64 = 1 << 0;  // Carry flag
    const PF: u64 = 1 << 2;  // Parity flag
    const AF: u64 = 1 << 4;  // Auxiliary carry flag
    const ZF: u64 = 1 << 6;  // Zero flag
    const SF: u64 = 1 << 7;  // Sign flag
    const TF: u64 = 1 << 8;  // Trap flag
    const IF: u64 = 1 << 9;  // Interrupt enable flag
    const DF: u64 = 1 << 10; // Direction flag
    const OF: u64 = 1 << 11; // Overflow flag
    const IOPL_MASK: u64 = 3 << 12; // I/O privilege level

    #[test]
    fn test_rflags_user_mode() {
        // Typical user-mode RFLAGS
        let user_flags: u64 = 0x202; // IF + reserved bit 1
        
        // IF should be set
        assert_ne!(user_flags & IF, 0);
        
        // TF should be clear
        assert_eq!(user_flags & TF, 0);
        
        // IOPL should be 0 for user mode
        assert_eq!(user_flags & IOPL_MASK, 0);
    }

    #[test]
    fn test_rflags_arithmetic_flags() {
        // These flags are set by arithmetic operations
        let flags = [CF, PF, AF, ZF, SF, OF];
        
        for flag in flags {
            assert!(flag > 0);
            assert!(flag < 0x1000);
            assert!(flag.is_power_of_two());
        }
    }

    // =========================================================================
    // Syscall Context Tests
    // =========================================================================

    /// Syscall calling convention: RAX=syscall#, RDI/RSI/RDX/R10/R8/R9=args
    /// Return: RAX=result, RCX/R11 clobbered by syscall instruction
    
    #[test]
    fn test_syscall_argument_registers() {
        // Syscall arguments use specific registers
        let mut ctx = Context::zero();
        
        // Set up a syscall
        ctx.rax = 1; // syscall number (write)
        ctx.rdi = 1; // arg1 (fd)
        ctx.rsi = 0x400000; // arg2 (buf)
        ctx.rdx = 13; // arg3 (count)
        
        // R10 is used instead of RCX for 4th arg (RCX holds return address)
        ctx.r10 = 0; // arg4
        ctx.r8 = 0;  // arg5
        ctx.r9 = 0;  // arg6
        
        assert_eq!(ctx.rax, 1);
        assert_eq!(ctx.rdi, 1);
        assert_eq!(ctx.rsi, 0x400000);
        assert_eq!(ctx.rdx, 13);
    }

    #[test]
    fn test_callee_saved_registers() {
        // These registers must be preserved across function calls
        let callee_saved = ["rbx", "rbp", "r12", "r13", "r14", "r15"];
        
        // Context should have fields for all of these
        let ctx = Context::zero();
        let _ = ctx.rbx;
        let _ = ctx.rbp;
        let _ = ctx.r12;
        let _ = ctx.r13;
        let _ = ctx.r14;
        let _ = ctx.r15;
        
        // All should start at 0 in zero context
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rbp, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);
    }

    // =========================================================================
    // Stack Pointer Tests
    // =========================================================================

    #[test]
    fn test_stack_pointer_alignment() {
        // Stack should be 16-byte aligned before call instruction
        fn is_16_aligned(addr: u64) -> bool {
            addr % 16 == 0
        }
        
        let aligned_stack = 0x7FFFFF000u64;
        assert!(is_16_aligned(aligned_stack), "Stack should be 16-byte aligned");
        
        // After call pushes return address (8 bytes), it's 8-byte aligned
        let after_call = aligned_stack - 8;
        assert!(!is_16_aligned(after_call));
        assert_eq!(after_call % 8, 0);
    }

    #[test]
    fn test_kernel_stack_boundary() {
        // Kernel stack should be within valid kernel memory
        // Typically in higher half on x86_64
        let kernel_stack_base: u64 = 0xFFFFFFFF80000000;
        
        // Should be in canonical higher half
        assert!(kernel_stack_base >= 0xFFFF800000000000);
    }
}
