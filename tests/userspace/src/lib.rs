//! NexaOS Userspace Test Suite
//!
//! 测试用户空间组件：
//! - nrlib: C 兼容的 libc 实现
//! - libs/*: 用户空间库 (ncryptolib, nssl, nzip, nh2, nh3, ntcp2)
//! - programs/*: 用户空间程序

// 子模块
pub mod nrlib;
pub mod libs;
pub mod programs;
