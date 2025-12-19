<template>
  <div class="space-y-8">
    <!-- Hero Section -->
    <div class="text-center py-12">
      <h1 class="text-4xl font-bold bg-gradient-to-r from-cyan-500 to-blue-600 bg-clip-text text-transparent">
        {{ t('home.welcome') }}
      </h1>
      <p class="mt-4 text-lg text-slate-600 dark:text-slate-400 max-w-2xl mx-auto">
        {{ t('home.description') }}
      </p>
    </div>

    <!-- Quick Navigation -->
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
      <router-link 
        v-for="item in navItems" 
        :key="item.route" 
        :to="item.route"
        class="group p-6 bg-white dark:bg-slate-800 rounded-xl shadow-sm hover:shadow-md transition-all border border-slate-200 dark:border-slate-700"
      >
        <div class="flex items-center space-x-4">
          <div :class="['w-12 h-12 rounded-lg flex items-center justify-center', item.bgColor]">
            <component :is="item.icon" class="w-6 h-6 text-white" />
          </div>
          <div>
            <h3 class="font-semibold text-slate-900 dark:text-slate-100 group-hover:text-cyan-500 transition-colors">
              {{ t(item.label) }}
            </h3>
            <p class="text-sm text-slate-500 dark:text-slate-400">{{ t(item.desc) }}</p>
          </div>
        </div>
      </router-link>
    </div>

    <!-- Presets Section -->
    <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
      <h2 class="text-xl font-semibold mb-4">{{ t('home.presets') }}</h2>
      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <button
          v-for="preset in presets"
          :key="preset.name"
          @click="applyPreset(preset.name)"
          :class="[
            'p-4 rounded-lg border-2 text-left transition-all',
            'hover:border-cyan-500 hover:shadow-md',
            preset.active 
              ? 'border-cyan-500 bg-cyan-50 dark:bg-cyan-900/20' 
              : 'border-slate-200 dark:border-slate-600'
          ]"
        >
          <div class="flex items-center space-x-2 mb-2">
            <span class="text-2xl">{{ preset.icon }}</span>
            <span class="font-medium">{{ t(preset.label) }}</span>
          </div>
          <p class="text-sm text-slate-500 dark:text-slate-400">{{ t(preset.desc) }}</p>
        </button>
      </div>
    </div>

    <!-- Status Cards -->
    <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
      <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium text-slate-500 dark:text-slate-400">{{ t('build.kernelSize') }}</h3>
          <span class="text-2xl font-bold text-slate-900 dark:text-slate-100">~2.1 MB</span>
        </div>
      </div>
      <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium text-slate-500 dark:text-slate-400">{{ t('build.rootfsSize') }}</h3>
          <span class="text-2xl font-bold text-slate-900 dark:text-slate-100">~15 MB</span>
        </div>
      </div>
      <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium text-slate-500 dark:text-slate-400">{{ t('features.title') }}</h3>
          <span class="text-2xl font-bold text-slate-900 dark:text-slate-100">24 / 32</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { h } from 'vue';
import { useI18n } from 'vue-i18n';
import { useConfigStore } from '@/stores/config';

const { t } = useI18n();
const store = useConfigStore();

// Simple SVG icons as functional components
const IconCpu = () => h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24', class: 'w-6 h-6' }, [
  h('path', { 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'stroke-width': '2', d: 'M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z' })
]);

const IconPuzzle = () => h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24', class: 'w-6 h-6' }, [
  h('path', { 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'stroke-width': '2', d: 'M11 4a2 2 0 114 0v1a1 1 0 001 1h3a1 1 0 011 1v3a1 1 0 01-1 1h-1a2 2 0 100 4h1a1 1 0 011 1v3a1 1 0 01-1 1h-3a1 1 0 01-1-1v-1a2 2 0 10-4 0v1a1 1 0 01-1 1H7a1 1 0 01-1-1v-3a1 1 0 00-1-1H4a2 2 0 110-4h1a1 1 0 001-1V7a1 1 0 011-1h3a1 1 0 001-1V4z' })
]);

const IconTerminal = () => h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24', class: 'w-6 h-6' }, [
  h('path', { 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'stroke-width': '2', d: 'M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z' })
]);

const IconPlay = () => h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24', class: 'w-6 h-6' }, [
  h('path', { 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'stroke-width': '2', d: 'M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z' }),
  h('path', { 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'stroke-width': '2', d: 'M21 12a9 9 0 11-18 0 9 9 0 0118 0z' })
]);

const navItems = [
  { route: '/features', label: 'nav.features', desc: 'features.description', icon: IconCpu, bgColor: 'bg-gradient-to-br from-blue-500 to-blue-600' },
  { route: '/modules', label: 'nav.modules', desc: 'modules.description', icon: IconPuzzle, bgColor: 'bg-gradient-to-br from-purple-500 to-purple-600' },
  { route: '/programs', label: 'nav.programs', desc: 'programs.description', icon: IconTerminal, bgColor: 'bg-gradient-to-br from-green-500 to-green-600' },
  { route: '/build', label: 'nav.build', desc: 'build.description', icon: IconPlay, bgColor: 'bg-gradient-to-br from-orange-500 to-orange-600' },
];

const presets = [
  { name: 'full', label: 'presets.full', desc: 'presets.fullDesc', icon: '🚀', active: false },
  { name: 'minimal', label: 'presets.minimal', desc: 'presets.minimalDesc', icon: '📦', active: false },
  { name: 'embedded', label: 'presets.embedded', desc: 'presets.embeddedDesc', icon: '🔧', active: false },
  { name: 'server', label: 'presets.server', desc: 'presets.serverDesc', icon: '🖥️', active: false },
];

async function applyPreset(name: string) {
  await store.applyPreset(name);
}
</script>
