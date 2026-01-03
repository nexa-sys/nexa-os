/**
 * NexaOS Build System - Initramfs Builder
 */

import { join } from 'path';
import { mkdir, copyFile, writeFile, chmod, symlink, unlink, readdir } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, cargoBuild, getFileSize, stripBinary, createEmptyArchive } from '../exec.js';
import { getStdRustFlags, getNrlibRustFlags, getPicRustFlags } from '../env.js';
import { buildAllModules } from './modules.js';

/**
 * Create init script
 */
async function createInitScript(initramfsDir: string): Promise<void> {
  logger.step('Creating init script...');
  
  const initScript = `#!/bin/sh
# Minimal initramfs init script
# Purpose: Mount proc/sys, detect root device, mount it, and pivot to real root

echo "[initramfs] Starting early userspace init..."

# Mount essential filesystems
mount -t proc none /proc 2>/dev/null || echo "[initramfs] proc already mounted"
mount -t sysfs none /sys 2>/dev/null || echo "[initramfs] sys already mounted"

echo "[initramfs] Early init complete, kernel will handle root mounting"
echo "[initramfs] If you see this, something went wrong - dropping to emergency shell"

# Drop to emergency shell if we get here
exec /bin/sh
`;
  
  const initPath = join(initramfsDir, 'init');
  await writeFile(initPath, initScript);
  await chmod(initPath, 0o755);
}

/**
 * Create README file
 */
async function createReadme(initramfsDir: string): Promise<void> {
  const readme = `NexaOS Initramfs
================

This is a minimal initial RAM filesystem designed for early boot.

Purpose:
- Provide emergency recovery shell
- Mount /proc and /sys  
- Load necessary drivers (future)
- Detect and prepare root device (future)
- Bridge to real root filesystem

Contents:
- /init - Early init script executed by kernel
- /bin/sh - Emergency shell for recovery
- /dev, /proc, /sys - Mount points for virtual filesystems
- /sysroot - Mount point for real root filesystem
- /lib/modules - Loadable kernel modules (.nkm)

Note: The actual root mounting is currently handled by the kernel's
boot_stages module. This initramfs serves as a safety net and
provides the foundation for future driver loading capabilities.
`;
  
  await writeFile(join(initramfsDir, 'README.txt'), readme);
}

/**
 * Build emergency shell for initramfs
 */
