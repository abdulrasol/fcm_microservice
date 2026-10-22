import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import path from 'path'

export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      // The admin API of the backend, with the session cookie untouched.
      '/api': {
        target: process.env.VITE_API_PROXY || 'http://localhost:8080',
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true
  }
})
