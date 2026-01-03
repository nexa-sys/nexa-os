<script setup lang="ts">
import { ref } from 'vue'

interface Network {
  id: string
  name: string
  type: 'bridge' | 'vlan' | 'bond' | 'vxlan'
  status: 'active' | 'inactive'
  cidr?: string
  gateway?: string
  vlanId?: number
  ports: number
}

const networks = ref<Network[]>([
  { id: '1', name: 'vmbr0', type: 'bridge', status: 'active', cidr: '192.168.1.0/24', gateway: '192.168.1.1', ports: 12 },
  { id: '2', name: 'vmbr1', type: 'bridge', status: 'active', cidr: '10.0.0.0/24', gateway: '10.0.0.1', ports: 5 },
  { id: '3', name: 'vlan100', type: 'vlan', status: 'active', vlanId: 100, ports: 3 },
])
</script>

<template>
  <div class="p-6 space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Network</h1>
        <p class="text-dark-400 mt-1">Manage virtual networks and bridges</p>
      </div>
      <button class="btn-primary flex items-center space-x-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>Create Network</span>
      </button>
    </div>

    <div class="card overflow-hidden">
      <table class="w-full">
        <thead class="bg-dark-700/50 border-b border-dark-600">
          <tr>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Name</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Type</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Status</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">CIDR / VLAN</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Connected Ports</th>
            <th class="px-4 py-3 text-right text-xs font-medium text-dark-400 uppercase">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-dark-600">
          <tr v-for="net in networks" :key="net.id" class="hover:bg-dark-700/30">
            <td class="px-4 py-4">
              <div class="flex items-center space-x-3">
                <div class="w-8 h-8 bg-dark-700 rounded-lg flex items-center justify-center">
                  <svg class="w-4 h-4 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9"/>
                  </svg>
                </div>
                <span class="text-white font-medium">{{ net.name }}</span>
              </div>
            </td>
            <td class="px-4 py-4">
              <span class="text-sm text-dark-300 capitalize">{{ net.type }}</span>
            </td>
            <td class="px-4 py-4">
              <span class="inline-flex items-center space-x-2">
                <span :class="['w-2 h-2 rounded-full', net.status === 'active' ? 'bg-green-500' : 'bg-gray-500']" />
                <span class="text-sm text-dark-300 capitalize">{{ net.status }}</span>
              </span>
            </td>
            <td class="px-4 py-4">
              <span class="text-sm text-dark-300">{{ net.cidr || (net.vlanId ? `VLAN ${net.vlanId}` : '-') }}</span>
            </td>
            <td class="px-4 py-4">
              <span class="text-sm text-dark-300">{{ net.ports }}</span>
            </td>
            <td class="px-4 py-4 text-right">
              <button class="p-2 text-dark-400 hover:text-white hover:bg-dark-700 rounded-lg">
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z"/>
                </svg>
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
