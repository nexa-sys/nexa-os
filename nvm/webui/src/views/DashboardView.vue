<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useVmsStore } from '@/stores/vms'
import { Chart, registerables } from 'chart.js'
import { Line, Doughnut } from 'vue-chartjs'

Chart.register(...registerables)

const vmsStore = useVmsStore()

// Stats
const stats = ref({
  totalVms: 0,
  runningVms: 0,
  totalCpu: 0,
  usedCpu: 0,
  totalMemoryGb: 64,
  usedMemoryGb: 0,
  totalStorageTb: 10,
  usedStorageTb: 0,
  networkThroughput: 0,
  alerts: 0,
})

// Resource usage chart data
const cpuChartData = computed(() => ({
  labels: ['Used', 'Available'],
  datasets: [{
    data: [stats.value.usedCpu, stats.value.totalCpu - stats.value.usedCpu],
    backgroundColor: ['#6366f1', '#1e1e2e'],
    borderWidth: 0,
  }],
}))

const memoryChartData = computed(() => ({
  labels: ['Used', 'Available'],
  datasets: [{
    data: [stats.value.usedMemoryGb, stats.value.totalMemoryGb - stats.value.usedMemoryGb],
    backgroundColor: ['#22c55e', '#1e1e2e'],
    borderWidth: 0,
  }],
}))

const chartOptions = {
  responsive: true,
  maintainAspectRatio: false,
  cutout: '75%',
  plugins: {
    legend: { display: false },
  },
}

// Activity chart
const activityChartData = ref({
  labels: ['00:00', '04:00', '08:00', '12:00', '16:00', '20:00', '24:00'],
  datasets: [
    {
      label: 'CPU %',
      data: [25, 30, 45, 60, 55, 40, 35],
      borderColor: '#6366f1',
      backgroundColor: 'rgba(99, 102, 241, 0.1)',
      fill: true,
      tension: 0.4,
    },
    {
      label: 'Memory %',
      data: [40, 42, 50, 65, 60, 55, 50],
      borderColor: '#22c55e',
      backgroundColor: 'rgba(34, 197, 94, 0.1)',
      fill: true,
      tension: 0.4,
    },
  ],
})

const activityChartOptions = {
  responsive: true,
  maintainAspectRatio: false,
  interaction: {
    intersect: false,
    mode: 'index' as const,
  },
  plugins: {
    legend: {
      position: 'bottom' as const,
      labels: { color: '#a1a1aa' },
    },
  },
  scales: {
    x: {
      grid: { color: '#27272a' },
      ticks: { color: '#a1a1aa' },
    },
    y: {
      grid: { color: '#27272a' },
      ticks: { color: '#a1a1aa' },
      min: 0,
      max: 100,
    },
  },
}

// Recent events
const recentEvents = ref([
  { id: 1, type: 'success', message: 'VM "web-server-01" started successfully', time: '2 min ago' },
  { id: 2, type: 'info', message: 'Backup completed for "db-server"', time: '15 min ago' },
  { id: 3, type: 'warning', message: 'High CPU usage on node-02', time: '1 hour ago' },
  { id: 4, type: 'success', message: 'Live migration completed', time: '2 hours ago' },
])

onMounted(async () => {
  await vmsStore.fetchVms()
  
  stats.value.totalVms = vmsStore.vms.length
  stats.value.runningVms = vmsStore.runningVms.length
  stats.value.usedCpu = vmsStore.totalCpu
  stats.value.totalCpu = 64
  stats.value.usedMemoryGb = Math.round(vmsStore.totalMemory / 1024)
})
</script>

