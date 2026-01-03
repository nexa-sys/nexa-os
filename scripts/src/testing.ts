/**
 * NexaOS Development Kit - Testing Module
 * 
 * tests/ workspace 统一管理所有测试:
 *   tests/kernel/     - 内核测试
 *   tests/userspace/  - 用户空间测试
 *   tests/modules/    - 内核模块测试
 *   tests/nvm/        - NVM 平台测试
 */

import { spawn } from 'child_process';
import { existsSync, readFileSync } from 'fs';
import { resolve } from 'path';
import { parse as parseYaml } from 'yaml';
import { logger } from './logger.js';
import { BuildEnvironment } from './types.js';

// =============================================================================
// Types
// =============================================================================

export interface TestTarget {
  name: string;
  crate: string;
  path: string;
  toolchain: string;
  features?: string[];
  coverage?: {
    source_dirs: string[];
    exclude?: string[];
  };
}

export interface TestConfig {
  workspace: {
    path: string;
    manifest: string;
    toolchain: string;
  };
  targets: Record<string, TestTarget>;
  categories?: Record<string, {
    pattern: string;
    parallel: boolean;
    timeout: number;
    skip_quick?: boolean;
  }>;
  coverage?: {
    thresholds: {
      statements: number;
      branches: number;
      functions: number;
      lines: number;
    };
    formats: string[];
    output_dir: string;
  };
  quality_gates?: {
    tests: {
      min_pass_rate: number;
      max_flaky_rate: number;
    };
    coverage: {
      min_statements: number;
      min_branches: number;
      min_functions: number;
      min_lines: number;
    };
  };
}

export interface TestRunResult {
  target: string;
  success: boolean;
  passed: number;
  failed: number;
  skipped: number;
  duration: number;
  warnings: number;
  errors: number;
  output: string;
}

// =============================================================================
// Configuration
// =============================================================================

export function loadTestConfig(projectRoot: string): TestConfig {
  const configPath = resolve(projectRoot, 'config/testing.yaml');
  
  if (!existsSync(configPath)) {
    // Default config
    return {
      workspace: {
        path: 'tests/',
        manifest: 'tests/Cargo.toml',
        toolchain: 'nightly',
      },
      targets: {
        kernel: {
          name: '内核测试',
          crate: 'nexa-kernel-tests',
          path: 'tests/kernel',
          toolchain: 'nightly',
          features: ['net_full', 'kernel_full', 'fs_full', 'gfx_full'],
        },
        userspace: {
          name: '用户空间测试',
          crate: 'nexa-userspace-tests',
          path: 'tests/userspace',
          toolchain: 'nightly',
        },
        modules: {
          name: '内核模块测试',
          crate: 'nexa-modules-tests',
          path: 'tests/modules',
          toolchain: 'nightly',
        },
        nvm: {
          name: 'NVM 平台测试',
          crate: 'nexa-nvm-tests',
          path: 'tests/nvm',
          toolchain: 'nightly',
        },
      },
    };
  }
  
  const content = readFileSync(configPath, 'utf-8');
  return parseYaml(content) as TestConfig;
}

// =============================================================================
// Test Runner
// =============================================================================

export async function runTests(
  env: BuildEnvironment,
  options: {
    target?: string;
    filter?: string;
    verbose?: boolean;
    quick?: boolean;
  } = {}
): Promise<TestRunResult[]> {
  const config = loadTestConfig(env.projectRoot);
  const results: TestRunResult[] = [];
  
  // Determine which targets to run
  let targetsToRun: [string, TestTarget][];
  
  if (options.target) {
    const targetConfig = config.targets[options.target];
    if (!targetConfig) {
      logger.error(`未知测试目标: ${options.target}`);
      logger.info(`可用目标: ${Object.keys(config.targets).join(', ')}`);
      return [{
        target: options.target,
        success: false,
        passed: 0,
        failed: 0,
        skipped: 0,
        duration: 0,
        warnings: 0,
        errors: 0,
        output: `Unknown target: ${options.target}`,
      }];
    }
    targetsToRun = [[options.target, targetConfig]];
  } else {
    targetsToRun = Object.entries(config.targets);
  }
  
  logger.section(`运行测试 (${targetsToRun.length} 个目标)`);
  
  for (const [targetName, targetConfig] of targetsToRun) {
    logger.step(`测试: ${targetConfig.name}`);
    
    const result = await runTargetTests(env, config, targetName, targetConfig, options);
    results.push(result);
    
    if (result.success) {
      logger.success(`${targetConfig.name}: ${result.passed} 通过`);
    } else {
      logger.error(`${targetConfig.name}: ${result.failed} 失败`);
    }
  }
  
  return results;
}

