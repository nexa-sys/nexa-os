/**
 * NexaOS Build System - Main Builder
 * Orchestrates the complete build process
 */

import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult, BuildProfile } from './types.js';
import { createBuildEnvironment, ensureBuildDirs } from './env.js';
import { logger } from './logger.js';
import { getFileSize, timedStep } from './exec.js';
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
  cleanAll,
  cleanBuild,
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
    if (!kernelResult.success) return kernelResult;
    
    // Build UEFI loader
    await timedStep('Building UEFI loader', () => buildUefiLoader(this.env));
    
    // Build kernel modules
    await timedStep('Building kernel modules', () => buildAllModules(this.env));
    
    // Build rootfs (includes nrlib, libs, programs)
    const rootfsResult = await timedStep('Building rootfs', () => buildRootfs(this.env));
    if (!rootfsResult.success) return rootfsResult;
    
    // Build initramfs
    await timedStep('Building initramfs', () => buildInitramfs(this.env));
    
    // Build ISO
    const isoResult = await timedStep('Building ISO', () => buildIso(this.env));
    if (!isoResult.success) return isoResult;
    
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
    if (!kernelResult.success) return kernelResult;
    
    await timedStep('Building initramfs', () => buildInitramfs(this.env));
    
    const isoResult = await timedStep('Building ISO', () => buildIso(this.env));
    if (!isoResult.success) return isoResult;
    
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
   * Run a build profile
   */
  async run(profile: BuildProfile): Promise<BuildStepResult> {
    switch (profile) {
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
      case 'iso':
        return this.buildIsoOnly();
      case 'clean':
        return this.clean();
      default:
        logger.error(`Unknown profile: ${profile}`);
        return { success: false, duration: 0, error: `Unknown profile: ${profile}` };
    }
  }
  
  /**
   * Run multiple build steps in sequence
   */
  async runSteps(steps: BuildProfile[]): Promise<BuildStepResult> {
    const startTime = Date.now();
    
    for (const step of steps) {
      logger.info(`Running step: ${step}`);
      const result = await this.run(step);
      if (!result.success) {
        logger.error(`Step '${step}' failed`);
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
    
    if (existsSync(this.env.isoFile)) {
      artifacts.push({
        name: 'ISO',
        path: this.env.isoFile,
        size: await getFileSize(this.env.isoFile),
      });
    }
    
    logger.summary(artifacts);
  }
}
