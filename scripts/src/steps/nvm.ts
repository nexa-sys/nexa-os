/**
 * NexaOS Build System - NVM (Hypervisor) Builder
 * 
 * Builds the NVM enterprise hypervisor platform components:
 * - nvm-server: Hypervisor daemon with WebGUI
 * - nvmctl: Management CLI
 * 
 * NVM is built separately from userspace programs because:
 * 1. It uses standard Rust targets (not NexaOS userspace targets)
 * 2. It requires building the Vue.js WebGUI frontend first
 * 3. It has complex feature flags for enterprise functionality
 */

import { join } from 'path';
import { mkdir, copyFile, chmod, rm, readdir } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, getFileSize } from '../exec.js';

const NVM_DIR = 'nvm';
const WEBUI_DIR = 'nvm/webui';

/**
 * Build NVM Vue.js frontend
 */
async function buildNvmFrontend(env: BuildEnvironment): Promise<BuildStepResult> {
  const webUiDir = join(env.projectRoot, WEBUI_DIR);
  const distDir = join(webUiDir, 'dist');
  
  // Check if webui exists
  if (!existsSync(webUiDir)) {
    logger.warn('NVM WebUI directory not found, skipping frontend build');
    return { success: true, duration: 0 };
  }
  
  // Check if node_modules exists
  const nodeModules = join(webUiDir, 'node_modules');
  if (!existsSync(nodeModules)) {
    logger.step('Installing NVM WebUI dependencies...');
    const installResult = await exec('npm', ['install'], {
      cwd: webUiDir,
    });
    
    if (installResult.exitCode !== 0) {
      logger.error('Failed to install NVM WebUI dependencies');
      logger.error(installResult.stderr);
      return { success: false, duration: 0, error: installResult.stderr };
    }
  }
  
  // Build frontend
  logger.step('Building NVM WebUI (Vue.js)...');
  const buildResult = await exec('npm', ['run', 'build'], {
    cwd: webUiDir,
  });
  
  if (buildResult.exitCode !== 0) {
    logger.error('Failed to build NVM WebUI');
    logger.error(buildResult.stderr);
    return { success: false, duration: 0, error: buildResult.stderr };
  }
  
  // Verify dist was created
  if (!existsSync(distDir)) {
    logger.error('NVM WebUI build did not produce dist/ directory');
    return { success: false, duration: 0, error: 'Missing dist directory' };
  }
  
  const files = await readdir(distDir, { recursive: true });
  logger.success(`NVM WebUI built: ${files.length} files`);
  
  return { success: true, duration: 0 };
}

/**
 * Build NVM Rust binaries (nvm-server and nvmctl)
 * Uses NexaOS userspace target with dynamic linking to nrlib
 */
