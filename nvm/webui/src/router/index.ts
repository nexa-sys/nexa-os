import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'login',
    component: () => import('@/views/LoginView.vue'),
    meta: { guest: true }
  },
  {
    path: '/',
    name: 'dashboard',
    component: () => import('@/views/DashboardView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/vms',
    name: 'vms',
    component: () => import('@/views/VmsView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/vms/create',
    name: 'vm-create',
    component: () => import('@/views/VmCreateView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/vms/:id',
    name: 'vm-detail',
    component: () => import('@/views/VmDetailView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/vms/:id/edit',
    name: 'vm-edit',
    component: () => import('@/views/VmCreateView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/storage',
    name: 'storage',
    component: () => import('@/views/StorageView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/network',
    name: 'network',
    component: () => import('@/views/NetworkView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/cluster',
    name: 'cluster',
    component: () => import('@/views/ClusterView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/templates',
    name: 'templates',
    component: () => import('@/views/TemplatesView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/backup',
    name: 'backup',
    component: () => import('@/views/BackupView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/users',
    name: 'users',
    component: () => import('@/views/UsersView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('@/views/SettingsView.vue'),
    meta: { requiresAuth: true }
  },
  {
    path: '/console/:id',
    name: 'console',
    component: () => import('@/views/ConsoleView.vue'),
    meta: { requiresAuth: true }
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

// Navigation guard
router.beforeEach((to, _from, next) => {
  const authStore = useAuthStore()
  
  if (to.meta.requiresAuth && !authStore.isAuthenticated) {
    next({ name: 'login', query: { redirect: to.fullPath } })
  } else if (to.meta.guest && authStore.isAuthenticated) {
    next({ name: 'dashboard' })
  } else {
    next()
  }
})

export default router
