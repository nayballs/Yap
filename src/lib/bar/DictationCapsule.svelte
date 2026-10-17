<script>
  // The dictation overlay, in the Yap bar's place while a dictation records,
  // transcribes or just failed: the red dot + scrolling amplitude waveform,
  // the live partial text when there is some, "Transcribing…", or the error.
  // Dark ink like the rest of the bar; the moving waveform and the pulsing
  // dot carry its visibility on any background. A lock after the dot: push-
  // to-talk's double-tap made it hands-free. "No sound from <mic>…" in the
  // waveform's place: the dead-mic watchdog heard nothing but digital
  // silence (the recording carries on).
  //
  // While it records: ✕ (cancel) on the left and ✓ (stop and paste, as the
  // hotkey's second press) on the right. Cancelled: "Dictation cancelled"
  // with Undo while Rust keeps the audio (pipeline.rs, `UNDO_MS`; a hairline
  // runs down meanwhile) — Undo transcribes it and pastes it into the app
  // the dictation started in. The capsule is a `data-region` then, so the
  // click-through bar takes clicks on it (bar.rs); it's never focused, so
  // the app you were in keeps the keyboard.
  import { invoke } from '@tauri-apps/api/core';
  import { dictation } from './dictation.svelte.js';

  // Smooth left glide: keep the newest words visible by translating the text
  // run left whenever it outgrows the clip box (transition on the transform).
  let clipEl = $state(null);
  let textEl = $state(null);
  let shift = $state(0);
  $effect(() => {
    void dictation.words;
    if (clipEl && textEl) {
      shift = Math.min(0, clipEl.clientWidth - textEl.scrollWidth);
    }
  });

  const processing = $derived(dictation.state === 'processing' || dictation.state === 'processing-slow');
  const recording = $derived(dictation.state === 'recording');
  const cancelled = $derived(dictation.state === 'cancelled');

  // Every button goes through `bar_action` (Rust gives the focus back to the
  // app you're in, should the click have activated the bar).
  function act(action) {
    invoke('bar_action', { action }).catch((e) =>
      invoke('frontend_log', { msg: `bar: ${action} failed: ${e}` }).catch(() => {})
    );
  }
</script>

<div
  class="capsule"
  class:err={dictation.state === 'error' || (!!dictation.micSilent && !cancelled)}
  class:rec={recording}
  class:cancelled
  data-region={recording || cancelled ? 'dictation' : undefined}
