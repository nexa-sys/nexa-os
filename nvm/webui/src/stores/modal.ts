import { defineStore } from 'pinia'
import { ref, shallowRef, markRaw } from 'vue'
import type { Component } from 'vue'

export type ConfirmDialogType = 'danger' | 'warning' | 'info' | 'success'

export interface ModalOptions {
  title?: string
  size?: 'sm' | 'md' | 'lg' | 'xl' | 'full'
  closable?: boolean
  props?: Record<string, any>
  onClose?: () => void
  onConfirm?: () => void | Promise<void>
}

export interface Modal {
  id: string
  component: Component
  options: ModalOptions
}

let modalId = 0

export const useModalStore = defineStore('modal', () => {
  const modals = ref<Modal[]>([])
  const confirmDialog = ref<{
    visible: boolean
    title: string
    message: string
    type: ConfirmDialogType
    confirmText: string
    cancelText: string
    resolve: ((value: boolean) => void) | null
  }>({
    visible: false,
    title: '',
    message: '',
    type: 'warning',
    confirmText: 'Confirm',
    cancelText: 'Cancel',
    resolve: null,
  })

  function open(component: Component, options: ModalOptions = {}) {
    const id = `modal-${++modalId}`
    modals.value.push({
      id,
      component: markRaw(component),
      options: {
        closable: true,
        size: 'md',
        ...options,
      },
    })
    return id
  }

  function close(id?: string) {
    if (id) {
      const modal = modals.value.find(m => m.id === id)
      modal?.options.onClose?.()
      modals.value = modals.value.filter(m => m.id !== id)
    } else if (modals.value.length > 0) {
      const modal = modals.value[modals.value.length - 1]
      modal.options.onClose?.()
      modals.value.pop()
    }
  }

  function closeAll() {
    modals.value.forEach(m => m.options.onClose?.())
    modals.value = []
  }

  function confirm(options: {
    title: string
    message: string
    type?: ConfirmDialogType
    confirmText?: string
    cancelText?: string
  }): Promise<boolean> {
    return new Promise((resolve) => {
      confirmDialog.value = {
        visible: true,
        title: options.title,
        message: options.message,
        type: options.type || 'warning',
        confirmText: options.confirmText || 'Confirm',
        cancelText: options.cancelText || 'Cancel',
        resolve,
      }
    })
  }

  function resolveConfirm(result: boolean) {
    confirmDialog.value.resolve?.(result)
    confirmDialog.value.visible = false
    confirmDialog.value.resolve = null
  }

  return {
    modals,
    confirmDialog,
    open,
    close,
    closeAll,
    confirm,
    resolveConfirm,
  }
})
