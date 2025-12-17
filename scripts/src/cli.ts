#!/usr/bin/env node
/**
 * NexaOS Development Kit (NDK) - CLI
 * TypeScript-based build system and development tools
 */

import { Command } from 'commander';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { existsSync, statSync } from 'fs';
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
// Coverage Command
// =============================================================================

// Helper to get test environment with isolated target directory
// This prevents config.toml inheritance issues with build-std
function getTestEnv(projectRoot: string): NodeJS.ProcessEnv {
  const targetDir = resolve(projectRoot, 'build', 'tests-target');
  return {
    ...process.env,
    CARGO_TARGET_DIR: targetDir,
  };
}

// Ensure cargo-llvm-cov is installed
async function ensureLlvmCovInstalled(testDir: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const checkChild = spawn('cargo', ['llvm-cov', '--version'], {
      cwd: testDir,
      stdio: 'pipe',
    });
    
    checkChild.on('error', () => {
      // Command not found, install it
      installLlvmCov().then(resolve).catch(reject);
    });
    
    checkChild.on('close', (code) => {
      if (code !== 0) {
        logger.warn('cargo-llvm-cov not found. Installing...');
        installLlvmCov().then(resolve).catch(reject);
      } else {
        resolve();
      }
    });
  });
}

async function installLlvmCov(): Promise<void> {
  return new Promise((resolve, reject) => {
    const installChild = spawn('cargo', ['install', 'cargo-llvm-cov'], {
      stdio: 'inherit',
    });
    
    installChild.on('close', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`Installation failed with code ${code}`));
    });
    installChild.on('error', reject);
  });
}

const coverageCmd = program
  .command('coverage')
  .alias('cov')
  .description('Run tests with code coverage analysis (requires cargo-llvm-cov)');

// coverage run - run tests with coverage
coverageCmd
  .command('run')
  .description('Run tests and generate coverage report')
  .option('-f, --format <format>', 'Output format: text, html, lcov, json', 'text')
  .option('-o, --output <path>', 'Output path for coverage report')
  .option('--open', 'Open HTML report in browser (html format only)')
  .option('--filter <pattern>', 'Run only tests matching pattern')
  .option('--fail-under <threshold>', 'Fail if coverage is below threshold (0-100)', '0')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    // Ensure cargo-llvm-cov is installed
    try {
      await ensureLlvmCovInstalled(testDir);
    } catch (err: any) {
      logger.error(`Failed to install cargo-llvm-cov: ${err.message}`);
      process.exit(1);
    }
    
    // Build llvm-cov command
    const args = ['llvm-cov'];
    
    // Output format
    switch (options.format) {
      case 'html':
        args.push('--html');
        break;
      case 'lcov':
        args.push('--lcov');
        break;
      case 'json':
        args.push('--json');
        break;
      // 'text' is default
    }
    
    // Output path
    if (options.output) {
      args.push('--output-path', options.output);
    }
    
    // Open in browser
    if (options.open && options.format === 'html') {
      args.push('--open');
    }
    
    // Fail threshold
    if (options.failUnder && parseFloat(options.failUnder) > 0) {
      args.push('--fail-under-lines', options.failUnder);
    }
    
    // Filter
    if (options.filter) {
      args.push('--', options.filter);
    }
    
    logger.step('Running tests with coverage analysis...');
    logger.info(`Format: ${options.format}`);
    
    const covChild = spawn('cargo', args, {
      cwd: testDir,
      stdio: 'inherit',
      env: getTestEnv(projectRoot)
    });
    
    covChild.on('close', (code) => {
      if (code === 0) {
        logger.success('Coverage analysis complete!');
        if (options.output) {
          logger.info(`Report saved to: ${options.output}`);
        }
      } else {
        logger.error(`Coverage analysis failed with exit code ${code}`);
      }
      process.exit(code || 0);
    });
    
    covChild.on('error', (err) => {
      logger.error(`Failed to run coverage: ${err.message}`);
      process.exit(1);
    });
  });

// coverage html - shortcut for HTML report
coverageCmd
  .command('html')
  .description('Generate HTML coverage report and open in browser')
  .option('-o, --output <path>', 'Output directory', 'coverage')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    const outputDir = resolve(projectRoot, options.output);
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    // Ensure cargo-llvm-cov is installed
    try {
      await ensureLlvmCovInstalled(testDir);
    } catch (err: any) {
      logger.error(`Failed to install cargo-llvm-cov: ${err.message}`);
      process.exit(1);
    }
    
    logger.step('Generating HTML coverage report...');
    
    const args = ['llvm-cov', '--html', '--output-dir', outputDir, '--open'];
    
    const child = spawn('cargo', args, {
      cwd: testDir,
      stdio: 'inherit',
      env: getTestEnv(projectRoot)
    });
    
    child.on('close', (code) => {
      if (code === 0) {
        logger.success(`HTML report generated at: ${outputDir}`);
      } else {
        logger.error(`Failed to generate report (exit code ${code})`);
        logger.info('Make sure cargo-llvm-cov is installed: cargo install cargo-llvm-cov');
      }
      process.exit(code || 0);
    });
    
    child.on('error', (err) => {
      logger.error(`Failed to run coverage: ${err.message}`);
      process.exit(1);
    });
  });

