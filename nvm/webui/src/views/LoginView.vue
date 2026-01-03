<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
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
const rememberMe = ref(false)
const loginError = ref('')

// System info for footer
const systemInfo = ref({
  version: '1.0.0',
  build: '20250124',
  hostname: window.location.hostname,
  secure: window.location.protocol === 'https:',
})

// Current time for display
const currentTime = ref(new Date().toLocaleTimeString())
setInterval(() => {
  currentTime.value = new Date().toLocaleTimeString()
}, 1000)

// Connection status
const connectionStatus = computed(() => {
  return systemInfo.value.secure ? 'Secure Connection (TLS)' : 'Unencrypted Connection'
})

onMounted(() => {
  if (authStore.isAuthenticated) {
    router.push('/')
  }
  // Restore remembered username
  const savedUsername = localStorage.getItem('nvm_remember_user')
  if (savedUsername) {
    username.value = savedUsername
    rememberMe.value = true
  }
})

async function handleLogin() {
  loginError.value = ''
  
  if (!username.value.trim()) {
    loginError.value = 'Username is required'
    return
  }
  if (!password.value) {
    loginError.value = 'Password is required'
    return
  }

  loading.value = true
  
  const success = await authStore.login(username.value.trim(), password.value)
  
  if (success) {
    // Handle remember me
    if (rememberMe.value) {
      localStorage.setItem('nvm_remember_user', username.value.trim())
    } else {
      localStorage.removeItem('nvm_remember_user')
    }
    
    notificationStore.success('Welcome', `Logged in as ${authStore.user?.username || username.value}`)
    router.push('/')
  } else {
    loginError.value = authStore.error || 'Authentication failed. Please check your credentials.'
  }
  
  loading.value = false
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !loading.value) {
    handleLogin()
  }
}
</script>

