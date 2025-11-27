/// POSIX pipe implementation for IPC
use spin::Mutex;

const PIPE_BUF_SIZE: usize = 4096; // POSIX minimum
const MAX_PIPES: usize = 16;

/// Pipe state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeState {
    Open,
    ReadClosed,
    WriteClosed,
    Closed,
}

/// A single pipe buffer
#[derive(Clone, Copy)]
struct PipeBuffer {
    data: [u8; PIPE_BUF_SIZE],
    read_pos: usize,
    write_pos: usize,
    count: usize,
    state: PipeState,
}

impl PipeBuffer {
    const fn new() -> Self {
        Self {
            data: [0; PIPE_BUF_SIZE],
            read_pos: 0,
            write_pos: 0,
            count: 0,
            state: PipeState::Closed,
        }
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn is_full(&self) -> bool {
        self.count >= PIPE_BUF_SIZE
    }

    fn available_space(&self) -> usize {
        PIPE_BUF_SIZE - self.count
    }

    fn write(&mut self, data: &[u8]) -> usize {
        let available = self.available_space();
        let to_write = core::cmp::min(data.len(), available);

        for i in 0..to_write {
            self.data[self.write_pos] = data[i];
            self.write_pos = (self.write_pos + 1) % PIPE_BUF_SIZE;
        }

        self.count += to_write;
        to_write
    }

    fn read(&mut self, buffer: &mut [u8]) -> usize {
        let to_read = core::cmp::min(buffer.len(), self.count);

        for i in 0..to_read {
            buffer[i] = self.data[self.read_pos];
            self.read_pos = (self.read_pos + 1) % PIPE_BUF_SIZE;
        }

        self.count -= to_read;
        to_read
    }
}

static PIPES: Mutex<[PipeBuffer; MAX_PIPES]> = Mutex::new([PipeBuffer::new(); MAX_PIPES]);

/// Pipe descriptor (index into PIPES array)
pub type PipeId = usize;

/// Create a new pipe, returning (read_end, write_end) pipe IDs
pub fn create_pipe() -> Result<(PipeId, PipeId), &'static str> {
    let mut pipes = PIPES.lock();

    // Find a free slot
    for (idx, pipe) in pipes.iter_mut().enumerate() {
        if pipe.state == PipeState::Closed {
            *pipe = PipeBuffer::new();
            pipe.state = PipeState::Open;

            // For now, return the same index for both ends
            // In a full implementation, we'd track read/write ends separately
            return Ok((idx, idx));
        }
    }

    Err("Too many pipes open")
}

/// Read from a pipe
pub fn pipe_read(pipe_id: PipeId, buffer: &mut [u8]) -> Result<usize, &'static str> {
    let mut pipes = PIPES.lock();

    if pipe_id >= MAX_PIPES {
        return Err("Invalid pipe ID");
    }

    let pipe = &mut pipes[pipe_id];

    match pipe.state {
        PipeState::Closed => Err("Pipe is closed"),
        PipeState::WriteClosed if pipe.is_empty() => Ok(0), // EOF
        _ => Ok(pipe.read(buffer)),
    }
}

/// Write to a pipe
pub fn pipe_write(pipe_id: PipeId, data: &[u8]) -> Result<usize, &'static str> {
    let mut pipes = PIPES.lock();

    if pipe_id >= MAX_PIPES {
        return Err("Invalid pipe ID");
    }

    let pipe = &mut pipes[pipe_id];

    match pipe.state {
        PipeState::Closed | PipeState::ReadClosed => Err("Pipe read end closed (SIGPIPE)"),
        _ => {
            if pipe.is_full() {
                Err("Pipe buffer full (would block)")
            } else {
                Ok(pipe.write(data))
            }
        }
    }
}

/// Close the read end of a pipe
pub fn close_pipe_read(pipe_id: PipeId) -> Result<(), &'static str> {
    let mut pipes = PIPES.lock();

    if pipe_id >= MAX_PIPES {
        return Err("Invalid pipe ID");
    }

    let pipe = &mut pipes[pipe_id];

    match pipe.state {
        PipeState::Open => {
            pipe.state = PipeState::ReadClosed;
            Ok(())
        }
        PipeState::WriteClosed => {
            pipe.state = PipeState::Closed;
            Ok(())
        }
        _ => Err("Invalid pipe state"),
    }
}

/// Close the write end of a pipe
pub fn close_pipe_write(pipe_id: PipeId) -> Result<(), &'static str> {
    let mut pipes = PIPES.lock();

    if pipe_id >= MAX_PIPES {
        return Err("Invalid pipe ID");
    }

    let pipe = &mut pipes[pipe_id];

    match pipe.state {
        PipeState::Open => {
            pipe.state = PipeState::WriteClosed;
            Ok(())
        }
        PipeState::ReadClosed => {
            pipe.state = PipeState::Closed;
            Ok(())
        }
        _ => Err("Invalid pipe state"),
    }
}

