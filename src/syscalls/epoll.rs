//! Epoll syscall implementation
//!
//! Provides epoll_create1, epoll_ctl, epoll_wait, and eventfd for async I/O.

use crate::posix;
use crate::{kinfo, kwarn};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::types::*;

// ============================================================================
// Epoll Constants
// ============================================================================

pub const EPOLLIN: u32 = 0x001;
pub const EPOLLPRI: u32 = 0x002;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLRDHUP: u32 = 0x2000;
pub const EPOLLEXCLUSIVE: u32 = 1 << 28;
pub const EPOLLWAKEUP: u32 = 1 << 29;
pub const EPOLLONESHOT: u32 = 1 << 30;
pub const EPOLLET: u32 = 1 << 31;

pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

pub const EPOLL_CLOEXEC: i32 = 0x80000;

// Eventfd flags
pub const EFD_SEMAPHORE: u32 = 1;
pub const EFD_CLOEXEC: u32 = 0x80000;
pub const EFD_NONBLOCK: u32 = 0x800;

// ============================================================================
// Epoll Data Structures
// ============================================================================

/// Epoll event (matches Linux layout)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

/// Registered file descriptor in epoll
#[derive(Clone)]
struct EpollEntry {
    events: u32,
    data: u64,
}

/// Epoll instance
struct EpollInstance {
    entries: BTreeMap<i32, EpollEntry>,
    flags: i32,
}

impl EpollInstance {
    fn new(flags: i32) -> Self {
        Self {
            entries: BTreeMap::new(),
            flags,
        }
    }
}

/// Eventfd instance
struct EventFd {
    counter: AtomicU64,
    flags: u32,
}

impl EventFd {
    fn new(initval: u32, flags: u32) -> Self {
        Self {
            counter: AtomicU64::new(initval as u64),
            flags,
        }
    }
}

// ============================================================================
// Global State
// ============================================================================

const MAX_EPOLL_INSTANCES: usize = 64;
const MAX_EVENTFD_INSTANCES: usize = 64;

static EPOLL_INSTANCES: Mutex<[Option<EpollInstance>; MAX_EPOLL_INSTANCES]> =
    Mutex::new([const { None }; MAX_EPOLL_INSTANCES]);
static EVENTFD_INSTANCES: Mutex<[Option<EventFd>; MAX_EVENTFD_INSTANCES]> =
    Mutex::new([const { None }; MAX_EVENTFD_INSTANCES]);

// File descriptor base for epoll/eventfd (high range to avoid conflicts)
const EPOLL_FD_BASE: u64 = 0x10000;
const EVENTFD_FD_BASE: u64 = 0x20000;

// ============================================================================
// Epoll Syscalls
// ============================================================================

/// SYS_EPOLL_CREATE1 - Create epoll instance
pub fn epoll_create1(flags: i32) -> u64 {
    kinfo!("[SYS_EPOLL_CREATE1] flags={:#x}", flags);

    if flags != 0 && flags != EPOLL_CLOEXEC {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let mut instances = EPOLL_INSTANCES.lock();
    for (i, slot) in instances.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(EpollInstance::new(flags));
            let fd = EPOLL_FD_BASE + i as u64;
            kinfo!("[SYS_EPOLL_CREATE1] Created epoll instance, fd={}", fd);
            posix::set_errno(0);
            return fd;
        }
    }

    kwarn!("[SYS_EPOLL_CREATE1] No free epoll slots");
    posix::set_errno(posix::errno::EMFILE);
    u64::MAX
}