async function buildInitramfsShell(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.step('Building emergency shell for initramfs...');
  
  const startTime = Date.now();
  const initramfsBuild = join(env.buildDir, 'initramfs-build');
  const initramfsDir = join(env.buildDir, 'initramfs');
  
  await mkdir(initramfsBuild, { recursive: true });
  await mkdir(join(initramfsBuild, 'sysroot', 'lib'), { recursive: true });
  await mkdir(join(initramfsDir, 'bin'), { recursive: true });
  
  // Create Cargo.toml for initramfs build
  const cargoToml = `[package]
name = "initramfs-tools"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "sh"
path = "../../userspace/programs/user/shell/src/main.rs"

[dependencies]
libc = "0.2"

[profile.release]
panic = "abort"
opt-level = 2
lto = false
`;
  await writeFile(join(initramfsBuild, 'Cargo.toml'), cargoToml);
  
  // Build nrlib staticlib for shell
  const nrlibSrc = join(env.projectRoot, 'userspace/nrlib');
  
  const nrlibResult = await cargoBuild(env, {
    cwd: nrlibSrc,
    target: env.targets.userspace,
    release: true,
    buildStd: ['core'],
    rustflags: getNrlibRustFlags(),
    logName: 'initramfs-nrlib',
  });
  
  if (!nrlibResult.success) {
    logger.error('Failed to build nrlib for initramfs');
    return nrlibResult;
  }
  
  // Copy to sysroot
  const staticlib = join(env.projectRoot, 'userspace/target/x86_64-nexaos-userspace/release/libnrlib.a');
  await copyFile(staticlib, join(initramfsBuild, 'sysroot/lib/libc.a'));
  await createEmptyArchive(join(initramfsBuild, 'sysroot/lib/libunwind.a'));
  
  // Build shell with std
  const rustflags = getStdRustFlags(join(initramfsBuild, 'sysroot/lib'));
  
  const shellResult = await cargoBuild(env, {
    cwd: initramfsBuild,
    target: env.targets.userspace,
    release: true,
    buildStd: ['std', 'panic_abort'],
    rustflags,
    logName: 'initramfs-shell',
  });
  
  if (!shellResult.success) {
    logger.error('Failed to build emergency shell');
    return shellResult;
  }
  
  // Copy shell to initramfs
  const shellSrc = join(initramfsBuild, 'target/x86_64-nexaos-userspace/release/sh');
  const shellDst = join(initramfsDir, 'bin/sh');
  await copyFile(shellSrc, shellDst);
  await stripBinary(shellDst, true);
  
  const size = await getFileSize(shellDst);
  logger.success(`Emergency shell built: ${size}`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build libraries for initramfs
 */
async function buildInitramfsLibs(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.step('Building libraries for initramfs...');
  
  const startTime = Date.now();
  const initramfsDir = join(env.buildDir, 'initramfs');
  const lib64Dir = join(initramfsDir, 'lib64');
  
  await mkdir(lib64Dir, { recursive: true });
  
  // Build shared library
  const nrlibSrc = join(env.projectRoot, 'userspace/nrlib');
  
  const result = await cargoBuild(env, {
    cwd: nrlibSrc,
    target: env.targets.userspacePic,
    release: true,
    buildStd: ['core'],
    rustflags: getPicRustFlags(env.projectRoot),
    logName: 'initramfs-libnrlib',
  });
  
  if (!result.success) {
    logger.error('Failed to build libnrlib.so');
    return result;
  }
  
  const sharedlib = join(env.projectRoot, 'userspace/target/x86_64-nexaos-userspace-pic/release/libnrlib.so');
  const destPath = join(lib64Dir, 'libnrlib.so');
  
  await copyFile(sharedlib, destPath);
  await stripBinary(destPath, true);
  
  // Create symlinks
  const symlinks = ['libc.so', 'libc.so.6', 'libc.musl-x86_64.so.1'];
  for (const link of symlinks) {
    const linkPath = join(lib64Dir, link);
    try { await unlink(linkPath); } catch {}
    await symlink('libnrlib.so', linkPath);
  }
  
  logger.success('libnrlib.so installed');
  
  // Build dynamic linker
  const ldSrc = join(env.projectRoot, 'userspace/ld-nrlib');
  
  const ldFlags = [
    '-C opt-level=s',
    '-C panic=abort',
    '-C linker=rust-lld',
    '-C link-arg=--pie',
    '-C link-arg=-e_start',
    '-C link-arg=--no-dynamic-linker',
    '-C link-arg=-soname=ld-nrlib-x86_64.so.1',
  ].join(' ');
  
  const ldResult = await cargoBuild(env, {
    cwd: ldSrc,
    target: env.targets.ld,
    release: true,
    buildStd: ['core'],
    rustflags: ldFlags,
    logName: 'initramfs-ld-nrlib',
  });
  
  if (!ldResult.success) {
    logger.error('Failed to build dynamic linker');
    return ldResult;
  }
  
  const ldBin = join(env.projectRoot, 'userspace/target/x86_64-nexaos-ld/release/ld-nrlib');
  const ldDest = join(lib64Dir, 'ld-nrlib-x86_64.so.1');
  
  await copyFile(ldBin, ldDest);
  await stripBinary(ldDest, true);
  await chmod(ldDest, 0o755);
  
  // Create linker symlinks
  const ldSymlinks = ['ld-linux-x86-64.so.2', 'ld-musl-x86_64.so.1', 'ld-nexaos.so.1'];
  for (const link of ldSymlinks) {
    const linkPath = join(lib64Dir, link);
    try { await unlink(linkPath); } catch {}
    await symlink('ld-nrlib-x86_64.so.1', linkPath);
  }
  
  const ldSize = await getFileSize(ldDest);
  logger.success(`Dynamic linker installed (${ldSize})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Create CPIO archive
 */
async function createCpioArchive(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.step('Creating CPIO archive...');
  
  const startTime = Date.now();
  const initramfsDir = join(env.buildDir, 'initramfs');
  const stagingDir = join(initramfsDir, 'staging');
  
  // Clean staging directory
  await exec('rm', ['-rf', stagingDir]);
  
  // Create staging structure
  const stagingDirs = ['bin', 'dev', 'proc', 'sys', 'sysroot', 'lib64', 'lib/modules'];
  for (const dir of stagingDirs) {
    await mkdir(join(stagingDir, dir), { recursive: true });
  }
  
  // Copy essential files
  await copyFile(join(initramfsDir, 'init'), join(stagingDir, 'init'));
  await copyFile(join(initramfsDir, 'README.txt'), join(stagingDir, 'README.txt'));
  await copyFile(join(initramfsDir, 'bin/sh'), join(stagingDir, 'bin/sh'));
  
  // Copy libraries
  const lib64Src = join(initramfsDir, 'lib64');
  if (existsSync(lib64Src)) {
    const files = await readdir(lib64Src);
    for (const file of files) {
      const src = join(lib64Src, file);
      const dst = join(stagingDir, 'lib64', file);
      await copyFile(src, dst);
    }
  }
  
  // Copy kernel modules
  const modulesDir = join(env.buildDir, 'modules');
  if (existsSync(modulesDir)) {
    const nkmFiles = (await readdir(modulesDir)).filter(f => f.endsWith('.nkm'));
    for (const nkm of nkmFiles) {
      await copyFile(join(modulesDir, nkm), join(stagingDir, 'lib/modules', nkm));
      logger.info(`Added module: ${nkm}`);
    }
  }

  // Standard stream symlinks
  const stdioLinks: Array<[string, string]> = [
    ['stdin', '/proc/self/fd/0'],
    ['stdout', '/proc/self/fd/1'],
    ['stderr', '/proc/self/fd/2'],
  ];
  for (const [name, target] of stdioLinks) {
    const linkPath = join(stagingDir, 'dev', name);
    try { await unlink(linkPath); } catch {}
    await symlink(target, linkPath);
  }
  
  // Create CPIO archive
  const cpioPath = env.initramfsCpio;
  
  // Use find | cpio to create archive
  const findResult = await exec('sh', [
    '-c',
    `cd "${stagingDir}" && find . | cpio -o -H newc > "${cpioPath}"`,
  ]);
  
  if (findResult.exitCode !== 0) {
    logger.error('Failed to create CPIO archive');
    return { success: false, duration: Date.now() - startTime, error: findResult.stderr };
  }
  
  const size = await getFileSize(cpioPath);
  logger.success(`Initramfs created: ${cpioPath} (${size})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build complete initramfs
 */
export async function buildInitramfs(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building Initramfs');
  
  const startTime = Date.now();
  const initramfsDir = join(env.buildDir, 'initramfs');
  
  // Create directory structure
  const dirs = ['bin', 'dev', 'proc', 'sys', 'sysroot', 'lib64', 'lib/modules'];
  for (const dir of dirs) {
    await mkdir(join(initramfsDir, dir), { recursive: true });
  }
  
  // Create init script and readme
  await createInitScript(initramfsDir);
  await createReadme(initramfsDir);
  
  // Build shell
  const shellResult = await buildInitramfsShell(env);
  if (!shellResult.success) return shellResult;
  
  // Build libraries
  const libsResult = await buildInitramfsLibs(env);
  if (!libsResult.success) return libsResult;
  
  // Build kernel modules
  await buildAllModules(env);
  
  // Create CPIO archive
  const cpioResult = await createCpioArchive(env);
  
  logger.success('Initramfs build complete');
  
  return {
    success: cpioResult.success,
    duration: Date.now() - startTime,
  };
}
