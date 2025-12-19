<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';

const { t } = useI18n();

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

const props = defineProps<{
  modules: Record<string, ModuleStats>;
}>();

const expandedModules = ref<Set<string>>(new Set());

const sortedModules = computed(() => {
  return Object.entries(props.modules).sort((a, b) => a[0].localeCompare(b[0]));
});

const toggleModule = (name: string) => {
  if (expandedModules.value.has(name)) {
    expandedModules.value.delete(name);
  } else {
    expandedModules.value.add(name);
  }
};

const getCoverageClass = (pct: number) =>
  pct >= 70 ? 'coverage-high' : pct >= 40 ? 'coverage-medium' : 'coverage-low';

const getBarColor = (pct: number) =>
  pct >= 70 ? 'var(--coverage-high)' : pct >= 40 ? 'var(--coverage-medium)' : 'var(--coverage-low)';

const calcPct = (covered: number, total: number) =>
  total > 0 ? (covered / total * 100) : 0;

const formatUncoveredLines = (lines: number[]): string => {
  if (!lines || lines.length === 0) return '-';
  if (lines.length <= 10) return lines.join(', ');
  
  // Compress ranges
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
  
  const result = ranges.slice(0, 8).join(', ');
  return ranges.length > 8 ? `${result}, ...` : result;
};
</script>

<template>
  <section>
    <h2>📦 {{ t('module.title') }} <span class="expand-hint">{{ t('module.expandHint') }}</span></h2>
    <div class="table-container">
      <table>
        <thead>
          <tr>
            <th>{{ t('module.file') }}</th>
            <th>{{ t('module.stmts') }}</th>
            <th>{{ t('module.branch') }}</th>
            <th>{{ t('module.funcs') }}</th>
            <th>{{ t('module.linesCol') }}</th>
            <th class="progress-col"></th>
            <th>{{ t('module.uncoveredLines') }}</th>
          </tr>
        </thead>
        <tbody>
          <template v-for="[name, m] in sortedModules" :key="name">
            <tr class="module-row" @click="toggleModule(name)">
              <td>
                <span class="expand-icon">{{ expandedModules.has(name) ? '▼' : '▶' }}</span>
                <strong>📁 {{ name }}</strong>
              </td>
              <td :class="getCoverageClass(calcPct(m.coveredStatements, m.totalStatements))">
                {{ calcPct(m.coveredStatements, m.totalStatements).toFixed(1) }}%
              </td>
              <td :class="getCoverageClass(calcPct(m.coveredBranches, m.totalBranches))">
                {{ calcPct(m.coveredBranches, m.totalBranches).toFixed(1) }}%
              </td>
              <td :class="getCoverageClass(calcPct(m.coveredFunctions, m.totalFunctions))">
                {{ calcPct(m.coveredFunctions, m.totalFunctions).toFixed(1) }}%
              </td>
              <td :class="getCoverageClass(calcPct(m.coveredLines, m.totalLines))">
                {{ calcPct(m.coveredLines, m.totalLines).toFixed(1) }}%
              </td>
              <td class="progress-col">
                <div class="progress-bar">
                  <div 
                    class="progress-fill" 
                    :style="{ 
                      width: `${calcPct(m.coveredFunctions, m.totalFunctions)}%`,
                      background: getBarColor(calcPct(m.coveredFunctions, m.totalFunctions))
                    }"
                  ></div>
                </div>
              </td>
              <td></td>
            </tr>
            <template v-if="expandedModules.has(name) && m.files">
              <tr 
                v-for="f in m.files.sort((a, b) => a.filePath.localeCompare(b.filePath))" 
                :key="`${name}-${f.filePath}`"
                class="file-row"
              >
                <td class="file-name">
                  <span class="file-icon">📄</span>
                  {{ f.filePath.split('/').pop() }}
                </td>
                <td :class="getCoverageClass(calcPct(f.coveredStatements, f.totalStatements))">
                  {{ calcPct(f.coveredStatements, f.totalStatements).toFixed(1) }}%
                </td>
                <td :class="getCoverageClass(calcPct(f.coveredBranches, f.totalBranches))">
                  {{ calcPct(f.coveredBranches, f.totalBranches).toFixed(1) }}%
                </td>
                <td :class="getCoverageClass(calcPct(f.coveredFunctions, f.totalFunctions))">
                  {{ calcPct(f.coveredFunctions, f.totalFunctions).toFixed(1) }}%
                </td>
                <td :class="getCoverageClass(calcPct(f.coveredLines, f.totalLines))">
                  {{ calcPct(f.coveredLines, f.totalLines).toFixed(1) }}%
                </td>
                <td></td>
                <td class="uncovered-lines">{{ formatUncoveredLines(f.uncoveredLineNumbers) }}</td>
              </tr>
            </template>
          </template>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.table-container {
  overflow-x: auto;
}

table {
  width: 100%;
  border-collapse: collapse;
  margin: 20px 0;
}

th, td {
  padding: 10px;
  text-align: left;
  border-bottom: 1px solid var(--border);
}

th {
  background: var(--bg-secondary);
  color: var(--accent);
  font-size: 13px;
  font-weight: 600;
  position: sticky;
  top: 0;
}

.module-row {
  cursor: pointer;
  transition: background 0.2s;
}

.module-row:hover {
  background: var(--bg-secondary);
}

.expand-icon {
  display: inline-block;
  width: 16px;
  font-size: 10px;
  color: var(--text-muted);
}

.file-row {
  background: var(--bg-tertiary);
}

.file-name {
  padding-left: 30px !important;
}

.file-icon {
  color: var(--text-muted);
  margin-right: 5px;
}

.progress-col {
  width: 150px;
}

.progress-bar {
  width: 100%;
  height: 6px;
  background: var(--border);
  border-radius: 3px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  transition: width 0.3s ease;
}

.uncovered-lines {
  font-family: monospace;
  font-size: 11px;
  color: var(--coverage-low);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
