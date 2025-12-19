/**
 * Coverage HTML Report Generator
 * 
 * Generates HTML reports with Vue-based UI and i18n support.
 * Falls back to static HTML if Vue UI is not built.
 */

import { readFileSync, existsSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import type { CoverageStats, TestResult } from './coverage.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

/**
 * Try to use the Vue-based coverage UI, fall back to static HTML if not available
 */
export function generateHtmlReport(stats: CoverageStats, testResults: TestResult[]): string {
  const vueUiPath = resolve(__dirname, '..', 'ui', 'coverage', 'dist', 'index.html');
  
  if (existsSync(vueUiPath)) {
    return generateVueHtmlReport(stats, testResults, vueUiPath);
  }
  
  // Fall back to static HTML
  return generateStaticHtmlReport(stats, testResults);
}

/**
 * Generate HTML report using Vue UI template
 */
function generateVueHtmlReport(
  stats: CoverageStats, 
  testResults: TestResult[],
  templatePath: string
): string {
  let html = readFileSync(templatePath, 'utf-8');
  
  // Prepare coverage data
  const coverageData = {
    timestamp: new Date().toISOString(),
    summary: {
      totalTests: stats.totalTests,
      passedTests: stats.passedTests,
      failedTests: stats.failedTests,
      testPassRate: stats.testPassRate,
      statementCoveragePct: stats.statementCoveragePct,
      branchCoveragePct: stats.branchCoveragePct,
      functionCoveragePct: stats.functionCoveragePct,
      lineCoveragePct: stats.lineCoveragePct,
      totalStatements: stats.totalStatements,
      coveredStatements: stats.coveredStatements,
      totalBranches: stats.totalBranches,
      coveredBranches: stats.coveredBranches,
      totalFunctions: stats.totalFunctions,
      coveredFunctions: stats.coveredFunctions,
      totalLines: stats.totalLines,
      coveredLines: stats.coveredLines,
    },
    modules: stats.modules,
    tests: Object.fromEntries(testResults.map(t => [t.name, t.passed]))
  };
  
  // Inject coverage data as a script tag
  const dataScript = `<script id="coverage-data" type="application/json">${JSON.stringify(coverageData)}</script>`;
  html = html.replace('</head>', `${dataScript}\n</head>`);
  
  return html;
}

/** Format uncovered line ranges to be compact */
function formatUncoveredLines(lines: number[], maxLength: number): string {
  if (lines.length === 0) return '';
  
  // Group consecutive lines into ranges
  const ranges: string[] = [];
  let rangeStart = lines[0];
  let rangeEnd = lines[0];
  
  for (let i = 1; i <= lines.length; i++) {
    if (i < lines.length && lines[i] === rangeEnd + 1) {
      rangeEnd = lines[i];
    } else {
      if (rangeStart === rangeEnd) {
        ranges.push(`${rangeStart}`);
      } else {
        ranges.push(`${rangeStart}-${rangeEnd}`);
      }
      if (i < lines.length) {
        rangeStart = lines[i];
        rangeEnd = lines[i];
      }
    }
  }
  
  // Build result string, truncate if needed
  let result = '';
  for (const range of ranges) {
    if (result.length + range.length + 2 > maxLength) {
      result += ', ...';
      break;
    }
    if (result) result += ', ';
    result += range;
  }
  
  return result;
}

/**
 * Generate static HTML report (fallback when Vue UI is not built)
 */
function generateStaticHtmlReport(stats: CoverageStats, testResults: TestResult[]): string {
  const now = new Date().toISOString().replace('T', ' ').slice(0, 19);
  
  const getCoverageClass = (pct: number) => 
    pct >= 70 ? 'coverage-high' : pct >= 40 ? 'coverage-medium' : 'coverage-low';
  
  const getBarColor = (pct: number) =>
    pct >= 70 ? '#00ff88' : pct >= 40 ? '#ffcc00' : '#ff4444';
  
  let moduleRows = '';
  for (const [name, m] of Object.entries(stats.modules).sort()) {
    const sPct = m.totalStatements > 0 ? (m.coveredStatements / m.totalStatements * 100) : 0;
    const bPct = m.totalBranches > 0 ? (m.coveredBranches / m.totalBranches * 100) : 0;
    const fPct = m.totalFunctions > 0 ? (m.coveredFunctions / m.totalFunctions * 100) : 0;
    const lPct = m.totalLines > 0 ? (m.coveredLines / m.totalLines * 100) : 0;
    
    moduleRows += `
      <tr class="module-row" data-module="${name}">
        <td><strong>📁 ${name}</strong></td>
        <td class="${getCoverageClass(sPct)}">${sPct.toFixed(1)}%</td>
        <td class="${getCoverageClass(bPct)}">${bPct.toFixed(1)}%</td>
        <td class="${getCoverageClass(fPct)}">${fPct.toFixed(1)}%</td>
        <td class="${getCoverageClass(lPct)}">${lPct.toFixed(1)}%</td>
        <td style="width: 150px;">
          <div class="progress-bar">
            <div class="progress-fill" style="width: ${fPct}%; background: ${getBarColor(fPct)};"></div>
          </div>
        </td>
        <td></td>
      </tr>`;
    
    // Add file rows (collapsed by default)
    if (m.files) {
      for (const f of m.files.sort((a, b) => a.filePath.localeCompare(b.filePath))) {
        const fsPct = f.totalStatements > 0 ? (f.coveredStatements / f.totalStatements * 100) : 0;
        const fbPct = f.totalBranches > 0 ? (f.coveredBranches / f.totalBranches * 100) : 0;
        const ffPct = f.totalFunctions > 0 ? (f.coveredFunctions / f.totalFunctions * 100) : 0;
        const flPct = f.totalLines > 0 ? (f.coveredLines / f.totalLines * 100) : 0;
        
        const uncoveredLines = f.uncoveredLineNumbers.length > 10 
          ? formatUncoveredLines(f.uncoveredLineNumbers, 30)
          : f.uncoveredLineNumbers.join(', ');
        
        moduleRows += `
      <tr class="file-row" data-module="${name}" style="display: none;">
        <td style="padding-left: 30px;"><span style="color: #666;">📄</span> ${f.filePath.split('/').pop()}</td>
        <td class="${getCoverageClass(fsPct)}">${fsPct.toFixed(1)}%</td>
        <td class="${getCoverageClass(fbPct)}">${fbPct.toFixed(1)}%</td>
        <td class="${getCoverageClass(ffPct)}">${ffPct.toFixed(1)}%</td>
        <td class="${getCoverageClass(flPct)}">${flPct.toFixed(1)}%</td>
        <td></td>
        <td class="uncovered-lines">${uncoveredLines || '-'}</td>
      </tr>`;
      }
    }
  }
  
  let testRows = '';
  for (const test of testResults.sort((a, b) => a.name.localeCompare(b.name))) {
    testRows += `
      <tr>
        <td class="${test.passed ? 'test-pass' : 'test-fail'}">${test.passed ? '✓' : '✗'}</td>
        <td>${test.name}</td>
      </tr>`;
  }
  
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>NexaOS Kernel Coverage Report</title>
  <style>
    :root {
      --bg-primary: #1a1a2e;
      --bg-secondary: #252545;
      --bg-tertiary: #1e1e3e;
      --text-primary: #eee;
      --text-secondary: #888;
      --text-muted: #666;
      --accent: #00d9ff;
      --accent-secondary: #a0a0ff;
      --border: #333;
      --coverage-high: #00ff88;
      --coverage-medium: #ffcc00;
      --coverage-low: #ff4444;
    }
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: var(--bg-primary); color: var(--text-primary); }
    .container { max-width: 1400px; margin: 0 auto; }
    .header { display: flex; justify-content: space-between; align-items: center; border-bottom: 2px solid var(--accent); padding-bottom: 10px; margin-bottom: 10px; }
    h1 { color: var(--accent); margin: 0; }
    .lang-btn { background: var(--accent); color: var(--bg-primary); border: none; border-radius: 8px; padding: 8px 16px; cursor: pointer; font-weight: 600; }
    h2 { color: var(--accent-secondary); margin-top: 30px; }
    .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 15px; margin: 20px 0; }
    .stat-card { background: var(--bg-secondary); border-radius: 8px; padding: 15px; text-align: center; }
    .stat-card h3 { margin: 0; color: var(--text-secondary); font-size: 12px; }
    .stat-card .value { font-size: 28px; font-weight: bold; margin: 8px 0; }
    .stat-card .sub { font-size: 11px; color: var(--text-muted); }
    .coverage-high { color: var(--coverage-high); }
    .coverage-medium { color: var(--coverage-medium); }
    .coverage-low { color: var(--coverage-low); }
    table { width: 100%; border-collapse: collapse; margin: 20px 0; }
    th, td { padding: 10px; text-align: left; border-bottom: 1px solid var(--border); }
    th { background: var(--bg-secondary); color: var(--accent); font-size: 13px; }
    tr:hover { background: var(--bg-secondary); }
    .module-row { cursor: pointer; }
    .module-row:hover { background: #303060; }
    .file-row { background: var(--bg-tertiary); }
    .progress-bar { width: 100%; height: 6px; background: var(--border); border-radius: 3px; overflow: hidden; }
    .progress-fill { height: 100%; }
    .test-pass { color: var(--coverage-high); }
    .test-fail { color: var(--coverage-low); }
    .timestamp { color: var(--text-muted); font-size: 12px; margin-bottom: 20px; }
    .uncovered-lines { font-family: monospace; font-size: 11px; color: #ff8888; max-width: 200px; overflow: hidden; text-overflow: ellipsis; }
    .expand-hint { font-size: 11px; color: var(--text-muted); margin-left: 5px; }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <h1>🔬 <span data-i18n="title">NexaOS Kernel Test Coverage Report</span></h1>
      <button class="lang-btn" onclick="toggleLang()"><span id="lang-label">中文</span></button>
    </div>
    <div class="timestamp"><span data-i18n="generated">Generated</span>: ${now}</div>
    
    <div class="summary">
      <div class="stat-card">
        <h3 data-i18n="testPassRate">Test Pass Rate</h3>
        <div class="value ${getCoverageClass(stats.testPassRate)}">${stats.testPassRate.toFixed(1)}%</div>
        <div class="sub">${stats.passedTests} / ${stats.totalTests} <span data-i18n="tests">tests</span></div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="statementCoverage">Statement Coverage</h3>
        <div class="value ${getCoverageClass(stats.statementCoveragePct)}">${stats.statementCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredStatements} / ${stats.totalStatements} <span data-i18n="stmts">stmts</span></div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="branchCoverage">Branch Coverage</h3>
        <div class="value ${getCoverageClass(stats.branchCoveragePct)}">${stats.branchCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredBranches} / ${stats.totalBranches} <span data-i18n="branches">branches</span></div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="functionCoverage">Function Coverage</h3>
        <div class="value ${getCoverageClass(stats.functionCoveragePct)}">${stats.functionCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredFunctions} / ${stats.totalFunctions} <span data-i18n="funcs">funcs</span></div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="lineCoverage">Line Coverage</h3>
        <div class="value ${getCoverageClass(stats.lineCoveragePct)}">${stats.lineCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredLines} / ${stats.totalLines} <span data-i18n="lines">lines</span></div>
      </div>
    </div>
    
    <h2>📦 <span data-i18n="moduleTitle">Module Coverage</span> <span class="expand-hint" data-i18n="expandHint">(click to expand)</span></h2>
    <table>
      <thead><tr>
        <th data-i18n="file">File</th>
        <th data-i18n="stmtsPct">% Stmts</th>
        <th data-i18n="branchPct">% Branch</th>
        <th data-i18n="funcsPct">% Funcs</th>
        <th data-i18n="linesPct">% Lines</th>
        <th></th>
        <th data-i18n="uncoveredLines">Uncovered Lines</th>
      </tr></thead>
      <tbody>${moduleRows}</tbody>
    </table>
    
    <h2>🧪 <span data-i18n="testResults">Test Results</span></h2>
    <table>
      <thead><tr><th data-i18n="status">Status</th><th data-i18n="testName">Test Name</th></tr></thead>
      <tbody>${testRows}</tbody>
    </table>
  </div>
  
  <script>
    // i18n support
    const i18n = {
      en: {
        title: 'NexaOS Kernel Test Coverage Report',
        generated: 'Generated',
        testPassRate: 'Test Pass Rate',
        statementCoverage: 'Statement Coverage',
        branchCoverage: 'Branch Coverage',
        functionCoverage: 'Function Coverage',
        lineCoverage: 'Line Coverage',
        tests: 'tests',
        stmts: 'stmts',
        branches: 'branches',
        funcs: 'funcs',
        lines: 'lines',
        moduleTitle: 'Module Coverage',
        expandHint: '(click to expand)',
        file: 'File',
        stmtsPct: '% Stmts',
        branchPct: '% Branch',
        funcsPct: '% Funcs',
        linesPct: '% Lines',
        uncoveredLines: 'Uncovered Lines',
        testResults: 'Test Results',
        status: 'Status',
        testName: 'Test Name'
      },
      zh: {
        title: 'NexaOS 内核测试覆盖率报告',
        generated: '生成时间',
        testPassRate: '测试通过率',
        statementCoverage: '语句覆盖率',
        branchCoverage: '分支覆盖率',
        functionCoverage: '函数覆盖率',
        lineCoverage: '行覆盖率',
        tests: '测试',
        stmts: '语句',
        branches: '分支',
        funcs: '函数',
        lines: '行',
        moduleTitle: '模块覆盖率',
        expandHint: '（点击展开）',
        file: '文件',
        stmtsPct: '语句%',
        branchPct: '分支%',
        funcsPct: '函数%',
        linesPct: '行%',
        uncoveredLines: '未覆盖行号',
        testResults: '测试结果',
        status: '状态',
        testName: '测试名称'
      }
    };
    
    let currentLang = localStorage.getItem('coverage-lang') || (navigator.language.startsWith('zh') ? 'zh' : 'en');
    
    function applyLang(lang) {
      document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.getAttribute('data-i18n');
        if (i18n[lang][key]) {
          el.textContent = i18n[lang][key];
        }
      });
      document.getElementById('lang-label').textContent = lang === 'en' ? '中文' : 'EN';
      document.documentElement.lang = lang;
    }
    
    function toggleLang() {
      currentLang = currentLang === 'en' ? 'zh' : 'en';
      localStorage.setItem('coverage-lang', currentLang);
      applyLang(currentLang);
    }
    
    // Apply initial language
    applyLang(currentLang);
    
    // Module expand/collapse
    document.querySelectorAll('.module-row').forEach(row => {
      row.addEventListener('click', () => {
        const module = row.dataset.module;
        document.querySelectorAll('.file-row[data-module="' + module + '"]').forEach(fileRow => {
          fileRow.style.display = fileRow.style.display === 'none' ? 'table-row' : 'none';
        });
      });
    });
  </script>
</body>
</html>`;
}
