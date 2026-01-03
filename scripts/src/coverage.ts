/**
 * Coverage Analysis Module
 * 
 * Analyzes test files to determine actual function coverage by parsing
 * Rust source code and matching function calls in tests.
 * 
 * Key features:
 * - Deep module path analysis (crate::mod::submod::func)
 * - Struct/impl method tracking
 * - Type alias resolution
 * - Re-export tracking via use statements
 * - Integration test correlation
 */

import { readFileSync, readdirSync } from 'fs';
import { resolve } from 'path';

// =============================================================================
// Types
// =============================================================================

export interface FunctionInfo {
  name: string;
  filePath: string;
  lineStart: number;
  lineEnd: number;
  isPub: boolean;
  implType?: string;       // If this is a method, the impl type (e.g., "SignalState")
  isMethod: boolean;       // true if inside impl block
}

export interface BranchInfo {
  lineNumber: number;
  type: 'if' | 'match' | 'loop' | 'while';
  covered: boolean;
}

export interface FileStats {
  filePath: string;           // 相对于 src/ 的路径
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;         // 代码行数（不含注释和空行）
  coveredLines: number;
  totalStatements: number;    // 语句数
  coveredStatements: number;
  totalBranches: number;      // 分支数
  coveredBranches: number;
  uncoveredLineNumbers: number[];  // 未覆盖的行号
  functions: FunctionInfo[];  // 文件中的所有函数
}

export interface ModuleStats {
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  coveragePct: number;
  files: FileStats[];         // 模块中的所有文件统计
}

export interface CoverageStats {
  totalFunctions: number;
  coveredFunctions: number;
  functionCoveragePct: number;
  totalLines: number;
  coveredLines: number;
  lineCoveragePct: number;
  totalStatements: number;
  coveredStatements: number;
  statementCoveragePct: number;
  totalBranches: number;
  coveredBranches: number;
  branchCoveragePct: number;
  totalTests: number;
  passedTests: number;
  failedTests: number;
  testPassRate: number;
  modules: Record<string, ModuleStats>;
}

export interface TestResult {
  name: string;
  passed: boolean;
}

// =============================================================================
// File Discovery
// =============================================================================

/** Recursively find all .rs files in a directory */
export function findRustFiles(dir: string): string[] {
  const files: string[] = [];
  
  try {
    const entries = readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = resolve(dir, entry.name);
      if (entry.isDirectory()) {
        files.push(...findRustFiles(fullPath));
      } else if (entry.name.endsWith('.rs')) {
        files.push(fullPath);
      }
    }
  } catch {
    // Ignore errors
  }
  
  return files;
}

/** Auto-discover kernel modules from src/ directory */
export function discoverKernelModules(projectRoot: string): string[] {
  const srcDir = resolve(projectRoot, 'src');
  const modules: string[] = [];
  
  try {
    const entries = readdirSync(srcDir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.isDirectory()) {
        modules.push(entry.name);
      }
    }
  } catch {
    // Fallback to known modules
    return ['arch', 'boot', 'drivers', 'fs', 'interrupts', 'ipc',
            'kmod', 'mm', 'net', 'process', 'safety', 'scheduler',
            'security', 'smp', 'syscalls', 'tty', 'udrv'];
  }
  
  return modules.sort();
}

// =============================================================================
// Rust Code Parsing
// =============================================================================

/** Extract impl block type name (e.g., "impl SignalState" -> "SignalState") */
function parseImplType(line: string): string | null {
  // Match: impl Type, impl<T> Type, impl Trait for Type
  const implMatch = line.match(/impl(?:<[^>]+>)?\s+(?:(\w+)\s+for\s+)?(\w+)/);
  if (implMatch) {
    return implMatch[2] || implMatch[1];
  }
  return null;
}

