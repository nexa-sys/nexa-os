<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useVmsStore } from '@/stores/vms'

const route = useRoute()
const router = useRouter()
const vmsStore = useVmsStore()

const vmId = route.params.id as string
const connected = ref(false)
const loading = ref(true)
const error = ref<string | null>(null)
const consoleMessages = ref<string[]>([])

// Console display
const canvasRef = ref<HTMLCanvasElement | null>(null)
let ws: WebSocket | null = null
let reconnectAttempts = 0
const maxReconnectAttempts = 3
let canvasCtx: CanvasRenderingContext2D | null = null
let displayWidth = 800
let displayHeight = 600

onMounted(async () => {
  // Fetch VM info
  await vmsStore.fetchVm(vmId)
  
  if (!vmsStore.selectedVm) {
    error.value = 'VM not found'
    loading.value = false
    return
  }

  if (vmsStore.selectedVm.status !== 'running') {
    error.value = 'VM must be running to access console'
    loading.value = false
    return
  }

  connectWebSocket()
})

function initCanvas(width: number, height: number) {
  displayWidth = width
  displayHeight = height
  
  nextTick(() => {
    if (canvasRef.value) {
      canvasRef.value.width = width
      canvasRef.value.height = height
      canvasCtx = canvasRef.value.getContext('2d')
      
      // Fill with black initially
      if (canvasCtx) {
        canvasCtx.fillStyle = '#000000'
        canvasCtx.fillRect(0, 0, width, height)
      }
    }
  })
}

function renderFramebuffer(data: ArrayBuffer) {
  if (!canvasCtx || !canvasRef.value) return
  
  const view = new DataView(data)
  
  // Parse header: [type(1), width(2), height(2), reserved(3), data...]
  const frameType = view.getUint8(0)
  if (frameType !== 0x01) {
    console.warn('Unknown frame type:', frameType)
    return
  }
  
  const width = view.getUint16(1, true)  // Little endian
  const height = view.getUint16(3, true)
  
  // Update canvas size if needed
  if (width !== displayWidth || height !== displayHeight) {
    initCanvas(width, height)
    return // Canvas will be ready on next frame
  }
  
  // Get pixel data (RGBA format, 4 bytes per pixel)
  const pixelData = new Uint8ClampedArray(data, 8)
  
  // Create ImageData and draw to canvas
  const imageData = new ImageData(pixelData, width, height)
  canvasCtx.putImageData(imageData, 0, 0)
}

