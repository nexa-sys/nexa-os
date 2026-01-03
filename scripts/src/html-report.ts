/**
 * NexaOS Coverage HTML Report Generator
 * 
 * 支持多目标覆盖率报告：
 *   - 总览视图（所有目标聚合）
 *   - 目标视图（每个目标详情）
 *   - 模块视图（模块 → 文件）
 *   - 支持中英文切换
 */

import { readFileSync, existsSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import type { CoverageStats, TestResult } from './coverage.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

/**
 * Generate HTML coverage report
 */
export function generateHtmlReport(stats: CoverageStats, testResults: TestResult[]): string {
  const vueUiPath = resolve(__dirname, '..', 'ui', 'coverage', 'dist', 'index.html');
  
  if (existsSync(vueUiPath)) {
    return generateVueHtmlReport(stats, testResults, vueUiPath);
  }
  
  return generateStaticHtmlReport(stats, testResults);
}

/**
 * Vue UI template injection
 */
function generateVueHtmlReport(
  stats: CoverageStats, 
  testResults: TestResult[],
  templatePath: string
): string {
  let html = readFileSync(templatePath, 'utf-8');
  
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
    targets: stats.targets,
    modules: stats.modules,
    tests: Object.fromEntries(testResults.map(t => [t.name, t.passed]))
  };
  
  const dataScript = `<script id="coverage-data" type="application/json">${JSON.stringify(coverageData)}</script>`;
  html = html.replace('</head>', `${dataScript}\n</head>`);
  
  return html;
}

// Helper functions
const getCoverageClass = (pct: number) => 
  pct >= 70 ? 'coverage-high' : pct >= 40 ? 'coverage-medium' : 'coverage-low';

const getBarColor = (pct: number) =>
  pct >= 70 ? '#00ff88' : pct >= 40 ? '#ffcc00' : '#ff4444';

