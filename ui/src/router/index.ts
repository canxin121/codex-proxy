import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      redirect: '/overview',
    },
    {
      path: '/overview',
      name: 'overview',
      component: () => import('@/views/OverviewView.vue'),
      meta: {
        label: '总览',
      },
    },
    {
      path: '/credentials',
      name: 'credentials',
      component: () => import('@/views/CredentialsView.vue'),
      meta: {
        label: '凭证',
      },
    },
    {
      path: '/api-keys',
      name: 'api-keys',
      component: () => import('@/views/ApiKeysView.vue'),
      meta: {
        label: 'API Keys',
      },
    },
    {
      path: '/requests',
      name: 'requests',
      component: () => import('@/views/RequestsView.vue'),
      meta: {
        label: '请求记录',
      },
    },
    {
      path: '/auth/callback',
      name: 'auth-callback',
      component: () => import('@/views/AuthCallbackView.vue'),
      meta: {
        label: 'Auth 回调',
      },
    },
  ],
})

export default router
