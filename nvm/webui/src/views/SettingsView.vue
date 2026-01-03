<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { api } from '../api'

const tabs = ref([
  { id: 'general', name: 'General', icon: 'cog' },
  { id: 'network', name: 'Network', icon: 'globe' },
  { id: 'security', name: 'Security', icon: 'shield' },
  { id: 'backup', name: 'Backup', icon: 'cloud' },
  { id: 'notifications', name: 'Notifications', icon: 'bell' },
])

const activeTab = ref('general')
const loading = ref(true)
const saving = ref(false)
const error = ref<string | null>(null)

// Settings state
const settings = ref({
  general: {
    clusterName: '',
    timezone: 'UTC',
    language: 'en',
    autoUpdate: true,
  },
  network: {
    managementInterface: 'eth0',
    dnsServers: ['8.8.8.8', '8.8.4.4'],
    ntpServers: ['pool.ntp.org'],
  },
  security: {
    twoFactorAuth: false,
    sessionTimeout: 30,
    passwordPolicy: 'strong',
    auditLogging: true,
  },
  backup: {
    autoBackup: true,
    backupSchedule: 'daily',
    retentionDays: 30,
    backupLocation: '/backup',
  },
  notifications: {
    emailEnabled: false,
    emailRecipients: '',
    slackWebhook: '',
    alertThresholds: {
      cpu: 90,
      memory: 85,
      disk: 80,
    },
  },
})

async function fetchSettings() {
  try {
    loading.value = true
    error.value = null
    const response = await api.get('/system/config')
    if (response.data.success && response.data.data) {
      const config = response.data.data
      settings.value.general.clusterName = config.cluster_name || 'local-cluster'
      settings.value.general.timezone = config.timezone || 'UTC'
      settings.value.network.dnsServers = config.dns_servers || ['8.8.8.8']
      settings.value.security.sessionTimeout = config.session_timeout || 30
      settings.value.security.auditLogging = config.audit_logging ?? true
    }
  } catch (e: any) {
    error.value = e.message || 'Failed to load settings'
    console.error('Failed to fetch settings:', e)
  } finally {
    loading.value = false
  }
}

async function saveSettings() {
  try {
    saving.value = true
    await api.put('/system/config', {
      cluster_name: settings.value.general.clusterName,
      timezone: settings.value.general.timezone,
      dns_servers: settings.value.network.dnsServers,
      session_timeout: settings.value.security.sessionTimeout,
      audit_logging: settings.value.security.auditLogging,
    })
    console.log('Settings saved successfully')
  } catch (e: any) {
    error.value = e.message || 'Failed to save settings'
    console.error('Failed to save settings:', e)
  } finally {
    saving.value = false
  }
}

onMounted(() => {
  fetchSettings()
})
</script>

