<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import SummaryCards from './components/SummaryCards.vue';
import ModuleTable from './components/ModuleTable.vue';
import TestResults from './components/TestResults.vue';

const { t, locale } = useI18n();

// Coverage data will be injected by the build script
const coverageData = ref<CoverageData | null>(null);

// Theme
const isDark = ref(true);

// Language switching
const toggleLocale = () => {
  locale.value = locale.value === 'en' ? 'zh' : 'en';
  localStorage.setItem('coverage-locale', locale.value);
};

// Toggle theme
const toggleTheme = () => {
  isDark.value = !isDark.value;
  document.documentElement.classList.toggle('light-theme', !isDark.value);
};

// Load data on mount
onMounted(() => {
  // Data is embedded in the HTML as a script
  const dataScript = document.getElementById('coverage-data');
  if (dataScript) {
    try {
      coverageData.value = JSON.parse(dataScript.textContent || '{}');
    } catch (e) {
      console.error('Failed to parse coverage data:', e);
    }
  }
});

const formattedDate = computed(() => {
  if (!coverageData.value?.timestamp) return '';
  return new Date(coverageData.value.timestamp).toLocaleString(
    locale.value === 'zh' ? 'zh-CN' : 'en-US'
  );
});
</script>

<template>
  <div class="app" :class="{ 'light-theme': !isDark }">
    <header class="header">
      <h1>🔬 {{ t('title') }}</h1>
      <div class="header-controls">
        <button @click="toggleTheme" class="btn-icon" :title="isDark ? t('theme.light') : t('theme.dark')">
          {{ isDark ? '☀️' : '🌙' }}
        </button>
        <button @click="toggleLocale" class="btn-lang">
          {{ locale === 'en' ? '中文' : 'EN' }}
        </button>
      </div>
    </header>
    
    <main v-if="coverageData" class="container">
      <p class="timestamp">{{ t('generated') }}: {{ formattedDate }}</p>
      
      <SummaryCards :summary="coverageData.summary" />
      
      <ModuleTable :modules="coverageData.modules" />
      
      <TestResults :tests="coverageData.tests" />
    </main>
    
    <div v-else class="loading">
      Loading coverage data...
    </div>
  </div>
</template>

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

.light-theme {
  --bg-primary: #f5f5f5;
  --bg-secondary: #fff;
  --bg-tertiary: #fafafa;
  --text-primary: #333;
  --text-secondary: #666;
  --text-muted: #999;
  --accent: #0088cc;
  --accent-secondary: #5050aa;
  --border: #ddd;
  --coverage-high: #00aa55;
  --coverage-medium: #cc9900;
  --coverage-low: #dd3333;
}

* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  background: var(--bg-primary);
  color: var(--text-primary);
  line-height: 1.5;
}

.app {
  min-height: 100vh;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 20px;
  border-bottom: 2px solid var(--accent);
  max-width: 1400px;
  margin: 0 auto;
}

.header h1 {
  color: var(--accent);
  font-size: 1.5rem;
}

.header-controls {
  display: flex;
  gap: 10px;
}

.btn-icon {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 12px;
  cursor: pointer;
  font-size: 16px;
}

.btn-icon:hover {
  background: var(--bg-tertiary);
}

.btn-lang {
  background: var(--accent);
  color: var(--bg-primary);
  border: none;
  border-radius: 8px;
  padding: 8px 16px;
  cursor: pointer;
  font-weight: 600;
  font-size: 14px;
}

.btn-lang:hover {
  opacity: 0.9;
}

.container {
  max-width: 1400px;
  margin: 0 auto;
  padding: 20px;
}

.timestamp {
  color: var(--text-muted);
  font-size: 12px;
  margin-bottom: 20px;
}

.loading {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 50vh;
  color: var(--text-secondary);
}

h2 {
  color: var(--accent-secondary);
  margin-top: 30px;
  margin-bottom: 15px;
}

.expand-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-left: 5px;
  font-weight: normal;
}
</style>
