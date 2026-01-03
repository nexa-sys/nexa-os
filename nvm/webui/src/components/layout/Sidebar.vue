<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute, RouterLink } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const authStore = useAuthStore()
const collapsed = ref(false)
const clusterStatus = ref<'healthy' | 'warning' | 'error'>('healthy')

interface NavItem {
  name: string
  path: string
  icon: string
  permission?: string
  badge?: number
  children?: NavItem[]
}

interface NavSection {
  title: string
  items: NavItem[]
}

const navSections = ref<NavSection[]>([
  {
    title: 'Infrastructure',
    items: [
      { name: 'Dashboard', path: '/', icon: 'dashboard' },
      { name: 'Virtual Machines', path: '/vms', icon: 'server' },
      { name: 'Templates', path: '/templates', icon: 'template' },
    ]
  },
  {
    title: 'Resources',
    items: [
      { name: 'Storage', path: '/storage', icon: 'storage' },
      { name: 'Network', path: '/network', icon: 'network' },
      { name: 'Cluster', path: '/cluster', icon: 'cluster' },
    ]
  },
  {
    title: 'Operations',
    items: [
      { name: 'Backup & Recovery', path: '/backup', icon: 'backup' },
      { name: 'Tasks', path: '/tasks', icon: 'tasks' },
    ]
  },
  {
    title: 'Administration',
    items: [
      { name: 'Users & Roles', path: '/users', icon: 'users', permission: 'admin' },
      { name: 'Settings', path: '/settings', icon: 'settings' },
    ]
  },
])

const filteredSections = computed(() => {
  return navSections.value.map(section => ({
    ...section,
    items: section.items.filter(item => {
      if (!item.permission) return true
      return authStore.hasPermission(item.permission) || authStore.isAdmin
    })
  })).filter(section => section.items.length > 0)
})

const isActive = (path: string) => {
  if (path === '/') return route.path === '/'
  return route.path.startsWith(path)
}

const clusterStatusColor = computed(() => {
  switch (clusterStatus.value) {
    case 'healthy': return 'bg-success-500'
    case 'warning': return 'bg-warning-500'
    case 'error': return 'bg-danger-500'
    default: return 'bg-dark-500'
  }
})

const clusterStatusText = computed(() => {
  switch (clusterStatus.value) {
    case 'healthy': return 'All Systems Operational'
    case 'warning': return 'Warning: Check Alerts'
    case 'error': return 'Critical Issues Detected'
    default: return 'Status Unknown'
  }
})

// Icon components as SVG paths
const icons: Record<string, string> = {
  dashboard: 'M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6',
  server: 'M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01',
  template: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z',
  storage: 'M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4',
  network: 'M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9',
  cluster: 'M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10',
  backup: 'M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12',
  tasks: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4',
  users: 'M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z',
  settings: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z',
}

function toggleCollapse() {
  collapsed.value = !collapsed.value
  localStorage.setItem('nvm_sidebar_collapsed', String(collapsed.value))
}

onMounted(() => {
  const savedState = localStorage.getItem('nvm_sidebar_collapsed')
  if (savedState === 'true') {
    collapsed.value = true
  }
})
</script>

