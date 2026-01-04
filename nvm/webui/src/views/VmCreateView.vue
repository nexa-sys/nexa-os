<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useVmsStore } from '@/stores/vms'
import { useNotificationStore } from '@/stores/notification'
import { api } from '@/api'
import { vmsApi } from '@/api/vms'

const router = useRouter()
const route = useRoute()
const vmsStore = useVmsStore()
const notificationStore = useNotificationStore()

// Edit mode detection
const isEditMode = computed(() => route.name === 'vm-edit' && !!route.params.id)
const vmId = computed(() => route.params.id as string | undefined)
const loadingVm = ref(false)

// Form steps - expanded for enterprise configuration
const currentStep = ref(1)
const totalSteps = 6
const loadingTemplates = ref(true)
const loadingNetworks = ref(true)
const loadingStoragePools = ref(true)
const loadingIsos = ref(true)

// Disk configuration interface
interface DiskConfig {
  id: string
  size_gb: number
  storage_pool: string
  format: string
  bus: string
  cache: string
  bootable: boolean
  ssd_emulation: boolean
  iops_limit?: number
}

// Network interface configuration
interface NetworkConfig {
  id: string
  network: string
  mac: string
  model: string
  vlan_id?: number
  inbound_limit_mbps?: number
  outbound_limit_mbps?: number
}

// Form data with enterprise configuration
const form = ref({
  // Basic info
  name: '',
  description: '',
  template: '',
  tags: [] as string[],
  
  // CPU configuration
  cpu: {
    sockets: 1,
    cores_per_socket: 2,
    threads_per_core: 1,
    model: 'host-passthrough',
    hot_add: false,
    nested_virt: false,
    reservation_mhz: undefined as number | undefined,
    limit_mhz: undefined as number | undefined,
    shares: 'normal',
  },
  
  // Memory configuration
  memory: {
    size_mb: 2048,
    max_size_mb: undefined as number | undefined,
    hot_add: false,
    reservation_mb: undefined as number | undefined,
    ballooning: true,
    ksm: false,
    huge_pages: false,
  },
  
  // Storage configuration
  disks: [
    {
      id: crypto.randomUUID(),
      size_gb: 20,
      storage_pool: 'local',
      format: 'qcow2',
      bus: 'virtio',
      cache: 'writeback',
      bootable: true,
      ssd_emulation: false,
    }
  ] as DiskConfig[],
  
  // Network configuration
  networks: [
    {
      id: crypto.randomUUID(),
      network: 'default',
      mac: '',
      model: 'virtio',
      vlan_id: undefined,
      inbound_limit_mbps: undefined,
      outbound_limit_mbps: undefined,
    }
  ] as NetworkConfig[],
  
  // CD/DVD
  cdrom: {
    enabled: false,
    iso: '',
    bus: 'sata',
  },
  
  // Boot configuration
  boot: {
    firmware: 'uefi',
    secure_boot: false,
    order: ['disk', 'cdrom', 'network'],
    machine_type: 'q35',
    menu_timeout: 0,
    backend: 'jit',  // Execution backend: jit (software, 5-15% faster), vmx (Intel), svm (AMD)
  },
  
  // Security configuration
  security: {
    tpm: false,
    tpm_version: '2.0',
    sev: false,
    isolation: 'hypervisor',
  },
  
  // Advanced options
  advanced: {
    start_after_create: false,
    host_node: undefined as string | undefined,
  },
})

// Available options from API
const templates = ref([{ id: '', name: 'Blank VM', description: 'Start from scratch' }])
const networks = ref<Array<{ id: string; name: string; description: string; cidr?: string }>>([])
const storagePools = ref<Array<{ id: string; name: string; available_gb: number; total_gb: number }>>([])
const isoImages = ref<Array<{ id: string; name: string; size_mb: number }>>([])
const clusterNodes = ref<Array<{ id: string; name: string; status: string }>>([])

// Disk format options
const diskFormats = ['qcow2', 'raw', 'vmdk', 'vdi']
const diskBusOptions = ['virtio', 'scsi', 'ide', 'sata', 'nvme']
const diskCacheOptions = ['writeback', 'writethrough', 'none', 'directsync', 'unsafe']
const nicModels = ['virtio', 'e1000', 'e1000e', 'vmxnet3', 'rtl8139']
const cpuModels = ['host-passthrough', 'host-model', 'qemu64', 'Skylake-Server', 'EPYC']
const firmwareOptions = ['uefi', 'bios']
const machineTypes = ['q35', 'i440fx', 'virt']
const backendOptions = [
  { value: 'jit', label: 'JIT (Software)', description: '5-15% faster, no hardware virtualization needed' },
  { value: 'vmx', label: 'VMX (Intel VT-x)', description: 'Intel hardware virtualization' },
  { value: 'svm', label: 'SVM (AMD-V)', description: 'AMD hardware virtualization' },
  { value: 'auto', label: 'Auto-detect', description: 'Choose best available backend' },
]

// Computed total vCPUs
const totalVcpus = computed(() => 
  form.value.cpu.sockets * form.value.cpu.cores_per_socket * form.value.cpu.threads_per_core
)

// Computed total disk size
const totalDiskGb = computed(() => 
  form.value.disks.reduce((sum, d) => sum + d.size_gb, 0)
)

