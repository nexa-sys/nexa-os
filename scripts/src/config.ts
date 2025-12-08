/**
 * NexaOS Build System - Configuration Loader
 * Parses configuration files from config/ directory:
 *   - config/build.yaml     - Main build settings and profiles
 *   - config/modules.yaml   - Kernel modules configuration
 *   - config/programs.yaml  - Userspace programs configuration
 *   - config/libraries.yaml - Userspace libraries configuration
 *   - config/features.yaml  - Compile-time feature flags
 */

import { readFile } from 'fs/promises';
import { existsSync } from 'fs';
import { parse as parseYaml } from 'yaml';
import { join } from 'path';
import { 
  BuildConfig, 
  ProgramConfig, 
  ModuleConfig, 
  LibraryConfig,
  ModulesConfig,
  ProgramsConfig,
  LibrariesConfig,
  MainBuildConfig,
  BuildProfileConfig,
  FeatureFlagsConfig
} from './types.js';

let cachedConfig: BuildConfig | null = null;

/**
 * Load and parse the build configuration from YAML files in config/
 */
export async function loadBuildConfig(projectRoot: string): Promise<BuildConfig> {
  if (cachedConfig) {
    return cachedConfig;
  }
  
  const configDir = join(projectRoot, 'config');
  
  // Load all configuration files
  const [buildContent, modulesContent, programsContent, librariesContent] = await Promise.all([
    readFile(join(configDir, 'build.yaml'), 'utf-8'),
    readFile(join(configDir, 'modules.yaml'), 'utf-8'),
    readFile(join(configDir, 'programs.yaml'), 'utf-8'),
    readFile(join(configDir, 'libraries.yaml'), 'utf-8')
  ]);
  
  // Load features.yaml if it exists
  let featuresConfig: FeatureFlagsConfig | undefined;
  const featuresPath = join(configDir, 'features.yaml');
  if (existsSync(featuresPath)) {
    const featuresContent = await readFile(featuresPath, 'utf-8');
    featuresConfig = parseYaml(featuresContent) as FeatureFlagsConfig;
  }
  
  const buildConfig = parseYaml(buildContent) as MainBuildConfig;
  const modulesConfig = parseYaml(modulesContent) as ModulesConfig;
  const programsConfig = parseYaml(programsContent) as ProgramsConfig;
  const librariesConfig = parseYaml(librariesContent) as LibrariesConfig;
  
  // Merge into unified BuildConfig structure
  cachedConfig = mergeConfigs(buildConfig, modulesConfig, programsConfig, librariesConfig, featuresConfig);
  return cachedConfig;
}

/**
 * Merge separate config files into unified BuildConfig
 */
