//! ReadyNow! Persistence
//!
//! Pre-warm JIT cache from persisted data for instant startup.
//! Three persistence formats with different compatibility guarantees:
//!
//! 1. **Profile** - Runtime profiling data (branch stats, call targets, hot blocks)
//!    - Full forward AND backward compatibility
//!    - Small size, fast to load
//!    - Used to guide JIT compilation decisions
//!
//! 2. **RI (Runtime Intermediate)** - Platform-neutral IR representation
//!    - Backward compatible (old RI works on new JIT)
//!    - Medium size, moderate load time
//!    - Skip decoding, go straight to optimization
//!
//! 3. **Native Code** - Pre-compiled machine code
//!    - Same-generation only (version must match exactly)
//!    - Largest size, instant load (mmap)
//!    - Zero warmup - execute immediately

use std::collections::HashMap;
use std::io::{Read, Write, Cursor};
use std::path::Path;
use std::fs::File;

use super::{JitResult, JitError};
use super::ir::{IrBlock, IrBasicBlock, IrInstr, IrOp, VReg, ExitReason, IrFlags, BlockId};
use super::profile::ProfileDb;
use super::cache::{CompiledBlock, CompileTier, compute_checksum};
use std::sync::atomic::AtomicU64;

/// ReadyNow! cache version
pub const READYNOW_VERSION: u32 = 1;

/// Profile format magic
pub const PROFILE_MAGIC: &[u8; 4] = b"NVMP";

/// RI format magic  
pub const RI_MAGIC: &[u8; 4] = b"NVRI";

/// Native code format magic
pub const NATIVE_MAGIC: &[u8; 4] = b"NVNC";

/// ReadyNow! cache manager
pub struct ReadyNowCache {
    /// Base directory for cache files
    cache_dir: String,
    /// VM instance ID (for isolation)
    instance_id: String,
    /// JIT version (for native code compatibility)
    jit_version: u32,
    /// Target architecture
    arch: Architecture,
}

/// Target architecture
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    Aarch64,
}

impl Architecture {
    fn to_u8(self) -> u8 {
        match self {
            Architecture::X86_64 => 0,
            Architecture::Aarch64 => 1,
        }
    }
    
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Architecture::X86_64),
            1 => Some(Architecture::Aarch64),
            _ => None,
        }
    }
}

impl ReadyNowCache {
    pub fn new(cache_dir: &str, instance_id: &str) -> Self {
        Self {
            cache_dir: cache_dir.to_string(),
            instance_id: instance_id.to_string(),
            jit_version: READYNOW_VERSION,
            arch: Architecture::X86_64,
        }
    }
    
    // ========================================================================
    // Profile Persistence (Full Compatibility)
    // ========================================================================
    
    /// Save profile data
    pub fn save_profile(&self, profile: &ProfileDb) -> JitResult<()> {
        let path = self.profile_path();
        let data = self.serialize_profile(profile)?;
        
        let mut file = File::create(&path)
            .map_err(|_| JitError::IoError)?;
        file.write_all(&data)
            .map_err(|_| JitError::IoError)?;
        
        Ok(())
    }
    
    /// Load profile data
    pub fn load_profile(&self) -> JitResult<ProfileDb> {
        let path = self.profile_path();
        let mut file = File::open(&path)
            .map_err(|_| JitError::IoError)?;
        
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|_| JitError::IoError)?;
        
