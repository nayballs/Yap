<script>
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  // idle | recording | processing | needs-model | error
  let state = $state('idle');
  // Scrolling amplitude waveform (Claude Code style): newest bar on the right.
  const MAX_BARS = 80;
  const AMP_GAIN = 3.5;
  let history = $state([]);
  let errorMsg = $state('Transcription failed');

  onMount(() => {
    const unlisteners = [];
    listen('yap-state', (e) => {
      state = e.payload;
      if (state !== 'recording') history = [];
    }).then((u) => unlisteners.push(u));
    listen('yap-error', (e) => {
      if (e.payload) errorMsg = e.payload;
    }).then((u) => unlisteners.push(u));
    listen('yap-amp', (e) => {
      const v = Math.min(1, Math.pow(Math.max(0, e.payload ?? 0) * AMP_GAIN, 0.7));
      const next = history.length >= MAX_BARS ? history.slice(1) : history.slice();
      next.push(v);
      history = next;
    }).then((u) => unlisteners.push(u));

    return () => unlisteners.forEach((u) => u && u());
  });
</script>

{#if state === 'recording' || state === 'processing' || state === 'error'}
  <div class="overlay">
    <div class="capsule" class:err={state === 'error'}>
      {#if state === 'recording'}
        <span class="dot rec"></span>
        <div class="wave">
          {#each history as v}
            <span style="height:{Math.max(7, Math.round(v * 100))}%"></span>
          {/each}
        </div>
      {:else if state === 'processing'}
        <span class="dot proc"></span>
        <span class="txt">Transcribing…</span>
      {:else}
        <span class="dot errdot"></span>
        <span class="txt">{errorMsg}</span>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100vw;
    height: 100vh;
    pointer-events: none;
    /* gentle fade-in on mount */
    animation: fade-in 0.18s ease;
  }

  .capsule {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 6px 14px;
    border-radius: 999px;
    background: rgba(15, 17, 24, 0.92);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(14px);
    color: #e5e7eb;
    font-size: 12px;
  }

  .dot {
    flex: 0 0 auto;
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .dot.rec {
    background: radial-gradient(circle at 35% 30%, #f87171, #dc2626);
    box-shadow: 0 0 8px rgba(239, 68, 68, 0.6);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot.proc {
    background: radial-gradient(circle at 35% 30%, #fbbf24, #d97706);
    box-shadow: 0 0 8px rgba(245, 158, 11, 0.5);
    animation: pulse 0.8s ease-in-out infinite;
  }
  .dot.errdot {
    background: radial-gradient(circle at 35% 30%, #f87171, #dc2626);
    box-shadow: 0 0 8px rgba(239, 68, 68, 0.55);
  }
  .capsule.err {
    border-color: rgba(239, 68, 68, 0.45);
  }

  .wave {
    display: flex;
    align-items: center;
    justify-content: flex-end; /* newest bar on the right, older scroll left */
    gap: 1.5px;
    height: 18px;
    width: 245px;
    overflow: hidden;
  }
  .wave span {
    flex: 0 0 auto;
    width: 2px;
    min-height: 2px;
    background: #d1d5db; /* light grey, Claude-Code-style */
    border-radius: 1px;
    transition: height 0.06s linear;
  }

  .txt {
    font-weight: 600;
    letter-spacing: 0.02em;
    color: #f1f3f7;
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

  @keyframes fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
