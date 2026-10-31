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
    // The main window's chunk (ControlPanel) is just under 720 kB (708 kB on
    // 2026-10-30 with Chat's documents, 712 on 2026-10-31 with a selection's
    // quote, the panel's two views and find across a text's pieces: the panel
    // itself loads on demand, its state, the quote and the pill don't; 716
    // the same day with each model's effort on the chip and in Settings — the
    // picker's own chunk has the rest), on
    // purpose: loading its views on demand was measured (2026-10-22) and
    // Home came up no sooner — V8
    // compiles code when it first runs, and the load's time is its IPC
    // calls (docs/architecture/frontend.md, "One chunk per window").
    chunkSizeWarningLimit: 720,
  },
});
