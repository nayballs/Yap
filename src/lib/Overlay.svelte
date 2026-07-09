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

  // ---- Live partial transcript (streaming preview) ----
  // The backend emits a full partial line every ~500 ms tick. Dumping a whole
  // tick's words at once reads chunky, so the overlay paces the reveal
  // word-by-word between ticks (typewriter feel, drained fast enough to stay
  // real-time), fades each new word in, and glides the clipped line left with
  // a transform instead of jumping.
  let words = $state([]); // revealed words (rendered as keyed spans)
  let queue = []; // words waiting to be revealed
  let revealTimer = null;

  function stopReveal() {
    if (revealTimer) {
      clearTimeout(revealTimer);
      revealTimer = null;
    }
  }
  function clearPartial() {
    stopReveal();
    queue = [];
    words = [];
  }
  function revealStep() {
    revealTimer = null;
    const w = queue.shift();
    if (w === undefined) return;
    words = [...words, w];
    if (queue.length) {
      // Drain the pending words within ~400 ms — comfortably before the next
      // backend tick — but never faster than 40 ms/word or slower than 110 ms.
      const pace = Math.max(40, Math.min(110, 400 / queue.length));
      revealTimer = setTimeout(revealStep, pace);
    }
  }
  function onPartial(text) {
    const tw = text.split(/\s+/).filter(Boolean);
    if (tw.length < words.length) {
      // The decode changed its mind and the line shrank (rare wholesale
      // replace) — snap; pacing a rewrite would just look laggy.
      clearPartial();
      words = tw;
      return;
    }
    // Words already on screen that changed (smart_diff tail rewrite) are wrong
    // — correct them in place immediately; only genuinely-new words get paced.
    let common = 0;
    while (common < words.length && words[common] === tw[common]) common++;
    if (common < words.length) words = tw.slice(0, words.length);
    queue = tw.slice(words.length);
    if (!revealTimer && queue.length) revealStep();
  }

  // Smooth left glide: keep the newest words visible by translating the text
  // run left whenever it outgrows the clip box (transition on the transform).
  let clipEl = $state(null);
  let textEl = $state(null);
  let shift = $state(0);
  $effect(() => {
    void words;
    if (clipEl && textEl) {
      shift = Math.min(0, clipEl.clientWidth - textEl.scrollWidth);
    }
  });

  onMount(() => {
    const unlisteners = [];
    listen('yap-state', (e) => {
      state = e.payload;
      if (state !== 'recording') history = [];
      // Keep the partial visible through the brief "processing" state, then drop
      // it once we're idle/needs-model/error so it never lingers.
      if (state === 'idle' || state === 'needs-model' || state === 'error') clearPartial();
      if (state === 'recording') clearPartial();
    }).then((u) => unlisteners.push(u));
    listen('yap-error', (e) => {
      if (e.payload) errorMsg = e.payload;
    }).then((u) => unlisteners.push(u));
    listen('yap-partial', (e) => {
      if (typeof e.payload === 'string') onPartial(e.payload);
    }).then((u) => unlisteners.push(u));
    listen('yap-amp', (e) => {
      const v = Math.min(1, Math.pow(Math.max(0, e.payload ?? 0) * AMP_GAIN, 0.7));
      const next = history.length >= MAX_BARS ? history.slice(1) : history.slice();
      next.push(v);
      history = next;
    }).then((u) => unlisteners.push(u));

    return () => {
      stopReveal();
      unlisteners.forEach((u) => u && u());
    };
  });
</script>

{#if state === 'recording' || state === 'processing' || state === 'processing-slow' || state === 'error'}
  <div class="overlay">
    <div class="capsule" class:err={state === 'error'}>
      {#if state === 'recording'}
        <span class="dot rec"></span>
        {#if words.length}
          <div class="partial" bind:this={clipEl}>
            <span class="ptext" bind:this={textEl} style="transform: translateX({shift}px)">
              {#each words as w, i (i)}<span class="pw">{w}&nbsp;</span>{/each}
            </span>
          </div>
        {:else}
          <div class="wave">
            {#each history as v}
              <span style="height:{Math.max(7, Math.round(v * 100))}%"></span>
            {/each}
          </div>
        {/if}
      {:else if state === 'processing' || state === 'processing-slow'}
        <span class="dot proc"></span>
        {#if words.length}
          <div class="partial" bind:this={clipEl}>
            <span class="ptext" bind:this={textEl} style="transform: translateX({shift}px)">
              {#each words as w, i (i)}<span class="pw">{w}&nbsp;</span>{/each}
            </span>
          </div>
        {:else}
          <span class="txt">{state === 'processing-slow' ? 'Transcribing (CPU — slow)…' : 'Transcribing…'}</span>
        {/if}
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

  /* Light capsule — matches the app's LIGHT identity (a little white Yap card
     floating on screen), not the dark hero card. White surface + warm border +
     ink text + the burnt-orange accent (the app's accent-on-light) carrying the
     waveform (2026-07-09: pill goes light to match the settings/main window).
     The red pulsing dot + moving waveform carry visibility on any background,
     so no drop shadow is needed — which also dodges the boxy-shadow artifact on
     the tightly-fitted transparent WebView2 window. */
  .capsule {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 6px 14px;
    border-radius: 999px;
    background: var(--yap-s2, #ffffff);
    border: 1px solid var(--yap-border-hover, #c8c2b3);
    color: var(--yap-fg, #23211b);
    font-size: 12px;
  }

  .dot {
    flex: 0 0 auto;
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .dot.rec {
    background: radial-gradient(circle at 35% 30%, #e5645e, var(--yap-danger, #c23b32));
    box-shadow: 0 0 7px color-mix(in srgb, var(--yap-danger, #c23b32) 45%, transparent);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot.proc {
    background: radial-gradient(circle at 35% 30%, #e39a3f, var(--yap-primary, #c2690a));
    box-shadow: 0 0 7px color-mix(in srgb, var(--yap-primary, #c2690a) 45%, transparent);
    animation: pulse 0.8s ease-in-out infinite;
  }
  .dot.errdot {
    background: radial-gradient(circle at 35% 30%, #e5645e, var(--yap-danger, #c23b32));
    box-shadow: 0 0 7px color-mix(in srgb, var(--yap-danger, #c23b32) 45%, transparent);
  }
  .capsule.err {
    border-color: var(--yap-danger, #c23b32);
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
    background: var(--yap-primary, #c2690a); /* burnt orange — the app's accent on light */
    border-radius: 1px;
    transition: height 0.06s linear;
  }

  .txt {
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--yap-fg, #23211b);
  }

  /* Live partial transcript: single clipped line reading like live captions.
     The text run glides left (transform transition) once it outgrows the box
     so the newest words stay visible, older ones fading out at the left edge;
     each newly revealed word fades in (word-paced by the script above). */
  .partial {
    width: 245px;
    white-space: nowrap;
    overflow: hidden;
    font-size: 12px;
    color: var(--yap-fg-80, rgba(35, 33, 27, 0.82));
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

  @keyframes fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
