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
  .version('1.0.0');

// Full build
program
  .command('full')
  .alias('all')
  .description('Full system build (kernel, userspace, rootfs, initramfs, ISO)')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildFull();
    process.exit(result.success ? 0 : 1);
  });

// Quick build
program
  .command('quick')
  .alias('q')
  .description('Quick build (kernel + initramfs + ISO, no rootfs rebuild)')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildQuick();
    process.exit(result.success ? 0 : 1);
  });

// Kernel
program
  .command('kernel')
  .alias('k')
  .description('Build kernel only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildKernelOnly();
    process.exit(result.success ? 0 : 1);
  });

// Userspace
program
  .command('userspace')
  .alias('u')
  .description('Build userspace programs only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildUserspaceOnly();
    process.exit(result.success ? 0 : 1);
  });

// Libraries
program
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

// Modules
program
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

// Programs
program
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

// Initramfs
program
  .command('initramfs')
  .alias('i')
  .description('Build initramfs only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildInitramfsOnly();
    process.exit(result.success ? 0 : 1);
  });

// Rootfs
program
  .command('rootfs')
  .alias('r')
  .description('Build root filesystem only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildRootfsOnly();
    process.exit(result.success ? 0 : 1);
  });

// Swap
program
  .command('swap')
  .description('Build swap image only')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildSwapOnly();
    process.exit(result.success ? 0 : 1);
  });

// ISO
program
  .command('iso')
  .description('Build ISO only (requires existing kernel)')
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildIsoOnly();
    process.exit(result.success ? 0 : 1);
  });

// Clean
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

// Multi-step build (renamed from 'run' to 'steps' to avoid conflict with QEMU run)
program
  .command('steps <steps...>')
  .alias('s')
  .description('Run multiple build steps in sequence (e.g., ndk steps kernel iso)')
  .action(async (steps: string[]) => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.runSteps(steps as BuildStep[]);
    process.exit(result.success ? 0 : 1);
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

// Default action (no command = full build)
program
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildFull();
    process.exit(result.success ? 0 : 1);
  });

// Parse and execute
program.parse();
