<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { api } from '../api'

interface Backup {
  id: string
  vmName: string
  type: string
  size: string
  date: string
  status: string
}

const backups = ref<Backup[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

async function fetchBackups() {
  try {
    loading.value = true
    error.value = null
    const response = await api.get('/backup/jobs')
    if (response.data.success && response.data.data) {
      backups.value = response.data.data.map((backup: any) => ({
        id: backup.id,
        vmName: backup.vm_name || backup.target || 'Unknown',
        type: backup.backup_type || 'full',
        size: backup.size_bytes ? `${(backup.size_bytes / (1024 * 1024 * 1024)).toFixed(1)} GB` : '-',
        date: backup.created_at ? new Date(backup.created_at * 1000).toLocaleString() : '-',
        status: backup.status || 'unknown'
      }))
    }
  } catch (e: any) {
    error.value = e.message || 'Failed to load backups'
    console.error('Failed to fetch backups:', e)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  fetchBackups()
})
</script>

<template>
  <div class="p-6 space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Backup</h1>
        <p class="text-dark-400 mt-1">Manage VM backups and snapshots</p>
      </div>
      <div class="flex space-x-3">
        <button class="btn-secondary">Schedule Backup</button>
        <button class="btn-primary flex items-center space-x-2">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
          </svg>
          <span>Backup Now</span>
        </button>
      </div>
    </div>

    <div class="card overflow-hidden">
      <table class="w-full">
        <thead class="bg-dark-700/50 border-b border-dark-600">
          <tr>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">VM Name</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Type</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Size</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Date</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Status</th>
            <th class="px-4 py-3 text-right text-xs font-medium text-dark-400 uppercase">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-dark-600">
          <tr v-for="backup in backups" :key="backup.id" class="hover:bg-dark-700/30">
            <td class="px-4 py-4 text-white">{{ backup.vmName }}</td>
            <td class="px-4 py-4 text-dark-300 capitalize">{{ backup.type }}</td>
            <td class="px-4 py-4 text-dark-300">{{ backup.size }}</td>
            <td class="px-4 py-4 text-dark-300">{{ backup.date }}</td>
            <td class="px-4 py-4">
              <span class="inline-flex items-center space-x-1">
                <span class="w-2 h-2 rounded-full bg-green-500" />
                <span class="text-sm text-dark-300 capitalize">{{ backup.status }}</span>
              </span>
            </td>
            <td class="px-4 py-4 text-right">
              <button class="text-sm text-accent-400 hover:text-accent-300 mr-4">Restore</button>
              <button class="text-sm text-red-400 hover:text-red-300">Delete</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
