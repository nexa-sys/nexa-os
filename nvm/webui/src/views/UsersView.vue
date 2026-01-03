<script setup lang="ts">
import { ref } from 'vue'

const users = ref([
  { id: '1', username: 'admin', email: 'admin@example.com', role: 'Administrator', status: 'active', lastLogin: '2025-01-15 10:30' },
  { id: '2', username: 'operator', email: 'ops@example.com', role: 'Operator', status: 'active', lastLogin: '2025-01-14 16:45' },
  { id: '3', username: 'viewer', email: 'view@example.com', role: 'Viewer', status: 'inactive', lastLogin: '2025-01-10 09:00' },
])
</script>

<template>
  <div class="p-6 space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Users</h1>
        <p class="text-dark-400 mt-1">Manage users and permissions</p>
      </div>
      <button class="btn-primary flex items-center space-x-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>Add User</span>
      </button>
    </div>

    <div class="card overflow-hidden">
      <table class="w-full">
        <thead class="bg-dark-700/50 border-b border-dark-600">
          <tr>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">User</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Role</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Status</th>
            <th class="px-4 py-3 text-left text-xs font-medium text-dark-400 uppercase">Last Login</th>
            <th class="px-4 py-3 text-right text-xs font-medium text-dark-400 uppercase">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-dark-600">
          <tr v-for="user in users" :key="user.id" class="hover:bg-dark-700/30">
            <td class="px-4 py-4">
              <div class="flex items-center space-x-3">
                <div class="w-8 h-8 bg-accent-500 rounded-full flex items-center justify-center">
                  <span class="text-sm font-medium text-white">{{ user.username.slice(0, 2).toUpperCase() }}</span>
                </div>
                <div>
                  <p class="text-white font-medium">{{ user.username }}</p>
                  <p class="text-xs text-dark-500">{{ user.email }}</p>
                </div>
              </div>
            </td>
            <td class="px-4 py-4 text-dark-300">{{ user.role }}</td>
            <td class="px-4 py-4">
              <span :class="['px-2 py-1 text-xs rounded-full', user.status === 'active' ? 'bg-green-500/10 text-green-400' : 'bg-gray-500/10 text-gray-400']">
                {{ user.status }}
              </span>
            </td>
            <td class="px-4 py-4 text-dark-300">{{ user.lastLogin }}</td>
            <td class="px-4 py-4 text-right">
              <button class="text-sm text-accent-400 hover:text-accent-300 mr-4">Edit</button>
              <button class="text-sm text-red-400 hover:text-red-300">Delete</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
