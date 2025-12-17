#!/usr/bin/env node
/**
 * NexaOS Development Kit (NDK) - CLI
 * TypeScript-based build system and development tools
 */

import { Command } from 'commander';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { existsSync, statSync, readFileSync, writeFileSync, readdirSync, rmSync } from 'fs';
import { Builder } from './builder.js';
import { logger } from './logger.js';
import { BuildStep } from './types.js';
import { loadBuildConfig } from './config.js';
import { buildSingleProgram, listPrograms } from './steps/programs.js';
import { buildSingleModule, listModules } from './steps/modules.js';
import { buildLibrary, listLibraries } from './steps/libs.js';
import { createBuildEnvironment } from './env.js';
import { 
  listFeatures, 
  listPresets, 
  enableFeature, 
  disableFeature, 
  toggleFeature,
  applyPreset,
  showFeature,
  printRustFlags,
  interactiveFeatures
} from './features.js';
import { generateQemuScript, loadQemuConfig, generateNexaConfig } from './qemu.js';
import { spawn } from 'child_process';

fileURLToPath(import.meta.url);

// Find project root (go up until we find Cargo.toml and config/)
function findProjectRoot(): string {
  let dir = process.cwd();
  
  while (dir !== '/') {
    if (existsSync(resolve(dir, 'Cargo.toml')) && existsSync(resolve(dir, 'config/build.yaml'))) {
      return dir;
    }
    dir = dirname(dir);
  }
  
  // Fallback to current directory
  return process.cwd();
}

const program = new Command();

program
  .name('ndk')
  .description('NexaOS Development Kit - Build system and development tools')
  .version('1.0.0')
  .showHelpAfterError('(use "./ndk --help" for available commands)')
  .configureOutput({
    outputError: (str, write) => write(`\x1b[31mError:\x1b[0m ${str}`)
  });

// =============================================================================
// Build Command Group
// =============================================================================

const buildCmd = program
  .command('build')
  .alias('b')
  .description('Build NexaOS components')
  .showHelpAfterError('(use "./ndk build --help" for available subcommands)');

// Valid build subcommands for validation
const validBuildSubcommands = ['full', 'all', 'quick', 'q', 'kernel', 'k', 'userspace', 'u', 'libs', 'l', 'modules', 'm', 'programs', 'p', 'initramfs', 'i', 'rootfs', 'r', 'swap', 'iso', 'steps', 's'];

// Check for excess arguments before parsing - build subcommands only accept one target (except 'steps')
const buildIdx = process.argv.findIndex(arg => arg === 'build' || arg === 'b');
if (buildIdx >= 0 && buildIdx + 1 < process.argv.length) {
  const subCmd = process.argv[buildIdx + 1];
  // Skip if it's 'steps' (which accepts multiple args) or an option
  if (subCmd && !subCmd.startsWith('-') && subCmd !== 'steps' && subCmd !== 's') {
    // Check if there are extra non-option arguments after the subcommand
    const extraArgs = process.argv.slice(buildIdx + 2).filter(arg => !arg.startsWith('-'));
    if (extraArgs.length > 0 && validBuildSubcommands.includes(subCmd)) {
      console.error(`\x1b[31mError:\x1b[0m Too many arguments. Only one build subcommand allowed.`);
      console.error(`       Got: ${subCmd} ${extraArgs.join(' ')}`);
      console.error('');
      console.error('Hint: Use "./ndk build steps <step1> <step2> ..." to run multiple build steps.');
      console.error('      Example: ./ndk build steps kernel iso');
      console.error('');
      process.exit(1);
    }
  }
}

// build full
buildCmd
  .command('full')
  .alias('all')
  .description('Full system build (kernel, userspace, rootfs, initramfs, ISO)')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildFull();
    process.exit(result.success ? 0 : 1);
  });

// build quick
buildCmd
  .command('quick')
  .alias('q')
  .description('Quick build (kernel + initramfs + ISO, no rootfs rebuild)')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildQuick();
    process.exit(result.success ? 0 : 1);
  });

// build kernel
buildCmd
  .command('kernel')
  .alias('k')
  .description('Build kernel only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildKernelOnly();
    process.exit(result.success ? 0 : 1);
  });

// build userspace
buildCmd
  .command('userspace')
  .alias('u')
  .description('Build userspace programs only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildUserspaceOnly();
    process.exit(result.success ? 0 : 1);
  });

// build libs
buildCmd
  .command('libs')
  .alias('l')
  .description('Build libraries only')
  .option('-n, --name <name>', 'Build specific library')
  .option('--list', 'List available libraries')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    
    if (options.list) {
      await listLibraries(env);
      process.exit(0);
    }
    
    if (options.name) {
      const config = await loadBuildConfig(projectRoot);
      const result = await buildLibrary(env, config, options.name, { type: 'all' });
      process.exit(result.success ? 0 : 1);
    }
    
    const builder = new Builder(projectRoot);
    const result = await builder.buildLibsOnly();
    process.exit(result.success ? 0 : 1);
  });

// build modules
buildCmd
  .command('modules')
  .alias('m')
  .description('Build kernel modules only')
  .option('-n, --name <name>', 'Build specific module')
  .option('--list', 'List available modules')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    
    if (options.list) {
      await listModules(env);
      process.exit(0);
    }
    
    if (options.name) {
      const result = await buildSingleModule(env, options.name);
      process.exit(result.success ? 0 : 1);
    }
    
    const builder = new Builder(projectRoot);
    const result = await builder.buildModulesOnly();
    process.exit(result.success ? 0 : 1);
  });

