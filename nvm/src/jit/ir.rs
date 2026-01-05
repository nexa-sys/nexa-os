//! Intermediate Representation (IR)
//!
//! SSA-form IR for x86 → native compilation.
//! Designed for efficient optimization and code generation.

use std::sync::Arc;
use std::collections::HashMap;

/// Virtual register (SSA value)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VReg(pub u32);

impl VReg {
    pub const NONE: VReg = VReg(u32::MAX);
    
    pub fn is_valid(&self) -> bool {
        self.0 != u32::MAX
    }
}

/// IR operation
#[derive(Debug, Clone)]
pub enum IrOp {
    // Constants
    Const(i64),
    ConstF64(f64),
    
    // Load/Store guest registers
    LoadGpr(u8),           // Load guest GPR[n]
    StoreGpr(u8, VReg),    // Store to guest GPR[n]
    LoadFlags,             // Load guest RFLAGS
    StoreFlags(VReg),      // Store guest RFLAGS
    LoadRip,               // Load guest RIP
    StoreRip(VReg),        // Store guest RIP
    
    // Memory operations
    Load8(VReg),           // Load 8-bit from addr
    Load16(VReg),
    Load32(VReg),
    Load64(VReg),
    Store8(VReg, VReg),    // Store 8-bit: addr, value
    Store16(VReg, VReg),
    Store32(VReg, VReg),
    Store64(VReg, VReg),
    
    // Arithmetic (result, operand1, operand2)
    Add(VReg, VReg),
    Sub(VReg, VReg),
    Mul(VReg, VReg),
    IMul(VReg, VReg),
    Div(VReg, VReg),
    IDiv(VReg, VReg),
    Neg(VReg),
    
    // Logical
    And(VReg, VReg),
    Or(VReg, VReg),
    Xor(VReg, VReg),
    Not(VReg),
    
    // Shifts
    Shl(VReg, VReg),
    Shr(VReg, VReg),
    Sar(VReg, VReg),
    Rol(VReg, VReg),
    Ror(VReg, VReg),
    
    // Comparison (sets flags VReg)
    Cmp(VReg, VReg),
    Test(VReg, VReg),
    
    // Flag extraction
    GetCF(VReg),           // Extract CF from flags
    GetZF(VReg),
    GetSF(VReg),
    GetOF(VReg),
    GetPF(VReg),
    
    // Conditional
    Select(VReg, VReg, VReg), // cond, true_val, false_val
    
    // Sign/Zero extension
    Sext8(VReg),
    Sext16(VReg),
    Sext32(VReg),
    Zext8(VReg),
    Zext16(VReg),
    Zext32(VReg),
    
    // Truncation
    Trunc8(VReg),
    Trunc16(VReg),
    Trunc32(VReg),
    
    // Control flow
    Jump(BlockId),
    Branch(VReg, BlockId, BlockId),  // cond, true_block, false_block
    Call(u64),                       // Direct call
    CallIndirect(VReg),              // Indirect call
    Ret,
    
    // Special
    Syscall,
    Cpuid,
    Rdtsc,
    Hlt,
    Nop,
    
    // I/O
    In8(VReg),             // IN AL, port
    In16(VReg),
    In32(VReg),
    Out8(VReg, VReg),      // OUT port, value
    Out16(VReg, VReg),
    Out32(VReg, VReg),
    
    // Phi node (SSA)
    Phi(Vec<(BlockId, VReg)>),
    
    // Exit VM (for interpreter/deopt)
    Exit(ExitReason),
}

/// Exit reason for VM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Normal,
    Halt,
    Interrupt(u8),
    Exception(u8, u32),
    IoRead(u16, u8),
    IoWrite(u16, u8),
    Mmio(u64, u8, bool),
    Hypercall,
    Reset,
}

/// Basic block ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// IR instruction
#[derive(Debug, Clone)]
pub struct IrInstr {
    /// Destination register (if any)
    pub dst: VReg,
    /// Operation
    pub op: IrOp,
    /// Original guest RIP (for debugging)
    pub guest_rip: u64,
    /// Instruction flags
    pub flags: IrFlags,
}