/// SYS_EPOLL_CTL - Control epoll instance
pub fn epoll_ctl(epfd: u64, op: i32, fd: i32, event: *const EpollEvent) -> u64 {
    kinfo!(
        "[SYS_EPOLL_CTL] epfd={} op={} fd={} event={:?}",
        epfd,
        op,
        fd,
        event
    );

    if epfd < EPOLL_FD_BASE || epfd >= EPOLL_FD_BASE + MAX_EPOLL_INSTANCES as u64 {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    let idx = (epfd - EPOLL_FD_BASE) as usize;
    let mut instances = EPOLL_INSTANCES.lock();

    let instance = match instances[idx].as_mut() {
        Some(inst) => inst,
        None => {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        }
    };

    match op {
        EPOLL_CTL_ADD => {
            if event.is_null() {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }

            if instance.entries.contains_key(&fd) {
                posix::set_errno(posix::errno::EEXIST);
                return u64::MAX;
            }

            let ev = unsafe { &*event };
            let ev_events = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(ev.events)) };
            let ev_data = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(ev.data)) };
            instance.entries.insert(
                fd,
                EpollEntry {
                    events: ev_events,
                    data: ev_data,
                },
            );
            kinfo!(
                "[SYS_EPOLL_CTL] Added fd {} with events {:#x}",
                fd,
                ev_events
            );
        }
        EPOLL_CTL_DEL => {
            if instance.entries.remove(&fd).is_none() {
                posix::set_errno(posix::errno::ENOENT);
                return u64::MAX;
            }
            kinfo!("[SYS_EPOLL_CTL] Removed fd {}", fd);
        }
        EPOLL_CTL_MOD => {
            if event.is_null() {
                posix::set_errno(posix::errno::EFAULT);
                return u64::MAX;
            }

            let entry = match instance.entries.get_mut(&fd) {
                Some(e) => e,
                None => {
                    posix::set_errno(posix::errno::ENOENT);
                    return u64::MAX;
                }
            };

            let ev = unsafe { &*event };
            let ev_events = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(ev.events)) };
            let ev_data = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(ev.data)) };
            entry.events = ev_events;
            entry.data = ev_data;
            kinfo!(
                "[SYS_EPOLL_CTL] Modified fd {} with events {:#x}",
                fd,
                ev_events
            );
        }
        _ => {
            posix::set_errno(posix::errno::EINVAL);
            return u64::MAX;
        }
    }

    posix::set_errno(0);
    0
}