async function runTargetTests(
  env: BuildEnvironment,
  config: TestConfig,
  targetName: string,
  targetConfig: TestTarget,
  options: {
    filter?: string;
    verbose?: boolean;
    quick?: boolean;
  }
): Promise<TestRunResult> {
  const startTime = Date.now();
  const result: TestRunResult = {
    target: targetName,
    success: true,
    passed: 0,
    failed: 0,
    skipped: 0,
    duration: 0,
    warnings: 0,
    errors: 0,
    output: '',
  };
  
  const workspacePath = resolve(env.projectRoot, config.workspace.path);
  
  if (!existsSync(workspacePath)) {
    result.success = false;
    result.output = `Workspace not found: ${workspacePath}`;
    result.errors = 1;
    return result;
  }
  
  // Build cargo command - run in tests/ workspace
  const toolchain = `+${targetConfig.toolchain}`;
  const args = [toolchain, 'test', '-p', targetConfig.crate];
  
  // Add features
  if (targetConfig.features && targetConfig.features.length > 0) {
    args.push('--features', targetConfig.features.join(','));
  }
  
  // Add filter
  if (options.filter) {
    args.push(options.filter);
  }
  
  // Add test runner args
  args.push('--', '--test-threads=1');
  
  if (options.quick) {
    args.push('--skip', 'slow_');
    args.push('--skip', 'stress_');
  }
  
  return new Promise((resolvePromise) => {
    const child = spawn('cargo', args, {
      cwd: workspacePath,
      env: {
        ...process.env,
        // 使用 workspace 的 target 目录以利用编译缓存
        CARGO_TARGET_DIR: resolve(workspacePath, 'target'),
      },
      stdio: ['inherit', 'pipe', 'pipe'],
    });
    
    let stdout = '';
    let stderr = '';
    
    child.stdout?.on('data', (data) => {
      const str = data.toString();
      stdout += str;
      if (options.verbose) {
        process.stdout.write(data);
      }
    });
    
    child.stderr?.on('data', (data) => {
      const str = data.toString();
      stderr += str;
      if (options.verbose) {
        process.stderr.write(data);
      }
    });
    
    child.on('close', (code) => {
      result.output = stdout + stderr;
      result.duration = Date.now() - startTime;
      
      // Count warnings and errors
      const warningMatches = result.output.match(/warning:/g);
      const errorMatches = result.output.match(/error\[E\d+\]/g);
      result.warnings = warningMatches ? warningMatches.length : 0;
      result.errors = errorMatches ? errorMatches.length : 0;
      
      // Parse test results
      const testPattern = /test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)/g;
      let match;
      while ((match = testPattern.exec(result.output)) !== null) {
        if (match[2] === 'ok') {
          result.passed++;
        } else if (match[2] === 'FAILED') {
          result.failed++;
        } else {
          result.skipped++;
        }
      }
      
      result.success = code === 0 && result.failed === 0;
      resolvePromise(result);
    });
    
    child.on('error', (err) => {
      result.success = false;
      result.output = err.message;
      result.errors = 1;
      resolvePromise(result);
    });
  });
}

// =============================================================================
// Test Listing
// =============================================================================

export async function listTestTargets(env: BuildEnvironment): Promise<void> {
  const config = loadTestConfig(env.projectRoot);
  
  console.log('\n📋 可用测试目标\n');
  
  for (const [id, target] of Object.entries(config.targets)) {
    const pathExists = existsSync(resolve(env.projectRoot, target.path));
    const status = pathExists ? '✓' : '✗';
    const statusColor = pathExists ? '\x1b[32m' : '\x1b[31m';
    
    console.log(`  ${statusColor}${status}\x1b[0m \x1b[1m${id}\x1b[0m - ${target.name}`);
    console.log(`     Crate: ${target.crate}`);
    console.log(`     Path: ${target.path}`);
    if (target.features && target.features.length > 0) {
      console.log(`     Features: ${target.features.join(', ')}`);
    }
    console.log('');
  }
}

