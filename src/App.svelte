<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Pill from './lib/Pill.svelte';
  import Settings from './lib/Settings.svelte';

  // The pill window and the settings window load the same SPA;
  // pick the view from the window label.
  const isSettings = getCurrentWindow().label === 'settings';
</script>

{#if isSettings}
  <Settings />
{:else}
  <Pill />
{/if}
