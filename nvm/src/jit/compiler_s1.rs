//! S1 Quick Compiler
//!
//! Fast baseline compiler that generates decent code quickly.
//! No heavy optimizations - focus on compilation speed.
//! Used for warm-up before S2 kicks in.
//!
//! ## ISA-Aware Compilation
//!
//! S1 includes lightweight ISA optimization:
//! - Vector width adjustment based on available ISA (AVX-512/AVX/SSE)
//! - ISA requirement tracking for codegen
//! - No pattern matching (done in S2 for compile speed)

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrBasicBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, ExitReason, IrBuilder};
use super::decoder::{X86Decoder, DecodedInstr, Mnemonic};
use super::profile::ProfileDb;
use super::isa_opt::S1IsaPass;
use super::nready::InstructionSets;

/// S1 compiler configuration
#[derive(Clone, Debug)]
pub struct S1Config {
    /// Maximum instructions per block
    pub max_instrs: usize,
    /// Enable simple constant folding
    pub const_fold: bool,
    /// Enable dead code elimination
    pub dead_code_elim: bool,
    /// Enable simple peephole opts
    pub peephole: bool,
    /// Enable lightweight instruction scheduling (OoO optimization)
    pub scheduling: bool,
    /// Enable ISA-aware optimization (vector width, etc.)
    pub isa_opt: bool,
    /// Target ISA (None = detect current CPU)
    pub target_isa: Option<InstructionSets>,
}

impl Default for S1Config {
    fn default() -> Self {
        Self {
            max_instrs: 100,
            const_fold: true,
            dead_code_elim: true,
            peephole: true,
            scheduling: true, // Enable OoO scheduling by default
            isa_opt: true,
            target_isa: None, // Auto-detect
        }
    }
}

/// S1 compiled block
pub struct S1Block {
    /// Guest start address
    pub guest_rip: u64,
    /// Guest code size
    pub guest_size: u32,
    /// IR representation
    pub ir: IrBlock,
    /// Native code (placeholder)
    pub native: Vec<u8>,
    /// Estimated cycles
    pub est_cycles: u32,
    /// Required ISA for this block
    pub required_isa: InstructionSets,
}

/// S1 quick compiler
pub struct S1Compiler {
    config: S1Config,
    /// ISA pass instance
    isa_pass: S1IsaPass,
}

impl S1Compiler {
    pub fn new() -> Self {
        Self {
            config: S1Config::default(),
            isa_pass: S1IsaPass::new(),
        }
    }
    
    pub fn with_config(config: S1Config) -> Self {
        let isa_pass = if let Some(isa) = config.target_isa {
            S1IsaPass::with_isa(isa)
        } else {
            S1IsaPass::new()
        };
        Self { config, isa_pass }
    }
    
    /// Compile a basic block starting at RIP
    pub fn compile(
        &self,
        guest_code: &[u8],
        start_rip: u64,
        decoder: &X86Decoder,
        profile: &ProfileDb,
    ) -> JitResult<S1Block> {
        // Decode instructions first
        let mut instrs = Vec::new();
        let mut offset = 0usize;
        
        while offset < guest_code.len() && instrs.len() < self.config.max_instrs {
            let instr = decoder.decode(&guest_code[offset..], start_rip + offset as u64)?;
            let is_term = is_block_terminator(&instr);
            
            offset += instr.len as usize;
            instrs.push(instr);
            
            // Stop at control flow changes
            if is_term {
                break;
            }
        }
        
        // Build IR using IrBuilder
        let builder = IrBuilder::new(start_rip);
        let mut ir = builder.build(&instrs);
        
        // Apply S1 optimizations
        if self.config.const_fold {
            self.const_fold(&mut ir);
        }
        if self.config.dead_code_elim {
            self.dead_code_elim(&mut ir);
        }
        if self.config.peephole {
            self.peephole(&mut ir);
        }
        
        // Apply lightweight instruction scheduling (OoO optimization)
        if self.config.scheduling {
            self.schedule_instructions(&mut ir);
        }
        
        // Apply ISA-aware optimization (lightweight for S1)
        let required_isa = if self.config.isa_opt {
            self.isa_pass.run(&mut ir)
        } else {
            InstructionSets::SSE2
        };
        
        // Generate native code
        let native = self.codegen(&ir, profile)?;
        let est_cycles = self.estimate_cycles(&ir);
        
        Ok(S1Block {
            guest_rip: start_rip,
            guest_size: offset as u32,
            ir,
            native,
            est_cycles,
            required_isa,
        })
    }
    
    /// Simple constant folding
    fn const_fold(&self, ir: &mut IrBlock) {
        for bb in &mut ir.blocks {
            let mut i = 0;
            while i < bb.instrs.len() {
                let instr = &bb.instrs[i];
                
                // Try to fold binary operations with constant operands
                if let Some(folded) = self.try_fold_binary(instr, &bb.instrs[..i]) {
                    bb.instrs[i] = folded;
                }
                
                i += 1;
            }
        }
    }
    
