//! Inter-Process Communication (IPC) subsystem for NexaOS
//!
//! This module contains IPC-related functionality including:
//! - Message passing channels
//! - POSIX pipes
//! - POSIX signal handling

pub mod core;
pub mod pipe;
pub mod signal;

// Re-export commonly used items from core
pub use core::{clear, create_channel, init, receive, send, Channel, IpcError, Message};

// Re-export from pipe
pub use pipe::{
    close_pipe_read, close_pipe_write, create_pipe, init as init_pipes, pipe_read, pipe_write,
    PipeId,
};

// Re-export socketpair functions
pub use pipe::{
    close_socketpair_end, create_socketpair, socketpair_has_data, socketpair_read,
    socketpair_write, SocketpairId,
};

// Re-export from signal
pub use signal::{
    default_signal_action, init as init_signal, SignalAction, SignalState, NSIG, SIGABRT,
    SIGALRM, SIGBUS, SIGCHLD, SIGCONT, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGPIPE,
    SIGQUIT, SIGSEGV, SIGSTOP, SIGTERM, SIGTSTP, SIGTRAP, SIGUSR1, SIGUSR2,
};
