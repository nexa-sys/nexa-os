<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold">{{ t('features.title') }}</h1>
        <p class="text-slate-600 dark:text-slate-400">{{ t('features.description') }}</p>
      </div>
      <div class="flex space-x-2">
        <button
          @click="store.saveConfig()"
          class="px-4 py-2 bg-cyan-500 text-white rounded-lg hover:bg-cyan-600 transition-colors"
        >
          {{ t('common.save') }}
        </button>
      </div>
    </div>

    <div v-if="store.loading" class="text-center py-12">
      <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-cyan-500 mx-auto"></div>
      <p class="mt-4 text-slate-600 dark:text-slate-400">{{ t('common.loading') }}</p>
    </div>

    <div v-else-if="store.features" class="space-y-6">
      <FeatureCategory
        v-for="(category, categoryName) in store.features"
        :key="categoryName"
        :name="categoryName"
        :features="category"
        @toggle="(name) => store.toggleFeature(categoryName as any, name)"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useConfigStore } from '@/stores/config';
import FeatureCategory from '@/components/FeatureCategory.vue';

const { t } = useI18n();
const store = useConfigStore();

onMounted(() => {
  store.loadConfig();
});
</script>
