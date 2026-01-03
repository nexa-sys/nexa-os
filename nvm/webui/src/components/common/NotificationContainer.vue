<script setup lang="ts">
import { useNotificationStore } from '@/stores/notification'
import type { NotificationType } from '@/stores/notification'

const notificationStore = useNotificationStore()

const iconMap: Record<NotificationType, string> = {
  success: 'M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z',
  error: 'M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z',
  warning: 'M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z',
  info: 'M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z',
}

const styleMap: Record<NotificationType, { icon: string; border: string; bg: string }> = {
  success: { 
    icon: 'text-success-400', 
    border: 'border-success-600/30', 
    bg: 'bg-success-900/20'
  },
  error: { 
    icon: 'text-danger-400', 
    border: 'border-danger-600/30', 
    bg: 'bg-danger-900/20'
  },
  warning: { 
    icon: 'text-warning-400', 
    border: 'border-warning-600/30', 
    bg: 'bg-warning-900/20'
  },
  info: { 
    icon: 'text-primary-400', 
    border: 'border-primary-600/30', 
    bg: 'bg-primary-900/20'
  },
}
</script>

<template>
  <div class="fixed top-16 right-4 z-50 space-y-3 w-96 pointer-events-none">
    <TransitionGroup name="notification">
      <div
        v-for="notification in notificationStore.notifications"
        :key="notification.id"
        :class="[
          'pointer-events-auto bg-dark-800 rounded-lg shadow-modal border p-4 flex items-start gap-3',
          styleMap[notification.type].border
        ]"
      >
        <!-- Icon -->
        <div :class="['p-1.5 rounded-lg flex-shrink-0', styleMap[notification.type].bg]">
          <svg :class="['w-5 h-5', styleMap[notification.type].icon]" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" :d="iconMap[notification.type]"/>
          </svg>
        </div>
        
        <!-- Content -->
        <div class="flex-1 min-w-0 pt-0.5">
          <p class="text-sm font-medium text-white">{{ notification.title }}</p>
          <p v-if="notification.message" class="text-sm text-dark-400 mt-1 leading-relaxed">{{ notification.message }}</p>
        </div>
        
        <!-- Close button -->
        <button
          v-if="notification.closable"
          class="flex-shrink-0 text-dark-500 hover:text-white transition-colors p-1 -mr-1 -mt-1"
          @click="notificationStore.remove(notification.id)"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
          </svg>
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.notification-enter-active {
  transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
}

.notification-leave-active {
  transition: all 0.2s ease-out;
}

.notification-enter-from {
  opacity: 0;
  transform: translateX(100%) scale(0.95);
}

.notification-leave-to {
  opacity: 0;
  transform: translateX(20px) scale(0.95);
}

.notification-move {
  transition: transform 0.3s cubic-bezier(0.16, 1, 0.3, 1);
}
</style>
