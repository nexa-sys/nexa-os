# NexaOS

NexaOS is an experimental operating system written in Rust. It aims to deliver a self-contained, POSIX-leaning, Unix-like environment while offering partial compatibility with existing Linux software. The project explores a hybrid-kernel architecture and currently targets Multiboot2 + GRUB as its boot pathway.

## Vision

- **Rust-first foundations**: implement the kernel and core services in Rust to leverage memory safety and modern tooling.
- **Hybrid kernel design**: combine microkernel-style isolation with pragmatic monolithic elements where performance or simplicity demands it.
- **Self-contained runtime**: ship with enough userland components to boot, explore the system, and develop directly on-device.
- **Linux-friendly interfaces**: provide partial compatibility layers so selected Linux applications and tooling can run with minimal adaptation.

## Current Status

NexaOS now boots into 64-bit long mode through a Multiboot2-compliant GRUB flow, prints diagnostic banners over VGA text mode and the serial console, and parses the Multiboot memory map for future subsystem bring-up. Memory management, scheduling, and device abstractions are still skeletal and will evolve rapidly. Expect frequent breaking changes while the foundational pieces take shape.

## Architectural Highlights

| Area            | Notes |
|-----------------|-------|
| Boot flow       | Multiboot2-compliant loader via GRUB, handing control to the Rust kernel entry point. |
| Kernel core     | Hybrid model experimenting with both message-passing services and in-kernel execution for latency-sensitive tasks. |
| Platform target | Initially x86_64 with QEMU as the reference virtual machine; broader hardware enablement will follow. |
| Compatibility   | POSIX-inspired APIs with a gradual build-out of Linux syscall shims where practical. |

## Roadmap (early sketch)

1. Establish the Rust bootstrapping path and low-level runtime (linker scripts, paging, interrupt setup).
2. Bring up a minimal kernel shell for diagnostics and basic process management.
3. Implement rudimentary file system access and IPC primitives.
4. Introduce a developer-facing build pipeline and automated tests.
5. Iterate on Linux compatibility layers and userland tooling.

## Getting Started

To get started with NexaOS development, you'll need to set up your environment and familiarize yourself with the project's structure.

### Prerequisites

- Rust nightly toolchain with the `rust-src` and `llvm-tools-preview` components (`rustup toolchain install nightly` and `rustup component add rust-src llvm-tools-preview --toolchain nightly`).
- A working C toolchain (e.g. `build-essential` on Debian/Ubuntu) so the bundled GAS bootstrap can be assembled via `cc`.
- `ld.lld` (preferred) or GNU `ld` to satisfy the custom kernel linker invocation. The build scripts automatically fall back to `ld` when `ld.lld` is absent.
- `grub-mkrescue` and `xorriso` for packaging a bootable ISO.
- `qemu-system-x86_64` (or actual hardware, if you're daring) to launch the resulting image.

### Build & Run (work in progress)

```bash
# Clone the repo (if you haven't already)
git clone https://github.com/nexa-sys/nexa-os.git
cd nexa-os

# Ensure the right toolchain in this repo
rustup override set nightly
rustup component add rust-src llvm-tools-preview --toolchain nightly

# Build the kernel ELF (requires a C toolchain + lld available in PATH)
cargo build --release

# Produce a bootable ISO using GRUB
./scripts/build-iso.sh

# Boot the ISO in QEMU (serial output is forwarded to your terminal)
./scripts/run-qemu.sh
```

> ℹ️ **Troubleshooting:** 如果构建输出提示缺少 `cc`、`ld.lld` 或 `ld`，请安装相应编译工具链；同时确保 `grub-mkrescue`、`xorriso`、`qemu-system-x86_64` 可用。

更多中文说明、环境配置与调试/验证技巧可参考：

- [`docs/zh/getting-started.md`](docs/zh/getting-started.md)：环境准备与构建指南。
- [`docs/zh/tests.md`](docs/zh/tests.md)：当前测试流程与自动化计划。

## Contributing

Contributions, experiments, and feedback are very welcome. Until the contribution guidelines are published, feel free to open an issue to discuss ideas, report bugs, or coordinate larger contributions.

## License

This project is released under the terms described in `LICENSE` in the repository root.