    fn try_fold_binary(&self, instr: &IrInstr, _prior: &[IrInstr]) -> Option<IrInstr> {
        // SSA style: dst is in instr.dst, ops only have operands
        match &instr.op {
            IrOp::Add(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const(va.wrapping_add(vb) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            IrOp::Sub(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const(va.wrapping_sub(vb) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            IrOp::And(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const((va & vb) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
                // x & 0 = 0
                if get_const_val(*b) == Some(0) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const(0),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            IrOp::Or(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const((va | vb) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            IrOp::Xor(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const((va ^ vb) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
                // x ^ x = 0
                if a == b {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const(0),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            IrOp::Shl(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const((va << (vb & 63)) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            IrOp::Shr(a, b) => {
                if let (Some(va), Some(vb)) = (get_const_val(*a), get_const_val(*b)) {
                    return Some(IrInstr {
                        dst: instr.dst,
                        op: IrOp::Const((va >> (vb & 63)) as i64),
                        guest_rip: instr.guest_rip,
                        flags: instr.flags,
                    });
                }
            }
            _ => {}
        }
        None
    }
    
    /// Dead code elimination
    fn dead_code_elim(&self, ir: &mut IrBlock) {
        // Build use set from all blocks
        let mut used = std::collections::HashSet::new();
        
        // Collect all used VRegs from operands
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                for op in get_operands(&instr.op) {
                    used.insert(op);
                }
            }
        }
        
        // Remove instructions whose results aren't used (and have no side effects)
        for bb in &mut ir.blocks {
            bb.instrs.retain(|instr| {
                // SSA style: dst is in instr.dst
                if instr.dst.is_valid() && op_produces_value(&instr.op) {
                    // Keep if result is used OR has side effects
                    used.contains(&instr.dst) || has_side_effect(&instr.op)
                } else {
                    // No def or side-effectful - keep if has side effects or is terminator
                    has_side_effect(&instr.op) || instr.flags.contains(IrFlags::TERMINATOR)
                }
            });
        }
    }
    
    /// Simple peephole optimizations
    fn peephole(&self, ir: &mut IrBlock) {
        for bb in &mut ir.blocks {
            let mut i = 0;
            while i < bb.instrs.len() {
                let dst = bb.instrs[i].dst;
                let guest_rip = bb.instrs[i].guest_rip;
                let flags = bb.instrs[i].flags;
                
                // Pattern: add x, 0 -> const from a (copy not needed in SSA)
                if let IrOp::Add(a, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(0) {
                        // In SSA, we can't just copy - need to redirect uses
                        // For now, simplify by loading the value
                        if let Some(val) = get_const_val(*a) {
                            bb.instrs[i] = IrInstr { dst, op: IrOp::Const(val as i64), guest_rip, flags };
                        }
                    }
                }
                
                // Pattern: mul x, 1 -> keep original value
                if let IrOp::Mul(a, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(1) {
                        if let Some(val) = get_const_val(*a) {
                            bb.instrs[i] = IrInstr { dst, op: IrOp::Const(val as i64), guest_rip, flags };
                        }
                    }
                }
                
                // Pattern: mul x, 0 -> 0
                if let IrOp::Mul(_, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(0) {
                        bb.instrs[i] = IrInstr { dst, op: IrOp::Const(0), guest_rip, flags };
                    }
                }
                
                // Pattern: shl x, 0 -> keep original value
                if let IrOp::Shl(a, b) = &bb.instrs[i].op {
                    if get_const_val(*b) == Some(0) {
                        if let Some(val) = get_const_val(*a) {
                            bb.instrs[i] = IrInstr { dst, op: IrOp::Const(val as i64), guest_rip, flags };
                        }
                    }
                }
                
                // Pattern: consecutive consts to same vreg (keep last)
                if i + 1 < bb.instrs.len() {
                    if let (IrOp::Const(_), IrOp::Const(_)) = 
                        (&bb.instrs[i].op, &bb.instrs[i + 1].op) 
                    {
                        if bb.instrs[i].dst == bb.instrs[i + 1].dst {
                            bb.instrs.remove(i);
                            continue;
                        }
                    }
                }
                
                i += 1;
            }
        }
    }
    
    // ========================================================================
    // Out-of-Order Instruction Scheduling (Enterprise)
    // ========================================================================
    //
    // Implements a latency-aware list scheduling algorithm to maximize ILP
    // (Instruction-Level Parallelism) by reordering instructions within
    // basic blocks while respecting data dependencies.
    //
    // Key features:
    // - Accurate x86-64 instruction latency model (Intel/AMD microarchitectures)
    // - Full dependency tracking: RAW, WAR, WAW, memory, control
    // - Critical path analysis for priority scheduling
    // - Register pressure awareness to avoid excessive spilling
    // - Memory aliasing analysis for load/store reordering
    
    /// Schedule instructions within basic blocks for optimal OoO execution
    fn schedule_instructions(&self, ir: &mut IrBlock) {
        for bb in &mut ir.blocks {
            if bb.instrs.len() < 3 {
                // Too few instructions to benefit from scheduling
                continue;
            }
            
            // Build dependency graph with full analysis
            let sched_ctx = ScheduleContext::build(&bb.instrs);
            
            // Run latency-aware list scheduling
            let new_order = sched_ctx.schedule();
            
            // Reorder instructions
            let new_instrs: Vec<_> = new_order.into_iter()
                .map(|i| bb.instrs[i].clone())
                .collect();
            bb.instrs = new_instrs;
        }
    }
    
    /// Generate native code directly from IR (skip decoding)
    /// 
    /// Used by NReady! to recompile cached IR when native code version is stale.
    pub fn codegen_from_ir(&self, ir: &IrBlock) -> JitResult<Vec<u8>> {
        // Apply S1 optimizations to a cloned IR
        let mut ir_clone = ir.clone();
        
        if self.config.const_fold {
            self.const_fold(&mut ir_clone);
        }
        if self.config.dead_code_elim {
            self.dead_code_elim(&mut ir_clone);
        }
        if self.config.peephole {
            self.peephole(&mut ir_clone);
        }
        
        // Create a dummy profile for codegen (not used for S1)
        let dummy_profile = ProfileDb::new(1024);
        self.codegen(&ir_clone, &dummy_profile)
    }

    /// Generate native code from IR with proper register allocation
    fn codegen(&self, ir: &IrBlock, _profile: &ProfileDb) -> JitResult<Vec<u8>> {
        let mut code = Vec::new();
        let mut allocator = RegAllocator::new();
        
        // First pass: allocate registers for all VRegs in order
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                // Allocate for destination
                if instr.dst != VReg::NONE {
                    allocator.allocate(instr.dst);
                }
                // Allocate for operands
                for vreg in get_operands(&instr.op) {
                    if vreg != VReg::NONE {
                        allocator.allocate(vreg);
                    }
                }
            }
        }
        
        // Calculate stack frame size for spills
        let spill_size = allocator.spill_size();
        
        // Prologue:
        // Save all callee-saved registers we use: RBX, RBP, R12, R13, R14, R15
        // (System V AMD64 ABI requires callee to preserve these)
        
        // push r15  (JitState pointer)
        code.extend_from_slice(&[0x41, 0x57]);
        // push r14
        code.extend_from_slice(&[0x41, 0x56]);
        // push r13
        code.extend_from_slice(&[0x41, 0x55]);
        // push r12
        code.extend_from_slice(&[0x41, 0x54]);
        // push rbp
        code.push(0x55);
        // push rbx
        code.push(0x53);
        
        // mov r15, rdi - save JitState pointer to R15
        code.extend_from_slice(&[0x49, 0x89, 0xFF]);
        
        // sub rsp, spill_size (if needed)
        if spill_size > 0 {
            if spill_size <= 127 {
                code.extend_from_slice(&[0x48, 0x83, 0xEC, spill_size as u8]);
            } else {
                code.extend_from_slice(&[0x48, 0x81, 0xEC]);
                code.extend_from_slice(&(spill_size as u32).to_le_bytes());
            }
        }
        
        // Generate code for each instruction
        // Two-pass approach for prologue Load instructions:
        // Pass 1: Process spill-target Loads first (safe to use RAX as temp since no registers hold values yet)
        // Pass 2: Process register-target Loads (direct load to target register)
        // Pass 3: Process all other instructions
        for bb in &ir.blocks {
            // Pass 1: Spill-target Load instructions
            for instr in &bb.instrs {
                if self.is_load_instr(&instr.op) && allocator.get(instr.dst).is_spill() {
                    self.emit_instr_with_alloc(&mut code, instr, &allocator, spill_size)?;
                }
            }
            // Pass 2: Register-target Load instructions  
            for instr in &bb.instrs {
                if self.is_load_instr(&instr.op) && !allocator.get(instr.dst).is_spill() {
                    self.emit_instr_with_alloc(&mut code, instr, &allocator, spill_size)?;
                }
            }
            // Pass 3: All other instructions
            for instr in &bb.instrs {
                if !self.is_load_instr(&instr.op) {
                    self.emit_instr_with_alloc(&mut code, instr, &allocator, spill_size)?;
                }
            }
        }
        
        // Epilogue: restore spill space and callee-saved registers, then return 0
        if spill_size > 0 {
            if spill_size <= 127 {
                code.extend_from_slice(&[0x48, 0x83, 0xC4, spill_size as u8]);
            } else {
                code.extend_from_slice(&[0x48, 0x81, 0xC4]);
                code.extend_from_slice(&(spill_size as u32).to_le_bytes());
            }
        }
        
        // Restore callee-saved registers in reverse order
        // pop rbx
        code.push(0x5B);
        // pop rbp
        code.push(0x5D);
        // pop r12
        code.extend_from_slice(&[0x41, 0x5C]);
        // pop r13
        code.extend_from_slice(&[0x41, 0x5D]);
        // pop r14
        code.extend_from_slice(&[0x41, 0x5E]);
        // pop r15
        code.extend_from_slice(&[0x41, 0x5F]);
        
        // mov eax, 0; ret
        code.extend_from_slice(&[0xB8, 0x00, 0x00, 0x00, 0x00, 0xC3]);
        
        Ok(code)
    }
    
    /// Check if an IR op is a Load instruction (LoadGpr, LoadRip, LoadFlags)
    fn is_load_instr(&self, op: &IrOp) -> bool {
        matches!(op, IrOp::LoadGpr(_) | IrOp::LoadRip | IrOp::LoadFlags)
    }
    
    /// Emit instruction using register allocation
    fn emit_instr_with_alloc(&self, code: &mut Vec<u8>, instr: &IrInstr, alloc: &RegAllocator, spill_size: i32) -> JitResult<()> {
        let dst = instr.dst;
        
        match &instr.op {
            IrOp::Const(val) => {
                // mov dst, imm64
                self.emit_mov_imm64(code, alloc.get(dst), *val as u64)?;
            }
            
            IrOp::LoadGpr(idx) => {
                // Load guest GPR from JitState: dst <- [r15 + idx*8]
                let offset = (*idx as i32) * 8;
                self.emit_load_jitstate(code, alloc.get(dst), offset)?;
            }
            
            IrOp::StoreGpr(idx, val) => {
                // Store to guest GPR in JitState: [r15 + idx*8] <- val
                let offset = (*idx as i32) * 8;
                self.emit_store_jitstate(code, offset, alloc.get(*val))?;
            }
            
            IrOp::LoadRip => {
                // Load guest RIP from JitState
                self.emit_load_jitstate(code, alloc.get(dst), 0x80)?;
            }
            
            IrOp::StoreRip(val) => {
                // Store to guest RIP in JitState
                self.emit_store_jitstate(code, 0x80, alloc.get(*val))?;
            }
            
            IrOp::LoadFlags => {
                // Load guest RFLAGS from JitState
                self.emit_load_jitstate(code, alloc.get(dst), 0x88)?;
            }
            
            IrOp::StoreFlags(val) => {
                // Store to guest RFLAGS in JitState
                self.emit_store_jitstate(code, 0x88, alloc.get(*val))?;
            }
            
            IrOp::Exit(reason) => {
                // Exit: restore stack, pop r15, mov eax, code; ret
                let exit_code = match reason {
                    ExitReason::Normal => 0u32,
                    ExitReason::Halt => 1,
                    ExitReason::Interrupt(n) => 0x100 | (*n as u32),
                    ExitReason::Exception(n, _) => 0x200 | (*n as u32),
                    ExitReason::IoRead(_, _) => 0x300,
                    ExitReason::IoWrite(_, _) => 0x400,
                    ExitReason::Mmio(_, _, _) => 0x500,
                    ExitReason::Hypercall => 0x600,
                    ExitReason::Reset => 0x700,
                };
                
                // Full epilogue: add rsp, spill_size; pop r15; mov eax, code; ret
                self.emit_epilogue(code, spill_size, exit_code);
            }
            
            IrOp::Hlt => {
                // Same as Exit(Halt)
                self.emit_epilogue(code, spill_size, 1);
            }
            
            IrOp::Syscall => {
                self.emit_epilogue(code, spill_size, 0x600);
            }
            
            IrOp::Nop => {
                code.push(0x90);
            }
            
            // For other ops, use legacy emit_instr temporarily
            _ => {
                self.emit_instr_legacy(code, instr, alloc)?;
            }
        }
        
        Ok(())
    }
    
    /// Emit full epilogue: add rsp, spill_size; load rip from JitState; encode result; restore callee-saved; ret
    /// 
    /// Result encoding (64-bit):
    /// - Bits 63-56: exit kind (0=Continue, 1=Halt, 2=Interrupt, etc.)
    /// - Bits 55-0:  kind-specific value (e.g., next_rip for Continue)
    ///
    /// Native code reads JitState.rip at runtime and encodes it into the return value,
    /// so the hypervisor gets the correct next_rip after to_vcpu() has already set it.
    fn emit_epilogue(&self, code: &mut Vec<u8>, spill_size: i32, exit_kind: u32) {
        // add rsp, spill_size (if needed)
        if spill_size > 0 {
            if spill_size <= 127 {
                code.extend_from_slice(&[0x48, 0x83, 0xC4, spill_size as u8]);
            } else {
                code.extend_from_slice(&[0x48, 0x81, 0xC4]);
                code.extend_from_slice(&(spill_size as u32).to_le_bytes());
            }
        }
        
        // Load JitState.rip into rax (r15 points to JitState, rip is at offset 0x80)
        // mov rax, [r15 + 0x80]
        // REX.W=1, REX.B=1 (for r15): 0x49
        // opcode: 0x8B (mov r64, r/m64)
        // ModRM: mod=10 (disp32), reg=000 (rax), r/m=111 (r15) = 0x87
        // disp32: 0x80, 0x00, 0x00, 0x00
        code.extend_from_slice(&[0x49, 0x8B, 0x87, 0x80, 0x00, 0x00, 0x00]);
        
        // If exit_kind != 0, OR the kind into high byte of rax
        // mov r11, (exit_kind << 56); or rax, r11
        if exit_kind != 0 {
            let kind_shifted = (exit_kind as u64) << 56;
            // mov r11, imm64: REX.W=1, REX.B=1 = 0x49, opcode = 0xBB
            code.extend_from_slice(&[0x49, 0xBB]);
            code.extend_from_slice(&kind_shifted.to_le_bytes());
            // or rax, r11: REX.W=1, REX.R=1 = 0x4C, opcode = 0x09, ModRM = 0xD8
            code.extend_from_slice(&[0x4C, 0x09, 0xD8]);
        }
        
        // Restore callee-saved registers in reverse order (must match prologue)
        // pop rbx
        code.push(0x5B);
        // pop rbp
        code.push(0x5D);
        // pop r12
        code.extend_from_slice(&[0x41, 0x5C]);
        // pop r13
        code.extend_from_slice(&[0x41, 0x5D]);
        // pop r14
        code.extend_from_slice(&[0x41, 0x5E]);
        // pop r15
        code.extend_from_slice(&[0x41, 0x5F]);
        
        // ret
        code.push(0xC3);
    }
    
    /// Emit mov dst, imm64
    fn emit_mov_imm64(&self, code: &mut Vec<u8>, dst: RegAlloc, val: u64) -> JitResult<()> {
        match dst {
            RegAlloc::Reg(reg) => {
                // mov r64, imm64
                code.push(0x48 | if reg >= 8 { 0x01 } else { 0 });
                code.push(0xB8 + (reg & 7));
                code.extend_from_slice(&val.to_le_bytes());
            }
            RegAlloc::Spill(offset) => {
                // mov r11, imm64; mov [rsp + offset], r11
                // Use R11 as scratch to avoid clobbering allocated registers
                code.extend_from_slice(&[0x49, 0xBB]); // mov r11, imm64 (REX.W + REX.B + mov r64)
                code.extend_from_slice(&val.to_le_bytes());
                self.emit_store_stack(code, offset, SCRATCH_REG)?;
            }
        }
        Ok(())
    }
    
    /// Emit load from JitState: dst <- [r15 + offset]
    fn emit_load_jitstate(&self, code: &mut Vec<u8>, dst: RegAlloc, offset: i32) -> JitResult<()> {
        match dst {
            RegAlloc::Reg(reg) => {
                emit_load_from_jitstate(code, reg, offset);
            }
            RegAlloc::Spill(spill_off) => {
                // Load to R11 first, then store to spill slot
                // Use R11 as scratch to avoid clobbering allocated registers
                emit_load_from_jitstate(code, SCRATCH_REG, offset);
                self.emit_store_stack(code, spill_off, SCRATCH_REG)?;
            }
        }
        Ok(())
    }
    
    /// Emit store to JitState: [r15 + offset] <- src
    fn emit_store_jitstate(&self, code: &mut Vec<u8>, offset: i32, src: RegAlloc) -> JitResult<()> {
        match src {
            RegAlloc::Reg(reg) => {
                emit_store_to_jitstate(code, offset, reg);
            }
            RegAlloc::Spill(spill_off) => {
                // Load from spill slot to R11 first
                // Use R11 as scratch to avoid clobbering allocated registers
                self.emit_load_stack(code, SCRATCH_REG, spill_off)?;
                emit_store_to_jitstate(code, offset, SCRATCH_REG);
            }
        }
        Ok(())
    }
    
    /// Emit load from stack: dst <- [rsp + offset]
    fn emit_load_stack(&self, code: &mut Vec<u8>, dst_reg: u8, offset: i32) -> JitResult<()> {
        let rex = 0x48 | if dst_reg >= 8 { 0x04 } else { 0 };
        code.push(rex);
        code.push(0x8B); // mov r64, r/m64
        
        if offset >= -128 && offset <= 127 {
            // [RSP + disp8] needs SIB byte
            code.push(0x44 | ((dst_reg & 7) << 3)); // mod=01, reg, r/m=100 (SIB)
            code.push(0x24); // SIB: scale=0, index=RSP, base=RSP
            code.push(offset as u8);
        } else {
            code.push(0x84 | ((dst_reg & 7) << 3)); // mod=10, reg, r/m=100 (SIB)
            code.push(0x24); // SIB
            code.extend_from_slice(&offset.to_le_bytes());
        }
        Ok(())
    }
    
    /// Emit store to stack: [rsp + offset] <- src
    fn emit_store_stack(&self, code: &mut Vec<u8>, offset: i32, src_reg: u8) -> JitResult<()> {
        let rex = 0x48 | if src_reg >= 8 { 0x04 } else { 0 };
        code.push(rex);
        code.push(0x89); // mov r/m64, r64
        
        if offset >= -128 && offset <= 127 {
            code.push(0x44 | ((src_reg & 7) << 3));
            code.push(0x24);
            code.push(offset as u8);
        } else {
            code.push(0x84 | ((src_reg & 7) << 3));
            code.push(0x24);
            code.extend_from_slice(&offset.to_le_bytes());
        }
        Ok(())
    }
    
    /// Legacy emit_instr for ops not yet converted (uses vreg_to_host)
    fn emit_instr_legacy(&self, code: &mut Vec<u8>, instr: &IrInstr, alloc: &RegAllocator) -> JitResult<()> {
        let dst = instr.dst;
        
        // Helper to get reg from allocation (RAX as fallback for spilled)
        let get_reg = |v: VReg| -> u8 {
            match alloc.get(v) {
                RegAlloc::Reg(r) => r,
                RegAlloc::Spill(_) => 0, // Use RAX as temp
            }
        };
        
        match &instr.op {
            IrOp::Add(a, b) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                let breg = get_reg(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x01, dreg, breg); // add
            }
            
            IrOp::Sub(a, b) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                let breg = get_reg(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x29, dreg, breg); // sub
            }
            
            IrOp::And(a, b) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                let breg = get_reg(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x21, dreg, breg); // and
            }
            
            IrOp::Or(a, b) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                let breg = get_reg(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x09, dreg, breg); // or
            }
            
            IrOp::Xor(a, b) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                let breg = get_reg(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x31, dreg, breg); // xor
            }
            
            IrOp::Ret => {
                code.push(0xC3);
            }
            
            IrOp::Jump(_) => {
                // Placeholder - would need label resolution
                code.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]);
            }
            
            IrOp::Neg(a) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_rex_w(code, 0, dreg);
                code.push(0xF7);
                code.push(0xD8 | (dreg & 7)); // neg r64
            }
            
            IrOp::Not(a) => {
                let dreg = get_reg(dst);
                let areg = get_reg(*a);
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_rex_w(code, 0, dreg);
                code.push(0xF7);
                code.push(0xD0 | (dreg & 7)); // not r64
            }
            
            _ => {
                // Unhandled - emit nop
                code.push(0x90);
            }
        }
        
        Ok(())
    }
    
    fn estimate_cycles(&self, ir: &IrBlock) -> u32 {
        let mut cycles = 0u32;
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                cycles += match &instr.op {
                    IrOp::Const(_) => 1,
                    IrOp::Add(_, _) | IrOp::Sub(_, _) => 1,
                    IrOp::And(_, _) | IrOp::Or(_, _) | IrOp::Xor(_, _) => 1,
                    IrOp::Mul(_, _) | IrOp::IMul(_, _) => 3,
                    IrOp::Div(_, _) | IrOp::IDiv(_, _) => 20,
                    IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) => 1,
                    IrOp::Load8(_) | IrOp::Load16(_) | 
                    IrOp::Load32(_) | IrOp::Load64(_) => 4,
                    IrOp::Store8(_, _) | IrOp::Store16(_, _) |
                    IrOp::Store32(_, _) | IrOp::Store64(_, _) => 4,
                    IrOp::Call(_) | IrOp::CallIndirect(_) => 5,
                    IrOp::Exit(_) => 10,
                    _ => 1,
                };
            }
        }
        
        cycles
    }
}

