import { defineConfig } from 'vite';
export default defineConfig({
  root: 'src',
  publicDir: 'public',
  build: {
    target: 'es2022',
    outDir: '../dist',
    emptyOutDir: true,
    rollupOptions: { input: { main: 'src/index.html', tray: 'src/tray-panel.html' } },
  },
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
});