/// SYS_EPOLL_WAIT - Wait for events
pub fn epoll_wait(epfd: u64, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> u64 {
    kinfo!(
        "[SYS_EPOLL_WAIT] epfd={} maxevents={} timeout={}",
        epfd,
        maxevents,
        timeout
    );

    if maxevents <= 0 || events.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    if epfd < EPOLL_FD_BASE || epfd >= EPOLL_FD_BASE + MAX_EPOLL_INSTANCES as u64 {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    let idx = (epfd - EPOLL_FD_BASE) as usize;

    // For now, implement a simple poll-based approach
    // In a real implementation, this would block and wake on events
    let instances = EPOLL_INSTANCES.lock();
    let instance = match instances[idx].as_ref() {
        Some(inst) => inst,
        None => {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        }
    };

    // Check each registered fd for readiness
    // For now, assume all sockets are ready for write and eventfds are ready for read
    let mut count = 0;
    for (&fd, entry) in instance.entries.iter() {
        if count >= maxevents as usize {
            break;
        }

        let mut ready_events = 0u32;

        // Check if this is an eventfd
        if fd as u64 >= EVENTFD_FD_BASE
            && (fd as u64) < EVENTFD_FD_BASE + MAX_EVENTFD_INSTANCES as u64
        {
            let efd_idx = (fd as u64 - EVENTFD_FD_BASE) as usize;
            let eventfds = EVENTFD_INSTANCES.lock();
            if let Some(ref efd) = eventfds[efd_idx] {
                if efd.counter.load(Ordering::Relaxed) > 0 {
                    ready_events |= EPOLLIN;
                }
            }
            ready_events |= EPOLLOUT; // eventfds are always writable
        } else {
            // For regular fds, assume writable (simplified)
            if entry.events & EPOLLOUT != 0 {
                ready_events |= EPOLLOUT;
            }
        }

        if ready_events != 0 {
            unsafe {
                let event_ptr = events.add(count);
                (*event_ptr).events = ready_events & entry.events;
                (*event_ptr).data = entry.data;
            }
            count += 1;
        }
    }

    // If no events and timeout is non-zero, we should block
    // For now, just return 0 events (timeout)
    if count == 0 && timeout != 0 {
        // TODO: Implement proper blocking with timeout
        // For now, yield and return immediately
        drop(instances);
        crate::scheduler::do_schedule();
    }

    kinfo!("[SYS_EPOLL_WAIT] Returning {} events", count);
    posix::set_errno(0);
    count as u64
}

/// SYS_EPOLL_PWAIT - Wait for events with signal mask
pub fn epoll_pwait(
    epfd: u64,
    events: *mut EpollEvent,
    maxevents: i32,
    timeout: i32,
    _sigmask: *const u64,
    _sigsetsize: usize,
) -> u64 {
    // For now, ignore signal mask and call regular epoll_wait
    epoll_wait(epfd, events, maxevents, timeout)
}

// ============================================================================
// Eventfd Syscalls
// ============================================================================

/// SYS_EVENTFD2 - Create eventfd
pub fn eventfd2(initval: u32, flags: u32) -> u64 {
    kinfo!("[SYS_EVENTFD2] initval={} flags={:#x}", initval, flags);

    let valid_flags = EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE;
    if flags & !valid_flags != 0 {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let mut instances = EVENTFD_INSTANCES.lock();
    for (i, slot) in instances.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(EventFd::new(initval, flags));
            let fd = EVENTFD_FD_BASE + i as u64;
            kinfo!("[SYS_EVENTFD2] Created eventfd, fd={}", fd);
            posix::set_errno(0);
            return fd;
        }
    }

    kwarn!("[SYS_EVENTFD2] No free eventfd slots");
    posix::set_errno(posix::errno::EMFILE);
    u64::MAX
}

/// Read from eventfd
pub fn eventfd_read(fd: u64, buf: *mut u64) -> u64 {
    if fd < EVENTFD_FD_BASE || fd >= EVENTFD_FD_BASE + MAX_EVENTFD_INSTANCES as u64 {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    let idx = (fd - EVENTFD_FD_BASE) as usize;
    let instances = EVENTFD_INSTANCES.lock();

    let efd = match instances[idx].as_ref() {
        Some(e) => e,
        None => {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        }
    };

    let value = if efd.flags & EFD_SEMAPHORE != 0 {
        // Semaphore mode: decrement by 1
        loop {
            let current = efd.counter.load(Ordering::Relaxed);
            if current == 0 {
                if efd.flags & EFD_NONBLOCK != 0 {
                    posix::set_errno(posix::errno::EAGAIN);
                    return u64::MAX;
                }
                // Would block - for now return EAGAIN
                posix::set_errno(posix::errno::EAGAIN);
                return u64::MAX;
            }
            if efd
                .counter
                .compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break 1u64;
            }
        }
    } else {
        // Normal mode: read and reset to 0
        let value = efd.counter.swap(0, Ordering::SeqCst);
        if value == 0 {
            if efd.flags & EFD_NONBLOCK != 0 {
                posix::set_errno(posix::errno::EAGAIN);
                return u64::MAX;
            }
            posix::set_errno(posix::errno::EAGAIN);
            return u64::MAX;
        }
        value
    };

    unsafe {
        *buf = value;
    }
    posix::set_errno(0);
    8 // Return bytes read
}

/// Write to eventfd
pub fn eventfd_write(fd: u64, value: u64) -> u64 {
    if fd < EVENTFD_FD_BASE || fd >= EVENTFD_FD_BASE + MAX_EVENTFD_INSTANCES as u64 {
        posix::set_errno(posix::errno::EBADF);
        return u64::MAX;
    }

    if value == u64::MAX {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let idx = (fd - EVENTFD_FD_BASE) as usize;
    let instances = EVENTFD_INSTANCES.lock();

    let efd = match instances[idx].as_ref() {
        Some(e) => e,
        None => {
            posix::set_errno(posix::errno::EBADF);
            return u64::MAX;
        }
    };

    // Add value to counter (with overflow check)
    loop {
        let current = efd.counter.load(Ordering::Relaxed);
        let new_value = match current.checked_add(value) {
            Some(v) if v < u64::MAX - 1 => v,
            _ => {
                if efd.flags & EFD_NONBLOCK != 0 {
                    posix::set_errno(posix::errno::EAGAIN);
                    return u64::MAX;
                }
                posix::set_errno(posix::errno::EAGAIN);
                return u64::MAX;
            }
        };

        if efd
            .counter
            .compare_exchange(current, new_value, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            break;
        }
    }

    posix::set_errno(0);
    8 // Return bytes written
}

/// Close epoll/eventfd
pub fn close_epoll_or_eventfd(fd: u64) -> bool {
    if fd >= EPOLL_FD_BASE && fd < EPOLL_FD_BASE + MAX_EPOLL_INSTANCES as u64 {
        let idx = (fd - EPOLL_FD_BASE) as usize;
        let mut instances = EPOLL_INSTANCES.lock();
        if instances[idx].is_some() {
            instances[idx] = None;
            return true;
        }
    } else if fd >= EVENTFD_FD_BASE && fd < EVENTFD_FD_BASE + MAX_EVENTFD_INSTANCES as u64 {
        let idx = (fd - EVENTFD_FD_BASE) as usize;
        let mut instances = EVENTFD_INSTANCES.lock();
        if instances[idx].is_some() {
            instances[idx] = None;
            return true;
        }
    }
    false
}

/// Check if fd is epoll/eventfd
pub fn is_epoll_or_eventfd(fd: u64) -> bool {
    (fd >= EPOLL_FD_BASE && fd < EPOLL_FD_BASE + MAX_EPOLL_INSTANCES as u64)
        || (fd >= EVENTFD_FD_BASE && fd < EVENTFD_FD_BASE + MAX_EVENTFD_INSTANCES as u64)
}