/// Initialize pipe subsystem
pub fn init() {
    crate::kinfo!(
        "POSIX pipe subsystem initialized ({} pipes, {} bytes each)",
        MAX_PIPES,
        PIPE_BUF_SIZE
    );
}

// ============================================================================
// Socketpair implementation (bidirectional pipes for AF_UNIX sockets)
// ============================================================================

const MAX_SOCKETPAIRS: usize = 8;

/// Socketpair state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketpairState {
    Open,
    FirstClosed,
    SecondClosed,
    Closed,
}

/// A socketpair consists of two bidirectional pipe buffers
#[derive(Clone, Copy)]
pub struct SocketpairBuffer {
    /// Data from socket[0] to socket[1]
    buf_0_to_1: [u8; PIPE_BUF_SIZE],
    read_pos_0_to_1: usize,
    write_pos_0_to_1: usize,
    count_0_to_1: usize,
    /// Data from socket[1] to socket[0]
    buf_1_to_0: [u8; PIPE_BUF_SIZE],
    read_pos_1_to_0: usize,
    write_pos_1_to_0: usize,
    count_1_to_0: usize,
    /// State
    state: SocketpairState,
}

impl SocketpairBuffer {
    const fn new() -> Self {
        Self {
            buf_0_to_1: [0; PIPE_BUF_SIZE],
            read_pos_0_to_1: 0,
            write_pos_0_to_1: 0,
            count_0_to_1: 0,
            buf_1_to_0: [0; PIPE_BUF_SIZE],
            read_pos_1_to_0: 0,
            write_pos_1_to_0: 0,
            count_1_to_0: 0,
            state: SocketpairState::Closed,
        }
    }

    /// Write from socket 0 (data goes to buf_0_to_1, readable by socket 1)
    fn write_from_0(&mut self, data: &[u8]) -> usize {
        let available = PIPE_BUF_SIZE - self.count_0_to_1;
        let to_write = core::cmp::min(data.len(), available);

        for i in 0..to_write {
            self.buf_0_to_1[self.write_pos_0_to_1] = data[i];
            self.write_pos_0_to_1 = (self.write_pos_0_to_1 + 1) % PIPE_BUF_SIZE;
        }
        self.count_0_to_1 += to_write;
        to_write
    }

    /// Write from socket 1 (data goes to buf_1_to_0, readable by socket 0)
    fn write_from_1(&mut self, data: &[u8]) -> usize {
        let available = PIPE_BUF_SIZE - self.count_1_to_0;
        let to_write = core::cmp::min(data.len(), available);

        for i in 0..to_write {
            self.buf_1_to_0[self.write_pos_1_to_0] = data[i];
            self.write_pos_1_to_0 = (self.write_pos_1_to_0 + 1) % PIPE_BUF_SIZE;
        }
        self.count_1_to_0 += to_write;
        to_write
    }

    /// Read to socket 0 (data comes from buf_1_to_0, written by socket 1)
    fn read_to_0(&mut self, buffer: &mut [u8]) -> usize {
        let to_read = core::cmp::min(buffer.len(), self.count_1_to_0);

        for i in 0..to_read {
            buffer[i] = self.buf_1_to_0[self.read_pos_1_to_0];
            self.read_pos_1_to_0 = (self.read_pos_1_to_0 + 1) % PIPE_BUF_SIZE;
        }
        self.count_1_to_0 -= to_read;
        to_read
    }

    /// Read to socket 1 (data comes from buf_0_to_1, written by socket 0)
    fn read_to_1(&mut self, buffer: &mut [u8]) -> usize {
        let to_read = core::cmp::min(buffer.len(), self.count_0_to_1);

        for i in 0..to_read {
            buffer[i] = self.buf_0_to_1[self.read_pos_0_to_1];
            self.read_pos_0_to_1 = (self.read_pos_0_to_1 + 1) % PIPE_BUF_SIZE;
        }
        self.count_0_to_1 -= to_read;
        to_read
    }

    fn is_read_empty_for_0(&self) -> bool {
        self.count_1_to_0 == 0
    }

    fn is_read_empty_for_1(&self) -> bool {
        self.count_0_to_1 == 0
    }

    fn is_write_full_for_0(&self) -> bool {
        self.count_0_to_1 >= PIPE_BUF_SIZE
    }

    fn is_write_full_for_1(&self) -> bool {
        self.count_1_to_0 >= PIPE_BUF_SIZE
    }
}

