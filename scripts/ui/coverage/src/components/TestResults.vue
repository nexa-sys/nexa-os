<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';

const { t } = useI18n();

const props = defineProps<{
  tests: Record<string, boolean>;
}>();

const filterText = ref('');
const filterStatus = ref<'all' | 'passed' | 'failed'>('all');

const sortedTests = computed(() => {
  return Object.entries(props.tests)
    .sort((a, b) => a[0].localeCompare(b[0]))
    .filter(([name, passed]) => {
      // Filter by status
      if (filterStatus.value === 'passed' && !passed) return false;
      if (filterStatus.value === 'failed' && passed) return false;
      
      // Filter by text
      if (filterText.value) {
        return name.toLowerCase().includes(filterText.value.toLowerCase());
      }
      return true;
    });
});

const testStats = computed(() => {
  const total = Object.keys(props.tests).length;
  const passed = Object.values(props.tests).filter(v => v).length;
  return { total, passed, failed: total - passed };
});
</script>

<template>
  <section>
    <h2>🧪 {{ t('tests.title') }}</h2>
    
    <div class="test-controls">
      <input 
        v-model="filterText"
        type="text" 
        :placeholder="t('tests.filter')"
        class="filter-input"
      />
      <div class="status-filters">
        <button 
          :class="{ active: filterStatus === 'all' }"
          @click="filterStatus = 'all'"
        >
          {{ t('tests.all') }} ({{ testStats.total }})
        </button>
        <button 
          :class="{ active: filterStatus === 'passed' }"
          @click="filterStatus = 'passed'"
        >
          ✓ {{ t('tests.passedOnly') }} ({{ testStats.passed }})
        </button>
        <button 
          :class="{ active: filterStatus === 'failed' }"
          @click="filterStatus = 'failed'"
        >
          ✗ {{ t('tests.failedOnly') }} ({{ testStats.failed }})
        </button>
      </div>
    </div>
    
    <div class="table-container">
      <table>
        <thead>
          <tr>
            <th class="status-col">{{ t('tests.status') }}</th>
            <th>{{ t('tests.testName') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="[name, passed] in sortedTests" :key="name">
            <td :class="passed ? 'test-pass' : 'test-fail'">
              {{ passed ? '✓' : '✗' }}
            </td>
            <td>{{ name }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    
    <p v-if="sortedTests.length === 0" class="no-results">
      No tests match the current filter.
    </p>
  </section>
</template>

<style scoped>
.test-controls {
  display: flex;
  flex-wrap: wrap;
  gap: 15px;
  margin-bottom: 15px;
}

.filter-input {
  flex: 1;
  min-width: 200px;
  padding: 10px 15px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 14px;
}

.filter-input:focus {
  outline: none;
  border-color: var(--accent);
}

.filter-input::placeholder {
  color: var(--text-muted);
}

.status-filters {
  display: flex;
  gap: 5px;
}

.status-filters button {
  padding: 8px 16px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-secondary);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 13px;
  transition: all 0.2s;
}

.status-filters button:hover {
  background: var(--bg-tertiary);
}

.status-filters button.active {
  background: var(--accent);
  color: var(--bg-primary);
  border-color: var(--accent);
}

.table-container {
  max-height: 500px;
  overflow-y: auto;
}

table {
  width: 100%;
  border-collapse: collapse;
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

.status-col {
  width: 80px;
  text-align: center;
}

tr:hover {
  background: var(--bg-secondary);
}

.test-pass {
  color: var(--coverage-high);
  text-align: center;
  font-weight: bold;
}

.test-fail {
  color: var(--coverage-low);
  text-align: center;
  font-weight: bold;
}

.no-results {
  color: var(--text-muted);
  text-align: center;
  padding: 20px;
}
</style>
