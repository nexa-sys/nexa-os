//! Security subsystem for NexaOS
//!
//! This module contains security-related functionality including:
//! - User authentication
//! - ELF binary loading and validation

pub mod auth;
pub mod elf;

// Re-export commonly used items from auth
pub use auth::{
    authenticate, create_user, current_gid, current_uid, current_user, enumerate_users, init,
    is_superuser, logout, require_admin, AuthError, Credentials, CurrentUser, UserSummary,
};

// Re-export from elf
pub use elf::ph_flags;
pub use elf::{
    Elf64Header, Elf64ProgramHeader, ElfClass, ElfData, ElfLoader, ElfType, LoadResult, PhType,
    ELF_MAGIC,
};
