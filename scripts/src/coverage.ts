/**
 * NexaOS Development Kit - Multi-Target Coverage Analysis
 * 
 * 支持 4 个测试目标的覆盖率分析：
 *   - kernel     (src/)
 *   - userspace  (userspace/)
 *   - modules    (modules/)
 *   - nvm        (nvm/src/)
 * 
 * 生成分层报告：总览 → 目标 → 模块 → 文件
 */

import { readdirSync, readFileSync, existsSync } from 'fs';
import { resolve, join, relative } from 'path';

// =============================================================================
// Types
// =============================================================================

export interface TestResult {
  name: string;
  passed: boolean;
  target?: string;  // kernel, userspace, modules, nvm
}

export interface FileCoverage {
  filePath: string;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  testedFunctions: string[];
  uncoveredFunctions: string[];
  uncoveredLineNumbers: number[];
}

export interface ModuleCoverage {
  name: string;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  files: FileCoverage[];
}

export interface TargetCoverage {
  name: string;
  displayName: string;
  sourcePath: string;
  testPath: string;
  totalTests: number;
  passedTests: number;
  failedTests: number;
  testPassRate: number;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  statementCoveragePct: number;
  branchCoveragePct: number;
  functionCoveragePct: number;
  lineCoveragePct: number;
  modules: Record<string, ModuleCoverage>;
}

export interface CoverageStats {
  // Aggregate stats
  totalTests: number;
  passedTests: number;
  failedTests: number;
  testPassRate: number;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  statementCoveragePct: number;
  branchCoveragePct: number;
  functionCoveragePct: number;
  lineCoveragePct: number;
  
  // Per-target breakdown
  targets: Record<string, TargetCoverage>;
  
  // Legacy: modules for backward compatibility with html-report.ts
  modules: Record<string, ModuleCoverage>;
}

// =============================================================================
// Target Definitions
// =============================================================================

interface TargetDef {
  name: string;
  displayName: string;
  sourcePath: string;
  testPath: string;
  excludeDirs: string[];
}

const TARGET_DEFINITIONS: TargetDef[] = [
  {
    name: 'kernel',
    displayName: '内核 (Kernel)',
    sourcePath: 'src',
    testPath: 'tests/kernel/src',
    excludeDirs: ['target', 'build'],
  },
  {
    name: 'userspace',
    displayName: '用户空间 (Userspace)',
    sourcePath: 'userspace',
    testPath: 'tests/userspace/src',
    excludeDirs: ['target', 'build'],
  },
  {
    name: 'modules',
    displayName: '内核模块 (Modules)',
    sourcePath: 'modules',
    testPath: 'tests/modules/src',
    excludeDirs: ['target'],
  },
  {
    name: 'nvm',
    displayName: 'NVM 平台',
    sourcePath: 'nvm/src',
    testPath: 'tests/nvm/src',
    excludeDirs: ['target', 'build', 'webui'],
  },
];

// =============================================================================
// Source Analysis
// =============================================================================

interface FunctionInfo {
  name: string;
  startLine: number;
  endLine: number;
  isTest: boolean;
  isPublic: boolean;
}

interface SourceAnalysis {
  functions: FunctionInfo[];
  statements: number;
  branches: number;
  lines: number;
  lineNumbers: number[];
}

/**
 * Parse Rust source file and extract function information
 */