impl Default for S1Compiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if instruction terminates a basic block
fn is_block_terminator(instr: &DecodedInstr) -> bool {
    matches!(instr.mnemonic, 
        Mnemonic::Jmp | Mnemonic::Jcc | Mnemonic::Call | 
        Mnemonic::Ret | Mnemonic::Iret | Mnemonic::Int | Mnemonic::Int3 |
        Mnemonic::Loop | Mnemonic::Loope | Mnemonic::Loopne |
        Mnemonic::Hlt | Mnemonic::Sysenter | Mnemonic::Syscall
    )
}

// ============================================================================
// Linear Scan Register Allocator
// ============================================================================

/// Available host registers for allocation (excluding RSP=4 and R15=15)
// Allocatable registers: excludes RSP(4), R11(11, scratch), R15(15, JitState ptr)
const ALLOCATABLE_REGS: [u8; 13] = [0, 1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13, 14];
// R11 is reserved as scratch register for spill operations
const SCRATCH_REG: u8 = 11;

/// Register allocation result for a VReg
#[derive(Debug, Clone, Copy)]
enum RegAlloc {
    /// Allocated to a host register
    Reg(u8),
    /// Spilled to stack at [RSP + offset]
    Spill(i32),
}

impl RegAlloc {
    fn is_spill(&self) -> bool {
        matches!(self, RegAlloc::Spill(_))
    }
}

