<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import ContextMenuHost from './lib/ui/ContextMenuHost.svelte';
  import { initAppearance } from './lib/appearance.js';

  // The settings, onboarding, notepad and overlay windows (and the notepad's
  // on-demand split preview) all load the same SPA; pick the rendered view
  // from the window label. Each window loads only its own view's code (a
  // chunk of its own): the Yap bar no longer parses the whole app — Chat,
  // Notes, Settings — to draw a pill, and neither does the notepad.
  const label = getCurrentWindow().label;
  // One arrow per view, never a ternary of imports: Vite gives each import()
  // its own preload list (the view's CSS chunk included), and a ternary of
  // them got one list — the last one's — so the main window lost its CSS.
  const VIEWS = {
    settings: () => import('./lib/ControlPanel.svelte'),
    onboarding: () => import('./lib/Onboarding.svelte'),
    notepad: () => import('./lib/Notepad.svelte'),
    'split-preview': () => import('./lib/SplitPreview.svelte'),
    overlay: () => import('./lib/Overlay.svelte'),
  };
  const view = (VIEWS[label] ?? VIEWS.overlay)();

  // Settings → General → Appearance (the reading font and size, Motion) in
  // every window, following each save live (lib/appearance.js).
  initAppearance();
  const isSettings = label === 'settings';
  const isOnboarding = label === 'onboarding';
  const isNotepad = label === 'notepad';

  // The overlay and the split preview need a transparent body (app.css). The
  // settings, onboarding and notepad windows are opaque, so override that
  // here or they show OS white. They also set an explicit `color-scheme` (for
  // native scrollbars/controls) — deliberately NOT global, because on a
  // transparent window it makes the WebView paint an opaque backdrop (grey
  // box bug). All three are warm-light (2026-07-09); the notepad uses Wispr
  // Flow's paper (`--yap-paper`).
  if (isSettings || isOnboarding) {
    document.documentElement.style.colorScheme = 'light';
    document.documentElement.style.background = '#f0ede7';
    document.body.style.background = '#f0ede7';
  } else if (isNotepad) {
    document.documentElement.style.colorScheme = 'light';
    document.documentElement.style.background = '#fcfcfb';
    document.body.style.background = '#fcfcfb';
  }
</script>

<!-- settings → the main control panel (Home feed + surfaces, Settings as a
     modal; the label is historic), onboarding, notepad → the meeting notepad
     docked beside a call (notepad.rs), split-preview, else the Yap bar. -->
{#await view then { default: View }}
  <View />
{/await}

{#if isSettings || isOnboarding || isNotepad}
  <!-- The window's one right-click menu and confirm dialog: every item with
       actions opens them (ui/contextMenu.svelte.js). -->
  <ContextMenuHost />
{/if}
