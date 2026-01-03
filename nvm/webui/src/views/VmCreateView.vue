<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useVmsStore } from '@/stores/vms'
import { useNotificationStore } from '@/stores/notification'
import type { VmCreateParams } from '@/stores/vms'

const router = useRouter()
const vmsStore = useVmsStore()
const notificationStore = useNotificationStore()

// Form steps
const currentStep = ref(1)
const totalSteps = 4

// Form data
const form = ref<VmCreateParams>({
  name: '',
  description: '',
  template: '',
  config: {
    cpu_cores: 2,
    memory_mb: 2048,
    disk_gb: 20,
    network: 'vmbr0',
    boot_order: ['disk', 'cdrom', 'network'],
  },
  tags: [],
})

// Templates
const templates = ref([
  { id: '', name: 'Blank VM', description: 'Start from scratch' },
  { id: 'ubuntu-22.04', name: 'Ubuntu 22.04 LTS', description: 'Popular Linux distribution' },
  { id: 'debian-12', name: 'Debian 12', description: 'Stable Linux distribution' },
  { id: 'centos-stream-9', name: 'CentOS Stream 9', description: 'Enterprise Linux' },
  { id: 'windows-server-2022', name: 'Windows Server 2022', description: 'Microsoft Server OS' },
])

// Networks
const networks = ref([
  { id: 'vmbr0', name: 'vmbr0 (Bridge)', description: '192.168.1.0/24' },
  { id: 'vmbr1', name: 'vmbr1 (Internal)', description: '10.0.0.0/24' },
])

// Validation
const isStepValid = computed(() => {
  switch (currentStep.value) {
    case 1:
      return form.value.name.length >= 3
    case 2:
      return true // Template selection is optional
    case 3:
      return form.value.config.cpu_cores >= 1 && 
             form.value.config.memory_mb >= 256 &&
             form.value.config.disk_gb >= 1
    case 4:
      return true // Review step
    default:
      return false
  }
})

function nextStep() {
  if (currentStep.value < totalSteps && isStepValid.value) {
    currentStep.value++
  }
}

function prevStep() {
  if (currentStep.value > 1) {
    currentStep.value--
  }
}

async function createVm() {
  const vm = await vmsStore.createVm(form.value)
  if (vm) {
    notificationStore.success('VM Created', `"${vm.name}" has been created successfully`)
    router.push(`/vms/${vm.id}`)
  } else {
    notificationStore.error('Creation Failed', vmsStore.error || 'Failed to create VM')
  }
}

// Tag input
const tagInput = ref('')

function addTag() {
  const tag = tagInput.value.trim()
  if (tag && form.value.tags && !form.value.tags.includes(tag)) {
    form.value.tags.push(tag)
    tagInput.value = ''
  }
}

function removeTag(tag: string) {
  if (form.value.tags) {
    form.value.tags = form.value.tags.filter(t => t !== tag)
  }
}
</script>

