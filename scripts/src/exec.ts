/**
 * NexaOS Build System - Command Executor
 * Wraps execa for consistent command execution with logging
 */

import { execa, ExecaError } from 'execa';
import { existsSync } from 'fs';
import { stat } from 'fs/promises';
import { logger } from './logger.js';
import { BuildEnvironment, ExecOptions, BuildStepResult } from './types.js';
import { getExportedEnv } from './env.js';

/**
 * Execute a command with full output
 */
export async function exec(
  command: string,
  args: string[],
  options: ExecOptions = {}
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  try {
    const result = await execa(command, args, {
      cwd: options.cwd,
      env: { ...process.env, ...options.env },
      stdio: options.stdio ?? 'pipe',
      timeout: options.timeout,
      reject: false,
    });
    
    return {
      stdout: String(result.stdout ?? ''),
      stderr: String(result.stderr ?? ''),
      exitCode: result.exitCode ?? 0,
    };
  } catch (error) {
    const execaError = error as ExecaError;
    return {
      stdout: String(execaError.stdout ?? ''),
      stderr: String(execaError.stderr ?? execaError.message),
      exitCode: execaError.exitCode ?? 1,
    };
  }
}

/**
 * Execute a command with inherited stdio (shows output in real-time)
 */
export async function execInherit(
  command: string,
  args: string[],
  options: ExecOptions = {}
): Promise<number> {
  try {
    const result = await execa(command, args, {
      cwd: options.cwd,
      env: { ...process.env, ...options.env },
      stdio: 'inherit',
      timeout: options.timeout,
      reject: false,
    });
    
    return result.exitCode ?? 0;
  } catch (error) {
    const execaError = error as ExecaError;
    return execaError.exitCode ?? 1;
  }
}

/**
 * Execute a command and stream output while capturing it
 */
export async function execStream(
  command: string,
  args: string[],
  options: ExecOptions & { onStdout?: (data: string) => void; onStderr?: (data: string) => void } = {}
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const stdoutChunks: string[] = [];
  const stderrChunks: string[] = [];
  
  const subprocess = execa(command, args, {
    cwd: options.cwd,
    env: { ...process.env, ...options.env },
    timeout: options.timeout,
    reject: false,
  });
  
  subprocess.stdout?.on('data', (data: Buffer) => {
    const str = data.toString();
    stdoutChunks.push(str);
    options.onStdout?.(str);
  });
  
  subprocess.stderr?.on('data', (data: Buffer) => {
    const str = data.toString();
    stderrChunks.push(str);
    options.onStderr?.(str);
  });
  
  const result = await subprocess;
  
  return {
    stdout: stdoutChunks.join(''),
    stderr: stderrChunks.join(''),
    exitCode: result.exitCode ?? 0,
  };
}

/**
 * Execute cargo build
 */
export async function cargoBuild(
  env: BuildEnvironment,
  options: {
    cwd: string;
    target: string;
    release?: boolean;
    package?: string;
    features?: string;
    buildStd?: string[];
    rustflags?: string;
    targetDir?: string;
    extraArgs?: string[];
  }
): Promise<BuildStepResult> {
  const startTime = Date.now();
  
  const args = ['build'];
  
  // Build std if specified
  if (options.buildStd && options.buildStd.length > 0) {
    args.push('-Z', `build-std=${options.buildStd.join(',')}`);
  }
  
  args.push('--target', options.target);
  
  if (options.release !== false) {
    args.push('--release');
  }
  
  if (options.package) {
    args.push('--package', options.package);
  }
  
  if (options.features) {
    args.push('--features', options.features);
  }
  
  if (options.targetDir) {
    args.push('--target-dir', options.targetDir);
  }
  
  if (options.extraArgs) {
    args.push(...options.extraArgs);
  }
  
  const execEnv: Record<string, string> = {
    ...getExportedEnv(env),
  };
  
  if (options.rustflags) {
    execEnv.RUSTFLAGS = options.rustflags;
  }
  
  const result = await exec('cargo', args, {
    cwd: options.cwd,
    env: execEnv,
  });
  
  const duration = Date.now() - startTime;
  
  return {
    success: result.exitCode === 0,
    duration,
    output: result.stdout,
    error: result.exitCode !== 0 ? result.stderr : undefined,
  };
}

/**
 * Check if required commands exist
 */
export async function requireCommands(commands: string[]): Promise<string[]> {
  const missing: string[] = [];
  
  for (const cmd of commands) {
    const result = await exec('which', [cmd]);
    if (result.exitCode !== 0) {
      missing.push(cmd);
    }
  }
  
  return missing;
}

/**
 * Get file size in human-readable format
 */
export async function getFileSize(path: string): Promise<string> {
  try {
    const stats = await stat(path);
    const bytes = stats.size;
    
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let size = bytes;
    let unitIndex = 0;
    
    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024;
      unitIndex++;
    }
    
    return `${size.toFixed(1)}${units[unitIndex]}`;
  } catch {
    return 'unknown';
  }
}

/**
 * Check if file exists
 */
export function fileExists(path: string): boolean {
  return existsSync(path);
}

/**
 * Strip a binary
 */
export async function stripBinary(path: string, stripAll: boolean = true): Promise<void> {
  const args = stripAll ? ['--strip-all', path] : ['--strip-unneeded', path];
  await exec('strip', args);
}

/**
 * Run ar to create archive
 */
export async function createEmptyArchive(path: string): Promise<void> {
  await exec('ar', ['crs', path]);
}

/**
 * Verify multiboot2 header
 */
export async function verifyMultiboot2(kernelPath: string): Promise<boolean> {
  const result = await exec('grub-file', ['--is-x86-multiboot2', kernelPath]);
  return result.exitCode === 0;
}

/**
 * Copy file with optional stripping
 */
export async function copyAndStrip(
  src: string, 
  dst: string, 
  strip: boolean = true,
  stripAll: boolean = true
): Promise<void> {
  await exec('cp', [src, dst]);
  
  if (strip) {
    await stripBinary(dst, stripAll);
  }
  
  await exec('chmod', ['755', dst]);
}

/**
 * Run a timed build step
 */
export async function timedStep<T>(
  name: string,
  fn: () => Promise<T>
): Promise<T> {
  logger.startTimer(name);
  logger.startSpinner(name);
  
  try {
    const result = await fn();
    logger.spinnerSuccess();
    logger.stepComplete(name);
    return result;
  } catch (error) {
    logger.spinnerFail();
    logger.error(`${name} failed`);
    throw error;
  }
}
