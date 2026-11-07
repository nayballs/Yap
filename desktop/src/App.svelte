<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import ContextMenuHost from './lib/ui/ContextMenuHost.svelte';
  import { initAppearance } from './lib/core/appearance.js';
  import { prefetchHome } from './lib/views/home/homeData.js';

  // The settings, onboarding, notepad, quick-note and overlay windows (and the
  // notepad's on-demand split preview, and each Markdown file's window,
  // `doc-…`) all load the same SPA; pick the rendered view
  // from the window label. Each window loads only its own view's code (a
  // chunk of its own): the Yap bar no longer parses the whole app — Chat,
  // Notes, Settings — to draw a pill, and neither does the notepad.
  const label = getCurrentWindow().label;
  // The main window: Home's data asked for now, while its view's code loads
  // (homeData.js), so Home can draw as soon as it mounts. The view mounts
  // once that answer is in (or after 300 ms at most), so its own start-up —
  // ~60 calls, 50 of them listeners — never goes ahead of Home's: since
  // 2026-11-01 its chunk is small enough to load before the answer comes
  // (frontend.md, "One chunk per window"), where it used to take longer.
  const homeFirst =
    label === 'settings' ? Promise.race([prefetchHome().catch(() => {}), new Promise((done) => setTimeout(done, 300))]) : null;
  // One arrow per view, never a ternary of imports: Vite gives each import()
  // its own preload list (the view's CSS chunk included), and a ternary of
  // them got one list — the last one's — so the main window lost its CSS.
  const VIEWS = {
    settings: () => import('./lib/views/ControlPanel.svelte'),
    onboarding: () => import('./lib/onboarding/Onboarding.svelte'),
    notepad: () => import('./lib/notepad/Notepad.svelte'),
    'split-preview': () => import('./lib/notepad/SplitPreview.svelte'),
    'quick-note': () => import('./lib/quicknote/QuickNote.svelte'),
    overlay: () => import('./lib/bar/Overlay.svelte'),
    // A Markdown file's window, one per file (`doc-<hash>`, notes/md_file.rs).
    doc: () => import('./lib/doc/DocWindow.svelte'),
  };
  // The kind of window: a Markdown file's are all "doc".
  const kind = label.startsWith('doc-') ? 'doc' : label;
  const view = homeFirst ? Promise.all([VIEWS.settings(), homeFirst]).then(([v]) => v) : (VIEWS[kind] ?? VIEWS.overlay)();

  // Settings → General → Appearance (the theme, the reading font and size,
  // Motion) and the bar's colours in every window, following each save live
  // (lib/core/appearance.js).
  initAppearance();
  const isSettings = label === 'settings';
  const isOnboarding = label === 'onboarding';
  const isNotepad = label === 'notepad';
  // (The quick note's too, since its page has a toolbar with menus.)
  const isQuickNote = label === 'quick-note';
  // (A Markdown file's too: its toolbar has menus, and closing asks first.)
  const isDoc = kind === 'doc';

  // The window's kind on <html> (index.html's head script already put it
  // there before the first paint; this is the fallback). app.css gives the
  // opaque windows — settings, onboarding, notepad — their background and
  // `color-scheme` per theme from it; the overlay and the split preview
  // keep a transparent body and no `color-scheme` (on a transparent window
  // it makes the WebView paint an opaque backdrop: the grey box bug).
  if (document.documentElement.dataset.window !== kind) document.documentElement.dataset.window = kind;
</script>

<!-- settings → the main control panel (Home feed + surfaces, Settings as a
     modal; the label is historic), onboarding, notepad → the meeting notepad
     docked beside a call (meetings/notepad.rs), split-preview, quick-note → the
     quick note (notes/quick_note.rs), else the Yap bar. -->
{#await view then { default: View }}
  <View />
{/await}

{#if isSettings || isOnboarding || isNotepad || isQuickNote || isDoc}
  <!-- The window's one right-click menu and confirm dialog: every item with
       actions opens them (ui/contextMenu.svelte.js). -->
  <ContextMenuHost />
{/if}
