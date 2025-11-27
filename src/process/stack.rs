//! User stack building utilities
//!
//! This module contains the UserStackBuilder and related functions for
//! constructing the initial user-space stack layout (argc, argv, envp, auxv).

use core::ptr;

use crate::elf::LoadResult;

use super::types::MAX_PROCESS_ARGS;

/// Random seed placed at the top of the stack for AT_RANDOM
const STACK_RANDOM_SEED: [u8; 16] = *b"NexaOSGuardSeed!";

// ELF auxiliary vector types
const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_RANDOM: u64 = 25;
const AT_EXECFN: u64 = 31;

/// Helper struct for building the initial user stack
/// 
/// This builder handles the case where the virtual address (what userspace sees)
/// differs from the physical address (where we actually write in the kernel).
pub(crate) struct UserStackBuilder {
    /// Current cursor position (virtual address for userspace)
    cursor: u64,
    /// Lower bound of virtual address space
    lower_bound: u64,
    /// Offset to add to virtual address to get physical address for writing
    /// write_addr = cursor + phys_offset
    phys_offset: i64,
}

impl UserStackBuilder {
    /// Create a new stack builder with given base address and size
    /// This version assumes identity mapping (phys == virt)
    pub fn new(base: u64, size: u64) -> Self {
        Self {
            cursor: base + size,
            lower_bound: base,
            phys_offset: 0,
        }
    }

    /// Create a new stack builder with separate virtual and physical addresses
    /// 
    /// - `virt_base`: Virtual base address (what userspace will see)
    /// - `size`: Size of the stack region
    /// - `phys_base`: Physical base address (where kernel writes)
    pub fn new_with_phys(virt_base: u64, size: u64, phys_base: u64) -> Self {
        Self {
            cursor: virt_base + size,
            lower_bound: virt_base,
            phys_offset: phys_base as i64 - virt_base as i64,
        }
    }

    /// Convert virtual address to physical address for writing
    #[inline]
    fn virt_to_phys(&self, virt: u64) -> u64 {
        (virt as i64 + self.phys_offset) as u64
    }

    /// Get the current stack pointer position (virtual address)
    pub fn current_ptr(&self) -> u64 {
        self.cursor
    }

    /// Pad the stack to the specified alignment
    pub fn pad_to_alignment(&mut self, align: u64) -> Result<(), &'static str> {
        debug_assert!(align.is_power_of_two());
        if align == 0 {
            return Ok(());
        }

        let mask = align - 1;
        let remainder = self.cursor & mask;
        if remainder == 0 {
            return Ok(());
        }

        let padding = remainder;
        self.cursor = self.cursor.checked_sub(padding).ok_or("Stack overflow")?;
        if self.cursor < self.lower_bound {
            return Err("Stack overflow");
        }

        unsafe {
            let phys_addr = self.virt_to_phys(self.cursor);
            ptr::write_bytes(phys_addr as *mut u8, 0, padding as usize);
        }

        Ok(())
    }

    /// Push raw bytes onto the stack
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<u64, &'static str> {
        if bytes.is_empty() {
            return Ok(self.cursor);
        }

        let len = bytes.len() as u64;
        self.cursor = self.cursor.checked_sub(len).ok_or("Stack overflow")?;
        if self.cursor < self.lower_bound {
            return Err("Stack overflow");
        }

        unsafe {
            let phys_addr = self.virt_to_phys(self.cursor);
            ptr::copy_nonoverlapping(bytes.as_ptr(), phys_addr as *mut u8, bytes.len());
        }

        Ok(self.cursor)
    }

    /// Push a null-terminated C string onto the stack
    pub fn push_cstring(&mut self, bytes: &[u8]) -> Result<u64, &'static str> {
        let null_ptr = self.push_bytes(&[0])?;
        if bytes.is_empty() {
            return Ok(null_ptr);
        }
        self.push_bytes(bytes)
    }

    /// Push a 64-bit value onto the stack (with 8-byte alignment)
    pub fn push_u64(&mut self, value: u64) -> Result<u64, &'static str> {
        self.pad_to_alignment(8)?;
        self.cursor = self.cursor.checked_sub(8).ok_or("Stack overflow")?;
        if self.cursor < self.lower_bound {
            return Err("Stack overflow");
        }
        unsafe {
            let phys_addr = self.virt_to_phys(self.cursor);
            (phys_addr as *mut u64).write(value);
        }
        Ok(self.cursor)
    }
}

