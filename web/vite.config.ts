import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The version the installer and the window carry, read from the one file that
// says it rather than written out a fourth time. See docs/rules/versioning.md.
const { version } = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
) as { version: string }

// Tauri needs a fixed port and must fail rather than silently pick another one:
// a debug build loads devUrl, and a wrong port is the blank window of
// docs/rules/done.md.
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  define: { __APP_VERSION__: JSON.stringify(version) },
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
    // design/tokens.css sits outside this package on purpose: it is the design
    // system's file, not the frontend's, and CI checks it where it lives.
    fs: { allow: [fileURLToPath(new URL('..', import.meta.url))] },
  },
  build: {
    // Tauri 2 targets WebView2 and WKWebView; both support ES2022.
    target: 'es2022',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'esbuild',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
})
