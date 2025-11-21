use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::{
    bootinfo,
    interrupts,
    logger,
    uefi_compat::NetworkDescriptor,
};

mod drivers;
pub mod arp;
pub mod ethernet;
pub mod ipv4;
pub mod stack;
pub mod udp;
pub mod udp_helper;
pub mod netlink;

pub use drivers::NetError;
pub use udp_helper::{UdpMessage, UdpConnectionContext, UdpStats};

const MAX_NET_DEVICES: usize = 4;

struct DeviceSlot {
    descriptor: Option<NetworkDescriptor>,
    driver: Option<drivers::DriverInstance>,
    irq_line: Option<u8>,
}

impl DeviceSlot {
    const fn new() -> Self {
        Self {
            descriptor: None,
            driver: None,
            irq_line: None,
        }
    }
}

struct NetState {
    slots: [DeviceSlot; MAX_NET_DEVICES],
    stack: stack::NetStack,
    activated: bool,
    last_poll_ms: u64,
}

impl NetState {
    const fn new() -> Self {
        Self {
            slots: [
                DeviceSlot::new(),
                DeviceSlot::new(),
                DeviceSlot::new(),
                DeviceSlot::new(),
            ],
            stack: stack::NetStack::new(),
            activated: false,
            last_poll_ms: 0,
        }
    }
}

static NET_STATE: Mutex<NetState> = Mutex::new(NetState::new());

static NET_INITIALIZED: AtomicBool = AtomicBool::new(false);

struct IrqCookie {
    device_index: usize,
}

static IRQ_COOKIES: Mutex<[IrqCookie; MAX_NET_DEVICES]> = Mutex::new([
    IrqCookie { device_index: 0 },
    IrqCookie { device_index: 1 },
    IrqCookie { device_index: 2 },
    IrqCookie { device_index: 3 },
]);

const INVALID_IRQ_LINE: u8 = 0xFF;

/// Access the network stack safely
pub fn with_net_stack<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut stack::NetStack) -> R,
{
    // We can access the stack even if not fully initialized (e.g. for socket creation)
    // but usually we want it initialized.
    // However, for syscalls, we might need it.
    let mut state = NET_STATE.lock();
    Some(f(&mut state.stack))
}

/// Send a batch of frames on a specific device
pub fn send_frames(device_index: usize, batch: &stack::TxBatch) -> Result<(), drivers::NetError> {
    if device_index >= MAX_NET_DEVICES {
        return Err(drivers::NetError::InvalidDevice);
    }
    
    let mut state = NET_STATE.lock();
    let slot = &mut state.slots[device_index];
    
    if let Some(driver) = &mut slot.driver {
        for frame in batch.frames() {
            driver.transmit(frame)?;
        }
        Ok(())
    } else {
        Err(drivers::NetError::InvalidDevice)
    }
}

/// Called from the UEFI compatibility layer to mirror descriptors into the
/// runtime registry.
pub fn ingest_boot_descriptor(index: usize, descriptor: NetworkDescriptor) {
    if index >= MAX_NET_DEVICES {
        crate::kwarn!(
            "net: dropping descriptor {} (max supported devices: {})",
            index,
            MAX_NET_DEVICES
        );
        return;
    }

    let mut state = NET_STATE.lock();
    state.slots[index].descriptor = Some(descriptor);
    state.slots[index].irq_line = Some(descriptor.interrupt_line);
    crate::kinfo!(
        "net: staged descriptor {} (if_type={}, mmio={:#x}+{:#x}, irq={})",
        index,
        descriptor.info.if_type,
        descriptor.mmio_base,
        descriptor.mmio_length,
        descriptor.interrupt_line
    );
}

/// Finalizes NIC drivers and the in-kernel network stack. Safe to call more
/// than once; subsequent calls become no-ops.
pub fn init() {
    if NET_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    let mut state = NET_STATE.lock();
    for idx in 0..MAX_NET_DEVICES {
        let Some(descriptor) = state.slots[idx].descriptor else {
            continue;
        };

        if descriptor.mmio_base == 0 {
            crate::kwarn!("net: descriptor {} missing MMIO base", idx);
            continue;
        }

        match drivers::DriverInstance::new(idx, descriptor) {
            Ok(mut driver) => {
                if let Err(err) = driver.init() {
                    crate::kerror!(
                        "net: driver init failed for idx {} ({:?})",
                        idx,
                        err
                    );
                    continue;
                }

                let mac = driver.mac_address();
                state.stack.register_device(idx, mac);
                register_device_irq(idx, descriptor.interrupt_line);
                crate::kinfo!(
                    "net: device {} online mac {:02x?}",
                    idx,
                    mac
                );
                state.slots[idx].driver = Some(driver);
            }
            Err(err) => {
                crate::kwarn!("net: unsupported NIC {} ({:?})", idx, err);
            }
        }
    }
    state.activated = true;
}

