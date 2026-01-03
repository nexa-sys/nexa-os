<template>
  <div class="min-h-screen flex">
    <!-- Sidebar -->
    <Sidebar v-if="authStore.isAuthenticated" />
    
    <!-- Main Content -->
    <main class="flex-1 flex flex-col" :class="{ 'ml-64': authStore.isAuthenticated }">
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
import { RouterView } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import Sidebar from '@/components/layout/Sidebar.vue'
import TopBar from '@/components/layout/TopBar.vue'
import NotificationContainer from '@/components/common/NotificationContainer.vue'
import ModalContainer from '@/components/common/ModalContainer.vue'

const authStore = useAuthStore()
</script>
