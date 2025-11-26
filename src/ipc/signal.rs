/// POSIX signal handling implementation

/// POSIX signal numbers
pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;

pub const NSIG: usize = 32;

/// Signal action types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalAction {
    Default,
    Ignore,
    Handler(u64), // User-space handler address
}

/// Per-process signal state
#[derive(Clone, Copy)]
pub struct SignalState {
    pending: u64, // Bitmask of pending signals
    blocked: u64, // Bitmask of blocked signals
    actions: [SignalAction; NSIG],
}

impl SignalState {
    pub const fn new() -> Self {
        Self {
            pending: 0,
            blocked: 0,
            actions: [SignalAction::Default; NSIG],
        }
    }

    /// Reset all signal handlers to SIG_DFL (required by POSIX exec)
    pub fn reset_to_default(&mut self) {
        self.pending = 0;
        // Note: blocked mask is preserved across exec per POSIX
        for action in &mut self.actions {
            *action = SignalAction::Default;
        }
    }

    /// Send a signal to this process
    pub fn send_signal(&mut self, signum: u32) -> Result<(), &'static str> {
        if signum == 0 || signum >= NSIG as u32 {
            return Err("Invalid signal number");
        }

        // Set the pending bit for this signal
        self.pending |= 1u64 << signum;
        Ok(())
    }

    /// Check if a signal is pending and not blocked
    pub fn has_pending_signal(&self) -> Option<u32> {
        let deliverable = self.pending & !self.blocked;
        if deliverable == 0 {
            return None;
        }

        // Find the lowest signal number
        for signum in 1..NSIG {
            if (deliverable & (1u64 << signum)) != 0 {
                return Some(signum as u32);
            }
        }
        None
    }

    /// Clear a pending signal
    pub fn clear_signal(&mut self, signum: u32) {
        if signum < NSIG as u32 {
            self.pending &= !(1u64 << signum);
        }
    }

    /// Set signal action
    pub fn set_action(
        &mut self,
        signum: u32,
        action: SignalAction,
    ) -> Result<SignalAction, &'static str> {
        if signum == 0 || signum >= NSIG as u32 {
            return Err("Invalid signal number");
        }

        // SIGKILL and SIGSTOP cannot be caught or ignored
        if signum == SIGKILL || signum == SIGSTOP {
            return Err("Cannot change SIGKILL or SIGSTOP");
        }

        let old_action = self.actions[signum as usize];
        self.actions[signum as usize] = action;
        Ok(old_action)
    }

    /// Get signal action
    pub fn get_action(&self, signum: u32) -> Result<SignalAction, &'static str> {
        if signum == 0 || signum >= NSIG as u32 {
            return Err("Invalid signal number");
        }
        Ok(self.actions[signum as usize])
    }

    /// Block a signal
    pub fn block_signal(&mut self, signum: u32) {
        if signum < NSIG as u32 {
            self.blocked |= 1u64 << signum;
        }
    }

    /// Unblock a signal
    pub fn unblock_signal(&mut self, signum: u32) {
        if signum < NSIG as u32 {
            self.blocked &= !(1u64 << signum);
        }
    }
}

/// Default signal handler behavior
pub fn default_signal_action(signum: u32) -> SignalAction {
    match signum {
        SIGCHLD | SIGCONT => SignalAction::Ignore,
        _ => SignalAction::Default,
    }
}

/// Initialize signal subsystem
pub fn init() {
    crate::kinfo!("Signal handling subsystem initialized");
}
