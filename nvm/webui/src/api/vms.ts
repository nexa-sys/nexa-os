import { api } from './index'
import type { Vm, VmCreateParams } from '@/stores/vms'

export const vmsApi = {
  // List all VMs
  async list(params?: { page?: number; per_page?: number; status?: string }) {
    return api.get('/vms', { params })
  },

  // Get VM by ID
  async get(id: string) {
    return api.get(`/vms/${id}`)
  },

  // Create VM
  async create(params: VmCreateParams) {
    return api.post('/vms', params)
  },

  // Update VM
  async update(id: string, params: Partial<Vm>) {
    return api.put(`/vms/${id}`, params)
  },

  // Delete VM
  async delete(id: string) {
    return api.delete(`/vms/${id}`)
  },

  // VM actions
  async start(id: string) {
    return api.post(`/vms/${id}/start`)
  },

  async stop(id: string, force = false) {
    return api.post(`/vms/${id}/stop`, { force })
  },

  async restart(id: string) {
    return api.post(`/vms/${id}/restart`)
  },

  async pause(id: string) {
    return api.post(`/vms/${id}/pause`)
  },

  async resume(id: string) {
    return api.post(`/vms/${id}/resume`)
  },

  async reset(id: string) {
    return api.post(`/vms/${id}/reset`)
  },

  // Migration
  async migrate(id: string, targetNode: string, options?: { live?: boolean }) {
    return api.post(`/vms/${id}/migrate`, { target_node: targetNode, ...options })
  },

  // Snapshots
  async listSnapshots(id: string) {
    return api.get(`/vms/${id}/snapshots`)
  },

  async createSnapshot(id: string, name: string, description?: string) {
    return api.post(`/vms/${id}/snapshots`, { name, description })
  },

  async deleteSnapshot(id: string, snapshotId: string) {
    return api.delete(`/vms/${id}/snapshots/${snapshotId}`)
  },

  async revertSnapshot(id: string, snapshotId: string) {
    return api.post(`/vms/${id}/snapshots/${snapshotId}/revert`)
  },

  // Clone
  async clone(id: string, name: string, options?: { full_clone?: boolean }) {
    return api.post(`/vms/${id}/clone`, { name, ...options })
  },

  // Console
  async getConsoleUrl(id: string, type: 'vnc' | 'spice' = 'vnc') {
    return api.get(`/vms/${id}/console`, { params: { type } })
  },

  // Stats
  async getStats(id: string) {
    return api.get(`/vms/${id}/stats`)
  },
}