/** Parse Rust file and extract function definitions with impl context */
export function parseRustFunctions(filePath: string): FunctionInfo[] {
  const functions: FunctionInfo[] = [];
  
  try {
    const content = readFileSync(filePath, 'utf-8');
    const lines = content.split('\n');
    
    const fnPattern = /^\s*(pub\s+)?(async\s+)?fn\s+(\w+)/;
    const implPattern = /^\s*impl/;
    
    let inFunction = false;
    let braceDepth = 0;
    let currentFn: FunctionInfo | null = null;
    
    // Track impl blocks
    let implStack: { type: string | null; depth: number }[] = [];
    let globalBraceDepth = 0;
    
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const trimmed = line.trim();
      
      // Skip comments
      if (trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*')) {
        continue;
      }
      
      const openBraces = (line.match(/{/g) || []).length;
      const closeBraces = (line.match(/}/g) || []).length;
      
      // Detect impl blocks
      if (implPattern.test(trimmed) && !inFunction) {
        const implType = parseImplType(trimmed);
        // If there's a brace on this line, push immediately
        if (openBraces > 0) {
          implStack.push({ type: implType, depth: globalBraceDepth + 1 });
        } else {
          // impl block will open on next line with brace
          implStack.push({ type: implType, depth: globalBraceDepth + 1 });
        }
      }
      
      // Update global brace depth
      globalBraceDepth += openBraces - closeBraces;
      
      // Pop impl blocks when we exit their scope
      while (implStack.length > 0 && globalBraceDepth < implStack[implStack.length - 1].depth) {
        implStack.pop();
      }
      
      const fnMatch = trimmed.match(fnPattern);
      if (fnMatch && !inFunction) {
        const currentImpl = implStack.length > 0 ? implStack[implStack.length - 1] : null;
        
        currentFn = {
          name: fnMatch[3],
          filePath,
          lineStart: i + 1,
          lineEnd: i + 1,
          isPub: !!fnMatch[1],
          implType: currentImpl?.type || undefined,
          isMethod: currentImpl !== null
        };
        
        inFunction = true;
        braceDepth = openBraces - closeBraces;
        
        if (braceDepth <= 0 && line.includes('{')) {
          currentFn.lineEnd = i + 1;
          functions.push(currentFn);
          currentFn = null;
          inFunction = false;
          braceDepth = 0;
        }
        continue;
      }
      
      if (inFunction && currentFn) {
        braceDepth += openBraces - closeBraces;
        
        if (braceDepth <= 0) {
          currentFn.lineEnd = i + 1;
          functions.push(currentFn);
          currentFn = null;
          inFunction = false;
          braceDepth = 0;
        }
      }
    }
  } catch {
    // Ignore read errors
  }
  
  return functions;
}

/** Parse file to count statements (executable lines) */
function countStatements(content: string): number {
  const lines = content.split('\n');
  let statements = 0;
  
  for (const line of lines) {
    const stripped = line.trim();
    // Count lines that likely contain executable statements
    if (stripped && 
        !stripped.startsWith('//') && 
        !stripped.startsWith('/*') &&
        !stripped.startsWith('*') &&
        !stripped.startsWith('*/') &&
        stripped !== '{' && 
        stripped !== '}' &&
        !stripped.startsWith('use ') &&
        !stripped.startsWith('mod ') &&
        !stripped.startsWith('pub mod ') &&
        !stripped.startsWith('#[') &&
        !stripped.startsWith('//!') &&
        !stripped.startsWith('///')) {
      statements++;
    }
  }
  
  return statements;
}

/** Parse file to find branch points (if, match, while, loop) */
function findBranches(content: string): { lineNumber: number; type: 'if' | 'match' | 'loop' | 'while' }[] {
  const lines = content.split('\n');
  const branches: { lineNumber: number; type: 'if' | 'match' | 'loop' | 'while' }[] = [];
  
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    
    // Skip comments
    if (line.startsWith('//') || line.startsWith('/*')) continue;
    
    // Match branch keywords at start of expressions
    if (/\bif\s+/.test(line) && !line.includes('else if')) {
      branches.push({ lineNumber: i + 1, type: 'if' });
    }
    if (/\belse\s+if\s+/.test(line)) {
      branches.push({ lineNumber: i + 1, type: 'if' });
    }
    if (/\bmatch\s+/.test(line)) {
      branches.push({ lineNumber: i + 1, type: 'match' });
    }
    if (/\bwhile\s+/.test(line)) {
      branches.push({ lineNumber: i + 1, type: 'while' });
    }
    if (/\bloop\s*\{/.test(line)) {
      branches.push({ lineNumber: i + 1, type: 'loop' });
    }
  }
  
  return branches;
}

