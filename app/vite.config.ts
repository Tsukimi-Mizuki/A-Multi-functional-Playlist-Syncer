import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// Tauri 开发服务器固定端口 1420，与 tauri.conf.json 的 devUrl 保持一致。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**', '**/target/**'] },
  },
  build: {
    target: 'chrome105',
    outDir: 'dist',
    emptyOutDir: true,
  },
})
