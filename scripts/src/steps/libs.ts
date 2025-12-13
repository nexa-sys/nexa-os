/**
 * NexaOS Build System - Libraries Builder
 * Build userspace libraries (libcrypto, libssl, etc.)
 * 
 * Libraries are auto-discovered from userspace/lib/ directory.
 * Each library must have a Cargo.toml with [package.metadata.nexaos] section.
 */

import { join } from 'path';
import { mkdir, copyFile, symlink, unlink } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult, LibraryConfig, BuildConfig } from '../types.js';
import { logger } from '../logger.js';
import { cargoBuild, stripBinary, getFileSize } from '../exec.js';
import { loadBuildConfig, getAllLibraries, findLibrary, getLibraryBuildOrder } from '../config.js';

const USERSPACE_DIR = 'userspace';

interface LibraryBuildOptions {
  type: 'static' | 'shared' | 'all';
  destDir?: string;
}

/**
 * Get the source path for a library
 */
function getLibrarySourcePath(env: BuildEnvironment, lib: LibraryConfig): string {
  // Libraries are in userspace/lib/<name>/
  return join(env.projectRoot, USERSPACE_DIR, lib.path);
}

/**
 * Build a static library
 */
async function buildLibraryStatic(
  env: BuildEnvironment,
  lib: LibraryConfig
): Promise<BuildStepResult> {
  const staticName = `lib${lib.output}.a`;
  logger.step(`Building ${lib.name} staticlib (${staticName})...`);
  
  const startTime = Date.now();
  const libSrc = getLibrarySourcePath(env, lib);
  const sysrootLib = join(env.sysrootDir, 'lib');
  
  await mkdir(sysrootLib, { recursive: true });
  
  // Use sysroot-pic/lib for building (cargo builds ALL crate-types)
  const sysrootPicLib = join(env.sysrootPicDir, 'lib');
  
  // Build Cargo features string from config
  const features = lib.features 
    ? Object.entries(lib.features).filter(([_, v]) => v).map(([k]) => k).join(',')
    : undefined;
  
  // Include both sysroot-pic/lib and sysroot/lib for library search paths
  const result = await cargoBuild(env, {
    cwd: libSrc,
    target: env.targets.lib,
    release: true,
    buildStd: ['std', 'core', 'alloc', 'panic_abort'],
    rustflags: `-C opt-level=2 -C panic=abort -L ${sysrootPicLib} -L ${sysrootLib}`,
    logName: `library-${lib.name}-static`,
    features,
  });
  
  if (!result.success) {
    logger.error(`Failed to build ${lib.name} staticlib`);
    return result;
  }
  
  const staticlib = join(env.projectRoot, USERSPACE_DIR, 'target/x86_64-nexaos-userspace-lib/release', `lib${lib.name}.a`);
  const destPath = join(sysrootLib, staticName);
  
  await copyFile(staticlib, destPath);
  
  const size = await getFileSize(staticlib);
  logger.success(`${staticName} installed (${size})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build a shared library
 */
async function buildLibraryShared(
  env: BuildEnvironment,
  lib: LibraryConfig,
  _config: BuildConfig,
  destDir?: string
): Promise<BuildStepResult> {
  const sharedName = `lib${lib.output}.so`;
  logger.step(`Building ${lib.name} shared library (${sharedName})...`);
  
  const startTime = Date.now();
  const libSrc = getLibrarySourcePath(env, lib);
  const dest = destDir ?? join(env.sysrootDir, 'lib');
  
  await mkdir(dest, { recursive: true });
  
  // Use separate target dir to avoid cache conflicts
  const sharedTargetDir = join(env.projectRoot, USERSPACE_DIR, 'target-shared');
  const sysrootPicLib = join(env.sysrootPicDir, 'lib');
  const sysrootLib = join(env.sysrootDir, 'lib');
  
  // Build Cargo features string from config
  const features = lib.features 
    ? Object.entries(lib.features).filter(([_, v]) => v).map(([k]) => k).join(',')
    : undefined;
  
  // Include both sysroot-pic/lib and sysroot/lib for library search paths
  // sysroot/lib contains the built .so files that dependent libraries need
  const result = await cargoBuild(env, {
    cwd: libSrc,
    target: env.targets.lib,
    release: true,
    buildStd: ['std', 'core', 'alloc', 'panic_abort'],
    rustflags: `-C opt-level=2 -C panic=abort -C relocation-model=pic -L ${sysrootPicLib} -L ${sysrootLib}`,
    targetDir: sharedTargetDir,
    logName: `library-${lib.name}-shared`,
    features,
  });
  
  if (!result.success) {
    logger.error(`Failed to build ${lib.name} shared library`);
    return result;
  }
  
  const sharedlib = join(sharedTargetDir, 'x86_64-nexaos-userspace-lib/release', `lib${lib.name}.so`);
  const destPath = join(dest, sharedName);
  
  await copyFile(sharedlib, destPath);
  await stripBinary(destPath, false);
  
  // Create version symlinks in destination
  if (lib.version) {
    const versionedName = `${sharedName}.${lib.version}`;
    const fullVersionName = `${sharedName}.${lib.version}.0.0`;
    
    for (const name of [versionedName, fullVersionName]) {
      const linkPath = join(dest, name);
      try { await unlink(linkPath); } catch {}
      await symlink(sharedName, linkPath);
    }
  }
  
  // Create Cargo package name symlinks for dynamic linking
  // e.g., libncryptolib.so -> libcrypto.so, libnssl.so -> libssl.so
  if (lib.name !== lib.output) {
    const cargoName = `lib${lib.name}.so`;
    const cargoLinkPath = join(dest, cargoName);
    try { await unlink(cargoLinkPath); } catch {}
    await symlink(sharedName, cargoLinkPath);
    logger.info(`Created symlink: ${cargoName} -> ${sharedName}`);
  }
  
  // Always create symlinks in sysroot-pic/lib for dependent library builds
  // This allows libraries like nssl to find libcrypto.so when linking
  // Symlinks point to rootfs/lib64 where actual .so files are installed
  await mkdir(sysrootPicLib, { recursive: true });
  
  // Create symlink for the output name (e.g., libcrypto.so)
  const picOutputLink = join(sysrootPicLib, sharedName);
  try { await unlink(picOutputLink); } catch {}
  await symlink(join('..', '..', 'rootfs', 'lib64', sharedName), picOutputLink);
  
  // Also create cargo package name symlink if different
  if (lib.name !== lib.output) {
    const cargoName = `lib${lib.name}.so`;
    const picCargoLink = join(sysrootPicLib, cargoName);
    try { await unlink(picCargoLink); } catch {}
    await symlink(sharedName, picCargoLink);
  }
  
  const size = await getFileSize(destPath);
  logger.success(`${sharedName} installed (${size})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Check and build library dependencies
 */
async function ensureDependencies(
  env: BuildEnvironment,
  config: BuildConfig,
  lib: LibraryConfig
): Promise<void> {
  if (!lib.depends || lib.depends.length === 0) return;
  
  for (const depName of lib.depends) {
    const depLib = findLibrary(config, depName);
    if (!depLib) {
      logger.warn(`Unknown dependency: ${depName}`);
      continue;
    }
    
    const depPath = join(env.sysrootDir, 'lib', `lib${depLib.output}.so`);
    if (!existsSync(depPath)) {
      logger.info(`Building dependency ${depName} first...`);
      await buildLibrary(env, config, depName, { type: 'all' });
    }
  }
}

/**
 * Build a single library
 */
export async function buildLibrary(
  env: BuildEnvironment,
  config: BuildConfig,
  name: string,
  options: LibraryBuildOptions = { type: 'all' }
): Promise<BuildStepResult> {
  const lib = findLibrary(config, name);
  if (!lib) {
    logger.error(`Unknown library: ${name}`);
    return { success: false, duration: 0, error: `Unknown library: ${name}` };
  }
  
  logger.section(`Building library: ${name}`);
  
  const startTime = Date.now();
  
  // Build dependencies first
  await ensureDependencies(env, config, lib);
  
  if (options.type === 'static' || options.type === 'all') {
    const result = await buildLibraryStatic(env, lib);
    if (!result.success) return result;
  }
  
  if (options.type === 'shared' || options.type === 'all') {
    const result = await buildLibraryShared(env, lib, config, options.destDir);
    if (!result.success) return result;
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build all libraries in correct order
 */
export async function buildAllLibraries(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building All NexaOS Libraries');
  
  const startTime = Date.now();
  const config = await loadBuildConfig(env.projectRoot);
  const buildOrder = getLibraryBuildOrder(config);
  
  let successCount = 0;
  let failCount = 0;
  
  for (const libName of buildOrder) {
    const result = await buildLibrary(env, config, libName, { type: 'all' });
    if (result.success) {
      successCount++;
    } else {
      failCount++;
    }
  }
  
  if (failCount > 0) {
    logger.warn(`Built ${successCount} libraries, ${failCount} failed`);
  } else {
    logger.success(`All ${successCount} libraries built successfully`);
  }
  
  return {
    success: failCount === 0,
    duration: Date.now() - startTime,
  };
}

/**
 * List all available libraries
 */
export async function listLibraries(env: BuildEnvironment): Promise<void> {
  const config = await loadBuildConfig(env.projectRoot);
  const libraries = getAllLibraries(config);
  
  logger.info('Available libraries (auto-discovered from userspace/lib/):');
  logger.info('');
  
  const rows = libraries.map(lib => [
    lib.name,
    `lib${lib.output}.so.${lib.version}`,
    lib.path,
    lib.depends.length > 0 ? lib.depends.join(', ') : '-',
    lib.enabled ? '✓' : '✗',
  ]);
  
  logger.table(['Package', 'Output', 'Path', 'Dependencies', 'Enabled'], rows);
  
  logger.info('');
  logger.info('Build order (topologically sorted):');
  logger.info(`  ${config.build_order.libraries.join(' → ')}`);
}
