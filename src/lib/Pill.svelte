<script>
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  // idle | recording | processing | needs-model
  let state = $state('idle');
  // Scrolling amplitude waveform (Claude Code style): each bar is the voice
  // loudness at a moment; newest appears on the right, older scroll off the left.
  const MAX_BARS = 80;
  const AMP_GAIN = 3.5;
  let history = $state([]);
  let downloading = $state(false);

  function applyScale(s) {
    document.documentElement.style.setProperty('--s', s ?? 1);
  }

  onMount(() => {
    const unlisteners = [];
    listen('blip-state', (e) => {
      state = e.payload;
      if (state !== 'recording') history = []; // start empty next time
    }).then((u) => unlisteners.push(u));
    listen('blip-amp', (e) => {
      // Shape the raw peak (gain + perceptual curve) and append as a new bar,
      // scrolling older bars off the left once full.
      const v = Math.min(1, Math.pow(Math.max(0, e.payload ?? 0) * AMP_GAIN, 0.7));
      const next = history.length >= MAX_BARS ? history.slice(1) : history.slice();
      next.push(v);
      history = next;
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

  function cancel() {
    invoke('cancel_recording');
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
        {#each history as v}
          <span data-tauri-drag-region style="height:{Math.max(7, Math.round(v * 100))}%"></span>
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

  {#if state === 'recording'}
    <button class="cancel" onclick={cancel} title="Cancel (discard)" aria-label="Cancel recording">
      <svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
        <path d="M6 6l12 12M18 6L6 18" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
      </svg>
    </button>
  {/if}

  <button class="gear" onclick={openSettings} title="Settings" aria-label="Settings">
    <svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <circle cx="12" cy="12" r="3.2" stroke="currentColor" stroke-width="1.7" />
      <path
        d="M12 2.5l1.1 2.2 2.4-.5.6 2.4 2.4.6-.5 2.4 2.2 1.1-2.2 1.1.5 2.4-2.4.6-.6 2.4-2.4-.5L12 21.5l-1.1-2.2-2.4.5-.6-2.4-2.4-.6.5-2.4L3.3 13l2.2-1.1-.5-2.4 2.4-.6.6-2.4 2.4.5L12 2.5z"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linejoin="round"
      />
    </svg>
  </button>
</div>

<style>
  .pill {
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: calc(11px * var(--s, 1));
    width: 100vw;
    height: 100vh;
    padding: 0 calc(14px * var(--s, 1)) 0 calc(15px * var(--s, 1));
    border-radius: 999px;
    background: linear-gradient(180deg, rgba(30, 33, 44, 0.94), rgba(15, 17, 24, 0.94));
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow:
      0 8px 28px rgba(0, 0, 0, 0.5),
      inset 0 1px 0 rgba(255, 255, 255, 0.06);
    backdrop-filter: blur(14px);
    font-size: calc(13px * var(--s, 1));
  }

  .dot {
    flex: 0 0 auto;
    width: calc(16px * var(--s, 1));
    height: calc(16px * var(--s, 1));
    border-radius: 50%;
    border: none;
    cursor: pointer;
    padding: 0;
    background: radial-gradient(circle at 35% 30%, #6b7280, #4b5563);
    transition:
      background 0.2s ease,
      box-shadow 0.25s ease,
      transform 0.15s ease;
  }
  .dot:hover {
    transform: scale(1.08);
  }
  .pill.idle .dot {
    background: radial-gradient(circle at 35% 30%, #60a5fa, #2563eb);
    box-shadow:
      0 0 0 calc(3px * var(--s, 1)) rgba(59, 130, 246, 0.2),
      0 0 calc(10px * var(--s, 1)) rgba(59, 130, 246, 0.35);
  }
  .pill.recording .dot {
    background: radial-gradient(circle at 35% 30%, #f87171, #dc2626);
    box-shadow:
      0 0 0 calc(4px * var(--s, 1)) rgba(239, 68, 68, 0.22),
      0 0 calc(12px * var(--s, 1)) rgba(239, 68, 68, 0.5);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .pill.processing .dot {
    background: radial-gradient(circle at 35% 30%, #fbbf24, #d97706);
    box-shadow: 0 0 calc(12px * var(--s, 1)) rgba(245, 158, 11, 0.45);
    animation: pulse 0.8s ease-in-out infinite;
  }
  .pill.needs-model .dot {
    background: radial-gradient(circle at 35% 30%, #9ca3af, #6b7280);
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
    font-weight: 600;
    letter-spacing: 0.02em;
    color: #f1f3f7;
  }

  .wave {
    display: flex;
    align-items: center;
    justify-content: flex-end; /* newest bar hugs the right, older scroll left */
    gap: calc(1.5px * var(--s, 1));
    height: calc(22px * var(--s, 1));
    flex: 1 1 auto;
    overflow: hidden;
  }
  .wave span {
    flex: 0 0 auto;
    width: calc(2px * var(--s, 1));
    min-height: calc(2px * var(--s, 1));
    background: #d1d5db; /* light grey, Claude-Code-style */
    border-radius: 1px;
    transition: height 0.06s linear;
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

  .cancel {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    width: calc(22px * var(--s, 1));
    height: calc(22px * var(--s, 1));
    border: none;
    background: transparent;
    color: #9ca3af;
    cursor: pointer;
    padding: 0;
    border-radius: 50%;
    transition:
      color 0.18s ease,
      background 0.18s ease;
  }
  .cancel svg {
    width: calc(14px * var(--s, 1));
    height: calc(14px * var(--s, 1));
    display: block;
  }
  .cancel:hover {
    color: #f87171;
    background: rgba(239, 68, 68, 0.14);
  }

  .gear {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    width: calc(26px * var(--s, 1));
    height: calc(26px * var(--s, 1));
    border: none;
    background: transparent;
    color: #9ca3af;
    cursor: pointer;
    padding: 0;
    border-radius: 50%;
    transition:
      color 0.18s ease,
      background 0.18s ease,
      transform 0.3s ease;
  }
  .gear svg {
    width: calc(16px * var(--s, 1));
    height: calc(16px * var(--s, 1));
    display: block;
  }
  .gear:hover {
    color: #f1f3f7;
    background: rgba(255, 255, 255, 0.1);
    transform: rotate(40deg);
  }
</style>
