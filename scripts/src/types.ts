/**
 * NexaOS Build System - Configuration Types
 * 
 * Configuration files are in config/ directory:
 *   - config/build.yaml     - Main build settings and profiles
 *   - config/modules.yaml   - Kernel modules configuration  
 *   - config/programs.yaml  - Userspace programs configuration
 *   - config/libraries.yaml - Userspace libraries configuration
 */

// Program configuration from config/programs.yaml
export interface ProgramConfig {
  package: string;
  binary?: string;  // defaults to package name
  dest: string;     // bin, sbin, etc.
  features?: string;
  link: 'std' | 'dyn';  // static or dynamic linking
}

export interface ProgramCategory {
  [category: string]: ProgramConfig[];
}

// Module configuration
export interface ModuleConfig {
  name: string;
  type: number;  // 1=fs, 2=blk, 3=chr, 4=net
  description: string;
  depends?: string[];
  enabled?: boolean;
}

export interface ModuleCategory {
  [category: string]: ModuleConfig[];
}

// Library configuration
export interface LibraryConfig {
  name: string;
  output: string;
  version: number;
  depends: string[];
}

// Build configuration root (merged from all config files)
export interface BuildConfig {
  programs: ProgramCategory;
  modules: ModuleCategory;
  libraries: LibraryConfig[];
  build_order: {
    libraries: string[];
  };
  settings?: BuildSettings;
  profile?: string;
  features?: Record<string, any>;
  featureFlags?: FeatureFlagsConfig;
}

// Feature flag definition
export interface FeatureDefinition {
  enabled: boolean;
  description: string;
  cfg_flag: string;
  dependencies?: string[];
  required?: boolean;
}

// Feature flags configuration from config/features.yaml
export interface FeatureFlagsConfig {
  network: Record<string, FeatureDefinition>;
  kernel: Record<string, FeatureDefinition>;
  filesystem: Record<string, FeatureDefinition>;
  security: Record<string, FeatureDefinition>;
  debug: Record<string, FeatureDefinition>;
  presets: Record<string, FeaturePreset>;
}

// Feature preset
export interface FeaturePreset {
  description: string;
  enable: string[];
  disable: string[];
}

// Main build.yaml configuration
export interface MainBuildConfig {
  settings: BuildSettings;
  profiles: Record<string, BuildProfileConfig>;
  build_order: {
    libraries: string[];
  };
  paths: Record<string, string>;
  features: Record<string, boolean>;
}

export interface BuildSettings {
  default_build_type: 'debug' | 'release';
  default_log_level: 'debug' | 'info' | 'warn' | 'error';
  target_arch: string;
  kernel_target: string;
  userspace_target: string;
  module_target: string;
}

export interface BuildProfileConfig {
  description: string;
  modules: Record<string, string[]>;
  features: Record<string, any>;
}

// Modules config file (config/modules.yaml)
export interface ModulesConfig {
  filesystem?: Record<string, ModuleDefinition>;
  block?: Record<string, ModuleDefinition>;
  memory?: Record<string, ModuleDefinition>;
  network?: Record<string, ModuleDefinition>;
  shared?: Record<string, any>;
  autoload?: Record<string, string[]>;
  signing?: Record<string, any>;
}

export interface ModuleDefinition {
  enabled: boolean;
  type: number;
  description: string;
  package: string;
  output: string;
  load_order: number;
  depends?: string[];
  provides?: string[];
  config?: Record<string, any>;
}

// Programs config file (config/programs.yaml)
export interface ProgramsConfig {
  [category: string]: ProgramDefinition[];
}

export interface ProgramDefinition {
  package: string;
  binary?: string;
  description?: string;
  dest: string;
  features?: string;
  link?: 'std' | 'dyn';
  enabled?: boolean;
  required?: boolean;
  production?: boolean;
}

// Libraries config file (config/libraries.yaml)
export interface LibrariesConfig {
  libraries: LibraryDefinition[];
  build_order: string[];
  install_paths?: Record<string, any>;
}

export interface LibraryDefinition {
  name: string;
  output: string;
  version: number;
  description?: string;
  depends?: string[];
  enabled?: boolean;
  features?: string;
  config?: Record<string, any>;
}

// Build type
export type BuildType = 'debug' | 'release';

// Log level
export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

// Build environment
export interface BuildEnvironment {
  projectRoot: string;
  scriptsDir: string;
  stepsDir: string;
  buildDir: string;
  distDir: string;
  targetDir: string;
  
  buildType: BuildType;
  logLevel: LogLevel;
  
  kernelTargetDir: string;
  userspaceTargetDir: string;
  
  // Target JSON paths
  targets: {
    kernel: string;
    userspace: string;
    userspacePic: string;
    userspaceDyn: string;
    ld: string;
    module: string;
    lib: string;
  };
  
  // Build artifacts
  kernelBin: string;
  initramfsCpio: string;
  rootfsImg: string;
  swapImg: string;
  isoFile: string;
  
  // Sysroot directories
  sysrootDir: string;
  sysrootPicDir: string;
}

// Build step result
export interface BuildStepResult {
  success: boolean;
  duration: number;  // in milliseconds
  output?: string;
  error?: string;
}

// Build step types (CLI commands)
export type BuildStep = 
  | 'full'
  | 'quick'
  | 'kernel'
  | 'userspace'
  | 'libs'
  | 'modules'
  | 'initramfs'
  | 'rootfs'
  | 'swap'
  | 'iso'
  | 'clean';

// RUSTFLAGS builder options
export interface RustFlagsOptions {
  optLevel?: number;
  panic?: 'abort' | 'unwind';
  linker?: string;
  imageBase?: string;
  entry?: string;
  libPaths?: string[];
  linkArgs?: string[];
  undefinedSymbols?: string[];
  relocationModel?: 'static' | 'pic' | 'dynamic-no-pic';
  codeModel?: 'small' | 'kernel' | 'medium' | 'large';
}

// Command execution options
export interface ExecOptions {
  cwd?: string;
  env?: Record<string, string>;
  stdio?: 'inherit' | 'pipe' | 'ignore';
  timeout?: number;
}
