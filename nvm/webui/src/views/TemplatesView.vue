<script setup lang="ts">
import { ref } from 'vue'

const templates = ref([
  { id: '1', name: 'Ubuntu 22.04 LTS', os: 'Linux', size: '2.5 GB', cpu: 2, memory: 2048, disk: 20 },
  { id: '2', name: 'Debian 12', os: 'Linux', size: '1.8 GB', cpu: 2, memory: 1024, disk: 16 },
  { id: '3', name: 'CentOS Stream 9', os: 'Linux', size: '3.2 GB', cpu: 2, memory: 2048, disk: 25 },
  { id: '4', name: 'Windows Server 2022', os: 'Windows', size: '8.5 GB', cpu: 4, memory: 4096, disk: 60 },
])
</script>

<template>
  <div class="p-6 space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Templates</h1>
        <p class="text-dark-400 mt-1">VM templates for quick deployment</p>
      </div>
      <button class="btn-primary flex items-center space-x-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>Create Template</span>
      </button>
    </div>

    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
      <div v-for="template in templates" :key="template.id" class="card p-5 hover:border-accent-500/50 transition-colors cursor-pointer">
        <div class="w-12 h-12 bg-dark-700 rounded-lg flex items-center justify-center mb-4">
          <svg class="w-6 h-6 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7v8a2 2 0 002 2h6M8 7V5a2 2 0 012-2h4.586a1 1 0 01.707.293l4.414 4.414a1 1 0 01.293.707V15a2 2 0 01-2 2h-2M8 7H6a2 2 0 00-2 2v10a2 2 0 002 2h8a2 2 0 002-2v-2"/>
          </svg>
        </div>
        <h3 class="text-white font-medium">{{ template.name }}</h3>
        <p class="text-sm text-dark-400 mt-1">{{ template.os }} • {{ template.size }}</p>
        <div class="mt-4 pt-4 border-t border-dark-600 grid grid-cols-3 gap-2 text-center text-xs text-dark-400">
          <div><span class="text-white font-medium">{{ template.cpu }}</span> vCPU</div>
          <div><span class="text-white font-medium">{{ template.memory }}</span> MB</div>
          <div><span class="text-white font-medium">{{ template.disk }}</span> GB</div>
        </div>
      </div>
    </div>
  </div>
</template>