<template>
  <div class="p-6 space-y-6">
    <!-- Header -->
    <div>
      <h1 class="text-2xl font-bold text-white">Settings</h1>
      <p class="text-dark-400 mt-1">Configure your NVM Hypervisor installation</p>
    </div>

    <div class="flex space-x-6">
      <!-- Sidebar -->
      <div class="w-64 flex-shrink-0">
        <nav class="space-y-1">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            :class="[
              'w-full flex items-center space-x-3 px-4 py-3 rounded-lg text-sm font-medium transition-colors',
              activeTab === tab.id
                ? 'bg-accent-500/10 text-accent-400'
                : 'text-dark-300 hover:bg-dark-700 hover:text-white'
            ]"
            @click="activeTab = tab.id"
          >
            <span>{{ tab.name }}</span>
          </button>
        </nav>
      </div>

      <!-- Content -->
      <div class="flex-1">
        <div class="card p-6">
          <!-- General Settings -->
          <div v-if="activeTab === 'general'" class="space-y-6">
            <h2 class="text-lg font-medium text-white">General Settings</h2>
            
            <div class="grid grid-cols-2 gap-6">
              <div>
                <label class="block text-sm font-medium text-dark-300 mb-2">Cluster Name</label>
                <input v-model="settings.general.clusterName" type="text" class="input w-full" />
              </div>
              <div>
                <label class="block text-sm font-medium text-dark-300 mb-2">Timezone</label>
                <select v-model="settings.general.timezone" class="input w-full">
                  <option value="UTC">UTC</option>
                  <option value="America/New_York">America/New_York</option>
                  <option value="Europe/London">Europe/London</option>
                  <option value="Asia/Tokyo">Asia/Tokyo</option>
                  <option value="Asia/Shanghai">Asia/Shanghai</option>
                </select>
              </div>
              <div>
                <label class="block text-sm font-medium text-dark-300 mb-2">Language</label>
                <select v-model="settings.general.language" class="input w-full">
                  <option value="en">English</option>
                  <option value="zh">中文</option>
                  <option value="ja">日本語</option>
                </select>
              </div>
              <div>
                <label class="flex items-center space-x-3">
                  <input v-model="settings.general.autoUpdate" type="checkbox" class="w-4 h-4 rounded border-dark-600 bg-dark-700 text-accent-500" />
                  <span class="text-sm text-dark-300">Enable automatic updates</span>
                </label>
              </div>
            </div>
          </div>

          <!-- Security Settings -->
          <div v-else-if="activeTab === 'security'" class="space-y-6">
            <h2 class="text-lg font-medium text-white">Security Settings</h2>
            
            <div class="space-y-4">
              <div class="flex items-center justify-between p-4 bg-dark-700/50 rounded-lg">
                <div>
                  <h3 class="text-white font-medium">Two-Factor Authentication</h3>
                  <p class="text-sm text-dark-400 mt-1">Require 2FA for all users</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="settings.security.twoFactorAuth" type="checkbox" class="sr-only peer">
                  <div class="w-11 h-6 bg-dark-600 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-accent-500 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-accent-500"></div>
                </label>
              </div>

              <div class="flex items-center justify-between p-4 bg-dark-700/50 rounded-lg">
                <div>
                  <h3 class="text-white font-medium">Audit Logging</h3>
                  <p class="text-sm text-dark-400 mt-1">Log all administrative actions</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="settings.security.auditLogging" type="checkbox" class="sr-only peer">
                  <div class="w-11 h-6 bg-dark-600 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-accent-500 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-accent-500"></div>
                </label>
              </div>

              <div>
                <label class="block text-sm font-medium text-dark-300 mb-2">Session Timeout (minutes)</label>
                <input v-model.number="settings.security.sessionTimeout" type="number" class="input w-full max-w-xs" />
              </div>

              <div>
                <label class="block text-sm font-medium text-dark-300 mb-2">Password Policy</label>
                <select v-model="settings.security.passwordPolicy" class="input w-full max-w-xs">
                  <option value="weak">Weak (8+ characters)</option>
                  <option value="medium">Medium (12+ chars, mixed case)</option>
                  <option value="strong">Strong (16+ chars, symbols required)</option>
                </select>
              </div>
            </div>
          </div>

          <!-- Notifications Settings -->
          <div v-else-if="activeTab === 'notifications'" class="space-y-6">
            <h2 class="text-lg font-medium text-white">Notification Settings</h2>
            
            <div class="space-y-6">
              <div class="flex items-center justify-between p-4 bg-dark-700/50 rounded-lg">
                <div>
                  <h3 class="text-white font-medium">Email Notifications</h3>
                  <p class="text-sm text-dark-400 mt-1">Send alerts via email</p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="settings.notifications.emailEnabled" type="checkbox" class="sr-only peer">
                  <div class="w-11 h-6 bg-dark-600 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-accent-500 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-accent-500"></div>
                </label>
              </div>

              <div v-if="settings.notifications.emailEnabled">
                <label class="block text-sm font-medium text-dark-300 mb-2">Email Recipients</label>
                <input v-model="settings.notifications.emailRecipients" type="text" class="input w-full" placeholder="admin@example.com, ops@example.com" />
              </div>

              <div>
                <label class="block text-sm font-medium text-dark-300 mb-2">Slack Webhook URL</label>
                <input v-model="settings.notifications.slackWebhook" type="text" class="input w-full" placeholder="https://hooks.slack.com/services/..." />
              </div>

              <div>
                <h3 class="text-white font-medium mb-4">Alert Thresholds</h3>
                <div class="grid grid-cols-3 gap-4">
                  <div>
                    <label class="block text-sm font-medium text-dark-300 mb-2">CPU Usage (%)</label>
                    <input v-model.number="settings.notifications.alertThresholds.cpu" type="number" min="0" max="100" class="input w-full" />
                  </div>
                  <div>
                    <label class="block text-sm font-medium text-dark-300 mb-2">Memory Usage (%)</label>
                    <input v-model.number="settings.notifications.alertThresholds.memory" type="number" min="0" max="100" class="input w-full" />
                  </div>
                  <div>
                    <label class="block text-sm font-medium text-dark-300 mb-2">Disk Usage (%)</label>
                    <input v-model.number="settings.notifications.alertThresholds.disk" type="number" min="0" max="100" class="input w-full" />
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Placeholder for other tabs -->
          <div v-else class="text-center py-12">
            <div class="w-16 h-16 mx-auto bg-dark-700 rounded-full flex items-center justify-center mb-4">
              <svg class="w-8 h-8 text-dark-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6V4m0 2a2 2 0 100 4m0-4a2 2 0 110 4m-6 8a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4m6 6v10m6-2a2 2 0 100-4m0 4a2 2 0 110-4m0 4v2m0-6V4"/>
              </svg>
            </div>
            <h3 class="text-lg font-medium text-white">{{ tabs.find(t => t.id === activeTab)?.name }} Settings</h3>
            <p class="text-dark-400 mt-2">Configuration options coming soon</p>
          </div>

          <!-- Save Button -->
          <div class="mt-8 pt-6 border-t border-dark-600 flex justify-end">
            <button class="btn-primary" @click="saveSettings">
              Save Changes
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
