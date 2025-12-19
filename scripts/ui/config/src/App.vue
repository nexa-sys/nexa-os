<template>
  <div class="min-h-screen bg-slate-50 dark:bg-slate-900 text-slate-900 dark:text-slate-100">
    <!-- Header -->
    <header class="sticky top-0 z-50 bg-white/80 dark:bg-slate-800/80 backdrop-blur-sm border-b border-slate-200 dark:border-slate-700">
      <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div class="flex items-center justify-between h-16">
          <div class="flex items-center space-x-3">
            <div class="w-8 h-8 rounded-lg bg-gradient-to-br from-cyan-500 to-blue-600 flex items-center justify-center">
              <span class="text-white font-bold text-sm">N</span>
            </div>
            <h1 class="text-xl font-semibold">{{ t('title') }}</h1>
          </div>
          
          <div class="flex items-center space-x-4">
            <!-- Language Toggle -->
            <button 
              @click="toggleLocale" 
              class="px-3 py-1.5 text-sm rounded-md bg-slate-100 dark:bg-slate-700 hover:bg-slate-200 dark:hover:bg-slate-600 transition-colors"
            >
              {{ locale === 'en' ? '中文' : 'English' }}
            </button>
            
            <!-- Dark Mode Toggle -->
            <button 
              @click="toggleDark" 
              class="p-2 rounded-md hover:bg-slate-100 dark:hover:bg-slate-700 transition-colors"
            >
              <svg v-if="isDark" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
              </svg>
              <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </header>

    <!-- Main Content -->
    <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
      <router-view />
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';

const { t, locale } = useI18n();
const isDark = ref(false);

onMounted(() => {
  isDark.value = document.documentElement.classList.contains('dark') ||
    window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (isDark.value) {
    document.documentElement.classList.add('dark');
  }
});

function toggleDark() {
  isDark.value = !isDark.value;
  document.documentElement.classList.toggle('dark');
}

function toggleLocale() {
  locale.value = locale.value === 'en' ? 'zh' : 'en';
}
</script>