function connectWebSocket() {
  // Connect to framebuffer console via WebSocket
  try {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/api/v2/vms/${vmId}/console/ws`
    ws = new WebSocket(wsUrl)
    ws.binaryType = 'arraybuffer'
    
    ws.onopen = () => {
      connected.value = true
      loading.value = false
      reconnectAttempts = 0
      consoleMessages.value.push('Connected to VM console')
    }
    
    ws.onerror = (e) => {
      console.error('WebSocket error:', e)
      if (!connected.value && reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++
        consoleMessages.value.push(`Connection failed, retrying (${reconnectAttempts}/${maxReconnectAttempts})...`)
        setTimeout(connectWebSocket, 2000)
      } else if (!connected.value) {
        error.value = 'Failed to connect to console. Please check that the VM is running and try again.'
        loading.value = false
      }
    }
    
    ws.onclose = (e) => {
      connected.value = false
      if (e.code === 4001) {
        error.value = 'VM is not running'
      } else if (e.code === 4004) {
        error.value = 'VM not found'
      } else if (!error.value) {
        consoleMessages.value.push('Connection closed')
      }
    }
    
    ws.onmessage = (event) => {
      // Handle binary framebuffer data
      if (event.data instanceof ArrayBuffer) {
        renderFramebuffer(event.data)
        return
      }
      
      // Handle JSON messages
      try {
        const data = JSON.parse(event.data)
        if (data.type === 'connected') {
          consoleMessages.value.push(`Console ready: ${data.message}`)
          // Initialize canvas with server-provided dimensions
          if (data.width && data.height) {
            initCanvas(data.width, data.height)
          }
        } else if (data.type === 'error') {
          consoleMessages.value.push(`Error: ${data.message}`)
        } else if (data.type === 'pong') {
          // Keep-alive response
        }
      } catch {
        console.warn('Unexpected message format:', event.data)
      }
    }
  } catch (e) {
    error.value = 'Failed to initialize console connection'
    loading.value = false
  }
}

onUnmounted(() => {
  if (ws) {
    ws.close()
  }
})

function sendCtrlAltDel() {
  if (ws && connected.value) {
    // Send Ctrl+Alt+Del key sequence
    ws.send(JSON.stringify({ type: 'key', keys: ['ctrl', 'alt', 'delete'] }))
  }
}

function requestFrame() {
  if (ws && connected.value) {
    ws.send(JSON.stringify({ type: 'request_frame' }))
  }
}

function toggleFullscreen() {
  const container = document.getElementById('console-container')
  if (container) {
    if (document.fullscreenElement) {
      document.exitFullscreen()
    } else {
      container.requestFullscreen()
    }
  }
}

// Handle keyboard events
function handleKeyDown(e: KeyboardEvent) {
  if (!ws || !connected.value) return
  
  ws.send(JSON.stringify({
    type: 'key',
    action: 'down',
    code: e.code,
    key: e.key,
    ctrlKey: e.ctrlKey,
    altKey: e.altKey,
    shiftKey: e.shiftKey
  }))
  
  // Prevent default for most keys when console is focused
  if (e.key !== 'F11' && e.key !== 'F12') {
    e.preventDefault()
  }
}

function handleKeyUp(e: KeyboardEvent) {
  if (!ws || !connected.value) return
  
  ws.send(JSON.stringify({
    type: 'key',
    action: 'up',
    code: e.code,
    key: e.key
  }))
}
</script>

<template>
  <div class="h-full flex flex-col bg-black">
    <!-- Toolbar -->
    <div class="h-12 bg-dark-800 border-b border-dark-600 flex items-center justify-between px-4">
      <div class="flex items-center space-x-4">
        <button
          class="text-dark-400 hover:text-white"
          @click="router.back()"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18"/>
          </svg>
        </button>
        <div>
          <h1 class="text-white font-medium">{{ vmsStore.selectedVm?.name || 'Console' }}</h1>
          <div class="flex items-center space-x-2 text-xs">
            <span :class="['w-2 h-2 rounded-full', connected ? 'bg-green-500' : 'bg-red-500']" />
            <span class="text-dark-400">{{ connected ? 'Connected' : 'Disconnected' }}</span>
          </div>
        </div>
      </div>
      
      <div class="flex items-center space-x-2">
        <button
          class="btn-secondary text-sm"
          :disabled="!connected"
          @click="sendCtrlAltDel"
        >
          Ctrl+Alt+Del
        </button>
        <button
          class="p-2 text-dark-400 hover:text-white hover:bg-dark-700 rounded-lg"
          @click="toggleFullscreen"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4"/>
          </svg>
        </button>
      </div>
    </div>

    <!-- Console Area -->
    <div id="console-container" class="flex-1 flex items-center justify-center bg-black">
      <!-- Loading -->
      <div v-if="loading" class="text-center">
        <svg class="animate-spin w-12 h-12 mx-auto text-accent-500" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
        </svg>
        <p class="text-dark-400 mt-4">Connecting to console...</p>
      </div>

      <!-- Error -->
      <div v-else-if="error" class="text-center">
        <div class="w-16 h-16 mx-auto bg-red-500/10 rounded-full flex items-center justify-center mb-4">
          <svg class="w-8 h-8 text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
          </svg>
        </div>
        <h3 class="text-lg font-medium text-white">Connection Error</h3>
        <p class="text-dark-400 mt-2">{{ error }}</p>
        <button class="btn-primary mt-4" @click="router.back()">
          Go Back
        </button>
      </div>

      <!-- Framebuffer Canvas -->
      <canvas
        v-else
        ref="canvasRef"
        class="max-w-full max-h-full border border-dark-600"
        tabindex="0"
        @keydown="handleKeyDown"
        @keyup="handleKeyUp"
      />
    </div>
  </div>
</template>
