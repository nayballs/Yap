<script>
  // The dictation overlay, in the Yap bar's place while a dictation records,
  // transcribes or just failed: the red dot + scrolling amplitude waveform,
  // the live partial text when there is some, "Transcribing…", or the error.
  // Dark ink like the rest of the bar; the moving waveform and the pulsing
  // dot carry its visibility on any background.
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
</script>

<div class="capsule" class:err={dictation.state === 'error'}>
  {#if dictation.state === 'error'}
    <span class="dot errdot"></span>
    <span class="txt">{dictation.errorMsg}</span>
  {:else}
    <span class="dot" class:rec={!processing} class:proc={processing}></span>
    {#if dictation.words.length}
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
  {/if}
</div>

<style>
  .capsule {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 380px;
    height: 38px;
    box-sizing: border-box;
    padding: 0 16px 0 14px;
    border-radius: 999px;
    background: #1c1a16;
    border: 1px solid rgba(255, 255, 255, 0.14);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.28);
    color: rgba(255, 255, 255, 0.9);
    font-size: 12.5px;
  }
  .capsule.err {
    border-color: rgba(229, 100, 94, 0.7);
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
    background: #f0b04a; /* Yap amber, brightened for the dark capsule */
    transition: height 0.06s linear;
  }

  .txt {
    font-weight: 600;
    letter-spacing: 0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Live partial transcript: one clipped line reading like live captions.
     The text run glides left (transform transition) once it outgrows the
     box so the newest words stay visible, older ones fading out at the left
     edge; each newly revealed word fades in. */
  .partial {
    width: 236px;
    white-space: nowrap;
    overflow: hidden;
    color: rgba(255, 255, 255, 0.86);
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
