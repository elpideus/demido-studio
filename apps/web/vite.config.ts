import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'

const path = (p: string) => fileURLToPath(new URL(p, import.meta.url))

// The same build runs in three places: inside the desktop shell, served by a Demido server, and as static
// files for the interactive preview. Relative base + hash routing keep it working from any folder or host.
export default defineConfig({
  base: './',
  plugins: [react()],
  resolve: {
    alias: {
      '@': path('./src'),
      '@brand': path('../../brand'),
    },
  },
  server: { host: '127.0.0.1', port: 5173, strictPort: true, fs: { allow: [path('../..')] } },
  preview: { host: '127.0.0.1', port: 5174, strictPort: true },
  build: { outDir: 'dist', sourcemap: true, chunkSizeWarningLimit: 900 },
})
