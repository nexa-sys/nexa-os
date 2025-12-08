//! NexaOS Network Stack
//!
//! This module provides the network stack implementation with conditional compilation
//! support for various protocols. Use feature flags to enable/disable protocols:
//!
//! - `net_ethernet` - Ethernet frame support (base feature)
//! - `net_arp` - ARP protocol support
//! - `net_ipv4` - IPv4 protocol support
//! - `net_udp` - UDP protocol support (requires ipv4)
//! - `net_tcp` - TCP protocol support (requires ipv4)
//! - `net_netlink` - Netlink socket support

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::{bootinfo, interrupts, logger, uefi_compat::NetworkDescriptor};

// Conditionally compile network protocol modules
#[cfg(feature = "net_arp")]
pub mod arp;
#[cfg(not(feature = "net_arp"))]
pub mod arp {
    //! ARP stub module (feature disabled)
}

mod drivers;

#[cfg(feature = "net_ethernet")]
pub mod ethernet;
#[cfg(not(feature = "net_ethernet"))]
pub mod ethernet {
    //! Ethernet stub module (feature disabled)
}

#[cfg(feature = "net_ipv4")]
pub mod ipv4;
#[cfg(not(feature = "net_ipv4"))]
pub mod ipv4 {
    //! IPv4 stub module (feature disabled)
}

pub mod modular;

#[cfg(feature = "net_netlink")]
pub mod netlink;
#[cfg(not(feature = "net_netlink"))]
pub mod netlink {
    //! Netlink stub module (feature disabled)
}

pub mod stack;

#[cfg(feature = "net_tcp")]
pub mod tcp;
#[cfg(not(feature = "net_tcp"))]
pub mod tcp {
    //! TCP stub module (feature disabled)
}

#[cfg(feature = "net_udp")]
pub mod udp;
#[cfg(not(feature = "net_udp"))]
pub mod udp {
    //! UDP stub module (feature disabled)
}

#[cfg(feature = "net_udp")]
pub mod udp_helper;
#[cfg(not(feature = "net_udp"))]
pub mod udp_helper {
    //! UDP helper stub module (feature disabled)
    /// Stub for UdpConnectionContext when UDP is disabled
    pub struct UdpConnectionContext;
    /// Stub for UdpStats when UDP is disabled
    pub struct UdpStats;
}

