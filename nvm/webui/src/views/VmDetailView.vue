<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useVmsStore } from '@/stores/vms'
import { useNotificationStore } from '@/stores/notification'
import { useModalStore } from '@/stores/modal'

const route = useRoute()
const router = useRouter()
const vmsStore = useVmsStore()
const notificationStore = useNotificationStore()
const modalStore = useModalStore()

const vmId = route.params.id as string
const activeTab = ref('overview')

const tabs = [
  { id: 'overview', name: 'Overview' },
  { id: 'hardware', name: 'Hardware' },
  { id: 'network', name: 'Network' },
  { id: 'snapshots', name: 'Snapshots' },
  { id: 'backup', name: 'Backup' },
  { id: 'logs', name: 'Logs' },
]

const vm = computed(() => vmsStore.selectedVm)

const statusColors: Record<string, string> = {
  running: 'bg-green-500',
  stopped: 'bg-gray-500',
  paused: 'bg-yellow-500',
  suspended: 'bg-blue-500',
  error: 'bg-red-500',
  migrating: 'bg-purple-500',
}

onMounted(async () => {
  await vmsStore.fetchVm(vmId)
})

async function handleAction(action: 'start' | 'stop' | 'restart' | 'pause' | 'resume') {
  if (!vm.value) return
  
  const success = await vmsStore.vmAction(vmId, action)
  if (success) {
    notificationStore.success('Action completed', `VM ${action} successful`)
  } else {
    notificationStore.error('Action failed', vmsStore.error || 'Unknown error')
  }
}

async function handleDelete() {
  if (!vm.value) return
  
  const confirmed = await modalStore.confirm({
    title: 'Delete Virtual Machine',
    message: `Are you sure you want to delete "${vm.value.name}"? This action cannot be undone.`,
    type: 'danger',
    confirmText: 'Delete',
  })
  
  if (!confirmed) return
  
  const success = await vmsStore.deleteVm(vmId)
  if (success) {
    notificationStore.success('VM Deleted', `"${vm.value.name}" has been deleted`)
    router.push('/vms')
  } else {
    notificationStore.error('Delete failed', vmsStore.error || 'Unknown error')
  }
}

function openConsole() {
  router.push(`/console/${vmId}`)
}
</script>

