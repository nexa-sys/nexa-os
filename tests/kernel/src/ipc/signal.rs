//! Signal tests (from src/ipc/signal.rs)

use crate::ipc::signal::{SignalState, SignalAction, SIGKILL, SIGTERM, SIGUSR1, SIGSTOP};

#[test]
fn test_signal_state_new() {
    let state = SignalState::new();
    assert!(state.has_pending_signal().is_none());
}

#[test]
fn test_signal_send_and_check() {
    let mut state = SignalState::new();
    state.send_signal(SIGUSR1).unwrap();
    let pending = state.has_pending_signal();
    assert_eq!(pending, Some(SIGUSR1));
}

#[test]
fn test_signal_clear() {
    let mut state = SignalState::new();
    state.send_signal(SIGUSR1).unwrap();
    assert!(state.has_pending_signal().is_some());
    state.clear_signal(SIGUSR1);
    assert!(state.has_pending_signal().is_none());
}

#[test]
fn test_signal_blocking() {
    let mut state = SignalState::new();
    state.block_signal(SIGUSR1);
    state.send_signal(SIGUSR1).unwrap();
    // Signal is pending but blocked
    assert!(state.has_pending_signal().is_none());
    state.unblock_signal(SIGUSR1);
    assert_eq!(state.has_pending_signal(), Some(SIGUSR1));
}

#[test]
fn test_signal_action_cannot_change_sigkill() {
    let mut state = SignalState::new();
    let result = state.set_action(SIGKILL, SignalAction::Ignore);
    assert!(result.is_err());
    let result = state.set_action(SIGSTOP, SignalAction::Ignore);
    assert!(result.is_err());
}

#[test]
fn test_signal_action_change() {
    let mut state = SignalState::new();
    let old = state.set_action(SIGTERM, SignalAction::Ignore).unwrap();
    assert_eq!(old, SignalAction::Default);
    let current = state.get_action(SIGTERM).unwrap();
    assert_eq!(current, SignalAction::Ignore);
}
