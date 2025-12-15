//! Address Token-based Access Control
//!
//! Implements HongMeng's address token mechanism for efficient
//! kernel object access without capability overhead.
//!
//! # Design Philosophy
//!
//! Capabilities hide kernel objects behind tokens and require kernel
//! involvement for every access. For frequently updated objects (like
//! page tables), this causes significant overhead.
//!
//! Address tokens supplement capabilities by:
//! - Mapping kernel objects directly to service address space
//! - Allowing direct read/write without kernel interposition
//! - Using `writev` syscall for read-only object updates
//!
//! # Token Types
//!
//! | Access | Read | Write | Security |
//! |--------|------|-------|----------|
//! | RO     | Direct | writev | High |
//! | RW     | Direct | Direct | Medium |
//!
//! # Security Guarantees
//!
//! - Only pre-selected kernel objects can be mapped
//! - Pointers in objects are never mapped RW (prevents corruption)
//! - Sanity checks on reads from RW objects
//! - Lock-free ring buffers for async messaging
//!
//! # Usage Examples
//!
//! ```ignore
//! // Create a token for a page cache object (RO)
//! let token = create_token(obj_addr, TokenAccess::ReadOnly)?;
//!
//! // Grant to memory manager
//! grant_token(token, mem_mgr_domain)?;
//!
//! // Memory manager reads directly
//! let data = token.read::<PageCacheEntry>()?;
//!
//! // Memory manager updates via writev
//! token.writev(offset, &new_data)?;
//! ```

use spin::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};

/// Maximum number of address tokens
pub const MAX_TOKENS: usize = 1024;

/// Token page size (kernel objects are page-aligned)
pub const TOKEN_PAGE_SIZE: usize = 4096;

/// Token access permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TokenAccess {
    /// Read-only access - writes require writev syscall
    ReadOnly = 0,
    /// Read-write access - direct memory access
    ReadWrite = 1,
}

/// Address token for kernel object access
#[derive(Debug, Clone, Copy)]
pub struct AddressToken {
    /// Unique token ID
    pub id: u32,
    /// Physical address of the kernel object
    pub phys_addr: u64,
    /// Virtual address mapped in grantee's space
    pub virt_addr: u64,
    /// Size of the mapped region
    pub size: u64,
    /// Access permissions
    pub access: TokenAccess,
    /// Owner domain ID
    pub owner_domain: u8,
    /// Grantee domain ID (0 = not granted)
    pub grantee_domain: u8,
    /// Object type for validation
    pub obj_type: KernelObjectType,
    /// Token flags
    pub flags: u32,
}

/// Kernel object types that can have tokens
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KernelObjectType {
    /// Page table entries
    PageTable = 0,
    /// Virtual memory space descriptor
    VSpace = 1,
    /// Page cache entries
    PageCache = 2,
    /// Operation log (async communication)
    OpLog = 3,
    /// Pre-allocated page cache (PCache)
    PCache = 4,
    /// File descriptor table
    FdTable = 5,
    /// Poll list for fd multiplexing
    PollList = 6,
    /// IPC endpoint
    IpcEndpoint = 7,
    /// Custom kernel object
    Custom = 255,
}

impl KernelObjectType {
    /// Check if this object type can be mapped read-write
    pub fn allows_rw(&self) -> bool {
        match self {
            // These contain only values, no pointers - safe for RW
            KernelObjectType::PCache => true,
            KernelObjectType::OpLog => true,
            KernelObjectType::VSpace => true,
            // These contain pointers or sensitive data - RO only
            KernelObjectType::PageTable => false,
            KernelObjectType::PageCache => false,
            KernelObjectType::FdTable => false,
            KernelObjectType::PollList => false,
            KernelObjectType::IpcEndpoint => false,
            KernelObjectType::Custom => false,
        }
    }
}

/// Token flags
pub mod token_flags {
    /// Token is valid
    pub const TOKEN_VALID: u32 = 1 << 0;
    /// Token has been granted
    pub const TOKEN_GRANTED: u32 = 1 << 1;
    /// Token is locked for modification
    pub const TOKEN_LOCKED: u32 = 1 << 2;
    /// Token supports batch operations
    pub const TOKEN_BATCH: u32 = 1 << 3;
}