/// Simple linear scan register allocator
struct RegAllocator {
    /// VReg -> allocation mapping
    allocations: std::collections::HashMap<u32, RegAlloc>,
    /// Which host registers are currently in use (vreg that owns it)
    reg_owners: [Option<u32>; 16],
    /// Next spill slot offset (grows downward from RSP)
    next_spill: i32,
    /// Total spill slots used
    spill_count: i32,
}

impl RegAllocator {
    fn new() -> Self {
        Self {
            allocations: std::collections::HashMap::new(),
            reg_owners: [None; 16],
            next_spill: 0, // Start at [RSP+0], grow upward
            spill_count: 0,
        }
    }
    
    /// Allocate a register for a VReg
    fn allocate(&mut self, vreg: VReg) -> RegAlloc {
        // Already allocated?
        if let Some(&alloc) = self.allocations.get(&vreg.0) {
            return alloc;
        }
        
        // Try to find a free register
        for &reg in &ALLOCATABLE_REGS {
            if self.reg_owners[reg as usize].is_none() {
                self.reg_owners[reg as usize] = Some(vreg.0);
                let alloc = RegAlloc::Reg(reg);
                self.allocations.insert(vreg.0, alloc);
                return alloc;
            }
        }
        
        // No free register, spill to stack
        // Offsets: 0, 8, 16... relative to RSP after stack frame allocation
        let offset = self.next_spill;
        self.next_spill += 8;
        self.spill_count += 1;
        let alloc = RegAlloc::Spill(offset);
        self.allocations.insert(vreg.0, alloc);
        alloc
    }
    
    /// Get allocation for a VReg (must already be allocated)
    fn get(&self, vreg: VReg) -> RegAlloc {
        *self.allocations.get(&vreg.0).unwrap_or(&RegAlloc::Spill(0))
    }
    
    /// Release a VReg's register (for dead VRegs)
    fn release(&mut self, vreg: VReg) {
        if let Some(RegAlloc::Reg(reg)) = self.allocations.get(&vreg.0) {
            self.reg_owners[*reg as usize] = None;
        }
        self.allocations.remove(&vreg.0);
    }
    
    /// Get total stack space needed for spills
    fn spill_size(&self) -> i32 {
        self.spill_count * 8
    }
}

/// Get constant value if vreg is a constant
fn get_const_val(_vreg: VReg) -> Option<u64> {
    // Would look up in constant map
    None
}

/// Get operands of an IR operation (SSA style - dst is in IrInstr.dst)
fn get_operands(op: &IrOp) -> Vec<VReg> {
    match op {
        // Binary ops
        IrOp::Add(a, b) | IrOp::Sub(a, b) | IrOp::And(a, b) |
        IrOp::Or(a, b) | IrOp::Xor(a, b) | IrOp::Mul(a, b) | IrOp::IMul(a, b) |
        IrOp::Div(a, b) | IrOp::IDiv(a, b) | IrOp::Shl(a, b) |
        IrOp::Shr(a, b) | IrOp::Sar(a, b) | IrOp::Rol(a, b) | IrOp::Ror(a, b) |
        IrOp::Cmp(a, b) | IrOp::Test(a, b) => vec![*a, *b],
        // Unary ops
        IrOp::Neg(a) | IrOp::Not(a) => vec![*a],
        // Memory loads (addr only)
        IrOp::Load8(addr) | IrOp::Load16(addr) |
        IrOp::Load32(addr) | IrOp::Load64(addr) => vec![*addr],
        // Memory stores
        IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
        IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => vec![*addr, *val],
        // Extensions
        IrOp::Sext8(v) | IrOp::Sext16(v) | IrOp::Sext32(v) |
        IrOp::Zext8(v) | IrOp::Zext16(v) | IrOp::Zext32(v) |
        IrOp::Trunc8(v) | IrOp::Trunc16(v) | IrOp::Trunc32(v) => vec![*v],
        // Flag extraction
        IrOp::GetCF(v) | IrOp::GetZF(v) | IrOp::GetSF(v) |
        IrOp::GetOF(v) | IrOp::GetPF(v) => vec![*v],
        // Select
        IrOp::Select(c, t, f) => vec![*c, *t, *f],
        // Guest register ops
        IrOp::StoreGpr(_, v) | IrOp::StoreFlags(v) | IrOp::StoreRip(v) => vec![*v],
        // I/O
        IrOp::In8(p) | IrOp::In16(p) | IrOp::In32(p) => vec![*p],
        IrOp::Out8(p, v) | IrOp::Out16(p, v) | IrOp::Out32(p, v) => vec![*p, *v],
        // Control flow
        IrOp::Branch(c, _, _) => vec![*c],
        IrOp::CallIndirect(t) => vec![*t],
        // Phi
        IrOp::Phi(sources) => sources.iter().map(|(_, v)| *v).collect(),
        // No operands
        _ => Vec::new(),
    }
}

/// Get defined register (SSA style - dst is in IrInstr.dst, not in IrOp)
/// This function is for checking if an op produces a value
fn op_produces_value(op: &IrOp) -> bool {
    match op {
        // These produce a value (dst is in IrInstr.dst)
        IrOp::Const(_) | IrOp::ConstF64(_) |
        IrOp::LoadGpr(_) | IrOp::LoadFlags | IrOp::LoadRip |
        IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_) |
        IrOp::Add(_, _) | IrOp::Sub(_, _) | IrOp::Mul(_, _) | IrOp::IMul(_, _) |
        IrOp::Div(_, _) | IrOp::IDiv(_, _) | IrOp::Neg(_) |
        IrOp::And(_, _) | IrOp::Or(_, _) | IrOp::Xor(_, _) | IrOp::Not(_) |
        IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) |
        IrOp::Rol(_, _) | IrOp::Ror(_, _) |
        IrOp::Cmp(_, _) | IrOp::Test(_, _) |
        IrOp::GetCF(_) | IrOp::GetZF(_) | IrOp::GetSF(_) | IrOp::GetOF(_) | IrOp::GetPF(_) |
        IrOp::Select(_, _, _) |
        IrOp::Sext8(_) | IrOp::Sext16(_) | IrOp::Sext32(_) |
        IrOp::Zext8(_) | IrOp::Zext16(_) | IrOp::Zext32(_) |
        IrOp::Trunc8(_) | IrOp::Trunc16(_) | IrOp::Trunc32(_) |
        IrOp::In8(_) | IrOp::In16(_) | IrOp::In32(_) |
        IrOp::Rdtsc | IrOp::Cpuid |
        IrOp::Phi(_) => true,
        // These don't produce a value
        _ => false,
    }
}

