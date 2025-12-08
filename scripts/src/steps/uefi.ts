/**
 * NexaOS Build System - UEFI Loader Builder
 */

import { join } from 'path';
import { mkdir, copyFile } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, getFileSize } from '../exec.js';

/**
 * Build UEFI loader
 */
export async function buildUefiLoader(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building UEFI Loader');
  
  const startTime = Date.now();
  
  const uefiLoaderDir = join(env.projectRoot, 'boot/uefi-loader');
  const outputPath = join(env.buildDir, 'BootX64.EFI');
  
  if (!existsSync(uefiLoaderDir)) {
    logger.warn('UEFI loader source not found, skipping');
    return { success: true, duration: Date.now() - startTime };
  }
  
  logger.step('Building UEFI loader...');
  
  // Build using cargo
  const result = await exec('cargo', [
    'build',
    '--release',
    '--target', 'x86_64-unknown-uefi',
  ], { cwd: uefiLoaderDir });
  
  if (result.exitCode !== 0) {
    logger.error('Failed to build UEFI loader');
    console.error(result.stderr);
    return { success: false, duration: Date.now() - startTime, error: result.stderr };
  }
  
  // Copy to build directory
  const builtLoader = join(uefiLoaderDir, 'target/x86_64-unknown-uefi/release/uefi-loader.efi');
  
  if (existsSync(builtLoader)) {
    await mkdir(env.buildDir, { recursive: true });
    await copyFile(builtLoader, outputPath);
    
    const size = await getFileSize(outputPath);
    logger.success(`UEFI loader built: ${outputPath} (${size})`);
  } else {
    logger.warn('UEFI loader binary not found after build');
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}
