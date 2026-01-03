<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useNotificationStore } from '@/stores/notification'

const router = useRouter()
const authStore = useAuthStore()
const notificationStore = useNotificationStore()

const showUserMenu = ref(false)
const showNotifications = ref(false)

const userInitials = computed(() => {
  if (!authStore.user?.username) return '?'
  return authStore.user.username.slice(0, 2).toUpperCase()
})

async function handleLogout() {
  await authStore.logout()
  notificationStore.success('Logged out', 'You have been logged out successfully')
  router.push('/login')
}

function closeMenus() {
  showUserMenu.value = false
  showNotifications.value = false
}
</script>

<template>
  <header class="h-16 bg-dark-800 border-b border-dark-600 flex items-center justify-between px-6">
    <!-- Left side - Breadcrumbs / Search -->
    <div class="flex items-center flex-1">
      <div class="relative w-96">
        <input
          type="text"
          placeholder="Search VMs, storage, networks..."
          class="w-full bg-dark-700 text-white text-sm rounded-lg pl-10 pr-4 py-2.5 border border-dark-600 focus:border-accent-500 focus:ring-1 focus:ring-accent-500 focus:outline-none placeholder-dark-400"
        />
        <svg
          class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-dark-400"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
        </svg>
      </div>
    </div>

    <!-- Right side - Actions -->
    <div class="flex items-center space-x-4">
      <!-- Quick Actions -->
      <button
        class="btn-primary flex items-center space-x-2"
        @click="router.push('/vms/create')"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
        </svg>
        <span>New VM</span>
      </button>

      <!-- Notifications -->
      <div class="relative">
        <button
          class="relative p-2 text-dark-300 hover:text-white hover:bg-dark-700 rounded-lg transition-colors"
          @click="showNotifications = !showNotifications"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"/>
          </svg>
          <span class="absolute top-1 right-1 w-2 h-2 bg-accent-500 rounded-full"></span>
        </button>

        <!-- Notifications Dropdown -->
        <div
          v-if="showNotifications"
          class="absolute right-0 mt-2 w-80 bg-dark-700 rounded-lg shadow-xl border border-dark-600 py-2 z-50"
          @click.stop
        >
          <div class="px-4 py-2 border-b border-dark-600">
            <h3 class="text-sm font-medium text-white">Notifications</h3>
          </div>
          <div class="max-h-96 overflow-y-auto">
            <div class="px-4 py-8 text-center text-dark-400 text-sm">
              No new notifications
            </div>
          </div>
        </div>
      </div>

      <!-- User Menu -->
      <div class="relative">
        <button
          class="flex items-center space-x-3 hover:bg-dark-700 rounded-lg px-3 py-2 transition-colors"
          @click="showUserMenu = !showUserMenu"
        >
          <div class="w-8 h-8 bg-accent-500 rounded-full flex items-center justify-center">
            <span class="text-sm font-medium text-white">{{ userInitials }}</span>
          </div>
          <div class="text-left hidden md:block">
            <div class="text-sm font-medium text-white">{{ authStore.user?.username || 'User' }}</div>
            <div class="text-xs text-dark-400">{{ authStore.isAdmin ? 'Administrator' : 'User' }}</div>
          </div>
          <svg class="w-4 h-4 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>
          </svg>
        </button>

        <!-- User Dropdown -->
        <div
          v-if="showUserMenu"
          class="absolute right-0 mt-2 w-48 bg-dark-700 rounded-lg shadow-xl border border-dark-600 py-2 z-50"
          @click.stop
        >
          <a href="#" class="block px-4 py-2 text-sm text-dark-300 hover:bg-dark-600 hover:text-white">
            Profile Settings
          </a>
          <a href="#" class="block px-4 py-2 text-sm text-dark-300 hover:bg-dark-600 hover:text-white">
            API Tokens
          </a>
          <hr class="my-2 border-dark-600">
          <button
            class="w-full text-left px-4 py-2 text-sm text-red-400 hover:bg-dark-600"
            @click="handleLogout"
          >
            Sign Out
          </button>
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
