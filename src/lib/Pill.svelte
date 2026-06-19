<script>
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  // idle | recording | processing | needs-model
  let state = $state('idle');
  let level = $state(0);
  let downloading = $state(false);

  function applyScale(s) {
    document.documentElement.style.setProperty('--s', s ?? 1);
  }

  onMount(() => {
    const unlisteners = [];
    listen('blip-state', (e) => {
      state = e.payload;
      if (state !== 'recording') level = 0;
    }).then((u) => unlisteners.push(u));
    listen('blip-level', (e) => {
      level = e.payload;
    }).then((u) => unlisteners.push(u));
    listen('blip-scale', (e) => applyScale(e.payload)).then((u) => unlisteners.push(u));

    // Apply the saved pill size on load.
    invoke('get_config')
      .then((cfg) => applyScale(cfg?.pillScale))
      .catch(() => {});

    return () => unlisteners.forEach((u) => u && u());
  });

  function toggle() {
    invoke('toggle_recording');
  }

  function openSettings() {
    invoke('open_settings');
  }

  async function download() {
    downloading = true;
    try {
      await invoke('download_model');
    } finally {
      downloading = false;
    }
  }

  const label = $derived(
    state === 'recording'
      ? 'Listening…'
      : state === 'processing'
        ? 'Transcribing…'
        : state === 'needs-model'
          ? 'Model needed'
          : 'Blip',
  );

  const bars = $derived(
    Array.from({ length: 16 }, (_, i) => {
      const wobble = 0.4 + 0.6 * Math.abs(Math.sin((i + 1) * 1.7));
      return Math.max(0.06, Math.min(1, level * wobble));
    }),
  );
</script>

<div class="pill {state}" data-tauri-drag-region>
  <button
    class="dot"
    onclick={toggle}
    title="Toggle dictation (or press your hotkey)"
    aria-label="Toggle dictation"
  ></button>

  <div class="body" data-tauri-drag-region>
    {#if state === 'recording'}
      <div class="wave" data-tauri-drag-region>
        {#each bars as h}
          <span data-tauri-drag-region style="height:{Math.round(h * 100)}%"></span>
        {/each}
      </div>
    {:else}
      <span class="label" data-tauri-drag-region>{label}</span>
    {/if}

    {#if state === 'needs-model'}
      <button class="dl" onclick={download} disabled={downloading}>
        {downloading ? 'Downloading…' : 'Download'}
      </button>
    {/if}
  </div>

  <button class="gear" onclick={openSettings} title="Settings" aria-label="Settings">⚙</button>
</div>

<style>
  .pill {
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: calc(10px * var(--s, 1));
    width: 100vw;
    height: 100vh;
    padding: 0 calc(12px * var(--s, 1)) 0 calc(14px * var(--s, 1));
    border-radius: 999px;
    background: rgba(18, 20, 28, 0.92);
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(12px);
    font-size: calc(13px * var(--s, 1));
  }

  .dot {
    flex: 0 0 auto;
    width: calc(18px * var(--s, 1));
    height: calc(18px * var(--s, 1));
    border-radius: 50%;
    border: none;
    cursor: pointer;
    padding: 0;
    background: #4b5563;
    transition:
      background 0.2s ease,
      box-shadow 0.2s ease;
  }
  .pill.idle .dot {
    background: #3b82f6;
  }
  .pill.recording .dot {
    background: #ef4444;
    box-shadow: 0 0 0 calc(4px * var(--s, 1)) rgba(239, 68, 68, 0.25);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .pill.processing .dot {
    background: #f59e0b;
    animation: pulse 0.8s ease-in-out infinite;
  }
  .pill.needs-model .dot {
    background: #6b7280;
  }

  @keyframes pulse {
    0%,
    100% {
      transform: scale(1);
      opacity: 1;
    }
    50% {
      transform: scale(0.82);
      opacity: 0.7;
    }
  }

  .body {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-width: 0;
    color: #e5e7eb;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .wave {
    display: flex;
    align-items: center;
    gap: calc(2px * var(--s, 1));
    height: calc(22px * var(--s, 1));
    flex: 1 1 auto;
  }
  .wave span {
    flex: 1 1 auto;
    min-height: 2px;
    background: #ef4444;
    border-radius: 2px;
    transition: height 0.08s linear;
  }

  .dl {
    flex: 0 0 auto;
    margin-left: calc(8px * var(--s, 1));
    padding: calc(4px * var(--s, 1)) calc(10px * var(--s, 1));
    border-radius: 6px;
    border: none;
    cursor: pointer;
    background: #3b82f6;
    color: #fff;
    font-size: inherit;
  }
  .dl:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .gear {
    flex: 0 0 auto;
    width: calc(22px * var(--s, 1));
    height: calc(22px * var(--s, 1));
    border: none;
    background: transparent;
    color: #9ca3af;
    cursor: pointer;
    font-size: calc(14px * var(--s, 1));
    line-height: 1;
    padding: 0;
    border-radius: 6px;
    transition:
      color 0.15s ease,
      background 0.15s ease;
  }
  .gear:hover {
    color: #e5e7eb;
    background: rgba(255, 255, 255, 0.08);
  }
</style>
