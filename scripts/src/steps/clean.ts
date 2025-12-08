/**
 * NexaOS Build System - Clean
 */

import { rm } from 'fs/promises';
import { join } from 'path';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec } from '../exec.js';

/**
 * Clean all build artifacts
 */
export async function cleanAll(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Cleaning Build Artifacts');
  
  const startTime = Date.now();
  
  // Remove build directory
  logger.step('Removing build directory...');
  await rm(env.buildDir, { recursive: true, force: true });
  
  // Remove dist directory
  logger.step('Removing dist directory...');
  await rm(env.distDir, { recursive: true, force: true });
  
  // Run cargo clean in project root
  logger.step('Running cargo clean...');
  await exec('cargo', ['clean'], { cwd: env.projectRoot });
  
  // Clean userspace nrlib
  const nrlibDir = join(env.projectRoot, 'userspace/nrlib');
  if (existsSync(nrlibDir)) {
    await exec('cargo', ['clean'], { cwd: nrlibDir });
  }
  
  // Clean userspace ncryptolib
  const ncryptolibDir = join(env.projectRoot, 'userspace/ncryptolib');
  if (existsSync(ncryptolibDir)) {
    await exec('cargo', ['clean'], { cwd: ncryptolibDir });
  }
  
  // Clean modules
  const modulesDir = join(env.projectRoot, 'modules');
  if (existsSync(modulesDir)) {
    const { readdir } = await import('fs/promises');
    const modules = await readdir(modulesDir);
    
    for (const module of modules) {
      const moduleDir = join(modulesDir, module);
      const cargoToml = join(moduleDir, 'Cargo.toml');
      if (existsSync(cargoToml)) {
        await exec('cargo', ['clean'], { cwd: moduleDir });
      }
    }
  }
  
  logger.success('Clean complete');
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Clean build directory only (keep cargo cache)
 */
export async function cleanBuild(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.step('Cleaning build directory only...');
  
  const startTime = Date.now();
  
  await rm(env.buildDir, { recursive: true, force: true });
  await rm(env.distDir, { recursive: true, force: true });
  
  logger.success('Build artifacts cleaned');
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}
