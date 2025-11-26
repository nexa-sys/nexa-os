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