bitflags::bitflags! {
    /// IR instruction flags
    #[derive(Debug, Clone, Copy, Default)]
    pub struct IrFlags: u16 {
        /// May trap/fault
        const MAY_TRAP = 1 << 0;
        /// Has side effects
        const SIDE_EFFECT = 1 << 1;
        /// Is terminator
        const TERMINATOR = 1 << 2;
        /// Updates flags
        const UPDATES_FLAGS = 1 << 3;
        /// Reads flags
        const READS_FLAGS = 1 << 4;
        /// Memory read
        const MEM_READ = 1 << 5;
        /// Memory write
        const MEM_WRITE = 1 << 6;
    }
}

/// Basic block
#[derive(Debug, Clone)]
pub struct IrBasicBlock {
    pub id: BlockId,
    pub instrs: Vec<IrInstr>,
    pub predecessors: Vec<BlockId>,
    pub successors: Vec<BlockId>,
    /// Entry RIP (guest address)
    pub entry_rip: u64,
}

impl IrBasicBlock {
    pub fn new(id: BlockId, entry_rip: u64) -> Self {
        Self {
            id,
            instrs: Vec::new(),
            predecessors: Vec::new(),
            successors: Vec::new(),
            entry_rip,
        }
    }
    
    pub fn push(&mut self, instr: IrInstr) {
        self.instrs.push(instr);
    }
    
    pub fn is_terminated(&self) -> bool {
        self.instrs.last()
            .map(|i| i.flags.contains(IrFlags::TERMINATOR))
            .unwrap_or(false)
    }
}

/// IR block (function/trace)
#[derive(Debug, Clone)]
pub struct IrBlock {
    /// Entry guest RIP
    pub entry_rip: u64,
    /// Guest code size (bytes)
    pub guest_size: usize,
    /// Basic blocks
    pub blocks: Vec<IrBasicBlock>,
    /// Entry block ID
    pub entry_block: BlockId,
    /// Next VReg ID
    pub next_vreg: u32,
    /// Metadata
    pub meta: IrBlockMeta,
}

/// Block metadata
#[derive(Debug, Clone, Default)]
pub struct IrBlockMeta {
    /// Number of guest instructions
    pub guest_instr_count: usize,
    /// Number of IR instructions
    pub ir_instr_count: usize,
    /// Has memory operations
    pub has_memory_ops: bool,
    /// Has I/O operations
    pub has_io_ops: bool,
    /// Has control flow
    pub has_branches: bool,
    /// Is loop
    pub is_loop: bool,
    /// Loop depth
    pub loop_depth: u32,
}

impl IrBlock {
    pub fn new(entry_rip: u64) -> Self {
        Self {
            entry_rip,
            guest_size: 0,
            blocks: vec![IrBasicBlock::new(BlockId(0), entry_rip)],
            entry_block: BlockId(0),
            next_vreg: 0,
            meta: IrBlockMeta::default(),
        }
    }
    
    pub fn alloc_vreg(&mut self) -> VReg {
        let v = VReg(self.next_vreg);
        self.next_vreg += 1;
        v
    }
    
    pub fn get_block(&self, id: BlockId) -> Option<&IrBasicBlock> {
        self.blocks.get(id.0 as usize)
    }
    
    pub fn get_block_mut(&mut self, id: BlockId) -> Option<&mut IrBasicBlock> {
        self.blocks.get_mut(id.0 as usize)
    }
    
    pub fn add_block(&mut self, entry_rip: u64) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(IrBasicBlock::new(id, entry_rip));
        id
    }
    
    pub fn entry_block(&self) -> &IrBasicBlock {
        &self.blocks[self.entry_block.0 as usize]
    }
    
    pub fn entry_block_mut(&mut self) -> &mut IrBasicBlock {
        &mut self.blocks[self.entry_block.0 as usize]
    }
}

/// IR builder (translates decoded x86 → IR)
pub struct IrBuilder {
    block: IrBlock,
    current_block: BlockId,
    /// Guest register → current VReg mapping
    gpr_map: [VReg; 16],
    flags_vreg: VReg,
    rip_vreg: VReg,
}

