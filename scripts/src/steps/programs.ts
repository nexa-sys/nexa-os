/**
 * NexaOS Build System - Userspace Programs Builder
 */

import { join } from 'path';
import { mkdir, copyFile, chmod } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult, ProgramConfig } from '../types.js';
import { logger } from '../logger.js';
import { cargoBuild, stripBinary, getFileSize, getStructuredLogPath } from '../exec.js';
import { getStdRustFlags, getDynRustFlags } from '../env.js';
import { loadBuildConfig, getAllPrograms, findProgram } from '../config.js';
import { buildAllNrlib } from './nrlib.js';

const USERSPACE_DIR = 'userspace';

/**
 * Build a single program
 */
async function buildProgram(
  env: BuildEnvironment,
  program: ProgramConfig
): Promise<BuildStepResult> {
  const linkType = program.link ?? 'dyn';
  logger.step(`Building ${program.package} (${linkType})...`);
  
  const startTime = Date.now();
  const userspaceDir = join(env.projectRoot, USERSPACE_DIR);
  
  // Select target and rustflags based on link type
  let target: string;
  let rustflags: string;
  
  if (linkType === 'dyn') {
    target = env.targets.userspaceDyn;
    rustflags = getDynRustFlags(join(env.sysrootPicDir, 'lib'));
  } else {
    target = env.targets.userspace;
    rustflags = getStdRustFlags(join(env.sysrootDir, 'lib'));
  }
  
  // Get structured log path using category
  // Cast to the internal type used by getStructuredLogPath
  const programCategory = program.category as Parameters<typeof getStructuredLogPath>[2];
  const logPath = programCategory 
    ? getStructuredLogPath('programs', program.package, programCategory)
    : getStructuredLogPath('programs', program.package, 'test');  // fallback to test
  
  const result = await cargoBuild(env, {
    cwd: userspaceDir,
    target,
    release: true,
    package: program.package,
    features: program.features,
    buildStd: ['std', 'panic_abort'],
    rustflags,
    logName: logPath ? undefined : `program-${program.package}`,  // Use old style if no structured path
    logPath,  // Use structured log path
  });
  
  if (!result.success) {
    logger.error(`Failed to build ${program.package}`);
    return result;
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Install a program to destination
 */
async function installProgram(
  env: BuildEnvironment,
  program: ProgramConfig,
  destDir: string
): Promise<BuildStepResult> {
  const linkType = program.link ?? 'dyn';
  const binary = program.binary ?? program.package;
  
  const targetName = linkType === 'dyn' 
    ? 'x86_64-nexaos-userspace-dynamic'
    : 'x86_64-nexaos-userspace';
  
  const src = join(env.projectRoot, USERSPACE_DIR, 'target', targetName, 'release', binary);
  const fullDest = join(destDir, program.dest);
  const dst = join(fullDest, binary);
  
  await mkdir(fullDest, { recursive: true });
  
  if (!existsSync(src)) {
    logger.error(`Failed to find ${binary} at ${src}`);
    return { success: false, duration: 0, error: `Binary not found: ${src}` };
  }
  
  await copyFile(src, dst);
  await stripBinary(dst, true);
  await chmod(dst, 0o755);
  
  const size = await getFileSize(dst);
  logger.success(`${binary} installed to /${program.dest} (${size})`);
  
  return { success: true, duration: 0 };
}

/**
 * Ensure nrlib is built
 */
async function ensureNrlib(env: BuildEnvironment): Promise<void> {
  const staticLib = join(env.sysrootDir, 'lib', 'libc.a');
  const picLib = join(env.sysrootPicDir, 'lib', 'libc.a');
  
  if (!existsSync(staticLib) || !existsSync(picLib)) {
    logger.info('Building nrlib first...');
    await buildAllNrlib(env);
  }
}

/**
 * Build and install all programs
 * Strategy: Group by (link_type, features) for batch building where possible,
 * fall back to individual builds for programs with custom features
 */
export async function buildAllPrograms(
  env: BuildEnvironment,
  destDir?: string
): Promise<BuildStepResult> {
  logger.section('Building Userspace Programs');
  
  const startTime = Date.now();
  const dest = destDir ?? join(env.buildDir, 'rootfs');
  const userspaceDir = join(env.projectRoot, USERSPACE_DIR);
  
  // Ensure nrlib is built first
  await ensureNrlib(env);
  
  const config = await loadBuildConfig(env.projectRoot);
  const programs = getAllPrograms(config);
  
  // Separate programs into batchable (no features) and individual (has features)
  const staticNoFeatures = programs.filter(p => (p.link ?? 'dyn') === 'std' && !p.features);
  const dynamicNoFeatures = programs.filter(p => (p.link ?? 'dyn') === 'dyn' && !p.features);
  const withFeatures = programs.filter(p => p.features);
  
  logger.info(`Building ${programs.length} programs: ${staticNoFeatures.length} static batch, ${dynamicNoFeatures.length} dynamic batch, ${withFeatures.length} with custom features`);
  
  const buildResults: Map<string, boolean> = new Map();
  
  // Phase 1: Batch build static programs without features
  if (staticNoFeatures.length > 0) {
    logger.step(`Building ${staticNoFeatures.length} static programs (batch)...`);
    const extraArgs = staticNoFeatures.flatMap(p => ['-p', p.package]);
    const result = await cargoBuild(env, {
      cwd: userspaceDir,
      target: env.targets.userspace,
      release: true,
      buildStd: ['std', 'panic_abort'],
      rustflags: getStdRustFlags(join(env.sysrootDir, 'lib')),
      extraArgs,
      logName: 'programs-static-batch',
    });
    for (const p of staticNoFeatures) buildResults.set(p.package, result.success);
    if (result.success) logger.success(`Built ${staticNoFeatures.length} static programs`);
  }
  
  // Phase 2: Batch build dynamic programs without features
  if (dynamicNoFeatures.length > 0) {
    logger.step(`Building ${dynamicNoFeatures.length} dynamic programs (batch)...`);
    const extraArgs = dynamicNoFeatures.flatMap(p => ['-p', p.package]);
    const result = await cargoBuild(env, {
      cwd: userspaceDir,
      target: env.targets.userspaceDyn,
      release: true,
      buildStd: ['std', 'panic_abort'],
      rustflags: getDynRustFlags(join(env.sysrootPicDir, 'lib')),
      extraArgs,
      logName: 'programs-dynamic-batch',
    });
    for (const p of dynamicNoFeatures) buildResults.set(p.package, result.success);
    if (result.success) logger.success(`Built ${dynamicNoFeatures.length} dynamic programs`);
  }
  
  // Phase 3: Build programs with custom features individually (in parallel)
  if (withFeatures.length > 0) {
    logger.step(`Building ${withFeatures.length} programs with custom features...`);
    const PARALLEL_LIMIT = Math.min(withFeatures.length, 4);
    
    for (let i = 0; i < withFeatures.length; i += PARALLEL_LIMIT) {
      const batch = withFeatures.slice(i, i + PARALLEL_LIMIT);
      const results = await Promise.all(
        batch.map(async (program) => {
          const result = await buildProgram(env, program);
          return { program, result };
        })
      );
      for (const { program, result } of results) {
        buildResults.set(program.package, result.success);
      }
    }
  }
  
  // Phase 4: Install successfully built programs
  let successCount = 0;
  let failCount = 0;
  
  for (const program of programs) {
    const buildSuccess = buildResults.get(program.package);
    if (buildSuccess) {
      const installResult = await installProgram(env, program, dest);
      if (installResult.success) {
        successCount++;
      } else {
        failCount++;
      }
    } else {
      failCount++;
    }
  }
  
  if (failCount > 0) {
    logger.warn(`Built ${successCount} programs, ${failCount} failed`);
  } else {
    logger.success(`All ${successCount} programs built successfully`);
  }
  
  return {
    success: failCount === 0,
    duration: Date.now() - startTime,
  };
}

/**
 * Build a single program by name
 */
export async function buildSingleProgram(
  env: BuildEnvironment,
  name: string,
  destDir?: string
): Promise<BuildStepResult> {
  const startTime = Date.now();
  const dest = destDir ?? join(env.buildDir, 'rootfs');
  
  // Ensure nrlib is built first
  await ensureNrlib(env);
  
  const config = await loadBuildConfig(env.projectRoot);
  const program = findProgram(config, name);
  
  if (!program) {
    logger.error(`Unknown program: ${name}`);
    return { success: false, duration: 0, error: `Unknown program: ${name}` };
  }
  
  const buildResult = await buildProgram(env, program);
  if (!buildResult.success) return buildResult;
  
  const installResult = await installProgram(env, program, dest);
  return {
    success: installResult.success,
    duration: Date.now() - startTime,
  };
}

/**
 * List all available programs
 */
export async function listPrograms(env: BuildEnvironment): Promise<void> {
  const config = await loadBuildConfig(env.projectRoot);
  const programs = getAllPrograms(config);
  
  logger.info('Available programs:');
  
  const rows = programs.map(p => [
    p.package,
    p.binary ?? p.package,
    `/${p.dest}`,
    p.link ?? 'dyn',
  ]);
  
  logger.table(['Package', 'Binary', 'Dest', 'Link'], rows);
}
