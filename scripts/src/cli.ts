#!/usr/bin/env node
/**
 * NexaOS Development Kit (NDK) - CLI
 * TypeScript-based build system and development tools
 */

import { Command } from 'commander';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { existsSync, statSync, writeFileSync, rmSync, mkdirSync } from 'fs';
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
import { 
  calculateCoverage, 
  CoverageStats, 
  TestResult
} from './coverage.js';
import { generateHtmlReport } from './html-report.js';
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
// Coverage Command - Test runner and report generation
// =============================================================================

interface TestRunResult {
  tests: TestResult[];
  warnings: number;
  errors: number;
  buildFailed: boolean;
  output: string;
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
      
      // Count warnings and errors (excluding test failure messages)
      const warningMatches = output.match(/warning:/g);
      // Only count compile errors, not "error: test failed"
      const errorMatches = output.match(/error\[E\d+\]/g);
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

// ANSI color codes for Jest-style output
const c = {
  reset: '\x1b[0m',
  bold: '\x1b[1m',
  dim: '\x1b[2m',
  green: '\x1b[32m',
  red: '\x1b[31m',
  yellow: '\x1b[33m',
  cyan: '\x1b[36m',
  white: '\x1b[37m',
  bgGreen: '\x1b[42m',
  bgRed: '\x1b[41m',
  bgYellow: '\x1b[43m',
};

// Check if colors should be used
const useColors = process.stdout.isTTY;
const color = (code: string, text: string) => useColors ? `${code}${text}${c.reset}` : text;

/** Format uncovered line numbers in compact ranges (e.g., "21,45-52,89") */
function formatUncoveredLines(lines: number[], maxLength: number = 25): string {
  if (lines.length === 0) return '';
  
  // Sort and dedupe
  const sorted = [...new Set(lines)].sort((a, b) => a - b);
  
  // Group into ranges
  const ranges: string[] = [];
  let start = sorted[0];
  let end = sorted[0];
  
  for (let i = 1; i < sorted.length; i++) {
    if (sorted[i] === end + 1) {
      end = sorted[i];
    } else {
      ranges.push(start === end ? String(start) : `${start}-${end}`);
      start = sorted[i];
      end = sorted[i];
    }
  }
  ranges.push(start === end ? String(start) : `${start}-${end}`);
  
  // Join and truncate if too long
  let result = ranges.join(',');
  if (result.length > maxLength) {
    result = result.slice(0, maxLength - 3) + '...';
  }
  
  return result;
}

// Generate text report (Jest-style with file details)
function generateTextReport(stats: CoverageStats, testResults: TestResult[], runResult: TestRunResult, verbose: boolean = false): string {
  const lines: string[] = [];
  
  // Header
  lines.push('');
  
  // Test Suites summary (Jest style)
  const failedTests = testResults.filter(t => !t.passed);
  const passedTests = testResults.filter(t => t.passed);
  
  if (failedTests.length > 0) {
    lines.push(color(c.bold + c.red, ` FAIL `) + color(c.dim, ' Tests'));
  } else {
    lines.push(color(c.bold + c.green, ` PASS `) + color(c.dim, ' Tests'));
  }
  lines.push('');
  
  // Build warnings/errors (compact)
  if (runResult.warnings > 0 || runResult.errors > 0) {
    const parts: string[] = [];
    if (runResult.errors > 0) {
      parts.push(color(c.red, `${runResult.errors} errors`));
    }
    if (runResult.warnings > 0) {
      parts.push(color(c.yellow, `${runResult.warnings} warnings`));
    }
    lines.push(color(c.dim, '  Build: ') + parts.join(', '));
    lines.push('');
  }
  
  // Test summary line (Jest style)
  lines.push(color(c.bold, 'Tests:  ') + 
    (failedTests.length > 0 ? color(c.red, `${failedTests.length} failed`) + ', ' : '') +
    color(c.green, `${passedTests.length} passed`) + 
    color(c.dim, `, ${stats.totalTests} total`));
  
  // Time (estimate based on test count)
  const estimatedTime = (stats.totalTests * 0.001).toFixed(2);
  lines.push(color(c.bold, 'Time:   ') + color(c.dim, `${estimatedTime}s`));
  lines.push('');
  
  // Coverage summary table - Jest style with % Stmts, % Branch, % Funcs, % Lines, Uncovered Lines
  const sortedModules = Object.entries(stats.modules).sort((a, b) => a[0].localeCompare(b[0]));
  
  // Calculate max file path length for proper column width
  let maxPathLen = 20;
  for (const [moduleName, m] of sortedModules) {
    maxPathLen = Math.max(maxPathLen, moduleName.length + 2);
    if (verbose && m.files) {
      for (const f of m.files) {
        maxPathLen = Math.max(maxPathLen, f.filePath.length + 4);
      }
    }
  }
  const pathCol = Math.min(maxPathLen + 1, 45); // Cap at 45 chars
  
  // Build header
  const sepLine = '-'.repeat(pathCol) + '|----------|----------|----------|----------|-------------------|';
  const headerLine = 'File'.padEnd(pathCol) + '| % Stmts  | % Branch | % Funcs  | % Lines  | Uncovered Line #s |';
  
  lines.push(color(c.bold + c.white, sepLine));
  lines.push(color(c.bold + c.white, headerLine));
  lines.push(color(c.bold + c.white, sepLine));
  
  // All modules and files
  for (const [moduleName, m] of sortedModules) {
    const sPct = m.totalStatements > 0 ? (m.coveredStatements / m.totalStatements * 100) : 0;
    const bPct = m.totalBranches > 0 ? (m.coveredBranches / m.totalBranches * 100) : 0;
    const fPct = m.totalFunctions > 0 ? (m.coveredFunctions / m.totalFunctions * 100) : 0;
    const lPct = m.totalLines > 0 ? (m.coveredLines / m.totalLines * 100) : 0;
    
    const sColor = sPct >= 70 ? c.green : sPct >= 40 ? c.yellow : c.red;
    const bColor = bPct >= 70 ? c.green : bPct >= 40 ? c.yellow : c.red;
    const fColor = fPct >= 70 ? c.green : fPct >= 40 ? c.yellow : c.red;
    const lColor = lPct >= 70 ? c.green : lPct >= 40 ? c.yellow : c.red;
    
    // Module row (bold)
    const modPath = moduleName.padEnd(pathCol - 1);
    lines.push(
      color(c.bold, ` ${modPath}`) +
      color(sColor, `|${sPct.toFixed(2).padStart(8)}% `) +
      color(bColor, `|${bPct.toFixed(2).padStart(8)}% `) +
      color(fColor, `|${fPct.toFixed(2).padStart(8)}% `) +
      color(lColor, `|${lPct.toFixed(2).padStart(8)}% `) +
      `|${' '.repeat(18)}|`
    );
    
    // File details in verbose mode
    if (verbose && m.files) {
      // Sort files by coverage (lowest first) to highlight problem areas
      const sortedFiles = [...m.files].sort((a, b) => {
        const aCov = a.totalFunctions > 0 ? a.coveredFunctions / a.totalFunctions : 0;
        const bCov = b.totalFunctions > 0 ? b.coveredFunctions / b.totalFunctions : 0;
        return aCov - bCov;
      });
      
      for (const f of sortedFiles) {
        const fsPct = f.totalStatements > 0 ? (f.coveredStatements / f.totalStatements * 100) : 0;
        const fbPct = f.totalBranches > 0 ? (f.coveredBranches / f.totalBranches * 100) : 0;
        const ffPct = f.totalFunctions > 0 ? (f.coveredFunctions / f.totalFunctions * 100) : 0;
        const flPct = f.totalLines > 0 ? (f.coveredLines / f.totalLines * 100) : 0;
        
        const fsColor = fsPct >= 70 ? c.green : fsPct >= 40 ? c.yellow : c.red;
        const fbColor = fbPct >= 70 ? c.green : fbPct >= 40 ? c.yellow : c.red;
        const ffColor = ffPct >= 70 ? c.green : ffPct >= 40 ? c.yellow : c.red;
        const flColor = flPct >= 70 ? c.green : flPct >= 40 ? c.yellow : c.red;
        
        // Truncate file path if needed
        let filePath = '  ' + f.filePath;
        if (filePath.length > pathCol - 1) {
          filePath = '  ...' + f.filePath.slice(-(pathCol - 6));
        }
        
        // Format uncovered lines
        const uncovStr = formatUncoveredLines(f.uncoveredLineNumbers, 17);
        
        lines.push(
          color(c.dim, filePath.padEnd(pathCol - 1)) +
          color(fsColor, `|${fsPct.toFixed(2).padStart(8)}% `) +
          color(fbColor, `|${fbPct.toFixed(2).padStart(8)}% `) +
          color(ffColor, `|${ffPct.toFixed(2).padStart(8)}% `) +
          color(flColor, `|${flPct.toFixed(2).padStart(8)}% `) +
          `|${uncovStr.padEnd(18)}|`
        );
      }
    }
  }
  
  // Totals row
  lines.push(color(c.bold + c.white, sepLine));
  const totalSPct = stats.statementCoveragePct;
  const totalBPct = stats.branchCoveragePct;
  const totalFPct = stats.functionCoveragePct;
  const totalLPct = stats.lineCoveragePct;
  
  const totalSColor = totalSPct >= 70 ? c.green : totalSPct >= 40 ? c.yellow : c.red;
  const totalBColor = totalBPct >= 70 ? c.green : totalBPct >= 40 ? c.yellow : c.red;
  const totalFColor = totalFPct >= 70 ? c.green : totalFPct >= 40 ? c.yellow : c.red;
  const totalLColor = totalLPct >= 70 ? c.green : totalLPct >= 40 ? c.yellow : c.red;
  
  lines.push(
    color(c.bold, ` ${'All files'.padEnd(pathCol - 1)}`) +
    color(c.bold + totalSColor, `|${totalSPct.toFixed(2).padStart(8)}% `) +
    color(c.bold + totalBColor, `|${totalBPct.toFixed(2).padStart(8)}% `) +
    color(c.bold + totalFColor, `|${totalFPct.toFixed(2).padStart(8)}% `) +
    color(c.bold + totalLColor, `|${totalLPct.toFixed(2).padStart(8)}% `) +
    `|${' '.repeat(18)}|`
  );
  lines.push(color(c.bold + c.white, sepLine));
  lines.push('');
  
  // Failed tests (always show if any)
  if (failedTests.length > 0) {
    lines.push(color(c.red + c.bold, '● Failed Tests'));
    lines.push('');
    for (const test of failedTests) {
      lines.push(color(c.red, `  ✕ `) + color(c.dim, test.name));
    }
    lines.push('');
  }
  
  // Verbose: show all test results
  if (verbose) {
    lines.push(color(c.dim, '● All Tests'));
    lines.push('');
    for (const test of testResults.sort((a, b) => a.name.localeCompare(b.name))) {
      if (test.passed) {
        lines.push(color(c.green, '  ✓ ') + color(c.dim, test.name));
      } else {
        lines.push(color(c.red, '  ✕ ') + test.name);
      }
    }
    lines.push('');
  }
  
  return lines.join('\n');
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
      statementCoveragePct: stats.statementCoveragePct,
      branchCoveragePct: stats.branchCoveragePct,
      functionCoveragePct: stats.functionCoveragePct,
      lineCoveragePct: stats.lineCoveragePct,
      totalStatements: stats.totalStatements,
      coveredStatements: stats.coveredStatements,
      totalBranches: stats.totalBranches,
      coveredBranches: stats.coveredBranches,
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
        defaultOutput = 'reports/coverage.html';
        break;
      case 'json':
        report = generateJsonReport(stats, runResult.tests);
        defaultOutput = 'reports/coverage.json';
        break;
      default:
        report = generateTextReport(stats, runResult.tests, runResult, options.verbose);
        break;
    }
    
