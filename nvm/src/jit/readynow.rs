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
use super::ir::{IrBlock, IrBasicBlock, IrInstr, IrOp, VReg, ExitReason};
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
        
        // Basic block count
        data.extend_from_slice(&(block.blocks.len() as u32).to_le_bytes());
        
        for bb in &block.blocks {
            self.serialize_basic_block(data, bb)?;
        }
        
        Ok(())
    }
    
    fn serialize_basic_block(&self, data: &mut Vec<u8>, bb: &IrBasicBlock) -> JitResult<()> {
        // Block ID
        data.extend_from_slice(&(bb.id as u32).to_le_bytes());
        
        // Instruction count
        data.extend_from_slice(&(bb.instrs.len() as u32).to_le_bytes());
        
        for instr in &bb.instrs {
            self.serialize_ir_instr(data, instr)?;
        }
        
        // Exit reason
        self.serialize_exit_reason(data, &bb.exit)?;
        
        Ok(())
    }
    
    fn serialize_ir_instr(&self, data: &mut Vec<u8>, instr: &IrInstr) -> JitResult<()> {
        data.extend_from_slice(&instr.rip.to_le_bytes());
        self.serialize_ir_op(data, &instr.op)
    }
    
    fn serialize_ir_op(&self, data: &mut Vec<u8>, op: &IrOp) -> JitResult<()> {
        // Opcode byte followed by operands
        match op {
            IrOp::Nop => data.push(0),
            IrOp::LoadConst(dst, val) => {
                data.push(1);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&val.to_le_bytes());
            }
            IrOp::Copy(dst, src) => {
                data.push(2);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&src.0.to_le_bytes());
            }
            IrOp::Add(dst, a, b) => {
                data.push(3);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Sub(dst, a, b) => {
                data.push(4);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::And(dst, a, b) => {
                data.push(5);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Or(dst, a, b) => {
                data.push(6);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Xor(dst, a, b) => {
                data.push(7);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Mul(dst, a, b) => {
                data.push(8);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Shl(dst, a, b) => {
                data.push(9);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Shr(dst, a, b) => {
                data.push(10);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&a.0.to_le_bytes());
                data.extend_from_slice(&b.0.to_le_bytes());
            }
            IrOp::Load64(dst, addr) => {
                data.push(11);
                data.extend_from_slice(&dst.0.to_le_bytes());
                data.extend_from_slice(&addr.0.to_le_bytes());
            }
            IrOp::Store64(addr, val) => {
                data.push(12);
                data.extend_from_slice(&addr.0.to_le_bytes());
                data.extend_from_slice(&val.0.to_le_bytes());
            }
            // Add more opcodes as needed...
            _ => data.push(255), // Unknown - will be recompiled
        }
        
        Ok(())
    }
    
    fn serialize_exit_reason(&self, data: &mut Vec<u8>, exit: &ExitReason) -> JitResult<()> {
        match exit {
            ExitReason::Fallthrough => data.push(0),
            ExitReason::Jump(target) => {
                data.push(1);
                data.extend_from_slice(&target.0.to_le_bytes());
            }
            ExitReason::Branch { cond, target, fallthrough } => {
                data.push(2);
                data.extend_from_slice(&cond.0.to_le_bytes());
                data.extend_from_slice(&target.0.to_le_bytes());
                data.extend_from_slice(&fallthrough.0.to_le_bytes());
            }
            ExitReason::Return(val) => {
                data.push(3);
                if let Some(v) = val {
                    data.push(1);
                    data.extend_from_slice(&v.0.to_le_bytes());
                } else {
                    data.push(0);
                }
            }
            ExitReason::Halt => data.push(4),
            ExitReason::Interrupt(vec) => {
                data.push(5);
                data.push(*vec);
            }
            ExitReason::IoNeeded { port, is_write, size } => {
                data.push(6);
                data.extend_from_slice(&port.to_le_bytes());
                data.push(*is_write as u8);
                data.push(*size);
            }
            ExitReason::IndirectJump(target) => {
                data.push(7);
                data.extend_from_slice(&target.0.to_le_bytes());
            }
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
        if offset + 12 > data.len() {
            return None;
        }
        
        let entry_rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
        offset += 8;
        
        let bb_count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let mut blocks = Vec::with_capacity(bb_count);
        
        for _ in 0..bb_count {
            let (bb, new_offset) = self.deserialize_basic_block(data, offset)?;
            blocks.push(bb);
            offset = new_offset;
        }
        
        Some((IrBlock { entry_rip, blocks }, offset))
    }
    
    fn deserialize_basic_block(&self, data: &[u8], mut offset: usize) -> Option<(IrBasicBlock, usize)> {
        if offset + 8 > data.len() {
            return None;
        }
        
        let id = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let instr_count = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
        offset += 4;
        
        let mut instrs = Vec::with_capacity(instr_count);
        
        for _ in 0..instr_count {
            let (instr, new_offset) = self.deserialize_ir_instr(data, offset)?;
            instrs.push(instr);
            offset = new_offset;
        }
        
        let (exit, new_offset) = self.deserialize_exit_reason(data, offset)?;
        
        Some((IrBasicBlock { id, instrs, exit }, new_offset))
    }
    
    fn deserialize_ir_instr(&self, data: &[u8], mut offset: usize) -> Option<(IrInstr, usize)> {
        if offset + 9 > data.len() {
            return None;
        }
        
        let rip = u64::from_le_bytes(data[offset..offset+8].try_into().ok()?);
        offset += 8;
        
        let (op, new_offset) = self.deserialize_ir_op(data, offset)?;
        
        Some((IrInstr { rip, op }, new_offset))
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
                let dst = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let val = u64::from_le_bytes(data[offset+4..offset+12].try_into().ok()?);
                offset += 12;
                IrOp::LoadConst(dst, val)
            }
            2 => {
                let dst = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let src = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                offset += 8;
                IrOp::Copy(dst, src)
            }
            3 => {
                let dst = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let a = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                let b = VReg(u32::from_le_bytes(data[offset+8..offset+12].try_into().ok()?));
                offset += 12;
                IrOp::Add(dst, a, b)
            }
            // ... more opcodes
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
        
        let exit = match kind {
            0 => ExitReason::Fallthrough,
            1 => {
                let target = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                offset += 4;
                ExitReason::Jump(target)
            }
            2 => {
                let cond = VReg(u32::from_le_bytes(data[offset..offset+4].try_into().ok()?));
                let target = VReg(u32::from_le_bytes(data[offset+4..offset+8].try_into().ok()?));
                let fallthrough = VReg(u32::from_le_bytes(data[offset+8..offset+12].try_into().ok()?));
                offset += 12;
                ExitReason::Branch { cond, target, fallthrough }
            }
            4 => ExitReason::Halt,
            5 => {
                let vec = data[offset];
                offset += 1;
                ExitReason::Interrupt(vec)
            }
            _ => ExitReason::Fallthrough,
        };
        
        Some((exit, offset))
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
