/**
 * NexaOS Build System - nrlib Builder
 * Build the NexaOS runtime library (static and shared)
 */

import { join } from 'path';
import { mkdir, copyFile, symlink, unlink } from 'fs/promises';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { cargoBuild, createEmptyArchive, stripBinary, getFileSize, exec } from '../exec.js';
import { getNrlibRustFlags, getPicRustFlags, getLdRustFlags } from '../env.js';

const NRLIB_DIR = 'userspace/nrlib';
const LD_NRLIB_DIR = 'userspace/ld-nrlib';

/**
 * Build nrlib static library (libc.a)
 */
export async function buildNrlibStatic(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.step('Building nrlib staticlib (libc.a)...');
  
  const startTime = Date.now();
  const nrlibSrc = join(env.projectRoot, NRLIB_DIR);
  
  // Ensure directories exist
  await mkdir(join(env.sysrootDir, 'lib'), { recursive: true });
  await mkdir(join(env.sysrootPicDir, 'lib'), { recursive: true });
  
  // Build non-PIC version for static linking
  const result = await cargoBuild(env, {
    cwd: nrlibSrc,
    target: env.targets.userspace,
    release: true,
    buildStd: ['core'],
    rustflags: getNrlibRustFlags(),
  });
  
  if (!result.success) {
    logger.error('Failed to build nrlib staticlib');
    return result;
  }
  
  // Copy to sysroot
  const staticlib = join(env.projectRoot, 'userspace/target/x86_64-nexaos-userspace/release/libnrlib.a');
  const destPath = join(env.sysrootDir, 'lib', 'libc.a');
  
  await copyFile(staticlib, destPath);
  
  // Create empty libunwind.a and libgcc_s.a
  await createEmptyArchive(join(env.sysrootDir, 'lib', 'libunwind.a'));
  await createEmptyArchive(join(env.sysrootDir, 'lib', 'libgcc_s.a'));
  
  const size = await getFileSize(staticlib);
  logger.success(`libc.a installed to sysroot (${size})`);
  
  // Build PIC version for dynamic linking
  logger.step('Building nrlib staticlib with PIC for PIE executables...');
  
  const picResult = await cargoBuild(env, {
    cwd: nrlibSrc,
    target: env.targets.userspacePic,
    release: true,
    buildStd: ['core'],
    rustflags: '-C opt-level=2 -C panic=abort -C relocation-model=pic',
  });
  
  if (!picResult.success) {
    logger.error('Failed to build nrlib PIC staticlib');
    return picResult;
  }
  
  const staticlibPic = join(env.projectRoot, 'userspace/target/x86_64-nexaos-userspace-pic/release/libnrlib.a');
  await copyFile(staticlibPic, join(env.sysrootPicDir, 'lib', 'libc.a'));
  await createEmptyArchive(join(env.sysrootPicDir, 'lib', 'libunwind.a'));
  await createEmptyArchive(join(env.sysrootPicDir, 'lib', 'libgcc_s.a'));
  await copyFile(staticlibPic, join(env.sysrootDir, 'lib', 'libc_pic.a'));
  
  const picSize = await getFileSize(staticlibPic);
  logger.success(`PIC libc.a installed to sysroot-pic (${picSize})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build nrlib shared library (libnrlib.so)
 */
export async function buildNrlibShared(
  env: BuildEnvironment,
  destDir?: string
): Promise<BuildStepResult> {
  logger.step('Building nrlib shared library (libnrlib.so)...');
  
  const startTime = Date.now();
  const nrlibSrc = join(env.projectRoot, NRLIB_DIR);
  const dest = destDir ?? join(env.sysrootDir, 'lib');
  
  await mkdir(dest, { recursive: true });
  
  const result = await cargoBuild(env, {
    cwd: nrlibSrc,
    target: env.targets.userspacePic,
    release: true,
    buildStd: ['core'],
    rustflags: getPicRustFlags(),
  });
  
  if (!result.success) {
    logger.error('Failed to build nrlib shared library');
    return result;
  }
  
  const sharedlib = join(env.projectRoot, 'userspace/target/x86_64-nexaos-userspace-pic/release/libnrlib.so');
  const destPath = join(dest, 'libnrlib.so');
  
  await copyFile(sharedlib, destPath);
  await stripBinary(destPath, false);
  
  // Create compatibility symlinks
  const symlinks = ['libc.so', 'libc.so.6', 'libc.musl-x86_64.so.1'];
  for (const link of symlinks) {
    const linkPath = join(dest, link);
    try {
      await unlink(linkPath);
    } catch {
      // Ignore if doesn't exist
    }
    await symlink('libnrlib.so', linkPath);
  }
  
  const size = await getFileSize(destPath);
  logger.success(`libnrlib.so installed (${size})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build dynamic linker (ld-nrlib-x86_64.so.1)
 */
export async function buildDynamicLinker(
  env: BuildEnvironment,
  destDir?: string
): Promise<BuildStepResult> {
  logger.step('Building dynamic linker (ld-nrlib-x86_64.so.1)...');
  
  const startTime = Date.now();
  const ldSrc = join(env.projectRoot, LD_NRLIB_DIR);
  const dest = destDir ?? join(env.sysrootDir, 'lib');
  
  await mkdir(dest, { recursive: true });
  
  const result = await cargoBuild(env, {
    cwd: ldSrc,
    target: env.targets.ld,
    release: true,
    buildStd: ['core'],
    rustflags: getLdRustFlags(),
  });
  
  if (!result.success) {
    logger.error('Failed to build dynamic linker');
    return result;
  }
  
  const ldBin = join(env.projectRoot, 'userspace/target/x86_64-nexaos-ld/release/ld-nrlib');
  const destPath = join(dest, 'ld-nrlib-x86_64.so.1');
  
  await copyFile(ldBin, destPath);
  await stripBinary(destPath, true);
  await exec('chmod', ['+x', destPath]);
  
  // Create compatibility symlinks
  const symlinks = ['ld-musl-x86_64.so.1', 'ld-nexaos.so.1', 'ld-linux-x86-64.so.2'];
  for (const link of symlinks) {
    const linkPath = join(dest, link);
    try {
      await unlink(linkPath);
    } catch {
      // Ignore if doesn't exist
    }
    await symlink('ld-nrlib-x86_64.so.1', linkPath);
  }
  
  const size = await getFileSize(destPath);
  logger.success(`ld-nrlib-x86_64.so.1 installed (${size})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build all nrlib components
 */
export async function buildAllNrlib(
  env: BuildEnvironment,
  destDir?: string
): Promise<BuildStepResult> {
  logger.section('Building nrlib Components');
  
  const startTime = Date.now();
  
  const staticResult = await buildNrlibStatic(env);
  if (!staticResult.success) return staticResult;
  
  const sharedResult = await buildNrlibShared(env, destDir);
  if (!sharedResult.success) return sharedResult;
  
  const ldResult = await buildDynamicLinker(env, destDir);
  if (!ldResult.success) return ldResult;
  
  logger.success('All nrlib components built');
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}