    if (options.output) {
      const outDir = dirname(options.output);
      if (!existsSync(outDir)) {
        mkdirSync(outDir, { recursive: true });
      }
      writeFileSync(options.output, report);
      logger.success(`Report saved to: ${options.output}`);
    } else if (defaultOutput && options.format !== 'text') {
      const outPath = resolve(projectRoot, defaultOutput);
      const outDir = dirname(outPath);
      if (!existsSync(outDir)) {
        mkdirSync(outDir, { recursive: true });
      }
      writeFileSync(outPath, report);
      logger.success(`Report saved to: ${outPath}`);
    } else {
      console.log(report);
    }
    
    if (options.open && options.format === 'html') {
      const outFile = options.output || resolve(projectRoot, 'reports/coverage.html');
      spawn('xdg-open', [outFile], { detached: true, stdio: 'ignore' }).unref();
    }
    
    logger.success('Coverage analysis complete!');
    process.exit(0);
  });

// coverage html - shortcut for HTML report
coverageCmd
  .command('html')
  .description('Generate HTML coverage report and open in browser')
  .option('-o, --output <path>', 'Output file', 'reports/coverage.html')
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
    const outDir = dirname(outPath);
    if (!existsSync(outDir)) {
      mkdirSync(outDir, { recursive: true });
    }
    writeFileSync(outPath, report);
    logger.success(`HTML report generated at: ${outPath}`);
    
    spawn('xdg-open', [outPath], { detached: true, stdio: 'ignore' }).unref();
    process.exit(0);
  });

