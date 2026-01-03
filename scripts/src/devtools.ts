/**
 * NexaOS Development Kit - Developer Tools
 * 
 * 企业级开发工具：格式化、lint、检查、文档、审计、基准测试等
 */

import { spawn, spawnSync } from 'child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync } from 'fs';
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
  
  // Boot info (no_std, stable)
  if (existsSync(resolve(projectRoot, 'boot/boot-info/Cargo.toml'))) {
    workspaces.push({
      name: 'boot-info',
      path: resolve(projectRoot, 'boot/boot-info'),
      manifest: resolve(projectRoot, 'boot/boot-info/Cargo.toml'),
      toolchain: 'stable',
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
  
  // NVM hypervisor (std, stable)
  if (existsSync(resolve(projectRoot, 'nvm/Cargo.toml'))) {
    workspaces.push({
      name: 'nvm',
      path: resolve(projectRoot, 'nvm'),
      manifest: resolve(projectRoot, 'nvm/Cargo.toml'),
      toolchain: 'stable',
      isNoStd: false,
    });
  }
  
  // Userspace workspace (std, nightly for no_std compat)
  if (existsSync(resolve(projectRoot, 'userspace/Cargo.toml'))) {
    workspaces.push({
      name: 'userspace',
      path: resolve(projectRoot, 'userspace'),
      manifest: resolve(projectRoot, 'userspace/Cargo.toml'),
      toolchain: 'nightly',
      isNoStd: false,
    });
  }
  
  // Modules workspace (no_std, nightly)
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

/**
 * 生成文档
 */
export async function generateDocs(
  env: BuildEnvironment,
  options: {
    open?: boolean;       // 打开浏览器
    private?: boolean;    // 包含私有项
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
  
  logger.section('Generating Documentation');
  
  // Filter to documentable workspaces (skip kernel for now)
  const targetWorkspaces = (options.workspace
    ? workspaces.filter(w => w.name === options.workspace)
    : workspaces
  ).filter(w => w.name !== 'kernel');  // Skip kernel (needs custom target)
  
  for (const workspace of targetWorkspaces) {
    logger.step(`Documenting: ${workspace.name}`);
    
    const args = [`+${workspace.toolchain}`, 'doc'];
    args.push('--manifest-path', workspace.manifest);
    args.push('--no-deps');  // Don't document dependencies
    
    if (options.private) {
      args.push('--document-private-items');
    }
    
    const docResult = spawnSync('cargo', args, {
      cwd: env.projectRoot,
      encoding: 'utf-8',
      stdio: ['inherit', 'pipe', 'pipe'],
    });
    
    result.output += docResult.stdout || '';
    result.output += docResult.stderr || '';
    
    if (docResult.status !== 0) {
      result.success = false;
      result.errorCount++;
      logger.error(`${workspace.name}: doc failed`);
    } else {
      logger.success(`${workspace.name}: documented`);
    }
  }
  
  // Open docs if requested
  if (options.open && result.success) {
    const docsPath = resolve(env.projectRoot, 'target/doc/index.html');
    if (existsSync(docsPath)) {
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
  options: {
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
