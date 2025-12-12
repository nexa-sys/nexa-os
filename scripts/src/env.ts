/**
 * NexaOS Build System - Build Environment
 * Manages build directories, paths, and environment variables
 */

import { join } from 'path';
import { mkdir } from 'fs/promises';
import { BuildEnvironment, BuildType, LogLevel } from './types.js';
import { getRootfsImgPath } from './qemu.js';

/**
 * Initialize build environment with all paths and settings
 */
export function createBuildEnvironment(projectRoot: string): BuildEnvironment {
  const buildType = (process.env.BUILD_TYPE as BuildType) ?? 'debug';
  const logLevel = (process.env.LOG_LEVEL as LogLevel) ?? 'debug';
  
  const buildDir = join(projectRoot, 'build');
  const distDir = join(projectRoot, 'dist');
  const targetDir = join(projectRoot, 'target');
  
  const kernelTargetDir = join(targetDir, 'x86_64-nexaos', buildType);
  const userspaceTargetDir = join(buildDir, 'userspace-build', 'target', 'x86_64-nexaos-userspace', 'release');
  
  const targetsDir = join(projectRoot, 'targets');
  
  // Get rootfs image path from qemu.yaml configuration
  const rootfsImg = getRootfsImgPath(projectRoot);
  
  return {
    projectRoot,
    scriptsDir: join(projectRoot, 'scripts'),
    stepsDir: join(projectRoot, 'scripts', 'steps'),
    buildDir,
    distDir,
    targetDir,
    
    buildType,
    logLevel,
    
    kernelTargetDir,
    userspaceTargetDir,
    
    targets: {
      kernel: join(targetsDir, 'x86_64-nexaos.json'),
      userspace: join(targetsDir, 'x86_64-nexaos-userspace.json'),
      userspacePic: join(targetsDir, 'x86_64-nexaos-userspace-pic.json'),
      userspaceDyn: join(targetsDir, 'x86_64-nexaos-userspace-dynamic.json'),
      ld: join(targetsDir, 'x86_64-nexaos-ld.json'),
      module: join(targetsDir, 'x86_64-nexaos-module.json'),
      lib: join(targetsDir, 'x86_64-nexaos-userspace-lib.json'),
    },
    
    kernelBin: join(kernelTargetDir, 'nexa-os'),
    initramfsCpio: join(buildDir, 'initramfs.cpio'),
    rootfsImg,
    swapImg: join(buildDir, 'swap.img'),
    isoFile: join(distDir, 'nexaos.iso'),
    
    sysrootDir: join(buildDir, 'userspace-build', 'sysroot'),
    sysrootPicDir: join(buildDir, 'userspace-build', 'sysroot-pic'),
  };
}

/**
 * Ensure all build directories exist
 */
export async function ensureBuildDirs(env: BuildEnvironment): Promise<void> {
  const dirs = [
    env.buildDir,
    env.distDir,
    join(env.buildDir, '.cache'),
    join(env.buildDir, '.locks'),
    join(env.buildDir, 'modules'),
    join(env.buildDir, 'initramfs'),
    join(env.buildDir, 'rootfs'),
    join(env.sysrootDir, 'lib'),
    join(env.sysrootPicDir, 'lib'),
  ];
  
  await Promise.all(dirs.map(dir => mkdir(dir, { recursive: true })));
}

/**
 * Get RUSTFLAGS for std userspace builds (static linking)
 */
export function getStdRustFlags(sysrootLib: string): string {
  const pthreadSymbols = [
    'pthread_mutexattr_settype',
    'pthread_mutexattr_init',
    'pthread_mutexattr_destroy',
    'pthread_mutex_init',
    'pthread_mutex_lock',
    'pthread_mutex_unlock',
    'pthread_mutex_destroy',
    'pthread_once',
    '__libc_single_threaded',
  ];
  
  const linkArgs = pthreadSymbols.map(s => `-C link-arg=-u${s}`).join(' ');
  
  return [
    '-C opt-level=2',
    '-C panic=abort',
    '-C linker=rust-lld',
    '-C link-arg=--image-base=0x01000000',
    '-C link-arg=--entry=_start',
    `-L ${sysrootLib}`,
    linkArgs,
  ].join(' ');
}

/**
 * Get RUSTFLAGS for dynamic linking (PIE executables)
 */
export function getDynRustFlags(sysrootPicLib: string): string {
  const undefinedSymbols = [
    '_start',
    'pthread_mutexattr_settype',
    'pthread_mutexattr_init', 
    'pthread_mutexattr_destroy',
    'pthread_mutex_init',
    'pthread_mutex_lock',
    'pthread_mutex_unlock',
    'pthread_mutex_destroy',
    'pthread_once',
    '__libc_single_threaded',
  ];
  
  const undefinedArgs = undefinedSymbols.map(s => `-C link-arg=--undefined=${s}`).join(' ');
  
  return [
    '-C opt-level=2',
    '-C panic=abort',
    '-C linker=rust-lld',
    '-C link-arg=--image-base=0x01000000',
    '-C link-arg=--entry=_start',
    `-L ${sysrootPicLib}`,
    '-C link-arg=-lc',
    undefinedArgs,
  ].join(' ');
}

/**
 * Get RUSTFLAGS for nrlib no-std build
 */
export function getNrlibRustFlags(): string {
  return '-C opt-level=2 -C panic=abort';
}

/**
 * Get RUSTFLAGS for PIC (shared libraries)
 */
export function getPicRustFlags(): string {
  return [
    '-C opt-level=2',
    '-C panic=abort',
    '-C relocation-model=pic',
    '-C link-arg=-u_start',
    '-C link-arg=-u_start_c',
    '-C link-arg=--export-dynamic',
  ].join(' ');
}

/**
 * Get RUSTFLAGS for dynamic linker
 */
export function getLdRustFlags(): string {
  return [
    '-C opt-level=s',
    '-C panic=abort',
    '-C linker=rust-lld',
    '-C link-arg=--pie',
    '-C link-arg=-e_start',
    '-C link-arg=--no-dynamic-linker',
    '-C link-arg=-soname=ld-nrlib-x86_64.so.1',
  ].join(' ');
}

/**
 * Get RUSTFLAGS for kernel modules
 */
export function getModuleRustFlags(): string {
  return '-C relocation-model=static -C code-model=kernel -C panic=abort';
}

/**
 * Export environment variables for child processes
 */
export function getExportedEnv(env: BuildEnvironment): Record<string, string> {
  return {
    PROJECT_ROOT: env.projectRoot,
    BUILD_DIR: env.buildDir,
    DIST_DIR: env.distDir,
    TARGET_DIR: env.targetDir,
    BUILD_TYPE: env.buildType,
    LOG_LEVEL: env.logLevel,
    KERNEL_TARGET_DIR: env.kernelTargetDir,
    USERSPACE_TARGET_DIR: env.userspaceTargetDir,
  };
}