// coverage json - generate JSON report
coverageCmd
  .command('json')
  .description('Generate JSON coverage report')
  .option('-o, --output <path>', 'Output file', 'reports/coverage.json')
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
    const outDir = dirname(outPath);
    if (!existsSync(outDir)) {
      mkdirSync(outDir, { recursive: true });
    }
    writeFileSync(outPath, report);
    logger.success(`JSON report generated: ${outPath}`);
    process.exit(0);
  });

// coverage clean - clean coverage data
coverageCmd
  .command('clean')
  .description('Clean coverage data and reports')
  .action(async () => {
    const projectRoot = findProjectRoot();
    
    logger.step('Cleaning coverage data...');
    
    // Clean old files in root (backward compatibility)
    const filesToRemove = ['coverage.html', 'coverage.json', 'lcov.info'];
    for (const file of filesToRemove) {
      const filePath = resolve(projectRoot, file);
      if (existsSync(filePath)) {
        rmSync(filePath);
        logger.info(`Removed: ${file}`);
      }
    }
    
    // Clean coverage directory
    const coverageDir = resolve(projectRoot, 'coverage');
    if (existsSync(coverageDir)) {
      rmSync(coverageDir, { recursive: true });
      logger.info('Removed: coverage/');
    }
    
    // Clean reports directory
    const reportsDir = resolve(projectRoot, 'reports');
    if (existsSync(reportsDir)) {
      const reportFiles = ['coverage.html', 'coverage.json'];
      for (const file of reportFiles) {
        const filePath = resolve(reportsDir, file);
        if (existsSync(filePath)) {
          rmSync(filePath);
          logger.info(`Removed: reports/${file}`);
        }
      }
    }
    
    logger.success('Coverage data cleaned');
    process.exit(0);
  });

