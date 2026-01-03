<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useVmsStore } from '@/stores/vms'
import { useNotificationStore } from '@/stores/notification'
import { useModalStore } from '@/stores/modal'
import type { Vm, VmStatus } from '@/stores/vms'

const router = useRouter()
const vmsStore = useVmsStore()
const notificationStore = useNotificationStore()
const modalStore = useModalStore()

const searchQuery = ref('')
const statusFilter = ref<VmStatus | 'all'>('all')
const selectedVms = ref<string[]>([])
const showActionsMenu = ref<string | null>(null)

const filteredVms = computed(() => {
  let result = vmsStore.vms
  
  if (statusFilter.value !== 'all') {
    result = result.filter(vm => vm.status === statusFilter.value)
  }
  
  if (searchQuery.value) {
    const query = searchQuery.value.toLowerCase()
    result = result.filter(vm => 
      vm.name.toLowerCase().includes(query) ||
      vm.id.toLowerCase().includes(query)
    )
  }
  
  return result
})

const statusColors: Record<VmStatus, string> = {
  running: 'bg-green-500',
  stopped: 'bg-gray-500',
  paused: 'bg-yellow-500',
  suspended: 'bg-blue-500',
  error: 'bg-red-500',
  migrating: 'bg-purple-500',
}

const statusLabels: Record<VmStatus, string> = {
  running: 'Running',
  stopped: 'Stopped',
  paused: 'Paused',
  suspended: 'Suspended',
  error: 'Error',
  migrating: 'Migrating',
}

onMounted(async () => {
  await vmsStore.fetchVms()
})

function toggleSelectAll() {
  if (selectedVms.value.length === filteredVms.value.length) {
    selectedVms.value = []
  } else {
    selectedVms.value = filteredVms.value.map(vm => vm.id)
  }
}

function toggleSelect(id: string) {
  const idx = selectedVms.value.indexOf(id)
  if (idx === -1) {
    selectedVms.value.push(id)
  } else {
    selectedVms.value.splice(idx, 1)
  }
}

async function handleAction(vm: Vm, action: 'start' | 'stop' | 'restart' | 'pause' | 'resume') {
  showActionsMenu.value = null
  
  const success = await vmsStore.vmAction(vm.id, action)
  if (success) {
    notificationStore.success('Action completed', `VM "${vm.name}" ${action} successful`)
  } else {
    notificationStore.error('Action failed', vmsStore.error || 'Unknown error')
  }
}

async function handleDelete(vm: Vm) {
  showActionsMenu.value = null
  
  const confirmed = await modalStore.confirm({
    title: 'Delete Virtual Machine',
    message: `Are you sure you want to delete "${vm.name}"? This action cannot be undone.`,
    type: 'danger',
    confirmText: 'Delete',
  })
  
  if (!confirmed) return
  
  const success = await vmsStore.deleteVm(vm.id)
  if (success) {
    notificationStore.success('VM Deleted', `"${vm.name}" has been deleted`)
  } else {
    notificationStore.error('Delete failed', vmsStore.error || 'Unknown error')
  }
}

function openConsole(vm: Vm) {
  router.push(`/console/${vm.id}`)
}
</script>

