# NexaOS Userspace Programs

程序按功能类别组织，便于管理和维护。

## 目录结构

```
programs/
├── core/           # 核心系统服务 (启动关键，静态链接)
│   ├── init/       # ni - System V init 实现
│   └── getty/      # TTY 登录提示
│
├── user/           # Shell 和用户认证
│   ├── shell/      # sh - POSIX shell
│   ├── login/      # 用户登录程序
│   ├── login_cmd/  # 登录命令工具
│   ├── logout/     # 用户登出
│   └── adduser/    # 添加用户账户
│
├── network/        # 网络工具
│   ├── nslookup/   # DNS 查询
│   ├── ip/         # 网络接口配置
│   ├── dhcp/       # DHCP 客户端
│   └── nurl/       # URL 获取工具 (类似 curl)
│
├── daemons/        # 系统守护进程
│   ├── ntpd/       # NTP 时间同步
│   └── uefi_compatd/ # UEFI 兼容性守护进程
│
├── system/         # 系统工具
│   └── dmesg/      # 显示内核环形缓冲区
│
├── coreutils/      # POSIX 标准工具
│   ├── ls/         # 列出目录内容
│   ├── cat/        # 连接并显示文件
│   ├── cp/         # 复制文件
│   ├── mv/         # 移动/重命名文件
│   ├── rm/         # 删除文件
│   ├── mkdir/      # 创建目录
│   ├── rmdir/      # 删除空目录
│   ├── touch/      # 更新文件时间戳
│   ├── stat/       # 显示文件状态
│   ├── pwd/        # 打印工作目录
│   ├── echo/       # 显示文本
│   ├── uname/      # 打印系统信息
│   ├── whoami/     # 打印当前用户名
│   ├── users/      # 打印登录用户
│   ├── id/         # 打印用户/组 ID
│   ├── clear/      # 清屏
│   ├── kill/       # 发送信号给进程
│   ├── killall/    # 按名称杀死进程
│   ├── grep/       # 搜索文件中的模式
│   ├── find/       # 在目录层次中搜索文件
│   ├── head/       # 输出文件的前几行
│   ├── tail/       # 输出文件的后几行
│   ├── wc/         # 计数单词/行/字符
│   ├── tee/        # 读取 stdin 并写入 stdout 和文件
│   ├── hostname/   # 显示或设置主机名
│   ├── uptime/     # 显示系统运行时间
│   ├── date/       # 打印或设置日期时间
│   ├── sleep/      # 延迟指定时间
│   └── env/        # 显示或修改环境变量
│
├── power/          # 电源管理
│   ├── reboot/     # 重启系统
│   ├── shutdown/   # 关闭系统
│   ├── halt/       # 停止系统
│   └── poweroff/   # 关闭电源
│
├── memory/         # 内存管理
│   ├── swapon/     # 启用交换空间
│   ├── swapoff/    # 禁用交换空间
│   ├── mkswap/     # 创建交换区域
│   └── free/       # 显示内存使用情况
│
├── ipc/            # 进程间通信
│   ├── ipc-create/ # 创建 IPC 对象
│   ├── ipc-send/   # 发送 IPC 消息
│   └── ipc-recv/   # 接收 IPC 消息
│
├── editors/        # 文本编辑器
│   └── edit/       # 简单文本编辑器
│
├── kmod/           # 内核模块管理
│   ├── lsmod/      # 列出已加载模块
│   ├── insmod/     # 插入内核模块
│   ├── rmmod/      # 移除内核模块
│   └── modinfo/    # 显示模块信息
│
└── test/           # 测试程序 (生产构建可排除)
    ├── crashtest/      # 崩溃测试
    ├── thread_test/    # 线程 API 测试
    ├── pthread_test/   # POSIX 线程测试
    ├── hello_dynamic/  # 动态链接测试
    └── hashmap_test/   # HashMap 实现测试
```

## 添加新程序

1. 在适当的类别目录下创建程序目录
2. 在 `userspace/Cargo.toml` 的 workspace members 中添加路径
3. 在 `config/programs.yaml` 中添加配置

## 构建

```bash
./ndk userspace         # 构建所有用户空间程序
./ndk full              # 完整构建 (包含程序)
```

## 日志

构建日志保存在 `logs/programs/<category>/` 目录下，按类别组织。
