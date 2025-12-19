<template>
  <div class="space-y-6">
    <div>
      <h1 class="text-2xl font-bold">{{ t('build.title') }}</h1>
      <p class="text-slate-600 dark:text-slate-400">{{ t('build.description') }}</p>
    </div>

    <!-- Build Options -->
    <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
      <h2 class="text-lg font-semibold mb-4">{{ t('build.type') }}</h2>
      <div class="flex space-x-4">
        <button
          @click="buildType = 'debug'"
          :class="[
            'px-6 py-3 rounded-lg border-2 transition-all',
            buildType === 'debug' 
              ? 'border-cyan-500 bg-cyan-50 dark:bg-cyan-900/20 text-cyan-700 dark:text-cyan-300' 
              : 'border-slate-200 dark:border-slate-600 hover:border-slate-300'
          ]"
        >
          <div class="flex items-center space-x-2">
            <span class="text-xl">🐛</span>
            <span class="font-medium">{{ t('build.debug') }}</span>
          </div>
        </button>
        <button
          @click="buildType = 'release'"
          :class="[
            'px-6 py-3 rounded-lg border-2 transition-all',
            buildType === 'release' 
              ? 'border-cyan-500 bg-cyan-50 dark:bg-cyan-900/20 text-cyan-700 dark:text-cyan-300' 
              : 'border-slate-200 dark:border-slate-600 hover:border-slate-300'
          ]"
        >
          <div class="flex items-center space-x-2">
            <span class="text-xl">🚀</span>
            <span class="font-medium">{{ t('build.release') }}</span>
          </div>
        </button>
      </div>
    </div>

    <!-- Size Estimates -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
      <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
        <h3 class="text-sm font-medium text-slate-500 dark:text-slate-400 mb-2">{{ t('build.kernelSize') }}</h3>
        <p class="text-3xl font-bold text-slate-900 dark:text-slate-100">~2.1 MB</p>
      </div>
      <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
        <h3 class="text-sm font-medium text-slate-500 dark:text-slate-400 mb-2">{{ t('build.rootfsSize') }}</h3>
        <p class="text-3xl font-bold text-slate-900 dark:text-slate-100">~15 MB</p>
      </div>
    </div>

    <!-- Build Button -->
    <div class="flex justify-center">
      <button
        @click="startBuild"
        :disabled="store.buildStatus === 'building'"
        :class="[
          'px-8 py-4 rounded-xl text-lg font-semibold transition-all',
          store.buildStatus === 'building'
            ? 'bg-slate-300 dark:bg-slate-600 cursor-not-allowed'
            : 'bg-gradient-to-r from-cyan-500 to-blue-600 text-white hover:from-cyan-600 hover:to-blue-700 shadow-lg hover:shadow-xl'
        ]"
      >
        <span v-if="store.buildStatus === 'building'" class="flex items-center space-x-2">
          <svg class="animate-spin h-5 w-5" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span>{{ t('build.building') }}</span>
        </span>
        <span v-else>{{ t('build.start') }}</span>
      </button>
    </div>

    <!-- Build Status -->
    <div v-if="store.buildStatus !== 'idle'" class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
      <div :class="[
        'flex items-center space-x-2 mb-4',
        store.buildStatus === 'success' ? 'text-green-600' : '',
        store.buildStatus === 'failed' ? 'text-red-600' : ''
      ]">
        <span v-if="store.buildStatus === 'success'" class="text-2xl">✅</span>
        <span v-else-if="store.buildStatus === 'failed'" class="text-2xl">❌</span>
        <span class="font-medium">
          {{ store.buildStatus === 'success' ? t('build.success') : store.buildStatus === 'failed' ? t('build.failed') : '' }}
        </span>
      </div>
      
      <!-- Build Output -->
      <div v-if="store.buildOutput.length > 0" class="bg-slate-900 rounded-lg p-4 font-mono text-sm text-slate-100 max-h-96 overflow-y-auto">
        <div v-for="(line, index) in store.buildOutput" :key="index" class="whitespace-pre-wrap">{{ line }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useConfigStore } from '@/stores/config';

const { t } = useI18n();
const store = useConfigStore();
const buildType = ref<'debug' | 'release'>('debug');

async function startBuild() {
  await store.startBuild(buildType.value);
}
</script>