/** Analyze a single Rust file for detailed stats */
export function analyzeRustFile(filePath: string, projectRoot: string): FileStats {
  const relativePath = filePath.replace(resolve(projectRoot, 'src') + '/', '');
  
  const stats: FileStats = {
    filePath: relativePath,
    totalFunctions: 0,
    coveredFunctions: 0,
    totalLines: 0,
    coveredLines: 0,
    totalStatements: 0,
    coveredStatements: 0,
    totalBranches: 0,
    coveredBranches: 0,
    uncoveredLineNumbers: [],
    functions: []
  };
  
  try {
    const content = readFileSync(filePath, 'utf-8');
    const lines = content.split('\n');
    
    // Count code lines (non-empty, non-comment)
    for (let i = 0; i < lines.length; i++) {
      const stripped = lines[i].trim();
      if (stripped && !stripped.startsWith('//')) {
        stats.totalLines++;
      }
    }
    
    // Count statements
    stats.totalStatements = countStatements(content);
    
    // Find branches
    const branches = findBranches(content);
    stats.totalBranches = branches.length;
    
    // Parse functions
    stats.functions = parseRustFunctions(filePath);
    stats.totalFunctions = stats.functions.length;
    
  } catch {
    // Ignore errors
  }
  
  return stats;
}

/** Analyze a kernel module to get all functions and line counts */
export function analyzeKernelModule(projectRoot: string, moduleName: string): { 
  functions: FunctionInfo[], 
  totalLines: number, 
  codeLines: number,
  files: FileStats[],
  totalStatements: number,
  totalBranches: number
} {
  const modulePath = resolve(projectRoot, 'src', moduleName);
  const result = { 
    functions: [] as FunctionInfo[], 
    totalLines: 0, 
    codeLines: 0,
    files: [] as FileStats[],
    totalStatements: 0,
    totalBranches: 0
  };
  
  const rustFiles = findRustFiles(modulePath);
  
  for (const file of rustFiles) {
    try {
      const content = readFileSync(file, 'utf-8');
      const lines = content.split('\n');
      result.totalLines += lines.length;
      
      for (const line of lines) {
        const stripped = line.trim();
        if (stripped && !stripped.startsWith('//')) {
          result.codeLines++;
        }
      }
      
      result.functions.push(...parseRustFunctions(file));
      
      // Analyze file for detailed stats
      const fileStats = analyzeRustFile(file, projectRoot);
      result.files.push(fileStats);
      result.totalStatements += fileStats.totalStatements;
      result.totalBranches += fileStats.totalBranches;
    } catch {
      // Ignore errors
    }
  }
  
  return result;
}

// =============================================================================
// Test Coverage Analysis
// =============================================================================

/** Check if a word is a Rust keyword (to filter out false positives) */
function isRustKeyword(word: string): boolean {
  const keywords = new Set([
    'if', 'else', 'for', 'while', 'loop', 'match', 'return', 'break',
    'continue', 'fn', 'let', 'mut', 'const', 'static', 'pub', 'use',
    'mod', 'struct', 'enum', 'impl', 'trait', 'type', 'where', 'async',
    'await', 'move', 'ref', 'self', 'super', 'crate', 'as', 'in',
    'Some', 'None', 'Ok', 'Err', 'true', 'false', 'assert', 'assert_eq',
    'assert_ne', 'debug_assert', 'panic', 'println', 'eprintln', 'format',
    'vec', 'box', 'dyn', 'unsafe', 'extern', 'sizeof', 'typeof',
    'tests', 'test', 'cfg', 'derive', 'allow', 'warn', 'deny',
  ]);
  return keywords.has(word);
}

