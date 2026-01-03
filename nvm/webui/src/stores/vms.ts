import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api } from '@/api'

export type VmStatus = 'running' | 'stopped' | 'paused' | 'suspended' | 'error' | 'migrating'

export interface VmConfig {
  cpu_cores: number
  memory_mb: number
  disk_gb: number
  network: string
  boot_order: string[]
}

export interface VmStats {
  cpu_usage: number
  memory_usage: number
  disk_read_bps: number
  disk_write_bps: number
  network_rx_bps: number
  network_tx_bps: number
}

// Detailed hardware info from VM details API
export interface VmDisk {
  id: string
  name: string
  size_gb: number
  format: string
  storage_pool: string
  bus: string
}

export interface VmNetwork {
  id: string
  mac: string
  network: string
  model: string
  ip?: string
}

export interface VmHardware {
  vcpus: number
  memory_mb: number
  disks: VmDisk[]
  networks: VmNetwork[]
  cdrom?: string
}

export interface Vm {
  id: string
  name: string
  status: VmStatus
  description?: string
  host_node?: string
  template?: string
  config: VmConfig
  hardware?: VmHardware  // Detailed hardware info (from detail API)
  stats?: VmStats
  created_at: string
  updated_at: string
  tags?: string[]
}

export interface VmCreateParams {
  name: string
  description?: string
  template?: string
  config: VmConfig
  tags?: string[]
}

export const useVmsStore = defineStore('vms', () => {
  const vms = ref<Vm[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const selectedVm = ref<Vm | null>(null)

  const runningVms = computed(() => vms.value.filter(vm => vm.status === 'running'))
  const stoppedVms = computed(() => vms.value.filter(vm => vm.status === 'stopped'))
  const totalCpu = computed(() => vms.value.reduce((sum, vm) => sum + vm.config.cpu_cores, 0))
  const totalMemory = computed(() => vms.value.reduce((sum, vm) => sum + vm.config.memory_mb, 0))

  async function fetchVms() {
    loading.value = true
    error.value = null
    
    try {
      const response = await api.get('/vms')
      vms.value = response.data.data || []
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || 'Failed to fetch VMs'
      vms.value = []
    } finally {
      loading.value = false
    }
  }

  async function fetchVm(id: string) {
    loading.value = true
    error.value = null
    
    try {
      const response = await api.get(`/vms/${id}`)
      const data = response.data.data
      
      // Map detailed VM response to Vm interface
      // Backend returns VmDetails with hardware object
      const vm: Vm = {
        id: data.id,
        name: data.name,
        status: data.status,
        description: data.description,
        host_node: data.node,
        template: data.template,
        config: {
          cpu_cores: data.hardware?.vcpus || data.config?.cpu_cores || 2,
          memory_mb: data.hardware?.memory_mb || data.config?.memory_mb || 2048,
          disk_gb: data.hardware?.disks?.[0]?.size_gb || data.config?.disk_gb || 20,
          network: data.hardware?.networks?.[0]?.network || data.config?.network || 'default',
          boot_order: data.config?.boot_order || ['disk', 'cdrom'],
        },
        hardware: data.hardware,  // Include full hardware details
        stats: data.metrics ? {
          cpu_usage: data.metrics.cpu_percent || 0,
          memory_usage: data.metrics.memory_used_mb ? (data.metrics.memory_used_mb / (data.hardware?.memory_mb || 1)) * 100 : 0,
          disk_read_bps: data.metrics.disk_read_bps || 0,
          disk_write_bps: data.metrics.disk_write_bps || 0,
          network_rx_bps: data.metrics.net_rx_bps || 0,
          network_tx_bps: data.metrics.net_tx_bps || 0,
        } : undefined,
        created_at: data.created_at ? new Date(data.created_at * 1000).toISOString() : new Date().toISOString(),
        updated_at: data.started_at ? new Date(data.started_at * 1000).toISOString() : new Date().toISOString(),
        tags: data.tags || [],
      }
      
      selectedVm.value = vm
      // Update in list
      const idx = vms.value.findIndex(v => v.id === id)
      if (idx !== -1) {
        vms.value[idx] = vm
      }
      return vm
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || 'Failed to fetch VM'
      return null
    } finally {
      loading.value = false
    }
  }

  async function createVm(params: VmCreateParams) {
    loading.value = true
    error.value = null
    
    try {
      const response = await api.post('/vms', params)
      const newVm = response.data.data
      vms.value.push(newVm)
      return newVm
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || 'Failed to create VM'
      return null
    } finally {
      loading.value = false
    }
  }

  async function updateVm(id: string, params: Partial<VmConfig>) {
    loading.value = true
    error.value = null
    
    try {
      const response = await api.put(`/vms/${id}`, params)
      const updatedVm = response.data.data
      const idx = vms.value.findIndex(vm => vm.id === id)
      if (idx !== -1) {
        vms.value[idx] = updatedVm
      }
      if (selectedVm.value?.id === id) {
        selectedVm.value = updatedVm
      }
      return updatedVm
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || 'Failed to update VM'
      return null
    } finally {
      loading.value = false
    }
  }

  async function deleteVm(id: string) {
    loading.value = true
    error.value = null
    
    try {
      await api.delete(`/vms/${id}`)
      vms.value = vms.value.filter(vm => vm.id !== id)
      if (selectedVm.value?.id === id) {
        selectedVm.value = null
      }
      return true
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || 'Failed to delete VM'
      return false
    } finally {
      loading.value = false
    }
  }

  async function vmAction(id: string, action: 'start' | 'stop' | 'restart' | 'pause' | 'resume' | 'reset') {
    loading.value = true
    error.value = null
    
    try {
      const response = await api.post(`/vms/${id}/${action}`)
      const result = response.data.data
      
      // Backend returns {task_id, status} not full VM, so update status locally
      if (result?.status) {
        const idx = vms.value.findIndex(vm => vm.id === id)
        if (idx !== -1) {
          vms.value[idx] = { ...vms.value[idx], status: result.status }
        }
        if (selectedVm.value?.id === id) {
          selectedVm.value = { ...selectedVm.value, status: result.status }
        }
      }
      
      // Optionally refresh full VM data after action
      await fetchVm(id)
      
      return true
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || `Failed to ${action} VM`
      return false
    } finally {
      loading.value = false
    }
  }

  async function migrateVm(id: string, targetNode: string) {
    loading.value = true
    error.value = null
    
    try {
      await api.post(`/vms/${id}/migrate`, { target_node: targetNode })
      return true
    } catch (e: any) {
      error.value = e.response?.data?.error?.message || 'Failed to migrate VM'
      return false
    } finally {
      loading.value = false
    }
  }

  return {
    vms,
    loading,
    error,
    selectedVm,
    runningVms,
    stoppedVms,
    totalCpu,
    totalMemory,
    fetchVms,
    fetchVm,
    createVm,
    updateVm,
    deleteVm,
    vmAction,
    migrateVm,
  }
})
