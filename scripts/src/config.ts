/**
 * NexaOS Build System - Configuration Loader
 * Parses build-config.yaml
 */

import { readFile } from 'fs/promises';
import { parse as parseYaml } from 'yaml';
import { join } from 'path';
import { 
  BuildConfig, 
  ProgramConfig, 
  ModuleConfig, 
  LibraryConfig
} from './types.js';

let cachedConfig: BuildConfig | null = null;

/**
 * Load and parse the build configuration from YAML
 */
export async function loadBuildConfig(projectRoot: string): Promise<BuildConfig> {
  if (cachedConfig) {
    return cachedConfig;
  }
  
  const configPath = join(projectRoot, 'scripts', 'build-config.yaml');
  const content = await readFile(configPath, 'utf-8');
  cachedConfig = parseYaml(content) as BuildConfig;
  return cachedConfig;
}

/**
 * Get all programs flattened from categories
 */
export function getAllPrograms(config: BuildConfig): ProgramConfig[] {
  const programs: ProgramConfig[] = [];
  
  for (const category of Object.values(config.programs)) {
    for (const program of category) {
      programs.push({
        ...program,
        binary: program.binary ?? program.package,
        link: program.link ?? 'dyn'
      });
    }
  }
  
  return programs;
}

/**
 * Get programs by category
 */
export function getProgramsByCategory(config: BuildConfig, categoryName: string): ProgramConfig[] {
  const category = config.programs[categoryName];
  if (!category) {
    return [];
  }
  
  return category.map(p => ({
    ...p,
    binary: p.binary ?? p.package,
    link: p.link ?? 'dyn'
  }));
}

/**
 * Find a specific program by name (package or binary name)
 */
export function findProgram(config: BuildConfig, name: string): ProgramConfig | undefined {
  for (const category of Object.values(config.programs)) {
    for (const program of category) {
      const binaryName = program.binary ?? program.package;
      if (program.package === name || binaryName === name) {
        return {
          ...program,
          binary: binaryName,
          link: program.link ?? 'dyn'
        };
      }
    }
  }
  return undefined;
}

/**
 * Get all modules flattened from categories
 */
export function getAllModules(config: BuildConfig): ModuleConfig[] {
  const modules: ModuleConfig[] = [];
  
  for (const category of Object.values(config.modules)) {
    modules.push(...category);
  }
  
  return modules;
}

/**
 * Get modules by category
 */
export function getModulesByCategory(config: BuildConfig, categoryName: string): ModuleConfig[] {
  return config.modules[categoryName] ?? [];
}

/**
 * Find a specific module by name
 */
export function findModule(config: BuildConfig, name: string): ModuleConfig | undefined {
  for (const category of Object.values(config.modules)) {
    const module = category.find(m => m.name === name);
    if (module) {
      return module;
    }
  }
  return undefined;
}

/**
 * Get all libraries
 */
export function getAllLibraries(config: BuildConfig): LibraryConfig[] {
  return config.libraries;
}

/**
 * Find a specific library by name
 */
export function findLibrary(config: BuildConfig, name: string): LibraryConfig | undefined {
  return config.libraries.find(l => l.name === name);
}

/**
 * Get library build order
 */
export function getLibraryBuildOrder(config: BuildConfig): string[] {
  return config.build_order.libraries;
}

/**
 * List all program names
 */
export function listProgramNames(config: BuildConfig): string[] {
  return getAllPrograms(config).map(p => p.package);
}

/**
 * List all module names
 */
export function listModuleNames(config: BuildConfig): string[] {
  return getAllModules(config).map(m => m.name);
}

/**
 * List all library names
 */
export function listLibraryNames(config: BuildConfig): string[] {
  return config.libraries.map(l => l.name);
}

/**
 * Get program categories
 */
export function getProgramCategories(config: BuildConfig): string[] {
  return Object.keys(config.programs);
}

/**
 * Get module categories  
 */
export function getModuleCategories(config: BuildConfig): string[] {
  return Object.keys(config.modules);
}
