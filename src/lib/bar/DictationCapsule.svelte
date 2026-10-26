<script>
  // The dictation overlay, in the Yap bar's place while a dictation records,
  // transcribes or just failed: the red dot + scrolling amplitude waveform,
  // the live partial text when there is some, "Transcribing…", or the error.
  // Dark ink like the rest of the bar; the moving waveform and the pulsing
  // dot carry its visibility on any background. A lock after the dot: push-
  // to-talk's double-tap made it hands-free. "No sound from <mic>…" in the
  // waveform's place: the dead-mic watchdog heard nothing but digital
  // silence (the recording carries on). "Starting the mic…" in its place
  // while a cold mic opens (dictation/mic.rs: the start chime waits for its first
  // real audio too), and "Loading the speech model…" instead of
  // "Transcribing…" while the model still loads after Yap started.
  //
  // While it records: ✕ (cancel) on the left and ✓ (stop and paste, as the
  // hotkey's second press) on the right, as Settings → Yap bar says
  // (`buttons`, bar.rs' `dictationButtons`): "off" (the default), "hover" —
  // only while the pointer is on the capsule (`hovered`: Rust's pointer,
  // the authority on hover in a click-through window) — or "always". Never
  // while the push-to-talk key is held (Wispr's bar draws them only
  // hands-free). Where
  // they may show, their places are kept (the capsule's ends) and they only
  // fade in and out there, so the capsule's width and the words never move.
  // Cancelled: "Dictation cancelled" with Undo while Rust keeps the audio
  // (dictation/pipeline.rs, `UNDO_MS`; a hairline runs down meanwhile) — Undo
  // transcribes it and pastes it into the app the dictation started in. The
  // capsule is a `data-region` whenever it has a button to offer, so the
  // click-through bar takes clicks on it (bar.rs); it's never focused, so
  // the app you were in keeps the keyboard.
  // Cleaned: "Cleaned up" with **Use what I said** for a few seconds after a
  // dictation whose words AI cleanup changed (pasting/as_said.rs) — it swaps the
  // paste for your words before cleanup, in the app you dictated into.
  // A saved transform (dictation/transforms.rs): its name while it runs ("Polish…",
  // the amber dot), then "Polish ✓" with **Undo** (the text from before put
  // back) for as long, or "Polish: no changes needed".
  // "Loading the AI model…" (with Skip cleanup) while the cleanup waits for
  // Yap's on-device model to load on its first use (ai/local_llm/on_demand.rs).
  // "Taking longer than usual…" instead of "Transcribing…" once the
  // transcription or the AI cleanup runs well past this PC's usual time
  // (dictation/slow.rs) — with **Skip cleanup** when it's the cleanup: the
  // words as transcribed, pasted now. "Hold on, still transcribing…" a
  // moment after the shortcut is pressed during it (the press is ignored).
  // Near a dictation's length limit (dictation/cap.rs): "1 minute left ·
  // it'll stop at 15 minutes" for a few seconds in the waveform's place,
  // then an amber hairline running down the time left.
  //
  // A question to the **Voice Assistant** (chat/assistant.rs; `dictation.assistant`
  // isn't 'idle') has its own variant, never mistaken for a dictation: an
  // amber edge and a ✦ where the red dot was; "Ask Yap…" beside the
  // waveform (or your words) while it listens, "Thinking…" while it's
  // transcribed and answered, then "Speaking…" with three bars while the
  // answer is read aloud. ■ stops the answer (`assistant-stop`) while it
  // thinks or speaks; ✕ / ✓ work as for a dictation while it listens.
  import { untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { dictation } from './dictation.svelte.js';

  /** buttons: 'off' | 'hover' | 'always'; hovered: the pointer is on it. */
  let { buttons = 'off', hovered = false } = $props();

  // Smooth left glide: keep the newest words visible by translating the text
  // run left whenever it outgrows the clip box (transition on the transform).
  // A line a word or so shorter (its last words heard again) keeps its place
  // rather than gliding back right and left again a moment later; a bigger
  // change, or a new line, follows it.
  const STAY_PX = 48;
  let clipEl = $state(null);
  let textEl = $state(null);
  let shift = $state(0);
  $effect(() => {
    const n = dictation.words.length;
    if (clipEl && textEl) {
      const target = Math.min(0, clipEl.clientWidth - textEl.scrollWidth);
      const current = untrack(() => shift);
      shift = n === 0 || target <= current || target - current > STAY_PX ? target : current;
    }
  });

  const processing = $derived(dictation.state === 'processing' || dictation.state === 'processing-slow');
  const recording = $derived(dictation.state === 'recording');
  const cancelled = $derived(dictation.state === 'cancelled');
  const cleaned = $derived(dictation.state === 'cleaned');
  // A saved transform's (dictation/transforms.rs): running, then done with Undo, or
  // nothing to change.
  const transforming = $derived(processing && !!dictation.transform && dictation.transformPhase === 'running');
  const transformed = $derived(cleaned && !!dictation.transform && dictation.transformPhase === 'done');
  const unchanged = $derived(cleaned && !!dictation.transform && dictation.transformPhase === 'unchanged');
  // The AI cleanup is taking longer than usual: Skip cleanup on offer.
  const skipOffer = $derived(processing && !!dictation.slow?.skip && !dictation.transform);
  // What the capsule says while it works.
  const workText = $derived(
    dictation.holdOn
      ? 'Hold on, still transcribing…'
      : dictation.modelLoading
        ? 'Loading the speech model…'
        : dictation.slow?.stage === 'loading'
          ? 'Loading the AI model…'
          : dictation.slow
            ? 'Taking longer than usual…'
            : 'Transcribing…'
  );
  // The push-to-talk key is down (not locked hands-free): no buttons.
  const keyHeld = $derived(dictation.held && !dictation.locked);
  // ✕ / ✓ have their places (and the capsule takes the pointer)…
  const offer = $derived(recording && buttons !== 'off' && !keyHeld);
  // …and are on screen.
  const showButtons = $derived(offer && (buttons === 'always' || hovered));

  // The Voice Assistant's variant.
  const ask = $derived(dictation.assistant !== 'idle' && !cancelled && !cleaned && dictation.state !== 'error');
  const askListening = $derived(ask && recording);
  const askSpeaking = $derived(ask && !recording && dictation.assistant === 'speaking');
  const askThinking = $derived(ask && !recording && !askSpeaking);

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
  class:offer
  class:show={showButtons}
  class:cancelled={cancelled || (cleaned && !unchanged) || skipOffer}
  class:ask
  class:askstop={askThinking || askSpeaking}
  data-region={offer || cancelled || (cleaned && !unchanged) || skipOffer
    ? 'dictation'
    : askThinking || askSpeaking
      ? 'assistant'
      : undefined}
  data-ask={ask ? dictation.assistant : undefined}
>
  {#if ask}
    {#if offer}
      <button class="cbtn cancel" aria-label="Cancel the question" title="Cancel" onclick={() => act('dictation-cancel')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
      </button>
    {/if}
    <span class="spark" class:listen={askListening} class:think={askThinking} aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2.5c.5 3.9 1.6 6.4 3.1 7.9 1.5 1.5 4 2.6 6.4 3.1-2.4.5-4.9 1.6-6.4 3.1-1.5 1.5-2.6 4-3.1 6.9-.5-2.9-1.6-5.4-3.1-6.9-1.5-1.5-4-2.6-6.9-3.1 2.9-.5 5.4-1.6 6.9-3.1 1.5-1.5 2.6-4 3.1-7.9z" /></svg>
    </span>
    {#if askListening}
      {#if dictation.locked}
        <span class="lock" role="img" aria-label="Hands-free: press the shortcut to stop" title="Hands-free: press the shortcut to stop">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7.5a4 4 0 0 1 8 0V11" /></svg>
        </span>
      {/if}
      {#if dictation.micSilent}
        <span class="txt warn" role="alert" title={dictation.micSilentFull}>{dictation.micSilent}</span>
      {:else if dictation.micWaiting}
        <span class="txt soft" role="status">Starting the mic…</span>
      {:else if dictation.words.length}
        <div class="partial" bind:this={clipEl}>
          <span class="ptext" bind:this={textEl} style="transform: translateX({shift}px)">
            {#each dictation.words as w, i (i)}<span class="pw">{w}&nbsp;</span>{/each}
          </span>
        </div>
      {:else}
        <span class="txt asklabel">{dictation.askAbout === 'screenshot' ? 'Ask about this screenshot…' : 'Ask Yap…'}</span>
        <div class="wave short" class:narrow={dictation.askAbout === 'screenshot'}>
          {#each dictation.history.slice(-48) as v}
            <span style="height:{Math.max(7, Math.round(v * 100))}%"></span>
          {/each}
        </div>
      {/if}
    {:else if askThinking}
      <span class="txt" role="status">Thinking…</span>
    {:else}
      <span class="speakbars" aria-hidden="true"><i></i><i></i><i></i></span>
      <span class="txt" role="status">Speaking…</span>
    {/if}
    {#if offer}
      <button class="cbtn done" aria-label="Stop and ask" title="Stop and ask" onclick={() => act('dictation-stop')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5.5 12.5l4 4 9-9" /></svg>
      </button>
    {/if}
    {#if askThinking || askSpeaking}
      <button class="sbtn" aria-label={askSpeaking ? 'Stop speaking' : 'Stop the answer'} title="Stop" onclick={() => act('assistant-stop')}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
      </button>
    {/if}
  {:else if cancelled}
    <span class="txt">Dictation cancelled</span>
    <button class="undo" onclick={() => act('dictation-undo')}>Undo</button>
    <span class="undoline" aria-hidden="true"><span style={`animation-duration:${dictation.undoMs}ms`}></span></span>
  {:else if transformed}
    <span class="txt">{dictation.transform}</span>
    <svg class="tick" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round" aria-label="done"><path d="M5.5 12.5l4 4 9-9" /></svg>
    <button class="undo" title="Put back the text from before" onclick={() => act('transform-undo')}>Undo</button>
    <span class="undoline" aria-hidden="true"><span style={`animation-duration:${dictation.offerMs}ms`}></span></span>
  {:else if unchanged}
    <span class="txt" role="status">{dictation.transform}: no changes needed</span>
  {:else if cleaned}
    <span class="txt">Cleaned up</span>
    <button class="undo" title="Swap it for your words before AI cleanup" onclick={() => act('as-said')}>Use what I said</button>
    <span class="undoline" aria-hidden="true"><span style={`animation-duration:${dictation.offerMs}ms`}></span></span>
  {:else if dictation.state === 'error'}
    <span class="dot errdot"></span>
    <span class="txt">{dictation.errorMsg}</span>
  {:else}
    {#if offer}
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
    {:else if dictation.micWaiting && !processing}
      <span class="txt soft" role="status">Starting the mic…</span>
    {:else if recording && dictation.capShow}
      <span class="txt soft" role="status">{dictation.capText}</span>
    {:else if dictation.words.length && !dictation.slow && !dictation.holdOn}
      <div class="partial" bind:this={clipEl}>
        <span class="ptext" bind:this={textEl} style="transform: translateX({shift}px)">
          {#each dictation.words as w, i (i)}<span class="pw">{w}&nbsp;</span>{/each}
        </span>
      </div>
    {:else if transforming}
      <span class="txt" role="status">{dictation.transform}…</span>
    {:else if processing}
      <span class="txt" class:soft={!!dictation.slow && !dictation.holdOn} role="status">{workText}</span>
      {#if skipOffer}
        <button class="undo" title="Paste the words as transcribed, without AI cleanup" onclick={() => act('skip-cleanup')}
          >Skip cleanup</button
        >
      {/if}
    {:else}
      <div class="wave">
        {#each dictation.history as v}
          <span style="height:{Math.max(7, Math.round(v * 100))}%"></span>
        {/each}
      </div>
    {/if}
    {#if recording && dictation.capMs > 0 && !dictation.micSilent}
      <!-- Near the length limit (dictation/cap.rs): the time left running out. -->
      {#key dictation.capKey}
        <span class="capline" aria-hidden="true">
          <span data-keep-motion style={`animation-duration:${dictation.capMs}ms`}></span>
        </span>
      {/key}
    {/if}
    {#if offer}
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
  /* Recording with ✕ / ✓ on offer: their places are kept in the capsule's
     ends (5 px in, 26 px wide, 8 px to the dot and the words) whether they
     show or not, so pointing at it moves nothing: they only fade in there. */
  .capsule.offer {
    padding: 0 39px;
    gap: 8px;
  }
  .capsule.cancelled {
    padding: 0 5px 0 16px;
    gap: 12px;
  }
  .capsule.err {
    border-color: rgba(229, 100, 94, 0.7);
  }

  /* The Voice Assistant's variant: an amber edge (a 1.5 px ring, inside
     the capsule so nothing reaches the window's edge) and a ✦ for the dot. */
  .capsule.ask {
    border-color: var(--bar-wave);
    box-shadow: inset 0 0 0 0.5px var(--bar-wave);
  }
  .capsule.ask.err {
    border-color: rgba(229, 100, 94, 0.7);
    box-shadow: none;
  }
  .capsule.askstop {
    padding-right: 6px;
  }
  .spark {
    display: flex;
    flex: 0 0 auto;
    color: var(--bar-wave);
  }
  .spark svg {
    width: 14px;
    height: 14px;
  }
  .spark.listen {
    animation: pulse 1.2s ease-in-out infinite;
  }
  .spark.think {
    animation: twinkle 1.6s linear infinite;
  }
  @keyframes twinkle {
    from {
      transform: rotate(0deg) scale(1);
    }
    50% {
      transform: rotate(180deg) scale(0.8);
    }
    to {
      transform: rotate(360deg) scale(1);
    }
  }
  .asklabel {
    color: var(--bar-text-soft);
  }
  .wave.short {
    width: 120px;
  }
  /* "Ask about this screenshot…" needs the room. */
  .wave.short.narrow {
    width: 64px;
  }
  /* Speaking: three bars in the bar's amber, then ■. */
  .speakbars {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 14px;
  }
  .speakbars i {
    display: block;
    width: 2.5px;
    height: 100%;
    border-radius: 1px;
    background: var(--bar-wave);
    animation: speak 0.9s ease-in-out infinite;
  }
  .speakbars i:nth-child(2) {
    animation-delay: -0.3s;
  }
  .speakbars i:nth-child(3) {
    animation-delay: -0.6s;
  }
  @keyframes speak {
    0%,
    100% {
      transform: scaleY(0.35);
    }
    50% {
      transform: scaleY(1);
    }
  }
  .sbtn {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    margin-left: 2px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: var(--bar-button);
    color: var(--bar-button-text);
    cursor: pointer;
  }
  .sbtn:hover {
    background: var(--bar-button-hover);
  }
  .sbtn svg {
    width: 16px;
    height: 16px;
  }
  @media (prefers-reduced-motion: reduce) {
    .spark.listen,
    .spark.think,
    .speakbars i {
      animation: none;
    }
  }

  /* ✕ and ✓: 26 px circles in the kept places; ✕ quiet, ✓ the bar's
     primary (ink on light, cream on dark). Hidden (not only transparent, so
     never clicked unseen) until shown; they fade and settle in, and fade out
     a little slower. */
  .cbtn {
    position: absolute;
    top: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    margin-top: -13px;
    padding: 0;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    opacity: 0;
    visibility: hidden;
    transform: scale(0.82);
    transition:
      opacity 0.2s ease,
      transform 0.2s ease,
      visibility 0s linear 0.2s,
      background 0.15s ease;
  }
  .capsule.show .cbtn {
    opacity: 1;
    visibility: visible;
    transform: none;
    transition:
      opacity 0.14s ease,
      transform 0.18s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      visibility 0s,
      background 0.15s ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .cbtn,
    .capsule.show .cbtn {
      transform: none;
    }
  }
  .cbtn svg {
    width: 14px;
    height: 14px;
  }
  .cbtn.cancel {
    left: 5px;
    background: var(--bar-hover);
    color: var(--bar-text-soft);
  }
  .cbtn.cancel:hover {
    background: var(--bar-active);
    color: var(--bar-text);
  }
  .cbtn.done {
    right: 5px;
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
  /* A transform's ✓, in the bar's amber. */
  .tick {
    flex: 0 0 auto;
    width: 13px;
    height: 13px;
    margin-left: -6px;
    color: var(--bar-wave);
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
  /* Near the length limit: the time left as an amber hairline running down
     (the time itself, so it moves with Motion reduced too). */
  .capline {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: 3px;
    height: 2px;
    border-radius: 1px;
    background: var(--bar-track);
    overflow: hidden;
  }
  .capline span {
    display: block;
    height: 100%;
    background: var(--bar-wave);
    transform-origin: left;
    animation: run-down linear forwards;
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
  /* "Starting the mic…": quieter than a warning, in the waveform's place
     until the mic delivers. */
  .txt.soft {
    color: var(--bar-text-soft);
    font-weight: 550;
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
