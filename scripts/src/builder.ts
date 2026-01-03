/**
 * NexaOS Build System - Main Builder
 * Orchestrates the complete build process
 */

import { existsSync } from 'fs';
import { readFile } from 'fs/promises';
import { BuildEnvironment, BuildStepResult, BuildStep } from './types.js';
import { createBuildEnvironment, ensureBuildDirs } from './env.js';
import { logger } from './logger.js';
import { getFileSize, timedStep, initLogsDir, getFailedBuilds, clearFailedBuilds } from './exec.js';
import {
  buildKernel,
  buildAllNrlib,
  buildAllLibraries,
  buildAllPrograms,
  buildAllModules,
  buildRootfs,
  buildInitramfs,
  buildIso,
  buildUefiLoader,
  buildSwapImage,
  cleanAll,
  cleanBuild,
  buildNvm,
  cleanNvm,
} from './steps/index.js';

export class Builder {
  private env: BuildEnvironment;
  
  constructor(projectRoot: string) {
    this.env = createBuildEnvironment(projectRoot);
  }
  
  /**
   * Get build environment
   */
  getEnvironment(): BuildEnvironment {
    return this.env;
  }
  
  /**
   * Initialize build environment
   */
  async init(): Promise<void> {
    await ensureBuildDirs(this.env);
    await initLogsDir(this.env.projectRoot);
    clearFailedBuilds();
  }
  
  /**
   * Full build: everything
   */
  async buildFull(): Promise<BuildStepResult> {
    logger.section('Full NexaOS System Build');
    logger.resetTimer();
    
    const startTime = Date.now();
    
    await this.init();
    
    // Build kernel
    const kernelResult = await timedStep('Building kernel', () => buildKernel(this.env));
    if (!kernelResult.success) {
      await this.printFailedBuilds();
      return kernelResult;
    }
    
    // Build UEFI loader
    await timedStep('Building UEFI loader', () => buildUefiLoader(this.env));
    
    // Build kernel modules
    await timedStep('Building kernel modules', () => buildAllModules(this.env));
    
    // Build rootfs (includes nrlib, libs, programs)
    const rootfsResult = await timedStep('Building rootfs', () => buildRootfs(this.env));
    if (!rootfsResult.success) {
      await this.printFailedBuilds();
      return rootfsResult;
    }
    
    // Build swap image
    await timedStep('Building swap image', () => buildSwapImage(this.env));
    
    // Build initramfs
    await timedStep('Building initramfs', () => buildInitramfs(this.env));
    
    // Build ISO
    const isoResult = await timedStep('Building ISO', () => buildIso(this.env));
    if (!isoResult.success) {
      await this.printFailedBuilds();
      return isoResult;
    }
    
    await this.printSummary();
    
    return {
      success: true,
      duration: Date.now() - startTime,
    };
  }
  
  /**
   * Quick build: kernel + initramfs + ISO (no rootfs rebuild)
   */
  async buildQuick(): Promise<BuildStepResult> {
    logger.section('Quick NexaOS Build');
    logger.resetTimer();
    
    const startTime = Date.now();
    
    await this.init();
    
    const kernelResult = await timedStep('Building kernel', () => buildKernel(this.env));
    if (!kernelResult.success) {
      await this.printFailedBuilds();
      return kernelResult;
    }
    
    await timedStep('Building initramfs', () => buildInitramfs(this.env));
    
    const isoResult = await timedStep('Building ISO', () => buildIso(this.env));
    if (!isoResult.success) {
      await this.printFailedBuilds();
      return isoResult;
    }
    
    await this.printSummary();
    
    return {
      success: true,
      duration: Date.now() - startTime,
    };
  }
  
