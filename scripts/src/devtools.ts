/**
 * NexaOS Development Kit - Developer Tools
 * 
 * 企业级开发工具：格式化、lint、检查、文档、审计、基准测试等
 */

import { spawn, spawnSync, execSync } from 'child_process';
import { existsSync, readdirSync, readFileSync, mkdirSync, writeFileSync } from 'fs';
import { resolve, relative, join } from 'path';
import { logger } from './logger.js';
import { BuildEnvironment } from './types.js';

// =============================================================================
// Types
// =============================================================================

export interface ToolResult {
  success: boolean;
  output: string;
  errorCount: number;
  warningCount: number;
  fixedCount?: number;
}

// =============================================================================
// Workspace Discovery
// =============================================================================

interface RustWorkspace {
  name: string;
  path: string;
  manifest: string;
  toolchain: 'stable' | 'nightly';
  isNoStd: boolean;
}

/**
 * 发现项目中所有 Rust 工作空间
 */
function discoverWorkspaces(projectRoot: string): RustWorkspace[] {
  const workspaces: RustWorkspace[] = [];
  
  // 主内核 (no_std, nightly)
  workspaces.push({
    name: 'kernel',
    path: projectRoot,
    manifest: resolve(projectRoot, 'Cargo.toml'),
    toolchain: 'nightly',
    isNoStd: true,
  });
  
  // 测试 crate (std, nightly)
  if (existsSync(resolve(projectRoot, 'tests/Cargo.toml'))) {
    workspaces.push({
      name: 'tests',
      path: resolve(projectRoot, 'tests'),
      manifest: resolve(projectRoot, 'tests/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: false,
    });
  }
  
  // Boot info (no_std, nightly)
  if (existsSync(resolve(projectRoot, 'boot/boot-info/Cargo.toml'))) {
    workspaces.push({
      name: 'boot-info',
      path: resolve(projectRoot, 'boot/boot-info'),
      manifest: resolve(projectRoot, 'boot/boot-info/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: true,
    });
  }
  
  // UEFI loader (no_std, nightly)
  if (existsSync(resolve(projectRoot, 'boot/uefi-loader/Cargo.toml'))) {
    workspaces.push({
      name: 'uefi-loader',
      path: resolve(projectRoot, 'boot/uefi-loader'),
      manifest: resolve(projectRoot, 'boot/uefi-loader/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: true,
    });
  }
  
  // NVM hypervisor (std, nightly - uses x86_64 crate which requires nightly features)
  if (existsSync(resolve(projectRoot, 'nvm/Cargo.toml'))) {
    workspaces.push({
      name: 'nvm',
      path: resolve(projectRoot, 'nvm'),
      manifest: resolve(projectRoot, 'nvm/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: false,
    });
  }
  
  // Userspace workspace (包含 nrlib, ld-nrlib, libs, programs)
  if (existsSync(resolve(projectRoot, 'userspace/Cargo.toml'))) {
    workspaces.push({
      name: 'userspace',
      path: resolve(projectRoot, 'userspace'),
      manifest: resolve(projectRoot, 'userspace/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: false,
    });
  }
  
  // Modules workspace (no_std, nightly) - 作为整体处理
  if (existsSync(resolve(projectRoot, 'modules/Cargo.toml'))) {
    workspaces.push({
      name: 'modules',
      path: resolve(projectRoot, 'modules'),
      manifest: resolve(projectRoot, 'modules/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: true,
    });
  }
  
  return workspaces;
}
// =============================================================================
// Format Tool (fmt)
// =============================================================================

/**
 * 格式化所有 Rust 代码
 */
export async function formatCode(
  env: BuildEnvironment,
  options: {
    check?: boolean;      // 只检查不修改
    verbose?: boolean;
    workspace?: string;   // 指定工作空间
  } = {}
): Promise<ToolResult> {
  const workspaces = discoverWorkspaces(env.projectRoot);
  const result: ToolResult = {
    success: true,
    output: '',
    errorCount: 0,
    warningCount: 0,
    fixedCount: 0,
  };
  
  logger.section(`Formatting Code${options.check ? ' (check mode)' : ''}`);
  
  // Filter workspaces if specified
  const targetWorkspaces = options.workspace
    ? workspaces.filter(w => w.name === options.workspace)
    : workspaces;
  
  if (targetWorkspaces.length === 0) {
    logger.error(`Workspace not found: ${options.workspace}`);
    logger.info(`Available: ${workspaces.map(w => w.name).join(', ')}`);
    result.success = false;
    return result;
  }
  
  for (const workspace of targetWorkspaces) {
    logger.step(`Formatting: ${workspace.name}`);
    
    const args = [`+${workspace.toolchain}`, 'fmt'];
    if (options.check) {
      args.push('--check');
    }
    args.push('--manifest-path', workspace.manifest);
    
    if (options.verbose) {
      args.push('--', '-v');
    }
    
    const fmtResult = spawnSync('cargo', args, {
      cwd: env.projectRoot,
      encoding: 'utf-8',
      stdio: ['inherit', 'pipe', 'pipe'],
    });
    
    result.output += fmtResult.stdout || '';
    result.output += fmtResult.stderr || '';
    
    if (fmtResult.status !== 0) {
      if (options.check) {
        // In check mode, non-zero means files need formatting
        const unformattedFiles = (fmtResult.stdout || '').match(/Diff in /g);
        result.errorCount += unformattedFiles ? unformattedFiles.length : 1;
        logger.warn(`${workspace.name}: formatting issues found`);
      } else {
        result.errorCount++;
        logger.error(`${workspace.name}: format failed`);
      }
      result.success = false;
    } else {
      logger.success(`${workspace.name}: ${options.check ? 'formatted correctly' : 'formatted'}`);
    }
  }
  
  return result;
}

// =============================================================================
// Lint Tool (clippy)
// =============================================================================

/**
 * 运行 Clippy lint 检查
 */
export async function lintCode(
  env: BuildEnvironment,
  options: {
    fix?: boolean;        // 自动修复
    strict?: boolean;     // 严格模式（warnings = errors）
    verbose?: boolean;
    workspace?: string;
  } = {}
): Promise<ToolResult> {
  const workspaces = discoverWorkspaces(env.projectRoot);
  const result: ToolResult = {
    success: true,
    output: '',
    errorCount: 0,
    warningCount: 0,
    fixedCount: 0,
  };
  
  logger.section('Running Clippy Lint');
  
  const targetWorkspaces = options.workspace
    ? workspaces.filter(w => w.name === options.workspace)
    : workspaces;
  
  for (const workspace of targetWorkspaces) {
    logger.step(`Linting: ${workspace.name}`);
    
    const args = [`+${workspace.toolchain}`, 'clippy'];
    args.push('--manifest-path', workspace.manifest);
    
    if (options.fix) {
      args.push('--fix', '--allow-dirty', '--allow-staged');
    }
    
    // Add clippy args
    args.push('--');
    
    if (options.strict) {
      args.push('-D', 'warnings');  // Deny all warnings
    }
    
    // Common lints to check
    args.push('-W', 'clippy::all');
    
    // For no_std crates, skip std-specific lints
    if (workspace.isNoStd) {
      args.push('-A', 'clippy::missing_safety_doc');  // Allow missing safety docs in kernel
    }
    
    const lintResult = spawnSync('cargo', args, {
      cwd: env.projectRoot,
      encoding: 'utf-8',
      stdio: ['inherit', 'pipe', 'pipe'],
      env: {
        ...process.env,
        // Skip kernel's custom target for clippy
        ...(workspace.isNoStd && workspace.name !== 'kernel' ? {} : {}),
      },
    });
    
    result.output += lintResult.stdout || '';
    result.output += lintResult.stderr || '';
    
    // Count warnings and errors
    const warnings = ((lintResult.stderr || '') + (lintResult.stdout || '')).match(/warning:/g);
    const errors = ((lintResult.stderr || '') + (lintResult.stdout || '')).match(/error\[E/g);
    
    result.warningCount += warnings ? warnings.length : 0;
    result.errorCount += errors ? errors.length : 0;
    
    if (lintResult.status !== 0) {
      result.success = false;
      logger.warn(`${workspace.name}: ${result.warningCount} warnings, ${result.errorCount} errors`);
    } else {
      logger.success(`${workspace.name}: lint passed`);
    }
  }
  
  return result;
}

// =============================================================================
// Check Tool (cargo check)
// =============================================================================

/**
 * 快速类型检查
 */
export async function checkCode(
  env: BuildEnvironment,
  options: {
    verbose?: boolean;
    workspace?: string;
  } = {}
): Promise<ToolResult> {
  const workspaces = discoverWorkspaces(env.projectRoot);
  const result: ToolResult = {
    success: true,
    output: '',
    errorCount: 0,
    warningCount: 0,
  };
  
  logger.section('Running Type Check');
  
  const targetWorkspaces = options.workspace
    ? workspaces.filter(w => w.name === options.workspace)
    : workspaces;
  
  for (const workspace of targetWorkspaces) {
    // Skip kernel for now (needs custom target)
    if (workspace.name === 'kernel') {
      logger.info(`Skipping ${workspace.name} (use 'ndk build kernel' instead)`);
      continue;
    }
    
    logger.step(`Checking: ${workspace.name}`);
    
    const args = [`+${workspace.toolchain}`, 'check'];
    args.push('--manifest-path', workspace.manifest);
    
    const checkResult = spawnSync('cargo', args, {
      cwd: env.projectRoot,
      encoding: 'utf-8',
      stdio: ['inherit', 'pipe', 'pipe'],
    });
    
    result.output += checkResult.stdout || '';
    result.output += checkResult.stderr || '';
    
    if (checkResult.status !== 0) {
      result.success = false;
      result.errorCount++;
      logger.error(`${workspace.name}: check failed`);
    } else {
      logger.success(`${workspace.name}: check passed`);
    }
  }
  
  return result;
}

// =============================================================================
// Documentation Tool
// =============================================================================

interface DocEntry {
  name: string;
  path: string;
  description: string;
  category: 'core' | 'drivers' | 'userspace' | 'tools';
}

// 预定义的文档描述
const DOC_DESCRIPTIONS: Record<string, { description: string; category: DocEntry['category'] }> = {
  // Core
  nexa_os: { description: 'NexaOS Kernel - Hybrid kernel core with POSIX syscalls, scheduler, and memory management', category: 'core' },
  nexa_boot_info: { description: 'Boot Info - Multiboot2/UEFI boot information structures', category: 'core' },
  
  // NVM & Tools
  nvm: { description: 'NVM Hypervisor - Enterprise virtualization platform with VT-x/AMD-V', category: 'tools' },
  nvmctl: { description: 'NVM CLI - Command-line interface for NVM management', category: 'tools' },
  nvm_server: { description: 'NVM Server - Web API and management server for NVM', category: 'tools' },
  
  // Tests
  kernel_tests: { description: 'Kernel Tests - Unit and integration tests for kernel subsystems', category: 'tools' },
  nexa_kernel_tests: { description: 'Kernel Tests - Unit and integration tests', category: 'tools' },
  nexa_nvm_tests: { description: 'NVM Tests - Tests for hypervisor platform', category: 'tools' },
  nexa_modules_tests: { description: 'Module Tests - Tests for kernel modules', category: 'tools' },
  nexa_userspace_tests: { description: 'Userspace Tests - Tests for userspace libraries', category: 'tools' },
  
  // Userspace - Core libs
  nrlib: { description: 'nrlib - POSIX-compatible C library for userspace programs', category: 'userspace' },
  ld_nrlib: { description: 'ld-nrlib - Dynamic linker for ELF executables', category: 'userspace' },
  
  // Userspace - Libraries
  ncryptolib: { description: 'ncryptolib - Cryptographic library (AES, SHA, etc.)', category: 'userspace' },
  nssl: { description: 'nssl - SSL/TLS implementation', category: 'userspace' },
  nzip: { description: 'nzip - Compression library (gzip, deflate)', category: 'userspace' },
  nh2: { description: 'nh2 - HTTP/2 protocol implementation', category: 'userspace' },
  nh3: { description: 'nh3 - HTTP/3 (QUIC) protocol implementation', category: 'userspace' },
  ntcp2: { description: 'ntcp2 - TCP/IP networking library', category: 'userspace' },
  
  // Userspace - Programs
  init: { description: 'init - System V init daemon (PID 1)', category: 'userspace' },
  shell: { description: 'shell - NexaOS command shell', category: 'userspace' },
  getty: { description: 'getty - Terminal login handler', category: 'userspace' },
  login: { description: 'login - User authentication program', category: 'userspace' },
  
  // Modules - Filesystems
  ext2_module: { description: 'ext2 - EXT2 filesystem driver', category: 'drivers' },
  ext3_module: { description: 'ext3 - EXT3 filesystem driver', category: 'drivers' },
  ext4_module: { description: 'ext4 - EXT4 filesystem driver', category: 'drivers' },
  swap_module: { description: 'swap - Swap space management module', category: 'drivers' },
  
  // Modules - Block devices
  ahci_module: { description: 'ahci - AHCI/SATA controller driver', category: 'drivers' },
  ide_module: { description: 'ide - IDE/ATA disk driver', category: 'drivers' },
  nvme_module: { description: 'nvme - NVMe SSD driver', category: 'drivers' },
  virtio_blk_module: { description: 'virtio_blk - VirtIO block device driver', category: 'drivers' },
  
  // Modules - Network
  e1000_module: { description: 'e1000 - Intel E1000 network driver', category: 'drivers' },
  virtio_net_module: { description: 'virtio_net - VirtIO network driver', category: 'drivers' },
  virtio_common: { description: 'virtio_common - VirtIO common abstractions', category: 'drivers' },
};

/**
 * 动态发现并构建文档条目列表
 */
function buildDocEntries(
  projectRoot: string
): DocEntry[] {
  const entries: DocEntry[] = [];
  const docDir = resolve(projectRoot, 'target/doc');
  const tripleDocDir = resolve(projectRoot, 'target/x86_64-unknown-linux-gnu/doc');
  
  // 检查两个可能的文档目录
  const dirsToCheck = [docDir, tripleDocDir].filter(d => existsSync(d));
  
  const seenCrates = new Set<string>();
  
  for (const dir of dirsToCheck) {
    try {
      const items = readdirSync(dir, { withFileTypes: true });
      
      for (const item of items) {
        if (!item.isDirectory()) continue;
        
        const crateName = item.name;
        
        // 跳过静态资源目录
        if (['static.files', 'src', 'src-files.js', 'implementors'].includes(crateName)) continue;
        if (crateName.startsWith('.')) continue;
        
        // 检查是否有 index.html
        const indexPath = resolve(dir, crateName, 'index.html');
        if (!existsSync(indexPath)) continue;
        
        // 避免重复
        if (seenCrates.has(crateName)) continue;
        seenCrates.add(crateName);
        
        // 获取描述和分类
        const info = DOC_DESCRIPTIONS[crateName] || {
          description: `${crateName} - Rust crate documentation`,
          category: 'userspace' as const,
        };
        
        entries.push({
          name: crateName,
          path: `${crateName}/index.html`,
          description: info.description,
          category: info.category,
        });
      }
    } catch {
      // 忽略读取错误
    }
  }
  
  // 按类别和名称排序
  const categoryOrder = { core: 0, drivers: 1, userspace: 2, tools: 3 };
  entries.sort((a, b) => {
    const catDiff = categoryOrder[a.category] - categoryOrder[b.category];
    if (catDiff !== 0) return catDiff;
    return a.name.localeCompare(b.name);
  });
  
  return entries;
}

/**
 * 生成统一的文档索引页
 * 优先使用 Vue UI，否则使用简单 HTML 作为 fallback
 */
function generateDocsIndex(
  projectRoot: string,
  successfulDocs: string[]
): void {
  const docsDir = resolve(projectRoot, 'target/doc');
  const indexPath = resolve(docsDir, 'index.html');
  const vueDistPath = resolve(projectRoot, 'scripts/ui/docs/dist/index.html');
  
  // 动态发现所有已生成的文档
  const docEntries = buildDocEntries(projectRoot);
  
  // 准备文档数据
  const docsData = {
    projectName: 'NexaOS',
    version: '1.0.0',
    timestamp: new Date().toISOString(),
    docs: docEntries,
    workspaces: successfulDocs,
  };
  
  let html: string;
  
  // 尝试使用 Vue UI
  if (existsSync(vueDistPath)) {
    html = readFileSync(vueDistPath, 'utf-8');
    // 注入文档数据
    const dataScript = `<script id="docs-data" type="application/json">${JSON.stringify(docsData)}</script>`;
    html = html.replace('</head>', `${dataScript}\n</head>`);
  } else {
    // Fallback: 使用简单 HTML
    html = generateFallbackDocsHtml(docEntries);
  }
  
  // 确保目录存在并写入
  mkdirSync(docsDir, { recursive: true });
  writeFileSync(indexPath, html);
}

/**
 * 生成简单的 fallback HTML（当 Vue UI 未构建时使用）
 */
function generateFallbackDocsHtml(docEntries: DocEntry[]): string {
  const categoryNames: Record<string, string> = {
    core: 'Core Components',
    drivers: 'Drivers & Modules',
    userspace: 'Userspace Libraries',
    tools: 'Tools & Testing',
  };
  
  // 按类别分组
  const byCategory: Record<string, DocEntry[]> = {};
  for (const entry of docEntries) {
    if (!byCategory[entry.category]) {
      byCategory[entry.category] = [];
    }
    byCategory[entry.category].push(entry);
  }
  
  const categoryOrder = ['core', 'drivers', 'userspace', 'tools'];
  
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>NexaOS Documentation</title>
  <style>
    :root {
      --bg-color: #0f1419;
      --text-color: #c5c5c5;
      --link-color: #4fc1ff;
      --header-bg: #1a1f25;
      --card-bg: #1a1f25;
      --border-color: #2a2f35;
    }
    body {
      font-family: "Inter", -apple-system, BlinkMacSystemFont, sans-serif;
      background-color: var(--bg-color);
      color: var(--text-color);
      margin: 0;
      padding: 0;
      line-height: 1.6;
    }
    .container { max-width: 1200px; margin: 0 auto; padding: 2rem; }
    header {
      background: var(--header-bg);
      padding: 2rem 0;
      border-bottom: 1px solid var(--border-color);
      margin-bottom: 2rem;
    }
    header h1 { margin: 0; font-size: 2rem; color: #fff; }
    header p { margin: 0.5rem 0 0 0; color: #888; }
    .section-title {
      font-size: 1.25rem;
      color: #fff;
      margin: 2rem 0 1rem 0;
      padding-bottom: 0.5rem;
      border-bottom: 1px solid var(--border-color);
    }
    .docs-grid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
      gap: 1.25rem;
    }
    .doc-card {
      background: var(--card-bg);
      border: 1px solid var(--border-color);
      border-radius: 8px;
      padding: 1.25rem;
      transition: border-color 0.2s, transform 0.2s;
    }
    .doc-card:hover {
      border-color: var(--link-color);
      transform: translateY(-2px);
    }
    .doc-card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
    .doc-card h3 a { color: var(--link-color); text-decoration: none; }
    .doc-card h3 a:hover { text-decoration: underline; }
    .doc-card p { margin: 0; color: #888; font-size: 0.85rem; }
    footer {
      margin-top: 3rem;
      padding-top: 1.5rem;
      border-top: 1px solid var(--border-color);
      text-align: center;
      color: #666;
      font-size: 0.85rem;
    }
    footer code {
      background: var(--header-bg);
      padding: 0.2rem 0.5rem;
      border-radius: 4px;
    }
  </style>
</head>
<body>
  <header>
    <div class="container">
      <h1>📚 NexaOS Documentation</h1>
      <p>API Reference and Developer Documentation</p>
    </div>
  </header>
  <main class="container">
${categoryOrder.filter(cat => byCategory[cat]?.length).map(cat => `
    <h2 class="section-title">${categoryNames[cat]}</h2>
    <div class="docs-grid">
${byCategory[cat].map(doc => `      <div class="doc-card">
        <h3><a href="${doc.path}">${doc.name}</a></h3>
        <p>${doc.description}</p>
      </div>`).join('\n')}
    </div>`).join('\n')}
  </main>
  <footer>
    <p>Generated by NexaOS Development Kit (NDK) · <code>./ndk doc</code></p>
    <p style="margin-top: 0.5rem; font-size: 0.75rem;">
      Run <code>cd scripts/ui/docs && npm install && npm run build</code> for enhanced UI
    </p>
  </footer>
</body>
</html>`;
}

/**
 * 生成文档
 * 
 * 支持所有工作空间，包括需要自定义 target 的内核
 */
export async function generateDocs(
  env: BuildEnvironment,
  options: {
    open?: boolean;       // 打开浏览器
    private?: boolean;    // 包含私有项
    workspace?: string;   // 指定工作空间
    all?: boolean;        // 生成所有文档（包括内核）
    list?: boolean;       // 列出可用工作空间
  } = {}
): Promise<ToolResult> {
  const workspaces = discoverWorkspaces(env.projectRoot);
  const result: ToolResult = {
    success: true,
    output: '',
    errorCount: 0,
    warningCount: 0,
  };
  
  // 列出可用工作空间
  if (options.list) {
    logger.section('Available Documentation Targets');
    console.log('');
    for (const ws of workspaces) {
      const noStdTag = ws.isNoStd ? ' [no_std]' : '';
      console.log(`  ${ws.name.padEnd(15)} ${ws.toolchain.padEnd(8)}${noStdTag}`);
    }
    console.log('');
    logger.info('Use --workspace <name> to document specific workspace');
    logger.info('Use --all to document all workspaces including kernel');
    return result;
  }
  
  logger.section('Generating Documentation');
  
  // 确定要生成文档的工作空间
  let targetWorkspaces: RustWorkspace[];
  if (options.workspace) {
    targetWorkspaces = workspaces.filter(w => w.name === options.workspace);
    if (targetWorkspaces.length === 0) {
      logger.error(`Workspace not found: ${options.workspace}`);
      result.success = false;
      result.errorCount = 1;
      return result;
    }
  } else if (options.all) {
    // 包含所有工作空间
    targetWorkspaces = workspaces;
  } else {
    // 默认跳过内核（需要特殊处理）和 uefi-loader
    targetWorkspaces = workspaces.filter(w => 
      w.name !== 'kernel' && w.name !== 'uefi-loader'
    );
  }
  
  const successfulDocs: string[] = [];
  const centralDocDir = resolve(env.projectRoot, 'target/doc');
  
  // 确保中央文档目录存在
  mkdirSync(centralDocDir, { recursive: true });
  
  for (const workspace of targetWorkspaces) {
    logger.step(`Documenting: ${workspace.name}`);
    
    const args: string[] = [`+${workspace.toolchain}`, 'doc'];
    args.push('--manifest-path', workspace.manifest);
    args.push('--no-deps');  // 不生成依赖项文档
    
    // Userspace 需要排除某些有 optional 依赖问题的 crate
    if (workspace.name === 'userspace') {
      args.push('--workspace');
      args.push('--exclude', 'uefi_compatd');  // 有 optional nrlib 依赖问题
    }
    
    // 关键：根目录 .cargo/config.toml 设置了默认 target 为内核 target
    // 所有非内核工作空间都必须显式指定 --target 来覆盖
    if (workspace.name === 'kernel') {
      // 内核需要自定义 target 和 build-std
      const targetJson = resolve(env.projectRoot, 'targets/x86_64-nexaos.json');
      args.push('--target', targetJson);
      args.push('-Zbuild-std=core,compiler_builtins,alloc');
      args.push('-Zbuild-std-features=compiler-builtins-mem');
      args.push('--target-dir', resolve(env.projectRoot, 'target/kernel-doc'));
    } else {
      // 所有其他工作空间（包括 no_std 的 modules、boot-info）都使用标准 Linux target
      // 这样可以生成文档而不需要 build-std
      args.push('--target', 'x86_64-unknown-linux-gnu');
      args.push('--target-dir', resolve(env.projectRoot, 'target'));
    }
    
    if (options.private) {
      args.push('--document-private-items');
    }
    
    const docResult = spawnSync('cargo', args, {
      cwd: workspace.path,
      encoding: 'utf-8',
      stdio: ['inherit', 'pipe', 'pipe'],
      env: {
        ...process.env,
        RUSTDOCFLAGS: '--enable-index-page -Zunstable-options',
      },
    });
    
    result.output += docResult.stdout || '';
    result.output += docResult.stderr || '';
    
    if (docResult.status !== 0) {
      result.success = false;
      result.errorCount++;
      logger.error(`${workspace.name}: doc failed`);
      // 显示错误详情
      if (docResult.stderr) {
        const lines = docResult.stderr.split('\n').slice(0, 10);
        for (const line of lines) {
          if (line.trim()) console.log(`  ${line}`);
        }
      }
    } else {
      successfulDocs.push(workspace.name);
      logger.success(`${workspace.name}: documented`);
    }
  }
  
  // 复制所有文档到中央位置
  logger.step('Consolidating documentation');
  
  // 复制内核文档
  if (successfulDocs.includes('kernel')) {
    const kernelDocSrc = resolve(env.projectRoot, 'target/kernel-doc/targets/x86_64-nexaos/doc');
    if (existsSync(kernelDocSrc)) {
      try {
        execSync(`cp -r ${kernelDocSrc}/* ${centralDocDir}/`, { stdio: 'ignore' });
      } catch {
        // 忽略复制错误
      }
    }
  }
  
  // 复制模块文档
  if (successfulDocs.includes('modules')) {
    const modulesDocSrc = resolve(env.projectRoot, 'target/modules-doc/targets/x86_64-nexaos-module/doc');
    if (existsSync(modulesDocSrc)) {
      try {
        execSync(`cp -r ${modulesDocSrc}/* ${centralDocDir}/`, { stdio: 'ignore' });
      } catch {
        // 忽略复制错误
      }
    }
  }
  
  // 复制标准工作空间文档（可能在 target/<triple>/doc 或 target/doc）
  const standardDocLocations = [
    resolve(env.projectRoot, 'target/x86_64-unknown-linux-gnu/doc'),
    resolve(env.projectRoot, 'target/doc'),
  ];
  
  for (const docLocation of standardDocLocations) {
    if (existsSync(docLocation) && docLocation !== centralDocDir) {
      try {
        // 复制所有内容（排除 .lock 文件）
        execSync(`cp -r ${docLocation}/* ${centralDocDir}/ 2>/dev/null || true`, { stdio: 'ignore' });
      } catch {
        // 忽略复制错误
      }
    }
  }
  
  // 生成统一索引页
  if (successfulDocs.length > 0) {
    logger.step('Generating documentation index');
    try {
      generateDocsIndex(env.projectRoot, successfulDocs);
      logger.success('Index generated: target/doc/index.html');
    } catch (e) {
      logger.warn('Failed to generate index page');
    }
  }
  
  // 打印文档位置
  if (result.success && successfulDocs.length > 0) {
    console.log('');
    logger.info(`Documentation generated at: ${resolve(env.projectRoot, 'target/doc')}`);
  }
  
  // 按需打开浏览器
  if (options.open && result.success) {
    const docsPath = resolve(env.projectRoot, 'target/doc/index.html');
    if (existsSync(docsPath)) {
      logger.info('Opening documentation in browser...');
      spawn('xdg-open', [docsPath], { detached: true, stdio: 'ignore' }).unref();
    }
  }
  
  return result;
}

// =============================================================================
// Audit Tool (security)
// =============================================================================

/**
 * 安全审计（检查已知漏洞）
 */
export async function auditDependencies(
  env: BuildEnvironment,
  options: {
    fix?: boolean;        // 尝试自动修复
    verbose?: boolean;
  } = {}
): Promise<ToolResult> {
  const result: ToolResult = {
    success: true,
    output: '',
    errorCount: 0,
    warningCount: 0,
  };
  
  logger.section('Security Audit');
  
  // Check if cargo-audit is installed
  const auditCheck = spawnSync('cargo', ['audit', '--version'], { encoding: 'utf-8' });
  
  if (auditCheck.status !== 0) {
    logger.warn('cargo-audit not installed. Installing...');
    const install = spawnSync('cargo', ['install', 'cargo-audit'], {
      stdio: 'inherit',
      encoding: 'utf-8',
    });
    if (install.status !== 0) {
      result.success = false;
      result.output = 'Failed to install cargo-audit';
      return result;
    }
  }
  
  // Run audit on main workspace
  logger.step('Auditing dependencies...');
  
  const args = ['audit'];
  if (options.fix) {
    args.push('--fix');
  }
  if (options.verbose) {
    args.push('--deny', 'warnings');
  }
  
  const auditResult = spawnSync('cargo', args, {
    cwd: env.projectRoot,
    encoding: 'utf-8',
    stdio: ['inherit', 'pipe', 'pipe'],
  });
  
  result.output = (auditResult.stdout || '') + (auditResult.stderr || '');
  
  // Count vulnerabilities
  const vulnMatches = result.output.match(/Vulnerability/g);
  result.errorCount = vulnMatches ? vulnMatches.length : 0;
  
  const warningMatches = result.output.match(/warning:/g);
  result.warningCount = warningMatches ? warningMatches.length : 0;
  
  if (auditResult.status !== 0 || result.errorCount > 0) {
    result.success = false;
    logger.error(`Found ${result.errorCount} vulnerabilities`);
  } else {
    logger.success('No known vulnerabilities found');
  }
  
  return result;
}

// =============================================================================
// Outdated Tool
// =============================================================================

/**
 * 检查过期依赖
 */
export async function checkOutdated(
  env: BuildEnvironment,
  _options: {
    workspace?: string;
  } = {}
): Promise<ToolResult> {
  const result: ToolResult = {
    success: true,
    output: '',
    errorCount: 0,
    warningCount: 0,
  };
  
  logger.section('Checking Outdated Dependencies');
  
  // Check if cargo-outdated is installed
  const check = spawnSync('cargo', ['outdated', '--version'], { encoding: 'utf-8' });
  
  if (check.status !== 0) {
    logger.warn('cargo-outdated not installed. Installing...');
    const install = spawnSync('cargo', ['install', 'cargo-outdated'], {
      stdio: 'inherit',
      encoding: 'utf-8',
    });
    if (install.status !== 0) {
      result.success = false;
      result.output = 'Failed to install cargo-outdated';
      return result;
    }
  }
  
  const args = ['outdated'];
  
  const outdatedResult = spawnSync('cargo', args, {
    cwd: env.projectRoot,
    encoding: 'utf-8',
    stdio: ['inherit', 'pipe', 'pipe'],
  });
  
  result.output = (outdatedResult.stdout || '') + (outdatedResult.stderr || '');
  
  // Count outdated
  const outdatedLines = result.output.split('\n').filter(l => l.includes('->'));
  result.warningCount = outdatedLines.length;
  
  if (result.warningCount > 0) {
    logger.warn(`${result.warningCount} dependencies have updates available`);
  } else {
    logger.success('All dependencies are up to date');
  }
  
  return result;
}

// =============================================================================
// SLOC Tool (lines of code)
// =============================================================================

interface SlocStats {
  files: number;
  blank: number;
  comment: number;
  code: number;
}

/**
 * 统计代码行数
 */
export async function countLines(env: BuildEnvironment): Promise<void> {
  logger.section('Lines of Code Statistics');
  
  const stats: Record<string, SlocStats> = {};
  
  const countDir = (dir: string, name: string) => {
    const dirPath = resolve(env.projectRoot, dir);
    if (!existsSync(dirPath)) return;
    
    const result: SlocStats = { files: 0, blank: 0, comment: 0, code: 0 };
    
    const processFile = (filePath: string) => {
      if (!filePath.endsWith('.rs')) return;
      
      result.files++;
      const content = readFileSync(filePath, 'utf-8');
      const lines = content.split('\n');
      
      let inBlockComment = false;
      
      for (const line of lines) {
        const trimmed = line.trim();
        
        if (trimmed === '') {
          result.blank++;
        } else if (inBlockComment) {
          result.comment++;
          if (trimmed.includes('*/')) {
            inBlockComment = false;
          }
        } else if (trimmed.startsWith('/*')) {
          result.comment++;
          if (!trimmed.includes('*/')) {
            inBlockComment = true;
          }
        } else if (trimmed.startsWith('//')) {
          result.comment++;
        } else {
          result.code++;
        }
      }
    };
    
    const walkDir = (d: string) => {
      try {
        const entries = readdirSync(d, { withFileTypes: true });
        for (const entry of entries) {
          const fullPath = join(d, entry.name);
          if (entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'target') {
            walkDir(fullPath);
          } else if (entry.isFile()) {
            processFile(fullPath);
          }
        }
      } catch {
        // Ignore errors
      }
    };
    
    walkDir(dirPath);
    stats[name] = result;
  };
  
  // Count each component
  countDir('src', 'Kernel');
  countDir('tests/src', 'Tests');
  countDir('userspace/nrlib/src', 'nrlib');
  countDir('userspace/programs', 'Programs');
  countDir('userspace/lib', 'Libraries');
  countDir('nvm/src', 'NVM');
  countDir('modules', 'Modules');
  countDir('boot', 'Boot');
  
  // Print table
  console.log('\n' + '─'.repeat(65));
  console.log(`${'Component'.padEnd(20)} ${'Files'.padStart(8)} ${'Blank'.padStart(8)} ${'Comment'.padStart(8)} ${'Code'.padStart(10)}`);
  console.log('─'.repeat(65));
  
  let total: SlocStats = { files: 0, blank: 0, comment: 0, code: 0 };
  
  for (const [name, s] of Object.entries(stats)) {
    console.log(
      `${name.padEnd(20)} ${s.files.toString().padStart(8)} ${s.blank.toString().padStart(8)} ${s.comment.toString().padStart(8)} ${s.code.toString().padStart(10)}`
    );
    total.files += s.files;
    total.blank += s.blank;
    total.comment += s.comment;
    total.code += s.code;
  }
  
  console.log('─'.repeat(65));
  console.log(
    `${'Total'.padEnd(20)} ${total.files.toString().padStart(8)} ${total.blank.toString().padStart(8)} ${total.comment.toString().padStart(8)} ${total.code.toString().padStart(10)}`
  );
  console.log('─'.repeat(65));
  console.log('');
}

// =============================================================================
// List Workspaces
// =============================================================================

export function listWorkspaces(env: BuildEnvironment): void {
  const workspaces = discoverWorkspaces(env.projectRoot);
  
  console.log('\n📦 Rust Workspaces\n');
  
  for (const ws of workspaces) {
    const exists = existsSync(ws.manifest);
    const status = exists ? '\x1b[32m✓\x1b[0m' : '\x1b[31m✗\x1b[0m';
    
    console.log(`  ${status} \x1b[1m${ws.name}\x1b[0m`);
    console.log(`     Path: ${relative(env.projectRoot, ws.path)}`);
    console.log(`     Toolchain: ${ws.toolchain}`);
    console.log(`     no_std: ${ws.isNoStd}`);
    console.log('');
  }
}