impl IrBuilder {
    pub fn new(entry_rip: u64) -> Self {
        let mut block = IrBlock::new(entry_rip);
        
        // Initialize guest register mappings
        let mut gpr_map = [VReg::NONE; 16];
        for i in 0..16 {
            let vreg = block.alloc_vreg();
            gpr_map[i] = vreg;
            block.entry_block_mut().push(IrInstr {
                dst: vreg,
                op: IrOp::LoadGpr(i as u8),
                guest_rip: entry_rip,
                flags: IrFlags::empty(),
            });
        }
        
        let flags_vreg = block.alloc_vreg();
        block.entry_block_mut().push(IrInstr {
            dst: flags_vreg,
            op: IrOp::LoadFlags,
            guest_rip: entry_rip,
            flags: IrFlags::empty(),
        });
        
        let rip_vreg = block.alloc_vreg();
        
        Self {
            block,
            current_block: BlockId(0),
            gpr_map,
            flags_vreg,
            rip_vreg,
        }
    }
    
    /// Build IR from decoded instructions
    pub fn build(mut self, instrs: &[super::decoder::DecodedInstr]) -> IrBlock {
        for instr in instrs {
            self.translate_instr(instr);
        }
        
        // Update metadata
        self.block.meta.guest_instr_count = instrs.len();
        self.block.meta.ir_instr_count = self.block.blocks
            .iter()
            .map(|b| b.instrs.len())
            .sum();
        
        self.block
    }
    
    fn translate_instr(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Mnemonic;
        
        match instr.mnemonic {
            Mnemonic::Mov => self.translate_mov(instr),
            Mnemonic::Add => self.translate_alu(instr, AluOp::Add),
            Mnemonic::Sub => self.translate_alu(instr, AluOp::Sub),
            Mnemonic::And => self.translate_alu(instr, AluOp::And),
            Mnemonic::Or => self.translate_alu(instr, AluOp::Or),
            Mnemonic::Xor => self.translate_alu(instr, AluOp::Xor),
            Mnemonic::Cmp => self.translate_cmp(instr),
            Mnemonic::Test => self.translate_test(instr),
            Mnemonic::Push => self.translate_push(instr),
            Mnemonic::Pop => self.translate_pop(instr),
            Mnemonic::Jmp => self.translate_jmp(instr),
            Mnemonic::Jcc => self.translate_jcc(instr),
            Mnemonic::Call => self.translate_call(instr),
            Mnemonic::Ret => self.translate_ret(instr),
            Mnemonic::Lea => self.translate_lea(instr),
            Mnemonic::Nop => { /* nothing */ }
            Mnemonic::Hlt => self.emit_exit(ExitReason::Halt, instr.rip, instr.rip + instr.len as u64),
            Mnemonic::Int | Mnemonic::Int3 => self.translate_int(instr),
            Mnemonic::In => self.translate_in(instr),
            Mnemonic::Out => self.translate_out(instr),
            _ => {
                // Fallback: exit to interpreter
                self.emit_exit(ExitReason::Normal, instr.rip, instr.rip + instr.len as u64);
            }
        }
    }
    
    fn translate_mov(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        let src = self.load_operand(&instr.operands[1], instr.rip);
        self.store_operand(&instr.operands[0], src, instr.rip);
    }
    
    fn translate_alu(&mut self, instr: &super::decoder::DecodedInstr, op: AluOp) {
        let dst_op = &instr.operands[0];
        let src1 = self.load_operand(dst_op, instr.rip);
        let src2 = self.load_operand(&instr.operands[1], instr.rip);
        
        let result = self.emit_alu(op, src1, src2, instr.rip);
        self.store_operand(dst_op, result, instr.rip);
        
        // Update flags
        let flags = self.emit_flags(op, src1, src2, result, instr.rip);
        self.flags_vreg = flags;
    }
    