// Fetch data from APIs
async function fetchTemplates() {
  try {
    loadingTemplates.value = true
    const response = await api.get('/templates')
    if (response.data.success && response.data.data) {
      const apiTemplates = response.data.data.map((tpl: any) => ({
        id: tpl.id,
        name: tpl.name || 'Unnamed',
        description: tpl.os_type || 'Custom template'
      }))
      templates.value = [{ id: '', name: 'Blank VM', description: 'Start from scratch' }, ...apiTemplates]
    }
  } catch (e) {
    console.error('Failed to fetch templates:', e)
  } finally {
    loadingTemplates.value = false
  }
}

async function fetchNetworks() {
  try {
    loadingNetworks.value = true
    const response = await api.get('/networks')
    if (response.data.success && response.data.data) {
      networks.value = response.data.data.map((net: any) => ({
        id: net.id || net.name,
        name: `${net.name} (${net.network_type || 'Bridge'})`,
        description: net.cidr || '',
        cidr: net.cidr
      }))
    }
    if (networks.value.length === 0) {
      networks.value = [{ id: 'default', name: 'default (Bridge)', description: '192.168.122.0/24' }]
    }
  } catch (e) {
    console.error('Failed to fetch networks:', e)
    networks.value = [{ id: 'default', name: 'default (Bridge)', description: '192.168.122.0/24' }]
  } finally {
    loadingNetworks.value = false
  }
}

async function fetchStoragePools() {
  try {
    loadingStoragePools.value = true
    const response = await api.get('/storage/pools')
    if (response.data.success && response.data.data) {
      storagePools.value = response.data.data.map((pool: any) => ({
        id: pool.name || pool.id,
        name: pool.name,
        available_gb: Math.floor((pool.total_bytes - pool.used_bytes) / (1024 * 1024 * 1024)),
        total_gb: Math.floor(pool.total_bytes / (1024 * 1024 * 1024))
      }))
    }
    if (storagePools.value.length === 0) {
      storagePools.value = [{ id: 'local', name: 'local', available_gb: 500, total_gb: 1000 }]
    }
  } catch (e) {
    console.error('Failed to fetch storage pools:', e)
    storagePools.value = [{ id: 'local', name: 'local', available_gb: 500, total_gb: 1000 }]
  } finally {
    loadingStoragePools.value = false
  }
}

async function fetchIsos() {
  try {
    loadingIsos.value = true
    const response = await api.get('/storage/isos')
    if (response.data.success && response.data.data) {
      isoImages.value = response.data.data.map((iso: any) => ({
        id: iso.id || iso.path,
        name: iso.name,
        size_mb: Math.floor((iso.size_bytes || 0) / (1024 * 1024))
      }))
    }
  } catch (e) {
    console.error('Failed to fetch ISOs:', e)
  } finally {
    loadingIsos.value = false
  }
}

async function fetchNodes() {
  try {
    const response = await api.get('/cluster/nodes')
    if (response.data.success && response.data.data) {
      clusterNodes.value = response.data.data.map((node: any) => ({
        id: node.id,
        name: node.name,
        status: node.status
      }))
    }
  } catch (e) {
    console.error('Failed to fetch cluster nodes:', e)
  }
}

// Load existing VM data for edit mode
async function loadVmForEdit(id: string) {
  try {
    loadingVm.value = true
    const response = await vmsApi.get(id)
    if (response.data.success && response.data.data) {
      const vm = response.data.data
      
      // Populate form with existing VM data
      form.value.name = vm.name || ''
      form.value.description = vm.description || ''
      form.value.tags = vm.tags || []
      
      // CPU configuration
      if (vm.config?.cpu) {
        const cpu = vm.config.cpu
        form.value.cpu.sockets = cpu.sockets || 1
        form.value.cpu.cores_per_socket = cpu.cores_per_socket || cpu.cores || 2
        form.value.cpu.threads_per_core = cpu.threads_per_core || 1
        form.value.cpu.model = cpu.model || 'host-passthrough'
        form.value.cpu.hot_add = cpu.hot_add || false
        form.value.cpu.nested_virt = cpu.nested_virt || false
        form.value.cpu.reservation_mhz = cpu.reservation_mhz
        form.value.cpu.limit_mhz = cpu.limit_mhz
      }
      
      // Memory configuration
      if (vm.config?.memory) {
        const mem = vm.config.memory
        form.value.memory.size_mb = mem.size_mb || 2048
        form.value.memory.max_size_mb = mem.max_size_mb
        form.value.memory.hot_add = mem.hot_add || false
        form.value.memory.reservation_mb = mem.reservation_mb
        form.value.memory.ballooning = mem.ballooning !== false
        form.value.memory.ksm = mem.ksm || false
        form.value.memory.huge_pages = mem.huge_pages || false
      }
      
      // Disks configuration
      if (vm.config?.disks && vm.config.disks.length > 0) {
        form.value.disks = vm.config.disks.map((disk: any) => ({
          id: disk.id || crypto.randomUUID(),
          size_gb: disk.size_gb || 20,
          storage_pool: disk.storage_pool || 'local',
          format: disk.format || 'qcow2',
          bus: disk.bus || 'virtio',
          cache: disk.cache || 'writeback',
          bootable: disk.bootable || false,
          ssd_emulation: disk.ssd_emulation || false,
          iops_limit: disk.iops_limit,
          // Preserve existing disk path for edit mode
          path: disk.path,
        }))
      }
      
      // Network configuration  
      if (vm.config?.networks && vm.config.networks.length > 0) {
        form.value.networks = vm.config.networks.map((net: any) => ({
          id: net.id || crypto.randomUUID(),
          network: net.network || 'default',
          mac: net.mac || '',
          model: net.model || 'virtio',
          vlan_id: net.vlan_id,
          inbound_limit_mbps: net.inbound_limit_mbps,
          outbound_limit_mbps: net.outbound_limit_mbps,
        }))
      }
      
      // CD/DVD configuration
      if (vm.config?.cdrom) {
        form.value.cdrom.enabled = vm.config.cdrom.enabled || false
        form.value.cdrom.iso = vm.config.cdrom.iso || ''
        form.value.cdrom.bus = vm.config.cdrom.bus || 'sata'
      }
      
      // Boot configuration
      if (vm.config?.boot) {
        const boot = vm.config.boot
        form.value.boot.firmware = boot.firmware || 'uefi'
        form.value.boot.secure_boot = boot.secure_boot || false
        form.value.boot.order = boot.order || ['disk', 'cdrom', 'network']
        form.value.boot.machine_type = boot.machine_type || 'q35'
        form.value.boot.menu_timeout = boot.menu_timeout || 0
      }
      
      // Security configuration
      if (vm.config?.security) {
        const sec = vm.config.security
        form.value.security.tpm = sec.tpm || false
        form.value.security.tpm_version = sec.tpm_version || '2.0'
        form.value.security.sev = sec.sev || false
        form.value.security.isolation = sec.isolation || 'hypervisor'
      }
      
      // Advanced options
      form.value.advanced.host_node = vm.host_node
      
      notificationStore.success('VM Loaded', `Editing "${vm.name}"`)
    } else {
      notificationStore.error('Load Failed', 'Failed to load VM configuration')
      router.push('/vms')
    }
  } catch (e: any) {
    console.error('Failed to load VM:', e)
    notificationStore.error('Load Failed', e.response?.data?.error?.message || 'Failed to load VM for editing')
    router.push('/vms')
  } finally {
    loadingVm.value = false
  }
}

