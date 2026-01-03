<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { api } from '../api'

interface StoragePool {
  id: string
  name: string
  type: 'local' | 'nfs' | 'iscsi' | 'ceph' | 'dir'
  status: 'online' | 'offline' | 'degraded'
  totalGb: number
  usedGb: number
  path: string
}

interface Volume {
  id: string
  name: string
  pool: string
  sizeGb: number
  allocatedGb: number
  format: string
  vmId: string | null
  vmName?: string
  createdAt: string
}

const activeTab = ref<'pools' | 'volumes'>('pools')
const storagePools = ref<StoragePool[]>([])
const volumes = ref<Volume[]>([])
const loading = ref(true)
const loadingVolumes = ref(false)
const error = ref<string | null>(null)

async function fetchStoragePools() {
  try {
    loading.value = true
    error.value = null
    const response = await api.get('/storage/pools')
    if (response.data.success && response.data.data) {
      storagePools.value = response.data.data.map((pool: any) => ({
        id: pool.id,
        name: pool.name,
        type: pool.pool_type || 'dir',
        status: pool.status || 'online',
        totalGb: Math.round((pool.total_bytes || 0) / (1024 * 1024 * 1024)),
        usedGb: Math.round((pool.used_bytes || 0) / (1024 * 1024 * 1024)),
        path: pool.path || ''
      }))
    }
  } catch (e: any) {
    error.value = e.message || 'Failed to load storage pools'
    console.error('Failed to fetch storage pools:', e)
  } finally {
    loading.value = false
  }
}

async function fetchVolumes() {
  try {
    loadingVolumes.value = true
    const response = await api.get('/storage/volumes')
    if (response.data.success && response.data.data) {
      volumes.value = response.data.data.map((vol: any) => ({
        id: vol.id,
        name: vol.name,
        pool: vol.pool || 'local',
        sizeGb: Math.round((vol.size_bytes || 0) / (1024 * 1024 * 1024)),
        allocatedGb: Math.round((vol.allocated_bytes || 0) / (1024 * 1024 * 1024)),
        format: vol.format || 'qcow2',
        vmId: vol.vm_id || null,
        createdAt: vol.created_at ? new Date(vol.created_at * 1000).toLocaleDateString() : '-'
      }))
    }
  } catch (e: any) {
    console.error('Failed to fetch volumes:', e)
  } finally {
    loadingVolumes.value = false
  }
}

onMounted(() => {
  fetchStoragePools()
  fetchVolumes()
})

const statusColors: Record<string, string> = {
  online: 'bg-green-500',
  offline: 'bg-red-500',
  degraded: 'bg-yellow-500',
}

function getUsagePercent(pool: StoragePool) {
  if (pool.totalGb === 0) return 0
  return Math.round((pool.usedGb / pool.totalGb) * 100)
}

const totalStorage = computed(() => storagePools.value.reduce((sum, p) => sum + p.totalGb, 0))
const usedStorage = computed(() => storagePools.value.reduce((sum, p) => sum + p.usedGb, 0))
const volumeCount = computed(() => volumes.value.length)
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

    <!-- Stats Summary -->
    <div class="grid grid-cols-3 gap-4">
      <div class="card p-4">
        <p class="text-sm text-dark-400">Total Storage</p>
        <p class="text-2xl font-bold text-white">{{ totalStorage }} GB</p>
      </div>
      <div class="card p-4">
        <p class="text-sm text-dark-400">Used Storage</p>
        <p class="text-2xl font-bold text-white">{{ usedStorage }} GB</p>
      </div>
      <div class="card p-4">
        <p class="text-sm text-dark-400">Volumes</p>
        <p class="text-2xl font-bold text-white">{{ volumeCount }}</p>
      </div>
    </div>

    <!-- Tabs -->
    <div class="border-b border-dark-600">
      <nav class="flex space-x-8">
        <button
          :class="[
            'py-3 px-1 text-sm font-medium border-b-2 transition-colors',
            activeTab === 'pools'
              ? 'border-accent-500 text-accent-400'
              : 'border-transparent text-dark-400 hover:text-white'
          ]"
          @click="activeTab = 'pools'"
        >
          Storage Pools
        </button>
        <button
          :class="[
            'py-3 px-1 text-sm font-medium border-b-2 transition-colors',
            activeTab === 'volumes'
              ? 'border-accent-500 text-accent-400'
              : 'border-transparent text-dark-400 hover:text-white'
          ]"
          @click="activeTab = 'volumes'"
        >
          Volumes / Disks
        </button>
      </nav>
    </div>

    <!-- Storage Pools Tab -->
    <div v-if="activeTab === 'pools'" class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
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
      
      <!-- Empty state for pools -->
      <div v-if="storagePools.length === 0 && !loading" class="col-span-full text-center py-12">
        <div class="w-16 h-16 mx-auto bg-dark-700 rounded-full flex items-center justify-center mb-4">
          <svg class="w-8 h-8 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"/>
          </svg>
        </div>
        <h3 class="text-lg font-medium text-white">No Storage Pools</h3>
        <p class="text-dark-400 mt-2">Add a storage pool to get started</p>
      </div>
    </div>

    <!-- Volumes Tab -->
    <div v-if="activeTab === 'volumes'" class="card overflow-hidden">
      <table class="w-full">
        <thead class="bg-dark-700/50">
          <tr>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Name</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Pool</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Size</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Format</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Attached To</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Created</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-dark-700">
          <tr v-for="vol in volumes" :key="vol.id" class="hover:bg-dark-700/30">
            <td class="px-4 py-3">
              <div class="flex items-center space-x-3">
                <div class="w-8 h-8 bg-dark-700 rounded flex items-center justify-center">
                  <svg class="w-4 h-4 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4"/>
                  </svg>
                </div>
                <span class="text-white font-medium">{{ vol.name }}</span>
              </div>
            </td>
            <td class="px-4 py-3 text-dark-400">{{ vol.pool }}</td>
            <td class="px-4 py-3 text-white">{{ vol.sizeGb }} GB</td>
            <td class="px-4 py-3">
              <span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-dark-600 text-dark-300">
                {{ vol.format }}
              </span>
            </td>
            <td class="px-4 py-3">
              <RouterLink 
                v-if="vol.vmId" 
                :to="`/vms/${vol.vmId}`"
                class="text-accent-400 hover:text-accent-300"
              >
                {{ vol.vmId }}
              </RouterLink>
              <span v-else class="text-dark-500">-</span>
            </td>
            <td class="px-4 py-3 text-dark-400">{{ vol.createdAt }}</td>
          </tr>
        </tbody>
      </table>
      
      <!-- Empty state for volumes -->
      <div v-if="volumes.length === 0 && !loadingVolumes" class="text-center py-12">
        <div class="w-16 h-16 mx-auto bg-dark-700 rounded-full flex items-center justify-center mb-4">
          <svg class="w-8 h-8 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4"/>
          </svg>
        </div>
        <h3 class="text-lg font-medium text-white">No Volumes</h3>
        <p class="text-dark-400 mt-2">Create a VM to add volumes</p>
      </div>
      
      <!-- Loading state -->
      <div v-if="loadingVolumes" class="text-center py-12">
        <svg class="animate-spin w-8 h-8 text-accent-500 mx-auto" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
        </svg>
      </div>
    </div>
  </div>
</template>