    fn translate_cmp(&mut self, instr: &super::decoder::DecodedInstr) {
        let src1 = self.load_operand(&instr.operands[0], instr.rip);
        let src2 = self.load_operand(&instr.operands[1], instr.rip);
        
        let flags = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: flags,
            op: IrOp::Cmp(src1, src2),
            guest_rip: instr.rip,
            flags: IrFlags::UPDATES_FLAGS,
        });
        self.flags_vreg = flags;
    }
    
    fn translate_test(&mut self, instr: &super::decoder::DecodedInstr) {
        let src1 = self.load_operand(&instr.operands[0], instr.rip);
        let src2 = self.load_operand(&instr.operands[1], instr.rip);
        
        let flags = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: flags,
            op: IrOp::Test(src1, src2),
            guest_rip: instr.rip,
            flags: IrFlags::UPDATES_FLAGS,
        });
        self.flags_vreg = flags;
    }
    
    fn translate_push(&mut self, instr: &super::decoder::DecodedInstr) {
        // RSP -= 8
        let rsp = self.gpr_map[4]; // RSP
        let eight = self.emit_const(8, instr.rip);
        let new_rsp = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: new_rsp,
            op: IrOp::Sub(rsp, eight),
            guest_rip: instr.rip,
            flags: IrFlags::empty(),
        });
        self.gpr_map[4] = new_rsp;
        
        // Store value
        let value = self.load_operand(&instr.operands[0], instr.rip);
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::Store64(new_rsp, value),
            guest_rip: instr.rip,
            flags: IrFlags::MEM_WRITE,
        });
    }
    
    fn translate_pop(&mut self, instr: &super::decoder::DecodedInstr) {
        let rsp = self.gpr_map[4];
        
        // Load value
        let value = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: value,
            op: IrOp::Load64(rsp),
            guest_rip: instr.rip,
            flags: IrFlags::MEM_READ,
        });
        
        // RSP += 8
        let eight = self.emit_const(8, instr.rip);
        let new_rsp = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: new_rsp,
            op: IrOp::Add(rsp, eight),
            guest_rip: instr.rip,
            flags: IrFlags::empty(),
        });
        self.gpr_map[4] = new_rsp;
        
        self.store_operand(&instr.operands[0], value, instr.rip);
    }
    
    fn translate_jmp(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        // Store all registers before exit
        let gpr_instrs: Vec<_> = (0..16).map(|i| IrInstr {
            dst: VReg::NONE,
            op: IrOp::StoreGpr(i as u8, self.gpr_map[i]),
            guest_rip: instr.rip,
            flags: IrFlags::empty(),
        }).collect();
        for ir in gpr_instrs {
            self.current_block_mut().push(ir);
        }
        
        let flags_vreg = self.flags_vreg;
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::StoreFlags(flags_vreg),
            guest_rip: instr.rip,
            flags: IrFlags::empty(),
        });
        
        // Set RIP to jump target
        match &instr.operands[0] {
            Operand::Rel(offset) => {
                let target = (instr.rip as i64 + instr.len as i64 + offset) as u64;
                let target_vreg = self.emit_const(target as i64, instr.rip);
                self.current_block_mut().push(IrInstr {
                    dst: VReg::NONE,
                    op: IrOp::StoreRip(target_vreg),
                    guest_rip: instr.rip,
                    flags: IrFlags::empty(),
                });
            }
            _ => {
                let target = self.load_operand(&instr.operands[0], instr.rip);
                self.current_block_mut().push(IrInstr {
                    dst: VReg::NONE,
                    op: IrOp::StoreRip(target),
                    guest_rip: instr.rip,
                    flags: IrFlags::empty(),
                });
            }
        }
        
        // Exit to execute at the new RIP
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::Exit(ExitReason::Normal),
            guest_rip: instr.rip,
            flags: IrFlags::TERMINATOR | IrFlags::SIDE_EFFECT,
        });
    }
    
    fn translate_jcc(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        // Get condition from opcode
        let cc = (instr.opcode & 0x0F) as u8;
        let cond = self.emit_condition(cc, instr.rip);
        
        if let Operand::Rel(offset) = &instr.operands[0] {
            let target = (instr.rip as i64 + instr.len as i64 + offset) as u64;
            let fallthrough = instr.rip + instr.len as u64;
            
            // Create target and fallthrough blocks
            let target_block = self.block.add_block(target);
            let fall_block = self.block.add_block(fallthrough);
            
            self.current_block_mut().push(IrInstr {
                dst: VReg::NONE,
                op: IrOp::Branch(cond, target_block, fall_block),
                guest_rip: instr.rip,
                flags: IrFlags::TERMINATOR,
            });
            
            self.block.meta.has_branches = true;
        }
    }
    
    fn translate_call(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        // Push return address
        let ret_addr = self.emit_const((instr.rip + instr.len as u64) as i64, instr.rip);
        let rsp = self.gpr_map[4];
        let eight = self.emit_const(8, instr.rip);
        let new_rsp = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: new_rsp,
            op: IrOp::Sub(rsp, eight),
            guest_rip: instr.rip,
            flags: IrFlags::empty(),
        });
        self.gpr_map[4] = new_rsp;
        
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::Store64(new_rsp, ret_addr),
            guest_rip: instr.rip,
            flags: IrFlags::MEM_WRITE,
        });
        
        // Jump to target
        match &instr.operands[0] {
            Operand::Rel(offset) => {
                let target = (instr.rip as i64 + instr.len as i64 + offset) as u64;
                self.current_block_mut().push(IrInstr {
                    dst: VReg::NONE,
                    op: IrOp::Call(target),
                    guest_rip: instr.rip,
                    flags: IrFlags::TERMINATOR | IrFlags::SIDE_EFFECT,
                });
            }
            _ => {
                let target = self.load_operand(&instr.operands[0], instr.rip);
                self.current_block_mut().push(IrInstr {
                    dst: VReg::NONE,
                    op: IrOp::CallIndirect(target),
                    guest_rip: instr.rip,
                    flags: IrFlags::TERMINATOR | IrFlags::SIDE_EFFECT,
                });
            }
        }
    }
    
    fn translate_ret(&mut self, instr: &super::decoder::DecodedInstr) {
        // Pop return address
        let rsp = self.gpr_map[4];
        let ret_addr = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: ret_addr,
            op: IrOp::Load64(rsp),
            guest_rip: instr.rip,
            flags: IrFlags::MEM_READ,
        });
        
        let eight = self.emit_const(8, instr.rip);
        let new_rsp = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: new_rsp,
            op: IrOp::Add(rsp, eight),
            guest_rip: instr.rip,
            flags: IrFlags::empty(),
        });
        self.gpr_map[4] = new_rsp;
        
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::Ret,
            guest_rip: instr.rip,
            flags: IrFlags::TERMINATOR,
        });
    }
    
    fn translate_lea(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        if let Operand::Mem(mem) = &instr.operands[1] {
            let addr = self.compute_effective_address(mem, instr.rip);
            self.store_operand(&instr.operands[0], addr, instr.rip);
        }
    }
    
    fn translate_int(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        let vector = match &instr.operands[0] {
            Operand::Imm(v) => *v as u8,
            _ => 3, // INT3
        };
        
        self.emit_exit(ExitReason::Interrupt(vector), instr.rip, instr.rip + instr.len as u64);
    }
    
    fn translate_in(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        let port = match &instr.operands[1] {
            Operand::Imm(v) => *v as u16,
            Operand::Reg(r) if r.index == 2 => { // DX
                // Dynamic port - need to exit
                self.emit_exit(ExitReason::IoRead(0, 0), instr.rip, instr.rip + instr.len as u64);
                return;
            }
            _ => 0,
        };
        
        let size = match &instr.operands[0] {
            Operand::Reg(r) => r.size,
            _ => 1,
        };
        
        self.emit_exit(ExitReason::IoRead(port, size), instr.rip, instr.rip + instr.len as u64);
        self.block.meta.has_io_ops = true;
    }
    
    fn translate_out(&mut self, instr: &super::decoder::DecodedInstr) {
        use super::decoder::Operand;
        
        let port = match &instr.operands[0] {
            Operand::Imm(v) => *v as u16,
            _ => 0,
        };
        
        let size = match &instr.operands[1] {
            Operand::Reg(r) => r.size,
            _ => 1,
        };
        
        self.emit_exit(ExitReason::IoWrite(port, size), instr.rip, instr.rip + instr.len as u64);
        self.block.meta.has_io_ops = true;
    }
    
    // Helper methods
    
    fn current_block_mut(&mut self) -> &mut IrBasicBlock {
        self.block.get_block_mut(self.current_block).unwrap()
    }
    
    fn emit_const(&mut self, value: i64, rip: u64) -> VReg {
        let vreg = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: vreg,
            op: IrOp::Const(value),
            guest_rip: rip,
            flags: IrFlags::empty(),
        });
        vreg
    }
    
    fn emit_exit(&mut self, reason: ExitReason, rip: u64, next_rip: u64) {
        // Collect GPR mappings first to avoid borrow conflict
        let gpr_instrs: Vec<_> = (0..16).map(|i| IrInstr {
            dst: VReg::NONE,
            op: IrOp::StoreGpr(i as u8, self.gpr_map[i]),
            guest_rip: rip,
            flags: IrFlags::empty(),
        }).collect();
        
        // Store all guest registers
        for instr in gpr_instrs {
            self.current_block_mut().push(instr);
        }
        
        // Store flags
        let flags_vreg = self.flags_vreg;
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::StoreFlags(flags_vreg),
            guest_rip: rip,
            flags: IrFlags::empty(),
        });
        
        // Store next RIP so execution can resume after exit
        let next_rip_vreg = self.block.alloc_vreg();
        self.current_block_mut().push(IrInstr {
            dst: next_rip_vreg,
            op: IrOp::Const(next_rip as i64),
            guest_rip: rip,
            flags: IrFlags::empty(),
        });
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::StoreRip(next_rip_vreg),
            guest_rip: rip,
            flags: IrFlags::empty(),
        });
        
        self.current_block_mut().push(IrInstr {
            dst: VReg::NONE,
            op: IrOp::Exit(reason),
            guest_rip: rip,
            flags: IrFlags::TERMINATOR | IrFlags::SIDE_EFFECT,
        });
    }
    
    fn load_operand(&mut self, op: &super::decoder::Operand, rip: u64) -> VReg {
        use super::decoder::Operand;
        
        match op {
            Operand::None => VReg::NONE,
            Operand::Reg(r) => {
                if r.kind == super::decoder::RegKind::Gpr {
                    self.gpr_map[r.index as usize]
                } else {
                    self.emit_const(0, rip)
                }
            }
            Operand::Imm(v) => self.emit_const(*v, rip),
            Operand::Mem(mem) => {
                let addr = self.compute_effective_address(mem, rip);
                let vreg = self.block.alloc_vreg();
                let load_op = match mem.size {
                    1 => IrOp::Load8(addr),
                    2 => IrOp::Load16(addr),
                    4 => IrOp::Load32(addr),
                    _ => IrOp::Load64(addr),
                };
                self.current_block_mut().push(IrInstr {
                    dst: vreg,
                    op: load_op,
                    guest_rip: rip,
                    flags: IrFlags::MEM_READ,
                });
                self.block.meta.has_memory_ops = true;
                vreg
            }
            Operand::Rel(offset) => {
                self.emit_const(*offset, rip)
            }
            Operand::Far { .. } => self.emit_const(0, rip),
        }
    }
    
    fn store_operand(&mut self, op: &super::decoder::Operand, value: VReg, rip: u64) {
        use super::decoder::Operand;
        
        match op {
            Operand::Reg(r) => {
                if r.kind == super::decoder::RegKind::Gpr {
                    self.gpr_map[r.index as usize] = value;
                }
            }
            Operand::Mem(mem) => {
                let addr = self.compute_effective_address(mem, rip);
                let store_op = match mem.size {
                    1 => IrOp::Store8(addr, value),
                    2 => IrOp::Store16(addr, value),
                    4 => IrOp::Store32(addr, value),
                    _ => IrOp::Store64(addr, value),
                };
                self.current_block_mut().push(IrInstr {
                    dst: VReg::NONE,
                    op: store_op,
                    guest_rip: rip,
                    flags: IrFlags::MEM_WRITE,
                });
                self.block.meta.has_memory_ops = true;
            }
            _ => {}
        }
    }
    
    fn compute_effective_address(&mut self, mem: &super::decoder::MemOp, rip: u64) -> VReg {
        let mut addr = if let Some(base) = &mem.base {
            if base.kind == super::decoder::RegKind::Rip {
                self.emit_const(rip as i64, rip)
            } else {
                self.gpr_map[base.index as usize]
            }
        } else {
            self.emit_const(0, rip)
        };
        
        if let Some(index) = &mem.index {
            let idx = self.gpr_map[index.index as usize];
            if mem.scale > 1 {
                let scale = self.emit_const(mem.scale as i64, rip);
                let scaled = self.block.alloc_vreg();
                self.current_block_mut().push(IrInstr {
                    dst: scaled,
                    op: IrOp::Mul(idx, scale),
                    guest_rip: rip,
                    flags: IrFlags::empty(),
                });
                let new_addr = self.block.alloc_vreg();
                self.current_block_mut().push(IrInstr {
                    dst: new_addr,
                    op: IrOp::Add(addr, scaled),
                    guest_rip: rip,
                    flags: IrFlags::empty(),
                });
                addr = new_addr;
            } else {
                let new_addr = self.block.alloc_vreg();
                self.current_block_mut().push(IrInstr {
                    dst: new_addr,
                    op: IrOp::Add(addr, idx),
                    guest_rip: rip,
                    flags: IrFlags::empty(),
                });
                addr = new_addr;
            }
        }
        
        if mem.disp != 0 {
            let disp = self.emit_const(mem.disp, rip);
            let new_addr = self.block.alloc_vreg();
            self.current_block_mut().push(IrInstr {
                dst: new_addr,
                op: IrOp::Add(addr, disp),
                guest_rip: rip,
                flags: IrFlags::empty(),
            });
            addr = new_addr;
        }
        
        addr
    }
    
    fn emit_alu(&mut self, op: AluOp, src1: VReg, src2: VReg, rip: u64) -> VReg {
        let result = self.block.alloc_vreg();
        let ir_op = match op {
            AluOp::Add => IrOp::Add(src1, src2),
            AluOp::Sub => IrOp::Sub(src1, src2),
            AluOp::And => IrOp::And(src1, src2),
            AluOp::Or => IrOp::Or(src1, src2),
            AluOp::Xor => IrOp::Xor(src1, src2),
        };
        self.current_block_mut().push(IrInstr {
            dst: result,
            op: ir_op,
            guest_rip: rip,
            flags: IrFlags::UPDATES_FLAGS,
        });
        result
    }
    
    fn emit_flags(&mut self, _op: AluOp, _src1: VReg, _src2: VReg, result: VReg, rip: u64) -> VReg {
        // Simplified: compute flags from result
        let flags = self.block.alloc_vreg();
        let zero = self.emit_const(0, rip);
        self.current_block_mut().push(IrInstr {
            dst: flags,
            op: IrOp::Cmp(result, zero),
            guest_rip: rip,
            flags: IrFlags::empty(),
        });
        flags
    }
    
    fn emit_condition(&mut self, cc: u8, rip: u64) -> VReg {
        let flags = self.flags_vreg;
        let cond = self.block.alloc_vreg();
        
        // Extract appropriate flag based on condition code
        let flag_op = match cc {
            0x0 => IrOp::GetOF(flags), // O
            0x1 => IrOp::GetOF(flags), // NO (negated later)
            0x2 => IrOp::GetCF(flags), // B/C
            0x3 => IrOp::GetCF(flags), // NB/NC
            0x4 => IrOp::GetZF(flags), // E/Z
            0x5 => IrOp::GetZF(flags), // NE/NZ
            0x6 => IrOp::GetCF(flags), // BE (CF|ZF)
            0x7 => IrOp::GetCF(flags), // NBE
            0x8 => IrOp::GetSF(flags), // S
            0x9 => IrOp::GetSF(flags), // NS
            0xA => IrOp::GetPF(flags), // P
            0xB => IrOp::GetPF(flags), // NP
            0xC => IrOp::GetSF(flags), // L (SF!=OF)
            0xD => IrOp::GetSF(flags), // NL
            0xE => IrOp::GetZF(flags), // LE (ZF|(SF!=OF))
            0xF => IrOp::GetZF(flags), // NLE
            _ => IrOp::Const(0),
        };
        
        self.current_block_mut().push(IrInstr {
            dst: cond,
            op: flag_op,
            guest_rip: rip,
            flags: IrFlags::READS_FLAGS,
        });
        
        cond
    }
}

#[derive(Debug, Clone, Copy)]
enum AluOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vreg() {
        assert!(!VReg::NONE.is_valid());
        assert!(VReg(0).is_valid());
    }
    
    #[test]
    fn test_ir_block() {
        let mut block = IrBlock::new(0x1000);
        let v1 = block.alloc_vreg();
        let v2 = block.alloc_vreg();
        assert_eq!(v1.0, 0);
        assert_eq!(v2.0, 1);
    }
}
