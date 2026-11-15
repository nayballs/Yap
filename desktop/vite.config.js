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
        if (chunk.type !== 'chunk' || !/(^|\/)ControlPanel-[^\/]*\.js$/.test(file)) continue;
        const kb = Buffer.byteLength(chunk.code) / 1000;
        if (kb > MAIN_CHUNK_BUDGET_KB) {
          this.warn(`the main window's chunk ${file} is ${kb.toFixed(1)} kB, over its ${MAIN_CHUNK_BUDGET_KB} kB budget (vite.config.js; frontend.md, "One chunk per window")`);
        }
      }
    },
  };
}

// Every other chunk: 450 kB at most — the biggest of Yap's own is the page
// editor's (richEditor, TipTap and ProseMirror, 435 kB), loaded the first time
// a page is edited. The drawing libraries are exempt (15 Nov 2026): mermaid's
// chunks (its core, each kind of diagram, its layouts: elk is 1.5 MB) and
// KaTeX's load only when an answer or a note draws a diagram or maths
// (ui/markdownVisuals.js), never before, so Vite's own warning is set past
// them and this one holds Yap's own chunks to the budget.
const CHUNK_BUDGET_KB = 450;
const DRAWING_LIBRARIES = /[\\/]node_modules[\\/](mermaid|@mermaid-js|elkjs|cytoscape[^\\/]*|d3[^\\/]*|dagre-d3-es|khroma|katex|chevrotain[^\\/]*|@chevrotain[^\\/]*|langium|roughjs|@iconify[^\\/]*|lodash-es|marked|stylis|dayjs|@upsetjs|es-toolkit|@braintree|vscode-[^\\/]*)[\\/]/;

function chunkBudget() {
  return {
    name: 'yap-chunk-budget',
    apply: 'build',
    writeBundle(_options, bundle) {
      for (const [file, chunk] of Object.entries(bundle)) {
        if (chunk.type !== 'chunk') continue;
        const kb = Buffer.byteLength(chunk.code) / 1000;
        if (kb <= CHUNK_BUDGET_KB) continue;
        const parts = Object.entries(chunk.modules);
        const total = parts.reduce((n, [, m]) => n + m.renderedLength, 0) || 1;
        const drawing = parts.filter(([id]) => DRAWING_LIBRARIES.test(id)).reduce((n, [, m]) => n + m.renderedLength, 0);
        if (drawing / total >= 0.8) continue;
        this.warn(`the chunk ${file} is ${kb.toFixed(1)} kB, over the ${CHUNK_BUDGET_KB} kB budget (vite.config.js)`);
      }
    },
  };
}

// Tauri expects a fixed dev port.
export default defineConfig({
  // The desktop app's folder (desktop/), wherever Vite is started from.
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [svelte(), mainChunkBudget(), chunkBudget()],
  clearScreen: false,
  server: {
    port: 51437,
    strictPort: true,
  },
  build: {
    // (Past the drawing libraries' biggest chunk: `chunkBudget` above holds
    // every other chunk to 450 kB, and the main window's to its own budget.)
    chunkSizeWarningLimit: 1600,
  },
});
