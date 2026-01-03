<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { api } from '../api'

interface ClusterNode {
  id: string
  name: string
  status: 'online' | 'offline' | 'maintenance'
  ip: string
  role: 'master' | 'worker' | 'standalone'
  cpu: { used: number; total: number }
  memory: { usedGb: number; totalGb: number }
  vms: number
}

const nodes = ref<ClusterNode[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

async function fetchNodes() {
  try {
    loading.value = true
    error.value = null
    const response = await api.get('/nodes')
    if (response.data.success && response.data.data) {
      nodes.value = response.data.data.map((node: any) => ({
        id: node.id,
        name: node.hostname || node.id,
        status: node.status || 'online',
        ip: node.ip_address || '',
        role: node.role || 'standalone',
        cpu: { 
          used: Math.round(node.cpu_usage * node.cpu_cores / 100) || 0, 
          total: node.cpu_cores || 1 
        },
        memory: { 
          usedGb: Math.round((node.memory_used_mb || 0) / 1024), 
          totalGb: Math.round((node.memory_total_mb || 0) / 1024) 
        },
        vms: node.vm_count || 0
      }))
    }
  } catch (e: any) {
    error.value = e.message || 'Failed to load nodes'
    console.error('Failed to fetch nodes:', e)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  fetchNodes()
})

const statusColors: Record<string, string> = {
  online: 'bg-green-500',
  offline: 'bg-red-500',
  maintenance: 'bg-yellow-500',
}
</script>

<template>
  <div class="p-6 space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Cluster</h1>
        <p class="text-dark-400 mt-1">Manage cluster nodes and high availability</p>
      </div>
      <button class="btn-primary flex items-center space-x-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>Add Node</span>
      </button>
    </div>

    <!-- Cluster Stats -->
    <div class="grid grid-cols-4 gap-4">
      <div class="card p-4">
        <p class="text-dark-400 text-sm">Total Nodes</p>
        <p class="text-2xl font-bold text-white mt-1">{{ nodes.length }}</p>
      </div>
      <div class="card p-4">
        <p class="text-dark-400 text-sm">Online</p>
        <p class="text-2xl font-bold text-green-400 mt-1">{{ nodes.filter(n => n.status === 'online').length }}</p>
      </div>
      <div class="card p-4">
        <p class="text-dark-400 text-sm">Total vCPUs</p>
        <p class="text-2xl font-bold text-white mt-1">{{ nodes.reduce((sum, n) => sum + n.cpu.total, 0) }}</p>
      </div>
      <div class="card p-4">
        <p class="text-dark-400 text-sm">Total Memory</p>
        <p class="text-2xl font-bold text-white mt-1">{{ nodes.reduce((sum, n) => sum + n.memory.totalGb, 0) }} GB</p>
      </div>
    </div>

    <!-- Nodes Grid -->
    <div class="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-4">
      <div v-for="node in nodes" :key="node.id" class="card p-5">
        <div class="flex items-start justify-between mb-4">
          <div class="flex items-center space-x-3">
            <div :class="['w-10 h-10 rounded-lg flex items-center justify-center', node.role === 'master' ? 'bg-accent-500/20' : 'bg-dark-700']">
              <svg class="w-5 h-5" :class="node.role === 'master' ? 'text-accent-400' : 'text-dark-400'" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/>
              </svg>
            </div>
            <div>
              <div class="flex items-center space-x-2">
                <h3 class="text-white font-medium">{{ node.name }}</h3>
                <span v-if="node.role === 'master'" class="text-xs bg-accent-500/20 text-accent-400 px-2 py-0.5 rounded">Master</span>
              </div>
              <p class="text-xs text-dark-500">{{ node.ip }}</p>
            </div>
          </div>
          <span class="inline-flex items-center space-x-1">
            <span :class="['w-2 h-2 rounded-full', statusColors[node.status]]" />
            <span class="text-xs text-dark-400 capitalize">{{ node.status }}</span>
          </span>
        </div>

        <div class="space-y-3">
          <div>
            <div class="flex justify-between text-xs mb-1">
              <span class="text-dark-400">CPU</span>
              <span class="text-dark-300">{{ node.cpu.used }} / {{ node.cpu.total }} cores</span>
            </div>
            <div class="h-1.5 bg-dark-700 rounded-full overflow-hidden">
              <div class="h-full bg-accent-500 rounded-full" :style="{ width: `${(node.cpu.used / node.cpu.total) * 100}%` }" />
            </div>
          </div>
          <div>
            <div class="flex justify-between text-xs mb-1">
              <span class="text-dark-400">Memory</span>
              <span class="text-dark-300">{{ node.memory.usedGb }} / {{ node.memory.totalGb }} GB</span>
            </div>
            <div class="h-1.5 bg-dark-700 rounded-full overflow-hidden">
              <div class="h-full bg-green-500 rounded-full" :style="{ width: `${(node.memory.usedGb / node.memory.totalGb) * 100}%` }" />
            </div>
          </div>
        </div>

        <div class="mt-4 pt-4 border-t border-dark-600 flex items-center justify-between">
          <span class="text-sm text-dark-400">{{ node.vms }} VMs</span>
          <button class="text-sm text-accent-400 hover:text-accent-300">Manage</button>
        </div>
      </div>
    </div>
  </div>
</template>
