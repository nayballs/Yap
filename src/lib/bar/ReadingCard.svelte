<script>
  // The reading card above the Yap bar (read_aloud.rs): the sentence being
  // read aloud, its current word lit up as it's said (karaoke), and the
  // controls — ⏮ back a sentence (or to this one's start), ⏸ / ▶, ⏭ on a
  // sentence, the speed (− 1.25× +), Stop. Esc stops too (bar.rs watches it
  // while the card is up; the bar never takes the focus). The text is in the
  // reading font and size from Settings → General → Appearance
  // (--yap-font-reading, OpenDyslexic included) with roomy lines; colours are
  // the bar's (--bar-*, Light and Dark). With nothing to read, the card is a
  // short note ("Nothing selected") for a few seconds.
  //
  // The whole card is a `data-region` (Rust makes the click-through window
  // take clicks only over it); no backdrop-filter, a 1–2 px shadow, as every
  // card on the transparent bar window.
  import { reading, readAction } from './reading.svelte.js';
  import { fillHotkey } from '../core/hotkeys.js';

  let textEl = $state(null);

  // Keep the word being said in view (a long sentence in a large font scrolls).
  $effect(() => {
    const w = reading.word;
    if (w == null || !textEl) return;
    const el = textEl.querySelector(`[data-w="${w}"]`);
    if (!el) return;
    const box = textEl.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    if (r.top < box.top || r.bottom > box.bottom) {
      textEl.scrollTop += r.top - box.top - box.height / 3;
    }
  });
  // A new sentence starts at its top.
  $effect(() => {
    reading.index;
    reading.session;
    if (textEl) textEl.scrollTop = 0;
  });

  const paused = $derived(reading.phase === 'paused');
  const speed = $derived(`${Number(reading.rate || 1).toFixed(2).replace(/\.?0+$/, '')}×`);
  const progress = $derived(reading.count > 0 ? ((reading.index + 1) / reading.count) * 100 : 0);
  const TIMING = {
    engine: 'Word timing from the voice itself',
    audio: 'Word timing worked out from the audio',
    estimate: 'Word timing estimated from the sentence',
  };
  const noteBody = $derived(reading.note ? fillHotkey(reading.note.body, reading.hotkey) : '');
</script>

<div
  class="card"
  class:note={reading.phase === 'note'}
  data-region="reading"
  role="region"
  aria-label="Reading aloud"
  data-phase={reading.phase}
  data-timing={reading.timing ?? ''}
