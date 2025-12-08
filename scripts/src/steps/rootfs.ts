/**
 * NexaOS Build System - Root Filesystem Builder
 */

import { join } from 'path';
import { mkdir, copyFile, writeFile, chmod, readdir } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, execInherit, getFileSize } from '../exec.js';
import { buildAllNrlib } from './nrlib.js';
import { buildAllPrograms } from './programs.js';
import { buildLibrary } from './libs.js';
import { loadBuildConfig } from '../config.js';

const ROOTFS_SIZE_MB = parseInt(process.env.ROOTFS_SIZE_MB ?? '50', 10);

/**
 * Setup rootfs directory structure
 */
async function setupRootfsDirs(rootfsDir: string): Promise<void> {
  logger.step('Creating rootfs directory structure...');
  
  const dirs = [
    'bin', 'sbin', 'etc/ni', 'etc/fonts/conf.d', 'dev', 'proc', 'sys',
    'tmp', 'var', 'home', 'root', 'lib64', 'usr/share/fonts/truetype',
    'var/cache/fontconfig',
  ];
  
  await Promise.all(dirs.map(d => mkdir(join(rootfsDir, d), { recursive: true })));
}

/**
 * Install configuration files
 */
async function installConfigs(env: BuildEnvironment, rootfsDir: string): Promise<void> {
  logger.step('Installing configuration files...');
  
  // Copy ni config
  const niConf = join(env.projectRoot, 'etc/ni/ni.conf');
  if (existsSync(niConf)) {
    await copyFile(niConf, join(rootfsDir, 'etc/ni/ni.conf'));
  }
  
  // Copy inittab
  const inittab = join(env.projectRoot, 'etc/inittab');
  if (existsSync(inittab)) {
    await copyFile(inittab, join(rootfsDir, 'etc/inittab'));
  }
  
  // Copy font config
  const fontsDir = join(env.projectRoot, 'etc/fonts');
  if (existsSync(fontsDir)) {
    logger.info('Installing font configuration...');
    const fontsConf = join(fontsDir, 'fonts.conf');
    if (existsSync(fontsConf)) {
      await copyFile(fontsConf, join(rootfsDir, 'etc/fonts/fonts.conf'));
    }
    
    const confD = join(fontsDir, 'conf.d');
    if (existsSync(confD)) {
      const files = await readdir(confD);
      for (const file of files.filter(f => f.endsWith('.conf'))) {
        await copyFile(join(confD, file), join(rootfsDir, 'etc/fonts/conf.d', file));
      }
    }
  }
  
  // Create motd
  const motd = `Welcome to NexaOS!

You are now running from the real root filesystem (ext2).
This system was mounted via pivot_root from initramfs.

`;
  await writeFile(join(rootfsDir, 'etc/motd'), motd);
  
  // Create fallback init script
  const initScript = `#!/bin/sh
# Simple init fallback
exec /sbin/ni
`;
  const initPath = join(rootfsDir, 'sbin/init');
  await writeFile(initPath, initScript);
  await chmod(initPath, 0o755);
  
  logger.success('Configuration files installed');
}

/**
 * Install CA certificates
 */
async function installCaCerts(env: BuildEnvironment, rootfsDir: string): Promise<void> {
  logger.step('Installing CA certificates...');
  
  const installScript = join(env.stepsDir, 'install-ca-certs.sh');
  if (existsSync(installScript)) {
    await execInherit('bash', [installScript, 'all']);
    await execInherit('bash', [installScript, 'rootfs', rootfsDir]);
  }
}

/**
 * Install libraries to rootfs
 */
async function installLibs(env: BuildEnvironment, rootfsDir: string): Promise<void> {
  logger.step('Installing libraries to rootfs...');
  
  const lib64Dir = join(rootfsDir, 'lib64');
  await mkdir(lib64Dir, { recursive: true });
  
  // Build and install nrlib
  await buildAllNrlib(env, lib64Dir);
  
  // Build and install other libraries
  const config = await loadBuildConfig(env.projectRoot);
  const libNames = ['ncryptolib', 'nssl', 'nzip', 'nhttp2'];
  
  for (const libName of libNames) {
    await buildLibrary(env, config, libName, { type: 'shared', destDir: lib64Dir });
  }
}

/**
 * Create ext2 filesystem image
 */
async function createExt2Image(env: BuildEnvironment, rootfsDir: string): Promise<BuildStepResult> {
  logger.step(`Creating ext2 filesystem image (${ROOTFS_SIZE_MB}MB)...`);
  
  const startTime = Date.now();
  const imgPath = env.rootfsImg;
  
  // Show rootfs contents
  logger.info('Rootfs directory contents:');
  await execInherit('ls', ['-lah', rootfsDir]);
  
  // Create image file
  logger.info('Creating disk image...');
  await exec('dd', ['if=/dev/zero', `of=${imgPath}`, 'bs=1M', `count=${ROOTFS_SIZE_MB}`, 'status=progress']);
  
  // Format as ext2
  logger.info('Formatting as ext2...');
  await exec('mkfs.ext2', ['-F', '-L', 'nexaos-root', imgPath]);
  
  // Mount and copy files
  logger.info('Copying files to ext2 filesystem...');
  
  const mountResult = await exec('mktemp', ['-d']);
  const mountPoint = mountResult.stdout.trim();
  
  await exec('sudo', ['mount', '-o', 'loop', imgPath, mountPoint]);
  
  // Copy all files
  await exec('sudo', ['cp', '-a', rootfsDir + '/', mountPoint + '/']);
  
  // Set permissions
  await exec('sudo', ['chmod', '755', join(mountPoint, 'bin')]);
  await exec('sudo', ['chmod', '755', join(mountPoint, 'sbin')]);
  await exec('sudo', ['chmod', '1777', join(mountPoint, 'tmp')]);
  
  // Unmount
  await exec('sudo', ['umount', mountPoint]);
  await exec('rmdir', [mountPoint]);
  
  const size = await getFileSize(imgPath);
  logger.success(`Root filesystem created: ${imgPath} (${size})`);
  
  // Verify
  const fileResult = await exec('file', [imgPath]);
  if (fileResult.stdout.includes('ext2')) {
    logger.success('Valid ext2 filesystem');
  } else {
    logger.warn('May not be a valid ext2 filesystem');
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build the complete root filesystem
 */
export async function buildRootfs(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building ext2 Root Filesystem');
  
  const startTime = Date.now();
  const rootfsDir = join(env.buildDir, 'rootfs');
  
  await setupRootfsDirs(rootfsDir);
  await installLibs(env, rootfsDir);
  await buildAllPrograms(env, rootfsDir);
  await installConfigs(env, rootfsDir);
  await installCaCerts(env, rootfsDir);
  
  const result = await createExt2Image(env, rootfsDir);
  
  logger.success('Rootfs build complete');
  
  return {
    success: result.success,
    duration: Date.now() - startTime,
  };
}
