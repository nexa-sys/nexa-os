/**
 * NexaOS Build System - Configuration Types
 */

// Program configuration from build-config.yaml
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

// Build configuration root
export interface BuildConfig {
  programs: ProgramCategory;
  modules: ModuleCategory;
  libraries: LibraryConfig[];
  build_order: {
    libraries: string[];
  };
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

// Build profile
export type BuildProfile = 
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
