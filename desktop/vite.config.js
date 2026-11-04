import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The main window's chunk (ControlPanel) has a budget of its own, in kB as
// Vite reports them: 179 kB on 2026-11-01, once every view but Home and
// every Settings page became a chunk of its own, loaded the first time it
// shows and fetched when the window is idle (it was 778 kB, over the 725 kB
// warning, with Chat, Notes and Settings' pages in it). Past the budget the
// build warns, as Vite's own warning does (docs/architecture/frontend.md,
// "One chunk per window").
const MAIN_CHUNK_BUDGET_KB = 200;

function mainChunkBudget() {
  return {
    name: 'yap-main-chunk-budget',
    apply: 'build',
    writeBundle(_options, bundle) {
      for (const [file, chunk] of Object.entries(bundle)) {
        if (chunk.type !== 'chunk' || !/(^|\/)ControlPanel-[^/]*\.js$/.test(file)) continue;
        const kb = Buffer.byteLength(chunk.code) / 1000;
        if (kb > MAIN_CHUNK_BUDGET_KB) {
          this.warn(`the main window's chunk ${file} is ${kb.toFixed(1)} kB, over its ${MAIN_CHUNK_BUDGET_KB} kB budget (vite.config.js; frontend.md, "One chunk per window")`);
        }
      }
    },
  };
}

// Tauri expects a fixed dev port.
export default defineConfig({
  // The desktop app's folder (desktop/), wherever Vite is started from.
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [svelte(), mainChunkBudget()],
  clearScreen: false,
  server: {
    port: 51437,
    strictPort: true,
  },
  build: {
    // Every other chunk: the biggest is the page editor's (richEditor, TipTap
    // and ProseMirror, 435 kB), loaded the first time a page is edited. The
    // main window's chunk has its own, smaller budget above.
    chunkSizeWarningLimit: 450,
  },
});