<template>
  <div class="min-h-screen login-background flex flex-col">
    <!-- Top Bar -->
    <header class="h-12 bg-dark-950/80 backdrop-blur-sm border-b border-dark-700 flex items-center px-6">
      <div class="flex items-center space-x-3">
        <div class="w-7 h-7 bg-gradient-to-br from-primary-500 to-primary-700 rounded flex items-center justify-center">
          <svg class="w-4 h-4 text-white" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 2L4 7v10l8 5 8-5V7l-8-5zm0 2.18l5.7 3.57v7.5L12 18.82l-5.7-3.57v-7.5L12 4.18z"/>
            <path d="M12 8a4 4 0 100 8 4 4 0 000-8zm0 2a2 2 0 110 4 2 2 0 010-4z"/>
          </svg>
        </div>
        <span class="text-white font-semibold">NVM Enterprise Hypervisor</span>
      </div>
      <div class="ml-auto flex items-center space-x-4 text-sm">
        <div class="flex items-center space-x-2 text-dark-400">
          <svg v-if="systemInfo.secure" class="w-4 h-4 text-green-500" fill="currentColor" viewBox="0 0 20 20">
            <path fill-rule="evenodd" d="M5 9V7a5 5 0 0110 0v2a2 2 0 012 2v5a2 2 0 01-2 2H5a2 2 0 01-2-2v-5a2 2 0 012-2zm8-2v2H7V7a3 3 0 016 0z" clip-rule="evenodd"/>
          </svg>
          <svg v-else class="w-4 h-4 text-yellow-500" fill="currentColor" viewBox="0 0 20 20">
            <path d="M10 2a5 5 0 00-5 5v2a2 2 0 00-2 2v5a2 2 0 002 2h10a2 2 0 002-2v-5a2 2 0 00-2-2H7V7a3 3 0 015.905-.75 1 1 0 001.937-.5A5.002 5.002 0 0010 2z"/>
          </svg>
          <span>{{ connectionStatus }}</span>
        </div>
        <span class="text-dark-500">|</span>
        <span class="text-dark-400">{{ currentTime }}</span>
      </div>
    </header>

    <!-- Main Content -->
    <main class="flex-1 flex items-center justify-center px-4 py-8">
      <div class="w-full max-w-md">
        <!-- Login Card -->
        <div class="login-card rounded-2xl overflow-hidden">
          <!-- Card Header -->
          <div class="bg-gradient-to-r from-primary-600 to-primary-700 px-8 py-6">
            <div class="flex items-center space-x-4">
              <div class="w-14 h-14 bg-white/10 rounded-xl flex items-center justify-center backdrop-blur-sm">
                <svg class="w-8 h-8 text-white" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 2L4 7v10l8 5 8-5V7l-8-5zm0 2.18l5.7 3.57v7.5L12 18.82l-5.7-3.57v-7.5L12 4.18z"/>
                  <path d="M12 8a4 4 0 100 8 4 4 0 000-8zm0 2a2 2 0 110 4 2 2 0 010-4z"/>
                </svg>
              </div>
              <div>
                <h1 class="text-2xl font-bold text-white">NVM Hypervisor</h1>
                <p class="text-primary-200 text-sm mt-0.5">Enterprise Virtualization Platform</p>
              </div>
            </div>
          </div>

          <!-- Card Body -->
          <div class="bg-dark-800 px-8 py-8">
            <!-- Error Alert -->
            <div v-if="loginError" class="mb-6 p-4 bg-red-900/30 border border-red-700 rounded-lg flex items-start space-x-3">
              <svg class="w-5 h-5 text-red-400 flex-shrink-0 mt-0.5" fill="currentColor" viewBox="0 0 20 20">
                <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd"/>
              </svg>
              <div>
                <p class="text-red-400 text-sm font-medium">Authentication Error</p>
                <p class="text-red-300/80 text-sm mt-0.5">{{ loginError }}</p>
              </div>
            </div>

            <form @submit.prevent="handleLogin" class="space-y-5" @keydown="handleKeydown">
              <!-- Username Field -->
              <div>
                <label for="username" class="block text-sm font-medium text-dark-300 mb-2">
                  Username
                </label>
                <div class="relative">
                  <input
                    id="username"
                    v-model="username"
                    type="text"
                    autocomplete="username"
                    class="input w-full pl-11"
                    placeholder="Enter username"
                    :disabled="loading"
                    autofocus
                  />
                  <svg class="absolute left-4 top-1/2 -translate-y-1/2 w-5 h-5 text-dark-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
                  </svg>
                </div>
              </div>

              <!-- Password Field -->
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
                    class="input w-full pl-11 pr-11"
                    placeholder="Enter password"
                    :disabled="loading"
                  />
                  <svg class="absolute left-4 top-1/2 -translate-y-1/2 w-5 h-5 text-dark-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"/>
                  </svg>
                  <button
                    type="button"
                    class="absolute right-4 top-1/2 -translate-y-1/2 text-dark-400 hover:text-white transition-colors"
                    @click="showPassword = !showPassword"
                    tabindex="-1"
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

              <!-- Options Row -->
              <div class="flex items-center justify-between">
                <label class="flex items-center cursor-pointer group">
                  <input 
                    v-model="rememberMe" 
                    type="checkbox" 
                    class="w-4 h-4 rounded border-dark-600 bg-dark-700 text-primary-500 focus:ring-primary-500 focus:ring-offset-dark-800 cursor-pointer"
                  >
                  <span class="ml-2 text-sm text-dark-400 group-hover:text-dark-300 transition-colors">Remember username</span>
                </label>
              </div>

              <!-- Login Button -->
              <button
                type="submit"
                class="w-full h-12 bg-gradient-to-r from-primary-600 to-primary-700 hover:from-primary-500 hover:to-primary-600 text-white font-medium rounded-lg transition-all duration-200 flex items-center justify-center space-x-2 shadow-lg shadow-primary-900/30 disabled:opacity-50 disabled:cursor-not-allowed"
                :disabled="loading"
              >
                <svg v-if="loading" class="animate-spin h-5 w-5 text-white" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
                </svg>
                <svg v-else class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 16l-4-4m0 0l4-4m-4 4h14m-5 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h7a3 3 0 013 3v1"/>
                </svg>
                <span>{{ loading ? 'Authenticating...' : 'Sign In' }}</span>
              </button>
            </form>

            <!-- Server Info -->
            <div class="mt-6 pt-6 border-t border-dark-700">
              <div class="grid grid-cols-2 gap-4 text-xs">
                <div class="text-dark-500">
                  <span class="block text-dark-400 mb-1">Server</span>
                  {{ systemInfo.hostname }}
                </div>
                <div class="text-dark-500 text-right">
                  <span class="block text-dark-400 mb-1">Version</span>
                  {{ systemInfo.version }} ({{ systemInfo.build }})
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Help Links -->
        <div class="mt-6 text-center text-sm">
          <a href="https://docs.nexaos.dev/nvm" target="_blank" class="text-dark-400 hover:text-primary-400 transition-colors">
            Documentation
          </a>
          <span class="mx-3 text-dark-600">•</span>
          <a href="https://github.com/nexa-sys/nexa-os/issues" target="_blank" class="text-dark-400 hover:text-primary-400 transition-colors">
            Support
          </a>
        </div>
      </div>
    </main>

    <!-- Footer -->
    <footer class="h-10 bg-dark-950/80 backdrop-blur-sm border-t border-dark-700 flex items-center justify-between px-6 text-xs text-dark-500">
      <span>© 2025 NexaOS Project. All rights reserved.</span>
      <span>NVM Enterprise v{{ systemInfo.version }}</span>
    </footer>
  </div>
</template>

<style scoped>
.login-background {
  background: linear-gradient(135deg, #0c1222 0%, #141e30 50%, #0c1222 100%);
  position: relative;
}

.login-background::before {
  content: '';
  position: absolute;
  inset: 0;
  background: 
    radial-gradient(ellipse at 20% 30%, rgba(59, 130, 246, 0.08) 0%, transparent 50%),
    radial-gradient(ellipse at 80% 70%, rgba(59, 130, 246, 0.05) 0%, transparent 50%);
  pointer-events: none;
}

.login-card {
  box-shadow: 
    0 0 0 1px rgba(255, 255, 255, 0.05),
    0 25px 50px -12px rgba(0, 0, 0, 0.6),
    0 0 80px -20px rgba(59, 130, 246, 0.15);
}
</style>