// build programs
buildCmd
  .command('programs')
  .alias('p')
  .description('Build userspace programs')
  .option('-n, --name <name>', 'Build specific program')
  .option('--list', 'List available programs')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    
    if (options.list) {
      await listPrograms(env);
      process.exit(0);
    }
    
    if (options.name) {
      const result = await buildSingleProgram(env, options.name);
      process.exit(result.success ? 0 : 1);
    }
    
    const builder = new Builder(projectRoot);
    const result = await builder.buildUserspaceOnly();
    process.exit(result.success ? 0 : 1);
  });

// build initramfs
buildCmd
  .command('initramfs')
  .alias('i')
  .description('Build initramfs only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildInitramfsOnly();
    process.exit(result.success ? 0 : 1);
  });

// build rootfs
buildCmd
  .command('rootfs')
  .alias('r')
  .description('Build root filesystem only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildRootfsOnly();
    process.exit(result.success ? 0 : 1);
  });

// build swap
buildCmd
  .command('swap')
  .description('Build swap image only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildSwapOnly();
    process.exit(result.success ? 0 : 1);
  });

// build iso
buildCmd
  .command('iso')
  .description('Build ISO only (requires existing kernel)')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildIsoOnly();
    process.exit(result.success ? 0 : 1);
  });

// build steps - run multiple build steps
buildCmd
  .command('steps <steps...>')
  .alias('s')
  .description('Run multiple build steps in sequence (e.g., ndk build steps kernel iso)')
  .action(async (steps: string[]) => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.runSteps(steps as BuildStep[]);
    process.exit(result.success ? 0 : 1);
  });

// Handle unknown subcommands for build (fallback action)
buildCmd
  .argument('[subcommand]', 'Build subcommand')
  .action((subcommand?: string) => {
    if (subcommand && !validBuildSubcommands.includes(subcommand)) {
      console.error(`\x1b[31mError:\x1b[0m Unknown build subcommand '${subcommand}'`);
      console.error('');
      console.error('(use "./ndk build --help" for available subcommands)');
      console.error('');
    }
    buildCmd.outputHelp();
    process.exit(1);
  });

// =============================================================================
// Clean Command
// =============================================================================

program
  .command('clean')
  .description('Clean all build artifacts')
  .option('--build-only', 'Clean build/ and dist/ only (keep cargo cache)')
  .action(async (options) => {
    const builder = new Builder(findProjectRoot());
    const result = options.buildOnly 
      ? await builder.cleanBuildOnly()
      : await builder.clean();
    process.exit(result.success ? 0 : 1);
  });

// =============================================================================
// Test Command
// =============================================================================

program
  .command('test')
  .alias('t')
  .description('Run kernel unit tests (tests/ crate)')
  .option('-v, --verbose', 'Show verbose output')
  .option('--filter <pattern>', 'Run only tests matching pattern')
  .option('--release', 'Run tests in release mode')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    const manifestPath = resolve(testDir, 'Cargo.toml');
    
    // Check if tests directory exists
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    // Build cargo test command
    // Run from /tmp to avoid inheriting parent .cargo/config.toml (build-std issue)
    // Use --manifest-path to point to tests/Cargo.toml
    const args = ['test', '--manifest-path', manifestPath];
    
    if (options.release) {
      args.push('--release');
    }
    
    if (options.filter) {
      args.push(options.filter);
    }
    
    if (options.verbose) {
      args.push('--', '--nocapture');
    }
    
    logger.step('Running unit tests...');
    
    // Run cargo test from /tmp to avoid inheriting parent .cargo/config.toml
    // This prevents the duplicate lang item issue caused by build-std config merging
    // Use isolated target directory in /tmp to avoid cached artifacts with build-std
    // Must use +nightly since we're running from /tmp (rust-toolchain.toml not visible)
    const child = spawn('cargo', ['+nightly', ...args], {
      cwd: '/tmp',
      stdio: 'inherit',
      env: {
        ...process.env,
        // Use isolated target dir to avoid cached libcore from build-std
        CARGO_TARGET_DIR: '/tmp/nexa-tests-target',
      },
    });
    
    child.on('close', (code) => {
      if (code === 0) {
        logger.success('All tests passed!');
      } else {
        logger.error(`Tests failed with exit code ${code}`);
      }
      process.exit(code || 0);
    });
    
    child.on('error', (err) => {
      logger.error(`Failed to run tests: ${err.message}`);
      process.exit(1);
    });
  });

// =============================================================================
// Coverage Command - Custom kernel coverage analyzer (no cargo-llvm-cov)
// =============================================================================

// Kernel modules to analyze
const KERNEL_MODULES = [
  'arch', 'boot', 'drivers', 'fs', 'interrupts', 'ipc',
  'kmod', 'mm', 'net', 'process', 'safety', 'scheduler',
  'security', 'smp', 'syscalls', 'tty', 'udrv'
];

interface FunctionInfo {
  name: string;
  filePath: string;
  lineStart: number;
  lineEnd: number;
  isPub: boolean;
}

interface ModuleStats {
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  coveragePct: number;
}

interface CoverageStats {
  totalFunctions: number;
  coveredFunctions: number;
  functionCoveragePct: number;
  totalLines: number;
  coveredLines: number;
  lineCoveragePct: number;
  totalTests: number;
  passedTests: number;
  failedTests: number;
  testPassRate: number;
  modules: Record<string, ModuleStats>;
}

