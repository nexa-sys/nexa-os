<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRoute, RouterLink } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const authStore = useAuthStore()

interface NavItem {
  name: string
  path: string
  icon: string
  permission?: string
  badge?: number
}

const navItems = ref<NavItem[]>([
  { name: 'Dashboard', path: '/', icon: 'chart-pie' },
  { name: 'Virtual Machines', path: '/vms', icon: 'server' },
  { name: 'Storage', path: '/storage', icon: 'database' },
  { name: 'Network', path: '/network', icon: 'globe' },
  { name: 'Cluster', path: '/cluster', icon: 'cube' },
  { name: 'Templates', path: '/templates', icon: 'document-duplicate' },
  { name: 'Backup', path: '/backup', icon: 'cloud' },
  { name: 'Users', path: '/users', icon: 'users', permission: 'admin' },
  { name: 'Settings', path: '/settings', icon: 'cog' },
])

const filteredNavItems = computed(() => {
  return navItems.value.filter(item => {
    if (!item.permission) return true
    return authStore.hasPermission(item.permission) || authStore.isAdmin
  })
})

const isActive = (path: string) => {
  if (path === '/') return route.path === '/'
  return route.path.startsWith(path)
}

const iconMap: Record<string, string> = {
  'chart-pie': 'M11 3.055A9.001 9.001 0 1020.945 13H11V3.055z M20.488 9H15V3.512A9.025 9.025 0 0120.488 9z',
  'server': 'M4 4v5h12V4H4zm0 7v5h12v-5H4z M6 6h2v1H6V6zm0 7h2v1H6v-1z',
  'database': 'M3 12v3c0 1.657 3.134 3 7 3s7-1.343 7-3v-3c0 1.657-3.134 3-7 3s-7-1.343-7-3z M3 7v3c0 1.657 3.134 3 7 3s7-1.343 7-3V7c0 1.657-3.134 3-7 3S3 8.657 3 7z M17 5c0 1.657-3.134 3-7 3S3 6.657 3 5s3.134-3 7-3 7 1.343 7 3z',
  'globe': 'M10 18a8 8 0 100-16 8 8 0 000 16zM4.332 8.027a6.012 6.012 0 011.912-2.706C6.512 5.73 6.974 6 7.5 6A1.5 1.5 0 019 7.5V8a2 2 0 004 0 2 2 0 011.523-1.943A5.977 5.977 0 0116 10c0 .34-.028.675-.083 1H15a2 2 0 00-2 2v2.197A5.973 5.973 0 0110 16v-2a2 2 0 00-2-2 2 2 0 01-2-2 2 2 0 00-1.668-1.973z',
  'cube': 'M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z',
  'document-duplicate': 'M7 9a2 2 0 012-2h6a2 2 0 012 2v6a2 2 0 01-2 2H9a2 2 0 01-2-2V9z M5 3a2 2 0 00-2 2v6a2 2 0 002 2V5h8a2 2 0 00-2-2H5z',
  'cloud': 'M5.5 16a3.5 3.5 0 01-.369-6.98 4 4 0 117.753-1.977A4.5 4.5 0 1113.5 16h-8z',
  'users': 'M9 6a3 3 0 11-6 0 3 3 0 016 0zM17 6a3 3 0 11-6 0 3 3 0 016 0zM12.93 17c.046-.327.07-.66.07-1a6.97 6.97 0 00-1.5-4.33A5 5 0 0119 16v1h-6.07zM6 11a5 5 0 015 5v1H1v-1a5 5 0 015-5z',
  'cog': 'M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z',
}
</script>

<template>
  <aside class="w-64 bg-dark-800 border-r border-dark-600 flex flex-col h-full">
    <!-- Logo -->
    <div class="h-16 flex items-center px-6 border-b border-dark-600">
      <div class="flex items-center space-x-3">
        <div class="w-8 h-8 bg-accent-500 rounded-lg flex items-center justify-center">
          <svg class="w-5 h-5 text-white" fill="currentColor" viewBox="0 0 20 20">
            <path d="M10 2L3 7v9l7 5 7-5V7l-7-5z"/>
          </svg>
        </div>
        <span class="text-xl font-bold text-white">NVM</span>
      </div>
    </div>

    <!-- Navigation -->
    <nav class="flex-1 py-4 overflow-y-auto">
      <ul class="space-y-1 px-3">
        <li v-for="item in filteredNavItems" :key="item.path">
          <RouterLink
            :to="item.path"
            :class="[
              'flex items-center px-3 py-2.5 rounded-lg text-sm font-medium transition-colors',
              isActive(item.path)
                ? 'bg-accent-500/10 text-accent-400 border-l-2 border-accent-500'
                : 'text-dark-300 hover:bg-dark-700 hover:text-white'
            ]"
          >
            <svg class="w-5 h-5 mr-3 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
              <path :d="iconMap[item.icon]" fill-rule="evenodd" clip-rule="evenodd"/>
            </svg>
            {{ item.name }}
            <span
              v-if="item.badge"
              class="ml-auto bg-accent-500 text-white text-xs px-2 py-0.5 rounded-full"
            >
              {{ item.badge }}
            </span>
          </RouterLink>
        </li>
      </ul>
    </nav>

    <!-- Footer -->
    <div class="p-4 border-t border-dark-600">
      <div class="flex items-center text-xs text-dark-400">
        <span>NVM Hypervisor</span>
        <span class="mx-2">•</span>
        <span>v1.0.0</span>
      </div>
    </div>
  </aside>
</template>