>
  {#if reading.phase === 'note' && reading.note}
    <button class="close" aria-label="Close" onclick={() => readAction('dismiss')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
    <span class="chip" class:error={reading.note.kind === 'error'}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z" /><path d="M15.5 9a4 4 0 0 1 0 6" /></svg>
      Read aloud
    </span>
    <div class="title" role="status">{reading.note.title}</div>
    {#if noteBody}<div class="body">{noteBody}</div>{/if}
  {:else}
    <div class="head">
      <span class="chip" title={TIMING[reading.timing] ?? ''}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z" /><path d="M15.5 9a4 4 0 0 1 0 6" />{#if !paused}<path d="M18.5 6.5a7.5 7.5 0 0 1 0 11" />{/if}</svg>
        {paused ? 'Paused' : 'Reading aloud'}
      </span>
      {#if reading.count > 1}<span class="count">{reading.index + 1} of {reading.count}</span>{/if}
      <button class="close" aria-label="Stop reading" title="Stop reading (Esc)" onclick={() => readAction('stop')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
    <p class="text" bind:this={textEl} aria-live="off">
      {#each reading.parts as part, i (`${reading.session}:${reading.index}:${i}`)}{#if part.w != null}<span
            class="w"
            class:now={part.w === reading.word}
            data-w={part.w}>{part.text}</span
          >{:else}{part.text}{/if}{/each}
    </p>
    <div class="controls">
      <button class="ctl" aria-label="Back a sentence" title="Back a sentence" onclick={() => readAction('back')}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M6 5h2v14H6z" /><path d="M19 5.8v12.4a.8.8 0 0 1-1.2.7L9.6 12.7a.8.8 0 0 1 0-1.4l8.2-6.2a.8.8 0 0 1 1.2.7z" /></svg>
      </button>
      <button class="ctl play" aria-label={paused ? 'Resume' : 'Pause'} aria-pressed={paused} onclick={() => readAction('toggle')}>
        {#if paused}
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5.6v12.8a.9.9 0 0 0 1.4.8l10-6.4a.9.9 0 0 0 0-1.6l-10-6.4A.9.9 0 0 0 8 5.6z" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6.5" y="5" width="4" height="14" rx="1.2" /><rect x="13.5" y="5" width="4" height="14" rx="1.2" /></svg>
        {/if}
      </button>
      <button class="ctl" aria-label="Next sentence" title="Next sentence" onclick={() => readAction('next')}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M16 5h2v14h-2z" /><path d="M5 5.8v12.4a.8.8 0 0 0 1.2.7l8.2-6.2a.8.8 0 0 0 0-1.4L6.2 5.1A.8.8 0 0 0 5 5.8z" /></svg>
      </button>
      <span class="speed" role="group" aria-label="Speed">
        <button class="step" aria-label="Slower" title="Slower" disabled={reading.rate <= 0.5} onclick={() => readAction('slower')}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 12h12" /></svg>
        </button>
        <span class="rate" aria-live="polite">{speed}</span>
        <button class="step" aria-label="Faster" title="Faster" disabled={reading.rate >= 2} onclick={() => readAction('faster')}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 12h12M12 6v12" /></svg>
        </button>
      </span>
      <span class="grow"></span>
      <button class="stop" onclick={() => readAction('stop')}>
        <span class="square" aria-hidden="true"></span>
        Stop
      </button>
    </div>
    {#if reading.cut}
      <div class="cut">Reading the first 200,000 characters</div>
    {/if}
    <div class="progress" aria-hidden="true"><div style={`width:${progress}%`}></div></div>
  {/if}
</div>

<style>
  /* A card above the bar, as BarCard (400 wide in Wispr's measure; the
     reading card a little wider for its text), the bar's tokens. */
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    width: 440px;
    max-width: calc(100vw - 24px);
    box-sizing: border-box;
    padding: 16px 18px 18px;
    border-radius: 16px;
    overflow: hidden;
    background: var(--bar-surface);
    border: 1px solid var(--bar-border);
    box-shadow: var(--bar-shadow);
    color: var(--bar-text);
    pointer-events: auto;
  }
  .card.note {
    width: 400px;
    padding: 20px;
    align-items: flex-start;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-right: 30px;
  }
  .close {
    position: absolute;
    top: 13px;
    right: 13px;
    display: flex;
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--bar-text-soft);
    opacity: 0.4;
    cursor: pointer;
    transition:
      opacity 0.15s ease,
      background 0.15s ease;
  }
  .close:hover {
    opacity: 0.85;
    background: var(--bar-hover);
  }
  .close svg {
    width: 14px;
    height: 14px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--bar-chip);
    color: var(--bar-chip-text);
    font-size: 12px;
    font-weight: 550;
    line-height: 20px;
  }
  .note .chip {
    margin-bottom: 10px;
  }
  .chip.error {
    background: var(--bar-chip-error);
    color: var(--bar-chip-error-text);
  }
  .chip svg {
    width: 13px;
    height: 13px;
  }
  .count {
    font-size: 12px;
    font-weight: 600;
    color: var(--bar-text-muted);
    font-variant-numeric: tabular-nums;
  }
  .title {
    align-self: stretch;
    padding-right: 40px;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
  }
  .body {
    margin-top: 8px;
    font-size: 15px;
    line-height: 20px;
    color: var(--bar-text-muted);
  }
  /* The sentence: the reading font and size (Settings → Appearance), with
     roomy lines; long ones scroll inside the card, the word being said kept
     in view. */
  .text {
    margin: 12px 0 0;
    max-height: calc(17px * var(--yap-reading-scale) * 1.65 * var(--yap-reading-leading) * 5);
    overflow-y: auto;
    overscroll-behavior: contain;
    font-family: var(--yap-font-reading);
    font-size: calc(17px * var(--yap-reading-scale));
    line-height: calc(1.65 * var(--yap-reading-leading));
    letter-spacing: 0.005em;
    word-spacing: 0.04em;
    color: var(--bar-text);
    overflow-wrap: anywhere;
    user-select: none;
  }
  /* The word being said: an amber mark, drawn with a box-shadow so lighting
     it never moves the words. */
  .w {
    border-radius: 4px;
    transition:
      background-color 0.08s ease,
      box-shadow 0.08s ease,
      color 0.08s ease;
  }
  .w.now {
    background: var(--bar-chip);
    box-shadow: 0 0 0 2px var(--bar-chip);
    color: var(--bar-chip-text);
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 14px;
  }
  .ctl {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: none;
    color: var(--bar-text-soft);
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .ctl:hover {
    background: var(--bar-hover);
  }
  .ctl svg {
    width: 16px;
    height: 16px;
  }
  .ctl.play {
    width: 36px;
    height: 36px;
    background: var(--bar-button);
    color: var(--bar-button-text);
  }
  .ctl.play:hover {
    background: var(--bar-button-hover);
  }
  .ctl.play svg {
    width: 17px;
    height: 17px;
  }
  .speed {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-left: 8px;
    padding: 2px;
    border-radius: 999px;
    border: 1px solid var(--bar-border);
  }
  .step {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: none;
    color: var(--bar-text-soft);
    cursor: pointer;
  }
  .step:hover:not(:disabled) {
    background: var(--bar-hover);
  }
  .step:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .step svg {
    width: 13px;
    height: 13px;
  }
  .rate {
    min-width: 42px;
    text-align: center;
    font-size: 13px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  .grow {
    flex: 1;
  }
  .stop {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 6px 12px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--bar-text-soft);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .stop:hover {
    background: var(--bar-hover);
  }
  .square {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: var(--bar-recording);
  }
  .cut {
    margin-top: 8px;
    font-size: 12px;
    color: var(--bar-text-muted);
  }
  /* How far through the text: a hairline along the bottom. */
  .progress {
    position: absolute;
    left: 18px;
    right: 18px;
    bottom: 0;
    height: 2px;
    border-radius: 2px 2px 0 0;
    background: var(--bar-track);
    overflow: hidden;
  }
  .progress > div {
    height: 100%;
    background: var(--bar-accent);
    transition: width 0.3s ease;
  }
</style>
