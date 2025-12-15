//! Mock signal handling for testing
//!
//! Simulates POSIX signal handling without actual kernel signals.

use std::collections::{HashMap, HashSet, VecDeque};

/// Signal numbers (POSIX)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Signal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGTRAP = 5,
    SIGABRT = 6,
    SIGBUS = 7,
    SIGFPE = 8,
    SIGKILL = 9,
    SIGUSR1 = 10,
    SIGSEGV = 11,
    SIGUSR2 = 12,
    SIGPIPE = 13,
    SIGALRM = 14,
    SIGTERM = 15,
    SIGSTKFLT = 16,
    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTSTP = 20,
    SIGTTIN = 21,
    SIGTTOU = 22,
    SIGURG = 23,
    SIGXCPU = 24,
    SIGXFSZ = 25,
    SIGVTALRM = 26,
    SIGPROF = 27,
    SIGWINCH = 28,
    SIGIO = 29,
    SIGPWR = 30,
    SIGSYS = 31,
}

impl Signal {
    /// Convert from integer
    pub fn from_i32(num: i32) -> Option<Self> {
        match num {
            1 => Some(Signal::SIGHUP),
            2 => Some(Signal::SIGINT),
            3 => Some(Signal::SIGQUIT),
            4 => Some(Signal::SIGILL),
            5 => Some(Signal::SIGTRAP),
            6 => Some(Signal::SIGABRT),
            7 => Some(Signal::SIGBUS),
            8 => Some(Signal::SIGFPE),
            9 => Some(Signal::SIGKILL),
            10 => Some(Signal::SIGUSR1),
            11 => Some(Signal::SIGSEGV),
            12 => Some(Signal::SIGUSR2),
            13 => Some(Signal::SIGPIPE),
            14 => Some(Signal::SIGALRM),
            15 => Some(Signal::SIGTERM),
            16 => Some(Signal::SIGSTKFLT),
            17 => Some(Signal::SIGCHLD),
            18 => Some(Signal::SIGCONT),
            19 => Some(Signal::SIGSTOP),
            20 => Some(Signal::SIGTSTP),
            21 => Some(Signal::SIGTTIN),
            22 => Some(Signal::SIGTTOU),
            23 => Some(Signal::SIGURG),
            24 => Some(Signal::SIGXCPU),
            25 => Some(Signal::SIGXFSZ),
            26 => Some(Signal::SIGVTALRM),
            27 => Some(Signal::SIGPROF),
            28 => Some(Signal::SIGWINCH),
            29 => Some(Signal::SIGIO),
            30 => Some(Signal::SIGPWR),
            31 => Some(Signal::SIGSYS),
            _ => None,
        }
    }

    /// Check if signal can be caught or ignored
    pub fn can_be_caught(&self) -> bool {
        !matches!(self, Signal::SIGKILL | Signal::SIGSTOP)
    }

    /// Check if signal can be blocked
    pub fn can_be_blocked(&self) -> bool {
        !matches!(self, Signal::SIGKILL | Signal::SIGSTOP)
    }

    /// Get default action for signal
    pub fn default_action(&self) -> SignalAction {
        match self {
            Signal::SIGKILL | Signal::SIGTERM | Signal::SIGINT |
            Signal::SIGQUIT | Signal::SIGABRT | Signal::SIGSEGV |
            Signal::SIGILL | Signal::SIGFPE | Signal::SIGBUS |
            Signal::SIGSYS | Signal::SIGXCPU | Signal::SIGXFSZ |
            Signal::SIGSTKFLT => SignalAction::Terminate,
            
            Signal::SIGSTOP | Signal::SIGTSTP | Signal::SIGTTIN |
            Signal::SIGTTOU => SignalAction::Stop,
            
            Signal::SIGCONT => SignalAction::Continue,
            
            Signal::SIGCHLD | Signal::SIGURG | Signal::SIGWINCH |
            Signal::SIGIO => SignalAction::Ignore,
            
            _ => SignalAction::Terminate,
        }
    }

    /// Check if this signal generates a core dump by default
    pub fn generates_core(&self) -> bool {
        matches!(self,
            Signal::SIGQUIT | Signal::SIGILL | Signal::SIGTRAP |
            Signal::SIGABRT | Signal::SIGBUS | Signal::SIGFPE |
            Signal::SIGSEGV | Signal::SIGXCPU | Signal::SIGXFSZ |
            Signal::SIGSYS
        )
    }
}

