# UEFI Compatibility Boot Flow

This note captures the end-to-end handoff strategy between UEFI firmware, the NexaOS kernel, and user space components. It accompanies the updated build pipeline where the initramfs only provides an early userspace trampoline and the full root filesystem lives on the standalone ext2 disk image (`build/rootfs.ext2`).

## Stage 1: Bootloader / Early Kernel (UEFI Environment)
- Boot services are operational; `SystemTable` is valid.
- Capture graphics output protocol (GOP) state and persist it in `EARLY_FRAMEBUFFER_INFO` for later mapping.
- Query block I/O media geometry (`block_size`, `last_block`, `media_id`) and store immutable metadata.
- If available, locate the Simple Network Protocol (SNP) and extract MAC address; complement this with a lightweight PCI enumeration via `EFI_PCI_IO_PROTOCOL` to record BARs and interrupt lines.
- Optional: capture USB controller characteristics (xHCI base, endpoints) to enable richer post-boot HID support. Legacy PS/2 emulation remains the fallback for key input.

```rust
let gop = boot_services.locate_protocol::<GraphicsOutput>()?;
let mode = gop.current_mode_info();
let fb_phys = mode.framebuffer as u64;
let snp = boot_services.locate_protocol::<SimpleNetwork>()?;
```

## Stage 2: ExitBootServices()
- After collecting immutable firmware state, invoke `ExitBootServices()`.
- Firmware-owned protocols disappear; previously captured physical resources remain valid.
- NexaOS assumes full control over memory management and interrupt routing.

## Stage 3: Kernel Control
- Initialize paging, APIC/IOAPIC, timers, and the physical memory allocator.
- Promote persisted GOP, block, and network descriptors into kernel subsystems.
- Launch the primary user process after establishing Ring 3 execution context.

## Stage 4: User Space UEFI Compatibility Service
- `init` spawns `/sbin/uefi-compatd`, a helper responsible for brokering access to the preserved UEFI-era resources.
- Framebuffer: expose `/dev/fb0` and allow `mmap()` once the request is validated against `EARLY_FRAMEBUFFER_INFO`.
- Network: provide a syscall-backed accessor that maps the recorded MMIO region into user space or mediates access via ring buffers.
- Input: either surface PS/2-compatible events or, once USB state is tracked, hand over xHCI descriptors for direct management.

```c
int fd = open("/dev/fb0", O_RDWR);
void *fb = mmap(NULL, height * stride, PROT_WRITE, MAP_SHARED, fd, 0);
```

## Build and Run Implications
- `scripts/build-userspace.sh` no longer embeds `rootfs.ext2` inside the initramfs payload.
- `scripts/run-qemu.sh` now refuses to start without `build/rootfs.ext2` and always attaches it as a virtio block device (`/dev/vda`).
- The minimal initramfs ships only the emergency shell and bootstrap helpers, aligning the runtime with the staged boot flow outlined above.