onMounted(async () => {
  // Fetch reference data in parallel
  await Promise.all([
    fetchTemplates(),
    fetchNetworks(),
    fetchStoragePools(),
    fetchIsos(),
    fetchNodes()
  ])
  
  // If in edit mode, load existing VM data
  if (isEditMode.value && vmId.value) {
    await loadVmForEdit(vmId.value)
  }
})

// Disk management functions
function addDisk() {
  form.value.disks.push({
    id: crypto.randomUUID(),
    size_gb: 20,
    storage_pool: storagePools.value[0]?.id || 'local',
    format: 'qcow2',
    bus: 'virtio',
    cache: 'writeback',
    bootable: false,
    ssd_emulation: false,
  })
}

function removeDisk(index: number) {
  // Enterprise feature: Allow removing all disks for diskless VMs
  // (PXE boot, live ISO, thin clients, etc.)
  form.value.disks.splice(index, 1)
  
  // Ensure at least one remaining disk is bootable (if any disks remain)
  if (form.value.disks.length > 0 && !form.value.disks.some(d => d.bootable)) {
    form.value.disks[0].bootable = true
  }
}

function setBootDisk(index: number) {
  form.value.disks.forEach((d, i) => d.bootable = i === index)
}

// Network management functions
function addNetwork() {
  form.value.networks.push({
    id: crypto.randomUUID(),
    network: networks.value[0]?.id || 'default',
    mac: '',
    model: 'virtio',
    vlan_id: undefined,
    inbound_limit_mbps: undefined,
    outbound_limit_mbps: undefined,
  })
}

function removeNetwork(index: number) {
  if (form.value.networks.length > 1) {
    form.value.networks.splice(index, 1)
  }
}

function generateMac(index: number) {
  const mac = `52:54:00:${Math.floor(Math.random() * 256).toString(16).padStart(2, '0')}:${Math.floor(Math.random() * 256).toString(16).padStart(2, '0')}:${Math.floor(Math.random() * 256).toString(16).padStart(2, '0')}`
  form.value.networks[index].mac = mac
}