// =============================================================================
// Summary Generation
// =============================================================================

export function generateTestSummary(results: TestRunResult[]): string {
  const lines: string[] = [];
  
  lines.push('\n' + '='.repeat(60));
  lines.push('                    测试摘要');
  lines.push('='.repeat(60) + '\n');
  
  let totalPassed = 0;
  let totalFailed = 0;
  let totalSkipped = 0;
  let totalDuration = 0;
  
  for (const result of results) {
    totalPassed += result.passed;
    totalFailed += result.failed;
    totalSkipped += result.skipped;
    totalDuration += result.duration;
    
    const status = result.success ? '\x1b[32m✓ 通过\x1b[0m' : '\x1b[31m✗ 失败\x1b[0m';
    lines.push(`  ${status} ${result.target}`);
    lines.push(`       测试: ${result.passed} 通过, ${result.failed} 失败, ${result.skipped} 跳过`);
    lines.push(`       耗时: ${(result.duration / 1000).toFixed(2)}s`);
    lines.push('');
  }
  
  lines.push('-'.repeat(60));
  
  const totalTests = totalPassed + totalFailed + totalSkipped;
  const passRate = totalTests > 0 ? ((totalPassed / totalTests) * 100).toFixed(1) : '0.0';
  
  lines.push(`  总计: ${totalTests} 测试 | ${totalPassed} 通过 | ${totalFailed} 失败 | ${totalSkipped} 跳过`);
  lines.push(`  通过率: ${passRate}%`);
  lines.push(`  总耗时: ${(totalDuration / 1000).toFixed(2)}s`);
  lines.push('\n' + '='.repeat(60) + '\n');
  
  return lines.join('\n');
}

// =============================================================================
// Quality Gate Check
// =============================================================================

export function checkQualityGates(
  env: BuildEnvironment,
  testResults: TestRunResult[],
  coverageStats?: { statementCoveragePct: number; branchCoveragePct: number; functionCoveragePct: number; lineCoveragePct: number }
): { passed: boolean; violations: string[] } {
  const config = loadTestConfig(env.projectRoot);
  const gates = config.quality_gates;
  const violations: string[] = [];
  
  if (!gates) {
    return { passed: true, violations: [] };
  }
  
  // Check test pass rate
  const totalTests = testResults.reduce((sum, r) => r.passed + r.failed + r.skipped + sum, 0);
  const passedTests = testResults.reduce((sum, r) => sum + r.passed, 0);
  const passRate = totalTests > 0 ? (passedTests / totalTests) * 100 : 0;
  
  if (passRate < gates.tests.min_pass_rate) {
    violations.push(`测试通过率 ${passRate.toFixed(1)}% 低于最低要求 ${gates.tests.min_pass_rate}%`);
  }
  
  // Check coverage thresholds
  if (coverageStats && gates.coverage) {
    if (coverageStats.statementCoveragePct < gates.coverage.min_statements) {
      violations.push(`语句覆盖率 ${coverageStats.statementCoveragePct.toFixed(1)}% 低于最低要求 ${gates.coverage.min_statements}%`);
    }
    if (coverageStats.branchCoveragePct < gates.coverage.min_branches) {
      violations.push(`分支覆盖率 ${coverageStats.branchCoveragePct.toFixed(1)}% 低于最低要求 ${gates.coverage.min_branches}%`);
    }
    if (coverageStats.functionCoveragePct < gates.coverage.min_functions) {
      violations.push(`函数覆盖率 ${coverageStats.functionCoveragePct.toFixed(1)}% 低于最低要求 ${gates.coverage.min_functions}%`);
    }
    if (coverageStats.lineCoveragePct < gates.coverage.min_lines) {
      violations.push(`行覆盖率 ${coverageStats.lineCoveragePct.toFixed(1)}% 低于最低要求 ${gates.coverage.min_lines}%`);
    }
  }
  
  return {
    passed: violations.length === 0,
    violations,
  };
}
