//! S1 Quick Compiler
//!
//! Fast baseline compiler that generates decent code quickly.
//! No heavy optimizations - focus on compilation speed.
//! Used for warm-up before S2 kicks in.

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrBasicBlock, IrInstr, IrOp, IrFlags, VReg, BlockId, ExitReason, IrBuilder};
use super::decoder::{X86Decoder, DecodedInstr, Mnemonic};
use super::profile::ProfileDb;

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
}

impl Default for S1Config {
    fn default() -> Self {
        Self {
            max_instrs: 100,
            const_fold: true,
            dead_code_elim: true,
            peephole: true,
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
}

/// S1 quick compiler
pub struct S1Compiler {
    config: S1Config,
}

impl S1Compiler {
    pub fn new() -> Self {
        Self {
            config: S1Config::default(),
        }
    }
    
    pub fn with_config(config: S1Config) -> Self {
        Self { config }
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
        
        // Generate native code
        let native = self.codegen(&ir, profile)?;
        let est_cycles = self.estimate_cycles(&ir);
        
        Ok(S1Block {
            guest_rip: start_rip,
            guest_size: offset as u32,
            ir,
            native,
            est_cycles,
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
    
    /// Generate native code from IR
    fn codegen(&self, ir: &IrBlock, _profile: &ProfileDb) -> JitResult<Vec<u8>> {
        let mut code = Vec::new();
        
        for bb in &ir.blocks {
            for instr in &bb.instrs {
                self.emit_instr(&mut code, instr)?;
            }
        }
        
        Ok(code)
    }
    
    fn emit_instr(&self, code: &mut Vec<u8>, instr: &IrInstr) -> JitResult<()> {
        let dst = instr.dst;
        
        match &instr.op {
            IrOp::Const(val) => {
                // mov r64, imm64
                let reg = vreg_to_host(dst);
                code.push(0x48 | ((reg >> 3) << 2)); // REX.W + REX.B if needed
                code.push(0xB8 + (reg & 7));
                code.extend_from_slice(&(*val as u64).to_le_bytes());
            }
            
            IrOp::LoadGpr(idx) => {
                // Load from guest state
                let reg = vreg_to_host(dst);
                let _ = (reg, idx); // Placeholder
                code.push(0x90); // NOP
            }
            
            IrOp::Add(a, b) => {
                // SSA: dst = a + b
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                // If dst != a, mov dst, a first
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                // add dst, b
                emit_alu_reg_reg(code, 0x01, dreg, breg);
            }
            
            IrOp::Sub(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x29, dreg, breg);
            }
            
            IrOp::And(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x21, dreg, breg);
            }
            
            IrOp::Or(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x09, dreg, breg);
            }
            
            IrOp::Xor(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_alu_reg_reg(code, 0x31, dreg, breg);
            }
            
            IrOp::Mul(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                
                // imul r64, r64
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_rex_w(code, dreg, breg);
                code.extend_from_slice(&[0x0F, 0xAF]);
                code.push(0xC0 | ((dreg & 7) << 3) | (breg & 7));
            }
            
            IrOp::Shl(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                
                // Move shift amount to CL if not constant
                if let Some(shift) = get_const_val(*b) {
                    // shl r64, imm8
                    emit_rex_w(code, dreg, 0);
                    code.push(0xC1);
                    code.push(0xE0 | (dreg & 7));
                    code.push(shift as u8 & 63);
                } else {
                    let breg = vreg_to_host(*b);
                    emit_mov_reg_reg(code, 1, breg); // mov rcx, b
                    emit_rex_w(code, dreg, 0);
                    code.push(0xD3);
                    code.push(0xE0 | (dreg & 7));
                }
            }
            
            IrOp::Shr(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                
                if let Some(shift) = get_const_val(*b) {
                    emit_rex_w(code, dreg, 0);
                    code.push(0xC1);
                    code.push(0xE8 | (dreg & 7));
                    code.push(shift as u8 & 63);
                } else {
                    let breg = vreg_to_host(*b);
                    emit_mov_reg_reg(code, 1, breg);
                    emit_rex_w(code, dreg, 0);
                    code.push(0xD3);
                    code.push(0xE8 | (dreg & 7));
                }
            }
            
            IrOp::Sar(a, b) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                
                if let Some(shift) = get_const_val(*b) {
                    emit_rex_w(code, dreg, 0);
                    code.push(0xC1);
                    code.push(0xF8 | (dreg & 7)); // SAR /7
                    code.push(shift as u8 & 63);
                } else {
                    let breg = vreg_to_host(*b);
                    emit_mov_reg_reg(code, 1, breg);
                    emit_rex_w(code, dreg, 0);
                    code.push(0xD3);
                    code.push(0xF8 | (dreg & 7));
                }
            }
            
            IrOp::Load8(addr) | IrOp::Load16(addr) | 
            IrOp::Load32(addr) | IrOp::Load64(addr) => {
                // SSA: dst = mem[addr]
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*addr);
                
                match &instr.op {
                    IrOp::Load8(_) => {
                        // movzx r64, byte [addr]
                        emit_rex_w(code, dreg, areg);
                        code.extend_from_slice(&[0x0F, 0xB6]);
                        code.push((dreg & 7) << 3 | (areg & 7));
                    }
                    IrOp::Load64(_) => {
                        // mov r64, [addr]
                        emit_rex_w(code, dreg, areg);
                        code.push(0x8B);
                        code.push((dreg & 7) << 3 | (areg & 7));
                    }
                    _ => {
                        // Similar patterns for 16/32 bit
                        emit_rex_w(code, dreg, areg);
                        code.push(0x8B);
                        code.push((dreg & 7) << 3 | (areg & 7));
                    }
                }
            }
            
            IrOp::Store8(addr, val) | IrOp::Store16(addr, val) |
            IrOp::Store32(addr, val) | IrOp::Store64(addr, val) => {
                let areg = vreg_to_host(*addr);
                let vreg = vreg_to_host(*val);
                
                // mov [addr], r
                emit_rex_w(code, vreg, areg);
                code.push(0x89);
                code.push((vreg & 7) << 3 | (areg & 7));
            }
            
            IrOp::Cmp(a, b) => {
                // SSA: dst = flags(a cmp b)
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                let dreg = vreg_to_host(dst);
                
                // cmp a, b
                emit_alu_reg_reg(code, 0x39, areg, breg);
                
                // lahf to capture flags
                code.push(0x9F);
                // mov dst, rax (flags in AH)
                emit_mov_reg_reg(code, dreg, 0);
            }
            
            IrOp::Test(a, b) => {
                let areg = vreg_to_host(*a);
                let breg = vreg_to_host(*b);
                let dreg = vreg_to_host(dst);
                
                // test a, b
                emit_alu_reg_reg(code, 0x85, areg, breg);
                
                code.push(0x9F); // lahf
                emit_mov_reg_reg(code, dreg, 0);
            }
            
            IrOp::Call(target) => {
                // Direct call to address
                // call rel32 - would need address resolution
                let _ = target;
                code.extend_from_slice(&[0xE8, 0x00, 0x00, 0x00, 0x00]); // Placeholder
            }
            
            IrOp::CallIndirect(target) => {
                // call helper function
                let treg = vreg_to_host(*target);
                emit_rex_w(code, 0, treg);
                code.push(0xFF);
                code.push(0xD0 | (treg & 7));
            }
            
            IrOp::Jump(block_id) => {
                // jmp rel32 - would need label resolution
                let _ = block_id;
                code.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]); // Placeholder
            }
            