/** Common method names that should not count as unique coverage */
function isCommonMethod(word: string): boolean {
  const commonMethods = new Set([
    'new', 'default', 'clone', 'drop', 'from', 'into', 'try_from', 'try_into',
    'len', 'is_empty', 'push', 'pop', 'get', 'set', 'insert', 'remove',
    'iter', 'map', 'filter', 'collect', 'unwrap', 'expect', 'ok', 'err',
    'as_ref', 'as_mut', 'as_ptr', 'as_slice', 'to_string', 'to_vec',
    'read', 'write', 'flush', 'close', 'open', 'with_capacity',
    'contains', 'clear', 'extend', 'reserve', 'truncate', 'split',
  ]);
  return commonMethods.has(word);
}

/** 
 * Extract all module references from test file with full path analysis
 * Returns map of module -> set of submodule paths used
 */
function extractModuleReferences(content: string): Map<string, Set<string>> {
  const modules = new Map<string, Set<string>>();
  
  const addModule = (mod: string, subpath?: string) => {
    if (!modules.has(mod)) {
      modules.set(mod, new Set());
    }
    if (subpath) {
      modules.get(mod)!.add(subpath);
    }
  };
  
  // Match: use crate::module::submod::...
  // Captures: crate::ipc::signal -> mod=ipc, subpath=signal
  const usePattern = /use\s+crate::(\w+)(?:::(\w+))?/g;
  let match;
  while ((match = usePattern.exec(content)) !== null) {
    addModule(match[1], match[2]);
  }
  
  // Match: crate::module::submod::func
  const crateCallPattern = /crate::(\w+)(?:::(\w+))?::/g;
  while ((match = crateCallPattern.exec(content)) !== null) {
    addModule(match[1], match[2]);
  }
  
  return modules;
}

/** 
 * Extract types used in test file (struct instantiation, method calls)
 * Returns set of type names like "SignalState", "ProcessContext" etc.
 */
