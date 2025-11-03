# NexaOS 测试与验证指南

> 更新时间：2025-11-03

本文档总结 NexaOS 生产级系统的测试流程与验证方法，帮助开发者确保代码质量符合生产标准。测试涵盖功能验证、POSIX 合规性检查、安全测试和性能基准测试。

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

## 5. POSIX 合规性测试

### 错误码验证
```bash
# 验证 errno 值符合 POSIX 标准
grep -r "EPERM\|ENOENT\|EAGAIN" src/posix.rs
```

### 系统调用接口测试
- 验证系统调用号与 Linux ABI 一致
- 验证参数传递符合 x86_64 calling convention
- 验证返回值符合 POSIX 规范

### 文件系统语义测试
- 验证路径解析符合 Unix 约定
- 验证文件权限检查正确实施
- 验证 stat 结构体布局与 Linux 兼容

## 6. 安全测试

### 权限隔离验证
```bash
# 验证用户态代码无法直接访问内核内存
# 验证 Ring 3 到 Ring 0 转换仅通过系统调用
```

### 多用户安全测试
- 验证非 root 用户权限限制
- 验证密码哈希不可逆
- 验证用户会话隔离

## 7. 混合内核架构验证

### 性能特征测试
- 验证内核态组件执行效率
- 验证用户态服务隔离性
- 验证 IPC 机制开销

### 模块化测试
- 验证系统服务可独立重启
- 验证内核核心不依赖用户态服务
- 验证驱动程序加载/卸载

## 8. 持续集成（规划中）

### 自动化测试套件
- **单元测试**：Rust `#[cfg(test)]` 模块测试内核逻辑
- **集成测试**：QEMU 自动化测试完整启动流程
- **回归测试**：Git bisect 自动定位引入问题的提交
- **性能基准**：跟踪系统调用延迟、内存使用等指标

### CI/CD 流水线
```yaml
# GitHub Actions 示例
- name: Build kernel
  run: cargo build --release
  
- name: Run QEMU tests
  run: ./scripts/test-qemu.sh
  
- name: POSIX compliance check
  run: ./scripts/check-posix.sh
```

## 9. 生产就绪检查清单

在发布生产版本前，必须通过以下检查：

- [ ] 所有 Multiboot2 验证通过
- [ ] QEMU 启动无错误、无警告
- [ ] 所有系统调用符合 POSIX 规范
- [ ] 用户态/内核态隔离完整
- [ ] 多用户权限正确实施
- [ ] 内存管理无泄漏
- [ ] 异常处理覆盖所有情况
- [ ] 文档与代码同步
- [ ] 安全审计通过
- [ ] 性能指标达标

欢迎贡献生产级测试方案和自动化工具——可在 Issue 中讨论或直接提交 PR！
