# NexaOS 测试与验证指南

> 更新时间：2025-10-23

本文档总结当前可行的测试流程与建议，帮助开发者在本地验证 NexaOS 的关键功能。由于项目仍处于早期阶段，这些检查以手动/脚本方式为主；后续将逐步引入自动化测试。

## 1. 构建工序快速回归

在提交代码或调试前，建议先完成以下命令，确认核心工具链可用：

```bash
cargo build --release
./scripts/build-iso.sh
```

> 如果缺少 `grub-mkrescue`、`xorriso` 或 `mtools` 等依赖，脚本会在开头给出明确提示。

生成的 `dist/nexaos.iso` 是后续所有测试的输入。

## 2. Multiboot 头验证

```bash
grub-file --is-x86-multiboot2 target/x86_64-nexaos/release/nexa-os
```

若命令返回退出码 `0`（无输出），表示内核 ELF 文件的 Multiboot2 头位于镜像开头且格式正确；非零退出码则代表 GRUB 仍无法识别，需要回溯最近的链接脚本或启动文件改动。

## 3. QEMU 启动冒烟测试

```bash
./scripts/run-qemu.sh
```

预期终端输出：

```
[NexaOS] Kernel entry.
Welcome to NexaOS kernel bootstrap!
[mem] Detected ... memory regions:
  - ...
[mem] No boot modules supplied.
[NexaOS] Initialization complete.
System halted. Enjoy exploring the code!
```

若看到持续滚动的 `hlt` 或 QEMU 自动退出，可根据串口日志排查引导阶段问题。

## 4. 内存映射检查

在 QEMU 启动过程中，串口日志会打印出 GRUB 提供的内存分布。检查重点：

- 至少存在一段 `Usable` 类型的内存区域。
- 地址区间不重叠、排序合理。
- 若添加 boot module（例如在 `grub.cfg` 中加入 `module2`），确认日志显示正确的起止地址和名称。

## 5. 未来自动化方向（规划）

- **串口快照比对**：将预期输出写入基准文件，借助 `expect`/`pexpect` 或 `pytest` 脚本自动比较串口日志。
- **单元测试**：为纯 Rust 逻辑（如内存映射解析）引入 `#[cfg(test)]` 单元测试，并通过 host 构建执行。
- **CI 集成**：在 GitHub Actions 或自建 CI 中安装 `qemu-system-x86_64`、`grub-mkrescue` 等工具，实现 PR 冒烟验证。

欢迎贡献更完善的测试脚本或自动化方案——可在 Issue 中讨论或直接提交 PR！