static SOCKETPAIRS: Mutex<[SocketpairBuffer; MAX_SOCKETPAIRS]> =
    Mutex::new([SocketpairBuffer::new(); MAX_SOCKETPAIRS]);

/// Socketpair ID (index into SOCKETPAIRS array)
pub type SocketpairId = usize;

/// Create a new socketpair, returning the socketpair index
pub fn create_socketpair() -> Result<SocketpairId, &'static str> {
    let mut pairs = SOCKETPAIRS.lock();

    for (idx, pair) in pairs.iter_mut().enumerate() {
        if pair.state == SocketpairState::Closed {
            *pair = SocketpairBuffer::new();
            pair.state = SocketpairState::Open;
            return Ok(idx);
        }
    }

    Err("Too many socketpairs open")
}

/// Read from a socketpair end
/// `end` is 0 or 1 indicating which socket end is reading
pub fn socketpair_read(
    pair_id: SocketpairId,
    end: usize,
    buffer: &mut [u8],
) -> Result<usize, &'static str> {
    let mut pairs = SOCKETPAIRS.lock();

    if pair_id >= MAX_SOCKETPAIRS {
        return Err("Invalid socketpair ID");
    }

    let pair = &mut pairs[pair_id];

    match pair.state {
        SocketpairState::Closed => Err("Socketpair is closed"),
        SocketpairState::FirstClosed if end == 0 => Err("This socket end is closed"),
        SocketpairState::SecondClosed if end == 1 => Err("This socket end is closed"),
        _ => {
            let bytes = if end == 0 {
                pair.read_to_0(buffer)
            } else {
                pair.read_to_1(buffer)
            };
            Ok(bytes)
        }
    }
}

/// Write to a socketpair end
/// `end` is 0 or 1 indicating which socket end is writing
pub fn socketpair_write(
    pair_id: SocketpairId,
    end: usize,
    data: &[u8],
) -> Result<usize, &'static str> {
    let mut pairs = SOCKETPAIRS.lock();

    if pair_id >= MAX_SOCKETPAIRS {
        return Err("Invalid socketpair ID");
    }

    let pair = &mut pairs[pair_id];

    match pair.state {
        SocketpairState::Closed => Err("Socketpair is closed"),
        SocketpairState::FirstClosed if end == 0 => Err("This socket end is closed"),
        SocketpairState::SecondClosed if end == 1 => Err("This socket end is closed"),
        // Check if peer is closed (SIGPIPE condition)
        SocketpairState::FirstClosed if end == 1 => Err("Peer socket closed (SIGPIPE)"),
        SocketpairState::SecondClosed if end == 0 => Err("Peer socket closed (SIGPIPE)"),
        _ => {
            let is_full = if end == 0 {
                pair.is_write_full_for_0()
            } else {
                pair.is_write_full_for_1()
            };

            if is_full {
                Err("Socketpair buffer full (would block)")
            } else {
                let bytes = if end == 0 {
                    pair.write_from_0(data)
                } else {
                    pair.write_from_1(data)
                };
                Ok(bytes)
            }
        }
    }
}

/// Close one end of a socketpair
pub fn close_socketpair_end(pair_id: SocketpairId, end: usize) -> Result<(), &'static str> {
    let mut pairs = SOCKETPAIRS.lock();

    if pair_id >= MAX_SOCKETPAIRS {
        return Err("Invalid socketpair ID");
    }

    let pair = &mut pairs[pair_id];

    match (pair.state, end) {
        (SocketpairState::Open, 0) => {
            pair.state = SocketpairState::FirstClosed;
            Ok(())
        }
        (SocketpairState::Open, 1) => {
            pair.state = SocketpairState::SecondClosed;
            Ok(())
        }
        (SocketpairState::FirstClosed, 1) | (SocketpairState::SecondClosed, 0) => {
            pair.state = SocketpairState::Closed;
            Ok(())
        }
        _ => Err("Invalid socketpair state or already closed"),
    }
}

/// Check if socketpair has data available for reading on given end
pub fn socketpair_has_data(pair_id: SocketpairId, end: usize) -> Result<bool, &'static str> {
    let pairs = SOCKETPAIRS.lock();

    if pair_id >= MAX_SOCKETPAIRS {
        return Err("Invalid socketpair ID");
    }

    let pair = &pairs[pair_id];

    match pair.state {
        SocketpairState::Closed => Err("Socketpair is closed"),
        _ => {
            let has_data = if end == 0 {
                !pair.is_read_empty_for_0()
            } else {
                !pair.is_read_empty_for_1()
            };
            Ok(has_data)
        }
    }
}
