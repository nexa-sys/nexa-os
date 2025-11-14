# Enabling Kernel TCP via the UEFI Compatibility Flow

This document describes how to leverage the existing four-stage UEFI compatibility pipeline to bring up a NIC and implement a minimal in-kernel TCP path. It ties concrete NexaOS components to each stage, lists the data that must survive `ExitBootServices`, and outlines the incremental work required for DMA-safe device access, interrupt delivery, and a lightweight TCP stack.

## Stage Alignment Overview

| Stage | Firmware / OS Actor | NexaOS Components | Responsibilities |
| --- | --- | --- | --- |
| 1. Bootloader / Early Kernel | UEFI loader while Boot Services are active | `boot/uefi-loader/src/main.rs` (`collect_device_table`, `collect_network_devices`, `pci_snapshot_*`) | Capture GOP, Block I/O, SNP state. Snapshot PCI BARs/IRQs via `EFI_PCI_IO_PROTOCOL`. Persist data inside the `nexa_boot_info::BootInfo` block (`NetworkDeviceInfo`, `PciDeviceInfo`). |
| 2. `ExitBootServices()` | UEFI firmware + loader | `boot/uefi-loader` right before jumping to `kernel_main_uefi` | Invoke `ExitBootServices` only after all descriptors are recorded. Firmware tear-down removes protocols but leaves MMIO, BAR, and DMA state intact. |
| 3. Kernel take-over | NexaOS kernel | `src/lib.rs` (UEFI entry), `src/uefi_compat.rs`, `src/paging.rs`, `src/syscall.rs`, `src/fs.rs` | Promote boot-info records into runtime structures. Map GOP + NIC MMIO, install `/dev/net*` file nodes, expose descriptors through syscalls 240-243. |
| 4. User-space broker | `/sbin/uefi-compatd` + future networking daemons | `userspace/uefi_compatd.rs`, `userspace/nrlib` syscall shims | Query compat syscalls, publish device metadata, optionally request MMIO mappings or proxy I/O on behalf of higher-level services (e.g., DHCP, link bring-up). |

## Stage 1: Data Capture in the UEFI Loader

1. **GOP framebuffer** – `collect_framebuffer_info` persists `FramebufferInfo` (phys base, pitch, resolution, pixel format). The loader already records this through `BootInfo::framebuffer`.
2. **Block devices** – `collect_block_devices` iterates `BlockIO` handles, builds `BlockDeviceInfo`, and snapshots PCI BARs for storage controllers.
3. **Network devices (SNP)** – `collect_network_devices` obtains `SimpleNetwork` handles, copies the current MAC address, MTU, filter settings, and link status into `NetworkDeviceInfo`.
4. **PCI augmentation** – `pci_snapshot_for_handle` plus `apply_pci_snapshot` record BAR base/length, interrupt line/pin, and vendor/device IDs for every enumerated handle; this is crucial because SNP itself never exposes BAR0.

> **Contract:** Everything required to re-drive the hardware (BARs, MAC, media flags) must be serialized into `BootInfo`. Nothing else is available once Boot Services vanish.

## Stage 2: Controlled `ExitBootServices`

- The loader validates the memory map, calls `ExitBootServices`, and immediately jumps into `kernel_main_uefi` with a pointer to `BootInfo`.
- Hardware (framebuffer linear aperture, NIC MMIO windows, DMA rings) continue to exist untouched. This is the foundation for “plan B”: the kernel can resume ownership without re-enumerating SNP.

## Stage 3: Kernel Integration (`src/uefi_compat.rs`)

1. **Reset + init** – `uefi_compat::reset()` clears prior state, then `init()` pulls framebuffer and device tables from `bootinfo`.
2. **MMIO mapping** – `map_mmio_region` invokes `paging::map_device_region` for each framebuffer/NIC/block BAR. That guarantees identity mappings (or kernel-virtual mappings) before any userspace mapping.
3. **Descriptor tables** – `NETWORK_DEVICES` and `BLOCK_DEVICES` store `NetworkDescriptor` / `BlockDescriptor` values derived from boot info + PCI metadata. Each includes BAR base/length, BAR flags, and IRQ lines.
4. **Device nodes** – `install_device_nodes()` adds `/dev/net{0-7}`, `/dev/block{0-7}`, and `/dev/fb0` into the runtime FS with permissive metadata (currently character/block devices backed by future drivers).
5. **Syscalls** – `SYS_UEFI_GET_{COUNTS,FB_INFO,NET_INFO,BLOCK_INFO}` copy descriptors into userspace buffers, enabling brokers to discover hardware without trusting firmware services.

## Stage 4: User-Space Broker (`/sbin/uefi-compatd`)

- The daemon calls the compat syscalls, prints or logs inventory, and can hand off descriptors to specialized agents (framebuffer compositor, NIC driver, block bridge, etc.).
- For networking, the daemon can either:
  1. Request the kernel to map the NIC BAR into its address space (future `SYS_NET_MAP_MMIO`), then issue programmed I/O directly.
  2. Or act purely as a control-plane service while the kernel owns the data plane (preferred for TCP).

## Bringing Up a Basic Kernel TCP Stack

The persisted descriptors already contain enough information to re-initialize a NIC after `ExitBootServices`. The remaining work falls into five layers:

### 1. Kernel Device Abstraction

- Create `src/net/mod.rs` with:
  - `struct NetDevice { desc: NetworkDescriptor, regs: NonNull<u8>, irq: u8, mac: [u8; 6], .. }`
  - Traits for TX/RX operations (`start()`, `stop()`, `submit_tx(buf)`, `poll_rx(...)`).
- On boot, have `uefi_compat::init()` register each descriptor with the new net subsystem (instead of only the FS layer). This keeps a single source of truth.

### 2. MMIO + DMA Bring-Up

- Select one NIC to support first (e.g., **Intel E1000** exposed by QEMU/VMware). Add a driver under `src/net/drivers/e1000.rs` reusing the BAR base/length captured earlier.
- Use `paging::map_device_region` output (currently identity) to build a `DeviceMapping { phys: u64, virt: *mut u8, len: usize }` that driver code can safely dereference.
- Allocate DMA regions via the existing physical memory manager (`src/memory.rs`). Ensure buffers obey alignment and physically-contiguous requirements for the NIC (e.g., 16-byte alignment for descriptors).

### 3. Interrupt + Poll Integration

- Wire the stored `interrupt_line` / `interrupt_pin` into IOAPIC programming (`src/interrupts.rs`). Deliver them to a new `net::interrupt_handler(vector)` that schedules RX/TX work.
- Provide a fallback polling loop (e.g., invoked from the scheduler tick) for early bring-up before MSI/MSI-X is configured.

### 4. L2/L3 Plumbing

- Build a minimal Ethernet/IP stack in `src/net/stack/`:
  - `ethernet.rs` – frame parsing, MAC filtering, ARP module with small cache.
  - `ipv4.rs` – header validation, checksum, fragmentation rejection (initially drop fragments).
  - `icmp.rs` – respond to echo requests for diagnostics.
- Provide a simple socket-like API (e.g., `net::tcp::Socket`) but keep it kernel-internal for now; userspace can access via a future syscall once stable.

### 5. Minimal TCP Implementation

- Start with passive open + active open for a single connection at a time:
  1. **Handshake** – implement SYN/SYN-ACK/ACK exchange with sequence tracking and retransmit timers (driven by HPET/TSC via `time.rs`).
  2. **Send path** – copy payloads from kernel buffers (e.g., `/dev/tcp-test`) into a TX queue, segment by MSS, maintain unacked queue.
  3. **Receive path** – assemble in-order bytes, ACK cumulatively, drop out-of-window data.
  4. **Timeouts** – reuse scheduler timers or add a wheel to retransmit after RTO.

- Expose a prototype character device `/dev/tcp0` that lets a test app read/write a single stream (think `cat /dev/tcp0` piping to `/dev/tty`). Once stable, promote to a BSD-like socket API.

## Kernel/User Interfaces Needed

1. **Net descriptor registry** – Already available (`NetworkDescriptor`). Add helper `uefi_compat::each_network_device(|desc| ...)`.
2. **MMIO borrowing** – Provide `paging::map_device_region` wrappers returning `VirtAddr` for drivers; optionally add a syscall for userspace mapping with permissions checks.
3. **Interrupt registration** – Extend `interrupts::register_device_irq(vector, handler)` so drivers can hook closures.
4. **Socket syscalls (future)** – `SYS_SOCKET`, `SYS_CONNECT`, `SYS_ACCEPT`, `SYS_SEND`, `SYS_RECV` once the stack matures.

## Validation Strategy

1. **Unit tests (hosted)** – For checksum, ARP cache eviction, TCP sequence math (use `#[cfg(test)]` inside `net::stack`).
2. **QEMU smoke** – Boot via `scripts/run-qemu.sh` with `-device e1000,netdev=n0 -netdev user,id=n0,hostfwd=tcp::5555-:8080` and verify:
   - `uefi-compatd` reports the NIC descriptor (MAC, BAR0, IRQ).
   - Kernel driver configures RX/TX rings and logs link-up via `kinfo!`.
3. **Ping test** – Implement ICMP echo responses -> `ping 10.0.2.15` from the host should succeed.
4. **TCP echo** – Bring up `/dev/tcp0` echo service; connect from host (`nc 127.0.0.1 5555`) and check payload integrity.

## Implementation Checklist

1. [ ] Add `src/net/` module scaffold + driver trait.
2. [ ] Teach `uefi_compat::init()` to register descriptors with the net module besides creating `/dev/net*` nodes.
3. [ ] Implement Intel E1000 (or VirtIO-Net) driver using BAR metadata and DMA helpers.
4. [ ] Integrate IOAPIC routing for stored IRQ line/pin; add MSI later.
5. [ ] Build Ethernet/IP/ARP/ICMP layers with bounded buffers and logging.
6. [ ] Layer a constrained TCP implementation (single-connection, no congestion control initially) focused on correctness.
7. [ ] Expose a temporary testing character device + CLI tool in `userspace/shell.rs` (`netcat`-style) to drive the stack.
8. [ ] Expand syscall surface for sockets only after the kernel stack is stable.

Following this progression keeps the system debuggable at every milestone: you can stop after Stage 4 to validate descriptor integrity, after the driver to validate ARP/ping, and finally after TCP to exercise end-to-end streaming over the preserved NIC state.
