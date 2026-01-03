<script setup lang="ts">
import { useModalStore } from '@/stores/modal'

const modalStore = useModalStore()

const sizeClasses: Record<string, string> = {
  sm: 'max-w-md',
  md: 'max-w-lg',
  lg: 'max-w-2xl',
  xl: 'max-w-4xl',
  full: 'max-w-full mx-4',
}

const typeIcons: Record<string, string> = {
  danger: 'M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z',
  warning: 'M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z',
  info: 'M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z',
  success: 'M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z',
}

function closeModal(id: string) {
  modalStore.close(id)
}
</script>

<template>
  <Teleport to="body">
    <!-- Dynamic Modals -->
    <TransitionGroup name="modal">
      <div
        v-for="modal in modalStore.modals"
        :key="modal.id"
        class="modal-container"
      >
        <!-- Backdrop -->
        <div
          class="modal-overlay"
          @click="modal.options.closable ? closeModal(modal.id) : null"
        />
        
        <!-- Modal Content -->
        <div :class="['modal', sizeClasses[modal.options.size || 'md']]">
          <!-- Header -->
          <div v-if="modal.options.title" class="modal-header">
            <h3 class="modal-title">{{ modal.options.title }}</h3>
            <button
              v-if="modal.options.closable"
              class="text-dark-500 hover:text-white p-1 -mr-1 transition-colors"
              @click="closeModal(modal.id)"
            >
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
              </svg>
            </button>
          </div>
          
          <!-- Body -->
          <component
            :is="modal.component"
            v-bind="modal.options.props"
            @close="closeModal(modal.id)"
          />
        </div>
      </div>
    </TransitionGroup>

    <!-- Confirm Dialog -->
    <Transition name="modal">
      <div
        v-if="modalStore.confirmDialog.visible"
        class="modal-container"
      >
        <div
          class="modal-overlay"
          @click="modalStore.resolveConfirm(false)"
        />
        
        <div class="modal max-w-md">
          <div class="p-6">
            <!-- Icon -->
            <div :class="[
              'w-14 h-14 rounded-full flex items-center justify-center mx-auto mb-5',
              {
                'bg-danger-900/30 text-danger-400': modalStore.confirmDialog.type === 'danger',
                'bg-warning-900/30 text-warning-400': modalStore.confirmDialog.type === 'warning',
                'bg-primary-900/30 text-primary-400': modalStore.confirmDialog.type === 'info',
                'bg-success-900/30 text-success-400': modalStore.confirmDialog.type === 'success',
              }
            ]">
              <svg class="w-7 h-7" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" :d="typeIcons[modalStore.confirmDialog.type || 'info']"/>
              </svg>
            </div>
            
            <!-- Title -->
            <h3 class="text-lg font-semibold text-white text-center mb-2">
              {{ modalStore.confirmDialog.title }}
            </h3>
            
            <!-- Message -->
            <p class="text-dark-400 text-center mb-6 leading-relaxed">
              {{ modalStore.confirmDialog.message }}
            </p>
            
            <!-- Actions -->
            <div class="flex gap-3">
              <button
                class="flex-1 btn-secondary"
                @click="modalStore.resolveConfirm(false)"
              >
                {{ modalStore.confirmDialog.cancelText }}
              </button>
              <button
                :class="[
                  'flex-1',
                  modalStore.confirmDialog.type === 'danger' ? 'btn-danger' : 'btn-primary'
                ]"
                @click="modalStore.resolveConfirm(true)"
              >
                {{ modalStore.confirmDialog.confirmText }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-enter-active {
  transition: all 0.25s cubic-bezier(0.16, 1, 0.3, 1);
}

.modal-leave-active {
  transition: all 0.15s ease-out;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from .modal,
.modal-leave-to .modal {
  transform: scale(0.95) translateY(10px);
}
</style>