/// Signal action type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    Default,
    Ignore,
    Terminate,
    Stop,
    Continue,
    Handler(usize), // Handler address (simulated)
}

/// Signal mask (bitmap of blocked signals)
#[derive(Debug, Clone, Default)]
pub struct SignalMask {
    blocked: HashSet<Signal>,
}

impl SignalMask {
    pub fn new() -> Self {
        Self {
            blocked: HashSet::new(),
        }
    }

    /// Block a signal
    pub fn block(&mut self, sig: Signal) -> bool {
        if sig.can_be_blocked() {
            self.blocked.insert(sig);
            true
        } else {
            false
        }
    }

    /// Unblock a signal
    pub fn unblock(&mut self, sig: Signal) {
        self.blocked.remove(&sig);
    }

    /// Check if signal is blocked
    pub fn is_blocked(&self, sig: Signal) -> bool {
        self.blocked.contains(&sig)
    }

    /// Block multiple signals
    pub fn block_set(&mut self, signals: &[Signal]) {
        for &sig in signals {
            self.block(sig);
        }
    }

    /// Unblock all signals
    pub fn unblock_all(&mut self) {
        self.blocked.clear();
    }

    /// Get set of blocked signals
    pub fn blocked_signals(&self) -> Vec<Signal> {
        self.blocked.iter().cloned().collect()
    }
}

/// Signal handler configuration
#[derive(Debug, Clone)]
pub struct SignalHandler {
    pub action: SignalAction,
    pub mask: SignalMask, // Signals to block during handler
    pub flags: u32,
}

impl Default for SignalHandler {
    fn default() -> Self {
        Self {
            action: SignalAction::Default,
            mask: SignalMask::new(),
            flags: 0,
        }
    }
}

/// Pending signal info
#[derive(Debug, Clone)]
pub struct PendingSignal {
    pub signal: Signal,
    pub sender_pid: u32,
    pub timestamp: u64,
}

/// Mock process signal state
pub struct MockSignalState {
    handlers: HashMap<Signal, SignalHandler>,
    mask: SignalMask,
    pending: VecDeque<PendingSignal>,
    pid: u32,
    stopped: bool,
    terminated: bool,
    exit_signal: Option<Signal>,
}

impl MockSignalState {
    pub fn new(pid: u32) -> Self {
        Self {
            handlers: HashMap::new(),
            mask: SignalMask::new(),
            pending: VecDeque::new(),
            pid,
            stopped: false,
            terminated: false,
            exit_signal: None,
        }
    }

    /// Set signal handler
    pub fn set_handler(&mut self, sig: Signal, handler: SignalHandler) -> Result<SignalHandler, i32> {
        if !sig.can_be_caught() && handler.action != SignalAction::Default {
            return Err(22); // EINVAL
        }
        
        let old = self.handlers.get(&sig).cloned().unwrap_or_default();
        self.handlers.insert(sig, handler);
        Ok(old)
    }

    /// Get current handler for signal
    pub fn get_handler(&self, sig: Signal) -> SignalHandler {
        self.handlers.get(&sig).cloned().unwrap_or_default()
    }

    /// Send a signal to this process
    pub fn send_signal(&mut self, sig: Signal, sender_pid: u32) {
        // Cannot send signals to terminated process
        if self.terminated {
            return;
        }

        // SIGCONT can resume a stopped process
        if sig == Signal::SIGCONT && self.stopped {
            self.stopped = false;
            // Discard pending SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU
            self.pending.retain(|p| !matches!(p.signal,
                Signal::SIGSTOP | Signal::SIGTSTP |
                Signal::SIGTTIN | Signal::SIGTTOU
            ));
        }

        // Add to pending queue
        self.pending.push_back(PendingSignal {
            signal: sig,
            sender_pid,
            timestamp: 0, // Would use real time in kernel
        });
    }