        self.deserialize_profile(&data)
    }
    
    fn serialize_profile(&self, profile: &ProfileDb) -> JitResult<Vec<u8>> {
        let mut data = Vec::new();
        
        // Header
        data.extend_from_slice(PROFILE_MAGIC);
        data.extend_from_slice(&READYNOW_VERSION.to_le_bytes());
        
        // Profile data (delegated to ProfileDb)
        data.extend_from_slice(&profile.serialize());
        
        Ok(data)
    }
    
    fn deserialize_profile(&self, data: &[u8]) -> JitResult<ProfileDb> {
        if data.len() < 8 {
            return Err(JitError::InvalidFormat);
        }
        
        if &data[0..4] != PROFILE_MAGIC {
            return Err(JitError::InvalidFormat);
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
        
        // Profile format is forward/backward compatible
        // Version differences handled in ProfileDb::deserialize
        
        ProfileDb::deserialize(&data[8..])
            .ok_or(JitError::InvalidFormat)
    }
    
    fn profile_path(&self) -> String {
        format!("{}/{}.profile", self.cache_dir, self.instance_id)
    }
    
    // ========================================================================
    // RI Persistence (Backward Compatible)
    // ========================================================================
    
    /// Save IR blocks
    pub fn save_ri(&self, blocks: &HashMap<u64, IrBlock>) -> JitResult<()> {
        let path = self.ri_path();
        let data = self.serialize_ri(blocks)?;
        
        let mut file = File::create(&path)
            .map_err(|_| JitError::IoError)?;
        file.write_all(&data)
            .map_err(|_| JitError::IoError)?;
        
        Ok(())
    }
    
    /// Load IR blocks
    pub fn load_ri(&self) -> JitResult<HashMap<u64, IrBlock>> {
        let path = self.ri_path();
        let mut file = File::open(&path)
            .map_err(|_| JitError::IoError)?;
        
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|_| JitError::IoError)?;
        
        self.deserialize_ri(&data)
    }
    
    fn serialize_ri(&self, blocks: &HashMap<u64, IrBlock>) -> JitResult<Vec<u8>> {
        let mut data = Vec::new();
        
        // Header
        data.extend_from_slice(RI_MAGIC);
        data.extend_from_slice(&READYNOW_VERSION.to_le_bytes());
        
        // Block count
        data.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        
        // Each block
        for (&rip, block) in blocks {
            data.extend_from_slice(&rip.to_le_bytes());
            self.serialize_ir_block(&mut data, block)?;
        }
        
        Ok(data)
    }
    
    fn serialize_ir_block(&self, data: &mut Vec<u8>, block: &IrBlock) -> JitResult<()> {
        // Entry RIP
        data.extend_from_slice(&block.entry_rip.to_le_bytes());
        
        // Guest size
        data.extend_from_slice(&(block.guest_size as u32).to_le_bytes());
        
        // Next VReg ID
        data.extend_from_slice(&block.next_vreg.to_le_bytes());
        
        // Entry block ID
        data.extend_from_slice(&block.entry_block.0.to_le_bytes());
        
        // Basic block count
        data.extend_from_slice(&(block.blocks.len() as u32).to_le_bytes());
        
        for bb in &block.blocks {
            self.serialize_basic_block(data, bb)?;
        }
        
        Ok(())
    }
    
    fn serialize_basic_block(&self, data: &mut Vec<u8>, bb: &IrBasicBlock) -> JitResult<()> {
        // Block ID
        data.extend_from_slice(&bb.id.0.to_le_bytes());
        
        // Entry RIP
        data.extend_from_slice(&bb.entry_rip.to_le_bytes());
        
        // Predecessor count and IDs
        data.extend_from_slice(&(bb.predecessors.len() as u32).to_le_bytes());
        for pred in &bb.predecessors {
            data.extend_from_slice(&pred.0.to_le_bytes());
        }
        
        // Successor count and IDs
        data.extend_from_slice(&(bb.successors.len() as u32).to_le_bytes());
        for succ in &bb.successors {
            data.extend_from_slice(&succ.0.to_le_bytes());
        }
        
        // Instruction count
        data.extend_from_slice(&(bb.instrs.len() as u32).to_le_bytes());
        
        for instr in &bb.instrs {
            self.serialize_ir_instr(data, instr)?;
        }
        
        Ok(())
    }
    
    fn serialize_ir_instr(&self, data: &mut Vec<u8>, instr: &IrInstr) -> JitResult<()> {
        // Destination VReg
        data.extend_from_slice(&instr.dst.0.to_le_bytes());
        
        // Guest RIP
        data.extend_from_slice(&instr.guest_rip.to_le_bytes());
        
        // Flags
        data.extend_from_slice(&instr.flags.bits().to_le_bytes());
        
        // Operation
        self.serialize_ir_op(data, &instr.op)
    }
    
    fn serialize_ir_op(&self, data: &mut Vec<u8>, op: &IrOp) -> JitResult<()> {
        // Opcode byte followed by operands (SSA style - no dst in op)
        match op {
            IrOp::Nop => data.push(0),
            IrOp::Const(val) => {
                data.push(1);
                data.extend_from_slice(&val.to_le_bytes());
            }
            IrOp::ConstF64(val) => {
                data.push(2);
                data.extend_from_slice(&val.to_le_bytes());
            }
            IrOp::LoadGpr(idx) => {
                data.push(3);
                data.push(*idx);
            }
            IrOp::StoreGpr(idx, val) => {
                data.push(4);
                data.push(*idx);
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::LoadFlags => data.push(5),
            IrOp::StoreFlags(val) => {
                data.push(6);
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::LoadRip => data.push(7),
            IrOp::StoreRip(val) => {
                data.push(8);
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Load8(addr) => {
                data.push(10);
                data.extend_from_slice(&addr.0.to_le_bytes());
            }
            IrOp::Load16(addr) => {
                data.push(11);
                data.extend_from_slice(&addr.0.to_le_bytes());
            }
            IrOp::Load32(addr) => {
                data.push(12);
                data.extend_from_slice(&addr.0.to_le_bytes());
            }
            IrOp::Load64(addr) => {
                data.push(13);
                data.extend_from_slice(&addr.0.to_le_bytes());
            }
            IrOp::Store8(addr, val) => {
                data.push(14);
                data.extend_from_slice(&addr.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Store16(addr, val) => {
                data.push(15);
                data.extend_from_slice(&addr.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Store32(addr, val) => {
                data.push(16);
                data.extend_from_slice(&addr.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Store64(addr, val) => {
                data.push(17);
                data.extend_from_slice(&addr.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Add(a, b) => {
                data.push(20);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Sub(a, b) => {
                data.push(21);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Mul(a, b) => {
                data.push(22);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::IMul(a, b) => {
                data.push(23);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Div(a, b) => {
                data.push(24);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::IDiv(a, b) => {
                data.push(25);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Neg(a) => {
                data.push(26);
                data.extend_from_slice(&a.0.to_le_bytes());
            }
            IrOp::And(a, b) => {
                data.push(30);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Or(a, b) => {
                data.push(31);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Xor(a, b) => {
                data.push(32);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Not(a) => {
                data.push(33);
                data.extend_from_slice(&a.0.to_le_bytes());
            }
            IrOp::Shl(a, b) => {
                data.push(40);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Shr(a, b) => {
                data.push(41);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Sar(a, b) => {
                data.push(42);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Rol(a, b) => {
                data.push(43);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Ror(a, b) => {
                data.push(44);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Cmp(a, b) => {
                data.push(50);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Test(a, b) => {
                data.push(51);
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::GetCF(flags) => {
                data.push(52);
                data.extend_from_slice(&flags.0.to_le_bytes());
            }
            IrOp::GetZF(flags) => {
                data.push(53);
                data.extend_from_slice(&flags.0.to_le_bytes());
            }
            IrOp::GetSF(flags) => {
                data.push(54);
                data.extend_from_slice(&flags.0.to_le_bytes());
            }
            IrOp::GetOF(flags) => {
                data.push(55);
                data.extend_from_slice(&flags.0.to_le_bytes());
            }
            IrOp::GetPF(flags) => {
                data.push(56);
                data.extend_from_slice(&flags.0.to_le_bytes());
            }
            IrOp::Select(cond, t, f) => {
                data.push(60);
                data.extend_from_slice(&cond.0.to_le_bytes());
                data.extend_from_slice(&t.0.to_le_bytes());
                data.extend_from_slice(&f.0.to_le_bytes());
            }
            IrOp::Sext8(v) => {
                data.push(70);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Sext16(v) => {
                data.push(71);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Sext32(v) => {
                data.push(72);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Zext8(v) => {
                data.push(73);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Zext16(v) => {
                data.push(74);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Zext32(v) => {
                data.push(75);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Trunc8(v) => {
                data.push(76);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Trunc16(v) => {
                data.push(77);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            IrOp::Trunc32(v) => {
                data.push(78);
                data.extend_from_slice(&v.0.to_le_bytes());
            }
            // Control flow
            IrOp::Jump(target) => {
                data.push(100);
                data.extend_from_slice(&target.0.to_le_bytes());
            }
            IrOp::Branch(cond, t, f) => {
                data.push(101);
                data.extend_from_slice(&cond.0.to_le_bytes());
                data.extend_from_slice(&t.0.to_le_bytes());
                data.extend_from_slice(&f.0.to_le_bytes());
            }
            IrOp::Call(addr) => {
                data.push(102);
                data.extend_from_slice(&addr.to_le_bytes());
            }
            IrOp::CallIndirect(target) => {
                data.push(103);
                data.extend_from_slice(&target.0.to_le_bytes());
            }
            IrOp::Ret => data.push(104),
            // Special
            IrOp::Syscall => data.push(110),
            IrOp::Cpuid => data.push(111),
            IrOp::Rdtsc => data.push(112),
            IrOp::Hlt => data.push(113),
            // I/O
            IrOp::In8(port) => {
                data.push(120);
                data.extend_from_slice(&port.0.to_le_bytes());
            }
            IrOp::In16(port) => {
                data.push(121);
                data.extend_from_slice(&port.0.to_le_bytes());
            }
            IrOp::In32(port) => {
                data.push(122);
                data.extend_from_slice(&port.0.to_le_bytes());
            }
            IrOp::Out8(port, val) => {
                data.push(123);
                data.extend_from_slice(&port.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Out16(port, val) => {
                data.push(124);
                data.extend_from_slice(&port.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            IrOp::Out32(port, val) => {
                data.push(125);
                data.extend_from_slice(&port.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            // Phi
            IrOp::Phi(entries) => {
                data.push(130);
                data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                for (block, vreg) in entries {
                    data.extend_from_slice(&block.0.to_le_bytes());
                    data.extend_from_slice(&vreg.0.to_le_bytes());
                }
            }
            // Exit
            IrOp::Exit(reason) => {
                data.push(200);
                self.serialize_exit_reason(data, reason)?;
            }
        }
        
        Ok(())
    }
    
    fn serialize_exit_reason(&self, data: &mut Vec<u8>, reason: &ExitReason) -> JitResult<()> {
        match reason {
            ExitReason::Normal => data.push(0),
            ExitReason::Halt => data.push(1),
            ExitReason::Interrupt(vec) => {
                data.push(2);
                data.push(*vec);
            }
            ExitReason::Exception(vec, code) => {
                data.push(3);
                data.push(*vec);
                data.extend_from_slice(&code.to_le_bytes());
            }
            ExitReason::IoRead(port, size) => {
                data.push(4);
                data.extend_from_slice(&port.to_le_bytes());
                data.push(*size);
            }
            ExitReason::IoWrite(port, size) => {
                data.push(5);
                data.extend_from_slice(&port.to_le_bytes());
                data.push(*size);
            }
            ExitReason::Mmio(addr, size, is_write) => {
                data.push(6);
                data.extend_from_slice(&addr.to_le_bytes());
                data.push(*size);
                data.push(*is_write as u8);
            }
            ExitReason::Hypercall => data.push(7),
            ExitReason::Reset => data.push(8),
        }
        Ok(())
    }
    
    fn deserialize_ri(&self, data: &[u8]) -> JitResult<HashMap<u64, IrBlock>> {
        if data.len() < 12 {
            return Err(JitError::InvalidFormat);
        }
        
        if &data[0..4] != RI_MAGIC {
            return Err(JitError::InvalidFormat);
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
        
        // RI format is backward compatible only
        if version > READYNOW_VERSION {
            return Err(JitError::IncompatibleVersion);
        }
        
        let block_count = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let mut offset = 12;
        let mut blocks = HashMap::new();
        
        for _ in 0..block_count {
            if offset + 8 > data.len() {
                break;
            }
            
            let rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            offset += 8;
            
            if let Some((block, new_offset)) = self.deserialize_ir_block(data, offset) {
                blocks.insert(rip, block);
                offset = new_offset;
            } else {
                break;
            }
        }
        
        Ok(blocks)
    }
    
    fn deserialize_ir_block(&self, data: &[u8], mut offset: usize) -> Option<(IrBlock, usize)> {
        if offset + 24 > data.len() {
            return None;
        }
        
        let entry_rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
        offset += 8;
        
        let guest_size = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let next_vreg = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?);
        offset += 4;
        
        let entry_block = BlockId(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
        offset += 4;
        
        let bb_count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let mut blocks = Vec::with_capacity(bb_count);
        
        for _ in 0..bb_count {
            let (bb, new_offset) = self.deserialize_basic_block(data, offset)?;
            blocks.push(bb);
            offset = new_offset;
        }
        
        let block = IrBlock {
            entry_rip,
            guest_size,
            blocks,
            entry_block,
            next_vreg,
            meta: Default::default(),
        };
        
        Some((block, offset))
    }
    
    fn deserialize_basic_block(&self, data: &[u8], mut offset: usize) -> Option<(IrBasicBlock, usize)> {
        if offset + 16 > data.len() {
            return None;
        }
        
        let id = BlockId(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
        offset += 4;
        
        let entry_rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
        offset += 8;
        
        // Predecessors
        let pred_count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        let mut predecessors = Vec::with_capacity(pred_count);
        for _ in 0..pred_count {
            if offset + 4 > data.len() { return None; }
            predecessors.push(BlockId(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?)));
            offset += 4;
        }
        
        // Successors
        let succ_count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        let mut successors = Vec::with_capacity(succ_count);
        for _ in 0..succ_count {
            if offset + 4 > data.len() { return None; }
            successors.push(BlockId(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?)));
            offset += 4;
        }
        
        // Instructions
        let instr_count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let mut instrs = Vec::with_capacity(instr_count);
        
        for _ in 0..instr_count {
            let (instr, new_offset) = self.deserialize_ir_instr(data, offset)?;
            instrs.push(instr);
            offset = new_offset;
        }
        
        Some((IrBasicBlock { id, instrs, predecessors, successors, entry_rip }, offset))
    }
    
    fn deserialize_ir_instr(&self, data: &[u8], mut offset: usize) -> Option<(IrInstr, usize)> {
        if offset + 14 > data.len() {
            return None;
        }
        
        // Destination VReg
        let dst = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
        offset += 4;
        
        // Guest RIP
        let guest_rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
        offset += 8;
        
        // Flags
        let flags = IrFlags::from_bits_truncate(u16::from_le_bytes(data[offset..offset+2].try_into().ok()?));
        offset += 2;
        
        let (op, new_offset) = self.deserialize_ir_op(data, offset)?;
        
        Some((IrInstr { dst, op, guest_rip, flags }, new_offset))
    }
    
    fn deserialize_ir_op(&self, data: &[u8], mut offset: usize) -> Option<(IrOp, usize)> {
        if offset >= data.len() {
            return None;
        }
        
        let opcode = data[offset];
        offset += 1;
        
        let op = match opcode {
            0 => IrOp::Nop,
            1 => {
                let val = i64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
                offset += 8;
                IrOp::Const(val)
            }
            2 => {
                let val = f64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
                offset += 8;
                IrOp::ConstF64(val)
            }
            3 => {
                let idx = data[offset];
                offset += 1;
                IrOp::LoadGpr(idx)
            }
            4 => {
                let idx = data[offset];
                offset += 1;
                let val = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::StoreGpr(idx, val)
            }
            5 => IrOp::LoadFlags,
            6 => {
                let val = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::StoreFlags(val)
            }
            7 => IrOp::LoadRip,
            8 => {
                let val = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::StoreRip(val)
            }
            10 => {
                let addr = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Load8(addr)
            }
            11 => {
                let addr = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Load16(addr)
            }
            12 => {
                let addr = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Load32(addr)
            }
            13 => {
                let addr = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Load64(addr)
            }
            14 | 15 | 16 | 17 => {
                let addr = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let val = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                match opcode {
                    14 => IrOp::Store8(addr, val),
                    15 => IrOp::Store16(addr, val),
                    16 => IrOp::Store32(addr, val),
                    _ => IrOp::Store64(addr, val),
                }
            }
            20 | 21 | 22 | 23 | 24 | 25 => {
                let a = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let b = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                match opcode {
                    20 => IrOp::Add(a, b),
                    21 => IrOp::Sub(a, b),
                    22 => IrOp::Mul(a, b),
                    23 => IrOp::IMul(a, b),
                    24 => IrOp::Div(a, b),
                    _ => IrOp::IDiv(a, b),
                }
            }
            26 => {
                let a = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Neg(a)
            }
            30 | 31 | 32 => {
                let a = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let b = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                match opcode {
                    30 => IrOp::And(a, b),
                    31 => IrOp::Or(a, b),
                    _ => IrOp::Xor(a, b),
                }
            }
            33 => {
                let a = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Not(a)
            }
            40 | 41 | 42 | 43 | 44 => {
                let a = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let b = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                match opcode {
                    40 => IrOp::Shl(a, b),
                    41 => IrOp::Shr(a, b),
                    42 => IrOp::Sar(a, b),
                    43 => IrOp::Rol(a, b),
                    _ => IrOp::Ror(a, b),
                }
            }
            50 | 51 => {
                let a = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let b = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                if opcode == 50 { IrOp::Cmp(a, b) } else { IrOp::Test(a, b) }
            }
            52 | 53 | 54 | 55 | 56 => {
                let flags = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                match opcode {
                    52 => IrOp::GetCF(flags),
                    53 => IrOp::GetZF(flags),
                    54 => IrOp::GetSF(flags),
                    55 => IrOp::GetOF(flags),
                    _ => IrOp::GetPF(flags),
                }
            }
            60 => {
                let cond = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let t = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                let f = VReg(u32::from_le_bytes(data[offset+8..offset+12].try_into().ok()?));
                offset += 12;
                IrOp::Select(cond, t, f)
            }
            70..=78 => {
                let v = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                match opcode {
                    70 => IrOp::Sext8(v),
                    71 => IrOp::Sext16(v),
                    72 => IrOp::Sext32(v),
                    73 => IrOp::Zext8(v),
                    74 => IrOp::Zext16(v),
                    75 => IrOp::Zext32(v),
                    76 => IrOp::Trunc8(v),
                    77 => IrOp::Trunc16(v),
                    _ => IrOp::Trunc32(v),
                }
            }
            100 => {
                let target = BlockId(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::Jump(target)
            }
            101 => {
                let cond = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let t = BlockId(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                let f = BlockId(u32::from_le_bytes(data[offset+8..offset+12].try_into().ok()?));
                offset += 12;
                IrOp::Branch(cond, t, f)
            }
            102 => {
                let addr = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
                offset += 8;
                IrOp::Call(addr)
            }
            103 => {
                let target = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                IrOp::CallIndirect(target)
            }
            104 => IrOp::Ret,
            110 => IrOp::Syscall,
            111 => IrOp::Cpuid,
            112 => IrOp::Rdtsc,
            113 => IrOp::Hlt,
            120 | 121 | 122 => {
                let port = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                match opcode {
                    120 => IrOp::In8(port),
                    121 => IrOp::In16(port),
                    _ => IrOp::In32(port),
                }
            }
            123 | 124 | 125 => {
                let port = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let val = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                match opcode {
                    123 => IrOp::Out8(port, val),
                    124 => IrOp::Out16(port, val),
                    _ => IrOp::Out32(port, val),
                }
            }
            130 => {
                let count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
                offset += 4;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let block = BlockId(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                    let vreg = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                    offset += 8;
                    entries.push((block, vreg));
                }
                IrOp::Phi(entries)
            }
            200 => {
                let (reason, new_offset) = self.deserialize_exit_reason(data, offset)?;
                offset = new_offset;
                IrOp::Exit(reason)
            }
            _ => IrOp::Nop,
        };
        
        Some((op, offset))
    }
    
    fn deserialize_exit_reason(&self, data: &[u8], mut offset: usize) -> Option<(ExitReason, usize)> {
        if offset >= data.len() {
            return None;
        }
        
        let kind = data[offset];
        offset += 1;
        
        let reason = match kind {
            0 => ExitReason::Normal,
            1 => ExitReason::Halt,
            2 => {
                let vec = data[offset];
                offset += 1;
                ExitReason::Interrupt(vec)
            }
            3 => {
                let vec = data[offset];
                offset += 1;
                let code = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?);
                offset += 4;
                ExitReason::Exception(vec, code)
            }
            4 => {
                let port = u16::from_le_bytes(data[offset..offset+2].try_into().ok()?);
                offset += 2;
                let size = data[offset];
                offset += 1;
                ExitReason::IoRead(port, size)
            }
            5 => {
                let port = u16::from_le_bytes(data[offset..offset+2].try_into().ok()?);
                offset += 2;
                let size = data[offset];
                offset += 1;
                ExitReason::IoWrite(port, size)
            }
            6 => {
                let addr = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
                offset += 8;
                let size = data[offset];
                offset += 1;
                let is_write = data[offset] != 0;
                offset += 1;
                ExitReason::Mmio(addr, size, is_write)
            }
            7 => ExitReason::Hypercall,
            8 => ExitReason::Reset,
            _ => ExitReason::Normal,
        };
        
        Some((reason, offset))
    }
    
    fn ri_path(&self) -> String {
        format!("{}/{}.ri", self.cache_dir, self.instance_id)
    }
    
    // ========================================================================
    // Native Code Persistence (Same-Generation Only)
    // ========================================================================
    
    /// Save compiled native code
    pub fn save_native(&self, blocks: &HashMap<u64, CompiledBlock>) -> JitResult<()> {
        let path = self.native_path();
        let data = self.serialize_native(blocks)?;
        
        let mut file = File::create(&path)
            .map_err(|_| JitError::IoError)?;
        file.write_all(&data)
            .map_err(|_| JitError::IoError)?;
        
        Ok(())
    }
    
    /// Load compiled native code
    /// Returns None if version mismatch (must recompile)
    pub fn load_native(&self) -> JitResult<Option<HashMap<u64, NativeBlockInfo>>> {
        let path = self.native_path();
        let mut file = File::open(&path)
            .map_err(|_| JitError::IoError)?;
        
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|_| JitError::IoError)?;
        
        self.deserialize_native(&data)
    }
    
    fn serialize_native(&self, blocks: &HashMap<u64, CompiledBlock>) -> JitResult<Vec<u8>> {
        let mut data = Vec::new();
        
        // Header
        data.extend_from_slice(NATIVE_MAGIC);
        data.extend_from_slice(&self.jit_version.to_le_bytes());
        data.push(self.arch.to_u8());
        data.extend_from_slice(&[0, 0, 0]); // Padding
        
        // Block count
        data.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        
        // Each block
        for (&rip, block) in blocks {
            // Guest RIP
            data.extend_from_slice(&rip.to_le_bytes());
            // Guest size
            data.extend_from_slice(&block.guest_size.to_le_bytes());
            // Host size
            data.extend_from_slice(&block.host_size.to_le_bytes());
            // Tier
            data.push(match block.tier {
                CompileTier::Interpreter => 0,
                CompileTier::S1 => 1,
                CompileTier::S2 => 2,
            });
            // Guest instruction count
            data.extend_from_slice(&block.guest_instrs.to_le_bytes());
            // Guest checksum
            data.extend_from_slice(&block.guest_checksum.to_le_bytes());
            // Padding
            data.extend_from_slice(&[0, 0, 0]);
            
            // Native code
            if !block.host_code.is_null() {
                let code = unsafe {
                    std::slice::from_raw_parts(block.host_code, block.host_size as usize)
                };
                data.extend_from_slice(code);
            }
        }
        
        Ok(data)
    }
    
    fn deserialize_native(&self, data: &[u8]) -> JitResult<Option<HashMap<u64, NativeBlockInfo>>> {
        if data.len() < 16 {
            return Err(JitError::InvalidFormat);
        }
        
        if &data[0..4] != NATIVE_MAGIC {
            return Err(JitError::InvalidFormat);
        }
        
        let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
        let arch = Architecture::from_u8(data[8])
            .ok_or(JitError::InvalidFormat)?;
        
        // Native code requires exact version AND architecture match
        if version != self.jit_version || arch != self.arch {
            return Ok(None); // Must recompile
        }
        
        let block_count = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let mut offset = 16;
        let mut blocks = HashMap::new();
        
        for _ in 0..block_count {
            if offset + 32 > data.len() {
                break;
            }
            
            let guest_rip = u64::from_le_bytes(data[offset..offset+8].try_into().unwrap());
            let guest_size = u32::from_le_bytes(data[offset+8..offset+12].try_into().unwrap());
            let host_size = u32::from_le_bytes(data[offset+12..offset+16].try_into().unwrap());
            let tier = match data[offset+16] {
                0 => CompileTier::Interpreter,
                1 => CompileTier::S1,
                _ => CompileTier::S2,
            };
            let guest_instrs = u32::from_le_bytes(data[offset+17..offset+21].try_into().unwrap());
            let guest_checksum = u64::from_le_bytes(data[offset+21..offset+29].try_into().unwrap());
            
            offset += 32;
            
            // Native code
            let code_end = offset + host_size as usize;
            if code_end > data.len() {
                break;
            }
            
            let native_code = data[offset..code_end].to_vec();
            offset = code_end;
            
            blocks.insert(guest_rip, NativeBlockInfo {
                guest_rip,
                guest_size,
                host_size,
                tier,
                guest_instrs,
                guest_checksum,
                native_code,
            });
        }
        
        Ok(Some(blocks))
    }
    
    fn native_path(&self) -> String {
        format!("{}/{}.{}.native", self.cache_dir, self.instance_id, self.jit_version)
    }
    
    // ========================================================================
    // Cache Management
    // ========================================================================
    
    /// Check if cache exists
    pub fn has_profile(&self) -> bool {
        Path::new(&self.profile_path()).exists()
    }
    
    pub fn has_ri(&self) -> bool {
        Path::new(&self.ri_path()).exists()
    }
    
    pub fn has_native(&self) -> bool {
        Path::new(&self.native_path()).exists()
    }
    
    /// Clear all cache files
    pub fn clear(&self) -> JitResult<()> {
        let _ = std::fs::remove_file(self.profile_path());
        let _ = std::fs::remove_file(self.ri_path());
        let _ = std::fs::remove_file(self.native_path());
        Ok(())
    }
    
    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            profile_size: std::fs::metadata(&self.profile_path())
                .map(|m| m.len())
                .unwrap_or(0),
            ri_size: std::fs::metadata(&self.ri_path())
                .map(|m| m.len())
                .unwrap_or(0),
            native_size: std::fs::metadata(&self.native_path())
                .map(|m| m.len())
                .unwrap_or(0),
        }
    }
}

/// Native block info (without raw pointer)
pub struct NativeBlockInfo {
    pub guest_rip: u64,
    pub guest_size: u32,
    pub host_size: u32,
    pub tier: CompileTier,
    pub guest_instrs: u32,
    pub guest_checksum: u64,
    pub native_code: Vec<u8>,
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub profile_size: u64,
    pub ri_size: u64,
    pub native_size: u64,
}

impl CacheStats {
    pub fn total_size(&self) -> u64 {
        self.profile_size + self.ri_size + self.native_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_profile_roundtrip() {
        let cache = ReadyNowCache::new("/tmp", "test");
        let profile = ProfileDb::new(1000);
        
        profile.record_block(0x1000);
        profile.record_branch(0x1010, true);
        
        let data = cache.serialize_profile(&profile).unwrap();
        let restored = cache.deserialize_profile(&data).unwrap();
        
        assert_eq!(restored.get_block_count(0x1000), 1);
    }
    
    #[test]
    fn test_architecture() {
        assert_eq!(Architecture::from_u8(0), Some(Architecture::X86_64));
        assert_eq!(Architecture::from_u8(1), Some(Architecture::Aarch64));
        assert_eq!(Architecture::from_u8(99), None);
    }
}