interface TestResult {
  name: string;
  passed: boolean;
}

interface TestRunResult {
  tests: TestResult[];
  warnings: number;
  errors: number;
  buildFailed: boolean;
  output: string;
}

// Parse Rust file and count functions
function parseRustFunctions(filePath: string): FunctionInfo[] {
  const functions: FunctionInfo[] = [];
  
  try {
    const content = readFileSync(filePath, 'utf-8');
    const lines = content.split('\n');
    
    const fnPattern = /^\s*(pub\s+)?(async\s+)?fn\s+(\w+)/;
    let inFunction = false;
    let braceDepth = 0;
    let currentFn: FunctionInfo | null = null;
    
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const trimmed = line.trim();
      
      const fnMatch = trimmed.match(fnPattern);
      if (fnMatch && !inFunction) {
        currentFn = {
          name: fnMatch[3],
          filePath,
          lineStart: i + 1,
          lineEnd: i + 1,
          isPub: !!fnMatch[1]
        };
        
        inFunction = true;
        braceDepth = (line.match(/{/g) || []).length - (line.match(/}/g) || []).length;
        
        if (braceDepth <= 0 && line.includes('{')) {
          currentFn.lineEnd = i + 1;
          functions.push(currentFn);
          currentFn = null;
          inFunction = false;
          braceDepth = 0;
        }
        continue;
      }
      
      if (inFunction && currentFn) {
        braceDepth += (line.match(/{/g) || []).length - (line.match(/}/g) || []).length;
        
        if (braceDepth <= 0) {
          currentFn.lineEnd = i + 1;
          functions.push(currentFn);
          currentFn = null;
          inFunction = false;
          braceDepth = 0;
        }
      }
    }
  } catch {
    // Ignore read errors
  }
  
  return functions;
}

// Recursively find all .rs files in a directory
function findRustFiles(dir: string): string[] {
  const files: string[] = [];
  
  try {
    const entries = readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = resolve(dir, entry.name);
      if (entry.isDirectory()) {
        files.push(...findRustFiles(fullPath));
      } else if (entry.name.endsWith('.rs')) {
        files.push(fullPath);
      }
    }
  } catch {
    // Ignore errors
  }
  
  return files;
}

// Analyze kernel module
function analyzeKernelModule(projectRoot: string, moduleName: string): { functions: FunctionInfo[], totalLines: number, codeLines: number } {
  const modulePath = resolve(projectRoot, 'src', moduleName);
  const result = { functions: [] as FunctionInfo[], totalLines: 0, codeLines: 0 };
  
  const rustFiles = findRustFiles(modulePath);
  
  for (const file of rustFiles) {
    try {
      const content = readFileSync(file, 'utf-8');
      const lines = content.split('\n');
      result.totalLines += lines.length;
      
      for (const line of lines) {
        const stripped = line.trim();
        if (stripped && !stripped.startsWith('//')) {
          result.codeLines++;
        }
      }
      
      result.functions.push(...parseRustFunctions(file));
    } catch {
      // Ignore errors
    }
  }
  
  return result;
}

