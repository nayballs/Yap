<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Pill from './lib/Pill.svelte';
  import Settings from './lib/Settings.svelte';

  // The pill window and the settings window load the same SPA;
  // pick the view from the window label.
  const isSettings = getCurrentWindow().label === 'settings';

  // The pill window needs a transparent body (app.css). The settings window
  // is an opaque window, so override that here or it shows the OS white.
  if (isSettings) {
    document.documentElement.style.background = '#0f1117';
    document.body.style.background = '#0f1117';
  }
</script>

{#if isSettings}
  <Settings />
{:else}
  <Pill />
{/if}