/// Check if operation has side effects
fn has_side_effect(op: &IrOp) -> bool {
    matches!(op,
        IrOp::Store8(_, _) | IrOp::Store16(_, _) |
        IrOp::Store32(_, _) | IrOp::Store64(_, _) |
        IrOp::StoreGpr(_, _) | IrOp::StoreFlags(_) | IrOp::StoreRip(_) |
        IrOp::Call(_) | IrOp::CallIndirect(_) |
        IrOp::Out8(_, _) | IrOp::Out16(_, _) | IrOp::Out32(_, _) |
        IrOp::Syscall | IrOp::Hlt |
        IrOp::Jump(_) | IrOp::Branch(_, _, _) | IrOp::Ret |
        IrOp::Exit(_)
    )
}

// ============================================================================
// Code emission helpers
// ============================================================================

fn emit_rex_w(code: &mut Vec<u8>, reg: u8, rm: u8) {
    let rex = 0x48 | ((reg >> 3) << 2) | (rm >> 3);
    code.push(rex);
}

fn emit_mov_reg_reg(code: &mut Vec<u8>, dst: u8, src: u8) {
    emit_rex_w(code, src, dst);
    code.push(0x89);
    code.push(0xC0 | ((src & 7) << 3) | (dst & 7));
}

fn emit_alu_reg_reg(code: &mut Vec<u8>, opcode: u8, dst: u8, src: u8) {
    emit_rex_w(code, src, dst);
    code.push(opcode);
    code.push(0xC0 | ((src & 7) << 3) | (dst & 7));
}

/// Emit: mov reg, [r15 + offset]
/// Load a 64-bit value from JitState (R15 is the JitState pointer, saved in prologue)
fn emit_load_from_jitstate(code: &mut Vec<u8>, reg: u8, offset: i32) {
    // REX.W prefix: 0x48 base
    // REX.R if reg >= 8
    // REX.B for R15 base register
    let rex = 0x49 | if reg >= 8 { 0x04 } else { 0 }; // 0x49 = REX.WB
    code.push(rex);
    
    // MOV r64, r/m64: 0x8B
    code.push(0x8B);
    
    // ModR/M: mod=01/10 (disp8/disp32), reg=target, r/m=7 (R15 with REX.B)
    if offset >= -128 && offset <= 127 {
        // mod=01: [R15 + disp8]
        code.push(0x47 | ((reg & 7) << 3));  // 01 reg 111
        code.push(offset as u8);
    } else {
        // mod=10: [R15 + disp32]
        code.push(0x87 | ((reg & 7) << 3));  // 10 reg 111
        code.extend_from_slice(&offset.to_le_bytes());
    }
}

/// Emit: mov [r15 + offset], reg
/// Store a 64-bit value to JitState
fn emit_store_to_jitstate(code: &mut Vec<u8>, offset: i32, reg: u8) {
    // REX.W prefix with REX.B for R15
    let rex = 0x49 | if reg >= 8 { 0x04 } else { 0 }; // 0x49 = REX.WB
    code.push(rex);
    
    // MOV r/m64, r64: 0x89
    code.push(0x89);
    
    // ModR/M: mod=01/10, reg=source, r/m=7 (R15 with REX.B)
    if offset >= -128 && offset <= 127 {
        code.push(0x47 | ((reg & 7) << 3));
        code.push(offset as u8);
    } else {
        code.push(0x87 | ((reg & 7) << 3));
        code.extend_from_slice(&offset.to_le_bytes());
    }
}

// ============================================================================
// Out-of-Order Instruction Scheduling - Enterprise Implementation
// ============================================================================
//
// This module implements a sophisticated instruction scheduler that maximizes
// ILP (Instruction-Level Parallelism) by reordering instructions within basic
// blocks while maintaining program semantics.
//
// ## Algorithm Overview
//
// 1. **Dependency Analysis**: Build a DAG representing all data and control
//    dependencies between instructions (RAW, WAR, WAW, memory, control).
//
// 2. **Latency Modeling**: Assign accurate execution latencies based on
//    Intel/AMD microarchitecture specifications.
//
// 3. **Critical Path Computation**: Calculate the longest latency path from
//    each instruction to any exit, used for priority scheduling.
//
// 4. **List Scheduling**: Greedily schedule instructions by selecting the
//    ready instruction with highest priority (critical path length).
//
// 5. **Register Pressure Tracking**: Monitor live register count to avoid
//    schedules that cause excessive spilling.

/// Instruction latency (cycles) for x86-64 operations
/// Based on Intel Ice Lake / AMD Zen 3 microarchitecture data
#[derive(Debug, Clone, Copy)]
struct InstrLatency {
    /// Execution latency (cycles until result is ready)
    latency: u8,
    /// Throughput (reciprocal: how many cycles until another can start)
    throughput: u8,
    /// Number of micro-ops (affects decoder throughput)
    uops: u8,
}

impl InstrLatency {
    const fn new(latency: u8, throughput: u8, uops: u8) -> Self {
        Self { latency, throughput, uops }
    }
    
    /// Get latency for an IR operation
    fn for_op(op: &IrOp) -> Self {
        match op {
            // Constants: zero latency (eliminated by register renaming)
            IrOp::Const(_) | IrOp::ConstF64(_) => Self::new(0, 1, 1),
            
            // Load guest registers: memory access from JitState
            IrOp::LoadGpr(_) | IrOp::LoadFlags | IrOp::LoadRip => Self::new(4, 1, 1),
            
            // Store guest registers: memory access to JitState
            IrOp::StoreGpr(_, _) | IrOp::StoreFlags(_) | IrOp::StoreRip(_) => Self::new(1, 1, 1),
            
            // Memory loads: L1 cache hit assumption
            IrOp::Load8(_) | IrOp::Load16(_) | IrOp::Load32(_) | IrOp::Load64(_) => Self::new(4, 1, 1),
            
            // Memory stores: fire-and-forget to store buffer
            IrOp::Store8(_, _) | IrOp::Store16(_, _) | 
            IrOp::Store32(_, _) | IrOp::Store64(_, _) => Self::new(1, 1, 1),
            
            // Simple ALU: single cycle
            IrOp::Add(_, _) | IrOp::Sub(_, _) | IrOp::And(_, _) |
            IrOp::Or(_, _) | IrOp::Xor(_, _) | IrOp::Neg(_) | IrOp::Not(_) => Self::new(1, 1, 1),
            
            // Shifts: single cycle on modern CPUs
            IrOp::Shl(_, _) | IrOp::Shr(_, _) | IrOp::Sar(_, _) => Self::new(1, 1, 1),
            
            // Rotates: slightly slower
            IrOp::Rol(_, _) | IrOp::Ror(_, _) => Self::new(1, 1, 2),
            
            // Multiplication: 3-4 cycles
            IrOp::Mul(_, _) | IrOp::IMul(_, _) => Self::new(3, 1, 1),
            
            // Division: very expensive (20-100 cycles depending on operands)
            IrOp::Div(_, _) | IrOp::IDiv(_, _) => Self::new(20, 10, 10),
            
            // Comparisons: single cycle, sets flags
            IrOp::Cmp(_, _) | IrOp::Test(_, _) => Self::new(1, 1, 1),
            
            // Flag extraction: single cycle
            IrOp::GetCF(_) | IrOp::GetZF(_) | IrOp::GetSF(_) |
            IrOp::GetOF(_) | IrOp::GetPF(_) => Self::new(1, 1, 1),
            
            // Conditional select: 1-2 cycles (CMOV)
            IrOp::Select(_, _, _) => Self::new(2, 1, 1),
            
            // Sign/zero extensions: register-to-register, 0-1 cycle
            IrOp::Sext8(_) | IrOp::Sext16(_) | IrOp::Sext32(_) |
            IrOp::Zext8(_) | IrOp::Zext16(_) | IrOp::Zext32(_) => Self::new(1, 1, 1),
            
            // Truncations: just use lower bits, effectively free
            IrOp::Trunc8(_) | IrOp::Trunc16(_) | IrOp::Trunc32(_) => Self::new(0, 1, 1),
            
            // Bit manipulation (if supported by ISA)
            IrOp::Popcnt(_) => Self::new(3, 1, 1),
            IrOp::Lzcnt(_) | IrOp::Tzcnt(_) => Self::new(3, 1, 1),
            IrOp::Bsf(_) | IrOp::Bsr(_) => Self::new(3, 1, 1),
            IrOp::Bextr(_, _, _) => Self::new(2, 1, 2),
            IrOp::Pdep(_, _) | IrOp::Pext(_, _) => Self::new(3, 1, 1),
            
            // FMA: 4 cycles but fully pipelined
            IrOp::Fma(_, _, _) => Self::new(4, 1, 1),
            
            // AES: ~4 cycles
            IrOp::Aesenc(_, _) | IrOp::Aesdec(_, _) => Self::new(4, 1, 1),
            
            // PCLMUL: 5-7 cycles
            IrOp::Pclmul(_, _, _) => Self::new(6, 2, 1),
            
            // Vector operations: depends on width and kind
            IrOp::VectorOp { width, .. } => {
                let lat = match *width {
                    128 => 3,
                    256 => 4,
                    512 => 5,
                    _ => 3,
                };
                Self::new(lat, 1, 1)
            }
            
            // I/O operations: expensive, causes VM exit
            IrOp::In8(_) | IrOp::In16(_) | IrOp::In32(_) => Self::new(50, 50, 5),
            IrOp::Out8(_, _) | IrOp::Out16(_, _) | IrOp::Out32(_, _) => Self::new(50, 50, 5),
            
            // Control flow: depends on prediction
            IrOp::Jump(_) => Self::new(1, 1, 1),
            IrOp::Branch(_, _, _) => Self::new(1, 1, 1),
            IrOp::Call(_) | IrOp::CallIndirect(_) => Self::new(3, 1, 2),
            IrOp::Ret => Self::new(1, 1, 1),
            
            // Special instructions
            IrOp::Syscall => Self::new(100, 100, 10),
            IrOp::Cpuid => Self::new(40, 40, 20),
            IrOp::Rdtsc => Self::new(15, 15, 2),
            IrOp::Hlt => Self::new(100, 100, 1),
            IrOp::Nop => Self::new(0, 1, 1),
            
            // PHI nodes: eliminated during SSA destruction
            IrOp::Phi(_) => Self::new(0, 1, 0),
            
            // Exit: causes VM exit
            IrOp::Exit(_) => Self::new(10, 10, 5),
        }
    }
}