<template>
  <div class="p-6 max-w-4xl mx-auto">
    <!-- Header -->
    <div class="mb-8">
      <button
        class="flex items-center text-dark-400 hover:text-white mb-4"
        @click="router.back()"
      >
        <svg class="w-5 h-5 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18"/>
        </svg>
        Back to VMs
      </button>
      <h1 class="text-2xl font-bold text-white">Create Virtual Machine</h1>
      <p class="text-dark-400 mt-1">Configure and deploy a new virtual machine</p>
    </div>

    <!-- Progress Steps -->
    <div class="mb-8">
      <div class="flex items-center justify-between">
        <div
          v-for="step in totalSteps"
          :key="step"
          class="flex items-center"
          :class="{ 'flex-1': step < totalSteps }"
        >
          <div
            :class="[
              'w-10 h-10 rounded-full flex items-center justify-center font-medium text-sm',
              currentStep >= step
                ? 'bg-accent-500 text-white'
                : 'bg-dark-700 text-dark-400'
            ]"
          >
            {{ step }}
          </div>
          <div
            v-if="step < totalSteps"
            :class="[
              'flex-1 h-1 mx-4',
              currentStep > step ? 'bg-accent-500' : 'bg-dark-700'
            ]"
          />
        </div>
      </div>
      <div class="flex justify-between mt-2">
        <span class="text-xs text-dark-400">Basic Info</span>
        <span class="text-xs text-dark-400">Template</span>
        <span class="text-xs text-dark-400">Resources</span>
        <span class="text-xs text-dark-400">Review</span>
      </div>
    </div>

    <!-- Step Content -->
    <div class="card p-6">
      <!-- Step 1: Basic Info -->
      <div v-if="currentStep === 1" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Basic Information</h2>
        
        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">
            VM Name <span class="text-red-400">*</span>
          </label>
          <input
            v-model="form.name"
            type="text"
            class="input w-full"
            placeholder="e.g., web-server-01"
          />
          <p class="text-xs text-dark-500 mt-1">Must be at least 3 characters</p>
        </div>

        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">
            Description
          </label>
          <textarea
            v-model="form.description"
            class="input w-full h-24 resize-none"
            placeholder="Optional description for this VM"
          />
        </div>

        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">
            Tags
          </label>
          <div class="flex flex-wrap gap-2 mb-2">
            <span
              v-for="tag in form.tags"
              :key="tag"
              class="inline-flex items-center bg-dark-700 text-dark-300 px-2 py-1 rounded text-sm"
            >
              {{ tag }}
              <button class="ml-1 text-dark-500 hover:text-white" @click="removeTag(tag)">×</button>
            </span>
          </div>
          <div class="flex space-x-2">
            <input
              v-model="tagInput"
              type="text"
              class="input flex-1"
              placeholder="Add a tag"
              @keyup.enter="addTag"
            />
            <button class="btn-secondary" @click="addTag">Add</button>
          </div>
        </div>
      </div>

      <!-- Step 2: Template -->
      <div v-else-if="currentStep === 2" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Select Template</h2>
        
        <div class="grid grid-cols-2 gap-4">
          <button
            v-for="template in templates"
            :key="template.id"
            :class="[
              'p-4 rounded-lg border-2 text-left transition-colors',
              form.template === template.id
                ? 'border-accent-500 bg-accent-500/10'
                : 'border-dark-600 hover:border-dark-500'
            ]"
            @click="form.template = template.id"
          >
            <h3 class="text-white font-medium">{{ template.name }}</h3>
            <p class="text-sm text-dark-400 mt-1">{{ template.description }}</p>
          </button>
        </div>
      </div>

      <!-- Step 3: Resources -->
      <div v-else-if="currentStep === 3" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Configure Resources</h2>

        <div class="grid grid-cols-2 gap-6">
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">
              CPU Cores
            </label>
            <div class="flex items-center space-x-4">
              <input
                v-model.number="form.config.cpu_cores"
                type="range"
                min="1"
                max="32"
                class="flex-1"
              />
              <span class="text-white font-medium w-12 text-right">{{ form.config.cpu_cores }}</span>
            </div>
          </div>

          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">
              Memory (MB)
            </label>
            <div class="flex items-center space-x-4">
              <input
                v-model.number="form.config.memory_mb"
                type="range"
                min="256"
                max="131072"
                step="256"
                class="flex-1"
              />
              <span class="text-white font-medium w-20 text-right">{{ form.config.memory_mb }} MB</span>
            </div>
          </div>

          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">
              Disk Size (GB)
            </label>
            <div class="flex items-center space-x-4">
              <input
                v-model.number="form.config.disk_gb"
                type="range"
                min="1"
                max="2048"
                class="flex-1"
              />
              <span class="text-white font-medium w-16 text-right">{{ form.config.disk_gb }} GB</span>
            </div>
          </div>

          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">
              Network
            </label>
            <select v-model="form.config.network" class="input w-full">
              <option v-for="net in networks" :key="net.id" :value="net.id">
                {{ net.name }} - {{ net.description }}
              </option>
            </select>
          </div>
        </div>
      </div>

      <!-- Step 4: Review -->
      <div v-else-if="currentStep === 4" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Review Configuration</h2>

        <div class="bg-dark-700/50 rounded-lg p-4 space-y-4">
          <div class="flex justify-between">
            <span class="text-dark-400">Name</span>
            <span class="text-white">{{ form.name }}</span>
          </div>
          <div v-if="form.description" class="flex justify-between">
            <span class="text-dark-400">Description</span>
            <span class="text-white">{{ form.description }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-dark-400">Template</span>
            <span class="text-white">{{ templates.find(t => t.id === form.template)?.name || 'Blank VM' }}</span>
          </div>
          <hr class="border-dark-600">
          <div class="flex justify-between">
            <span class="text-dark-400">CPU</span>
            <span class="text-white">{{ form.config.cpu_cores }} vCPUs</span>
          </div>
          <div class="flex justify-between">
            <span class="text-dark-400">Memory</span>
            <span class="text-white">{{ form.config.memory_mb }} MB</span>
          </div>
          <div class="flex justify-between">
            <span class="text-dark-400">Disk</span>
            <span class="text-white">{{ form.config.disk_gb }} GB</span>
          </div>
          <div class="flex justify-between">
            <span class="text-dark-400">Network</span>
            <span class="text-white">{{ form.config.network }}</span>
          </div>
          <div v-if="form.tags && form.tags.length > 0" class="flex justify-between">
            <span class="text-dark-400">Tags</span>
            <span class="text-white">{{ form.tags.join(', ') }}</span>
          </div>
        </div>
      </div>

      <!-- Navigation -->
      <div class="flex justify-between mt-8 pt-6 border-t border-dark-600">
        <button
          v-if="currentStep > 1"
          class="btn-secondary"
          @click="prevStep"
        >
          Previous
        </button>
        <div v-else />
        
        <button
          v-if="currentStep < totalSteps"
          class="btn-primary"
          :disabled="!isStepValid"
          @click="nextStep"
        >
          Next
        </button>
        <button
          v-else
          class="btn-primary"
          :disabled="vmsStore.loading"
          @click="createVm"
        >
          <svg v-if="vmsStore.loading" class="animate-spin -ml-1 mr-2 h-4 w-4 text-white" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
          </svg>
          {{ vmsStore.loading ? 'Creating...' : 'Create VM' }}
        </button>
      </div>
    </div>
  </div>
</template>
