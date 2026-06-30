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
});