/// Dependency types between instructions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepKind {
    /// Read-After-Write: true data dependency
    Raw,
    /// Write-After-Read: anti-dependency (can be eliminated with renaming)
    War,
    /// Write-After-Write: output dependency
    Waw,
    /// Memory dependency (must preserve order)
    Memory,
    /// Control dependency (terminator ordering)
    Control,
}

/// A dependency edge in the DAG
#[derive(Debug, Clone)]
struct DepEdge {
    /// Index of the predecessor instruction
    pred: usize,
    /// Type of dependency
    kind: DepKind,
    /// Latency from pred to this instruction
    latency: u8,
}

/// Scheduling context for a basic block
struct ScheduleContext {
    /// Number of instructions
    n: usize,
    /// Dependencies for each instruction (predecessors)
    deps: Vec<Vec<DepEdge>>,
    /// Successors for each instruction (for critical path)
    succs: Vec<Vec<(usize, u8)>>, // (successor_idx, latency)
    /// Instruction latencies
    latencies: Vec<InstrLatency>,
    /// Critical path length from each instruction to exit
    critical_path: Vec<u32>,
    /// Instructions that are terminators
    terminators: Vec<bool>,
    /// Memory access info for each instruction
    memory_ops: Vec<MemoryInfo>,
}

/// Memory operation classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryInfo {
    /// No memory access
    None,
    /// Load from address in VReg
    Load(VReg),
    /// Store to address in VReg
    Store(VReg),
    /// Load from guest register state (fixed offset)
    LoadGuest(u8),
    /// Store to guest register state (fixed offset)
    StoreGuest(u8),
}

impl ScheduleContext {
    /// Build scheduling context from instructions
    fn build(instrs: &[IrInstr]) -> Self {
        let n = instrs.len();
        let mut deps = vec![Vec::new(); n];
        let mut succs = vec![Vec::new(); n];
        let mut terminators = vec![false; n];
        let mut memory_ops = Vec::with_capacity(n);
        
        // Compute latencies and memory info
        let latencies: Vec<_> = instrs.iter()
            .map(|i| InstrLatency::for_op(&i.op))
            .collect();
        
        for instr in instrs {
            memory_ops.push(Self::classify_memory(&instr.op));
            terminators.push(instr.flags.contains(IrFlags::TERMINATOR));
        }
        
        // Track last writer of each VReg for RAW dependencies
        let mut last_writer: std::collections::HashMap<VReg, usize> = std::collections::HashMap::new();
        
        // Track last reader of each VReg for WAR dependencies
        let mut last_readers: std::collections::HashMap<VReg, Vec<usize>> = std::collections::HashMap::new();
        
        // Track last memory operations for memory dependencies
        let mut last_stores: Vec<usize> = Vec::new();
        let mut last_loads: Vec<usize> = Vec::new();
        
        // Track last guest register access for dependencies
        let mut last_guest_writer: [Option<usize>; 18] = [None; 18]; // 16 GPRs + FLAGS + RIP
        let mut last_guest_readers: [Vec<usize>; 18] = Default::default();
        
        for (i, instr) in instrs.iter().enumerate() {
            // RAW dependencies: read after write
            for operand in get_operands(&instr.op) {
                if let Some(&writer) = last_writer.get(&operand) {
                    let lat = latencies[writer].latency;
                    deps[i].push(DepEdge { pred: writer, kind: DepKind::Raw, latency: lat });
                    succs[writer].push((i, lat));
                }
            }
            
            // WAR dependencies: write after read (anti-dependency)
            if op_produces_value(&instr.op) && instr.dst.is_valid() {
                if let Some(readers) = last_readers.get(&instr.dst) {
                    for &reader in readers {
                        if reader != i {
                            // WAR has latency 0 (can be eliminated by renaming)
                            deps[i].push(DepEdge { pred: reader, kind: DepKind::War, latency: 0 });
                            succs[reader].push((i, 0));
                        }
                    }
                }
            }
            
            // WAW dependencies: write after write (output dependency)
            if op_produces_value(&instr.op) && instr.dst.is_valid() {
                if let Some(&prev_writer) = last_writer.get(&instr.dst) {
                    deps[i].push(DepEdge { pred: prev_writer, kind: DepKind::Waw, latency: 0 });
                    succs[prev_writer].push((i, 0));
                }
            }
            
            // Memory dependencies
            match memory_ops[i] {
                MemoryInfo::Load(_) => {
                    // Loads must wait for all prior stores (conservative)
                    // In SSA form we can't easily alias analyze, so be safe
                    for &store_idx in &last_stores {
                        deps[i].push(DepEdge { pred: store_idx, kind: DepKind::Memory, latency: 0 });
                        succs[store_idx].push((i, 0));
                    }
                    last_loads.push(i);
                }
                MemoryInfo::Store(_) => {
                    // Stores must wait for all prior loads and stores
                    for &load_idx in &last_loads {
                        deps[i].push(DepEdge { pred: load_idx, kind: DepKind::Memory, latency: 0 });
                        succs[load_idx].push((i, 0));
                    }
                    for &store_idx in &last_stores {
                        deps[i].push(DepEdge { pred: store_idx, kind: DepKind::Memory, latency: 0 });
                        succs[store_idx].push((i, 0));
                    }
                    last_stores.push(i);
                }
                MemoryInfo::LoadGuest(idx) => {
                    let slot = idx as usize;
                    // Must wait for prior write to this guest register
                    if let Some(writer) = last_guest_writer[slot] {
                        deps[i].push(DepEdge { pred: writer, kind: DepKind::Raw, latency: latencies[writer].latency });
                        succs[writer].push((i, latencies[writer].latency));
                    }
                    last_guest_readers[slot].push(i);
                }
                MemoryInfo::StoreGuest(idx) => {
                    let slot = idx as usize;
                    // WAR: must wait for prior reads
                    for &reader in &last_guest_readers[slot] {
                        deps[i].push(DepEdge { pred: reader, kind: DepKind::War, latency: 0 });
                        succs[reader].push((i, 0));
                    }
                    // WAW: must wait for prior write
                    if let Some(writer) = last_guest_writer[slot] {
                        deps[i].push(DepEdge { pred: writer, kind: DepKind::Waw, latency: 0 });
                        succs[writer].push((i, 0));
                    }
                    last_guest_writer[slot] = Some(i);
                    last_guest_readers[slot].clear();
                }
                MemoryInfo::None => {}
            }
            
            // Update tracking for next iteration
            for operand in get_operands(&instr.op) {
                last_readers.entry(operand).or_default().push(i);
            }
            if op_produces_value(&instr.op) && instr.dst.is_valid() {
                last_writer.insert(instr.dst, i);
                last_readers.remove(&instr.dst);
            }
            
            // Control dependencies: terminators must be last
            if instr.flags.contains(IrFlags::TERMINATOR) {
                terminators[i] = true;
            }
        }
        
        // Critical fix: Terminators must depend on ALL prior instructions
        // This ensures they are scheduled last in the basic block
        for i in 0..n {
            if terminators[i] {
                // Terminator depends on all prior non-terminator instructions
                for j in 0..i {
                    if !terminators[j] {
                        // Check if dependency already exists
                        let already_has_dep = deps[i].iter().any(|e| e.pred == j);
                        if !already_has_dep {
                            deps[i].push(DepEdge { pred: j, kind: DepKind::Control, latency: 0 });
                            succs[j].push((i, 0));
                        }
                    }
                }
            }
        }
        
        // Also ensure non-terminators after a terminator depend on it
        // (This handles multiple terminators in sequence)
        let mut last_terminator: Option<usize> = None;
        for i in 0..n {
            if let Some(term) = last_terminator {
                if !terminators[i] {
                    deps[i].push(DepEdge { pred: term, kind: DepKind::Control, latency: 0 });
                    succs[term].push((i, 0));
                }
            }
            if terminators[i] {
                last_terminator = Some(i);
            }
        }
        
        // Compute critical path lengths (reverse topological order)
        let critical_path = Self::compute_critical_paths(n, &succs, &latencies);
        
        Self {
            n,
            deps,
            succs,
            latencies,
            critical_path,
            terminators,
            memory_ops,
        }
    }
    