// coverage lcov - generate lcov report for CI integration
coverageCmd
  .command('lcov')
  .description('Generate LCOV report (for CI integration)')
  .option('-o, --output <path>', 'Output file', 'lcov.info')
  .action(async (options) => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    const outputFile = resolve(projectRoot, options.output);
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    // Ensure cargo-llvm-cov is installed
    try {
      await ensureLlvmCovInstalled(testDir);
    } catch (err: any) {
      logger.error(`Failed to install cargo-llvm-cov: ${err.message}`);
      process.exit(1);
    }
    
    logger.step('Generating LCOV coverage report...');
    
    const args = ['llvm-cov', '--lcov', '--output-path', outputFile];
    
    const child = spawn('cargo', args, {
      cwd: testDir,
      stdio: 'inherit',
      env: getTestEnv(projectRoot)
    });
    
    child.on('close', (code) => {
      if (code === 0) {
        logger.success(`LCOV report generated: ${outputFile}`);
      } else {
        logger.error(`Failed to generate report (exit code ${code})`);
      }
      process.exit(code || 0);
    });
    
    child.on('error', (err) => {
      logger.error(`Failed to run coverage: ${err.message}`);
      process.exit(1);
    });
  });

// coverage summary - show coverage summary
coverageCmd
  .command('summary')
  .alias('sum')
  .description('Show coverage summary (text output)')
  .action(async () => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    
    if (!existsSync(testDir)) {
      logger.error('Tests directory not found. Expected: tests/');
      process.exit(1);
    }
    
    // Ensure cargo-llvm-cov is installed
    try {
      await ensureLlvmCovInstalled(testDir);
    } catch (err: any) {
      logger.error(`Failed to install cargo-llvm-cov: ${err.message}`);
      process.exit(1);
    }
    
    logger.step('Analyzing test coverage...');
    
    const child = spawn('cargo', ['llvm-cov'], {
      cwd: testDir,
      stdio: 'inherit',
      env: getTestEnv(projectRoot)
    });
    
    child.on('close', (code) => {
      process.exit(code || 0);
    });
    
    child.on('error', (err) => {
      logger.error(`Failed to run coverage: ${err.message}`);
      process.exit(1);
    });
  });

// coverage clean - clean coverage data
coverageCmd
  .command('clean')
  .description('Clean coverage data and reports')
  .action(async () => {
    const projectRoot = findProjectRoot();
    const testDir = resolve(projectRoot, 'tests');
    const coverageDir = resolve(projectRoot, 'coverage');
    
    logger.step('Cleaning coverage data...');
    
    // Clean cargo-llvm-cov data
    const child = spawn('cargo', ['llvm-cov', 'clean'], {
      cwd: testDir,
      stdio: 'inherit',
      env: getTestEnv(projectRoot)
    });
    
    child.on('close', (code) => {
      // Also remove coverage directory if it exists
      if (existsSync(coverageDir)) {
        const rmChild = spawn('rm', ['-rf', coverageDir], {
          stdio: 'inherit',
        });
        rmChild.on('close', () => {
          logger.success('Coverage data cleaned');
          process.exit(0);
        });
      } else {
        logger.success('Coverage data cleaned');
        process.exit(code || 0);
      }
    });
  });

// Default coverage action (show summary)
coverageCmd.action(async () => {
  const projectRoot = findProjectRoot();
  const testDir = resolve(projectRoot, 'tests');
  
  if (!existsSync(testDir)) {
    logger.error('Tests directory not found. Expected: tests/');
    process.exit(1);
  }
  
  // Ensure cargo-llvm-cov is installed
  try {
    await ensureLlvmCovInstalled(testDir);
  } catch (err: any) {
    logger.error(`Failed to install cargo-llvm-cov: ${err.message}`);
    process.exit(1);
  }
  
  logger.step('Analyzing test coverage...');
  
  const child = spawn('cargo', ['llvm-cov'], {
    cwd: testDir,
    stdio: 'inherit',
    env: getTestEnv(projectRoot)
  });
  
  child.on('close', (code) => {
    process.exit(code || 0);
  });
  
  child.on('error', (err) => {
    logger.error(`Failed to run coverage: ${err.message}`);
    logger.info('Make sure cargo-llvm-cov is installed: cargo install cargo-llvm-cov');
    process.exit(1);
  });
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
