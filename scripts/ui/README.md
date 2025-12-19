# NexaOS UI Tools

这个目录包含 NexaOS 的 Web UI 工具。

## 目录结构

```
scripts/ui/
├── coverage/           # 代码覆盖率报告 UI (Vue)
│   ├── src/           # Vue 源码
│   ├── dist/          # 构建输出
│   └── build.ts       # 构建脚本
│
├── config/            # OS 配置裁剪 UI (Vue)
│   ├── src/           # Vue 源码
│   │   ├── views/     # 页面组件
│   │   ├── components/# 复用组件
│   │   ├── stores/    # Pinia 状态管理
│   │   ├── api/       # API 客户端
│   │   ├── locales/   # i18n 国际化
│   │   └── router/    # Vue Router
│   └── dist/          # 构建输出
│
└── config-api/        # 配置 API 后端 (FastAPI)
    └── nexaos_config_api/
        ├── main.py    # FastAPI 应用
        └── config.py  # 配置文件管理
```

## 使用方法

### 代码覆盖率报告

```bash
# 生成 HTML 覆盖率报告
./ndk coverage html
```

### OS 配置 UI

```bash
# 启动配置 UI (前端 + API)
./ndk ui

# 仅启动 API 服务
./ndk ui api

# 构建前端生产版本
./ndk ui build
```

## 功能

### 覆盖率 UI (`coverage/`)
- 测试结果展示
- 代码覆盖率统计
- 模块级覆盖详情
- 中英文支持

### 配置 UI (`config/` + `config-api/`)
- 内核功能开关 (网络、图形、安全等)
- 内核模块选择 (ext2, e1000, virtio 等)
- 用户空间程序选择
- 预设配置 (full, minimal, embedded, server)
- 一键构建
- 大小估算
- 中英文支持

## 开发

### 前端开发

```bash
cd scripts/ui/config
npm install
npm run dev
```

### 后端开发

```bash
cd scripts/ui/config-api
pip install -e .
uvicorn nexaos_config_api.main:app --reload --port 8765
```

## API 端点

| 端点 | 方法 | 描述 |
|------|------|------|
| `/api/features` | GET/PUT | 内核功能配置 |
| `/api/modules` | GET/PUT | 内核模块配置 |
| `/api/programs` | GET/PUT | 用户程序配置 |
| `/api/presets` | GET | 获取预设列表 |
| `/api/presets/{name}/apply` | POST | 应用预设 |
| `/api/build` | POST | 开始构建 |
| `/api/build/status` | GET | 构建状态 |
| `/api/estimates` | GET | 大小估算 |
