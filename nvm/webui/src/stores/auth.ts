import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api } from '@/api'

export interface User {
  id: string
  username: string
  email?: string
  roles: string[]
  permissions: string[]
}

/** API Login Response format from backend */
interface LoginApiResponse {
  success: boolean
  token?: string
  csrf_token?: string
  user?: User
  error?: string
}

/** API Response wrapper for /auth/me */
interface MeApiResponse {
  success: boolean
  data?: User
  error?: { code: number; message: string }
}

export const useAuthStore = defineStore('auth', () => {
  const user = ref<User | null>(null)
  const token = ref<string | null>(localStorage.getItem('nvm_token'))
  const csrfToken = ref<string | null>(localStorage.getItem('nvm_csrf'))
  const loading = ref(false)
  const error = ref<string | null>(null)

  const isAuthenticated = computed(() => !!token.value)
  const isAdmin = computed(() => user.value?.roles.includes('admin') ?? false)

  async function login(username: string, password: string): Promise<boolean> {
    loading.value = true
    error.value = null
    
    try {
      const response = await api.post<LoginApiResponse>('/auth/login', { username, password })
      const data = response.data
      
      // Backend returns: { success, token, csrf_token, user, error }
      if (data.success && data.token && data.user) {
        token.value = data.token
        csrfToken.value = data.csrf_token || null
        user.value = data.user
        
        localStorage.setItem('nvm_token', data.token)
        if (data.csrf_token) {
          localStorage.setItem('nvm_csrf', data.csrf_token)
        }
        return true
      } else {
        error.value = data.error || 'Login failed'
        return false
      }
    } catch (e: any) {
      // Handle network errors or non-2xx responses
      if (e.response?.data) {
        const respData = e.response.data as LoginApiResponse
        error.value = respData.error || 'Invalid credentials'
      } else if (e.message) {
        error.value = e.message
      } else {
        error.value = 'Network error. Please check your connection.'
      }
      return false
    } finally {
      loading.value = false
    }
  }

  async function logout(): Promise<void> {
    try {
      await api.post('/auth/logout')
    } catch {
      // Ignore logout errors - we still clear local state
    }
    
    token.value = null
    csrfToken.value = null
    user.value = null
    localStorage.removeItem('nvm_token')
    localStorage.removeItem('nvm_csrf')
  }

  async function fetchUser(): Promise<boolean> {
    if (!token.value) return false
    
    try {
      const response = await api.get<MeApiResponse>('/auth/me')
      const data = response.data
      
      if (data.success && data.data) {
        user.value = data.data
        return true
      } else {
        // Token might be invalid
        await logout()
        return false
      }
    } catch {
      // Token invalid or expired
      await logout()
      return false
    }
  }

  async function refreshToken(): Promise<boolean> {
    if (!token.value) return false
    
    try {
      const response = await api.post<LoginApiResponse>('/auth/refresh')
      const data = response.data
      
      if (data.success && data.token) {
        token.value = data.token
        csrfToken.value = data.csrf_token || null
        if (data.user) user.value = data.user
        
        localStorage.setItem('nvm_token', data.token)
        if (data.csrf_token) {
          localStorage.setItem('nvm_csrf', data.csrf_token)
        }
        return true
      }
      return false
    } catch {
      return false
    }
  }

  function hasPermission(permission: string): boolean {
    if (!user.value) return false
    return user.value.permissions.includes(permission) || 
           user.value.permissions.includes('*')
  }

  function hasRole(role: string): boolean {
    return user.value?.roles.includes(role) ?? false
  }

  // Auto-restore session on init
  if (token.value) {
    fetchUser()
  }

  return {
    // State
    user,
    token,
    csrfToken,
    loading,
    error,
    // Computed
    isAuthenticated,
    isAdmin,
    // Actions
    login,
    logout,
    fetchUser,
    refreshToken,
    hasPermission,
    hasRole,
  }
})