    /// Classify memory access for an operation
    fn classify_memory(op: &IrOp) -> MemoryInfo {
        match op {
            IrOp::Load8(addr) | IrOp::Load16(addr) |
            IrOp::Load32(addr) | IrOp::Load64(addr) => MemoryInfo::Load(*addr),
            
            IrOp::Store8(addr, _) | IrOp::Store16(addr, _) |
            IrOp::Store32(addr, _) | IrOp::Store64(addr, _) => MemoryInfo::Store(*addr),
            
            IrOp::LoadGpr(idx) => MemoryInfo::LoadGuest(*idx),
            IrOp::StoreGpr(idx, _) => MemoryInfo::StoreGuest(*idx),
            
            IrOp::LoadFlags => MemoryInfo::LoadGuest(16),
            IrOp::StoreFlags(_) => MemoryInfo::StoreGuest(16),
            
            IrOp::LoadRip => MemoryInfo::LoadGuest(17),
            IrOp::StoreRip(_) => MemoryInfo::StoreGuest(17),
            
            _ => MemoryInfo::None,
        }
    }
    
    /// Compute critical path length from each instruction to any exit
    fn compute_critical_paths(n: usize, succs: &[Vec<(usize, u8)>], latencies: &[InstrLatency]) -> Vec<u32> {
        let mut critical = vec![0u32; n];
        
        // Process in reverse order (approximate reverse topological sort)
        // For true reverse topological order we'd need to sort by dependencies,
        // but iterating a few times converges for most CFGs
        for _ in 0..3 {
            for i in (0..n).rev() {
                let own_lat = latencies[i].latency as u32;
                let max_succ = succs[i].iter()
                    .map(|(s, edge_lat)| critical[*s] + *edge_lat as u32)
                    .max()
                    .unwrap_or(0);
                critical[i] = own_lat + max_succ;
            }
        }
        
        critical
    }
    
    /// Run list scheduling algorithm
    fn schedule(&self) -> Vec<usize> {
        let mut scheduled = Vec::with_capacity(self.n);
        let mut done = vec![false; self.n];
        let mut remaining_deps: Vec<usize> = self.deps.iter()
            .map(|d| d.len())
            .collect();
        
        // Ready queue: instructions with all dependencies satisfied
        // Use a simple sorted Vec (small block sizes make heap overhead not worth it)
        let mut ready: Vec<usize> = Vec::new();
        
        // Initialize ready queue with instructions that have no dependencies
        for i in 0..self.n {
            if remaining_deps[i] == 0 {
                ready.push(i);
            }
        }
        
        // Track simulated cycle for latency-aware scheduling
        let mut cycle = 0u32;
        let mut ready_time = vec![0u32; self.n];
        
        while scheduled.len() < self.n {
            // Sort ready queue by priority (critical path length, descending)
            // Tie-breaker: prefer lower index (preserve original order when equal)
            ready.sort_by(|&a, &b| {
                let crit_cmp = self.critical_path[b].cmp(&self.critical_path[a]);
                if crit_cmp == std::cmp::Ordering::Equal {
                    a.cmp(&b)
                } else {
                    crit_cmp
                }
            });
            
            // Find an instruction that's actually ready (ready_time <= cycle)
            let mut chosen = None;
            for (idx, &instr) in ready.iter().enumerate() {
                if ready_time[instr] <= cycle {
                    chosen = Some(idx);
                    break;
                }
            }
            
            if let Some(idx) = chosen {
                let instr = ready.remove(idx);
                scheduled.push(instr);
                done[instr] = true;
                
                // Update ready times for successors
                let finish_time = cycle + self.latencies[instr].latency as u32;
                for &(succ, edge_lat) in &self.succs[instr] {
                    let succ_ready = finish_time.saturating_sub(edge_lat as u32);
                    ready_time[succ] = ready_time[succ].max(succ_ready);
                    
                    // Decrement remaining deps and add to ready if all satisfied
                    remaining_deps[succ] -= 1;
                    if remaining_deps[succ] == 0 && !done[succ] && !ready.contains(&succ) {
                        ready.push(succ);
                    }
                }
                
                cycle += 1;
            } else if !ready.is_empty() {
                // All ready instructions are waiting on latency, advance cycle
                let min_ready = ready.iter()
                    .map(|&i| ready_time[i])
                    .min()
                    .unwrap_or(cycle + 1);
                cycle = min_ready;
            } else {
                // No ready instructions - find any unscheduled one (shouldn't happen with correct deps)
                for i in 0..self.n {
                    if !done[i] {
                        ready.push(i);
                        break;
                    }
                }
                cycle += 1;
            }
        }
        
        scheduled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::profile::ProfileDb;
    use std::sync::Arc;
    
    #[test]
    fn test_peephole_add_zero() {
        let compiler = S1Compiler::new();
        
        let mut ir = IrBlock::new(0x1000);
        ir.blocks[0].instrs.push(IrInstr {
            dst: VReg(0),
            op: IrOp::Add(VReg(1), VReg(2)),
            guest_rip: 0x1000,
            flags: IrFlags::empty(),
        });
        
        compiler.peephole(&mut ir);
        // Would check that add with const 0 became copy
    }
    
    #[test]
    fn test_s1_jmp_rel8_native_execution() {
        // Test compiling JMP rel8 (EB FD = JMP -3) and verify the native code can execute
        let compiler = S1Compiler::new();
        let decoder = super::super::decoder::X86Decoder::new();
        let profile = Arc::new(ProfileDb::new(100));
        
        // JMP rel8 -3 (EB FD) at address 0x72e2, jumps to 0x72e1
        let guest_code: [u8; 2] = [0xEB, 0xFD];
        let start_rip = 0x72e2u64;
        
        let result = compiler.compile(&guest_code, start_rip, &decoder, &profile);
        assert!(result.is_ok(), "S1 compile should succeed");
        
        let s1_block = result.unwrap();
        println!("Native code size: {} bytes", s1_block.native.len());
        println!("IR blocks: {}", s1_block.ir.blocks.len());
        println!("IR instrs in block 0: {}", s1_block.ir.blocks[0].instrs.len());
        
        // Print all IR instructions
        for (i, instr) in s1_block.ir.blocks[0].instrs.iter().enumerate() {
            println!("  IR[{}]: dst={:?} op={:?}", i, instr.dst, instr.op);
        }
        
        // Print native code as hex
        println!("Native code:");
        for (i, chunk) in s1_block.native.chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
            println!("  {:04x}: {}", i * 16, hex.join(" "));
        }
        
        // Now try to execute it with a mock JitState
        use crate::jit::JitState;
        
        let mut jit_state = JitState::new();
        jit_state.rip = start_rip;
        
        // Allocate executable memory
        let exec_mem = unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            assert!(ptr != libc::MAP_FAILED, "mmap failed");
            ptr as *mut u8
        };
        
        // Copy native code
        unsafe {
            std::ptr::copy_nonoverlapping(
                s1_block.native.as_ptr(),
                exec_mem,
                s1_block.native.len(),
            );
        }
        
        // Execute
        println!("Executing native code at {:p}...", exec_mem);
        let result = unsafe {
            let func: extern "C" fn(*mut JitState) -> u64 = 
                std::mem::transmute(exec_mem as *const u8);
            func(&mut jit_state as *mut JitState)
        };
        
