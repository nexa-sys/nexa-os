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

export interface Vm {
  id: string
  name: string
  status: VmStatus
  description?: string
  host_node?: string
  template?: string
  config: VmConfig
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
      selectedVm.value = response.data.data
      // Update in list
      const idx = vms.value.findIndex(vm => vm.id === id)
      if (idx !== -1) {
        vms.value[idx] = selectedVm.value!
      }
      return selectedVm.value
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
      const updatedVm = response.data.data
      const idx = vms.value.findIndex(vm => vm.id === id)
      if (idx !== -1) {
        vms.value[idx] = updatedVm
      }
      if (selectedVm.value?.id === id) {
        selectedVm.value = updatedVm
      }
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
