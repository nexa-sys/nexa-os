/**
 * NexaOS Build System - Kernel Builder
 */

import { join } from 'path';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { cargoBuild, verifyMultiboot2, getFileSize, exec } from '../exec.js';
import { loadBuildConfig, getEnabledFeatureFlags } from '../config.js';

export async function buildKernel(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section(`Building NexaOS Kernel (${env.buildType})`);
  
  const startTime = Date.now();
  
  // Load configuration to get feature flags
  const config = await loadBuildConfig(env.projectRoot);
  const enabledFeatures = getEnabledFeatureFlags(config);
  
  // Build features string for cargo
  let featuresStr: string | undefined;
  if (enabledFeatures.length > 0) {
    // Convert cfg flags to cargo feature names (they match in our setup)
    featuresStr = enabledFeatures.join(',');
    logger.info(`Enabled features: ${featuresStr}`);
  }
  
  logger.step('Compiling kernel...');
  
  const result = await cargoBuild(env, {
    cwd: env.projectRoot,
    target: env.targets.kernel,
    release: env.buildType === 'release',
    buildStd: undefined, // Kernel uses custom build
    features: featuresStr,
  });
  
  if (!result.success) {
    logger.error('Kernel compilation failed');
    if (result.error) {
      console.error(result.error);
    }
    return result;
  }
  
  const kernelPath = env.kernelBin;
  const size = await getFileSize(kernelPath);
  logger.success(`Kernel built: ${kernelPath} (${size})`);
  
  // Verify multiboot2 header
  const multiboot2Valid = await verifyMultiboot2(kernelPath);
  if (multiboot2Valid) {
    logger.success('Multiboot2 header verified');
  } else {
    logger.warn('Multiboot2 header verification failed');
  }
  
  // Generate kernel symbols if objcopy available
  const symsPath = join(env.buildDir, 'kernel.syms');
  const objcopyResult = await exec('objcopy', ['--only-keep-debug', kernelPath, symsPath]);
  if (objcopyResult.exitCode === 0) {
    logger.info(`Symbols exported: ${symsPath}`);
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}