function extractTypesUsed(content: string): Set<string> {
  const types = new Set<string>();
  
  // Type::method() pattern - capture Type
  const typeMethodPattern = /([A-Z][a-zA-Z0-9_]*)::\w+\s*[(<]/g;
  let match;
  while ((match = typeMethodPattern.exec(content)) !== null) {
    if (!isRustKeyword(match[1])) {
      types.add(match[1]);
    }
  }
  
  // let x: Type = ... or fn(x: Type) pattern
  const typeAnnotationPattern = /:\s*(?:&(?:mut\s+)?)?([A-Z][a-zA-Z0-9_]*)/g;
  while ((match = typeAnnotationPattern.exec(content)) !== null) {
    if (!isRustKeyword(match[1])) {
      types.add(match[1]);
    }
  }
  
  // use crate::module::{Type1, Type2}
  const useImportPattern = /use\s+crate::[^{]+::\{([^}]+)\}/g;
  while ((match = useImportPattern.exec(content)) !== null) {
    const items = match[1].split(',').map(s => s.trim());
    for (const item of items) {
      const name = item.split(' ')[0]; // Handle "Type as Alias"
      if (name && /^[A-Z]/.test(name) && !isRustKeyword(name)) {
        types.add(name);
      }
    }
  }
  
  return types;
}

/** Extract function/method calls from test file content */
function extractFunctionCalls(content: string): { 
  functions: Set<string>;
  methodCalls: Map<string, Set<string>>;  // Type -> methods called
  importedItems: Set<string>;  // Items imported via use statements (functions, constants)
} {
  const functions = new Set<string>();
  const methodCalls = new Map<string, Set<string>>();
  const importedItems = new Set<string>();
  
  const addMethodCall = (type: string, method: string) => {
    if (!methodCalls.has(type)) {
      methodCalls.set(type, new Set());
    }
    methodCalls.get(type)!.add(method);
  };
  
  // === Extract items from use statements ===
  // use crate::module::submod::{item1, item2}
  const useImportBracesPattern = /use\s+crate::[^{]+::\{([^}]+)\}/g;
  let match;
  while ((match = useImportBracesPattern.exec(content)) !== null) {
    const items = match[1].split(',').map(s => s.trim());
    for (const item of items) {
      const name = item.split(' ')[0]; // Handle "func as alias"
      if (name && !isRustKeyword(name)) {
        importedItems.add(name);
        // If it looks like a function (lowercase), add to functions too
        if (/^[a-z_]/.test(name)) {
          functions.add(name);
        }
      }
    }
  }
  
  // use crate::module::submod::item (single import)
  const useSingleImportPattern = /use\s+crate::[^;{]+::(\w+)\s*;/g;
  while ((match = useSingleImportPattern.exec(content)) !== null) {
    const name = match[1];
    if (!isRustKeyword(name)) {
      importedItems.add(name);
      if (/^[a-z_]/.test(name)) {
        functions.add(name);
      }
    }
  }
  
  // Type::method( pattern
  const typeMethodPattern = /([A-Z][a-zA-Z0-9_]*)::(\w+)\s*[(<]/g;
  while ((match = typeMethodPattern.exec(content)) !== null) {
    if (!isRustKeyword(match[1]) && !isRustKeyword(match[2])) {
      addMethodCall(match[1], match[2]);
    }
  }
  
  // .method( pattern for instance methods
  const instanceMethodPattern = /\.(\w+)\s*\(/g;
  while ((match = instanceMethodPattern.exec(content)) !== null) {
    const method = match[1];
    if (!isRustKeyword(method) && !isCommonMethod(method)) {
      functions.add(method);
    }
  }
  
  // module::func( pattern - standalone functions
  const moduleFuncPattern = /(?<![A-Z])(\w+)::(\w+)\s*\(/g;
  while ((match = moduleFuncPattern.exec(content)) !== null) {
    const func = match[2];
    // Skip if first part looks like a type (starts with uppercase)
    if (!/^[A-Z]/.test(match[1]) && !isRustKeyword(func)) {
      functions.add(func);
    }
  }
  
  // standalone func( - but not keywords, and require at least 3 chars
  const standaloneFuncPattern = /(?<![:\.\w])(\w{3,})\s*\(/g;
  while ((match = standaloneFuncPattern.exec(content)) !== null) {
    const func = match[1];
    if (!isRustKeyword(func) && !isCommonMethod(func) && !/^[A-Z]/.test(func)) {
      functions.add(func);
    }
  }
  
  // Function references (without calling) - let _ = func_name; or = func_name,
  // Only match identifiers that were imported
  importedItems.forEach(item => {
    // Check if this imported item is actually used (referenced) in code
    const usePattern = new RegExp(`\\b${item}\\b`, 'g');
    if (usePattern.test(content)) {
      if (/^[a-z_]/.test(item)) {
        functions.add(item);
      }
    }
  });
  
  return { functions, methodCalls, importedItems };
}

/** 
 * Build a map from type names to their defining module
 * Analyzes kernel source to know where each type is defined
 */
function buildTypeToModuleMap(projectRoot: string, kernelModules: string[]): Map<string, string> {
  const typeToModule = new Map<string, string>();
  
  for (const moduleName of kernelModules) {
    const modulePath = resolve(projectRoot, 'src', moduleName);
    const rustFiles = findRustFiles(modulePath);
    
    for (const file of rustFiles) {
      try {
        const content = readFileSync(file, 'utf-8');
        
        // Find struct definitions: pub struct TypeName
        const structPattern = /(?:pub\s+)?struct\s+([A-Z][a-zA-Z0-9_]*)/g;
        let match;
        while ((match = structPattern.exec(content)) !== null) {
          typeToModule.set(match[1], moduleName);
        }
        
        // Find enum definitions: pub enum TypeName
        const enumPattern = /(?:pub\s+)?enum\s+([A-Z][a-zA-Z0-9_]*)/g;
        while ((match = enumPattern.exec(content)) !== null) {
          typeToModule.set(match[1], moduleName);
        }
        
        // Find type aliases: pub type TypeName = ...
        const typeAliasPattern = /(?:pub\s+)?type\s+([A-Z][a-zA-Z0-9_]*)\s*=/g;
        while ((match = typeAliasPattern.exec(content)) !== null) {
          typeToModule.set(match[1], moduleName);
        }
        
      } catch {
        // Ignore errors
      }
    }
  }
  
  return typeToModule;
}

/** 
 * Build a map from function/const/static names to modules
 */
function buildFunctionToModuleMap(projectRoot: string, kernelModules: string[]): Map<string, { module: string; implType?: string }[]> {
  const funcToModule = new Map<string, { module: string; implType?: string }[]>();
  
  const addItem = (name: string, module: string, implType?: string) => {
    if (!funcToModule.has(name)) {
      funcToModule.set(name, []);
    }
    funcToModule.get(name)!.push({ module, implType });
  };
  
  for (const moduleName of kernelModules) {
    const modulePath = resolve(projectRoot, 'src', moduleName);
    const rustFiles = findRustFiles(modulePath);
    
    for (const file of rustFiles) {
      // Add functions
      const functions = parseRustFunctions(file);
      for (const func of functions) {
        addItem(func.name, moduleName, func.implType);
      }
      
      // Also add constants and statics
      try {
        const content = readFileSync(file, 'utf-8');
        
        // pub const NAME or pub static NAME
        const constPattern = /(?:pub\s+)?(?:const|static)\s+(\w+)\s*:/g;
        let match;
        while ((match = constPattern.exec(content)) !== null) {
          addItem(match[1], moduleName);
        }
      } catch {
        // Ignore errors
      }
    }
  }
  
  return funcToModule;
}

/** Analyze all test files to build coverage map */
export function analyzeTestCoverage(projectRoot: string, knownModules: string[]): Map<string, Set<string>> {
  // tests/ 重构后，内核测试在 tests/kernel/src/
  const testSrcDir = resolve(projectRoot, 'tests', 'kernel', 'src');
  const testFiles = findRustFiles(testSrcDir);
  
  // Map: module name -> set of covered function names
  const coverage = new Map<string, Set<string>>();
  
  // Initialize all modules
  for (const mod of knownModules) {
    coverage.set(mod, new Set());
  }
  
  // Build lookup maps for accurate coverage attribution
  const typeToModule = buildTypeToModuleMap(projectRoot, knownModules);
  const funcToModule = buildFunctionToModuleMap(projectRoot, knownModules);
  
  for (const testFile of testFiles) {
    try {
      const content = readFileSync(testFile, 'utf-8');
      
      // Extract references from test file
      const referencedModules = extractModuleReferences(content);
      const typesUsed = extractTypesUsed(content);
      const { functions: functionCalls, methodCalls, importedItems } = extractFunctionCalls(content);
      
      // Determine module from file path
      const relativePath = testFile.replace(testSrcDir, '').replace(/^[\/\\]/, '');
      const pathParts = relativePath.split(/[\/\\]/);
      const firstPart = pathParts[0].replace(/\.rs$/, '');
      
      // Skip mock and lib files
      if (firstPart === 'mock' || firstPart === 'lib') {
        continue;
      }
      
      // Combine all referenced items (functions, constants, etc.)
      const allReferencedItems = new Set([...functionCalls, ...importedItems]);
      
      // === Strategy 1: Direct module path mapping ===
      // If test file is in tests/kernel/src/ipc/, it tests the ipc module
      if (knownModules.includes(firstPart)) {
        const modFuncs = coverage.get(firstPart)!;
        allReferencedItems.forEach(fn => modFuncs.add(fn));
        
        // Add methods from methodCalls if the type belongs to this module
        methodCalls.forEach((methods, typeName) => {
          const typeModule = typeToModule.get(typeName);
          if (typeModule === firstPart) {
            methods.forEach(m => modFuncs.add(m));
          }
        });
      }
      
      // === Strategy 2: Use statement module references ===
      // use crate::ipc::signal::... -> tests ipc module
      referencedModules.forEach((_subpaths, mod) => {
        if (knownModules.includes(mod)) {
          const modFuncs = coverage.get(mod)!;
          allReferencedItems.forEach(fn => modFuncs.add(fn));
          
          // Add methods from types belonging to this module
          methodCalls.forEach((methods, typeName) => {
            const typeModule = typeToModule.get(typeName);
            if (typeModule === mod) {
              methods.forEach(m => modFuncs.add(m));
            }
          });
        }
      });
      
      // === Strategy 3: Type-based coverage attribution ===
      // If test uses SignalState, and SignalState is in ipc module, 
      // methods called on SignalState count as ipc coverage
      typesUsed.forEach(typeName => {
        const typeModule = typeToModule.get(typeName);
        if (typeModule && knownModules.includes(typeModule)) {
          const modFuncs = coverage.get(typeModule)!;
          
          // Add all methods called on this type
          const methods = methodCalls.get(typeName);
          if (methods) {
            methods.forEach(m => modFuncs.add(m));
          }
        }
      });
      
      // === Strategy 4: Function/const name reverse lookup ===
      // For standalone functions/constants, look up which module defines them
      allReferencedItems.forEach(itemName => {
        const locations = funcToModule.get(itemName);
        if (locations && locations.length > 0) {
          // If item is unique, attribute to its module
          // If not unique, attribute to all possible modules
          for (const loc of locations) {
            if (knownModules.includes(loc.module)) {
              coverage.get(loc.module)!.add(itemName);
            }
          }
        }
      });
      
      // === Strategy 5: Integration test filename parsing ===
      if (firstPart === 'integration' && pathParts.length > 1) {
        const filename = pathParts[1].replace(/\.rs$/, '');
        // e.g., scheduler_smp.rs -> tests scheduler and smp
        for (const mod of knownModules) {
          if (filename.includes(mod)) {
            const modFuncs = coverage.get(mod)!;
            allReferencedItems.forEach(fn => modFuncs.add(fn));
            
            methodCalls.forEach((methods, typeName) => {
              const typeModule = typeToModule.get(typeName);
              if (typeModule === mod) {
                methods.forEach(m => modFuncs.add(m));
              }
            });
          }
        }
      }
      
    } catch {
      // Ignore errors
    }
  }
  
  return coverage;
}

// =============================================================================
// Main Coverage Calculation
// =============================================================================

/** Calculate coverage statistics for all kernel modules */
export function calculateCoverage(
  projectRoot: string, 
  testResults: TestResult[]
): CoverageStats {
  const stats: CoverageStats = {
    totalFunctions: 0,
    coveredFunctions: 0,
    functionCoveragePct: 0,
    totalLines: 0,
    coveredLines: 0,
    lineCoveragePct: 0,
    totalStatements: 0,
    coveredStatements: 0,
    statementCoveragePct: 0,
    totalBranches: 0,
    coveredBranches: 0,
    branchCoveragePct: 0,
    totalTests: testResults.length,
    passedTests: testResults.filter(t => t.passed).length,
    failedTests: testResults.filter(t => !t.passed).length,
    testPassRate: 0,
    modules: {}
  };
  
  stats.testPassRate = stats.totalTests > 0 
    ? (stats.passedTests / stats.totalTests) * 100 
    : 0;
  
  // Auto-discover kernel modules
  const kernelModules = discoverKernelModules(projectRoot);
  
  // Analyze test files to find actually covered functions per module
  const testCoverage = analyzeTestCoverage(projectRoot, kernelModules);
  
  // Analyze each kernel module
  for (const moduleName of kernelModules) {
    const moduleInfo = analyzeKernelModule(projectRoot, moduleName);
    const moduleTotal = moduleInfo.functions.length;
    
    // Get function names that are covered by tests
    const coveredFuncNames = testCoverage.get(moduleName) || new Set<string>();
    
    // Count actually covered functions by matching names
    let moduleCovered = 0;
    let moduleCoveredLines = 0;
    let moduleCoveredStatements = 0;
    let moduleCoveredBranches = 0;
    
    // Update file-level coverage
    const updatedFiles: FileStats[] = [];
    
    for (const fileStats of moduleInfo.files) {
      const updatedFile = { ...fileStats };
      updatedFile.coveredFunctions = 0;
      updatedFile.coveredLines = 0;
      updatedFile.coveredStatements = 0;
      updatedFile.coveredBranches = 0;
      updatedFile.uncoveredLineNumbers = [];
      
      // Track which lines are covered via function coverage
      const coveredLineSet = new Set<number>();
      
      for (const func of fileStats.functions) {
        // Check if function is covered:
        // 1. Direct name match
        // 2. For methods, check if the impl type is referenced AND the method is called
        let isCovered = coveredFuncNames.has(func.name);
        
        // For common method names (new, default, etc), require more specific matching
        if (!isCovered && func.isMethod && func.implType) {
          // Check if impl type + method combination is covered
          // This is handled by extractFunctionCalls which tracks Type::method
          isCovered = coveredFuncNames.has(func.name);
        }
        
        if (isCovered) {
          updatedFile.coveredFunctions++;
          moduleCovered++;
          
          // Mark all lines in the function as covered
          for (let line = func.lineStart; line <= func.lineEnd; line++) {
            coveredLineSet.add(line);
          }
          
          const funcLines = func.lineEnd - func.lineStart + 1;
          moduleCoveredLines += funcLines;
          updatedFile.coveredLines += funcLines;
        }
      }
      
      // Calculate statement and branch coverage based on function coverage ratio
      const funcCoverageRatio = updatedFile.totalFunctions > 0 
        ? updatedFile.coveredFunctions / updatedFile.totalFunctions 
        : 0;
      
      updatedFile.coveredStatements = Math.round(updatedFile.totalStatements * funcCoverageRatio);
      updatedFile.coveredBranches = Math.round(updatedFile.totalBranches * funcCoverageRatio);
      
      moduleCoveredStatements += updatedFile.coveredStatements;
      moduleCoveredBranches += updatedFile.coveredBranches;
      
      // Calculate uncovered line numbers (lines not in any covered function)
      // For display, only show first/last few to avoid huge lists
      try {
        const content = readFileSync(resolve(projectRoot, 'src', fileStats.filePath), 'utf-8');
        const lines = content.split('\n');
        
        for (let i = 0; i < lines.length; i++) {
          const stripped = lines[i].trim();
          const lineNum = i + 1;
          
          // Only check executable lines
          if (stripped && 
              !stripped.startsWith('//') && 
              !stripped.startsWith('/*') &&
              !stripped.startsWith('*') &&
              !stripped.startsWith('use ') &&
              !stripped.startsWith('mod ') &&
              !stripped.startsWith('#[') &&
              stripped !== '{' && 
              stripped !== '}') {
            if (!coveredLineSet.has(lineNum)) {
              updatedFile.uncoveredLineNumbers.push(lineNum);
            }
          }
        }
      } catch {
        // Ignore errors
      }
      
      updatedFiles.push(updatedFile);
    }
    
    stats.modules[moduleName] = {
      totalFunctions: moduleTotal,
      coveredFunctions: moduleCovered,
      totalLines: moduleInfo.codeLines,
      coveredLines: moduleCoveredLines,
      totalStatements: moduleInfo.totalStatements,
      coveredStatements: moduleCoveredStatements,
      totalBranches: moduleInfo.totalBranches,
      coveredBranches: moduleCoveredBranches,
      coveragePct: moduleTotal > 0 ? (moduleCovered / moduleTotal) * 100 : 0,
      files: updatedFiles
    };
    
    stats.totalFunctions += moduleTotal;
    stats.coveredFunctions += moduleCovered;
    stats.totalLines += moduleInfo.codeLines;
    stats.coveredLines += moduleCoveredLines;
    stats.totalStatements += moduleInfo.totalStatements;
    stats.coveredStatements += moduleCoveredStatements;
    stats.totalBranches += moduleInfo.totalBranches;
    stats.coveredBranches += moduleCoveredBranches;
  }
  
  stats.functionCoveragePct = stats.totalFunctions > 0 
    ? (stats.coveredFunctions / stats.totalFunctions) * 100 
    : 0;
  stats.lineCoveragePct = stats.totalLines > 0 
    ? (stats.coveredLines / stats.totalLines) * 100 
    : 0;
  stats.statementCoveragePct = stats.totalStatements > 0 
    ? (stats.coveredStatements / stats.totalStatements) * 100 
    : 0;
  stats.branchCoveragePct = stats.totalBranches > 0 
    ? (stats.coveredBranches / stats.totalBranches) * 100 
    : 0;
  
  return stats;
}
