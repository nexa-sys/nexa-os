<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold">{{ t('modules.title') }}</h1>
        <p class="text-slate-600 dark:text-slate-400">{{ t('modules.description') }}</p>
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

    <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      <div
        v-for="(config, name) in store.modules"
        :key="name"
        class="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-4"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center space-x-3">
            <div :class="[
              'w-10 h-10 rounded-lg flex items-center justify-center',
              config.enabled ? 'bg-green-100 dark:bg-green-900/30' : 'bg-slate-100 dark:bg-slate-700'
            ]">
              <span class="text-lg">{{ getModuleIcon(name as string) }}</span>
            </div>
            <div>
              <h3 class="font-medium text-slate-900 dark:text-slate-100">{{ name }}</h3>
              <p class="text-sm text-slate-500 dark:text-slate-400">{{ getModuleCategory(name as string) }}</p>
            </div>
          </div>
          <button
            @click="store.toggleModule(name as string)"
            :class="[
              'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-cyan-500 focus:ring-offset-2',
              config.enabled ? 'bg-cyan-500' : 'bg-slate-200 dark:bg-slate-600'
            ]"
          >
            <span
              :class="[
                'pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
                config.enabled ? 'translate-x-5' : 'translate-x-0'
              ]"
            />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useConfigStore } from '@/stores/config';

const { t } = useI18n();
const store = useConfigStore();

onMounted(() => {
  store.loadConfig();
});

function getModuleIcon(name: string): string {
  const icons: Record<string, string> = {
    ext2: '📁', ext3: '📁', ext4: '📁',
    virtio_blk: '💾', ide: '💿', ahci: '💿', nvme: '💿',
    e1000: '🌐', virtio_net: '🌐',
    swap: '🔄'
  };
  return icons[name] || '📦';
}

function getModuleCategory(name: string): string {
  if (['ext2', 'ext3', 'ext4'].includes(name)) return 'Filesystem';
  if (['virtio_blk', 'ide', 'ahci', 'nvme'].includes(name)) return 'Block Device';
  if (['e1000', 'virtio_net'].includes(name)) return 'Network';
  if (name === 'swap') return 'Memory';
  return 'Other';
}
</script>