    /// Check if there are deliverable signals
    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|p| !self.mask.is_blocked(p.signal))
    }

    /// Deliver next pending signal
    pub fn deliver_signal(&mut self) -> Option<(Signal, SignalAction)> {
        if self.stopped || self.terminated {
            return None;
        }

        // Find first unblocked signal
        let idx = self.pending.iter().position(|p| !self.mask.is_blocked(p.signal))?;
        let pending = self.pending.remove(idx)?;
        let sig = pending.signal;

        let handler = self.get_handler(sig);
        let action = match handler.action {
            SignalAction::Default => sig.default_action(),
            SignalAction::Ignore => SignalAction::Ignore,
            other => other,
        };

        // Apply action
        match action {
            SignalAction::Terminate => {
                self.terminated = true;
                self.exit_signal = Some(sig);
            }
            SignalAction::Stop => {
                self.stopped = true;
            }
            SignalAction::Continue => {
                self.stopped = false;
            }
            _ => {}
        }

        Some((sig, action))
    }

    /// Block signal temporarily
    pub fn block_signal(&mut self, sig: Signal) -> bool {
        self.mask.block(sig)
    }

    /// Unblock signal
    pub fn unblock_signal(&mut self, sig: Signal) {
        self.mask.unblock(sig)
    }

    /// Get current signal mask
    pub fn get_mask(&self) -> &SignalMask {
        &self.mask
    }

    /// Set signal mask
    pub fn set_mask(&mut self, mask: SignalMask) {
        self.mask = mask;
    }

    /// Check if process is stopped
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Check if process is terminated
    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    /// Get termination signal if terminated
    pub fn exit_signal(&self) -> Option<Signal> {
        self.exit_signal
    }

    /// Get number of pending signals
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// Signal set operations helper
pub struct SignalSet {
    signals: HashSet<Signal>,
}