async function buildNvmBinaries(
  env: BuildEnvironment,
  features?: string
): Promise<BuildStepResult> {
  const nvmDir = join(env.projectRoot, NVM_DIR);
  const startTime = Date.now();
  
  // Default features for full enterprise build
  const cargoFeatures = features ?? 'full';
  
  // Use NexaOS userspace dynamic target (links to nrlib)
  const target = env.targets.userspaceDyn;
  
  logger.step(`Building NVM binaries (features: ${cargoFeatures})...`);
  
  // Build with NexaOS userspace target
  const args = [
    'build',
    '-Z', 'build-std=std,panic_abort',
    '--release',
    '--target', target,
    '--features', cargoFeatures,
  ];
  
  // Get rustflags for dynamic linking
  const { getDynRustFlags } = await import('../env.js');
  const rustflags = getDynRustFlags(join(env.sysrootPicDir, 'lib'));
  
  const result = await exec('cargo', args, {
    cwd: nvmDir,
    env: {
      ...process.env,
      CARGO_TARGET_DIR: join(nvmDir, 'target'),
      RUSTFLAGS: rustflags,
    },
  });
  
  if (result.exitCode !== 0) {
    logger.error('Failed to build NVM binaries');
    logger.error(result.stderr);
    return { success: false, duration: Date.now() - startTime, error: result.stderr };
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

// NexaOS userspace dynamic target name
const NVM_TARGET = 'x86_64-nexaos-userspace-dynamic';

/**
 * Install NVM binaries to destination directory
 */
async function installNvmBinaries(
  env: BuildEnvironment,
  destDir: string
): Promise<BuildStepResult> {
  const nvmDir = join(env.projectRoot, NVM_DIR);
  const releaseDir = join(nvmDir, 'target', NVM_TARGET, 'release');
  
  const binaries = [
    { name: 'nvm-server', dest: 'sbin' },
    { name: 'nvmctl', dest: 'bin' },
  ];
  
  for (const bin of binaries) {
    const src = join(releaseDir, bin.name);
    const destPath = join(destDir, bin.dest);
    const dst = join(destPath, bin.name);
    
    if (!existsSync(src)) {
      logger.error(`NVM binary not found: ${src}`);
      return { success: false, duration: 0, error: `Binary not found: ${src}` };
    }
    
    await mkdir(destPath, { recursive: true });
    await copyFile(src, dst);
    await chmod(dst, 0o755);
    
    const size = await getFileSize(dst);
    logger.success(`${bin.name} installed to /${bin.dest} (${size})`);
  }
  
  return { success: true, duration: 0 };
}

/**
 * Build all NVM components (frontend + binaries)
 */
export async function buildNvm(
  env: BuildEnvironment,
  options?: {
    features?: string;
    skipFrontend?: boolean;
    destDir?: string;
  }
): Promise<BuildStepResult> {
  logger.section('Building NVM Hypervisor Platform');
  
  const startTime = Date.now();
  const nvmDir = join(env.projectRoot, NVM_DIR);
  
  // Check if NVM directory exists
  if (!existsSync(nvmDir)) {
    logger.error('NVM directory not found');
    return { success: false, duration: 0, error: 'NVM directory not found' };
  }
  
  // Step 1: Build frontend (unless skipped)
  if (!options?.skipFrontend) {
    const frontendResult = await buildNvmFrontend(env);
    if (!frontendResult.success) {
      return frontendResult;
    }
  }
  
  // Step 2: Build Rust binaries
  const binaryResult = await buildNvmBinaries(env, options?.features);
  if (!binaryResult.success) {
    return binaryResult;
  }
  
  // Step 3: Install to destination (if specified)
  if (options?.destDir) {
    const installResult = await installNvmBinaries(env, options.destDir);
    if (!installResult.success) {
      return installResult;
    }
  }
  
  logger.success('NVM build completed');
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build NVM for EXVM distribution (optimized for hypervisor use)
 */
export async function buildNvmForExvm(
  env: BuildEnvironment,
  destDir: string
): Promise<BuildStepResult> {
  return buildNvm(env, {
    features: 'full',
    destDir,
  });
}

/**
 * Build NVM for VCEN distribution (management focus)
 */
export async function buildNvmForVcen(
  env: BuildEnvironment,
  destDir: string
): Promise<BuildStepResult> {
  // VCEN uses the same features but different deployment
  return buildNvm(env, {
    features: 'full',
    destDir,
  });
}

/**
 * Clean NVM build artifacts
 */
export async function cleanNvm(env: BuildEnvironment): Promise<BuildStepResult> {
  const nvmDir = join(env.projectRoot, NVM_DIR);
  const targetDir = join(nvmDir, 'target');
  const webUiDist = join(env.projectRoot, WEBUI_DIR, 'dist');
  
  logger.step('Cleaning NVM build artifacts...');
  
  // Clean Cargo target
  if (existsSync(targetDir)) {
    await rm(targetDir, { recursive: true, force: true });
    logger.info('Cleaned nvm/target/');
  }
  
  // Clean WebUI dist
  if (existsSync(webUiDist)) {
    await rm(webUiDist, { recursive: true, force: true });
    logger.info('Cleaned nvm/webui/dist/');
  }
  
  return { success: true, duration: 0 };
}

/**
 * Get NVM version from Cargo.toml
 */
export async function getNvmVersion(env: BuildEnvironment): Promise<string> {
  const cargoToml = join(env.projectRoot, NVM_DIR, 'Cargo.toml');
  
  try {
    const { readFile } = await import('fs/promises');
    const content = await readFile(cargoToml, 'utf-8');
    const match = content.match(/version\s*=\s*"([^"]+)"/);
    return match ? match[1] : 'unknown';
  } catch {
    return 'unknown';
  }
}

/**
 * List NVM binaries that would be built
 */
export async function listNvmBinaries(): Promise<void> {
  console.log('\nNVM Hypervisor Platform binaries:');
  console.log('  nvm-server  - Hypervisor daemon with WebGUI (→ /sbin)');
  console.log('  nvmctl      - Management CLI (→ /bin)');
  console.log('\nFeatures: vtx, amdv, sdn, monitoring, backup, distributed-storage,');
  console.log('          multi-tenant, webgui, ha, templates, licensing, database');
}
