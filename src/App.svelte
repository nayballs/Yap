<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import ControlPanel from './lib/ControlPanel.svelte';
  import Onboarding from './lib/Onboarding.svelte';
  import Notepad from './lib/Notepad.svelte';
  import SplitPreview from './lib/SplitPreview.svelte';
  import Overlay from './lib/Overlay.svelte';

  // The settings, onboarding, notepad and overlay windows (and the notepad's
  // on-demand split preview) all load the same SPA; pick the rendered view
  // from the window label.
  const label = getCurrentWindow().label;
  const isSettings = label === 'settings';
  const isOnboarding = label === 'onboarding';
  const isNotepad = label === 'notepad';
  const isSplitPreview = label === 'split-preview';

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

{#if isSettings}
  <!-- The "settings" window label is historic — it now hosts the main
       control panel (Home feed + surfaces), with Settings as a modal. -->
  <ControlPanel />
{:else if isOnboarding}
  <Onboarding />
{:else if isNotepad}
  <!-- The meeting notepad, docked beside a call (notepad.rs). -->
  <Notepad />
{:else if isSplitPreview}
  <SplitPreview />
{:else}
  <Overlay />
{/if}
