<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import ControlPanel from './lib/ControlPanel.svelte';
  import Onboarding from './lib/Onboarding.svelte';
  import Overlay from './lib/Overlay.svelte';

  // The settings, onboarding and overlay windows all load the same SPA;
  // pick the rendered view from the window label.
  const label = getCurrentWindow().label;
  const isSettings = label === 'settings';
  const isOnboarding = label === 'onboarding';

  // The overlay window needs a transparent body (app.css). The settings and
  // onboarding windows are opaque, so override that here or they show OS white.
  // They also set an explicit `color-scheme` (for native scrollbars/controls) —
  // deliberately NOT global, because on the transparent overlay window it
  // makes the WebView paint an opaque backdrop (grey box bug).
  // Settings (the ControlPanel) AND onboarding are warm-light (2026-07-09).
  if (isSettings || isOnboarding) {
    document.documentElement.style.colorScheme = 'light';
    document.documentElement.style.background = '#f0ede7';
    document.body.style.background = '#f0ede7';
  }
</script>

{#if isSettings}
  <!-- The "settings" window label is historic — it now hosts the main
       control panel (Home feed + surfaces), with Settings as a modal. -->
  <ControlPanel />
{:else if isOnboarding}
  <Onboarding />
{:else}
  <Overlay />
{/if}
