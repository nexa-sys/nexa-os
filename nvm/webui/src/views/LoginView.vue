<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useNotificationStore } from '@/stores/notification'

const router = useRouter()
const authStore = useAuthStore()
const notificationStore = useNotificationStore()

const username = ref('')
const password = ref('')
const loading = ref(false)
const showPassword = ref(false)

onMounted(() => {
  // If already logged in, redirect to dashboard
  if (authStore.isAuthenticated) {
    router.push('/')
  }
})

async function handleLogin() {
  if (!username.value || !password.value) {
    notificationStore.warning('Validation Error', 'Please enter username and password')
    return
  }

  loading.value = true
  
  const success = await authStore.login(username.value, password.value)
  
  if (success) {
    notificationStore.success('Welcome back!', `Logged in as ${username.value}`)
    router.push('/')
  } else {
    notificationStore.error('Login Failed', authStore.error || 'Invalid credentials')
  }
  
  loading.value = false
}
</script>

<template>
  <div class="min-h-screen bg-dark-900 flex items-center justify-center px-4">
    <div class="w-full max-w-md">
      <!-- Logo -->
      <div class="text-center mb-8">
        <div class="inline-flex items-center justify-center w-16 h-16 bg-accent-500 rounded-2xl mb-4">
          <svg class="w-10 h-10 text-white" fill="currentColor" viewBox="0 0 20 20">
            <path d="M10 2L3 7v9l7 5 7-5V7l-7-5z"/>
          </svg>
        </div>
        <h1 class="text-2xl font-bold text-white">NVM Hypervisor</h1>
        <p class="text-dark-400 mt-2">Enterprise Virtualization Platform</p>
      </div>

      <!-- Login Form -->
      <div class="bg-dark-800 rounded-xl border border-dark-600 p-8">
        <form @submit.prevent="handleLogin" class="space-y-6">
          <div>
            <label for="username" class="block text-sm font-medium text-dark-300 mb-2">
              Username
            </label>
            <input
              id="username"
              v-model="username"
              type="text"
              autocomplete="username"
              class="input w-full"
              placeholder="Enter your username"
              :disabled="loading"
            />
          </div>

          <div>
            <label for="password" class="block text-sm font-medium text-dark-300 mb-2">
              Password
            </label>
            <div class="relative">
              <input
                id="password"
                v-model="password"
                :type="showPassword ? 'text' : 'password'"
                autocomplete="current-password"
                class="input w-full pr-10"
                placeholder="Enter your password"
                :disabled="loading"
              />
              <button
                type="button"
                class="absolute right-3 top-1/2 -translate-y-1/2 text-dark-400 hover:text-white"
                @click="showPassword = !showPassword"
              >
                <svg v-if="showPassword" class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21"/>
                </svg>
                <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/>
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"/>
                </svg>
              </button>
            </div>
          </div>

          <div class="flex items-center justify-between">
            <label class="flex items-center">
              <input type="checkbox" class="w-4 h-4 rounded border-dark-600 bg-dark-700 text-accent-500 focus:ring-accent-500 focus:ring-offset-dark-800">
              <span class="ml-2 text-sm text-dark-400">Remember me</span>
            </label>
            <a href="#" class="text-sm text-accent-400 hover:text-accent-300">
              Forgot password?
            </a>
          </div>

          <button
            type="submit"
            class="btn-primary w-full flex items-center justify-center"
            :disabled="loading"
          >
            <svg v-if="loading" class="animate-spin -ml-1 mr-2 h-4 w-4 text-white" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
            </svg>
            {{ loading ? 'Signing in...' : 'Sign in' }}
          </button>
        </form>
      </div>

      <!-- Footer -->
      <div class="mt-8 text-center text-sm text-dark-500">
        <p>NVM Hypervisor v1.0.0</p>
        <p class="mt-1">© 2025 NexaOS Project. All rights reserved.</p>
      </div>
    </div>
  </div>
</template>