<template>
  <div class="p-6 space-y-6">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Virtual Machines</h1>
        <p class="text-dark-400 mt-1">Manage your virtual machine instances</p>
      </div>
      <RouterLink to="/vms/create" class="btn-primary flex items-center space-x-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>Create VM</span>
      </RouterLink>
    </div>

    <!-- Filters -->
    <div class="card p-4">
      <div class="flex items-center space-x-4">
        <!-- Search -->
        <div class="relative flex-1 max-w-md">
          <input
            v-model="searchQuery"
            type="text"
            placeholder="Search VMs..."
            class="input w-full pl-10"
          />
          <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
          </svg>
        </div>

        <!-- Status Filter -->
        <select v-model="statusFilter" class="input">
          <option value="all">All Status</option>
          <option value="running">Running</option>
          <option value="stopped">Stopped</option>
          <option value="paused">Paused</option>
          <option value="error">Error</option>
        </select>

        <!-- Bulk Actions -->
        <div v-if="selectedVms.length > 0" class="flex items-center space-x-2">
          <span class="text-sm text-dark-400">{{ selectedVms.length }} selected</span>
          <button class="btn-secondary text-sm">Start All</button>
          <button class="btn-secondary text-sm">Stop All</button>
        </div>
      </div>
    </div>

    <!-- VM Table -->
    <div class="card overflow-visible">
      <div v-if="vmsStore.loading" class="p-8 text-center">
        <svg class="animate-spin w-8 h-8 mx-auto text-accent-500" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
        </svg>
        <p class="text-dark-400 mt-4">Loading virtual machines...</p>
      </div>

      <div v-else-if="filteredVms.length === 0" class="p-8 text-center">
        <div class="w-16 h-16 mx-auto bg-dark-700 rounded-full flex items-center justify-center mb-4">
          <svg class="w-8 h-8 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/>
          </svg>
        </div>
        <h3 class="text-lg font-medium text-white">No virtual machines found</h3>
        <p class="text-dark-400 mt-2">Get started by creating your first VM</p>
        <RouterLink to="/vms/create" class="btn-primary mt-4 inline-flex items-center space-x-2">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
          </svg>
          <span>Create VM</span>
        </RouterLink>
      </div>

      <table v-else class="w-full">
        <thead class="bg-dark-700/50 border-b border-dark-600">
          <tr>
            <th class="w-12 px-4 py-3">
              <input
                type="checkbox"
                :checked="selectedVms.length === filteredVms.length && filteredVms.length > 0"
                :indeterminate="selectedVms.length > 0 && selectedVms.length < filteredVms.length"
                class="w-4 h-4 rounded border-dark-600 bg-dark-700 text-accent-500"
                @change="toggleSelectAll"
              />
            </th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase tracking-wider">Name</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase tracking-wider">Status</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase tracking-wider">Resources</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase tracking-wider">Node</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase tracking-wider">Uptime</th>
            <th class="px-4 py-3 text-right text-xs font-medium text-dark-400 uppercase tracking-wider">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-dark-600">
          <tr
            v-for="vm in filteredVms"
            :key="vm.id"
            class="hover:bg-dark-700/30 transition-colors relative"
          >
            <td class="px-4 py-4">
              <input
                type="checkbox"
                :checked="selectedVms.includes(vm.id)"
                class="w-4 h-4 rounded border-dark-600 bg-dark-700 text-accent-500"
                @change="toggleSelect(vm.id)"
              />
            </td>
            <td class="px-4 py-4">
              <div class="flex items-center space-x-3">
                <div class="w-10 h-10 bg-dark-700 rounded-lg flex items-center justify-center">
                  <svg class="w-5 h-5 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/>
                  </svg>
                </div>
                <div>
                  <RouterLink :to="`/vms/${vm.id}`" class="text-white font-medium hover:text-accent-400">
                    {{ vm.name }}
                  </RouterLink>
                  <p class="text-xs text-dark-500">{{ vm.id }}</p>
                </div>
              </div>
            </td>
            <td class="px-4 py-4">
              <span class="inline-flex items-center space-x-2">
                <span :class="['w-2 h-2 rounded-full', statusColors[vm.status]]" />
                <span class="text-sm text-dark-300">{{ statusLabels[vm.status] }}</span>
              </span>
            </td>
            <td class="px-4 py-4">
              <div class="text-sm text-dark-300">
                <span>{{ vm.config?.cpu_cores ?? 0 }} vCPU</span>
                <span class="mx-2">•</span>
                <span>{{ vm.config?.memory_mb ?? 0 }} MB</span>
                <span class="mx-2">•</span>
                <span>{{ vm.config?.disk_gb ?? 0 }} GB</span>
              </div>
            </td>
            <td class="px-4 py-4">
              <span class="text-sm text-dark-300">{{ vm.host_node || 'N/A' }}</span>
            </td>
            <td class="px-4 py-4">
              <span class="text-sm text-dark-300">{{ vm.status === 'running' ? '2d 5h' : '-' }}</span>
            </td>
            <td class="px-4 py-4">
              <div class="flex items-center justify-end space-x-2">
                <!-- Quick Actions -->
                <button
                  v-if="vm.status === 'stopped'"
                  class="p-2 text-green-400 hover:bg-dark-700 rounded-lg transition-colors"
                  title="Start"
                  @click="handleAction(vm, 'start')"
                >
                  <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
                    <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM9.555 7.168A1 1 0 008 8v4a1 1 0 001.555.832l3-2a1 1 0 000-1.664l-3-2z" clip-rule="evenodd"/>
                  </svg>
                </button>
                <button
                  v-if="vm.status === 'running'"
                  class="p-2 text-red-400 hover:bg-dark-700 rounded-lg transition-colors"
                  title="Stop"
                  @click="handleAction(vm, 'stop')"
                >
                  <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
                    <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8 7a1 1 0 00-1 1v4a1 1 0 001 1h4a1 1 0 001-1V8a1 1 0 00-1-1H8z" clip-rule="evenodd"/>
                  </svg>
                </button>
                <button
                  v-if="vm.status === 'running'"
                  class="p-2 text-blue-400 hover:bg-dark-700 rounded-lg transition-colors"
                  title="Console"
                  @click="openConsole(vm)"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"/>
                  </svg>
                </button>

                <!-- More Actions -->
                <div class="relative">
                  <button
                    class="p-2 text-dark-400 hover:text-white hover:bg-dark-700 rounded-lg transition-colors"
                    @click="showActionsMenu = showActionsMenu === vm.id ? null : vm.id"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z"/>
                    </svg>
                  </button>

                  <div
                    v-if="showActionsMenu === vm.id"
                    class="absolute right-0 mt-2 w-48 bg-dark-700 rounded-lg shadow-xl border border-dark-600 py-1 z-[9999]"
                  >
                    <RouterLink :to="`/vms/${vm.id}`" class="block px-4 py-2 text-sm text-dark-300 hover:bg-dark-600 hover:text-white">
                      View Details
                    </RouterLink>
                    <RouterLink :to="`/vms/${vm.id}/edit`" class="block px-4 py-2 text-sm text-dark-300 hover:bg-dark-600 hover:text-white">
                      Edit Configuration
                    </RouterLink>
                    <button class="w-full text-left px-4 py-2 text-sm text-dark-300 hover:bg-dark-600 hover:text-white">
                      Clone VM
                    </button>
                    <button class="w-full text-left px-4 py-2 text-sm text-dark-300 hover:bg-dark-600 hover:text-white">
                      Take Snapshot
                    </button>
                    <hr class="my-1 border-dark-600">
                    <button
                      class="w-full text-left px-4 py-2 text-sm text-red-400 hover:bg-dark-600"
                      @click="handleDelete(vm)"
                    >
                      Delete VM
                    </button>
                  </div>
                </div>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- Click outside to close menu -->
    <div
      v-if="showActionsMenu"
      class="fixed inset-0 z-[9998]"
      @click="showActionsMenu = null"
    />
  </div>
</template>
