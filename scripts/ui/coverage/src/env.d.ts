/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

interface CoverageData {
  timestamp: string;
  summary: {
    totalTests: number;
    passedTests: number;
    failedTests: number;
    testPassRate: number;
    statementCoveragePct: number;
    branchCoveragePct: number;
    functionCoveragePct: number;
    lineCoveragePct: number;
    totalStatements: number;
    coveredStatements: number;
    totalBranches: number;
    coveredBranches: number;
    totalFunctions: number;
    coveredFunctions: number;
    totalLines: number;
    coveredLines: number;
  };
  modules: Record<string, ModuleStats>;
  tests: Record<string, boolean>;
}

interface ModuleStats {
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  coveragePct: number;
  files: FileStats[];
}

interface FileStats {
  filePath: string;
  totalFunctions: number;
  coveredFunctions: number;
  totalLines: number;
  coveredLines: number;
  totalStatements: number;
  coveredStatements: number;
  totalBranches: number;
  coveredBranches: number;
  uncoveredLineNumbers: number[];
}