>
  {#if cancelled}
    <span class="txt">Dictation cancelled</span>
    <button class="undo" onclick={() => act('dictation-undo')}>Undo</button>
    <span class="undoline" aria-hidden="true"><span style={`animation-duration:${dictation.undoMs}ms`}></span></span>
  {:else if dictation.state === 'error'}
    <span class="dot errdot"></span>
    <span class="txt">{dictation.errorMsg}</span>
  {:else}
    {#if recording}
      <button class="cbtn cancel" aria-label="Cancel dictation" title="Cancel" onclick={() => act('dictation-cancel')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
      </button>
    {/if}
    <span class="dot" class:rec={!processing} class:proc={processing}></span>
    {#if dictation.locked && !processing}
      <span class="lock" role="img" aria-label="Hands-free: press the hotkey to stop" title="Hands-free: press the hotkey to stop">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7.5a4 4 0 0 1 8 0V11" /></svg>
      </span>
    {/if}
    {#if dictation.micSilent && !processing}
      <span class="txt warn" role="alert" title={dictation.micSilentFull}>{dictation.micSilent}</span>
    {:else if dictation.words.length}
      <div class="partial" bind:this={clipEl}>
        <span class="ptext" bind:this={textEl} style="transform: translateX({shift}px)">
          {#each dictation.words as w, i (i)}<span class="pw">{w}&nbsp;</span>{/each}
        </span>
      </div>
    {:else if processing}
      <span class="txt">{dictation.state === 'processing-slow' ? 'Transcribing (CPU — slow)…' : 'Transcribing…'}</span>
    {:else}
      <div class="wave">
        {#each dictation.history as v}
          <span style="height:{Math.max(7, Math.round(v * 100))}%"></span>
        {/each}
      </div>
    {/if}
    {#if recording}
      <button class="cbtn done" aria-label="Stop and paste" title="Stop and paste" onclick={() => act('dictation-stop')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5.5 12.5l4 4 9-9" /></svg>
      </button>
    {/if}
  {/if}
</div>

<style>
  .capsule {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 380px;
    height: 38px;
    box-sizing: border-box;
    padding: 0 16px 0 14px;
    border-radius: 999px;
    overflow: hidden;
    /* The bar's black with the cards' hairline; no shadow: the capsule sits
       14 px above the window's edge, which a shadow would reach (a grey box
       on a transparent WebView2 window). */
    background: var(--bar-surface);
    border: 1px solid var(--bar-border);
    color: var(--bar-text);
    font-size: 12.5px;
  }
  /* Recording: the ✕ and ✓ sit in the capsule's ends. */
  .capsule.rec {
    padding: 0 5px;
    gap: 8px;
  }
  .capsule.cancelled {
    padding: 0 5px 0 16px;
    gap: 12px;
  }
  .capsule.err {
    border-color: rgba(229, 100, 94, 0.7);
  }

  /* ✕ and ✓: 26 px circles; ✕ quiet, ✓ the bar's primary (ink on light,
     cream on dark). */
  .cbtn {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .cbtn svg {
    width: 14px;
    height: 14px;
  }
  .cbtn.cancel {
    background: var(--bar-hover);
    color: var(--bar-text-soft);
  }
  .cbtn.cancel:hover {
    background: var(--bar-active);
    color: var(--bar-text);
  }
  .cbtn.done {
    background: var(--bar-button);
    color: var(--bar-button-text);
  }
  .cbtn.done:hover {
    background: var(--bar-button-hover);
  }

  /* "Dictation cancelled · Undo", and the time left as a hairline. */
  .undo {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 12px;
    border: none;
    border-radius: 13px;
    background: var(--bar-button);
    color: var(--bar-button-text);
    font: inherit;
    font-size: 12.5px;
    font-weight: 650;
    cursor: pointer;
  }
  .undo:hover {
    background: var(--bar-button-hover);
  }
  .undoline {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: 3px;
    height: 2px;
    border-radius: 1px;
    background: var(--bar-track);
    overflow: hidden;
  }
  .undoline span {
    display: block;
    height: 100%;
    background: var(--bar-text-faint);
    transform-origin: left;
    animation: run-down linear forwards;
  }
  @keyframes run-down {
    from {
      transform: scaleX(1);
    }
    to {
      transform: scaleX(0);
    }
  }

  .dot {
    flex: 0 0 auto;
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .dot.rec {
    background: radial-gradient(circle at 35% 30%, #f0817b, #d9443b);
    box-shadow: 0 0 8px rgba(229, 100, 94, 0.55);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot.proc {
    background: radial-gradient(circle at 35% 30%, #f6c77a, #e2982a);
    box-shadow: 0 0 8px rgba(240, 176, 74, 0.5);
    animation: pulse 0.8s ease-in-out infinite;
  }
  .dot.errdot {
    background: radial-gradient(circle at 35% 30%, #f0817b, #d9443b);
  }

  .wave {
    display: flex;
    align-items: center;
    justify-content: flex-end; /* newest bar on the right, older scroll left */
    gap: 1.5px;
    height: 18px;
    width: 236px;
    overflow: hidden;
  }
  .wave span {
    flex: 0 0 auto;
    width: 2px;
    min-height: 2px;
    border-radius: 1px;
    background: var(--bar-wave); /* Yap amber, brightened for the dark capsule */
    transition: height 0.06s linear;
  }

  /* Hands-free: a small lock in the bar's amber, after the dot. */
  .lock {
    display: flex;
    flex: 0 0 auto;
    color: var(--bar-wave);
  }
  .lock svg {
    width: 13px;
    height: 13px;
  }

  .txt {
    font-weight: 600;
    letter-spacing: 0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* The dead-mic warning: one line, cut with "…" if a device's name is
     long (the whole message is its tooltip and the main window's toast). */
  .txt.warn {
    max-width: 236px;
    font-weight: 550;
  }

  /* Live partial transcript: one clipped line reading like live captions.
     The text run glides left (transform transition) once it outgrows the
     box so the newest words stay visible, older ones fading out at the left
     edge; each newly revealed word fades in. */
  .partial {
    width: 236px;
    white-space: nowrap;
    overflow: hidden;
    color: var(--bar-text-soft);
    mask-image: linear-gradient(90deg, transparent 0, #000 16px);
    -webkit-mask-image: linear-gradient(90deg, transparent 0, #000 16px);
  }
  .ptext {
    display: inline-block;
    transition: transform 0.35s ease-out;
    will-change: transform;
  }
  .pw {
    display: inline-block;
    animation: word-in 0.22s ease-out;
  }
  @keyframes word-in {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
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
</style>