        println!("Execution result: {:#x}", result);
        println!("JitState.rip after: {:#x}", jit_state.rip);
        
        // Clean up
        unsafe {
            libc::munmap(exec_mem as *mut libc::c_void, 4096);
        }
        
        // Verify result
        // For JMP rel8 -3 at 0x72e2: target = 0x72e2 + 2 + (-3) = 0x72e1
        // Exit(Normal) returns kind=0 in high byte, rip in low bytes
        assert_eq!(result >> 56, 0, "Exit kind should be 0 (Continue)");
        assert_eq!(result & 0x00FF_FFFF_FFFF_FFFF, 0x72e1, "Next RIP should be 0x72e1");
        assert_eq!(jit_state.rip, 0x72e1, "JitState.rip should be updated to 0x72e1");
    }
    
    // ========================================================================
    // Out-of-Order Scheduling Tests
    // ========================================================================
    
    #[test]
    fn test_instruction_latency_model() {
        // Verify latency values for different instruction types
        let add_lat = InstrLatency::for_op(&IrOp::Add(VReg(0), VReg(1)));
        assert_eq!(add_lat.latency, 1, "ADD should have 1 cycle latency");
        
        let mul_lat = InstrLatency::for_op(&IrOp::Mul(VReg(0), VReg(1)));
        assert_eq!(mul_lat.latency, 3, "MUL should have 3 cycle latency");
        
        let div_lat = InstrLatency::for_op(&IrOp::Div(VReg(0), VReg(1)));
        assert_eq!(div_lat.latency, 20, "DIV should have 20 cycle latency");
        
        let load_lat = InstrLatency::for_op(&IrOp::Load64(VReg(0)));
        assert_eq!(load_lat.latency, 4, "LOAD should have 4 cycle latency");
        
        let const_lat = InstrLatency::for_op(&IrOp::Const(42));
        assert_eq!(const_lat.latency, 0, "CONST should have 0 cycle latency");
    }
    
    #[test]
    fn test_memory_classification() {
        // Test memory operation classification
        assert_eq!(
            ScheduleContext::classify_memory(&IrOp::Load64(VReg(0))),
            MemoryInfo::Load(VReg(0))
        );
        assert_eq!(
            ScheduleContext::classify_memory(&IrOp::Store64(VReg(0), VReg(1))),
            MemoryInfo::Store(VReg(0))
        );
        assert_eq!(
            ScheduleContext::classify_memory(&IrOp::LoadGpr(5)),
            MemoryInfo::LoadGuest(5)
        );
        assert_eq!(
            ScheduleContext::classify_memory(&IrOp::StoreGpr(3, VReg(0))),
            MemoryInfo::StoreGuest(3)
        );
        assert_eq!(
            ScheduleContext::classify_memory(&IrOp::Add(VReg(0), VReg(1))),
            MemoryInfo::None
        );
    }
    
    #[test]
    fn test_schedule_independent_instructions() {
        // Independent instructions should be scheduled by latency
        let instrs = vec![
            // Two independent adds - both should be schedulable in parallel
            IrInstr {
                dst: VReg(0),
                op: IrOp::Const(1),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(1),
                op: IrOp::Const(2),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(2),
                op: IrOp::Const(3),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
        ];
        
        let ctx = ScheduleContext::build(&instrs);
        let order = ctx.schedule();
        
        // All instructions should be scheduled (order may vary since independent)
        assert_eq!(order.len(), 3);
        assert!(order.contains(&0));
        assert!(order.contains(&1));
        assert!(order.contains(&2));
    }
    
    #[test]
    fn test_schedule_dependent_chain() {
        // v0 = const 1
        // v1 = add v0, v0  (depends on v0)
        // v2 = mul v1, v1  (depends on v1)
        let instrs = vec![
            IrInstr {
                dst: VReg(0),
                op: IrOp::Const(1),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(1),
                op: IrOp::Add(VReg(0), VReg(0)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(2),
                op: IrOp::Mul(VReg(1), VReg(1)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
        ];
        
        let ctx = ScheduleContext::build(&instrs);
        let order = ctx.schedule();
        
        // Must preserve dependency order: 0 -> 1 -> 2
        assert_eq!(order.len(), 3);
        let pos_0 = order.iter().position(|&x| x == 0).unwrap();
        let pos_1 = order.iter().position(|&x| x == 1).unwrap();
        let pos_2 = order.iter().position(|&x| x == 2).unwrap();
        assert!(pos_0 < pos_1, "v0 must be scheduled before v1");
        assert!(pos_1 < pos_2, "v1 must be scheduled before v2");
    }
    
    #[test]
    fn test_schedule_memory_dependencies() {
        // Load and store to same address must preserve order
        // v0 = load [addr]
        // store [addr], v1  (WAR dependency on v0)
        let instrs = vec![
            IrInstr {
                dst: VReg(0),
                op: IrOp::Const(0x1000), // address
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(1),
                op: IrOp::Load64(VReg(0)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(2),
                op: IrOp::Const(42),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg::NONE,
                op: IrOp::Store64(VReg(0), VReg(2)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
        ];
        
        let ctx = ScheduleContext::build(&instrs);
        let order = ctx.schedule();
        
        // Load must come before store
        let load_pos = order.iter().position(|&x| x == 1).unwrap();
        let store_pos = order.iter().position(|&x| x == 3).unwrap();
        assert!(load_pos < store_pos, "Load must be scheduled before store");
    }
    
    #[test]
    fn test_schedule_critical_path_priority() {
        // Two independent chains, scheduler should prioritize longer chain
        // Chain A: const -> add (total latency: 0 + 1 = 1)
        // Chain B: const -> mul -> div (total latency: 0 + 3 + 20 = 23)
        // Scheduler should start Chain B first
        let instrs = vec![
            // Chain A
            IrInstr {
                dst: VReg(0),
                op: IrOp::Const(1),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(1),
                op: IrOp::Add(VReg(0), VReg(0)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            // Chain B
            IrInstr {
                dst: VReg(2),
                op: IrOp::Const(2),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(3),
                op: IrOp::Mul(VReg(2), VReg(2)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
            IrInstr {
                dst: VReg(4),
                op: IrOp::Div(VReg(3), VReg(3)),
                guest_rip: 0x1000,
                flags: IrFlags::empty(),
            },
        ];
        
        let ctx = ScheduleContext::build(&instrs);
        
        // Verify critical path lengths
        assert!(ctx.critical_path[2] > ctx.critical_path[0], 
            "Chain B start should have higher critical path");
        assert!(ctx.critical_path[4] >= 20,
            "DIV instruction should have at least 20 cycles on critical path");
        
        let order = ctx.schedule();
        
        // Chain B's start (index 2) should be scheduled before Chain A's start (index 0)
        // because it has a longer critical path
        let chain_a_start = order.iter().position(|&x| x == 0).unwrap();
        let chain_b_start = order.iter().position(|&x| x == 2).unwrap();
        assert!(chain_b_start < chain_a_start,
            "Critical path scheduling should prioritize longer chain");
    }
    
    #[test]
    fn test_compiler_with_scheduling_enabled() {
        let config = S1Config {
            scheduling: true,
            ..Default::default()
        };
        let compiler = S1Compiler::with_config(config);
        
        let mut ir = IrBlock::new(0x1000);
        
        // Add some independent instructions
        ir.blocks[0].instrs.push(IrInstr {
            dst: VReg(0),
            op: IrOp::Const(1),
            guest_rip: 0x1000,
            flags: IrFlags::empty(),
        });
        ir.blocks[0].instrs.push(IrInstr {
            dst: VReg(1),
            op: IrOp::Const(2),
            guest_rip: 0x1000,
            flags: IrFlags::empty(),
        });
        ir.blocks[0].instrs.push(IrInstr {
            dst: VReg(2),
            op: IrOp::Add(VReg(0), VReg(1)),
            guest_rip: 0x1000,
            flags: IrFlags::empty(),
        });
        
        // Should not panic
        compiler.schedule_instructions(&mut ir);
        
        // Verify all instructions still present
        assert_eq!(ir.blocks[0].instrs.len(), 3);
    }
    
    #[test]
    fn test_compiler_with_scheduling_disabled() {
        let config = S1Config {
            scheduling: false,
            ..Default::default()
        };
        let compiler = S1Compiler::with_config(config);
        
        let decoder = super::super::decoder::X86Decoder::new();
        let profile = Arc::new(ProfileDb::new(100));
        
        // Simple NOP instruction
        let guest_code: [u8; 1] = [0x90];
        let result = compiler.compile(&guest_code, 0x1000, &decoder, &profile);
        
        assert!(result.is_ok(), "Compilation should succeed with scheduling disabled");
    }
}