pub use drivers::NetError;
#[cfg(feature = "net_udp")]
pub use udp_helper::{UdpConnectionContext, UdpStats};
#[cfg(not(feature = "net_udp"))]
pub use udp_helper::{UdpConnectionContext, UdpStats};

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
    crate::kdebug!(
        "[send_frames] device_index={}, MAX_NET_DEVICES={}",
        device_index,
        MAX_NET_DEVICES
    );

    if device_index >= MAX_NET_DEVICES {
        crate::kerror!("[send_frames] ERROR: device_index >= MAX_NET_DEVICES");
        return Err(drivers::NetError::InvalidDevice);
    }

    let mut state = NET_STATE.lock();
    let slot = &mut state.slots[device_index];

    crate::kdebug!("[send_frames] driver present: {}", slot.driver.is_some());

    if let Some(driver) = &mut slot.driver {
        crate::kdebug!("[send_frames] Transmitting frames");
        for frame in batch.frames() {
            driver.transmit(frame)?;
        }
        crate::kdebug!("[send_frames] Successfully transmitted all frames");
        Ok(())
    } else {
        crate::kerror!("[send_frames] ERROR: No driver for device {}", device_index);
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
    crate::kinfo!("[net::init] Starting network initialization");

    if NET_INITIALIZED.swap(true, Ordering::SeqCst) {
        crate::kdebug!("[net::init] Already initialized, returning");
        return;
    }

    crate::kdebug!("[net::init] Acquiring NET_STATE lock");
    let mut state = NET_STATE.lock();

    crate::kdebug!("[net::init] Scanning {} device slots", MAX_NET_DEVICES);

    for idx in 0..MAX_NET_DEVICES {
        crate::kdebug!("[net::init] Checking slot {}", idx);

        let Some(descriptor) = state.slots[idx].descriptor else {
            crate::kdebug!("[net::init] Slot {} has no descriptor", idx);
            continue;
        };

        crate::kdebug!(
            "[net::init] Slot {} has descriptor: mmio={:#x}, irq={}",
            idx,
            descriptor.mmio_base,
            descriptor.interrupt_line
        );

        if descriptor.mmio_base == 0 {
            crate::kwarn!("net: descriptor {} missing MMIO base", idx);
            continue;
        }

        crate::kdebug!("[net::init] Creating driver for slot {}", idx);

        match drivers::DriverInstance::new(idx, descriptor) {
            Ok(mut driver) => {
                // First init to set up hardware state
                if let Err(err) = driver.init() {
                    crate::kerror!("net: driver init failed for idx {} ({:?})", idx, err);
                    continue;
                }

                // Move driver to final location in NetState
                state.slots[idx].driver = Some(driver);

                // CRITICAL: Update DMA descriptor base addresses after move
                // The E1000 hardware needs pointers to descriptors in their final location
                if let Some(ref mut final_driver) = state.slots[idx].driver {
                    final_driver.update_dma_addresses();
                }

                // Now get MAC address from the driver in its final location
                let mac = if let Some(ref driver) = state.slots[idx].driver {
                    driver.mac_address()
                } else {
                    continue;
                };

                state.stack.register_device(idx, mac);
                register_device_irq(idx, descriptor.interrupt_line);
                crate::kinfo!("net: device {} online mac {:02x?}", idx, mac);
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
    crate::kwarn!(
        "net: IRQ registration not supported yet, device {} will rely on polling",
        idx
    );

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

    // Debug: Check poll activity
    static mut POLL_COUNT: u32 = 0;
    static mut LAST_DEBUG_MS: u64 = 0;
    unsafe {
        POLL_COUNT += 1;
        let cnt = POLL_COUNT;
        // Log every 1000ms instead of every 200 calls
        if now_ms >= LAST_DEBUG_MS + 1000 {
            crate::kinfo!("[net::poll] poll #{}, now_ms={}", cnt, now_ms);
            LAST_DEBUG_MS = now_ms;
        }
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

fn drain_rx(
    driver: &mut drivers::DriverInstance,
    stack: &mut stack::NetStack,
    device_index: usize,
) {
    use core::sync::atomic::{AtomicU64, Ordering};
    static DRAIN_RX_CALLS: AtomicU64 = AtomicU64::new(0);
    static LAST_DRAIN_DEBUG_MS: AtomicU64 = AtomicU64::new(0);
    
    let call_count = DRAIN_RX_CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    let now_ms = logger::boot_time_us() / 1_000;
    let last_debug = LAST_DRAIN_DEBUG_MS.load(Ordering::Relaxed);
    if now_ms >= last_debug + 2000 {
        crate::kinfo!("[drain_rx] device {} call #{}", device_index, call_count);
        LAST_DRAIN_DEBUG_MS.store(now_ms, Ordering::Relaxed);
    }
    
    let mut scratch = [0u8; stack::MAX_FRAME_SIZE];
    let mut frame_count = 0;
    while let Some(len) = driver.drain_rx(&mut scratch) {
        frame_count += 1;

        // Dump ethernet frame info
        if len >= 14 {
            let ethertype = u16::from_be_bytes([scratch[12], scratch[13]]);
            crate::ktrace!(
                "[drain_rx] Frame {}: len={}, ethertype=0x{:04x} ({})",
                frame_count,
                len,
                ethertype,
                match ethertype {
                    0x0800 => "IPv4",
                    0x0806 => "ARP",
                    0x86DD => "IPv6",
                    _ => "unknown",
                }
            );

            // If IPv4, show protocol
            if ethertype == 0x0800 && len >= 34 {
                let proto = scratch[23];
                crate::ktrace!(
                    "[drain_rx] IPv4 protocol={} ({})",
                    proto,
                    match proto {
                        1 => "ICMP",
                        6 => "TCP",
                        17 => "UDP",
                        _ => "other",
                    }
                );
            }
        } else {
            crate::ktrace!("[drain_rx] Frame {} too short: len={}", frame_count, len);
        }

        let mut responses = stack::TxBatch::new();
        if let Err(err) = stack.handle_frame(device_index, &scratch[..len], &mut responses) {
            crate::kwarn!(
                "net: frame processing failed on device {} ({:?})",
                device_index,
                err
            );
        }

        transmit_batch(driver, &responses, device_index);
    }
    if frame_count > 0 {
        crate::ktrace!(
            "[drain_rx] Processed {} frames on device {}",
            frame_count,
            device_index
        );
    }
}

fn produce_pending_frames(
    driver: &mut drivers::DriverInstance,
    stack: &mut stack::NetStack,
    device_index: usize,
    now_ms: u64,
) -> Result<(), NetError> {
    let mut responses = stack::TxBatch::new();
    stack.poll_device(device_index, now_ms, &mut responses)?;
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
