<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import DocCard from './components/DocCard.vue';
import SearchBar from './components/SearchBar.vue';

const { t, locale } = useI18n();

// Documentation data will be injected by the build script
const docsData = ref<DocsData | null>(null);

// Theme
const isDark = ref(true);

// Search and filter
const searchQuery = ref('');
const selectedCategory = ref<string>('all');

// Language switching
const toggleLocale = () => {
  locale.value = locale.value === 'en' ? 'zh' : 'en';
  localStorage.setItem('docs-locale', locale.value);
};

// Toggle theme
const toggleTheme = () => {
  isDark.value = !isDark.value;
  document.documentElement.classList.toggle('light-theme', !isDark.value);
  localStorage.setItem('docs-theme', isDark.value ? 'dark' : 'light');
};

// Load data on mount
onMounted(() => {
  // Load theme preference
  const savedTheme = localStorage.getItem('docs-theme');
  if (savedTheme === 'light') {
    isDark.value = false;
    document.documentElement.classList.add('light-theme');
  }

  // Data is embedded in the HTML as a script
  const dataScript = document.getElementById('docs-data');
  if (dataScript) {
    try {
      docsData.value = JSON.parse(dataScript.textContent || '{}');
    } catch (e) {
      console.error('Failed to parse docs data:', e);
    }
  }
});

// Filtered docs
const filteredDocs = computed(() => {
  if (!docsData.value) return [];
  
  let docs = docsData.value.docs;
  
  // Filter by category
  if (selectedCategory.value !== 'all') {
    docs = docs.filter(d => d.category === selectedCategory.value);
  }
  
  // Filter by search query
  if (searchQuery.value) {
    const query = searchQuery.value.toLowerCase();
    docs = docs.filter(d => 
      d.name.toLowerCase().includes(query) ||
      d.description.toLowerCase().includes(query)
    );
  }
  
  return docs;
});

// Group docs by category
const categories = computed(() => {
  return ['all', 'core', 'drivers', 'userspace', 'tools'];
});

const formattedDate = computed(() => {
  if (!docsData.value?.timestamp) return '';
  return new Date(docsData.value.timestamp).toLocaleString(
    locale.value === 'zh' ? 'zh-CN' : 'en-US'
  );
});
</script>

<template>
  <div class="app" :class="{ 'light-theme': !isDark }">
    <header class="header">
      <div class="header-content">
        <div class="header-title">
          <h1>📚 {{ t('title') }}</h1>
          <p class="subtitle">{{ t('subtitle') }}</p>
        </div>
        <div class="header-controls">
          <button @click="toggleTheme" class="btn-icon" :title="isDark ? t('theme.light') : t('theme.dark')">
            {{ isDark ? '☀️' : '🌙' }}
          </button>
          <button @click="toggleLocale" class="btn-lang">
            {{ locale === 'en' ? '中文' : 'EN' }}
          </button>
        </div>
      </div>
    </header>
    
    <main v-if="docsData" class="container">
      <p class="timestamp">{{ t('generated') }}: {{ formattedDate }}</p>
      
      <SearchBar v-model="searchQuery" :placeholder="t('search')" />
      
      <div class="category-tabs">
        <button 
          v-for="cat in categories" 
          :key="cat"
          :class="['tab', { active: selectedCategory === cat }]"
          @click="selectedCategory = cat"
        >
          {{ t(`categories.${cat}`) }}
        </button>
      </div>
      
      <div v-if="filteredDocs.length > 0" class="docs-grid">
        <DocCard 
          v-for="doc in filteredDocs" 
          :key="doc.name" 
          :doc="doc"
        />
      </div>
      
      <div v-else class="no-results">
        {{ t('noResults') }}
      </div>
    </main>
    
    <div v-else class="loading">
      Loading documentation index...
    </div>
    
    <footer class="footer">
      <p>{{ t('footer.generated') }} · <code>{{ t('footer.command') }}</code></p>
    </footer>
  </div>
</template>

<style>
:root {
  --bg-primary: #0f1419;
  --bg-secondary: #1a1f25;
  --bg-tertiary: #252b33;
  --text-primary: #e6e6e6;
  --text-secondary: #a0a0a0;
  --text-muted: #666;
  --accent: #4fc1ff;
  --accent-hover: #66d4ff;
  --border: #2a2f35;
  --shadow: rgba(0, 0, 0, 0.3);
}

.light-theme {
  --bg-primary: #f8f9fa;
  --bg-secondary: #ffffff;
  --bg-tertiary: #f0f1f2;
  --text-primary: #1a1a1a;
  --text-secondary: #555;
  --text-muted: #888;
  --accent: #0066cc;
  --accent-hover: #0077ee;
  --border: #ddd;
  --shadow: rgba(0, 0, 0, 0.1);
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  background-color: var(--bg-primary);
  color: var(--text-primary);
  line-height: 1.6;
}

.app {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.header {
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
  padding: 1.5rem 2rem;
  position: sticky;
  top: 0;
  z-index: 100;
}

.header-content {
  max-width: 1400px;
  margin: 0 auto;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.header-title h1 {
  font-size: 1.75rem;
  font-weight: 700;
  color: var(--text-primary);
}

.subtitle {
  color: var(--text-secondary);
  font-size: 0.9rem;
  margin-top: 0.25rem;
}

.header-controls {
  display: flex;
  gap: 0.75rem;
}

.btn-icon {
  width: 40px;
  height: 40px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-tertiary);
  cursor: pointer;
  font-size: 1.25rem;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.btn-icon:hover {
  border-color: var(--accent);
}

.btn-lang {
  padding: 0.5rem 1rem;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-tertiary);
  color: var(--text-primary);
  cursor: pointer;
  font-weight: 500;
  transition: all 0.2s;
}

.btn-lang:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.container {
  max-width: 1400px;
  margin: 0 auto;
  padding: 2rem;
  flex: 1;
}

.timestamp {
  color: var(--text-muted);
  font-size: 0.85rem;
  margin-bottom: 1.5rem;
}

.category-tabs {
  display: flex;
  gap: 0.5rem;
  margin: 1.5rem 0;
  flex-wrap: wrap;
}

.tab {
  padding: 0.5rem 1.25rem;
  border: 1px solid var(--border);
  border-radius: 20px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 0.9rem;
  transition: all 0.2s;
}

.tab:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.tab.active {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

.docs-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(380px, 1fr));
  gap: 1.25rem;
  margin-top: 1.5rem;
}

.no-results {
  text-align: center;
  padding: 3rem;
  color: var(--text-muted);
  font-size: 1.1rem;
}

.loading {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 50vh;
  color: var(--text-secondary);
  font-size: 1.1rem;
}

.footer {
  border-top: 1px solid var(--border);
  padding: 1.5rem;
  text-align: center;
  color: var(--text-muted);
  font-size: 0.85rem;
}

.footer code {
  background: var(--bg-tertiary);
  padding: 0.2rem 0.5rem;
  border-radius: 4px;
  font-family: "JetBrains Mono", monospace;
}

@media (max-width: 768px) {
  .header-content {
    flex-direction: column;
    gap: 1rem;
    text-align: center;
  }
  
  .docs-grid {
    grid-template-columns: 1fr;
  }
  
  .container {
    padding: 1rem;
  }
}
</style>