// Validation
// Enterprise feature: Allow diskless VMs for PXE boot, ISO boot, or network boot scenarios
const isStepValid = computed(() => {
  switch (currentStep.value) {
    case 1: // Basic Info
      return form.value.name.length >= 3
    case 2: // CPU & Memory
      return totalVcpus.value >= 1 && form.value.memory.size_mb >= 256
    case 3: // Storage
      // Allow diskless VMs - useful for:
      // - PXE network boot (stateless clients)
      // - Live ISO/CD boot
      // - Network-attached storage only
      // - Thin clients
      // If disks exist, at least one should be bootable (unless booting from CD/network)
      if (form.value.disks.length === 0) {
        // Diskless VM is valid - will boot from CD, network, or other source
        return true
      }
      // If disks exist and CD/network boot is not primary, require bootable disk
      const primaryBoot = form.value.boot.order[0]
      if (primaryBoot === 'disk') {
        return form.value.disks.some(d => d.bootable)
      }
      return true // CD or network boot is primary
    case 4: // Network
      return form.value.networks.length >= 1
    case 5: // Boot & Security
      return true
    case 6: // Review
      return true
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

// Build enterprise API request
function buildRequest() {
  return {
    name: form.value.name,
    description: form.value.description || undefined,
    template: form.value.template || undefined,
    tags: form.value.tags,
    hardware: {
      cpu: {
        sockets: form.value.cpu.sockets,
        cores_per_socket: form.value.cpu.cores_per_socket,
        threads_per_core: form.value.cpu.threads_per_core,
        model: form.value.cpu.model,
        hot_add: form.value.cpu.hot_add,
        nested_virt: form.value.cpu.nested_virt,
        reservation_mhz: form.value.cpu.reservation_mhz,
        limit_mhz: form.value.cpu.limit_mhz,
        shares: form.value.cpu.shares,
      },
      memory: {
        size_mb: form.value.memory.size_mb,
        max_size_mb: form.value.memory.max_size_mb,
        hot_add: form.value.memory.hot_add,
        reservation_mb: form.value.memory.reservation_mb,
        ballooning: form.value.memory.ballooning,
        ksm: form.value.memory.ksm,
        huge_pages: form.value.memory.huge_pages,
      },
      disks: form.value.disks.map(d => ({
        size_gb: d.size_gb,
        storage_pool: d.storage_pool,
        format: d.format,
        bus: d.bus,
        cache: d.cache,
        bootable: d.bootable,
        ssd_emulation: d.ssd_emulation,
        iops_rd_limit: d.iops_limit,
        iops_wr_limit: d.iops_limit,
      })),
      networks: form.value.networks.map(n => ({
        network: n.network,
        mac: n.mac || undefined,
        model: n.model,
        vlan_id: n.vlan_id,
        inbound_limit_mbps: n.inbound_limit_mbps,
        outbound_limit_mbps: n.outbound_limit_mbps,
      })),
      cdrom: form.value.cdrom.enabled ? {
        iso: form.value.cdrom.iso || undefined,
        bus: form.value.cdrom.bus,
      } : undefined,
    },
    boot: {
      firmware: form.value.boot.firmware,
      secure_boot: form.value.boot.secure_boot,
      order: form.value.boot.order,
      machine_type: form.value.boot.machine_type,
      menu_timeout: form.value.boot.menu_timeout,
      backend: form.value.boot.backend,
    },
    security: {
      tpm: form.value.security.tpm,
      tpm_version: form.value.security.tpm_version,
      sev: form.value.security.sev,
      isolation: form.value.security.isolation,
    },
    start_after_create: form.value.advanced.start_after_create,
    host_node: form.value.advanced.host_node,
  }
}

async function createVm() {
  vmsStore.loading = true
  try {
    const request = buildRequest()
    const response = await api.post('/vms', request)
    if (response.data.success) {
      const newVm = response.data.data
      notificationStore.success('VM Created', `"${newVm.name}" has been created successfully`)
      router.push(`/vms/${newVm.id}`)
    } else {
      notificationStore.error('Creation Failed', response.data.error?.message || 'Failed to create VM')
    }
  } catch (e: any) {
    notificationStore.error('Creation Failed', e.response?.data?.error?.message || 'Failed to create VM')
  } finally {
    vmsStore.loading = false
  }
}

async function updateVm() {
  if (!vmId.value) return
  
  vmsStore.loading = true
  try {
    const request = buildRequest()
    const response = await vmsApi.update(vmId.value, request)
    if (response.data.success) {
      notificationStore.success('VM Updated', `"${form.value.name}" has been updated successfully`)
      router.push(`/vms/${vmId.value}`)
    } else {
      notificationStore.error('Update Failed', response.data.error?.message || 'Failed to update VM')
    }
  } catch (e: any) {
    notificationStore.error('Update Failed', e.response?.data?.error?.message || 'Failed to update VM')
  } finally {
    vmsStore.loading = false
  }
}

// Submit handler - creates or updates based on mode
async function handleSubmit() {
  if (isEditMode.value) {
    await updateVm()
  } else {
    await createVm()
  }
}

// Tag input
const tagInput = ref('')

function addTag() {
  const tag = tagInput.value.trim()
  if (tag && !form.value.tags.includes(tag)) {
    form.value.tags.push(tag)
    tagInput.value = ''
  }
}

function removeTag(tag: string) {
  form.value.tags = form.value.tags.filter(t => t !== tag)
}

// Memory presets
function setMemoryPreset(mb: number) {
  form.value.memory.size_mb = mb
}

// CPU presets
function setCpuPreset(sockets: number, cores: number) {
  form.value.cpu.sockets = sockets
  form.value.cpu.cores_per_socket = cores
}

// Format bytes for display
function formatSize(gb: number): string {
  if (gb >= 1024) {
    return `${(gb / 1024).toFixed(1)} TB`
  }
  return `${gb} GB`
}
</script>

<template>
  <div class="p-6 max-w-6xl mx-auto">
    <!-- Loading overlay for edit mode -->
    <div v-if="loadingVm" class="fixed inset-0 bg-dark-900/80 flex items-center justify-center z-50">
      <div class="text-center">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-accent-500 mx-auto mb-4"></div>
        <p class="text-white">Loading VM configuration...</p>
      </div>
    </div>
    
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
      <h1 class="text-2xl font-bold text-white">{{ isEditMode ? 'Edit Virtual Machine' : 'Create Virtual Machine' }}</h1>
      <p class="text-dark-400 mt-1">{{ isEditMode ? 'Modify VM configuration' : 'Enterprise-grade VM configuration' }}</p>
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
        <span class="text-xs text-dark-400">Basic</span>
        <span class="text-xs text-dark-400">CPU/Memory</span>
        <span class="text-xs text-dark-400">Storage</span>
        <span class="text-xs text-dark-400">Network</span>
        <span class="text-xs text-dark-400">Boot/Security</span>
        <span class="text-xs text-dark-400">Review</span>
      </div>
    </div>

    <!-- Step Content -->
    <div class="card p-6">
      <!-- Step 1: Basic Info -->
      <div v-if="currentStep === 1" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Basic Information</h2>
        
        <div class="grid grid-cols-2 gap-6">
          <div class="col-span-2 md:col-span-1">
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

          <div class="col-span-2 md:col-span-1">
            <label class="block text-sm font-medium text-dark-300 mb-2">
              Template
            </label>
            <select v-model="form.template" class="input w-full">
              <option v-for="tpl in templates" :key="tpl.id" :value="tpl.id">
                {{ tpl.name }}
              </option>
            </select>
          </div>
        </div>

        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">Description</label>
          <textarea
            v-model="form.description"
            class="input w-full h-24 resize-none"
            placeholder="Optional description for this VM"
          />
        </div>

        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">Tags</label>
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

      <!-- Step 2: CPU & Memory -->
      <div v-else-if="currentStep === 2" class="space-y-6">
        <h2 class="text-lg font-medium text-white">CPU Configuration</h2>
        
        <!-- CPU Presets -->
        <div class="flex gap-2 flex-wrap">
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': totalVcpus === 1 }"
            @click="setCpuPreset(1, 1)"
          >1 vCPU</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': totalVcpus === 2 }"
            @click="setCpuPreset(1, 2)"
          >2 vCPUs</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': totalVcpus === 4 }"
            @click="setCpuPreset(1, 4)"
          >4 vCPUs</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': totalVcpus === 8 }"
            @click="setCpuPreset(2, 4)"
          >8 vCPUs</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': totalVcpus === 16 }"
            @click="setCpuPreset(2, 8)"
          >16 vCPUs</button>
        </div>

        <div class="grid grid-cols-3 gap-4">
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Sockets</label>
            <input v-model.number="form.cpu.sockets" type="number" min="1" max="8" class="input w-full" />
          </div>
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Cores per Socket</label>
            <input v-model.number="form.cpu.cores_per_socket" type="number" min="1" max="128" class="input w-full" />
          </div>
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Threads per Core</label>
            <select v-model.number="form.cpu.threads_per_core" class="input w-full">
              <option :value="1">1 (No HT)</option>
              <option :value="2">2 (Hyperthreading)</option>
            </select>
          </div>
        </div>

        <div class="bg-dark-700/50 rounded p-3">
          <span class="text-dark-400">Total vCPUs: </span>
          <span class="text-white font-bold">{{ totalVcpus }}</span>
        </div>

        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">CPU Model</label>
            <select v-model="form.cpu.model" class="input w-full">
              <option v-for="model in cpuModels" :key="model" :value="model">{{ model }}</option>
            </select>
          </div>
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Shares</label>
            <select v-model="form.cpu.shares" class="input w-full">
              <option value="low">Low</option>
              <option value="normal">Normal</option>
              <option value="high">High</option>
            </select>
          </div>
        </div>

        <div class="flex gap-6">
          <label class="flex items-center">
            <input v-model="form.cpu.hot_add" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Enable CPU Hot-Add</span>
          </label>
          <label class="flex items-center">
            <input v-model="form.cpu.nested_virt" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Enable Nested Virtualization</span>
          </label>
        </div>

        <hr class="border-dark-600 my-6" />

        <h2 class="text-lg font-medium text-white">Memory Configuration</h2>

        <!-- Memory Presets -->
        <div class="flex gap-2 flex-wrap">
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 512 }"
            @click="setMemoryPreset(512)"
          >512 MB</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 1024 }"
            @click="setMemoryPreset(1024)"
          >1 GB</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 2048 }"
            @click="setMemoryPreset(2048)"
          >2 GB</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 4096 }"
            @click="setMemoryPreset(4096)"
          >4 GB</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 8192 }"
            @click="setMemoryPreset(8192)"
          >8 GB</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 16384 }"
            @click="setMemoryPreset(16384)"
          >16 GB</button>
          <button 
            class="btn-secondary text-sm"
            :class="{ 'ring-2 ring-accent-500': form.memory.size_mb === 32768 }"
            @click="setMemoryPreset(32768)"
          >32 GB</button>
        </div>

        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Memory Size (MB)</label>
            <input v-model.number="form.memory.size_mb" type="number" min="256" step="256" class="input w-full" />
          </div>
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Shares</label>
            <select v-model="form.memory.shares" class="input w-full">
              <option value="low">Low</option>
              <option value="normal">Normal</option>
              <option value="high">High</option>
            </select>
          </div>
        </div>

        <div class="flex gap-6 flex-wrap">
          <label class="flex items-center">
            <input v-model="form.memory.hot_add" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Enable Memory Hot-Add</span>
          </label>
          <label class="flex items-center">
            <input v-model="form.memory.ballooning" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Memory Ballooning</span>
          </label>
          <label class="flex items-center">
            <input v-model="form.memory.ksm" type="checkbox" class="mr-2" />
            <span class="text-dark-300">KSM (Memory Dedup)</span>
          </label>
          <label class="flex items-center">
            <input v-model="form.memory.huge_pages" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Huge Pages</span>
          </label>
        </div>
      </div>

      <!-- Step 3: Storage -->
      <div v-else-if="currentStep === 3" class="space-y-6">
        <div class="flex justify-between items-center">
          <h2 class="text-lg font-medium text-white">Storage Configuration</h2>
          <button class="btn-primary text-sm" @click="addDisk">
            <svg class="w-4 h-4 mr-1 inline" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6"/>
            </svg>
            Add Disk
          </button>
        </div>

        <!-- Diskless VM info banner -->
        <div v-if="form.disks.length === 0" class="bg-blue-500/10 border border-blue-500/30 rounded-lg p-4">
          <div class="flex items-start gap-3">
            <svg class="w-5 h-5 text-blue-400 mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
            </svg>
            <div>
              <h3 class="text-blue-400 font-medium">Diskless VM</h3>
              <p class="text-dark-300 text-sm mt-1">
                This VM has no hard disks configured. It can still boot from:
              </p>
              <ul class="text-dark-400 text-sm mt-2 list-disc list-inside space-y-1">
                <li><strong class="text-dark-300">CD/DVD ISO</strong> - Live distributions, rescue disks</li>
                <li><strong class="text-dark-300">Network (PXE)</strong> - Thin clients, diskless workstations</li>
                <li><strong class="text-dark-300">iSCSI/NAS</strong> - Network-attached storage boot</li>
              </ul>
              <p class="text-dark-400 text-sm mt-2">
                Make sure to configure the boot order in Step 5 to use CD-ROM or Network first.
              </p>
            </div>
          </div>
        </div>

        <div class="space-y-4">
          <div 
            v-for="(disk, index) in form.disks" 
            :key="disk.id"
            class="bg-dark-700/50 rounded-lg p-4"
          >
            <div class="flex justify-between items-center mb-4">
              <span class="text-white font-medium">
                Disk {{ index + 1 }}
                <span v-if="disk.bootable" class="ml-2 text-xs bg-accent-500 text-white px-2 py-0.5 rounded">Boot</span>
              </span>
              <div class="flex gap-2">
                <button 
                  v-if="!disk.bootable"
                  class="text-xs text-dark-400 hover:text-white"
                  @click="setBootDisk(index)"
                >Set as Boot</button>
                <button 
                  class="text-xs text-red-400 hover:text-red-300"
                  @click="removeDisk(index)"
                  :title="form.disks.length === 1 ? 'Remove to create diskless VM' : 'Remove disk'"
                >Remove</button>
              </div>
            </div>

            <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
              <div>
                <label class="block text-xs text-dark-400 mb-1">Size (GB)</label>
                <input v-model.number="disk.size_gb" type="number" min="1" max="16384" class="input w-full text-sm" />
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Storage Pool</label>
                <select v-model="disk.storage_pool" class="input w-full text-sm">
                  <option v-for="pool in storagePools" :key="pool.id" :value="pool.id">
                    {{ pool.name }} ({{ pool.available_gb }} GB free)
                  </option>
                </select>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Format</label>
                <select v-model="disk.format" class="input w-full text-sm">
                  <option v-for="fmt in diskFormats" :key="fmt" :value="fmt">{{ fmt.toUpperCase() }}</option>
                </select>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Bus</label>
                <select v-model="disk.bus" class="input w-full text-sm">
                  <option v-for="bus in diskBusOptions" :key="bus" :value="bus">{{ bus }}</option>
                </select>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Cache</label>
                <select v-model="disk.cache" class="input w-full text-sm">
                  <option v-for="cache in diskCacheOptions" :key="cache" :value="cache">{{ cache }}</option>
                </select>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">IOPS Limit</label>
                <input v-model.number="disk.iops_limit" type="number" min="0" class="input w-full text-sm" placeholder="Unlimited" />
              </div>
              <div class="col-span-2 flex gap-4 items-center">
                <label class="flex items-center text-sm">
                  <input v-model="disk.ssd_emulation" type="checkbox" class="mr-2" />
                  <span class="text-dark-300">SSD/TRIM</span>
                </label>
              </div>
            </div>
          </div>
        </div>

        <div class="bg-dark-700/50 rounded p-3">
          <span class="text-dark-400">Total Storage: </span>
          <span class="text-white font-bold">{{ formatSize(totalDiskGb) }}</span>
        </div>

        <hr class="border-dark-600 my-6" />

        <h3 class="text-md font-medium text-white">CD/DVD Drive</h3>
        <div class="flex items-center mb-4">
          <label class="flex items-center">
            <input v-model="form.cdrom.enabled" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Enable CD/DVD Drive</span>
          </label>
        </div>
        
        <div v-if="form.cdrom.enabled" class="grid grid-cols-2 gap-4">
          <div>
            <label class="block text-sm text-dark-400 mb-1">ISO Image</label>
            <select v-model="form.cdrom.iso" class="input w-full">
              <option value="">-- No media --</option>
              <option v-for="iso in isoImages" :key="iso.id" :value="iso.id">
                {{ iso.name }} ({{ iso.size_mb }} MB)
              </option>
            </select>
          </div>
          <div>
            <label class="block text-sm text-dark-400 mb-1">Bus Type</label>
            <select v-model="form.cdrom.bus" class="input w-full">
              <option value="sata">SATA</option>
              <option value="ide">IDE</option>
              <option value="scsi">SCSI</option>
            </select>
          </div>
        </div>
      </div>

      <!-- Step 4: Network -->
      <div v-else-if="currentStep === 4" class="space-y-6">
        <div class="flex justify-between items-center">
          <h2 class="text-lg font-medium text-white">Network Interfaces</h2>
          <button class="btn-primary text-sm" @click="addNetwork">
            <svg class="w-4 h-4 mr-1 inline" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6"/>
            </svg>
            Add Network Interface
          </button>
        </div>

        <div class="space-y-4">
          <div 
            v-for="(nic, index) in form.networks" 
            :key="nic.id"
            class="bg-dark-700/50 rounded-lg p-4"
          >
            <div class="flex justify-between items-center mb-4">
              <span class="text-white font-medium">Network Interface {{ index + 1 }}</span>
              <button 
                v-if="form.networks.length > 1"
                class="text-xs text-red-400 hover:text-red-300"
                @click="removeNetwork(index)"
              >Remove</button>
            </div>

            <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
              <div>
                <label class="block text-xs text-dark-400 mb-1">Network/Port Group</label>
                <select v-model="nic.network" class="input w-full text-sm">
                  <option v-for="net in networks" :key="net.id" :value="net.id">
                    {{ net.name }}
                  </option>
                </select>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Model</label>
                <select v-model="nic.model" class="input w-full text-sm">
                  <option v-for="model in nicModels" :key="model" :value="model">{{ model }}</option>
                </select>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">MAC Address</label>
                <div class="flex gap-1">
                  <input v-model="nic.mac" type="text" class="input flex-1 text-sm font-mono" placeholder="Auto-generate" />
                  <button class="btn-secondary text-xs px-2" @click="generateMac(index)">Gen</button>
                </div>
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">VLAN ID</label>
                <input v-model.number="nic.vlan_id" type="number" min="1" max="4094" class="input w-full text-sm" placeholder="None" />
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Inbound Limit (Mbps)</label>
                <input v-model.number="nic.inbound_limit_mbps" type="number" min="0" class="input w-full text-sm" placeholder="Unlimited" />
              </div>
              <div>
                <label class="block text-xs text-dark-400 mb-1">Outbound Limit (Mbps)</label>
                <input v-model.number="nic.outbound_limit_mbps" type="number" min="0" class="input w-full text-sm" placeholder="Unlimited" />
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Step 5: Boot & Security -->
      <div v-else-if="currentStep === 5" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Boot Configuration</h2>
        
        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Firmware</label>
            <select v-model="form.boot.firmware" class="input w-full">
              <option v-for="fw in firmwareOptions" :key="fw" :value="fw">{{ fw.toUpperCase() }}</option>
            </select>
          </div>
          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Machine Type</label>
            <select v-model="form.boot.machine_type" class="input w-full">
              <option v-for="mt in machineTypes" :key="mt" :value="mt">{{ mt }}</option>
            </select>
          </div>
        </div>

        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">Execution Backend</label>
          <div class="grid grid-cols-2 gap-3">
            <label 
              v-for="backend in backendOptions" 
              :key="backend.value"
              class="relative flex items-start p-3 rounded-lg border cursor-pointer transition-colors"
              :class="form.boot.backend === backend.value 
                ? 'border-primary-500 bg-primary-500/10' 
                : 'border-dark-600 hover:border-dark-500'"
            >
              <input 
                type="radio" 
                :value="backend.value" 
                v-model="form.boot.backend"
                class="mt-1 mr-3"
              />
              <div>
                <span class="text-white font-medium">{{ backend.label }}</span>
                <p class="text-xs text-dark-400 mt-0.5">{{ backend.description }}</p>
              </div>
            </label>
          </div>
        </div>

        <div class="flex gap-6">
          <label class="flex items-center">
            <input v-model="form.boot.secure_boot" type="checkbox" class="mr-2" :disabled="form.boot.firmware !== 'uefi'" />
            <span class="text-dark-300" :class="{ 'opacity-50': form.boot.firmware !== 'uefi' }">Secure Boot (UEFI only)</span>
          </label>
        </div>

        <div>
          <label class="block text-sm font-medium text-dark-300 mb-2">Boot Order</label>
          <div class="flex gap-2 flex-wrap">
            <span 
              v-for="(device, index) in form.boot.order"
              :key="device"
              class="bg-dark-700 px-3 py-1 rounded text-dark-300"
            >
              {{ index + 1 }}. {{ device }}
            </span>
          </div>
        </div>

        <hr class="border-dark-600 my-6" />

        <h2 class="text-lg font-medium text-white">Security Configuration</h2>

        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <div>
              <label class="flex items-center">
                <input v-model="form.security.tpm" type="checkbox" class="mr-2" />
                <span class="text-dark-300">Enable TPM (Trusted Platform Module)</span>
              </label>
              <p class="text-xs text-dark-500 ml-6">Required for Windows 11 and BitLocker</p>
            </div>
            <select 
              v-if="form.security.tpm"
              v-model="form.security.tpm_version" 
              class="input w-32"
            >
              <option value="1.2">TPM 1.2</option>
              <option value="2.0">TPM 2.0</option>
            </select>
          </div>

          <div>
            <label class="flex items-center">
              <input v-model="form.security.sev" type="checkbox" class="mr-2" />
              <span class="text-dark-300">Enable AMD SEV (Secure Encrypted Virtualization)</span>
            </label>
            <p class="text-xs text-dark-500 ml-6">Encrypts VM memory (AMD EPYC required)</p>
          </div>

          <div>
            <label class="block text-sm font-medium text-dark-300 mb-2">Isolation Level</label>
            <select v-model="form.security.isolation" class="input w-full">
              <option value="none">None</option>
              <option value="hypervisor">Hypervisor (Default)</option>
              <option value="hardware">Hardware (SEV/TDX)</option>
            </select>
          </div>
        </div>

        <hr class="border-dark-600 my-6" />

        <h2 class="text-lg font-medium text-white">Advanced Options</h2>

        <div class="space-y-4">
          <label class="flex items-center">
            <input v-model="form.advanced.start_after_create" type="checkbox" class="mr-2" />
            <span class="text-dark-300">Start VM after creation</span>
          </label>

          <div v-if="clusterNodes.length > 0">
            <label class="block text-sm font-medium text-dark-300 mb-2">Target Host</label>
            <select v-model="form.advanced.host_node" class="input w-full">
              <option :value="undefined">Auto-select</option>
              <option v-for="node in clusterNodes" :key="node.id" :value="node.id">
                {{ node.name }} ({{ node.status }})
              </option>
            </select>
          </div>
        </div>
      </div>

      <!-- Step 6: Review -->
      <div v-else-if="currentStep === 6" class="space-y-6">
        <h2 class="text-lg font-medium text-white">Review Configuration</h2>

        <div class="grid grid-cols-2 gap-6">
          <!-- Basic Info -->
          <div class="bg-dark-700/50 rounded-lg p-4">
            <h3 class="text-sm font-medium text-dark-400 mb-3">Basic Information</h3>
            <div class="space-y-2 text-sm">
              <div class="flex justify-between">
                <span class="text-dark-400">Name</span>
                <span class="text-white">{{ form.name }}</span>
              </div>
              <div v-if="form.description" class="flex justify-between">
                <span class="text-dark-400">Description</span>
                <span class="text-white truncate ml-4">{{ form.description }}</span>
              </div>
              <div v-if="form.tags.length > 0" class="flex justify-between">
                <span class="text-dark-400">Tags</span>
                <span class="text-white">{{ form.tags.join(', ') }}</span>
              </div>
            </div>
          </div>

          <!-- CPU & Memory -->
          <div class="bg-dark-700/50 rounded-lg p-4">
            <h3 class="text-sm font-medium text-dark-400 mb-3">Compute</h3>
            <div class="space-y-2 text-sm">
              <div class="flex justify-between">
                <span class="text-dark-400">vCPUs</span>
                <span class="text-white">{{ totalVcpus }} ({{ form.cpu.sockets }}S × {{ form.cpu.cores_per_socket }}C × {{ form.cpu.threads_per_core }}T)</span>
              </div>
              <div class="flex justify-between">
                <span class="text-dark-400">CPU Model</span>
                <span class="text-white">{{ form.cpu.model }}</span>
              </div>
              <div class="flex justify-between">
                <span class="text-dark-400">Memory</span>
                <span class="text-white">{{ form.memory.size_mb }} MB</span>
              </div>
              <div v-if="form.cpu.nested_virt" class="flex justify-between">
                <span class="text-dark-400">Nested Virt</span>
                <span class="text-green-400">Enabled</span>
              </div>
            </div>
          </div>

          <!-- Storage -->
          <div class="bg-dark-700/50 rounded-lg p-4">
            <h3 class="text-sm font-medium text-dark-400 mb-3">Storage ({{ form.disks.length }} disk{{ form.disks.length > 1 ? 's' : '' }})</h3>
            <div class="space-y-2 text-sm">
              <div v-for="(disk, idx) in form.disks" :key="disk.id" class="flex justify-between">
                <span class="text-dark-400">Disk {{ idx + 1 }}{{ disk.bootable ? ' (Boot)' : '' }}</span>
                <span class="text-white">{{ disk.size_gb }} GB {{ disk.format.toUpperCase() }} / {{ disk.bus }}</span>
              </div>
              <div class="flex justify-between pt-2 border-t border-dark-600">
                <span class="text-dark-400">Total</span>
                <span class="text-white font-medium">{{ formatSize(totalDiskGb) }}</span>
              </div>
            </div>
          </div>

          <!-- Network -->
          <div class="bg-dark-700/50 rounded-lg p-4">
            <h3 class="text-sm font-medium text-dark-400 mb-3">Network ({{ form.networks.length }} interface{{ form.networks.length > 1 ? 's' : '' }})</h3>
            <div class="space-y-2 text-sm">
              <div v-for="(nic, idx) in form.networks" :key="nic.id" class="flex justify-between">
                <span class="text-dark-400">NIC {{ idx + 1 }}</span>
                <span class="text-white">{{ nic.network }} / {{ nic.model }}</span>
              </div>
            </div>
          </div>

          <!-- Boot & Security -->
          <div class="bg-dark-700/50 rounded-lg p-4 col-span-2">
            <h3 class="text-sm font-medium text-dark-400 mb-3">Boot & Security</h3>
            <div class="grid grid-cols-2 gap-4 text-sm">
              <div class="flex justify-between">
                <span class="text-dark-400">Firmware</span>
                <span class="text-white">{{ form.boot.firmware.toUpperCase() }}</span>
              </div>
              <div class="flex justify-between">
                <span class="text-dark-400">Machine Type</span>
                <span class="text-white">{{ form.boot.machine_type }}</span>
              </div>
              <div class="flex justify-between">
                <span class="text-dark-400">Secure Boot</span>
                <span :class="form.boot.secure_boot ? 'text-green-400' : 'text-dark-500'">
                  {{ form.boot.secure_boot ? 'Enabled' : 'Disabled' }}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-dark-400">TPM</span>
                <span :class="form.security.tpm ? 'text-green-400' : 'text-dark-500'">
                  {{ form.security.tpm ? `Enabled (${form.security.tpm_version})` : 'Disabled' }}
                </span>
              </div>
            </div>
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
          @click="handleSubmit"
        >
          <svg v-if="vmsStore.loading" class="animate-spin -ml-1 mr-2 h-4 w-4 text-white" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
          </svg>
          {{ vmsStore.loading ? (isEditMode ? 'Saving...' : 'Creating...') : (isEditMode ? 'Save Changes' : 'Create VM') }}
        </button>
      </div>
    </div>
  </div>
</template>