// Default coverage action (show text summary)
coverageCmd
  .option('-v, --verbose', 'Show detailed file-level coverage')
  .option('-m, --module <name>', 'Show coverage for specific module only')
  .option('--show-uncovered', 'Show uncovered line numbers in output')
  .option('--threshold <pct>', 'Exit with error if coverage is below threshold', parseFloat)
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
    
    // Filter to specific module if requested
    if (options.module) {
      const modName = options.module;
      if (!stats.modules[modName]) {
        logger.error(`Module not found: ${modName}`);
        logger.info(`Available modules: ${Object.keys(stats.modules).join(', ')}`);
        process.exit(1);
      }
      
      // Create filtered stats
      const filteredStats: CoverageStats = {
        ...stats,
        modules: { [modName]: stats.modules[modName] }
      };
      
      const report = generateTextReport(filteredStats, runResult.tests, runResult, true); // Always verbose for single module
      console.log(report);
    } else {
      const report = generateTextReport(stats, runResult.tests, runResult, options.verbose);
      console.log(report);
    }
    
    // Check threshold
    if (options.threshold !== undefined) {
      const avgCoverage = (stats.statementCoveragePct + stats.branchCoveragePct + 
                          stats.functionCoveragePct + stats.lineCoveragePct) / 4;
      if (avgCoverage < options.threshold) {
        logger.error(`Coverage ${avgCoverage.toFixed(2)}% is below threshold ${options.threshold}%`);
        process.exit(1);
      }
    }
    
    process.exit(0);
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
