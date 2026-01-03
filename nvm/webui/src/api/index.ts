import axios from 'axios'
import type { AxiosInstance, AxiosResponse, InternalAxiosRequestConfig, AxiosError } from 'axios'

// API base URL - in production this will be the same origin
const API_BASE = import.meta.env.VITE_API_URL || '/api/v2'

// Retry configuration
const MAX_RETRIES = 3
const RETRY_DELAY = 1000 // ms
const RETRYABLE_STATUS_CODES = [408, 429, 500, 502, 503, 504]

// Create axios instance with enterprise configuration
export const api: AxiosInstance = axios.create({
  baseURL: API_BASE,
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
    'Accept': 'application/json',
  },
  // Validate status - don't reject on any HTTP status
  validateStatus: (status) => status < 500,
})

// Request interceptor - add auth token and CSRF token
api.interceptors.request.use(
  (config: InternalAxiosRequestConfig) => {
    const token = localStorage.getItem('nvm_token')
    const csrfToken = localStorage.getItem('nvm_csrf')
    
    if (token && config.headers) {
      config.headers.Authorization = `Bearer ${token}`
    }
    
    // Add CSRF token for state-changing requests
    if (csrfToken && config.headers && ['POST', 'PUT', 'DELETE', 'PATCH'].includes(config.method?.toUpperCase() || '')) {
      config.headers['X-CSRF-Token'] = csrfToken
    }
    
    return config
  },
  (error) => {
    return Promise.reject(error)
  }
)

// Response interceptor - handle errors and implement retry logic
api.interceptors.response.use(
  (response: AxiosResponse) => {
    return response
  },
  async (error: AxiosError) => {
    const config = error.config as InternalAxiosRequestConfig & { _retryCount?: number }
    
    // Handle 401 Unauthorized - redirect to login
    if (error.response?.status === 401) {
      localStorage.removeItem('nvm_token')
      localStorage.removeItem('nvm_csrf')
      
      // Redirect to login if not already there
      if (window.location.pathname !== '/login') {
        window.location.href = '/login'
      }
      return Promise.reject(error)
    }
    
    // Handle 403 Forbidden - permission denied
    if (error.response?.status === 403) {
      console.error('Permission denied:', error.response.data)
      return Promise.reject(error)
    }
    
    // Implement retry logic for certain errors
    const statusCode = error.response?.status || 0
    const isRetryable = RETRYABLE_STATUS_CODES.includes(statusCode) || error.code === 'ECONNABORTED'
    
    if (isRetryable && config) {
      config._retryCount = config._retryCount || 0
      
      if (config._retryCount < MAX_RETRIES) {
        config._retryCount++
        
        // Exponential backoff
        const delay = RETRY_DELAY * Math.pow(2, config._retryCount - 1)
        
        console.warn(`Request failed (${statusCode}), retrying in ${delay}ms... (attempt ${config._retryCount}/${MAX_RETRIES})`)
        
        await new Promise(resolve => setTimeout(resolve, delay))
        return api.request(config)
      }
    }
    
    return Promise.reject(error)
  }
)

// API response wrapper types
export interface ApiResponse<T = unknown> {
  success: boolean
  data?: T
  error?: {
    code: number | string
    message: string
    details?: unknown
  }
  meta?: {
    total?: number
    page?: number
    per_page?: number
  }
}

export interface PaginationParams {
  page?: number
  per_page?: number
  sort_by?: string
  sort_order?: 'asc' | 'desc'
}

// Type-safe API methods
export async function get<T>(url: string, params?: Record<string, unknown>): Promise<ApiResponse<T>> {
  const response = await api.get<ApiResponse<T>>(url, { params })
  return response.data
}

export async function post<T>(url: string, data?: unknown): Promise<ApiResponse<T>> {
  const response = await api.post<ApiResponse<T>>(url, data)
  return response.data
}

export async function put<T>(url: string, data?: unknown): Promise<ApiResponse<T>> {
  const response = await api.put<ApiResponse<T>>(url, data)
  return response.data
}

export async function patch<T>(url: string, data?: unknown): Promise<ApiResponse<T>> {
  const response = await api.patch<ApiResponse<T>>(url, data)
  return response.data
}

export async function del<T>(url: string, params?: Record<string, unknown>): Promise<ApiResponse<T>> {
  const response = await api.delete<ApiResponse<T>>(url, { params })
  return response.data
}

// File upload helper with progress tracking
export async function uploadFile(
  url: string, 
  file: File, 
  onProgress?: (percent: number) => void
): Promise<ApiResponse<unknown>> {
  const formData = new FormData()
  formData.append('file', file)
  
  const response = await api.post<ApiResponse<unknown>>(url, formData, {
    headers: {
      'Content-Type': 'multipart/form-data',
    },
    onUploadProgress: (progressEvent) => {
      if (onProgress && progressEvent.total) {
        const percent = Math.round((progressEvent.loaded * 100) / progressEvent.total)
        onProgress(percent)
      }
    },
  })
  
  return response.data
}

// Health check utility
export async function checkHealth(): Promise<boolean> {
  try {
    const response = await api.get('/health', { timeout: 5000 })
    return response.status === 200
  } catch {
    return false
  }
}

// Connection status utility
export interface ConnectionStatus {
  connected: boolean
  latency?: number
  lastCheck: Date
}

export async function getConnectionStatus(): Promise<ConnectionStatus> {
  const startTime = Date.now()
  try {
    await api.get('/health', { timeout: 5000 })
    return {
      connected: true,
      latency: Date.now() - startTime,
      lastCheck: new Date(),
    }
  } catch {
    return {
      connected: false,
      lastCheck: new Date(),
    }
  }
}
