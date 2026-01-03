//! Tests for udrv/address_token.rs - Address Token Access Control
//!
//! Tests the address token mechanism for efficient kernel object access.

use core::mem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udrv::address_token::{AddressToken, TokenAccess, KernelObjectType, MAX_TOKENS, TOKEN_PAGE_SIZE};

    // =========================================================================
    // Token Constants Tests
    // =========================================================================

    #[test]
    fn test_max_tokens() {
        // Should be a reasonable number for system-wide tokens
        assert!(MAX_TOKENS >= 256);
        assert!(MAX_TOKENS <= 65536);
        assert_eq!(MAX_TOKENS, 1024);
    }

    #[test]
    fn test_token_page_size() {
        // Must be standard 4KB page
        assert_eq!(TOKEN_PAGE_SIZE, 4096);
        // Must be power of 2
        assert!(TOKEN_PAGE_SIZE.is_power_of_two());
    }

    // =========================================================================
    // TokenAccess Tests
    // =========================================================================

    #[test]
    fn test_token_access_values() {
        assert_eq!(TokenAccess::ReadOnly as u8, 0);
        assert_eq!(TokenAccess::ReadWrite as u8, 1);
    }

    #[test]
    fn test_token_access_size() {
        assert_eq!(mem::size_of::<TokenAccess>(), 1);
    }

    #[test]
    fn test_token_access_distinct() {
        assert_ne!(TokenAccess::ReadOnly, TokenAccess::ReadWrite);
    }

    #[test]
    fn test_token_access_copy_clone() {
        let access = TokenAccess::ReadOnly;
        let access2 = access;
        let access3 = access.clone();
        assert_eq!(access, access2);
        assert_eq!(access, access3);
    }

    // =========================================================================
    // KernelObjectType Tests
    // =========================================================================

    #[test]
    fn test_kernel_object_type_values() {
        assert_eq!(KernelObjectType::PageTable as u8, 0);
        assert_eq!(KernelObjectType::VSpace as u8, 1);
        assert_eq!(KernelObjectType::PageCache as u8, 2);
    }

    #[test]
    fn test_kernel_object_type_size() {
        assert_eq!(mem::size_of::<KernelObjectType>(), 1);
    }

    #[test]
    fn test_kernel_object_types_distinct() {
        assert_ne!(KernelObjectType::PageTable, KernelObjectType::VSpace);
        assert_ne!(KernelObjectType::VSpace, KernelObjectType::PageCache);
        assert_ne!(KernelObjectType::PageTable, KernelObjectType::PageCache);
    }

    // =========================================================================
    // AddressToken Structure Tests
    // =========================================================================

    #[test]
    fn test_address_token_creation() {
        let token = AddressToken {
            id: 1,
            phys_addr: 0x1000,
            virt_addr: 0x2000,
            size: 4096,
            access: TokenAccess::ReadOnly,
            owner_domain: 0,
            grantee_domain: 1,
            obj_type: KernelObjectType::PageCache,
            flags: 0,
        };
        
        assert_eq!(token.id, 1);
        assert_eq!(token.phys_addr, 0x1000);
        assert_eq!(token.virt_addr, 0x2000);
        assert_eq!(token.size, 4096);
        assert_eq!(token.access, TokenAccess::ReadOnly);
        assert_eq!(token.owner_domain, 0);
        assert_eq!(token.grantee_domain, 1);
        assert_eq!(token.obj_type, KernelObjectType::PageCache);
        assert_eq!(token.flags, 0);
    }

    #[test]
    fn test_address_token_rw() {
        let token = AddressToken {
            id: 2,
            phys_addr: 0x10000,
            virt_addr: 0x20000,
            size: 8192,
            access: TokenAccess::ReadWrite,
            owner_domain: 0,
            grantee_domain: 2,
            obj_type: KernelObjectType::PageTable,
            flags: 0x1,
        };
        
        assert_eq!(token.access, TokenAccess::ReadWrite);
        assert!(token.size > TOKEN_PAGE_SIZE as u64);
    }

    #[test]
    fn test_address_token_copy() {
        let token = AddressToken {
            id: 3,
            phys_addr: 0x3000,
            virt_addr: 0x4000,
            size: 4096,
            access: TokenAccess::ReadOnly,
            owner_domain: 1,
            grantee_domain: 0,
            obj_type: KernelObjectType::VSpace,
            flags: 0,
        };
        
        let token2 = token;
        assert_eq!(token.id, token2.id);
        assert_eq!(token.phys_addr, token2.phys_addr);
    }

    #[test]
    fn test_address_token_ungrated() {
        // grantee_domain = 0 means not granted
        let token = AddressToken {
            id: 4,
            phys_addr: 0x5000,
            virt_addr: 0x6000,
            size: 4096,
            access: TokenAccess::ReadOnly,
            owner_domain: 1,
            grantee_domain: 0,
            obj_type: KernelObjectType::PageCache,
            flags: 0,
        };
        
        assert_eq!(token.grantee_domain, 0, "Token should not be granted yet");
    }

    #[test]
    fn test_address_token_alignment() {
        // Addresses should be page-aligned for efficient mapping
        let token = AddressToken {
            id: 5,
            phys_addr: 0x1000,
            virt_addr: 0x2000,
            size: 4096,
            access: TokenAccess::ReadWrite,
            owner_domain: 0,
            grantee_domain: 1,
            obj_type: KernelObjectType::PageTable,
            flags: 0,
        };
        
        assert_eq!(token.phys_addr % TOKEN_PAGE_SIZE as u64, 0);
        assert_eq!(token.virt_addr % TOKEN_PAGE_SIZE as u64, 0);
    }
}
