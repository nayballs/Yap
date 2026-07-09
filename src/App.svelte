<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Pill from './lib/Pill.svelte';
  import ControlPanel from './lib/ControlPanel.svelte';
  import Onboarding from './lib/Onboarding.svelte';
  import Overlay from './lib/Overlay.svelte';

  // The pill, settings, onboarding and overlay windows all load the same SPA;
  // pick the rendered view from the window label.
  const label = getCurrentWindow().label;
  const isSettings = label === 'settings';
  const isOnboarding = label === 'onboarding';
  const isOverlay = label === 'overlay';

  // The pill window needs a transparent body (app.css). The settings and
  // onboarding windows are opaque, so override that here or they show OS white.
  // They also set an explicit `color-scheme` (for native scrollbars/controls) —
  // deliberately NOT global, because on the transparent pill/overlay windows it
  // makes the WebView paint an opaque backdrop (grey box bug).
  // Settings (the ControlPanel) is warm-light; onboarding keeps its own dark
  // styling for now (self-contained hardcoded palette, restyle deferred).
  if (isSettings) {
    document.documentElement.style.colorScheme = 'light';
    document.documentElement.style.background = '#f0ede7';
    document.body.style.background = '#f0ede7';
  } else if (isOnboarding) {
    document.documentElement.style.colorScheme = 'dark';
    document.documentElement.dataset.yapTheme = 'dark'; // scoped dark tokens (app.css)
    document.documentElement.style.background = '#0f1117';
    document.body.style.background = '#0f1117';
  }
</script>

{#if isSettings}
  <!-- The "settings" window label is historic — it now hosts the main
       control panel (Home feed + surfaces), with Settings as a modal. -->
  <ControlPanel />
{:else if isOnboarding}
  <Onboarding />
{:else if isOverlay}
  <Overlay />
{:else}
  <Pill />
{/if}