<template>
  <div class="p-6 space-y-6">
    <!-- Loading -->
    <div v-if="vmsStore.loading && !vm" class="flex items-center justify-center h-64">
      <svg class="animate-spin w-8 h-8 text-accent-500" fill="none" viewBox="0 0 24 24">
        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
      </svg>
    </div>

    <!-- Not Found -->
    <div v-else-if="!vm" class="text-center py-12">
      <h2 class="text-xl font-medium text-white">VM Not Found</h2>
      <p class="text-dark-400 mt-2">The requested virtual machine does not exist.</p>
      <RouterLink to="/vms" class="btn-primary mt-4 inline-block">Back to VMs</RouterLink>
    </div>

    <!-- VM Details -->
    <template v-else>
      <!-- Header -->
      <div class="flex items-start justify-between">
        <div class="flex items-center space-x-4">
          <button
            class="p-2 text-dark-400 hover:text-white hover:bg-dark-700 rounded-lg"
            @click="router.push('/vms')"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18"/>
            </svg>
          </button>
          <div class="w-12 h-12 bg-dark-700 rounded-lg flex items-center justify-center">
            <svg class="w-6 h-6 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/>
            </svg>
          </div>
          <div>
            <div class="flex items-center space-x-3">
              <h1 class="text-2xl font-bold text-white">{{ vm.name }}</h1>
              <span class="inline-flex items-center space-x-1">
                <span :class="['w-2 h-2 rounded-full', statusColors[vm.status]]" />
                <span class="text-sm text-dark-400 capitalize">{{ vm.status }}</span>
              </span>
            </div>
            <p class="text-dark-500 text-sm">{{ vm.id }}</p>
          </div>
        </div>

        <div class="flex items-center space-x-3">
          <button
            v-if="vm.status === 'running'"
            class="btn-secondary flex items-center space-x-2"
            @click="openConsole"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"/>
            </svg>
            <span>Console</span>
          </button>
          
          <button
            v-if="vm.status === 'stopped'"
            class="btn-primary flex items-center space-x-2"
            @click="handleAction('start')"
          >
            <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM9.555 7.168A1 1 0 008 8v4a1 1 0 001.555.832l3-2a1 1 0 000-1.664l-3-2z" clip-rule="evenodd"/>
            </svg>
            <span>Start</span>
          </button>
          
          <button
            v-if="vm.status === 'running'"
            class="btn-danger flex items-center space-x-2"
            @click="handleAction('stop')"
          >
            <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8 7a1 1 0 00-1 1v4a1 1 0 001 1h4a1 1 0 001-1V8a1 1 0 00-1-1H8z" clip-rule="evenodd"/>
            </svg>
            <span>Stop</span>
          </button>

          <button
            class="p-2 text-dark-400 hover:text-red-400 hover:bg-dark-700 rounded-lg"
            title="Delete VM"
            @click="handleDelete"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"/>
            </svg>
          </button>
        </div>
      </div>

      <!-- Tabs -->
      <div class="border-b border-dark-600">
        <nav class="flex space-x-8">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            :class="[
              'py-3 px-1 text-sm font-medium border-b-2 transition-colors',
              activeTab === tab.id
                ? 'border-accent-500 text-accent-400'
                : 'border-transparent text-dark-400 hover:text-white hover:border-dark-500'
            ]"
            @click="activeTab = tab.id"
          >
            {{ tab.name }}
          </button>
        </nav>
      </div>

      <!-- Tab Content -->
      <div>
        <!-- Overview Tab -->
        <div v-if="activeTab === 'overview'" class="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <!-- Details -->
          <div class="lg:col-span-2 card p-6">
            <h3 class="text-lg font-medium text-white mb-4">Details</h3>
            <dl class="grid grid-cols-2 gap-4">
              <div>
                <dt class="text-sm text-dark-400">Name</dt>
                <dd class="text-white mt-1">{{ vm.name }}</dd>
              </div>
              <div>
                <dt class="text-sm text-dark-400">Status</dt>
                <dd class="text-white mt-1 capitalize">{{ vm.status }}</dd>
              </div>
              <div>
                <dt class="text-sm text-dark-400">Description</dt>
                <dd class="text-white mt-1">{{ vm.description || '-' }}</dd>
              </div>
              <div>
                <dt class="text-sm text-dark-400">Host Node</dt>
                <dd class="text-white mt-1">{{ vm.host_node || 'N/A' }}</dd>
              </div>
              <div>
                <dt class="text-sm text-dark-400">Created</dt>
                <dd class="text-white mt-1">{{ vm.created_at }}</dd>
              </div>
              <div>
                <dt class="text-sm text-dark-400">Updated</dt>
                <dd class="text-white mt-1">{{ vm.updated_at }}</dd>
              </div>
            </dl>
          </div>

          <!-- Resources -->
          <div class="card p-6">
            <h3 class="text-lg font-medium text-white mb-4">Resources</h3>
            <div class="space-y-4">
              <div>
                <div class="flex justify-between text-sm mb-2">
                  <span class="text-dark-400">CPU</span>
                  <span class="text-white">{{ vm.config.cpu_cores }} vCPUs</span>
                </div>
                <div class="h-2 bg-dark-700 rounded-full overflow-hidden">
                  <div class="h-full bg-accent-500 rounded-full" :style="{ width: `${vm.stats?.cpu_usage || 0}%` }" />
                </div>
              </div>
              <div>
                <div class="flex justify-between text-sm mb-2">
                  <span class="text-dark-400">Memory</span>
                  <span class="text-white">{{ vm.config.memory_mb }} MB</span>
                </div>
                <div class="h-2 bg-dark-700 rounded-full overflow-hidden">
                  <div class="h-full bg-green-500 rounded-full" :style="{ width: `${vm.stats?.memory_usage || 0}%` }" />
                </div>
              </div>
              <div>
                <div class="flex justify-between text-sm mb-2">
                  <span class="text-dark-400">Disk</span>
                  <span class="text-white">{{ vm.config.disk_gb }} GB</span>
                </div>
              </div>
              <div>
                <div class="flex justify-between text-sm">
                  <span class="text-dark-400">Network</span>
                  <span class="text-white">{{ vm.config.network }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Hardware Tab -->
        <div v-else-if="activeTab === 'hardware'" class="card p-6">
          <div class="flex items-center justify-between mb-6">
            <h3 class="text-lg font-medium text-white">Hardware Configuration</h3>
            <RouterLink :to="`/vms/${vmId}/edit`" class="btn-secondary text-sm">Edit</RouterLink>
          </div>
          <div class="space-y-4">
            <div class="flex items-center justify-between p-4 bg-dark-700/50 rounded-lg">
              <div class="flex items-center space-x-3">
                <div class="w-10 h-10 bg-dark-700 rounded-lg flex items-center justify-center">
                  <svg class="w-5 h-5 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z"/>
                  </svg>
                </div>
                <div>
                  <p class="text-white font-medium">CPU</p>
                  <p class="text-sm text-dark-400">{{ vm.config.cpu_cores }} virtual cores</p>
                </div>
              </div>
            </div>
            <div class="flex items-center justify-between p-4 bg-dark-700/50 rounded-lg">
              <div class="flex items-center space-x-3">
                <div class="w-10 h-10 bg-dark-700 rounded-lg flex items-center justify-center">
                  <svg class="w-5 h-5 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"/>
                  </svg>
                </div>
                <div>
                  <p class="text-white font-medium">Memory</p>
                  <p class="text-sm text-dark-400">{{ vm.config.memory_mb }} MB</p>
                </div>
              </div>
            </div>
            <div class="flex items-center justify-between p-4 bg-dark-700/50 rounded-lg">
              <div class="flex items-center space-x-3">
                <div class="w-10 h-10 bg-dark-700 rounded-lg flex items-center justify-center">
                  <svg class="w-5 h-5 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"/>
                  </svg>
                </div>
                <div>
                  <p class="text-white font-medium">Disk</p>
                  <p class="text-sm text-dark-400">{{ vm.config.disk_gb }} GB</p>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Other tabs placeholder -->
        <div v-else class="card p-6 text-center py-12">
          <div class="w-16 h-16 mx-auto bg-dark-700 rounded-full flex items-center justify-center mb-4">
            <svg class="w-8 h-8 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6V4m0 2a2 2 0 100 4m0-4a2 2 0 110 4m-6 8a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4m6 6v10m6-2a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4"/>
            </svg>
          </div>
          <h3 class="text-lg font-medium text-white">{{ tabs.find(t => t.id === activeTab)?.name }}</h3>
          <p class="text-dark-400 mt-2">This section is under development</p>
        </div>
      </div>
    </template>
  </div>
</template>
