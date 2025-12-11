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
 * Build and install all programs (parallel build, sequential install)
 */
export async function buildAllPrograms(
  env: BuildEnvironment,
  destDir?: string
): Promise<BuildStepResult> {
  logger.section('Building Userspace Programs');
  
  const startTime = Date.now();
  const dest = destDir ?? join(env.buildDir, 'rootfs');
  
  // Ensure nrlib is built first
  await ensureNrlib(env);
  
  const config = await loadBuildConfig(env.projectRoot);
  const programs = getAllPrograms(config);
  
  // Parallel compile concurrency limit
  const PARALLEL_LIMIT = Math.min(programs.length, 6);
  
  logger.info(`Building ${programs.length} programs in parallel (max ${PARALLEL_LIMIT} concurrent)...`);
  
  // Phase 1: Build all programs in parallel batches
  const buildResults: Map<string, boolean> = new Map();
  
  for (let i = 0; i < programs.length; i += PARALLEL_LIMIT) {
    const batch = programs.slice(i, i + PARALLEL_LIMIT);
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
  
  // Phase 2: Install successfully built programs (sequential to avoid file conflicts)
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