<template>
  <aside 
    :class="[
      'fixed left-0 top-0 h-full bg-dark-900 border-r border-dark-800 flex flex-col z-30 transition-all duration-300',
      collapsed ? 'w-16' : 'w-64'
    ]"
  >
    <!-- Logo Header -->
    <div class="h-14 flex items-center border-b border-dark-800 px-3">
      <RouterLink to="/" class="flex items-center gap-3 min-w-0">
        <div class="w-8 h-8 bg-gradient-to-br from-primary-500 to-primary-700 rounded-lg flex items-center justify-center flex-shrink-0">
          <svg class="w-5 h-5 text-white" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 2L4 7v10l8 5 8-5V7l-8-5zm0 2.18l5.7 3.57v7.5L12 18.82l-5.7-3.57v-7.5L12 4.18z"/>
            <path d="M12 8a4 4 0 100 8 4 4 0 000-8zm0 2a2 2 0 110 4 2 2 0 010-4z"/>
          </svg>
        </div>
        <span v-if="!collapsed" class="text-lg font-bold text-white truncate">NVM Enterprise</span>
      </RouterLink>
    </div>

    <!-- Cluster Status Banner -->
    <div v-if="!collapsed" class="mx-3 mt-3 p-2.5 bg-dark-800/50 rounded-lg border border-dark-700">
      <div class="flex items-center gap-2">
        <span :class="['w-2 h-2 rounded-full', clusterStatusColor]"></span>
        <span class="text-xs text-dark-300 truncate">{{ clusterStatusText }}</span>
      </div>
    </div>
    <div v-else class="mx-auto mt-3">
      <span :class="['w-3 h-3 rounded-full block', clusterStatusColor]"></span>
    </div>

    <!-- Navigation -->
    <nav class="flex-1 py-4 overflow-y-auto scrollbar-hide">
      <template v-for="(section, sIdx) in filteredSections" :key="sIdx">
        <!-- Section Title -->
        <div v-if="!collapsed" class="px-4 py-2 mt-2 first:mt-0">
          <span class="text-2xs font-semibold text-dark-500 uppercase tracking-wider">
            {{ section.title }}
          </span>
        </div>
        <div v-else class="h-px bg-dark-800 mx-3 my-2"></div>

        <!-- Section Items -->
        <ul class="space-y-0.5 px-2">
          <li v-for="item in section.items" :key="item.path">
            <RouterLink
              :to="item.path"
              :class="[
                'flex items-center gap-3 px-3 py-2.5 rounded-lg transition-all duration-200 group relative',
                isActive(item.path)
                  ? 'bg-primary-600/15 text-primary-400'
                  : 'text-dark-400 hover:text-white hover:bg-dark-800/50'
              ]"
              :title="collapsed ? item.name : undefined"
            >
              <!-- Active indicator -->
              <div 
                v-if="isActive(item.path)" 
                class="absolute left-0 top-1/2 -translate-y-1/2 w-0.5 h-6 bg-primary-500 rounded-r"
              ></div>
              
              <!-- Icon -->
              <svg class="w-5 h-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" :d="icons[item.icon]"/>
              </svg>
              
              <!-- Label -->
              <span v-if="!collapsed" class="text-sm font-medium truncate">{{ item.name }}</span>
              
              <!-- Badge -->
              <span
                v-if="item.badge && !collapsed"
                class="ml-auto bg-primary-500 text-white text-2xs px-1.5 py-0.5 rounded-full font-medium"
              >
                {{ item.badge }}
              </span>

              <!-- Tooltip for collapsed state -->
              <div 
                v-if="collapsed"
                class="absolute left-full ml-2 px-2 py-1 bg-dark-700 text-white text-sm rounded shadow-lg 
                       opacity-0 pointer-events-none group-hover:opacity-100 transition-opacity whitespace-nowrap z-50"
              >
                {{ item.name }}
                <span v-if="item.badge" class="ml-2 bg-primary-500 text-2xs px-1.5 py-0.5 rounded-full">
                  {{ item.badge }}
                </span>
              </div>
            </RouterLink>
          </li>
        </ul>
      </template>
    </nav>

    <!-- Footer -->
    <div class="border-t border-dark-800">
      <!-- Collapse Toggle -->
      <button 
        @click="toggleCollapse"
        class="w-full h-10 flex items-center justify-center text-dark-500 hover:text-white hover:bg-dark-800/50 transition-colors"
        :title="collapsed ? 'Expand sidebar' : 'Collapse sidebar'"
      >
        <svg 
          class="w-5 h-5 transition-transform duration-300" 
          :class="{ 'rotate-180': collapsed }"
          fill="none" stroke="currentColor" viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M11 19l-7-7 7-7m8 14l-7-7 7-7"/>
        </svg>
      </button>
      
      <!-- Version Info -->
      <div v-if="!collapsed" class="px-4 py-3 border-t border-dark-800">
        <div class="flex items-center justify-between text-2xs text-dark-500">
          <span>NVM Enterprise</span>
          <span>v1.0.0</span>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
/* Smooth sidebar transitions */
.router-link-active {
  @apply relative;
}

/* Tooltip arrow */
.group:hover .absolute {
  @apply opacity-100;
}
</style>