/// Token error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// Token table is full
    TableFull,
    /// Token not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// Invalid token state
    InvalidState,
    /// Object type doesn't allow requested access
    AccessDenied,
    /// Invalid address
    InvalidAddress,
    /// Size mismatch
    SizeMismatch,
    /// Token already granted
    AlreadyGranted,
    /// Token not granted
    NotGranted,
    /// Verification failed
    VerificationFailed,
}

/// Token manager
struct TokenManager {
    tokens: [Option<AddressToken>; MAX_TOKENS],
    next_id: u32,
    granted_count: usize,
}

impl TokenManager {
    const fn new() -> Self {
        const NONE: Option<AddressToken> = None;
        Self {
            tokens: [NONE; MAX_TOKENS],
            next_id: 1,
            granted_count: 0,
        }
    }
    
    fn create(&mut self, phys_addr: u64, size: u64, access: TokenAccess, obj_type: KernelObjectType, owner_domain: u8) -> Result<u32, TokenError> {
        // Check access permissions for object type
        if access == TokenAccess::ReadWrite && !obj_type.allows_rw() {
            return Err(TokenError::AccessDenied);
        }
        
        // Find empty slot
        for slot in self.tokens.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;
                
                *slot = Some(AddressToken {
                    id,
                    phys_addr,
                    virt_addr: 0, // Set on grant
                    size,
                    access,
                    owner_domain,
                    grantee_domain: 0,
                    obj_type,
                    flags: token_flags::TOKEN_VALID,
                });
                
                return Ok(id);
            }
        }
        
        Err(TokenError::TableFull)
    }
    
    fn get(&self, id: u32) -> Option<&AddressToken> {
        self.tokens.iter()
            .find_map(|slot| slot.as_ref().filter(|t| t.id == id))
    }
    
    fn get_mut(&mut self, id: u32) -> Option<&mut AddressToken> {
        self.tokens.iter_mut()
            .find_map(|slot| slot.as_mut().filter(|t| t.id == id))
    }
    
    fn grant(&mut self, id: u32, grantee_domain: u8, virt_addr: u64) -> Result<(), TokenError> {
        let token = self.get_mut(id).ok_or(TokenError::NotFound)?;
        
        if token.flags & token_flags::TOKEN_GRANTED != 0 {
            return Err(TokenError::AlreadyGranted);
        }
        
        token.grantee_domain = grantee_domain;
        token.virt_addr = virt_addr;
        token.flags |= token_flags::TOKEN_GRANTED;
        self.granted_count += 1;
        
        Ok(())
    }
    
    fn revoke(&mut self, id: u32) -> Result<(), TokenError> {
        let token = self.get_mut(id).ok_or(TokenError::NotFound)?;
        
        if token.flags & token_flags::TOKEN_GRANTED == 0 {
            return Err(TokenError::NotGranted);
        }
        
        token.grantee_domain = 0;
        token.virt_addr = 0;
        token.flags &= !token_flags::TOKEN_GRANTED;
        self.granted_count -= 1;
        
        Ok(())
    }
    
    fn destroy(&mut self, id: u32) -> Result<(), TokenError> {
        for slot in self.tokens.iter_mut() {
            if let Some(token) = slot {
                if token.id == id {
                    if token.flags & token_flags::TOKEN_GRANTED != 0 {
                        self.granted_count -= 1;
                    }
                    *slot = None;
                    return Ok(());
                }
            }
        }
        Err(TokenError::NotFound)
    }
}

// Global token manager
static TOKEN_MANAGER: Mutex<TokenManager> = Mutex::new(TokenManager::new());

/// Initialize address token subsystem
pub fn init() {
    crate::kinfo!("UDRV/Token: Initializing address token subsystem");
    crate::kinfo!("UDRV/Token: {} max tokens, page size {} bytes",
                  MAX_TOKENS, TOKEN_PAGE_SIZE);
}

/// Create a new address token
pub fn create_token(
    phys_addr: u64,
    size: u64,
    access: TokenAccess,
    obj_type: KernelObjectType,
    owner_domain: u8,
) -> Result<u32, TokenError> {
    TOKEN_MANAGER.lock().create(phys_addr, size, access, obj_type, owner_domain)
}

/// Grant a token to a domain
pub fn grant_token(id: u32, grantee_domain: u8, virt_addr: u64) -> Result<(), TokenError> {
    TOKEN_MANAGER.lock().grant(id, grantee_domain, virt_addr)
}

/// Revoke a granted token
pub fn revoke_token(id: u32) -> Result<(), TokenError> {
    TOKEN_MANAGER.lock().revoke(id)
}

