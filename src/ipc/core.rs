use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

const MAX_CHANNELS: usize = 32;
const MAX_MESSAGES: usize = 32;
const MAX_MESSAGE_SIZE: usize = 256;

#[derive(Clone, Copy)]
pub struct Message {
    len: usize,
    data: [u8; MAX_MESSAGE_SIZE],
}

impl Message {
    const fn empty() -> Self {
        Self {
            len: 0,
            data: [0; MAX_MESSAGE_SIZE],
        }
    }
}

#[derive(Clone, Copy)]
pub struct Channel {
    id: u32,
    head: usize,
    tail: usize,
    count: usize,
    messages: [Message; MAX_MESSAGES],
}

impl Channel {
    fn new(id: u32) -> Self {
        Self {
            id,
            head: 0,
            tail: 0,
            count: 0,
            messages: [Message::empty(); MAX_MESSAGES],
        }
    }

    fn push(&mut self, data: &[u8]) -> Result<(), IpcError> {
        if self.count == MAX_MESSAGES {
            return Err(IpcError::WouldBlock);
        }
        let mut msg = Message::empty();
        let len = core::cmp::min(data.len(), MAX_MESSAGE_SIZE);
        msg.data[..len].copy_from_slice(&data[..len]);
        msg.len = len;
        self.messages[self.tail] = msg;
        self.tail = (self.tail + 1) % MAX_MESSAGES;
        self.count += 1;
        Ok(())
    }

    fn pop(&mut self, dest: &mut [u8]) -> Result<usize, IpcError> {
        if self.count == 0 {
            return Err(IpcError::Empty);
        }
        let msg = self.messages[self.head];
        let len = core::cmp::min(msg.len, dest.len());
        dest[..len].copy_from_slice(&msg.data[..len]);
        self.head = (self.head + 1) % MAX_MESSAGES;
        self.count -= 1;
        Ok(len)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IpcError {
    NoSuchChannel,
    TableFull,
    WouldBlock,
    Empty,
    InvalidInput,
}

static CHANNELS: Mutex<[Option<Channel>; MAX_CHANNELS]> = Mutex::new([None; MAX_CHANNELS]);
static NEXT_CHANNEL_ID: AtomicU32 = AtomicU32::new(1);

pub fn init() {
    let mut channels = CHANNELS.lock();
    for slot in channels.iter_mut() {
        *slot = None;
    }
    NEXT_CHANNEL_ID.store(1, Ordering::SeqCst);
    crate::kinfo!("IPC subsystem initialized with {} channels", MAX_CHANNELS);
}

pub fn create_channel() -> Result<u32, IpcError> {
    let mut channels = CHANNELS.lock();
    if let Some(slot) = channels.iter_mut().find(|slot| slot.is_none()) {
        let id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::SeqCst);
        *slot = Some(Channel::new(id));
        Ok(id)
    } else {
        Err(IpcError::TableFull)
    }
}

pub fn send(channel_id: u32, data: &[u8]) -> Result<(), IpcError> {
    if data.is_empty() {
        return Err(IpcError::InvalidInput);
    }
    let mut channels = CHANNELS.lock();
    if let Some(channel) = channels.iter_mut().find_map(|slot| {
        slot.as_mut().and_then(|channel| {
            if channel.id == channel_id {
                Some(channel)
            } else {
                None
            }
        })
    }) {
        channel.push(data)
    } else {
        Err(IpcError::NoSuchChannel)
    }
}

pub fn receive(channel_id: u32, dest: &mut [u8]) -> Result<usize, IpcError> {
    if dest.is_empty() {
        return Err(IpcError::InvalidInput);
    }
    let mut channels = CHANNELS.lock();
    if let Some(channel) = channels.iter_mut().find_map(|slot| {
        slot.as_mut().and_then(|channel| {
            if channel.id == channel_id {
                Some(channel)
            } else {
                None
            }
        })
    }) {
        channel.pop(dest)
    } else {
        Err(IpcError::NoSuchChannel)
    }
}

pub fn clear(channel_id: u32) {
    let mut channels = CHANNELS.lock();
    if let Some(channel) = channels.iter_mut().find_map(|slot| {
        slot.as_mut().and_then(|channel| {
            if channel.id == channel_id {
                Some(channel)
            } else {
                None
            }
        })
    }) {
        *channel = Channel::new(channel_id);
    }
}