  /**
   * Build kernel only
   */
  async buildKernelOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building kernel', () => buildKernel(this.env));
  }
  
  /**
   * Build userspace only (nrlib + libs + programs)
   */
  async buildUserspaceOnly(): Promise<BuildStepResult> {
    logger.section('Building Userspace');
    logger.resetTimer();
    
    const startTime = Date.now();
    
    await this.init();
    
    await timedStep('Building nrlib', () => buildAllNrlib(this.env));
    await timedStep('Building libraries', () => buildAllLibraries(this.env));
    await timedStep('Building programs', () => buildAllPrograms(this.env));
    
    return {
      success: true,
      duration: Date.now() - startTime,
    };
  }
  
  /**
   * Build libraries only
   */
  async buildLibsOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building libraries', () => buildAllLibraries(this.env));
  }
  
  /**
   * Build modules only
   */
  async buildModulesOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building modules', () => buildAllModules(this.env));
  }
  
  /**
   * Build initramfs only
   */
  async buildInitramfsOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building initramfs', () => buildInitramfs(this.env));
  }
  
  /**
   * Build rootfs only
   */
  async buildRootfsOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building rootfs', () => buildRootfs(this.env));
  }
  
  /**
   * Build swap image only
   */
  async buildSwapOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building swap image', () => buildSwapImage(this.env));
  }
  
  /**
   * Build ISO only
   */
  async buildIsoOnly(): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building ISO', () => buildIso(this.env));
  }
  
  /**
   * Clean all
   */
  async clean(): Promise<BuildStepResult> {
    return cleanAll(this.env);
  }
  
  /**
   * Clean build only
   */
  async cleanBuildOnly(): Promise<BuildStepResult> {
    return cleanBuild(this.env);
  }
  
  /**
   * Build NVM hypervisor platform
   */
  async buildNvmOnly(options?: { features?: string; skipFrontend?: boolean }): Promise<BuildStepResult> {
    await this.init();
    return timedStep('Building NVM', () => buildNvm(this.env, options));
  }
  
  /**
   * Clean NVM build artifacts
   */
  async cleanNvmOnly(): Promise<BuildStepResult> {
    return cleanNvm(this.env);
  }
  
  /**
   * Run a build step
   */
  async run(step: BuildStep): Promise<BuildStepResult> {
    switch (step) {
      case 'full':
        return this.buildFull();
      case 'quick':
        return this.buildQuick();
      case 'kernel':
        return this.buildKernelOnly();
      case 'userspace':
        return this.buildUserspaceOnly();
      case 'libs':
        return this.buildLibsOnly();
      case 'modules':
        return this.buildModulesOnly();
      case 'initramfs':
        return this.buildInitramfsOnly();
      case 'rootfs':
        return this.buildRootfsOnly();
      case 'swap':
        return this.buildSwapOnly();
      case 'iso':
        return this.buildIsoOnly();
      case 'nvm':
        return this.buildNvmOnly();
      case 'clean':
        return this.clean();
      default:
        logger.error(`Unknown step: ${step}`);
        return { success: false, duration: 0, error: `Unknown step: ${step}` };
    }
  }
  
  /**
   * Run multiple build steps in sequence
   */
  async runSteps(steps: BuildStep[]): Promise<BuildStepResult> {
    const startTime = Date.now();
    
    for (const step of steps) {
      logger.info(`Running step: ${step}`);
      const result = await this.run(step);
      if (!result.success) {
        logger.error(`Step '${step}' failed`);
        await this.printFailedBuilds();
        return result;
      }
    }
    
    return {
      success: true,
      duration: Date.now() - startTime,
    };
  }
  
  /**
   * Print build summary
   */
  private async printSummary(): Promise<void> {
    const artifacts: { name: string; path: string; size?: string }[] = [];
    
    if (existsSync(this.env.kernelBin)) {
      artifacts.push({
        name: 'Kernel',
        path: this.env.kernelBin,
        size: await getFileSize(this.env.kernelBin),
      });
    }
    
    if (existsSync(this.env.initramfsCpio)) {
      artifacts.push({
        name: 'Initramfs',
        path: this.env.initramfsCpio,
        size: await getFileSize(this.env.initramfsCpio),
      });
    }
    
    if (existsSync(this.env.rootfsImg)) {
      artifacts.push({
        name: 'Root FS',
        path: this.env.rootfsImg,
        size: await getFileSize(this.env.rootfsImg),
      });
    }
    
    if (existsSync(this.env.swapImg)) {
      artifacts.push({
        name: 'Swap',
        path: this.env.swapImg,
        size: await getFileSize(this.env.swapImg),
      });
    }
    
    if (existsSync(this.env.isoFile)) {
      artifacts.push({
        name: 'ISO',
        path: this.env.isoFile,
        size: await getFileSize(this.env.isoFile),
      });
    }
    
    logger.summary(artifacts);
    
    // Print failed builds if any
    await this.printFailedBuilds();
  }
  
  /**
   * Print failed build logs
   */
  private async printFailedBuilds(): Promise<void> {
    const failedBuilds = getFailedBuilds();
    
    if (failedBuilds.length === 0) {
      return;
    }
    
    logger.section('Build Failures');
    logger.error(`${failedBuilds.length} build(s) failed`);
    
    for (const failed of failedBuilds) {
      logger.error(`\n${'='.repeat(80)}`);
      logger.error(`Failed: ${failed.name}`);
      logger.error(`Log file: ${failed.logPath}`);
      logger.error('='.repeat(80));
      
      try {
        const logContent = await readFile(failed.logPath, 'utf-8');
        // Print last 100 lines or full log if smaller
        const lines = logContent.split('\n');
        const displayLines = lines.slice(-100);
        
        if (lines.length > 100) {
          logger.error(`... (showing last 100 lines of ${lines.length}) ...\n`);
        }
        
        // Print with ANSI colors preserved
        console.error(displayLines.join('\n'));
      } catch (error) {
        logger.error(`Failed to read log file: ${error}`);
      }
    }
    
    logger.error(`\n${'='.repeat(80)}`);
    logger.error('Build logs saved in: logs/');
    logger.error('='.repeat(80));
  }
}