function analyzeRustSource(content: string): SourceAnalysis {
  const lines = content.split('\n');
  const functions: FunctionInfo[] = [];
  const codeLines: number[] = [];
  
  let inFunction = false;
  let braceDepth = 0;
  let currentFunc: FunctionInfo | null = null;
  let inMultiLineComment = false;
  let statements = 0;
  let branches = 0;
  
  for (let i = 0; i < lines.length; i++) {
    let line = lines[i];
    const lineNum = i + 1;
    
    // Handle multi-line comments
    if (inMultiLineComment) {
      if (line.includes('*/')) {
        inMultiLineComment = false;
        line = line.substring(line.indexOf('*/') + 2);
      } else {
        continue;
      }
    }
    
    // Check for start of multi-line comment
    if (line.includes('/*') && !line.includes('*/')) {
      inMultiLineComment = true;
      line = line.substring(0, line.indexOf('/*'));
    }
    
    // Remove single-line comments and strings for analysis
    const codeLine = line.replace(/\/\/.*$/, '').replace(/"[^"]*"/g, '""').trim();
    
    // Skip empty lines, pure attributes, and use statements
    if (!codeLine || codeLine.startsWith('//') || 
        (codeLine.startsWith('#[') && !codeLine.includes('fn ')) ||
        codeLine.startsWith('use ') || codeLine.startsWith('extern ')) {
      continue;
    }
    
    // Track code lines
    if (codeLine.length > 0 && !codeLine.startsWith('//')) {
      codeLines.push(lineNum);
    }
    
    // Count statements (lines ending with ; or {)
    if (codeLine.endsWith(';') || codeLine.endsWith('{') || codeLine.endsWith('}')) {
      statements++;
    }
    
    // Count branches
    if (/\b(if|else|match|while|for|loop)\b/.test(codeLine)) {
      branches++;
    }
    
    // Detect function definitions
    const fnMatch = codeLine.match(/^(pub\s+)?(async\s+)?fn\s+(\w+)/);
    if (fnMatch && !inFunction) {
      const isTest = lines.slice(Math.max(0, i - 3), i).some(l => 
        l.includes('#[test]') || l.includes('#[tokio::test]')
      );
      
      currentFunc = {
        name: fnMatch[3],
        startLine: lineNum,
        endLine: lineNum,
        isTest,
        isPublic: !!fnMatch[1],
      };
      inFunction = true;
      braceDepth = 0;
    }
    
    // Track brace depth
    if (inFunction) {
      const openBraces = (codeLine.match(/{/g) || []).length;
      const closeBraces = (codeLine.match(/}/g) || []).length;
      braceDepth += openBraces - closeBraces;
      
      if (braceDepth <= 0 && currentFunc) {
        currentFunc.endLine = lineNum;
        functions.push(currentFunc);
        currentFunc = null;
        inFunction = false;
      }
    }
  }
  
  // Handle unclosed function at EOF
  if (currentFunc) {
    currentFunc.endLine = lines.length;
    functions.push(currentFunc);
  }
  
  return {
    functions,
    statements,
    branches,
    lines: codeLines.length,
    lineNumbers: codeLines,
  };
}

/**
 * Extract tested function names from test file content
 */
