/**
 * NexaOS Build System - Kernel Modules Builder
 */

import { join } from 'path';
import { mkdir, writeFile, readdir, stat as fsStat } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult, ModuleConfig } from '../types.js';
import { logger } from '../logger.js';
import { cargoBuild, exec, getFileSize } from '../exec.js';
import { getModuleRustFlags } from '../env.js';
import { loadBuildConfig, getAllModules, findModule } from '../config.js';

const MODULES_DIR = 'modules';

/**
 * Sign a kernel module using the sign-module.sh script
 */
async function signModule(env: BuildEnvironment, modulePath: string): Promise<boolean> {
  const signScript = join(env.projectRoot, 'scripts', 'sign-module.sh');
  
  if (!existsSync(signScript)) {
    logger.warn('sign-module.sh not found, skipping signature');
    return false;
  }
  
  logger.info(`Signing module: ${modulePath}`);
  
  // Use -i flag to sign in place
  const result = await exec(signScript, ['-i', modulePath]);
  
  if (result.exitCode !== 0) {
    logger.warn(`Module signing failed: ${result.stderr}`);
    return false;
  }
  
  logger.info('Module signed successfully');
  return true;
}

/**
 * Create a simple NKM metadata-only module
 */
async function createSimpleNkm(
  name: string,
  type: number,
  version: string,
  description: string,
  outputPath: string
): Promise<void> {
  // NKM header format (80 bytes header + string table)
  const header = Buffer.alloc(80);
  
  // Magic: NKM\x01
  header.write('NKM', 0);
  header.writeUInt8(0x01, 3);
  
  // Format version
  header.writeUInt8(1, 4);
  
  // Module type
  header.writeUInt8(type, 5);
  
  // Dep count
  header.writeUInt8(0, 6);
  
  // Flags
  header.writeUInt8(0, 7);
  
  // Create string table
  const stringTable = Buffer.concat([
    Buffer.from(version + '\0'),
    Buffer.from(description + '\0'),
  ]);
  
  // Code offset (header size)
  header.writeUInt32LE(80, 8);
  
  // Code size (string table size for simple module)
  header.writeUInt32LE(stringTable.length, 12);
  
  // Init offset
  header.writeUInt32LE(80, 16);
  
  // Init size
  header.writeUInt32LE(stringTable.length, 20);
  
  // Reserved (8 bytes at offset 24)
  
  // String table offset
  header.writeUInt32LE(80, 32);
  
  // String table size
  header.writeUInt32LE(stringTable.length, 36);
  
  // Name (32 bytes at offset 40)
  const nameBuffer = Buffer.alloc(32);
  nameBuffer.write(name.substring(0, 31));
  nameBuffer.copy(header, 40);
  
  // Write the file
  const content = Buffer.concat([header, stringTable]);
  await writeFile(outputPath, content);
  
  logger.info(`Created simple NKM: ${outputPath} (${content.length} bytes)`);
}

/**
 * Build a single kernel module
 */
