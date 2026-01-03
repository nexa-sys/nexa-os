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
        class="fixed inset-0 z-50 flex items-center justify-center"
      >
        <!-- Backdrop -->
        <div
          class="absolute inset-0 bg-black/60 backdrop-blur-sm"
          @click="modal.options.closable ? closeModal(modal.id) : null"
        />
        
        <!-- Modal Content -->
        <div :class="['relative bg-dark-800 rounded-xl shadow-2xl border border-dark-600 w-full', sizeClasses[modal.options.size || 'md']]">
          <!-- Header -->
          <div v-if="modal.options.title" class="flex items-center justify-between px-6 py-4 border-b border-dark-600">
            <h3 class="text-lg font-semibold text-white">{{ modal.options.title }}</h3>
            <button
              v-if="modal.options.closable"
              class="text-dark-400 hover:text-white p-1"
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
        class="fixed inset-0 z-50 flex items-center justify-center"
      >
        <div
          class="absolute inset-0 bg-black/60 backdrop-blur-sm"
          @click="modalStore.resolveConfirm(false)"
        />
        
        <div class="relative bg-dark-800 rounded-xl shadow-2xl border border-dark-600 w-full max-w-md">
          <div class="p-6">
            <div :class="[
              'w-12 h-12 rounded-full flex items-center justify-center mx-auto mb-4',
              {
                'bg-red-500/10 text-red-400': modalStore.confirmDialog.type === 'danger',
                'bg-yellow-500/10 text-yellow-400': modalStore.confirmDialog.type === 'warning',
                'bg-blue-500/10 text-blue-400': modalStore.confirmDialog.type === 'info',
              }
            ]">
              <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
              </svg>
            </div>
            
            <h3 class="text-lg font-semibold text-white text-center mb-2">
              {{ modalStore.confirmDialog.title }}
            </h3>
            <p class="text-dark-400 text-center mb-6">
              {{ modalStore.confirmDialog.message }}
            </p>
            
            <div class="flex space-x-3">
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
.modal-enter-active,
.modal-leave-active {
  transition: all 0.2s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from > div:last-child,
.modal-leave-to > div:last-child {
  transform: scale(0.95);
}
</style>
