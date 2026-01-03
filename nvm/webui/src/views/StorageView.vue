<script setup lang="ts">
import { ref } from 'vue'

interface StoragePool {
  id: string
  name: string
  type: 'local' | 'nfs' | 'iscsi' | 'ceph'
  status: 'online' | 'offline' | 'degraded'
  totalGb: number
  usedGb: number
  path: string
}

const storagePools = ref<StoragePool[]>([
  { id: '1', name: 'local-lvm', type: 'local', status: 'online', totalGb: 500, usedGb: 250, path: '/dev/pve/data' },
  { id: '2', name: 'nfs-share', type: 'nfs', status: 'online', totalGb: 2000, usedGb: 800, path: '192.168.1.100:/export/vms' },
  { id: '3', name: 'ceph-pool', type: 'ceph', status: 'online', totalGb: 10000, usedGb: 3500, path: 'ceph://vm-pool' },
])

const statusColors: Record<string, string> = {
  online: 'bg-green-500',
  offline: 'bg-red-500',
  degraded: 'bg-yellow-500',
}

function getUsagePercent(pool: StoragePool) {
  return Math.round((pool.usedGb / pool.totalGb) * 100)
}
</script>

<template>
  <div class="p-6 space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Storage</h1>
        <p class="text-dark-400 mt-1">Manage storage pools and volumes</p>
      </div>
      <button class="btn-primary flex items-center space-x-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>Add Storage</span>
      </button>
    </div>

    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      <div v-for="pool in storagePools" :key="pool.id" class="card p-5">
        <div class="flex items-start justify-between mb-4">
          <div class="flex items-center space-x-3">
            <div class="w-10 h-10 bg-dark-700 rounded-lg flex items-center justify-center">
              <svg class="w-5 h-5 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"/>
              </svg>
            </div>
            <div>
              <h3 class="text-white font-medium">{{ pool.name }}</h3>
              <p class="text-xs text-dark-500 uppercase">{{ pool.type }}</p>
            </div>
          </div>
          <span class="inline-flex items-center space-x-1">
            <span :class="['w-2 h-2 rounded-full', statusColors[pool.status]]" />
            <span class="text-xs text-dark-400 capitalize">{{ pool.status }}</span>
          </span>
        </div>

        <div class="mb-2">
          <div class="flex justify-between text-sm mb-1">
            <span class="text-dark-400">Usage</span>
            <span class="text-white">{{ pool.usedGb }} / {{ pool.totalGb }} GB</span>
          </div>
          <div class="h-2 bg-dark-700 rounded-full overflow-hidden">
            <div
              :class="['h-full rounded-full', getUsagePercent(pool) > 80 ? 'bg-red-500' : 'bg-accent-500']"
              :style="{ width: `${getUsagePercent(pool)}%` }"
            />
          </div>
        </div>

        <p class="text-xs text-dark-500 truncate">{{ pool.path }}</p>
      </div>
    </div>
  </div>
</template>
