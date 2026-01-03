<template>
  <div class="min-h-screen bg-dark-950">
    <!-- Sidebar -->
    <Sidebar v-if="authStore.isAuthenticated" />
    
    <!-- Main Content -->
    <main 
      class="min-h-screen flex flex-col transition-all duration-300"
      :class="mainContentClass"
    >
      <!-- Top Bar -->
      <TopBar v-if="authStore.isAuthenticated" />
      
      <!-- Page Content -->
      <div class="flex-1 overflow-auto">
        <RouterView />
      </div>
    </main>
    
    <!-- Global Notifications -->
    <NotificationContainer />
    
    <!-- Modal Container -->
    <ModalContainer />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { RouterView } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import Sidebar from '@/components/layout/Sidebar.vue'
import TopBar from '@/components/layout/TopBar.vue'
import NotificationContainer from '@/components/common/NotificationContainer.vue'
import ModalContainer from '@/components/common/ModalContainer.vue'

const authStore = useAuthStore()

// Dynamic margin based on sidebar state (stored in localStorage)
const mainContentClass = computed(() => {
  if (!authStore.isAuthenticated) return ''
  const collapsed = localStorage.getItem('nvm_sidebar_collapsed') === 'true'
  return collapsed ? 'ml-16' : 'ml-64'
})
</script>