function extractTestedFunctions(testContent: string): Set<string> {
  const tested = new Set<string>();
  
  // Pattern: function_name(...) or module::function_name(...)
  const callPattern = /\b([a-z_][a-z0-9_]*)\s*\(/gi;
  const methodPattern = /\.([a-z_][a-z0-9_]*)\s*\(/gi;
  const assertPattern = /assert[!_]?\w*\s*\(\s*([a-z_][a-z0-9_]*)/gi;
  
  let match;
  
  while ((match = callPattern.exec(testContent)) !== null) {
    const name = match[1].toLowerCase();
    // Exclude common test/assert macros and keywords
    if (!['assert', 'assert_eq', 'assert_ne', 'println', 'print', 'format', 
          'vec', 'box', 'some', 'none', 'ok', 'err', 'if', 'for', 'while',
          'match', 'loop', 'fn', 'let', 'mut', 'const', 'static', 'type',
          'impl', 'trait', 'struct', 'enum', 'mod', 'use', 'pub', 'async',
          'await', 'return', 'break', 'continue', 'test', 'cfg'].includes(name)) {
      tested.add(name);
    }
  }
  
  while ((match = methodPattern.exec(testContent)) !== null) {
    tested.add(match[1].toLowerCase());
  }
  
  while ((match = assertPattern.exec(testContent)) !== null) {
    tested.add(match[1].toLowerCase());
  }
  
  return tested;
}

// =============================================================================
// Directory Scanning
// =============================================================================

/**
 * Recursively find all Rust source files in a directory
 */
function findRustFiles(dir: string, excludeDirs: string[] = []): string[] {
  const files: string[] = [];
  
  if (!existsSync(dir)) {
    return files;
  }
  
  const entries = readdirSync(dir, { withFileTypes: true });
  
  for (const entry of entries) {
    const fullPath = join(dir, entry.name);
    
    if (entry.isDirectory()) {
      if (!excludeDirs.includes(entry.name) && !entry.name.startsWith('.')) {
        files.push(...findRustFiles(fullPath, excludeDirs));
      }
    } else if (entry.name.endsWith('.rs') && !entry.name.endsWith('_test.rs')) {
      files.push(fullPath);
    }
  }
  
  return files;
}

/**
 * Find test files in test directory
 */
function findTestFiles(dir: string): string[] {
  const files: string[] = [];
  
  if (!existsSync(dir)) {
    return files;
  }
  
  const entries = readdirSync(dir, { withFileTypes: true });
  
  for (const entry of entries) {
    const fullPath = join(dir, entry.name);
    
    if (entry.isDirectory()) {
      files.push(...findTestFiles(fullPath));
    } else if (entry.name.endsWith('.rs')) {
      files.push(fullPath);
    }
  }
  
  return files;
}

// =============================================================================
// Module Detection
// =============================================================================

/**
 * Detect modules in kernel source tree (src/)
 */
function detectKernelModules(srcDir: string): Map<string, string[]> {
  const modules = new Map<string, string[]>();
  
  if (!existsSync(srcDir)) {
    return modules;
  }
  
  const entries = readdirSync(srcDir, { withFileTypes: true });
  
  for (const entry of entries) {
    if (entry.isDirectory() && !entry.name.startsWith('.') && entry.name !== 'target') {
      const modPath = join(srcDir, entry.name);
      const files = findRustFiles(modPath);
      if (files.length > 0) {
        modules.set(entry.name, files);
      }
    }
  }
  
  // Add root-level files as 'core' module
  const rootFiles = readdirSync(srcDir, { withFileTypes: true })
    .filter(e => e.isFile() && e.name.endsWith('.rs'))
    .map(e => join(srcDir, e.name));
  
  if (rootFiles.length > 0) {
    modules.set('core', rootFiles);
  }
  
  return modules;
}

/**
 * Detect modules in userspace (userspace/)
 */
function detectUserspaceModules(userspaceDir: string): Map<string, string[]> {
  const modules = new Map<string, string[]>();
  
  if (!existsSync(userspaceDir)) {
    return modules;
  }
  
  // Check nrlib
  const nrlibDir = join(userspaceDir, 'nrlib', 'src');
  if (existsSync(nrlibDir)) {
    const files = findRustFiles(nrlibDir);
    if (files.length > 0) {
      modules.set('nrlib', files);
    }
  }
  
  // Check ld-nrlib (dynamic linker)
  const ldNrlibDir = join(userspaceDir, 'ld-nrlib', 'src');
  if (existsSync(ldNrlibDir)) {
    const files = findRustFiles(ldNrlibDir);
    if (files.length > 0) {
      modules.set('ld-nrlib', files);
    }
  }
  
  // Check lib/ (shared libraries)
  const libDir = join(userspaceDir, 'lib');
  if (existsSync(libDir)) {
    for (const entry of readdirSync(libDir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        const srcPath = join(libDir, entry.name, 'src');
        if (existsSync(srcPath)) {
          const files = findRustFiles(srcPath);
          if (files.length > 0) {
            modules.set(`lib/${entry.name}`, files);
          }
        }
      }
    }
  }
  
  // Check programs/
  const programsDir = join(userspaceDir, 'programs');
  if (existsSync(programsDir)) {
    // Scan category dirs (core, user, network, etc.)
    for (const category of readdirSync(programsDir, { withFileTypes: true })) {
      if (category.isDirectory() && category.name !== 'target') {
        const categoryPath = join(programsDir, category.name);
        for (const prog of readdirSync(categoryPath, { withFileTypes: true })) {
          if (prog.isDirectory()) {
            const srcPath = join(categoryPath, prog.name, 'src');
            if (existsSync(srcPath)) {
              const files = findRustFiles(srcPath);
              if (files.length > 0) {
                modules.set(`programs/${category.name}/${prog.name}`, files);
              }
            }
          }
        }
      }
    }
  }
  
  return modules;
}

/**
 * Detect modules in kernel modules (modules/)
 */
function detectKernelModulesDir(modulesDir: string): Map<string, string[]> {
  const modules = new Map<string, string[]>();
  
  if (!existsSync(modulesDir)) {
    return modules;
  }
  
  for (const entry of readdirSync(modulesDir, { withFileTypes: true })) {
    if (entry.isDirectory() && entry.name !== 'target' && !entry.name.startsWith('.')) {
      const srcPath = join(modulesDir, entry.name, 'src');
      if (existsSync(srcPath)) {
        const files = findRustFiles(srcPath);
        if (files.length > 0) {
          modules.set(entry.name, files);
        }
      }
    }
  }
  
  return modules;
}

/**
 * Detect modules in NVM (nvm/src/)
 */
function detectNvmModules(nvmSrcDir: string): Map<string, string[]> {
  const modules = new Map<string, string[]>();
  
  if (!existsSync(nvmSrcDir)) {
    return modules;
  }
  
  // NVM subdirectories
  for (const entry of readdirSync(nvmSrcDir, { withFileTypes: true })) {
    if (entry.isDirectory() && !entry.name.startsWith('.')) {
      const files = findRustFiles(join(nvmSrcDir, entry.name));
      if (files.length > 0) {
        modules.set(entry.name, files);
      }
    }
  }
  
  // Root level files
  const rootFiles = readdirSync(nvmSrcDir, { withFileTypes: true })
    .filter(e => e.isFile() && e.name.endsWith('.rs'))
    .map(e => join(nvmSrcDir, e.name));
  
  if (rootFiles.length > 0) {
    modules.set('core', rootFiles);
  }
  
  return modules;
}

// =============================================================================
// Coverage Calculation
// =============================================================================

/**
 * Analyze coverage for a single target
 */
function analyzeTargetCoverage(
  projectRoot: string,
  targetDef: TargetDef,
  testResults: TestResult[]
): TargetCoverage {
  const sourcePath = resolve(projectRoot, targetDef.sourcePath);
  const testPath = resolve(projectRoot, targetDef.testPath);
  
  // Detect modules based on target
  let moduleMap: Map<string, string[]>;
  
  switch (targetDef.name) {
    case 'kernel':
      moduleMap = detectKernelModules(sourcePath);
      break;
    case 'userspace':
      moduleMap = detectUserspaceModules(sourcePath);
      break;
    case 'modules':
      moduleMap = detectKernelModulesDir(sourcePath);
      break;
    case 'nvm':
      moduleMap = detectNvmModules(sourcePath);
      break;
    default:
      moduleMap = new Map();
  }
  
  // Collect all tested functions from test files
  const testFiles = findTestFiles(testPath);
  const allTestedFunctions = new Set<string>();
  
  for (const testFile of testFiles) {
    try {
      const content = readFileSync(testFile, 'utf-8');
      const tested = extractTestedFunctions(content);
      tested.forEach(fn => allTestedFunctions.add(fn));
    } catch {
      // Ignore read errors
    }
  }
  
  // Analyze each module
  const modules: Record<string, ModuleCoverage> = {};
  
  let totalStatements = 0;
  let coveredStatements = 0;
  let totalBranches = 0;
  let coveredBranches = 0;
  let totalFunctions = 0;
  let coveredFunctions = 0;
  let totalLines = 0;
  let coveredLines = 0;
  
  for (const [moduleName, sourceFiles] of moduleMap) {
    const moduleCov: ModuleCoverage = {
      name: moduleName,
      totalStatements: 0,
      coveredStatements: 0,
      totalBranches: 0,
      coveredBranches: 0,
      totalFunctions: 0,
      coveredFunctions: 0,
      totalLines: 0,
      coveredLines: 0,
      files: [],
    };
    
    for (const sourceFile of sourceFiles) {
      try {
        const content = readFileSync(sourceFile, 'utf-8');
        const analysis = analyzeRustSource(content);
        
        // Calculate which functions are tested
        const nonTestFunctions = analysis.functions.filter(f => !f.isTest);
        const testedFns: string[] = [];
        const untestedFns: string[] = [];
        
        for (const fn of nonTestFunctions) {
          if (allTestedFunctions.has(fn.name.toLowerCase())) {
            testedFns.push(fn.name);
          } else {
            untestedFns.push(fn.name);
          }
        }
        
        // Calculate covered lines (lines in tested functions)
        const testedLineSet = new Set<number>();
        for (const fn of nonTestFunctions) {
          if (allTestedFunctions.has(fn.name.toLowerCase())) {
            for (let i = fn.startLine; i <= fn.endLine; i++) {
              testedLineSet.add(i);
            }
          }
        }
        
        const coveredLineCount = analysis.lineNumbers.filter(ln => testedLineSet.has(ln)).length;
        const uncoveredLineNumbers = analysis.lineNumbers.filter(ln => !testedLineSet.has(ln));
        
        // Estimate statement and branch coverage based on function coverage
        const fnCoverageRatio = nonTestFunctions.length > 0 
          ? testedFns.length / nonTestFunctions.length 
          : 0;
        
        const fileCov: FileCoverage = {
          filePath: relative(projectRoot, sourceFile),
          totalStatements: analysis.statements,
          coveredStatements: Math.round(analysis.statements * fnCoverageRatio),
          totalBranches: analysis.branches,
          coveredBranches: Math.round(analysis.branches * fnCoverageRatio),
          totalFunctions: nonTestFunctions.length,
          coveredFunctions: testedFns.length,
          totalLines: analysis.lines,
          coveredLines: coveredLineCount,
          testedFunctions: testedFns,
          uncoveredFunctions: untestedFns,
          uncoveredLineNumbers,
        };
        
        moduleCov.files.push(fileCov);
        moduleCov.totalStatements += fileCov.totalStatements;
        moduleCov.coveredStatements += fileCov.coveredStatements;
        moduleCov.totalBranches += fileCov.totalBranches;
        moduleCov.coveredBranches += fileCov.coveredBranches;
        moduleCov.totalFunctions += fileCov.totalFunctions;
        moduleCov.coveredFunctions += fileCov.coveredFunctions;
        moduleCov.totalLines += fileCov.totalLines;
        moduleCov.coveredLines += fileCov.coveredLines;
      } catch {
        // Ignore file read errors
      }
    }
    
    if (moduleCov.files.length > 0) {
      modules[moduleName] = moduleCov;
      totalStatements += moduleCov.totalStatements;
      coveredStatements += moduleCov.coveredStatements;
      totalBranches += moduleCov.totalBranches;
      coveredBranches += moduleCov.coveredBranches;
      totalFunctions += moduleCov.totalFunctions;
      coveredFunctions += moduleCov.coveredFunctions;
      totalLines += moduleCov.totalLines;
      coveredLines += moduleCov.coveredLines;
    }
  }
  
  // Filter test results for this target
  const targetTests = testResults.filter(t => 
    !t.target || t.target === targetDef.name
  );
  const passedTests = targetTests.filter(t => t.passed).length;
  const failedTests = targetTests.filter(t => !t.passed).length;
  
  return {
    name: targetDef.name,
    displayName: targetDef.displayName,
    sourcePath: targetDef.sourcePath,
    testPath: targetDef.testPath,
    totalTests: targetTests.length,
    passedTests,
    failedTests,
    testPassRate: targetTests.length > 0 ? (passedTests / targetTests.length) * 100 : 0,
    totalStatements,
    coveredStatements,
    totalBranches,
    coveredBranches,
    totalFunctions,
    coveredFunctions,
    totalLines,
    coveredLines,
    statementCoveragePct: totalStatements > 0 ? (coveredStatements / totalStatements) * 100 : 0,
    branchCoveragePct: totalBranches > 0 ? (coveredBranches / totalBranches) * 100 : 0,
    functionCoveragePct: totalFunctions > 0 ? (coveredFunctions / totalFunctions) * 100 : 0,
    lineCoveragePct: totalLines > 0 ? (coveredLines / totalLines) * 100 : 0,
    modules,
  };
}

/**
 * Calculate coverage for all targets
 */
export function calculateCoverage(
  projectRoot: string,
  testResults: TestResult[],
  targetFilter?: string
): CoverageStats {
  const targets: Record<string, TargetCoverage> = {};
  const allModules: Record<string, ModuleCoverage> = {};
  
  // Aggregate totals
  let totalTests = 0;
  let passedTests = 0;
  let failedTests = 0;
  let totalStatements = 0;
  let coveredStatements = 0;
  let totalBranches = 0;
  let coveredBranches = 0;
  let totalFunctions = 0;
  let coveredFunctions = 0;
  let totalLines = 0;
  let coveredLines = 0;
  
  // Filter targets if specified
  const targetDefs = targetFilter 
    ? TARGET_DEFINITIONS.filter(t => t.name === targetFilter)
    : TARGET_DEFINITIONS;
  
  for (const targetDef of targetDefs) {
    const targetCov = analyzeTargetCoverage(projectRoot, targetDef, testResults);
    targets[targetDef.name] = targetCov;
    
    // Aggregate stats
    totalTests += targetCov.totalTests;
    passedTests += targetCov.passedTests;
    failedTests += targetCov.failedTests;
    totalStatements += targetCov.totalStatements;
    coveredStatements += targetCov.coveredStatements;
    totalBranches += targetCov.totalBranches;
    coveredBranches += targetCov.coveredBranches;
    totalFunctions += targetCov.totalFunctions;
    coveredFunctions += targetCov.coveredFunctions;
    totalLines += targetCov.totalLines;
    coveredLines += targetCov.coveredLines;
    
    // Flatten modules for backward compatibility
    for (const [modName, modCov] of Object.entries(targetCov.modules)) {
      const prefixedName = `${targetDef.name}/${modName}`;
      allModules[prefixedName] = modCov;
    }
  }
  
  return {
    totalTests,
    passedTests,
    failedTests,
    testPassRate: totalTests > 0 ? (passedTests / totalTests) * 100 : 0,
    totalStatements,
    coveredStatements,
    totalBranches,
    coveredBranches,
    totalFunctions,
    coveredFunctions,
    totalLines,
    coveredLines,
    statementCoveragePct: totalStatements > 0 ? (coveredStatements / totalStatements) * 100 : 0,
    branchCoveragePct: totalBranches > 0 ? (coveredBranches / totalBranches) * 100 : 0,
    functionCoveragePct: totalFunctions > 0 ? (coveredFunctions / totalFunctions) * 100 : 0,
    lineCoveragePct: totalLines > 0 ? (coveredLines / totalLines) * 100 : 0,
    targets,
    modules: allModules,
  };
}

/**
 * Get available targets
 */
export function getAvailableTargets(): { name: string; displayName: string }[] {
  return TARGET_DEFINITIONS.map(t => ({
    name: t.name,
    displayName: t.displayName,
  }));
}