/// Build the initial stack for a user process
///
/// This function constructs the stack layout expected by the C runtime:
/// - argc (number of arguments)
/// - argv pointers (null-terminated array)
/// - envp pointers (null-terminated array, empty for now)
/// - auxiliary vectors (ELF aux info)
/// - argument strings
/// - random bytes for AT_RANDOM
///
/// Arguments:
/// - `argv`: Command line arguments
/// - `exec_path`: Path to the executable
/// - `stack_virt_base`: Virtual base address of the stack (what userspace sees)
/// - `stack_size`: Size of the stack region
/// - `stack_phys_base`: Physical base address of the stack (where kernel writes)
/// - `program`: Load result for the main program
/// - `interpreter`: Optional load result for the dynamic linker
pub fn build_initial_stack(
    argv: &[&[u8]],
    exec_path: &[u8],
    stack_virt_base: u64,
    stack_size: u64,
    stack_phys_base: u64,
    program: &LoadResult,
    interpreter: Option<&LoadResult>,
) -> Result<u64, &'static str> {
    let mut builder = UserStackBuilder::new_with_phys(stack_virt_base, stack_size, stack_phys_base);

    if argv.len() > MAX_PROCESS_ARGS {
        return Err("Too many arguments");
    }

    // Push random bytes for AT_RANDOM
    let random_ptr = builder.push_bytes(&STACK_RANDOM_SEED)?;

    // Push exec filename string if provided
    let execfn_ptr = if exec_path.is_empty() {
        None
    } else {
        Some(builder.push_cstring(exec_path)?)
    };

    // Push argument strings and collect pointers
    let mut arg_ptrs = [0u64; MAX_PROCESS_ARGS];
    for i in (0..argv.len()).rev() {
        arg_ptrs[i] = builder.push_cstring(argv[i])?;
    }

    let argc = argv.len();

    builder.pad_to_alignment(16)?;

    // Build auxiliary vector
    const AUX_MAX: usize = 16;
    let mut aux_entries: [(u64, u64); AUX_MAX] = [(AT_NULL, 0); AUX_MAX];
    let mut aux_len: usize = 0;

    aux_entries[aux_len] = (AT_PHDR, program.phdr_vaddr);
    aux_len += 1;
    aux_entries[aux_len] = (AT_PHENT, program.phentsize as u64);
    aux_len += 1;
    aux_entries[aux_len] = (AT_PHNUM, program.phnum as u64);
    aux_len += 1;
    aux_entries[aux_len] = (AT_PAGESZ, 4096);
    aux_len += 1;

    if let Some(interp) = interpreter {
        aux_entries[aux_len] = (AT_BASE, interp.base_addr);
        aux_len += 1;
    }

    aux_entries[aux_len] = (AT_FLAGS, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_ENTRY, program.entry_point);
    aux_len += 1;
    aux_entries[aux_len] = (AT_UID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_EUID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_GID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_EGID, 0);
    aux_len += 1;
    aux_entries[aux_len] = (AT_RANDOM, random_ptr);
    aux_len += 1;

    if let Some(ptr) = execfn_ptr {
        aux_entries[aux_len] = (AT_EXECFN, ptr);
        aux_len += 1;
    } else if argc > 0 {
        aux_entries[aux_len] = (AT_EXECFN, arg_ptrs[0]);
        aux_len += 1;
    }

    aux_entries[aux_len] = (AT_NULL, 0);
    aux_len += 1;

    // Push auxiliary vector (in reverse order)
    for (key, value) in aux_entries[..aux_len].iter().rev() {
        builder.push_u64(*value)?;
        builder.push_u64(*key)?;
    }

    // Final 16-byte alignment BEFORE pushing argc/argv/envp
    // This ensures the entire argc/argv/envp block is properly aligned
    // and there's no padding between argc and argv[0]
    builder.pad_to_alignment(16)?;

    // Push envp NULL terminator (no environment variables for now)
    builder.push_u64(0)?;

    // Push argv NULL terminator
    builder.push_u64(0)?;

    // Push argv pointers (in reverse order)
    for i in (0..argc).rev() {
        builder.push_u64(arg_ptrs[i])?;
    }

    // Push argc (must be immediately before argv with no padding!)
    builder.push_u64(argc as u64)?;

    Ok(builder.current_ptr())
}