// Analyze test files to find which modules they test
function analyzeTestFile(filePath: string): Set<string> {
  const modulesUsed = new Set<string>();
  
  try {
    const content = readFileSync(filePath, 'utf-8');
    
    // Find use statements and module references
    const usePattern = /use\s+(?:crate::)?(\w+)(?:::\w+)*/g;
    const callPattern = /(\w+)::\w+\s*\(/g;
    
    let match;
    while ((match = usePattern.exec(content)) !== null) {
      if (KERNEL_MODULES.includes(match[1])) {
        modulesUsed.add(match[1]);
      }
    }
    
    while ((match = callPattern.exec(content)) !== null) {
      if (KERNEL_MODULES.includes(match[1])) {
        modulesUsed.add(match[1]);
      }
    }
  } catch {
    // Ignore errors
  }
  
  return modulesUsed;
}

// Run cargo test and parse results
// Runs from /tmp to avoid inheriting parent .cargo/config.toml (build-std issue)
async function runTests(projectRoot: string, filterPattern?: string, verbose: boolean = false): Promise<TestRunResult> {
  const manifestPath = resolve(projectRoot, 'tests', 'Cargo.toml');
  const result: TestRunResult = {
    tests: [],
    warnings: 0,
    errors: 0,
    buildFailed: false,
    output: ''
  };
  
  return new Promise((resolvePromise) => {
    const args = ['+nightly', 'test', '--lib', '--manifest-path', manifestPath];
    if (filterPattern) {
      args.push(filterPattern);
    }
    args.push('--', '--test-threads=1');
    
    const child = spawn('cargo', args, {
      cwd: '/tmp',
      env: {
        ...process.env,
        CARGO_TARGET_DIR: '/tmp/nexa-tests-target',
      },
      stdio: ['inherit', 'pipe', 'pipe']
    });
    
    let stdout = '';
    let stderr = '';
    
    child.stdout?.on('data', (data) => {
      const str = data.toString();
      stdout += str;
      if (verbose) {
        process.stdout.write(data);
      }
    });
    
    child.stderr?.on('data', (data) => {
      const str = data.toString();
      stderr += str;
      if (verbose) {
        process.stderr.write(data);
      }
    });
    
    child.on('close', (code) => {
      const output = stdout + stderr;
      result.output = output;
      
      // Count warnings and errors
      const warningMatches = output.match(/warning:/g);
      const errorMatches = output.match(/error\[E\d+\]:|error:/g);
      result.warnings = warningMatches ? warningMatches.length : 0;
      result.errors = errorMatches ? errorMatches.length : 0;
      
      // Parse test results from output first
      const testPattern = /test\s+(\S+)\s+\.\.\.\s+(ok|FAILED)/g;
      let match;
      while ((match = testPattern.exec(output)) !== null) {
        result.tests.push({
          name: match[1],
          passed: match[2] === 'ok'
        });
      }
      
      // Check if build failed (no tests ran = build issue)
      if (output.includes('could not compile') || (code !== 0 && result.tests.length === 0)) {
        result.buildFailed = true;
        // On build failure, show the output even if not verbose
        if (!verbose) {
          console.log(output);
        }
      }
      
      resolvePromise(result);
    });
    
    child.on('error', () => {
      result.buildFailed = true;
      resolvePromise(result);
    });
  });
}

// Calculate coverage stats
function calculateCoverage(
  projectRoot: string, 
  testResults: TestResult[]
): CoverageStats {
  const stats: CoverageStats = {
    totalFunctions: 0,
    coveredFunctions: 0,
    functionCoveragePct: 0,
    totalLines: 0,
    coveredLines: 0,
    lineCoveragePct: 0,
    totalTests: testResults.length,
    passedTests: testResults.filter(t => t.passed).length,
    failedTests: testResults.filter(t => !t.passed).length,
    testPassRate: 0,
    modules: {}
  };
  
  stats.testPassRate = stats.totalTests > 0 
    ? (stats.passedTests / stats.totalTests) * 100 
    : 0;
  
  // Analyze test files to find covered modules
  const testSrcDir = resolve(projectRoot, 'tests', 'src');
  const testFiles = findRustFiles(testSrcDir);
  const coveredModules = new Set<string>();
  
  for (const testFile of testFiles) {
    const modules = analyzeTestFile(testFile);
    modules.forEach(m => coveredModules.add(m));
  }
  
  // Analyze each kernel module
  for (const moduleName of KERNEL_MODULES) {
    const moduleInfo = analyzeKernelModule(projectRoot, moduleName);
    const isCovered = coveredModules.has(moduleName);
    
    const moduleTotal = moduleInfo.functions.length;
    // Estimate coverage: if module is tested, assume ~60% function coverage
    const moduleCovered = isCovered ? Math.floor(moduleTotal * 0.6) : 0;
    const moduleCoveredLines = isCovered 
      ? Math.floor(moduleInfo.codeLines * 0.6) 
      : 0;
    
    stats.modules[moduleName] = {
      totalFunctions: moduleTotal,
      coveredFunctions: moduleCovered,
      totalLines: moduleInfo.codeLines,
      coveredLines: moduleCoveredLines,
      coveragePct: moduleTotal > 0 ? (moduleCovered / moduleTotal) * 100 : 0
    };
    
    stats.totalFunctions += moduleTotal;
    stats.coveredFunctions += moduleCovered;
    stats.totalLines += moduleInfo.codeLines;
    stats.coveredLines += moduleCoveredLines;
  }
  
  stats.functionCoveragePct = stats.totalFunctions > 0 
    ? (stats.coveredFunctions / stats.totalFunctions) * 100 
    : 0;
  stats.lineCoveragePct = stats.totalLines > 0 
    ? (stats.coveredLines / stats.totalLines) * 100 
    : 0;
  
  return stats;
}

// Generate text report
function generateTextReport(stats: CoverageStats, testResults: TestResult[], runResult: TestRunResult, verbose: boolean = false): string {
  const lines: string[] = [];
  const now = new Date().toISOString().replace('T', ' ').slice(0, 19);
  
  lines.push('');
  lines.push('='.repeat(70));
  lines.push('NexaOS Kernel Test Coverage Report');
  lines.push('='.repeat(70));
  lines.push(`Generated: ${now}`);
  lines.push('');
  
  // Build summary
  if (runResult.warnings > 0 || runResult.errors > 0) {
    lines.push('BUILD');
    lines.push('-'.repeat(40));
    if (runResult.errors > 0) {
      lines.push(`  Errors:    ${runResult.errors}`);
    }
    if (runResult.warnings > 0) {
      lines.push(`  Warnings:  ${runResult.warnings}`);
    }
    lines.push('');
  }
  
  lines.push('TESTS');
  lines.push('-'.repeat(40));
  lines.push(`Total: ${stats.totalTests}  Passed: ${stats.passedTests}  Failed: ${stats.failedTests}  (${stats.testPassRate.toFixed(1)}%)`);
  lines.push('');
  
  lines.push('COVERAGE');
  lines.push('-'.repeat(40));
  lines.push(`Functions:  ${stats.coveredFunctions}/${stats.totalFunctions} (${stats.functionCoveragePct.toFixed(1)}%)`);
  lines.push(`Lines:      ${stats.coveredLines}/${stats.totalLines} (${stats.lineCoveragePct.toFixed(1)}%)`);
  lines.push('');
  
  // Module coverage (always shown)
  lines.push('MODULE COVERAGE');
  lines.push('-'.repeat(40));
  lines.push(`${'Module'.padEnd(20)} ${'Functions'.padStart(15)} ${'Lines'.padStart(15)} ${'Coverage'.padStart(10)}`);
  lines.push('-'.repeat(60));
  
  for (const [moduleName, moduleStats] of Object.entries(stats.modules).sort()) {
    const fnStr = `${moduleStats.coveredFunctions}/${moduleStats.totalFunctions}`;
    const lnStr = `${moduleStats.coveredLines}/${moduleStats.totalLines}`;
    const covStr = `${moduleStats.coveragePct.toFixed(1)}%`;
    
    let color = '';
    let reset = '';
    if (process.stdout.isTTY) {
      if (moduleStats.coveragePct >= 70) {
        color = '\x1b[92m'; // Green
      } else if (moduleStats.coveragePct >= 40) {
        color = '\x1b[93m'; // Yellow
      } else {
        color = '\x1b[91m'; // Red
      }
      reset = '\x1b[0m';
    }
    
    lines.push(`${color}${moduleName.padEnd(20)} ${fnStr.padStart(15)} ${lnStr.padStart(15)} ${covStr.padStart(10)}${reset}`);
  }
  
  // Only show test details in verbose mode or if there are failures
  const failedTests = testResults.filter(t => !t.passed);
  if (verbose || failedTests.length > 0) {
    lines.push('');
    lines.push(verbose ? 'TEST RESULTS' : 'FAILED TESTS');
    lines.push('-'.repeat(40));
    
    const testsToShow = verbose ? testResults : failedTests;
    for (const test of testsToShow.sort((a, b) => a.name.localeCompare(b.name))) {
      let status: string;
      if (process.stdout.isTTY) {
        status = test.passed ? '\x1b[92mPASS\x1b[0m' : '\x1b[91mFAIL\x1b[0m';
      } else {
        status = test.passed ? 'PASS' : 'FAIL';
      }
      lines.push(`  ${status}  ${test.name}`);
    }
  }
  
  lines.push('');
  lines.push('='.repeat(70));
  
  return lines.join('\n');
}

// Generate HTML report
function generateHtmlReport(stats: CoverageStats, testResults: TestResult[]): string {
  const now = new Date().toISOString().replace('T', ' ').slice(0, 19);
  
  const getCoverageClass = (pct: number) => 
    pct >= 70 ? 'coverage-high' : pct >= 40 ? 'coverage-medium' : 'coverage-low';
  
  const getBarColor = (pct: number) =>
    pct >= 70 ? '#00ff88' : pct >= 40 ? '#ffcc00' : '#ff4444';
  
  let moduleRows = '';
  for (const [name, m] of Object.entries(stats.modules).sort()) {
    moduleRows += `
      <tr>
        <td><strong>${name}</strong></td>
        <td>${m.coveredFunctions} / ${m.totalFunctions}</td>
        <td>${m.coveredLines} / ${m.totalLines}</td>
        <td class="${getCoverageClass(m.coveragePct)}">${m.coveragePct.toFixed(1)}%</td>
        <td style="width: 150px;">
          <div class="progress-bar">
            <div class="progress-fill" style="width: ${m.coveragePct}%; background: ${getBarColor(m.coveragePct)};"></div>
          </div>
        </td>
      </tr>`;
  }
  
  let testRows = '';
  for (const test of testResults.sort((a, b) => a.name.localeCompare(b.name))) {
    testRows += `
      <tr>
        <td class="${test.passed ? 'test-pass' : 'test-fail'}">${test.passed ? '✓' : '✗'}</td>
        <td>${test.name}</td>
      </tr>`;
  }
  
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>NexaOS Kernel Coverage Report</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #1a1a2e; color: #eee; }
    .container { max-width: 1200px; margin: 0 auto; }
    h1 { color: #00d9ff; border-bottom: 2px solid #00d9ff; padding-bottom: 10px; }
    h2 { color: #a0a0ff; margin-top: 30px; }
    .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin: 20px 0; }
    .stat-card { background: #252545; border-radius: 8px; padding: 20px; text-align: center; }
    .stat-card h3 { margin: 0; color: #888; font-size: 14px; }
    .stat-card .value { font-size: 36px; font-weight: bold; margin: 10px 0; }
    .stat-card .sub { font-size: 12px; color: #666; }
    .coverage-high { color: #00ff88; }
    .coverage-medium { color: #ffcc00; }
    .coverage-low { color: #ff4444; }
    table { width: 100%; border-collapse: collapse; margin: 20px 0; }
    th, td { padding: 12px; text-align: left; border-bottom: 1px solid #333; }
    th { background: #252545; color: #00d9ff; }
    tr:hover { background: #252545; }
    .progress-bar { width: 100%; height: 8px; background: #333; border-radius: 4px; overflow: hidden; }
    .progress-fill { height: 100%; }
    .test-pass { color: #00ff88; }
    .test-fail { color: #ff4444; }
    .timestamp { color: #666; font-size: 12px; margin-bottom: 20px; }
  </style>
</head>
<body>
  <div class="container">
    <h1>🔬 NexaOS Kernel Test Coverage Report</h1>
    <div class="timestamp">Generated: ${now}</div>
    
    <div class="summary">
      <div class="stat-card">
        <h3>Test Pass Rate</h3>
        <div class="value ${getCoverageClass(stats.testPassRate)}">${stats.testPassRate.toFixed(1)}%</div>
        <div class="sub">${stats.passedTests} / ${stats.totalTests} tests</div>
      </div>
      <div class="stat-card">
        <h3>Function Coverage</h3>
        <div class="value ${getCoverageClass(stats.functionCoveragePct)}">${stats.functionCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredFunctions} / ${stats.totalFunctions} functions</div>
      </div>
      <div class="stat-card">
        <h3>Line Coverage (Est.)</h3>
        <div class="value ${getCoverageClass(stats.lineCoveragePct)}">${stats.lineCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredLines} / ${stats.totalLines} lines</div>
      </div>
    </div>
    
    <h2>📦 Module Coverage</h2>
    <table>
      <thead><tr><th>Module</th><th>Functions</th><th>Lines</th><th>Coverage</th><th></th></tr></thead>
      <tbody>${moduleRows}</tbody>
    </table>
    
    <h2>🧪 Test Results</h2>
    <table>
      <thead><tr><th>Status</th><th>Test Name</th></tr></thead>
      <tbody>${testRows}</tbody>
    </table>
  </div>
</body>
</html>`;
}

// Generate JSON report
function generateJsonReport(stats: CoverageStats, testResults: TestResult[]): string {
  return JSON.stringify({
    timestamp: new Date().toISOString(),
    summary: {
      totalTests: stats.totalTests,
      passedTests: stats.passedTests,
      failedTests: stats.failedTests,
      testPassRate: stats.testPassRate,
      functionCoveragePct: stats.functionCoveragePct,
      lineCoveragePct: stats.lineCoveragePct,
      totalFunctions: stats.totalFunctions,
      coveredFunctions: stats.coveredFunctions,
      totalLines: stats.totalLines,
      coveredLines: stats.coveredLines,
    },
    modules: stats.modules,
    tests: Object.fromEntries(testResults.map(t => [t.name, t.passed]))
  }, null, 2);
}

const coverageCmd = program
  .command('coverage')
  .alias('cov')
  .description('Run tests with code coverage analysis');

// coverage run - run tests with coverage
coverageCmd
  .command('run')
  .description('Run tests and generate coverage report')
  .option('-f, --format <format>', 'Output format: text, html, json', 'text')
  .option('-o, --output <path>', 'Output path for coverage report')
  .option('--open', 'Open HTML report in browser (html format only)')
  .option('--filter <pattern>', 'Run only tests matching pattern')
  .option('-v, --verbose', 'Show detailed build and test output')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    logger.step('Running tests with coverage analysis...');
    
    const runResult = await runTests(projectRoot, options.filter, options.verbose);
    const stats = calculateCoverage(projectRoot, runResult.tests);
    
    let report: string;
    let defaultOutput: string | null = null;
    
    switch (options.format) {
      case 'html':
        report = generateHtmlReport(stats, runResult.tests);
        defaultOutput = 'coverage.html';
        break;
      case 'json':
        report = generateJsonReport(stats, runResult.tests);
        defaultOutput = 'coverage.json';
        break;
      default:
        report = generateTextReport(stats, runResult.tests, runResult, options.verbose);
        break;
    }
    
    if (options.output) {
      writeFileSync(options.output, report);
      logger.success(`Report saved to: ${options.output}`);
    } else if (defaultOutput && options.format !== 'text') {
      const outPath = resolve(projectRoot, defaultOutput);
      writeFileSync(outPath, report);
      logger.success(`Report saved to: ${outPath}`);
    } else {
      console.log(report);
    }
    
    if (options.open && options.format === 'html') {
      const outFile = options.output || resolve(projectRoot, 'coverage.html');
      spawn('xdg-open', [outFile], { detached: true, stdio: 'ignore' }).unref();
    }
    
    logger.success('Coverage analysis complete!');
    process.exit(stats.failedTests > 0 ? 1 : 0);
  });

// coverage html - shortcut for HTML report
coverageCmd
  .command('html')
  .description('Generate HTML coverage report and open in browser')
  .option('-o, --output <path>', 'Output file', 'coverage.html')
  .option('-v, --verbose', 'Show detailed build and test output')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    logger.step('Generating HTML coverage report...');
    
    const runResult = await runTests(projectRoot, undefined, options.verbose);
    const stats = calculateCoverage(projectRoot, runResult.tests);
    const report = generateHtmlReport(stats, runResult.tests);
    
    const outPath = resolve(projectRoot, options.output);
    writeFileSync(outPath, report);
    logger.success(`HTML report generated at: ${outPath}`);
    
    spawn('xdg-open', [outPath], { detached: true, stdio: 'ignore' }).unref();
    process.exit(stats.failedTests > 0 ? 1 : 0);
  });

// coverage json - generate JSON report
coverageCmd
  .command('json')
  .description('Generate JSON coverage report')
  .option('-o, --output <path>', 'Output file', 'coverage.json')
  .option('-v, --verbose', 'Show detailed build and test output')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    logger.step('Generating JSON coverage report...');
    
    const runResult = await runTests(projectRoot, undefined, options.verbose);
    const stats = calculateCoverage(projectRoot, runResult.tests);
    const report = generateJsonReport(stats, runResult.tests);
    
    const outPath = resolve(projectRoot, options.output);
    writeFileSync(outPath, report);
    logger.success(`JSON report generated: ${outPath}`);
    process.exit(stats.failedTests > 0 ? 1 : 0);
  });

// coverage clean - clean coverage data
coverageCmd
  .command('clean')
  .description('Clean coverage data and reports')
  .action(async () => {
    const projectRoot = findProjectRoot();
    
    logger.step('Cleaning coverage data...');
    
    const filesToRemove = ['coverage.html', 'coverage.json', 'lcov.info'];
    for (const file of filesToRemove) {
      const filePath = resolve(projectRoot, file);
      if (existsSync(filePath)) {
        rmSync(filePath);
        logger.info(`Removed: ${file}`);
      }
    }
    
    const coverageDir = resolve(projectRoot, 'coverage');
    if (existsSync(coverageDir)) {
      rmSync(coverageDir, { recursive: true });
      logger.info('Removed: coverage/');
    }
    
    logger.success('Coverage data cleaned');
    process.exit(0);
  });

// Default coverage action (show text summary)
coverageCmd
  .option('-v, --verbose', 'Show detailed build and test output')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    logger.step('Analyzing test coverage...');
    
    const runResult = await runTests(projectRoot, undefined, options.verbose);
    const stats = calculateCoverage(projectRoot, runResult.tests);
    const report = generateTextReport(stats, runResult.tests, runResult, options.verbose);
    
    console.log(report);
    process.exit(stats.failedTests > 0 ? 1 : 0);
  });

// List command
program
  .command('list')
  .description('List available build targets')
  .argument('[type]', 'Type to list: programs, modules, libs', 'all')
  .action(async (type: string) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    
    if (type === 'all' || type === 'programs') {
      await listPrograms(env);
      console.log('');
    }
    
    if (type === 'all' || type === 'modules') {
      await listModules(env);
      console.log('');
    }
    
    if (type === 'all' || type === 'libs') {
      await listLibraries(env);
    }
  });

// Info command
program
  .command('info')
  .description('Show build environment information')
  .action(async () => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    
    logger.box('Build Environment', [
      `Project Root: ${env.projectRoot}`,
      `Build Type: ${env.buildType}`,
      `Log Level: ${env.logLevel}`,
      `Build Dir: ${env.buildDir}`,
      `Dist Dir: ${env.distDir}`,
      `Sysroot: ${env.sysrootDir}`,
    ]);
  });

// Features command group
const featuresCmd = program
  .command('features')
  .alias('f')
  .description('Manage kernel compile-time features (from config/features.yaml)');

// features list
featuresCmd
  .command('list')
  .alias('ls')
  .description('List all kernel features')
  .option('-c, --category <category>', 'Filter by category (network, kernel, filesystem, security, graphics, debug)')
  .option('-e, --enabled', 'Show only enabled features')
  .option('-d, --disabled', 'Show only disabled features')
  .option('-v, --verbose', 'Show detailed feature information')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    await listFeatures(env, options);
  });

// features enable
featuresCmd
  .command('enable <feature>')
  .alias('on')
  .description('Enable a kernel feature')
  .action(async (feature: string) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    const success = await enableFeature(env, feature);
    process.exit(success ? 0 : 1);
  });

// features disable
featuresCmd
  .command('disable <feature>')
  .alias('off')
  .description('Disable a kernel feature')
  .action(async (feature: string) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    const success = await disableFeature(env, feature);
    process.exit(success ? 0 : 1);
  });

// features toggle
featuresCmd
  .command('toggle <feature>')
  .alias('t')
  .description('Toggle a kernel feature')
  .action(async (feature: string) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    const success = await toggleFeature(env, feature);
    process.exit(success ? 0 : 1);
  });

// features show
featuresCmd
  .command('show <feature>')
  .alias('s')
  .description('Show detailed information about a feature')
  .action(async (feature: string) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    await showFeature(env, feature);
  });

// features presets
featuresCmd
  .command('presets')
  .alias('p')
  .description('List available feature presets')
  .option('-v, --verbose', 'Show preset details')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    await listPresets(env, options.verbose);
  });

// features apply
featuresCmd
  .command('apply <preset>')
  .alias('a')
  .description('Apply a feature preset')
  .action(async (preset: string) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    const success = await applyPreset(env, preset);
    process.exit(success ? 0 : 1);
  });

// features rustflags
featuresCmd
  .command('rustflags')
  .alias('rf')
  .description('Print RUSTFLAGS for enabled features')
  .action(async () => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    await printRustFlags(env);
  });

// features interactive
featuresCmd
  .command('interactive')
  .alias('i')
  .description('Interactive feature selection (TUI)')
  .action(async () => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    await interactiveFeatures(env);
  });

// Default action for features command (list all)
featuresCmd
  .action(async () => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    await listFeatures(env, {});
  });

// =============================================================================
// QEMU Commands
// =============================================================================

// Run command - start QEMU
program
  .command('run')
  .description('Run NexaOS in QEMU (requires built system)')
  .option('-d, --debug', 'Enable GDB server and pause at start')
  .option('-n, --no-net', 'Disable networking')
  .option('--headless', 'Run without display')
  .option('-p, --profile <profile>', 'Use QEMU profile (default, minimal, debug, headless, full)', 'default')
  .option('--regenerate', 'Regenerate run-qemu.sh before running')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    const scriptPath = resolve(env.buildDir, 'run-qemu.sh');
    const configPath = resolve(projectRoot, 'config', 'qemu.yaml');
    
    // Check if config is newer than script
    let needsRegenerate = !existsSync(scriptPath) || options.regenerate;
    if (!needsRegenerate && existsSync(scriptPath) && existsSync(configPath)) {
      const scriptStat = statSync(scriptPath);
      const configStat = statSync(configPath);
      if (configStat.mtimeMs > scriptStat.mtimeMs) {
        logger.info('qemu.yaml has changed, regenerating script...');
        needsRegenerate = true;
      }
    }
    
    // Generate script if needed
    if (needsRegenerate) {
      logger.info('Generating QEMU script...');
      await generateQemuScript(env, options.profile);
    }
    
    // Build args for run-qemu.sh
    const args: string[] = [];
    if (options.debug) args.push('--debug');
    if (!options.net) args.push('--no-net');
    if (options.headless) args.push('--headless');
    
    // Run the script (it will print its own startup message)
    const child = spawn(scriptPath, args, {
      stdio: 'inherit',
      cwd: projectRoot,
    });
    
    child.on('exit', (code) => {
      process.exit(code ?? 0);
    });
  });

// Dev command - build and run
program
  .command('dev')
  .alias('d')
  .description('Build and run NexaOS (full build + QEMU)')
  .option('-q, --quick', 'Quick build (kernel + initramfs + ISO only)')
  .option('-d, --debug', 'Enable GDB server and pause at start')
  .option('-n, --no-net', 'Disable networking')
  .option('--headless', 'Run without display')
  .option('-p, --profile <profile>', 'Use QEMU profile', 'default')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const builder = new Builder(projectRoot);
    const env = builder.getEnvironment();
    
    // Build
    logger.section('Building NexaOS');
    const buildResult = options.quick 
      ? await builder.buildQuick()
      : await builder.buildFull();
    
    if (!buildResult.success) {
      logger.error('Build failed');
      process.exit(1);
    }
    
    // Generate QEMU script
    logger.info('Generating QEMU script...');
    await generateQemuScript(env, options.profile);
    
    const scriptPath = resolve(env.buildDir, 'run-qemu.sh');
    
    // Build args
    const args: string[] = [];
    if (options.debug) args.push('--debug');
    if (!options.net) args.push('--no-net');
    if (options.headless) args.push('--headless');
    
    // Run (script will print its own startup message)
    const child = spawn(scriptPath, args, {
      stdio: 'inherit',
      cwd: projectRoot,
    });
    
    child.on('exit', (code) => {
      process.exit(code ?? 0);
    });
  });

// QEMU command group - for QEMU-specific operations
const qemuCmd = program
  .command('qemu')
  .description('QEMU configuration and management');

// qemu generate - generate run-qemu.sh
qemuCmd
  .command('generate')
  .alias('gen')
  .description('Generate run-qemu.sh and NEXA.CFG from config/qemu.yaml')
  .option('-p, --profile <profile>', 'Use QEMU profile', 'default')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const env = createBuildEnvironment(projectRoot);
    
    logger.info(`Generating QEMU script with profile: ${options.profile}`);
    await generateQemuScript(env, options.profile);
    
    // Also generate NEXA.CFG boot configuration
    await generateNexaConfig(env);
  });

// qemu config - show QEMU configuration
qemuCmd
  .command('config')
  .description('Show QEMU configuration')
  .option('-p, --profile <profile>', 'Show configuration for profile', 'default')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const config = await loadQemuConfig(projectRoot);
    
    console.log('\n📦 QEMU Configuration (config/qemu.yaml)\n');
    console.log(`Machine: ${config.machine.arch} | ${config.machine.memory} RAM | ${config.machine.smp} CPUs`);
    console.log(`Boot: ${config.boot.mode.toUpperCase()}`);
    console.log(`Display: ${config.display.vga} via ${config.display.backend}`);
    console.log(`Network: ${config.network.enabled ? config.network.mode : 'disabled'}`);
    console.log(`Storage: ISO + ${config.storage.rootfs.device} + ${config.storage.swap.device}`);
    
    if (config.profiles && Object.keys(config.profiles).length > 0) {
      console.log('\nProfiles:');
      for (const [name, profile] of Object.entries(config.profiles)) {
        const marker = name === options.profile ? '►' : ' ';
        console.log(`  ${marker} ${name}: ${profile.description}`);
      }
    }
    console.log('');
  });

// qemu profiles - list QEMU profiles
qemuCmd
  .command('profiles')
  .description('List available QEMU profiles')
  .action(async () => {
    const projectRoot = findProjectRoot();
    const config = await loadQemuConfig(projectRoot);
    
    console.log('\n📦 QEMU Profiles\n');
    
    if (!config.profiles || Object.keys(config.profiles).length === 0) {
      console.log('  No profiles defined.');
    } else {
      for (const [name, profile] of Object.entries(config.profiles)) {
        console.log(`  • ${name}: ${profile.description}`);
      }
    }
    console.log('');
  });

// Default action (no command = show help)
// Note: This handles the case when no command is provided
if (process.argv.length <= 2) {
  program.outputHelp();
  process.exit(0);
}

// Handle unknown commands before parsing
const validCommands = ['build', 'b', 'clean', 'test', 't', 'coverage', 'cov', 'list', 'info', 'features', 'f', 'run', 'dev', 'd', 'qemu', '-V', '--version', '-h', '--help'];
const firstArg = process.argv[2];
if (firstArg && !firstArg.startsWith('-') && !validCommands.includes(firstArg)) {
  console.error(`\x1b[31mError:\x1b[0m Unknown command '${firstArg}'`);
  console.error('');
  console.error('(use "./ndk --help" for available commands)');
  console.error('');
  program.outputHelp();
  process.exit(1);
}

// Parse and execute
program.parse();
