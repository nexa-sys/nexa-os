<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useNotificationStore } from '@/stores/notification'

const router = useRouter()
const route = useRoute()
const authStore = useAuthStore()
const notificationStore = useNotificationStore()

const showUserMenu = ref(false)
const showNotifications = ref(false)
const globalSearchQuery = ref('')
const showSearch = ref(false)

const userInitials = computed(() => {
  if (!authStore.user?.username) return '?'
  return authStore.user.username.slice(0, 2).toUpperCase()
})

// Dynamic breadcrumbs based on current route
const breadcrumbs = computed(() => {
  const crumbs = []
  const pathParts = route.path.split('/').filter(Boolean)
  
  // Map paths to friendly names
  const nameMap: Record<string, string> = {
    'vms': 'Virtual Machines',
    'storage': 'Storage',
    'network': 'Network',
    'cluster': 'Cluster',
    'templates': 'Templates',
    'backup': 'Backup & Recovery',
    'users': 'Users & Roles',
    'settings': 'Settings',
    'tasks': 'Tasks',
    'create': 'Create',
    'edit': 'Edit',
    'console': 'Console',
  }

  if (pathParts.length === 0) {
    crumbs.push({ name: 'Dashboard', path: '/' })
  } else {
    crumbs.push({ name: 'Dashboard', path: '/' })
    let currentPath = ''
    for (const part of pathParts) {
      currentPath += '/' + part
      const name = nameMap[part] || part.charAt(0).toUpperCase() + part.slice(1)
      crumbs.push({ name, path: currentPath })
    }
  }
  
  return crumbs
})

async function handleLogout() {
  showUserMenu.value = false
  await authStore.logout()
  notificationStore.success('Signed Out', 'You have been logged out successfully')
  router.push('/login')
}

function closeMenus() {
  showUserMenu.value = false
  showNotifications.value = false
}

function handleGlobalSearch() {
  if (globalSearchQuery.value.trim()) {
    // TODO: Implement global search
    console.log('Searching for:', globalSearchQuery.value)
  }
}
</script>

