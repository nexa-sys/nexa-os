import { createRouter, createWebHashHistory } from 'vue-router';
import Home from '@/views/Home.vue';

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      name: 'home',
      component: Home
    },
    {
      path: '/features',
      name: 'features',
      component: () => import('@/views/Features.vue')
    },
    {
      path: '/modules',
      name: 'modules',
      component: () => import('@/views/Modules.vue')
    },
    {
      path: '/programs',
      name: 'programs',
      component: () => import('@/views/Programs.vue')
    },
    {
      path: '/build',
      name: 'build',
      component: () => import('@/views/Build.vue')
    }
  ]
});

export default router;