function mergeConfigs(
  build: MainBuildConfig,
  modules: ModulesConfig,
  programs: ProgramsConfig,
  libraries: LibrariesConfig,
  featureFlags?: FeatureFlagsConfig
): BuildConfig {
  // Get current profile from environment or use default
  const profileName = process.env.BUILD_PROFILE || 'default';
  const profile: BuildProfileConfig | undefined = build.profiles[profileName] || build.profiles['default'];
  
  // Convert modules config to BuildConfig format, filtering by profile
  const enabledModules = profile?.modules || {};
  
  const moduleCategories: Record<string, ModuleConfig[]> = {};
  for (const [category, moduleList] of Object.entries(modules)) {
    if (category === 'shared' || category === 'autoload' || category === 'signing') continue;
    
    // Get the list of enabled modules for this category from the profile
    // If category is not in profile.modules, include all enabled modules
    // If category is in profile.modules with empty array [], include none
    // If category is in profile.modules with values, include only those
    const categoryInProfile = category in enabledModules;
    const enabledInCategory = enabledModules[category] || [];
    const categoryModules: ModuleConfig[] = [];
    
    for (const [name, config] of Object.entries(moduleList as Record<string, any>)) {
      // Module is enabled if:
      // 1. Explicitly enabled in modules.yaml (enabled !== false)
      // 2. AND either: category not specified in profile (include all) OR module name is in profile list
      const isEnabledInConfig = config.enabled !== false;
      const isEnabledInProfile = !categoryInProfile || enabledInCategory.includes(name);
      
      if (isEnabledInConfig && isEnabledInProfile) {
        categoryModules.push({
          name,
          type: config.type,
          description: config.description,
          depends: config.depends,
          enabled: true
        });
      }
    }
    
    if (categoryModules.length > 0) {
      moduleCategories[category] = categoryModules;
    }
  }
  
  // Convert programs config to BuildConfig format
  const programCategories: Record<string, ProgramConfig[]> = {};
  for (const [category, programList] of Object.entries(programs)) {
    if (!Array.isArray(programList)) continue;
    
    programCategories[category] = programList
      .filter((p: any) => p.enabled !== false)
      .map((p: any) => ({
        package: p.package,
        binary: p.binary,
        dest: p.dest,
        features: p.features,
        link: p.link || 'dyn'
      }));
  }
  
  // Convert libraries config
  const libraryList: LibraryConfig[] = (libraries.libraries || [])
    .filter((l: any) => l.enabled !== false)
    .map((l: any) => ({
      name: l.name,
      output: l.output,
      version: l.version,
      depends: l.depends || []
    }));
  
  return {
    programs: programCategories,
    modules: moduleCategories,
    libraries: libraryList,
    build_order: {
      libraries: libraries.build_order || build.build_order?.libraries || []
    },
    settings: build.settings,
    profile: profileName,
    features: profile?.features || {},
    featureFlags: featureFlags
  };
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

/**
 * Get all enabled feature flags for kernel compilation
 * Checks environment variables FEATURE_xxx to override config values
 * @returns Array of cfg_flag strings to pass to rustc
 */
export function getEnabledFeatureFlags(config: BuildConfig): string[] {
  if (!config.featureFlags) {
    return [];
  }
  
  const enabledFlags: string[] = [];
  const categories = ['network', 'kernel', 'filesystem', 'security', 'debug'] as const;
  
  for (const category of categories) {
    const categoryConfig = config.featureFlags[category];
    if (!categoryConfig) continue;
    
    for (const [name, feature] of Object.entries(categoryConfig)) {
      // Check for environment variable override (FEATURE_TCP=true/false)
      const envVar = `FEATURE_${name.toUpperCase()}`;
      const envValue = process.env[envVar];
      
      let isEnabled = feature.enabled;
      if (envValue !== undefined) {
        isEnabled = envValue.toLowerCase() === 'true' || envValue === '1';
      }
      
      // Check dependencies
      if (isEnabled && feature.dependencies) {
        for (const dep of feature.dependencies) {
          if (!isFeatureEnabled(config, dep)) {
            console.warn(`Warning: Feature '${name}' depends on '${dep}' which is disabled`);
            isEnabled = false;
            break;
          }
        }
      }
      
      if (isEnabled) {
        enabledFlags.push(feature.cfg_flag);
      }
    }
  }
  
  return enabledFlags;
}

/**
 * Check if a specific feature is enabled
 */
export function isFeatureEnabled(config: BuildConfig, featureName: string): boolean {
  if (!config.featureFlags) return false;
  
  const categories = ['network', 'kernel', 'filesystem', 'security', 'debug'] as const;
  
  for (const category of categories) {
    const categoryConfig = config.featureFlags[category];
    if (!categoryConfig) continue;
    
    const feature = categoryConfig[featureName];
    if (feature) {
      // Check environment variable override
      const envVar = `FEATURE_${featureName.toUpperCase()}`;
      const envValue = process.env[envVar];
      if (envValue !== undefined) {
        return envValue.toLowerCase() === 'true' || envValue === '1';
      }
      return feature.enabled;
    }
  }
  
  return false;
}

/**
 * Get feature flags as RUSTFLAGS string for cfg attributes
 * @returns RUSTFLAGS string like '--cfg net_tcp --cfg net_udp'
 */
export function getFeatureFlagsRustFlags(config: BuildConfig): string {
  const flags = getEnabledFeatureFlags(config);
  return flags.map(f => `--cfg ${f}`).join(' ');
}

/**
 * Apply a feature preset
 */
export function applyFeaturePreset(config: BuildConfig, presetName: string): void {
  if (!config.featureFlags?.presets) return;
  
  const preset = config.featureFlags.presets[presetName];
  if (!preset) {
    console.warn(`Warning: Feature preset '${presetName}' not found`);
    return;
  }
  
  const categories = ['network', 'kernel', 'filesystem', 'security', 'debug'] as const;
  
  // Apply enables
  for (const featureName of preset.enable) {
    for (const category of categories) {
      const categoryConfig = config.featureFlags[category];
      if (categoryConfig?.[featureName]) {
        categoryConfig[featureName].enabled = true;
      }
    }
  }
  
  // Apply disables
  for (const featureName of preset.disable) {
    for (const category of categories) {
      const categoryConfig = config.featureFlags[category];
      if (categoryConfig?.[featureName]) {
        categoryConfig[featureName].enabled = false;
      }
    }
  }
}

/**
 * List all available feature presets
 */
export function listFeaturePresets(config: BuildConfig): string[] {
  if (!config.featureFlags?.presets) return [];
  return Object.keys(config.featureFlags.presets);
}