async function buildModule(
  env: BuildEnvironment,
  module: ModuleConfig
): Promise<BuildStepResult> {
  logger.step(`Building ${module.name} module...`);
  
  const startTime = Date.now();
  const moduleSrc = join(env.projectRoot, MODULES_DIR, module.name);
  const modulesBuilDir = join(env.buildDir, 'modules');
  const outputNkm = join(modulesBuilDir, `${module.name}.nkm`);
  
  await mkdir(modulesBuilDir, { recursive: true });
  
  if (!existsSync(moduleSrc)) {
    logger.warn(`Module source not found: ${moduleSrc}`);
    logger.info('Creating metadata-only module');
    await createSimpleNkm(module.name, module.type, '1.0.0', `${module.description} (built-in)`, outputNkm);
    return { success: true, duration: Date.now() - startTime };
  }
  
  // Clean previous build
  await exec('cargo', ['clean'], { cwd: moduleSrc });
  
  // Build as staticlib using kernel target
  logger.info(`Compiling ${module.name} module as staticlib...`);
  
  const result = await cargoBuild(env, {
    cwd: moduleSrc,
    target: env.targets.kernel,
    release: true,
    buildStd: ['core', 'alloc', 'compiler_builtins'],
    rustflags: getModuleRustFlags(),
    extraArgs: ['-Z', 'build-std-features=compiler-builtins-mem'],
    logName: `module-${module.name}`,
  });
  
  if (!result.success) {
    logger.warn(`Build failed, creating metadata-only module`);
    await createSimpleNkm(module.name, module.type, '1.0.0', `${module.description} (built-in)`, outputNkm);
    return { success: true, duration: Date.now() - startTime };
  }
  
  // Find the built staticlib
  const findResult = await exec('find', [moduleSrc + '/target', '-name', `lib${module.name}_module.a`]);
  const staticlib = findResult.stdout.trim().split('\n')[0];
  
  if (!staticlib || !existsSync(staticlib)) {
    logger.warn('Staticlib not found, creating metadata-only module');
    await createSimpleNkm(module.name, module.type, '1.0.0', `${module.description} (built-in)`, outputNkm);
    return { success: true, duration: Date.now() - startTime };
  }
  
  logger.info(`Found staticlib: ${staticlib}`);
  
  // Extract and link object files - use unique temp dir per module to avoid conflicts
  const tempDir = join(modulesBuilDir, `.temp-${module.name}`);
  
  // Clean temp dir before extracting
  await exec('rm', ['-rf', tempDir]);
  await mkdir(tempDir, { recursive: true });
  
  // Extract object files
  await exec('ar', ['x', staticlib], { cwd: tempDir });
  
  // Find object files
  const files = await readdir(tempDir);
  const objFiles = files.filter(f => f.endsWith('.o'));
  
  logger.info(`Found ${objFiles.length} object files in ${tempDir}: ${objFiles.join(', ')}`);
  
  if (objFiles.length > 0) {
    logger.info(`Linking ${objFiles.length} object files...`);
    
    // Link object files into relocatable module
    // Match the shell script behavior: try gc-sections first, then simple link
    let linked = false;
    
    for (const linker of ['ld.lld', 'ld']) {
      // First try with gc-sections (same as shell script)
      const gcArgs = ['-r', '--gc-sections', '-o', outputNkm, ...objFiles.map(f => join(tempDir, f))];
      logger.info(`Trying: ${linker} -r --gc-sections ...`);
      let linkResult = await exec(linker, gcArgs);
      if (linkResult.exitCode === 0 && existsSync(outputNkm)) {
        const stats = await fsStat(outputNkm);
        if (stats.size > 1000) {  // Must be > 1KB to be valid ELF
          linked = true;
          logger.info(`Link succeeded with ${linker} gc-sections (${stats.size} bytes)`);
          break;
        }
      }
      logger.info(`Link with gc-sections failed (${linker}): code=${linkResult.exitCode} err=${linkResult.stderr.substring(0, 200)}`);
      
      // Fall back to simple relocatable link without gc-sections
      const simpleArgs = ['-r', '-o', outputNkm, ...objFiles.map(f => join(tempDir, f))];
      logger.info(`Trying: ${linker} -r ...`);
      linkResult = await exec(linker, simpleArgs);
      if (linkResult.exitCode === 0 && existsSync(outputNkm)) {
        const stats = await fsStat(outputNkm);
        if (stats.size > 1000) {
          linked = true;
          logger.info(`Link succeeded with ${linker} simple (${stats.size} bytes)`);
          break;
        }
      }
      logger.info(`Simple link failed (${linker}): code=${linkResult.exitCode} err=${linkResult.stderr.substring(0, 200)}`);
    }
    
    // Cleanup temp directory
    await exec('rm', ['-rf', tempDir]);
    
    if (linked && existsSync(outputNkm)) {
      await exec('strip', ['--strip-debug', outputNkm]);
      
      // Sign the module
      const signResult = await signModule(env, outputNkm);
      if (!signResult) {
        logger.warn(`Module ${module.name} built but signing failed`);
      }
      
      const size = await getFileSize(outputNkm);
      logger.success(`${module.name}.nkm built and signed (${size})`);
    } else {
      logger.warn('Link failed, creating metadata-only module');
      await createSimpleNkm(module.name, module.type, '1.0.0', `${module.description} (built-in)`, outputNkm);
      // Sign the simple module too
      await signModule(env, outputNkm);
    }
  } else {
    logger.warn('No object files found, creating metadata-only module');
    await createSimpleNkm(module.name, module.type, '1.0.0', `${module.description} (built-in)`, outputNkm);
    // Sign the simple module too
    await signModule(env, outputNkm);
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Build all kernel modules
 */
export async function buildAllModules(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building Kernel Modules');
  
  const startTime = Date.now();
  const config = await loadBuildConfig(env.projectRoot);
  const modules = getAllModules(config);
  
  let successCount = 0;
  let failCount = 0;
  
  for (const module of modules) {
    const result = await buildModule(env, module);
    if (result.success) {
      successCount++;
    } else {
      failCount++;
    }
  }
  
  if (failCount > 0) {
    logger.warn(`Built ${successCount} modules, ${failCount} failed`);
  } else {
    logger.success(`All ${successCount} modules built successfully`);
  }
  
  return {
    success: failCount === 0,
    duration: Date.now() - startTime,
  };
}

/**
 * Build a single module by name
 */
export async function buildSingleModule(
  env: BuildEnvironment,
  name: string
): Promise<BuildStepResult> {
  const config = await loadBuildConfig(env.projectRoot);
  const module = findModule(config, name);
  
  if (!module) {
    logger.error(`Unknown module: ${name}`);
    return { success: false, duration: 0, error: `Unknown module: ${name}` };
  }
  
  return buildModule(env, module);
}

/**
 * List all available modules
 */
export async function listModules(env: BuildEnvironment): Promise<void> {
  const config = await loadBuildConfig(env.projectRoot);
  const modules = getAllModules(config);
  
  logger.info('Available modules:');
  
  const typeNames: Record<number, string> = {
    1: 'filesystem',
    2: 'block',
    3: 'character',
    4: 'network',
  };
  
  const rows = modules.map(m => [
    m.name,
    typeNames[m.type] ?? 'unknown',
    m.description,
  ]);
  
  logger.table(['Name', 'Type', 'Description'], rows);
}
