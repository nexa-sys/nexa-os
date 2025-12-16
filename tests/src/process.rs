//! Process tests

use crate::process::{ProcessState, Context};

#[test]
fn test_process_state_comparison() {
    assert_ne!(ProcessState::Ready, ProcessState::Running);
    assert_ne!(ProcessState::Running, ProcessState::Sleeping);
    assert_ne!(ProcessState::Sleeping, ProcessState::Zombie);
}

#[test]
fn test_context_zero() {
    let ctx = Context::zero();
    assert_eq!(ctx.rax, 0);
    assert_eq!(ctx.rbx, 0);
    assert_eq!(ctx.rcx, 0);
    assert_eq!(ctx.rdx, 0);
    assert_eq!(ctx.rip, 0);
    // IF flag should be set (0x200)
    assert_eq!(ctx.rflags & 0x200, 0x200);
}
