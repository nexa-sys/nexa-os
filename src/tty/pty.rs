//! Pseudo-terminal (PTY) subsystem
//!
//! Provides a minimal Linux-compatible PTY interface:
//! - `/dev/ptmx` allocates a PTY master
//! - `/dev/pts/<n>` opens the corresponding slave (after unlock)
//! - Basic ioctls: TIOCGPTN, TIOCSPTLCK, TCGETS/TCSETS*, TIOCGWINSZ/TIOCSWINSZ, FIONREAD

use crate::process::Pid;
use spin::Mutex;

const MAX_PTYS: usize = 32;
const BUF_SIZE: usize = 4096;

// Linux termios layout as used by nrlib (`userspace/nrlib/src/libc_compat/io.rs`).
const NCCS: usize = 32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; NCCS],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl Termios {
    const fn default_raw() -> Self {
        Self {
            c_iflag: 0,
            c_oflag: 1,      // OPOST
            c_cflag: 0o60,   // CS8
            c_lflag: 0,
            c_line: 0,
            c_cc: [0; NCCS],
            c_ispeed: 38400,
            c_ospeed: 38400,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl WinSize {
    const fn default_80x25() -> Self {
        Self {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct RingBuf {
    data: [u8; BUF_SIZE],
    r: usize,
    w: usize,
    count: usize,
}

impl RingBuf {
    const fn new() -> Self {
        Self {
            data: [0; BUF_SIZE],
            r: 0,
            w: 0,
            count: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn is_full(&self) -> bool {
        self.count >= BUF_SIZE
    }

    fn available_data(&self) -> usize {
        self.count
    }

    fn write(&mut self, src: &[u8]) -> usize {
        let mut written = 0;
        while written < src.len() && !self.is_full() {
            self.data[self.w] = src[written];
            self.w = (self.w + 1) % BUF_SIZE;
            self.count += 1;
            written += 1;
        }
        written
    }

    fn read(&mut self, dst: &mut [u8]) -> usize {
        let mut read = 0;
        while read < dst.len() && !self.is_empty() {
            dst[read] = self.data[self.r];
            self.r = (self.r + 1) % BUF_SIZE;
            self.count -= 1;
            read += 1;
        }
        read
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyDirection {
    MasterReads,
    SlaveReads,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyIoResult {
    Bytes(usize),
    WouldBlock,
    Eof,
}

#[derive(Clone, Copy)]
struct Pty {
    allocated: bool,
    locked: bool,

    master_open: bool,
    slave_open: bool,

    // Data written by master is read by slave.
    m2s: RingBuf,
    // Data written by slave is read by master.
    s2m: RingBuf,

    master_waiter: Option<Pid>,
    slave_waiter: Option<Pid>,

    termios: Termios,
    winsize: WinSize,
}

impl Pty {
    const fn new() -> Self {
        Self {
            allocated: false,
            locked: true,
            master_open: false,
            slave_open: false,
            m2s: RingBuf::new(),
            s2m: RingBuf::new(),
            master_waiter: None,
            slave_waiter: None,
            termios: Termios::default_raw(),
            winsize: WinSize::default_80x25(),
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
        self.allocated = true;
        self.master_open = true;
        self.locked = true;
    }

    fn free_if_unused(&mut self) {
        if self.allocated && !self.master_open && !self.slave_open {
            *self = Self::new();
        }
    }
}

static PTYS: Mutex<[Pty; MAX_PTYS]> = Mutex::new([Pty::new(); MAX_PTYS]);

pub fn is_allocated(id: usize) -> bool {
    let ptys = PTYS.lock();
    id < MAX_PTYS && ptys[id].allocated
}

pub fn list_allocated_ids(mut cb: impl FnMut(usize)) {
    let ptys = PTYS.lock();
    for (i, pty) in ptys.iter().enumerate() {
        if pty.allocated {
            cb(i);
        }
    }
}

pub fn allocate_ptmx() -> Option<usize> {
    let mut ptys = PTYS.lock();
    for (i, pty) in ptys.iter_mut().enumerate() {
        if !pty.allocated {
            pty.reset();
            return Some(i);
        }
    }
    None
}

pub fn open_slave(id: usize) -> Result<(), ()> {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS {
        return Err(());
    }
    let pty = &mut ptys[id];
    if !pty.allocated || pty.master_open == false {
        return Err(());
    }
    if pty.locked {
        return Err(());
    }
    pty.slave_open = true;
    Ok(())
}

pub fn close_master(id: usize) {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS {
        return;
    }
    let pty = &mut ptys[id];
    if !pty.allocated {
        return;
    }
    pty.master_open = false;
    if let Some(pid) = pty.slave_waiter.take() {
        crate::scheduler::wake_process(pid);
    }
    if let Some(pid) = pty.master_waiter.take() {
        crate::scheduler::wake_process(pid);
    }
    pty.free_if_unused();
}

pub fn close_slave(id: usize) {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS {
        return;
    }
    let pty = &mut ptys[id];
    if !pty.allocated {
        return;
    }
    pty.slave_open = false;
    if let Some(pid) = pty.master_waiter.take() {
        crate::scheduler::wake_process(pid);
    }
    if let Some(pid) = pty.slave_waiter.take() {
        crate::scheduler::wake_process(pid);
    }
    pty.free_if_unused();
}

pub fn try_read(id: usize, dir: PtyDirection, dst: &mut [u8], waiter: Option<Pid>) -> PtyIoResult {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS {
        return PtyIoResult::Eof;
    }
    let pty = &mut ptys[id];
    if !pty.allocated {
        return PtyIoResult::Eof;
    }

    match dir {
        PtyDirection::MasterReads => {
            if !pty.slave_open && pty.s2m.is_empty() {
                return PtyIoResult::Eof;
            }
            if pty.s2m.is_empty() {
                if let Some(pid) = waiter {
                    pty.master_waiter = Some(pid);
                }
                return PtyIoResult::WouldBlock;
            }
            let n = pty.s2m.read(dst);
            PtyIoResult::Bytes(n)
        }
        PtyDirection::SlaveReads => {
            if !pty.master_open && pty.m2s.is_empty() {
                return PtyIoResult::Eof;
            }
            if pty.m2s.is_empty() {
                if let Some(pid) = waiter {
                    pty.slave_waiter = Some(pid);
                }
                return PtyIoResult::WouldBlock;
            }
            let n = pty.m2s.read(dst);
            PtyIoResult::Bytes(n)
        }
    }
}

pub fn try_write(id: usize, dir: PtyDirection, src: &[u8]) -> PtyIoResult {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS {
        return PtyIoResult::Eof;
    }
    let pty = &mut ptys[id];
    if !pty.allocated {
        return PtyIoResult::Eof;
    }

    match dir {
        // Master writes -> slave reads (m2s)
        PtyDirection::SlaveReads => {
            if !pty.slave_open {
                // No slave end: EPIPE semantics are handled at syscall layer.
                return PtyIoResult::Eof;
            }
            if pty.m2s.is_full() {
                return PtyIoResult::WouldBlock;
            }
            let n = pty.m2s.write(src);
            if n > 0 {
                if let Some(pid) = pty.slave_waiter.take() {
                    crate::scheduler::wake_process(pid);
                }
            }
            PtyIoResult::Bytes(n)
        }
        // Slave writes -> master reads (s2m)
        PtyDirection::MasterReads => {
            if !pty.master_open {
                return PtyIoResult::Eof;
            }
            if pty.s2m.is_full() {
                return PtyIoResult::WouldBlock;
            }
            let n = pty.s2m.write(src);
            if n > 0 {
                if let Some(pid) = pty.master_waiter.take() {
                    crate::scheduler::wake_process(pid);
                }
            }
            PtyIoResult::Bytes(n)
        }
    }
}

pub fn get_termios(id: usize) -> Option<Termios> {
    let ptys = PTYS.lock();
    if id >= MAX_PTYS || !ptys[id].allocated {
        return None;
    }
    Some(ptys[id].termios)
}

pub fn set_termios(id: usize, t: Termios) -> bool {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS || !ptys[id].allocated {
        return false;
    }
    ptys[id].termios = t;
    true
}

pub fn get_winsize(id: usize) -> Option<WinSize> {
    let ptys = PTYS.lock();
    if id >= MAX_PTYS || !ptys[id].allocated {
        return None;
    }
    Some(ptys[id].winsize)
}

pub fn set_winsize(id: usize, w: WinSize) -> bool {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS || !ptys[id].allocated {
        return false;
    }
    ptys[id].winsize = w;
    true
}

pub fn get_ptn(id: usize) -> Option<u32> {
    if !is_allocated(id) {
        return None;
    }
    Some(id as u32)
}

pub fn set_locked(id: usize, locked: bool) -> bool {
    let mut ptys = PTYS.lock();
    if id >= MAX_PTYS || !ptys[id].allocated {
        return false;
    }
    ptys[id].locked = locked;
    true
}

pub fn bytes_available_for_read(id: usize, dir: PtyDirection) -> usize {
    let ptys = PTYS.lock();
    if id >= MAX_PTYS || !ptys[id].allocated {
        return 0;
    }
    match dir {
        PtyDirection::MasterReads => ptys[id].s2m.available_data(),
        PtyDirection::SlaveReads => ptys[id].m2s.available_data(),
    }
}