/// Destroy a token
pub fn destroy_token(id: u32) -> Result<(), TokenError> {
    TOKEN_MANAGER.lock().destroy(id)
}

/// Get token info
pub fn get_token(id: u32) -> Option<AddressToken> {
    TOKEN_MANAGER.lock().get(id).copied()
}

/// Verify token access for writev operation
/// 
/// This is called when a service uses writev to update a read-only token
pub fn verify_writev(id: u32, offset: u64, len: u64, caller_domain: u8) -> Result<u64, TokenError> {
    let manager = TOKEN_MANAGER.lock();
    let token = manager.get(id).ok_or(TokenError::NotFound)?;
    
    // Check caller is the grantee
    if token.grantee_domain != caller_domain {
        return Err(TokenError::PermissionDenied);
    }
    
    // Check token is granted
    if token.flags & token_flags::TOKEN_GRANTED == 0 {
        return Err(TokenError::NotGranted);
    }
    
    // Check bounds
    if offset + len > token.size {
        return Err(TokenError::SizeMismatch);
    }
    
    // Return physical address for kernel to perform the write
    Ok(token.phys_addr + offset)
}

/// Lock-free ring buffer for async token communication
/// Used for PCache (pre-allocated pages) and OpLog
#[repr(C)]
pub struct TokenRingBuffer {
    /// Head index (consumer)
    head: AtomicU64,
    /// Tail index (producer)
    tail: AtomicU64,
    /// Buffer capacity (must be power of 2)
    capacity: u64,
    /// Entry size in bytes
    entry_size: u64,
    /// Buffer data offset
    data_offset: u64,
}

impl TokenRingBuffer {
    /// Create a new ring buffer
    pub const fn new(capacity: u64, entry_size: u64, data_offset: u64) -> Self {
        Self {
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            capacity,
            entry_size,
            data_offset,
        }
    }
    
    /// Check if buffer is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
    
    /// Check if buffer is full
    #[inline]
    pub fn is_full(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        (tail - head) >= self.capacity
    }
    
    /// Get number of entries
    #[inline]
    pub fn len(&self) -> u64 {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        tail - head
    }
    
    /// Push entry (producer side)
    /// Returns offset of entry or None if full
    pub fn push(&self) -> Option<u64> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Relaxed);
            
            if (tail - head) >= self.capacity {
                return None; // Full
            }
            
            // Try to claim the slot
            if self.tail.compare_exchange_weak(
                tail,
                tail + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ).is_ok() {
                // Calculate offset
                let index = tail & (self.capacity - 1);
                return Some(self.data_offset + index * self.entry_size);
            }
            // Retry on CAS failure
            core::hint::spin_loop();
        }
    }
    
    /// Pop entry (consumer side)
    /// Returns offset of entry or None if empty
    pub fn pop(&self) -> Option<u64> {
        loop {
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Acquire);
            
            if head == tail {
                return None; // Empty
            }
            
            // Try to consume the slot
            if self.head.compare_exchange_weak(
                head,
                head + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ).is_ok() {
                // Calculate offset
                let index = head & (self.capacity - 1);
                return Some(self.data_offset + index * self.entry_size);
            }
            // Retry on CAS failure
            core::hint::spin_loop();
        }
    }
}

/// Pre-allocated Page Cache (PCache) entry
/// Memory manager pre-allocates pages and sends to kernel via this structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PCacheEntry {
    /// Physical address of pre-allocated page
    pub phys_addr: u64,
    /// NUMA node hint
    pub numa_node: u8,
    /// Flags
    pub flags: u8,
    /// Reserved
    pub _reserved: [u8; 6],
}

/// Operation Log entry for async kernel-service communication
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OpLogEntry {
    /// Operation type
    pub op_type: u32,
    /// Target object ID
    pub object_id: u32,
    /// Operation parameter 1
    pub param1: u64,
    /// Operation parameter 2
    pub param2: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Operation log types
pub mod oplog_types {
    /// Page mapped to process
    pub const OP_PAGE_MAPPED: u32 = 1;
    /// Page unmapped from process
    pub const OP_PAGE_UNMAPPED: u32 = 2;
    /// Page table modified
    pub const OP_PT_MODIFIED: u32 = 3;
    /// VSpace updated
    pub const OP_VSPACE_UPDATED: u32 = 4;
}
