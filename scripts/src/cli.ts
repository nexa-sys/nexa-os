#!/usr/bin/env node
/**
 * NexaOS Build System - CLI
 * TypeScript rewrite of the build system
 */

import { Command } from 'commander';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { existsSync } from 'fs';
import { Builder } from './builder.js';
import { logger } from './logger.js';
import { BuildStep } from './types.js';
import { loadBuildConfig } from './config.js';
import { buildSingleProgram, listPrograms } from './steps/programs.js';
import { buildSingleModule, listModules } from './steps/modules.js';
import { buildLibrary, listLibraries } from './steps/libs.js';
import { createBuildEnvironment } from './env.js';

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
  .name('nexaos-build')
  .description('NexaOS TypeScript Build System')
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

// Multi-step build
program
  .command('run <steps...>')
  .description('Run multiple build steps in sequence')
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

// Default action (no command = full build)
program
  .action(async () => {
    const builder = new Builder(findProjectRoot());
    const result = await builder.buildFull();
    process.exit(result.success ? 0 : 1);
  });

// Parse and execute
program.parse();