fn register_device_irq(idx: usize, line: u8) {
    if line == 0 || line == INVALID_IRQ_LINE {
        crate::kwarn!("net: device {} missing legacy IRQ assignment", idx);
        return;
    }

    if line >= 16 {
        crate::kwarn!("net: IRQ {} for device {} exceeds PIC range", line, idx);
        return;
    }

    // IRQ registration temporarily disabled until interrupts module supports dynamic registration
    crate::kwarn!("net: IRQ registration not supported yet, device {} will rely on polling", idx);
    
    /*
    let mut cookies = IRQ_COOKIES.lock();
    cookies[idx].device_index = idx;
    let cookie_ptr = &cookies[idx] as *const IrqCookie as *mut ();
    drop(cookies);

    let handler: LegacyIrqHandler = net_irq_trampoline;
    match interrupts::register_legacy_irq(line, handler, cookie_ptr, "net") {
        Ok(()) => {
            crate::kinfo!("net: registered IRQ{} for device {}", line, idx);
        }
        Err(LegacyIrqError::AlreadyRegistered) => {
            crate::kwarn!(
                "net: IRQ{} already registered, device {} will rely on polling",
                line,
                idx
            );
        }
        Err(err) => {
            crate::kwarn!(
                "net: failed to register IRQ{} for device {} ({:?})",
                line,
                idx,
                err
            );
        }
    }
    */
}

fn net_irq_trampoline(_line: u8, ctx: *mut ()) {
    if ctx.is_null() {
        return;
    }
    let cookie = unsafe { &*(ctx as *const IrqCookie) };
    handle_irq(cookie.device_index);
}

/// Invoked from the shared IRQ dispatcher when the NIC asserts INTx.
pub fn handle_irq(device_index: usize) {
    let mut guard = NET_STATE.lock();
    let state = &mut *guard;
    let stack = &mut state.stack;
    
    if let Some(slot) = state.slots.get_mut(device_index) {
        if let Some(driver) = slot.driver.as_mut() {
            drain_rx(driver, stack, device_index);
        }
    }
}

/// Periodic polling hook (timer interrupt) used for link maintenance and
/// protocol timers.
pub fn poll() {
    if !NET_INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    let now_ms = logger::boot_time_us() / 1_000;
    let mut guard = NET_STATE.lock();
    let state = &mut *guard;
    
    if state.last_poll_ms == now_ms {
        return;
    }

    let stack = &mut state.stack;
    let slots = &mut state.slots;

    for idx in 0..MAX_NET_DEVICES {
        if let Some(driver) = slots[idx].driver.as_mut() {
            drain_rx(driver, stack, idx);
            if let Err(err) = driver.maintenance() {
                crate::kwarn!("net: maintenance error on device {} ({:?})", idx, err);
            }
            if let Err(err) = produce_pending_frames(driver, stack, idx, now_ms) {
                crate::kwarn!("net: stack poll error on device {} ({:?})", idx, err);
            }
        }
    }

    state.last_poll_ms = now_ms;
}

fn drain_rx(driver: &mut drivers::DriverInstance, stack: &mut stack::NetStack, device_index: usize) {
    let mut scratch = [0u8; stack::MAX_FRAME_SIZE];
    while let Some(len) = driver.drain_rx(&mut scratch) {
        let mut responses = stack::TxBatch::new();
        if let Err(err) = stack.handle_frame(device_index, &scratch[..len], &mut responses)
        {
            crate::kwarn!(
                "net: frame processing failed on device {} ({:?})",
                device_index,
                err
            );
        }

        transmit_batch(driver, &responses, device_index);
    }
}

fn produce_pending_frames(
    driver: &mut drivers::DriverInstance,
    stack: &mut stack::NetStack,
    device_index: usize,
    now_ms: u64,
) -> Result<(), NetError> {
    let mut responses = stack::TxBatch::new();
    stack
        .poll_device(device_index, now_ms, &mut responses)?;
    transmit_batch(driver, &responses, device_index);
    Ok(())
}

fn transmit_batch(
    driver: &mut drivers::DriverInstance,
    batch: &stack::TxBatch,
    device_index: usize,
) {
    for frame in batch.frames() {
        if let Err(err) = driver.transmit(frame) {
            crate::kwarn!(
                "net: failed to transmit frame on device {} ({:?})",
                device_index,
                err
            );
        }
    }
}
