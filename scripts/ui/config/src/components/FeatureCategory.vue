<template>
  <div class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
    <h2 class="text-lg font-semibold mb-4 capitalize">{{ t(`features.${name}`) }}</h2>
    <div class="space-y-3">
      <div
        v-for="(feature, featureName) in features"
        :key="featureName"
        class="flex items-center justify-between p-3 rounded-lg hover:bg-slate-50 dark:hover:bg-slate-700/50 transition-colors"
      >
        <div class="flex-1">
          <div class="flex items-center space-x-2">
            <span class="font-medium text-slate-900 dark:text-slate-100">{{ featureName }}</span>
            <span v-if="feature.required" class="text-xs px-2 py-0.5 bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 rounded">
              {{ t('common.required') }}
            </span>
          </div>
          <p class="text-sm text-slate-500 dark:text-slate-400 mt-0.5">{{ feature.description }}</p>
          <div v-if="feature.dependencies.length > 0" class="flex items-center space-x-1 mt-1">
            <span class="text-xs text-slate-400">{{ t('common.dependencies') }}:</span>
            <span v-for="dep in feature.dependencies" :key="dep" class="text-xs px-1.5 py-0.5 bg-slate-100 dark:bg-slate-700 rounded">
              {{ dep }}
            </span>
          </div>
        </div>
        <button
          @click="emit('toggle', featureName as string)"
          :disabled="feature.required"
          :class="[
            'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-cyan-500 focus:ring-offset-2',
            feature.enabled ? 'bg-cyan-500' : 'bg-slate-200 dark:bg-slate-600',
            feature.required ? 'opacity-50 cursor-not-allowed' : ''
          ]"
        >
          <span
            :class="[
              'pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
              feature.enabled ? 'translate-x-5' : 'translate-x-0'
            ]"
          />
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import type { FeatureCategory } from '@/stores/config';

const { t } = useI18n();

defineProps<{
  name: string;
  features: FeatureCategory;
}>();

const emit = defineEmits<{
  toggle: [name: string];
}>();
</script>
