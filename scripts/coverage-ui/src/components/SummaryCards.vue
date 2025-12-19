<script setup lang="ts">
import { useI18n } from 'vue-i18n';

const { t } = useI18n();

const props = defineProps<{
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
}>();

const getCoverageClass = (pct: number) =>
  pct >= 70 ? 'coverage-high' : pct >= 40 ? 'coverage-medium' : 'coverage-low';
</script>

<template>
  <div class="summary-cards">
    <div class="stat-card">
      <h3>{{ t('summary.testPassRate') }}</h3>
      <div class="value" :class="getCoverageClass(summary.testPassRate)">
        {{ summary.testPassRate.toFixed(1) }}%
      </div>
      <div class="sub">{{ summary.passedTests }} / {{ summary.totalTests }} {{ t('summary.tests') }}</div>
    </div>
    
    <div class="stat-card">
      <h3>{{ t('summary.statementCoverage') }}</h3>
      <div class="value" :class="getCoverageClass(summary.statementCoveragePct)">
        {{ summary.statementCoveragePct.toFixed(1) }}%
      </div>
      <div class="sub">{{ summary.coveredStatements }} / {{ summary.totalStatements }} {{ t('summary.stmts') }}</div>
    </div>
    
    <div class="stat-card">
      <h3>{{ t('summary.branchCoverage') }}</h3>
      <div class="value" :class="getCoverageClass(summary.branchCoveragePct)">
        {{ summary.branchCoveragePct.toFixed(1) }}%
      </div>
      <div class="sub">{{ summary.coveredBranches }} / {{ summary.totalBranches }} {{ t('summary.branches') }}</div>
    </div>
    
    <div class="stat-card">
      <h3>{{ t('summary.functionCoverage') }}</h3>
      <div class="value" :class="getCoverageClass(summary.functionCoveragePct)">
        {{ summary.functionCoveragePct.toFixed(1) }}%
      </div>
      <div class="sub">{{ summary.coveredFunctions }} / {{ summary.totalFunctions }} {{ t('summary.funcs') }}</div>
    </div>
    
    <div class="stat-card">
      <h3>{{ t('summary.lineCoverage') }}</h3>
      <div class="value" :class="getCoverageClass(summary.lineCoveragePct)">
        {{ summary.lineCoveragePct.toFixed(1) }}%
      </div>
      <div class="sub">{{ summary.coveredLines }} / {{ summary.totalLines }} {{ t('summary.lines') }}</div>
    </div>
  </div>
</template>

<style scoped>
.summary-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 15px;
  margin: 20px 0;
}

.stat-card {
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 15px;
  text-align: center;
  transition: transform 0.2s, box-shadow 0.2s;
}

.stat-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.stat-card h3 {
  margin: 0;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 500;
}

.stat-card .value {
  font-size: 28px;
  font-weight: bold;
  margin: 8px 0;
}

.stat-card .sub {
  font-size: 11px;
  color: var(--text-muted);
}

.coverage-high {
  color: var(--coverage-high);
}

.coverage-medium {
  color: var(--coverage-medium);
}

.coverage-low {
  color: var(--coverage-low);
}
</style>