            IrOp::Branch(cond, true_blk, false_blk) => {
                // Test condition and branch
                let creg = vreg_to_host(*cond);
                let _ = (true_blk, false_blk);
                
                // test cond, cond
                emit_alu_reg_reg(code, 0x85, creg, creg);
                
                // jnz to true_blk (placeholder - would need label resolution)
                code.extend_from_slice(&[0x0F, 0x85, 0x00, 0x00, 0x00, 0x00]); // jnz rel32
                // jmp to false_blk
                code.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]); // jmp rel32
            }
            
            IrOp::Ret => {
                code.push(0xC3);
            }
            
            IrOp::Exit(reason) => {
                // Exit VM - return to runtime with exit code
                // mov eax, exit_code
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
                code.push(0xB8);
                code.extend_from_slice(&exit_code.to_le_bytes());
                code.push(0xC3);
            }
            
            IrOp::Nop => {
                code.push(0x90);
            }
            
            IrOp::Neg(a) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
                if dreg != areg {
                    emit_mov_reg_reg(code, dreg, areg);
                }
                emit_rex_w(code, 0, dreg);
                code.push(0xF7);
                code.push(0xD8 | (dreg & 7)); // neg r64
            }
            
            IrOp::Not(a) => {
                let dreg = vreg_to_host(dst);
                let areg = vreg_to_host(*a);
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

/// Map virtual register to host register
fn vreg_to_host(vreg: VReg) -> u8 {
    // Simple mapping: vreg.0 mod 16
    // In real impl, would use register allocator
    (vreg.0 as u8) % 16
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

#[cfg(test)]
mod tests {
    use super::*;
    
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
}
