import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    host: '0.0.0.0',
    port: 5177,
    proxy: {
      '/admin': process.env.VITE_CODEX_PROXY_TARGET ?? 'http://127.0.0.1:8787',
      '/healthz': process.env.VITE_CODEX_PROXY_TARGET ?? 'http://127.0.0.1:8787',
      '/readyz': process.env.VITE_CODEX_PROXY_TARGET ?? 'http://127.0.0.1:8787',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
