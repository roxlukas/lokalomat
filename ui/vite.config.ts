/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port and no screen clearing (keeps Rust errors visible).
export default defineConfig({
  plugins: [svelte()],
  resolve: { alias: { '@edition': '/src/edition/free.ts' } },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    // the browser-only mock reads ../catalog/models.json
    fs: { allow: ['..'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2022',
    sourcemap: false,
    // index.html = desktop app, mobile.html = Android client (apps/mobile, M5)
    rollupOptions: {
      input: { main: 'index.html', mobile: 'mobile.html' },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
