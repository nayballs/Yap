import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 51437,
    strictPort: true,
  },
  build: {
    // The main window's chunk is ~590 KB, on purpose: loading its views on
    // demand was measured (2026-10-22) and Home came up no sooner — V8
    // compiles code when it first runs, and the load's time is its IPC
    // calls (docs/architecture/frontend.md, "One chunk per window").
    chunkSizeWarningLimit: 700,
  },
});
