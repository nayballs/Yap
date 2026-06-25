<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Pill from './lib/Pill.svelte';
  import Settings from './lib/Settings.svelte';
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
  if (isSettings || isOnboarding) {
    document.documentElement.style.background = '#0f1117';
    document.body.style.background = '#0f1117';
  }
</script>

{#if isSettings}
  <Settings />
{:else if isOnboarding}
  <Onboarding />
{:else if isOverlay}
  <Overlay />
{:else}
  <Pill />
{/if}