impl Default for SignalSet {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalSet {
    pub fn new() -> Self {
        Self {
            signals: HashSet::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new()
    }

    pub fn full() -> Self {
        let mut set = Self::new();
        for i in 1..=31 {
            if let Some(sig) = Signal::from_i32(i) {
                set.add(sig);
            }
        }
        set
    }

    pub fn add(&mut self, sig: Signal) {
        self.signals.insert(sig);
    }

    pub fn remove(&mut self, sig: Signal) {
        self.signals.remove(&sig);
    }

    pub fn contains(&self, sig: Signal) -> bool {
        self.signals.contains(&sig)
    }

    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    pub fn len(&self) -> usize {
        self.signals.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Signal> {
        self.signals.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_from_i32() {
        assert_eq!(Signal::from_i32(1), Some(Signal::SIGHUP));
        assert_eq!(Signal::from_i32(9), Some(Signal::SIGKILL));
        assert_eq!(Signal::from_i32(15), Some(Signal::SIGTERM));
        assert_eq!(Signal::from_i32(100), None);
    }

    #[test]
    fn test_signal_can_be_caught() {
        assert!(Signal::SIGTERM.can_be_caught());
        assert!(Signal::SIGINT.can_be_caught());
        assert!(!Signal::SIGKILL.can_be_caught());
        assert!(!Signal::SIGSTOP.can_be_caught());
    }

    #[test]
    fn test_signal_default_action() {
        assert_eq!(Signal::SIGTERM.default_action(), SignalAction::Terminate);
        assert_eq!(Signal::SIGSTOP.default_action(), SignalAction::Stop);
        assert_eq!(Signal::SIGCONT.default_action(), SignalAction::Continue);
        assert_eq!(Signal::SIGCHLD.default_action(), SignalAction::Ignore);
    }

    #[test]
    fn test_signal_mask() {
        let mut mask = SignalMask::new();
        
        assert!(!mask.is_blocked(Signal::SIGINT));
        
        mask.block(Signal::SIGINT);
        assert!(mask.is_blocked(Signal::SIGINT));
        
        mask.unblock(Signal::SIGINT);
        assert!(!mask.is_blocked(Signal::SIGINT));
    }

    #[test]
    fn test_signal_mask_uncatchable() {
        let mut mask = SignalMask::new();
        
        // Cannot block SIGKILL or SIGSTOP
        assert!(!mask.block(Signal::SIGKILL));
        assert!(!mask.block(Signal::SIGSTOP));
        assert!(!mask.is_blocked(Signal::SIGKILL));
        assert!(!mask.is_blocked(Signal::SIGSTOP));
    }

    #[test]
    fn test_signal_state_basic() {
        let mut state = MockSignalState::new(100);
        
        state.send_signal(Signal::SIGTERM, 1);
        assert!(state.has_pending());
        
        let (sig, action) = state.deliver_signal().unwrap();
        assert_eq!(sig, Signal::SIGTERM);
        assert_eq!(action, SignalAction::Terminate);
        assert!(state.is_terminated());
    }

    #[test]
    fn test_signal_state_blocked() {
        let mut state = MockSignalState::new(100);
        
        state.block_signal(Signal::SIGINT);
        state.send_signal(Signal::SIGINT, 1);
        
        // Signal is blocked, cannot deliver
        assert!(!state.has_pending());
        assert!(state.deliver_signal().is_none());
        
        // Unblock and deliver
        state.unblock_signal(Signal::SIGINT);
        assert!(state.has_pending());
        assert!(state.deliver_signal().is_some());
    }

    #[test]
    fn test_signal_state_ignore() {
        let mut state = MockSignalState::new(100);
        
        state.set_handler(Signal::SIGINT, SignalHandler {
            action: SignalAction::Ignore,
            ..Default::default()
        }).unwrap();
        
        state.send_signal(Signal::SIGINT, 1);
        let (_, action) = state.deliver_signal().unwrap();
        
        assert_eq!(action, SignalAction::Ignore);
        assert!(!state.is_terminated());
    }

    #[test]
    fn test_signal_state_cannot_catch_sigkill() {
        let mut state = MockSignalState::new(100);
        
        let result = state.set_handler(Signal::SIGKILL, SignalHandler {
            action: SignalAction::Ignore,
            ..Default::default()
        });
        
        assert!(result.is_err());
    }

    #[test]
    fn test_signal_state_stop_continue() {
        let mut state = MockSignalState::new(100);
        
        state.send_signal(Signal::SIGSTOP, 1);
        state.deliver_signal();
        assert!(state.is_stopped());
        
        state.send_signal(Signal::SIGCONT, 1);
        assert!(!state.is_stopped()); // SIGCONT resumes immediately
    }

    #[test]
    fn test_signal_state_pending_count() {
        let mut state = MockSignalState::new(100);
        
        state.send_signal(Signal::SIGUSR1, 1);
        state.send_signal(Signal::SIGUSR2, 1);
        state.send_signal(Signal::SIGALRM, 1);
        
        assert_eq!(state.pending_count(), 3);
        
        state.deliver_signal();
        assert_eq!(state.pending_count(), 2);
    }

    #[test]
    fn test_signal_set() {
        let mut set = SignalSet::new();
        assert!(set.is_empty());
        
        set.add(Signal::SIGINT);
        set.add(Signal::SIGTERM);
        
        assert!(set.contains(Signal::SIGINT));
        assert!(set.contains(Signal::SIGTERM));
        assert!(!set.contains(Signal::SIGHUP));
        assert_eq!(set.len(), 2);
        
        set.remove(Signal::SIGINT);
        assert!(!set.contains(Signal::SIGINT));
    }

    #[test]
    fn test_signal_set_full() {
        let set = SignalSet::full();
        assert!(!set.is_empty());
        assert!(set.contains(Signal::SIGINT));
        assert!(set.contains(Signal::SIGTERM));
        assert!(set.contains(Signal::SIGKILL));
    }

    #[test]
    fn test_signal_generates_core() {
        assert!(Signal::SIGABRT.generates_core());
        assert!(Signal::SIGSEGV.generates_core());
        assert!(Signal::SIGQUIT.generates_core());
        assert!(!Signal::SIGTERM.generates_core());
        assert!(!Signal::SIGKILL.generates_core());
    }

    #[test]
    fn test_signal_handler_custom() {
        let mut state = MockSignalState::new(100);
        
        let handler = SignalHandler {
            action: SignalAction::Handler(0xDEADBEEF),
            mask: SignalMask::new(),
            flags: 0,
        };
        
        state.set_handler(Signal::SIGUSR1, handler).unwrap();
        state.send_signal(Signal::SIGUSR1, 1);
        
        let (sig, action) = state.deliver_signal().unwrap();
        assert_eq!(sig, Signal::SIGUSR1);
        assert_eq!(action, SignalAction::Handler(0xDEADBEEF));
    }

    #[test]
    fn test_signal_priority() {
        let mut state = MockSignalState::new(100);
        
        // Block SIGUSR1
        state.block_signal(Signal::SIGUSR1);
        
        // Send blocked and unblocked signals
        state.send_signal(Signal::SIGUSR1, 1);
        state.send_signal(Signal::SIGUSR2, 1);
        
        // Should deliver SIGUSR2 first (unblocked)
        let (sig, _) = state.deliver_signal().unwrap();
        assert_eq!(sig, Signal::SIGUSR2);
    }
}
