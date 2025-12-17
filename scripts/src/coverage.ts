/**
 * Coverage Analysis Module
 * 
 * Analyzes test files to determine actual function coverage by parsing
 * Rust source code and matching function calls in tests.
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
}

export interface ModuleStats {
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  coveragePct: number;
}

export interface CoverageStats {
  totalFunctions: number;
  coveredFunctions: number;
  functionCoveragePct: number;
  totalLines: number;
  coveredLines: number;
  lineCoveragePct: number;
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

/** Parse Rust file and extract function definitions */
export function parseRustFunctions(filePath: string): FunctionInfo[] {
  const functions: FunctionInfo[] = [];
  
  try {
    const content = readFileSync(filePath, 'utf-8');
    const lines = content.split('\n');
    
    const fnPattern = /^\s*(pub\s+)?(async\s+)?fn\s+(\w+)/;
    let inFunction = false;
    let braceDepth = 0;
    let currentFn: FunctionInfo | null = null;
    
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const trimmed = line.trim();
      
      const fnMatch = trimmed.match(fnPattern);
      if (fnMatch && !inFunction) {
        currentFn = {
          name: fnMatch[3],
          filePath,
          lineStart: i + 1,
          lineEnd: i + 1,
          isPub: !!fnMatch[1]
        };
        
        inFunction = true;
        braceDepth = (line.match(/{/g) || []).length - (line.match(/}/g) || []).length;
        
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
        braceDepth += (line.match(/{/g) || []).length - (line.match(/}/g) || []).length;
        
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

/** Analyze a kernel module to get all functions and line counts */
export function analyzeKernelModule(projectRoot: string, moduleName: string): { 
  functions: FunctionInfo[], 
  totalLines: number, 
  codeLines: number 
} {
  const modulePath = resolve(projectRoot, 'src', moduleName);
  const result = { functions: [] as FunctionInfo[], totalLines: 0, codeLines: 0 };
  
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
    'new', 'default', 'clone', 'drop', 'from', 'into', 'try_from',
    'len', 'is_empty', 'push', 'pop', 'get', 'set', 'insert', 'remove',
    'iter', 'map', 'filter', 'collect', 'unwrap', 'expect', 'ok', 'err',
  ]);
  return keywords.has(word);
}

/** Extract all module references from test file (use crate::module::...) */
function extractModuleReferences(content: string): Set<string> {
  const modules = new Set<string>();
  
  // Match: use crate::module_name::...
  const usePattern = /use\s+crate::(\w+)/g;
  let match;
  while ((match = usePattern.exec(content)) !== null) {
    modules.add(match[1]);
  }
  
  // Match: crate::module_name::func
  const crateCallPattern = /crate::(\w+)::/g;
  while ((match = crateCallPattern.exec(content)) !== null) {
    modules.add(match[1]);
  }
  
  return modules;
}

/** Extract function calls from test file content */
function extractFunctionCalls(content: string): Set<string> {
  const calls = new Set<string>();
  
  // Match patterns for function calls AND function references
  const patterns = [
    // module::submod::func( or module::submod::func - capture func
    /(\w+)::(\w+)::(\w+)\s*[<(;,\s\n]/g,
    // module::func( or module::func or Type::method
    /(\w+)::(\w+)\s*[<(;,\s\n]/g,
    // .method( 
    /\.(\w+)\s*[<(]/g,
    // standalone func( - but not keywords
    /(?<![:\.\w])(\w+)\s*\(/g,
    // use statements: use crate::module::{func1, func2}
    /use\s+crate::\w+(?:::\w+)*::\{([^}]+)\}/g,
    // use statements: use crate::module::func
    /use\s+crate::\w+(?:::\w+)*::(\w+)\s*;/g,
  ];
  
  for (const pattern of patterns) {
    let match;
    while ((match = pattern.exec(content)) !== null) {
      // Handle use statement with braces: extract all items
      if (pattern.source.includes('{')) {
        const items = match[1].split(',').map(s => s.trim());
        for (const item of items) {
          const name = item.split(' ')[0]; // Handle "func as alias"
          if (name && !isRustKeyword(name) && name.length > 1) {
            calls.add(name);
          }
        }
        continue;
      }
      
      // Get all captured groups (function names)
      for (let i = 1; i < match.length; i++) {
        const funcName = match[i];
        if (funcName && !isRustKeyword(funcName) && funcName.length > 1) {
          calls.add(funcName);
        }
      }
    }
  }
  
  return calls;
}

/** Analyze all test files to build coverage map */
export function analyzeTestCoverage(projectRoot: string, knownModules: string[]): Map<string, Set<string>> {
  const testSrcDir = resolve(projectRoot, 'tests', 'src');
  const testFiles = findRustFiles(testSrcDir);
  
  // Map: module name -> set of covered function names
  const coverage = new Map<string, Set<string>>();
  
  // Initialize all modules
  for (const mod of knownModules) {
    coverage.set(mod, new Set());
  }
  
  for (const testFile of testFiles) {
    try {
      const content = readFileSync(testFile, 'utf-8');
      
      // Find which modules this test file references via `use crate::module`
      const referencedModules = extractModuleReferences(content);
      const functionCalls = extractFunctionCalls(content);
      
      // Also determine module from file path
      const relativePath = testFile.replace(testSrcDir, '').replace(/^[\/\\]/, '');
      const pathParts = relativePath.split(/[\/\\]/);
      const firstPart = pathParts[0].replace(/\.rs$/, '');
      
      // Determine target modules for this test file
      const targetModules = new Set<string>();
      
      // PRIMARY: Add module from test file's directory/name if it's a known module
      // This is the main source of truth - test files named after modules test those modules
      if (knownModules.includes(firstPart)) {
        targetModules.add(firstPart);
      }
      
      // SECONDARY: Add modules referenced via use statements
      // This helps catch cross-module tests
      referencedModules.forEach(m => {
        if (knownModules.includes(m)) {
          targetModules.add(m);
        }
      });
      
      // For integration tests, try to extract module from filename
      if (firstPart === 'integration' && pathParts.length > 1) {
        const filename = pathParts[1].replace(/\.rs$/, '');
        // e.g., scheduler_smp.rs -> scheduler
        for (const mod of knownModules) {
          if (filename.startsWith(mod)) {
            targetModules.add(mod);
          }
        }
        // Also check referenced modules from integration tests
        referencedModules.forEach(m => {
          if (knownModules.includes(m)) {
            targetModules.add(m);
          }
        });
      }
      
      // For mock directory, don't count as coverage for any real module
      if (firstPart === 'mock' || firstPart === 'lib') {
        continue;
      }
      
      // Add function calls to all target modules
      for (const mod of targetModules) {
        const modCalls = coverage.get(mod)!;
        functionCalls.forEach(fn => modCalls.add(fn));
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
    
    for (const func of moduleInfo.functions) {
      // Check if this function is called in tests
      const isCovered = coveredFuncNames.has(func.name);
      
      if (isCovered) {
        moduleCovered++;
        moduleCoveredLines += (func.lineEnd - func.lineStart + 1);
      }
    }
    
    stats.modules[moduleName] = {
      totalFunctions: moduleTotal,
      coveredFunctions: moduleCovered,
      totalLines: moduleInfo.codeLines,
      coveredLines: moduleCoveredLines,
      coveragePct: moduleTotal > 0 ? (moduleCovered / moduleTotal) * 100 : 0
    };
    
    stats.totalFunctions += moduleTotal;
    stats.coveredFunctions += moduleCovered;
    stats.totalLines += moduleInfo.codeLines;
    stats.coveredLines += moduleCoveredLines;
  }
  
  stats.functionCoveragePct = stats.totalFunctions > 0 
    ? (stats.coveredFunctions / stats.totalFunctions) * 100 
    : 0;
  stats.lineCoveragePct = stats.totalLines > 0 
    ? (stats.coveredLines / stats.totalLines) * 100 
    : 0;
  
  return stats;
}
