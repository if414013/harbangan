import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  base: '/_ui/',
  server: {
    port: 5173,
    proxy: {
      '/_ui/api': {
        target: 'http://backend:9999',
        changeOrigin: true,
        headers: {
          'X-Forwarded-Host': 'localhost:5173',
          'X-Forwarded-Proto': 'http',
        },
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