<template>
  <header class="h-14 bg-dark-900 border-b border-dark-800 flex items-center justify-between px-6 sticky top-0 z-20">
    <!-- Left side - Breadcrumbs -->
    <nav class="flex items-center space-x-2 text-sm">
      <template v-for="(crumb, idx) in breadcrumbs" :key="crumb.path">
        <RouterLink 
          v-if="idx < breadcrumbs.length - 1"
          :to="crumb.path"
          class="text-dark-400 hover:text-white transition-colors"
        >
          {{ crumb.name }}
        </RouterLink>
        <span v-else class="text-white font-medium">{{ crumb.name }}</span>
        <svg 
          v-if="idx < breadcrumbs.length - 1" 
          class="w-4 h-4 text-dark-600" 
          fill="currentColor" 
          viewBox="0 0 20 20"
        >
          <path fill-rule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clip-rule="evenodd"/>
        </svg>
      </template>
    </nav>

    <!-- Right side - Actions -->
    <div class="flex items-center gap-2">
      <!-- Global Search -->
      <div class="relative">
        <button
          v-if="!showSearch"
          class="p-2 text-dark-400 hover:text-white hover:bg-dark-800 rounded-lg transition-colors"
          @click="showSearch = true"
          title="Search (Ctrl+K)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
          </svg>
        </button>
        <div v-else class="relative">
          <input
            v-model="globalSearchQuery"
            type="text"
            placeholder="Search VMs, storage, networks..."
            class="w-72 bg-dark-800 text-white text-sm rounded-lg pl-10 pr-10 py-2 border border-dark-700 
                   focus:border-primary-500 focus:ring-1 focus:ring-primary-500 focus:outline-none placeholder-dark-500"
            @keydown.enter="handleGlobalSearch"
            @keydown.escape="showSearch = false; globalSearchQuery = ''"
            autofocus
          />
          <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-dark-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
          </svg>
          <button 
            class="absolute right-3 top-1/2 -translate-y-1/2 text-dark-500 hover:text-white"
            @click="showSearch = false; globalSearchQuery = ''"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
            </svg>
          </button>
        </div>
      </div>

      <!-- Quick Actions -->
      <button
        class="btn-primary btn-sm"
        @click="router.push('/vms/create')"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span class="hidden sm:inline">New VM</span>
      </button>

      <!-- Divider -->
      <div class="h-6 w-px bg-dark-700 mx-1"></div>

      <!-- Notifications -->
      <div class="relative">
        <button
          class="relative p-2 text-dark-400 hover:text-white hover:bg-dark-800 rounded-lg transition-colors"
          @click="showNotifications = !showNotifications"
          title="Notifications"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"/>
          </svg>
          <span class="absolute top-1.5 right-1.5 w-2 h-2 bg-primary-500 rounded-full"></span>
        </button>

        <!-- Notifications Dropdown -->
        <div
          v-if="showNotifications"
          class="dropdown-menu w-80 right-0"
          @click.stop
        >
          <div class="px-4 py-3 border-b border-dark-700 flex items-center justify-between">
            <h3 class="text-sm font-semibold text-white">Notifications</h3>
            <button class="text-xs text-primary-400 hover:text-primary-300">Mark all read</button>
          </div>
          <div class="max-h-96 overflow-y-auto">
            <div class="px-4 py-8 text-center text-dark-500 text-sm">
              <svg class="w-8 h-8 mx-auto mb-2 text-dark-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"/>
              </svg>
              No new notifications
            </div>
          </div>
        </div>
      </div>

      <!-- User Menu -->
      <div class="relative">
        <button
          class="flex items-center gap-2 hover:bg-dark-800 rounded-lg px-2 py-1.5 transition-colors"
          @click="showUserMenu = !showUserMenu"
        >
          <div class="w-8 h-8 bg-gradient-to-br from-primary-500 to-primary-700 rounded-lg flex items-center justify-center">
            <span class="text-xs font-semibold text-white">{{ userInitials }}</span>
          </div>
          <div class="text-left hidden lg:block">
            <div class="text-sm font-medium text-white leading-tight">{{ authStore.user?.username || 'User' }}</div>
            <div class="text-2xs text-dark-500">{{ authStore.isAdmin ? 'Administrator' : 'Operator' }}</div>
          </div>
          <svg class="w-4 h-4 text-dark-500 hidden lg:block" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>
          </svg>
        </button>

        <!-- User Dropdown -->
        <div
          v-if="showUserMenu"
          class="dropdown-menu w-56 right-0"
          @click.stop
        >
          <div class="px-4 py-3 border-b border-dark-700">
            <p class="text-sm font-medium text-white">{{ authStore.user?.username }}</p>
            <p class="text-xs text-dark-500 mt-0.5">{{ authStore.user?.email || 'No email set' }}</p>
          </div>
          <div class="py-1">
            <RouterLink to="/settings" class="dropdown-item flex items-center gap-2" @click="showUserMenu = false">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
              </svg>
              Profile Settings
            </RouterLink>
            <a href="#" class="dropdown-item flex items-center gap-2">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z"/>
              </svg>
              API Tokens
            </a>
            <a href="https://docs.nexaos.dev/nvm" target="_blank" class="dropdown-item flex items-center gap-2">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253"/>
              </svg>
              Documentation
            </a>
          </div>
          <div class="dropdown-divider"></div>
          <div class="py-1">
            <button
              class="dropdown-item flex items-center gap-2 text-danger-400 hover:text-danger-300 w-full"
              @click="handleLogout"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"/>
              </svg>
              Sign Out
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Click outside to close -->
    <div
      v-if="showUserMenu || showNotifications"
      class="fixed inset-0 z-40"
      @click="closeMenus"
    />
  </header>
</template>