<template>
  <div class="p-6 space-y-6">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold text-white">Dashboard</h1>
        <p class="text-dark-400 mt-1">Overview of your virtualization infrastructure</p>
      </div>
      <div class="flex items-center space-x-3">
        <span class="text-sm text-dark-400">Last updated: Just now</span>
        <button class="btn-secondary flex items-center space-x-2">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/>
          </svg>
          <span>Refresh</span>
        </button>
      </div>
    </div>

    <!-- Stats Cards -->
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
      <div class="card p-5">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-dark-400 text-sm">Virtual Machines</p>
            <p class="text-2xl font-bold text-white mt-1">{{ stats.totalVms }}</p>
            <p class="text-sm text-green-400 mt-1">{{ stats.runningVms }} running</p>
          </div>
          <div class="w-12 h-12 bg-accent-500/10 rounded-lg flex items-center justify-center">
            <svg class="w-6 h-6 text-accent-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2"/>
            </svg>
          </div>
        </div>
      </div>

      <div class="card p-5">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-dark-400 text-sm">CPU Usage</p>
            <p class="text-2xl font-bold text-white mt-1">{{ stats.usedCpu }} / {{ stats.totalCpu }}</p>
            <p class="text-sm text-dark-400 mt-1">vCPUs allocated</p>
          </div>
          <div class="w-12 h-12 bg-indigo-500/10 rounded-lg flex items-center justify-center">
            <svg class="w-6 h-6 text-indigo-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z"/>
            </svg>
          </div>
        </div>
      </div>

      <div class="card p-5">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-dark-400 text-sm">Memory</p>
            <p class="text-2xl font-bold text-white mt-1">{{ stats.usedMemoryGb }} / {{ stats.totalMemoryGb }} GB</p>
            <p class="text-sm text-dark-400 mt-1">{{ Math.round(stats.usedMemoryGb / stats.totalMemoryGb * 100) }}% used</p>
          </div>
          <div class="w-12 h-12 bg-green-500/10 rounded-lg flex items-center justify-center">
            <svg class="w-6 h-6 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"/>
            </svg>
          </div>
        </div>
      </div>

      <div class="card p-5">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-dark-400 text-sm">Storage</p>
            <p class="text-2xl font-bold text-white mt-1">{{ stats.usedStorageTb }} / {{ stats.totalStorageTb }} TB</p>
            <p class="text-sm text-dark-400 mt-1">{{ Math.round(stats.usedStorageTb / stats.totalStorageTb * 100) }}% used</p>
          </div>
          <div class="w-12 h-12 bg-purple-500/10 rounded-lg flex items-center justify-center">
            <svg class="w-6 h-6 text-purple-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"/>
            </svg>
          </div>
        </div>
      </div>
    </div>

    <!-- Charts Row -->
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
      <!-- Resource Usage -->
      <div class="card p-6">
        <h3 class="text-lg font-medium text-white mb-4">Resource Allocation</h3>
        <div class="grid grid-cols-2 gap-6">
          <div class="text-center">
            <div class="w-24 h-24 mx-auto relative">
              <Doughnut :data="cpuChartData" :options="chartOptions" />
              <div class="absolute inset-0 flex items-center justify-center">
                <span class="text-lg font-bold text-white">{{ Math.round(stats.usedCpu / stats.totalCpu * 100) }}%</span>
              </div>
            </div>
            <p class="text-dark-400 text-sm mt-2">CPU</p>
          </div>
          <div class="text-center">
            <div class="w-24 h-24 mx-auto relative">
              <Doughnut :data="memoryChartData" :options="chartOptions" />
              <div class="absolute inset-0 flex items-center justify-center">
                <span class="text-lg font-bold text-white">{{ Math.round(stats.usedMemoryGb / stats.totalMemoryGb * 100) }}%</span>
              </div>
            </div>
            <p class="text-dark-400 text-sm mt-2">Memory</p>
          </div>
        </div>
      </div>

      <!-- Activity Chart -->
      <div class="card p-6 lg:col-span-2">
        <h3 class="text-lg font-medium text-white mb-4">Resource Usage (24h)</h3>
        <div class="h-48">
          <Line :data="activityChartData" :options="activityChartOptions" />
        </div>
      </div>
    </div>

    <!-- Bottom Row -->
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
      <!-- Recent Events -->
      <div class="card p-6">
        <div class="flex items-center justify-between mb-4">
          <h3 class="text-lg font-medium text-white">Recent Events</h3>
          <a href="#" class="text-sm text-accent-400 hover:text-accent-300">View all</a>
        </div>
        <div class="space-y-3">
          <div
            v-for="event in recentEvents"
            :key="event.id"
            class="flex items-start space-x-3 p-3 rounded-lg bg-dark-700/50"
          >
            <div :class="[
              'w-2 h-2 mt-2 rounded-full',
              {
                'bg-green-400': event.type === 'success',
                'bg-blue-400': event.type === 'info',
                'bg-yellow-400': event.type === 'warning',
                'bg-red-400': event.type === 'error',
              }
            ]" />
            <div class="flex-1 min-w-0">
              <p class="text-sm text-white">{{ event.message }}</p>
              <p class="text-xs text-dark-500 mt-1">{{ event.time }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- Quick Actions -->
      <div class="card p-6">
        <h3 class="text-lg font-medium text-white mb-4">Quick Actions</h3>
        <div class="grid grid-cols-2 gap-3">
          <RouterLink to="/vms/create" class="flex items-center space-x-3 p-4 rounded-lg bg-dark-700/50 hover:bg-dark-700 transition-colors">
            <div class="w-10 h-10 bg-accent-500/10 rounded-lg flex items-center justify-center">
              <svg class="w-5 h-5 text-accent-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>
              </svg>
            </div>
            <span class="text-sm text-white">Create VM</span>
          </RouterLink>
          <RouterLink to="/templates" class="flex items-center space-x-3 p-4 rounded-lg bg-dark-700/50 hover:bg-dark-700 transition-colors">
            <div class="w-10 h-10 bg-purple-500/10 rounded-lg flex items-center justify-center">
              <svg class="w-5 h-5 text-purple-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7v8a2 2 0 002 2h6M8 7V5a2 2 0 012-2h4.586a1 1 0 01.707.293l4.414 4.414a1 1 0 01.293.707V15a2 2 0 01-2 2h-2M8 7H6a2 2 0 00-2 2v10a2 2 0 002 2h8a2 2 0 002-2v-2"/>
              </svg>
            </div>
            <span class="text-sm text-white">Templates</span>
          </RouterLink>
          <RouterLink to="/backup" class="flex items-center space-x-3 p-4 rounded-lg bg-dark-700/50 hover:bg-dark-700 transition-colors">
            <div class="w-10 h-10 bg-green-500/10 rounded-lg flex items-center justify-center">
              <svg class="w-5 h-5 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"/>
              </svg>
            </div>
            <span class="text-sm text-white">Backup</span>
          </RouterLink>
          <RouterLink to="/settings" class="flex items-center space-x-3 p-4 rounded-lg bg-dark-700/50 hover:bg-dark-700 transition-colors">
            <div class="w-10 h-10 bg-orange-500/10 rounded-lg flex items-center justify-center">
              <svg class="w-5 h-5 text-orange-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"/>
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/>
              </svg>
            </div>
            <span class="text-sm text-white">Settings</span>
          </RouterLink>
        </div>
      </div>
    </div>
  </div>
</template>