function formatUncoveredLines(lines: number[], maxLength: number): string {
  if (lines.length === 0) return '';
  
  const ranges: string[] = [];
  let start = lines[0];
  let end = lines[0];
  
  for (let i = 1; i <= lines.length; i++) {
    if (i < lines.length && lines[i] === end + 1) {
      end = lines[i];
    } else {
      ranges.push(start === end ? `${start}` : `${start}-${end}`);
      if (i < lines.length) {
        start = lines[i];
        end = lines[i];
      }
    }
  }
  
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

function getTargetIcon(target: string): string {
  switch (target) {
    case 'kernel': return '🔧';
    case 'userspace': return '📱';
    case 'modules': return '🧩';
    case 'nvm': return '☁️';
    default: return '📦';
  }
}

/**
 * Generate static HTML report with multi-target support
 */
function generateStaticHtmlReport(stats: CoverageStats, testResults: TestResult[]): string {
  const now = new Date().toISOString().replace('T', ' ').slice(0, 19);
  
  // Generate target summary cards
  let targetCards = '';
  for (const [targetName, target] of Object.entries(stats.targets || {})) {
    const fPct = target.functionCoveragePct;
    targetCards += `
      <div class="target-card" data-target="${targetName}">
        <div class="target-header">
          <span class="target-icon">${getTargetIcon(targetName)}</span>
          <span class="target-name">${target.displayName}</span>
        </div>
        <div class="target-stats">
          <div class="stat-row">
            <span data-i18n="tests">Tests</span>: 
            <span class="${target.testPassRate >= 90 ? 'coverage-high' : 'coverage-medium'}">
              ${target.passedTests}/${target.totalTests}
            </span>
          </div>
          <div class="stat-row">
            <span data-i18n="functions">Functions</span>: 
            <span class="${getCoverageClass(fPct)}">${fPct.toFixed(1)}%</span>
          </div>
          <div class="progress-bar">
            <div class="progress-fill" style="width: ${fPct}%; background: ${getBarColor(fPct)};"></div>
          </div>
        </div>
      </div>`;
  }
  
  // Generate module tables by target
  let targetSections = '';
  for (const [targetName, target] of Object.entries(stats.targets || {})) {
    let moduleRows = '';
    
    for (const [modName, mod] of Object.entries(target.modules).sort((a, b) => a[0].localeCompare(b[0]))) {
      const sPct = mod.totalStatements > 0 ? (mod.coveredStatements / mod.totalStatements * 100) : 0;
      const bPct = mod.totalBranches > 0 ? (mod.coveredBranches / mod.totalBranches * 100) : 0;
      const fPct = mod.totalFunctions > 0 ? (mod.coveredFunctions / mod.totalFunctions * 100) : 0;
      const lPct = mod.totalLines > 0 ? (mod.coveredLines / mod.totalLines * 100) : 0;
      
      moduleRows += `
        <tr class="module-row" data-module="${targetName}/${modName}">
          <td><strong>📁 ${modName}</strong></td>
          <td class="${getCoverageClass(sPct)}">${sPct.toFixed(1)}%</td>
          <td class="${getCoverageClass(bPct)}">${bPct.toFixed(1)}%</td>
          <td class="${getCoverageClass(fPct)}">${fPct.toFixed(1)}%</td>
          <td class="${getCoverageClass(lPct)}">${lPct.toFixed(1)}%</td>
          <td style="width: 120px;">
            <div class="progress-bar">
              <div class="progress-fill" style="width: ${fPct}%; background: ${getBarColor(fPct)};"></div>
            </div>
          </td>
          <td></td>
        </tr>`;
      
      // File rows
      if (mod.files) {
        for (const f of mod.files.sort((a, b) => a.filePath.localeCompare(b.filePath))) {
          const fsPct = f.totalStatements > 0 ? (f.coveredStatements / f.totalStatements * 100) : 0;
          const fbPct = f.totalBranches > 0 ? (f.coveredBranches / f.totalBranches * 100) : 0;
          const ffPct = f.totalFunctions > 0 ? (f.coveredFunctions / f.totalFunctions * 100) : 0;
          const flPct = f.totalLines > 0 ? (f.coveredLines / f.totalLines * 100) : 0;
          
          const uncoveredLines = f.uncoveredLineNumbers.length > 10 
            ? formatUncoveredLines(f.uncoveredLineNumbers, 30)
            : f.uncoveredLineNumbers.join(', ');
          
          const fileName = f.filePath.split('/').pop();
          
          moduleRows += `
        <tr class="file-row" data-module="${targetName}/${modName}" style="display: none;">
          <td style="padding-left: 30px;"><span style="color: #666;">📄</span> ${fileName}</td>
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
    
    if (moduleRows) {
      targetSections += `
      <div class="target-section" id="target-${targetName}">
        <h3>${getTargetIcon(targetName)} ${target.displayName}</h3>
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
      </div>`;
    }
  }
  
  // Test results
  let testRows = '';
  for (const test of testResults.sort((a, b) => a.name.localeCompare(b.name))) {
    testRows += `
      <tr>
        <td class="${test.passed ? 'test-pass' : 'test-fail'}">${test.passed ? '✓' : '✗'}</td>
        <td>${test.name}</td>
        <td>${test.target || '-'}</td>
      </tr>`;
  }
  
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>NexaOS Coverage Report</title>
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
    h1 { color: var(--accent); margin: 0; font-size: 24px; }
    h2 { color: var(--accent-secondary); margin-top: 30px; font-size: 18px; }
    h3 { color: var(--text-primary); margin: 20px 0 10px; font-size: 16px; }
    .lang-btn { background: var(--accent); color: var(--bg-primary); border: none; border-radius: 8px; padding: 8px 16px; cursor: pointer; font-weight: 600; }
    
    /* Summary cards */
    .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 15px; margin: 20px 0; }
    .stat-card { background: var(--bg-secondary); border-radius: 8px; padding: 15px; text-align: center; }
    .stat-card h3 { margin: 0; color: var(--text-secondary); font-size: 12px; }
    .stat-card .value { font-size: 28px; font-weight: bold; margin: 8px 0; }
    .stat-card .sub { font-size: 11px; color: var(--text-muted); }
    
    /* Target cards */
    .target-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 15px; margin: 20px 0; }
    .target-card { background: var(--bg-secondary); border-radius: 8px; padding: 15px; cursor: pointer; transition: transform 0.2s, box-shadow 0.2s; }
    .target-card:hover { transform: translateY(-2px); box-shadow: 0 4px 12px rgba(0,217,255,0.2); }
    .target-header { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
    .target-icon { font-size: 24px; }
    .target-name { font-weight: 600; color: var(--accent); }
    .target-stats { font-size: 13px; }
    .stat-row { display: flex; justify-content: space-between; margin: 5px 0; }
    
    /* Coverage classes */
    .coverage-high { color: var(--coverage-high); }
    .coverage-medium { color: var(--coverage-medium); }
    .coverage-low { color: var(--coverage-low); }
    
    /* Tables */
    table { width: 100%; border-collapse: collapse; margin: 15px 0; font-size: 13px; }
    th, td { padding: 8px 10px; text-align: left; border-bottom: 1px solid var(--border); }
    th { background: var(--bg-secondary); color: var(--accent); font-size: 12px; }
    tr:hover { background: var(--bg-secondary); }
    .module-row { cursor: pointer; }
    .module-row:hover { background: #303060; }
    .file-row { background: var(--bg-tertiary); }
    
    /* Progress bars */
    .progress-bar { width: 100%; height: 6px; background: var(--border); border-radius: 3px; overflow: hidden; }
    .progress-fill { height: 100%; transition: width 0.3s; }
    
    /* Test results */
    .test-pass { color: var(--coverage-high); }
    .test-fail { color: var(--coverage-low); }
    
    /* Misc */
    .timestamp { color: var(--text-muted); font-size: 12px; margin-bottom: 20px; }
    .uncovered-lines { font-family: monospace; font-size: 11px; color: #ff8888; max-width: 180px; overflow: hidden; text-overflow: ellipsis; }
    .expand-hint { font-size: 11px; color: var(--text-muted); margin-left: 5px; }
    
    /* Target sections */
    .target-section { margin: 25px 0; padding: 15px; background: var(--bg-tertiary); border-radius: 8px; }
    .target-section h3 { margin-top: 0; }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <h1>🔬 <span data-i18n="title">NexaOS Coverage Report</span></h1>
      <button class="lang-btn" onclick="toggleLang()"><span id="lang-label">中文</span></button>
    </div>
    <div class="timestamp"><span data-i18n="generated">Generated</span>: ${now}</div>
    
    <!-- Aggregate Summary -->
    <div class="summary">
      <div class="stat-card">
        <h3 data-i18n="testPassRate">Test Pass Rate</h3>
        <div class="value ${getCoverageClass(stats.testPassRate)}">${stats.testPassRate.toFixed(1)}%</div>
        <div class="sub">${stats.passedTests} / ${stats.totalTests} <span data-i18n="tests">tests</span></div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="statementCoverage">Statement Coverage</h3>
        <div class="value ${getCoverageClass(stats.statementCoveragePct)}">${stats.statementCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredStatements} / ${stats.totalStatements}</div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="branchCoverage">Branch Coverage</h3>
        <div class="value ${getCoverageClass(stats.branchCoveragePct)}">${stats.branchCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredBranches} / ${stats.totalBranches}</div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="functionCoverage">Function Coverage</h3>
        <div class="value ${getCoverageClass(stats.functionCoveragePct)}">${stats.functionCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredFunctions} / ${stats.totalFunctions}</div>
      </div>
      <div class="stat-card">
        <h3 data-i18n="lineCoverage">Line Coverage</h3>
        <div class="value ${getCoverageClass(stats.lineCoveragePct)}">${stats.lineCoveragePct.toFixed(1)}%</div>
        <div class="sub">${stats.coveredLines} / ${stats.totalLines}</div>
      </div>
    </div>
    
    <!-- Target Overview -->
    <h2>📦 <span data-i18n="targetOverview">Coverage by Target</span></h2>
    <div class="target-grid">
      ${targetCards}
    </div>
    
    <!-- Target Details -->
    <h2>📊 <span data-i18n="detailedCoverage">Detailed Coverage</span> <span class="expand-hint" data-i18n="expandHint">(click module to expand)</span></h2>
    ${targetSections}
    
    <!-- Test Results -->
    <h2>🧪 <span data-i18n="testResults">Test Results</span></h2>
    <table>
      <thead><tr>
        <th data-i18n="status">Status</th>
        <th data-i18n="testName">Test Name</th>
        <th data-i18n="target">Target</th>
      </tr></thead>
      <tbody>${testRows}</tbody>
    </table>
  </div>
  
  <script>
    // i18n
    const i18n = {
      en: {
        title: 'NexaOS Coverage Report',
        generated: 'Generated',
        testPassRate: 'Test Pass Rate',
        statementCoverage: 'Statement Coverage',
        branchCoverage: 'Branch Coverage',
        functionCoverage: 'Function Coverage',
        lineCoverage: 'Line Coverage',
        tests: 'tests',
        functions: 'Functions',
        targetOverview: 'Coverage by Target',
        detailedCoverage: 'Detailed Coverage',
        expandHint: '(click module to expand)',
        file: 'File',
        stmtsPct: '% Stmts',
        branchPct: '% Branch',
        funcsPct: '% Funcs',
        linesPct: '% Lines',
        uncoveredLines: 'Uncovered Lines',
        testResults: 'Test Results',
        status: 'Status',
        testName: 'Test Name',
        target: 'Target'
      },
      zh: {
        title: 'NexaOS 覆盖率报告',
        generated: '生成时间',
        testPassRate: '测试通过率',
        statementCoverage: '语句覆盖率',
        branchCoverage: '分支覆盖率',
        functionCoverage: '函数覆盖率',
        lineCoverage: '行覆盖率',
        tests: '测试',
        functions: '函数',
        targetOverview: '目标覆盖率',
        detailedCoverage: '详细覆盖率',
        expandHint: '（点击模块展开）',
        file: '文件',
        stmtsPct: '语句%',
        branchPct: '分支%',
        funcsPct: '函数%',
        linesPct: '行%',
        uncoveredLines: '未覆盖行号',
        testResults: '测试结果',
        status: '状态',
        testName: '测试名称',
        target: '目标'
      }
    };
    
    let currentLang = localStorage.getItem('coverage-lang') || (navigator.language.startsWith('zh') ? 'zh' : 'en');
    
    function applyLang(lang) {
      document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.getAttribute('data-i18n');
        if (i18n[lang][key]) el.textContent = i18n[lang][key];
      });
      document.getElementById('lang-label').textContent = lang === 'en' ? '中文' : 'EN';
      document.documentElement.lang = lang;
    }
    
    function toggleLang() {
      currentLang = currentLang === 'en' ? 'zh' : 'en';
      localStorage.setItem('coverage-lang', currentLang);
      applyLang(currentLang);
    }
    
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
    
    // Target card click - scroll to section
    document.querySelectorAll('.target-card').forEach(card => {
      card.addEventListener('click', () => {
        const target = card.dataset.target;
        const section = document.getElementById('target-' + target);
        if (section) {
          section.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
      });
    });
  </script>
</body>
</html>`;
}
