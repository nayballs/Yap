<script>
  // A notice card above the Yap bar (bar.rs `Card`, the default style):
  // Yap's notices while its main window isn't focused — notes it started,
  // an update, a guard rail, a call that ended, something that went wrong.
  // (A call prompt has its own layout, CallCard.) Drawn to Wispr's measured
  // card (flowbar-spec.md): 400 wide, black, a 1 px rgb(48,48,47) border,
  // radius 16, padding 20; a chip (the card's kind, with its glyph), the
  // title 15 / 600, the body 15 in rgb(179,178,173), then a right-aligned
  // row of a ghost button and a cream one; the ✕ 24 × 24 at 30 % top-right.
  // A timed card fades after `timeoutMs` with a hairline running down
  // (paused while the pointer is on it, or while the bar is hidden) and
  // reports `expireAction`; a `countdown` shows "Starting notes in 7…" with a
  // ring (and "Esc to cancel" when the card offers Esc), or, as its
  // `headline` (the quiet-meeting countdown), reads at a glance: "Ending
  // notes in 28 s…" in large type above the title, the seconds in a bigger
  // ring. Every answer goes back to Rust (`bar_card_action`).
  //
  // The Voice Assistant's answer (style "answer", assistant.rs) is the same
  // card: the question as a quiet quoted line, the answer's markdown in a
  // body that scrolls past ~10 lines, a `status` note under it ("Not read
  // aloud while a meeting records"), Open in Chat. It goes when its voice
  // ends (Rust), or on ✕ / Esc, which stop the voice.
  import { fadeTimer, countdownClock } from './cardTimers.svelte.js';
  import { fillHotkey } from '../hotkeys.js';
  import { renderMarkdown } from '../markdown.js';

  /** @type {{ card: any, paused?: boolean, onaction: (action: string) => void }} */
  let { card, paused = false, onaction } = $props();

  fadeTimer(() => card, () => paused, (a) => onaction(a));
  const clock = countdownClock(() => card);
  const RING = 2 * Math.PI * 7;
  const BIG_RING = 2 * Math.PI * 16;
  // The countdown's line: "Starting notes in 7…", at zero its `done` text.
  const countdownText = $derived(
    card.countdown
      ? clock.secs > 0
        ? card.countdown.headline
          ? `${card.countdown.label} ${clock.secs} s…`
          : `${card.countdown.label} ${clock.secs}…`
        : (card.countdown.done ?? 'Starting notes…')
      : ''
  );

  // The chip names the card's kind (Wispr's amber "New" chip).
  const CHIPS = {
    call: 'Call',
    update: 'Update',
    notes: 'Meeting',
    timer: 'Meeting',
    screen: 'Tip',
    error: 'Error',
    tip: 'Tip',
    dictionary: 'Dictionary',
    mic: 'Microphone',
    clipboard: 'Paste',
    assistant: 'Voice Assistant',
    settings: 'Settings',
  };
  const answer = $derived(card.style === 'answer');
  const chip = $derived(card.chip ?? CHIPS[card.icon] ?? null);
  // A card naming a hotkey (the push-to-talk hints) says `{key}`.
  const title = $derived(card.hotkey ? fillHotkey(card.title, card.hotkey) : card.title);
  const body = $derived(card.hotkey ? fillHotkey(card.body, card.hotkey) : card.body);
</script>

<div class="card" data-region={`card:${card.id}`} role="status" aria-live="polite">
  <button class="close" aria-label="Close" onclick={() => onaction(card.closeAction ?? '')}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
  </button>
  {#if chip}
    <span class="chip" class:error={card.icon === 'error'}>
      {#if card.icon === 'call'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z" /></svg>
      {:else if card.icon === 'update'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11" /><path d="M7 10.5l5 5 5-5" /><path d="M5 20h14" /></svg>
      {:else if card.icon === 'timer'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="13" r="8" /><path d="M12 9v4l2.5 2.5" /><path d="M9.5 2.5h5" /></svg>
      {:else if card.icon === 'screen'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8" /><path d="M12 16v4" /></svg>
      {:else if card.icon === 'error'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
      {:else if card.icon === 'tip'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 18h6" /><path d="M10 21h4" /><path d="M12 3a6 6 0 0 0-3.6 10.8c.6.5 1 1.2 1.1 2V16h5v-.2c.1-.8.5-1.5 1.1-2A6 6 0 0 0 12 3z" /></svg>
      {:else if card.icon === 'notes'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><circle cx="12" cy="12" r="8" /><circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" /></svg>
      {:else if card.icon === 'dictionary'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 19.5V5a2 2 0 0 1 2-2h14v15H6a2 2 0 0 0-2 2 2 2 0 0 0 2 2h14" /><path d="M9 7.5h7" /></svg>
      {:else if card.icon === 'mic'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5.5 11a6.5 6.5 0 0 0 13 0" /><path d="M12 17.5V21" /></svg>
      {:else if card.icon === 'clipboard'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="4" width="14" height="17" rx="2" /><path d="M9 4V3h6v1" /><path d="M9 10h6M9 14h4" /></svg>
      {:else if card.icon === 'settings'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4" /><circle cx="12" cy="12" r="4" /></svg>
      {:else if card.icon === 'assistant'}
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 2.5c.5 3.9 1.6 6.4 3.1 7.9 1.5 1.5 4 2.6 6.4 3.1-2.4.5-4.9 1.6-6.4 3.1-1.5 1.5-2.6 4-3.1 6.9-.5-2.9-1.6-5.4-3.1-6.9-1.5-1.5-4-2.6-6.9-3.1 2.9-.5 5.4-1.6 6.9-3.1 1.5-1.5 2.6-4 3.1-7.9z" /></svg>
      {/if}
      {chip}
    </span>
  {/if}
  {#if card.countdown?.headline}
    <div class="headline" role="timer" aria-live="off">
      <svg class="bigring" viewBox="0 0 40 40" aria-hidden="true">
        <circle cx="20" cy="20" r="16" class="track" />
        <circle cx="20" cy="20" r="16" class="run" stroke-dasharray={BIG_RING} stroke-dashoffset={BIG_RING * (1 - clock.fraction)} />
        <text x="20" y="20" class="secs" text-anchor="middle" dominant-baseline="central">{clock.secs}</text>
      </svg>
      <span class="headtext">{countdownText}</span>
    </div>
  {/if}
  {#if answer}
    <!-- The question, then the answer (renderMarkdown passes no raw HTML and sanitises its output). -->
    <div class="asked">“{title}”</div>
    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    {#if body}<div class="body answer">{@html renderMarkdown(body)}</div>{/if}
    {#if card.status}<div class="note">{card.status}</div>{/if}
  {:else}
    <div class="title" class:under={card.countdown?.headline}>{title}</div>
    {#if body}<div class="body">{body}</div>{/if}
  {/if}
  {#if card.countdown && !card.countdown.headline}
    <div class="countdown">
      <svg class="ring" viewBox="0 0 18 18" aria-hidden="true">
        <circle cx="9" cy="9" r="7" class="track" />
        <circle cx="9" cy="9" r="7" class="run" stroke-dasharray={RING} stroke-dashoffset={RING * (1 - clock.fraction)} />
      </svg>
      <span>{countdownText}</span>
      {#if card.escapeAction}<span class="esc">Esc to cancel</span>{/if}
    </div>
  {/if}
  {#if card.primary || card.secondary}
    <div class="actions">
      {#if card.secondary}
        <button class="ghost" onclick={() => onaction(card.secondary.id)}>{card.secondary.label}</button>
      {/if}
      {#if card.primary}
        <button class="cream" onclick={() => onaction(card.primary.id)}>{card.primary.label}</button>
      {/if}
    </div>
  {/if}
  {#if card.link}
    <div class="link">
      <button onclick={() => onaction(card.link.id)}>{card.link.label}</button>
    </div>
  {/if}
  {#if card.timeoutMs && !card.countdown}
    <div class="hairline">
      <div class="run-down" class:paused style={`animation-duration:${card.timeoutMs}ms`}></div>
    </div>
  {/if}
</div>

<style>
  /* Wispr's card, measured: 400 × auto, a 1 px edge, radius 16, padding 20,
     a 1–2 px shadow (nothing near the transparent window's edge). Colours:
     Yap's bar tokens (--bar-*, app.css). */
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 400px;
    box-sizing: border-box;
    padding: 20px;
    border-radius: 16px;
    overflow: hidden;
    background: var(--bar-surface);
    border: 1px solid var(--bar-border);
    box-shadow: var(--bar-shadow);
    color: var(--bar-text);
    pointer-events: auto;
  }
  /* The ✕: 24 × 24, 13 px from the top and right, at 30 %. */
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
    opacity: 0.3;
    cursor: pointer;
    transition:
      opacity 0.15s ease,
      background 0.15s ease;
  }
  .close:hover {
    opacity: 0.8;
    background: var(--bar-hover);
  }
  .close svg {
    width: 14px;
    height: 14px;
  }
  /* The chip: amber, radius 6, 12 / 550 / 20, padding 2 × 8, ink text. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 10px;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--bar-chip);
    color: var(--bar-chip-text);
    font-size: 12px;
    font-weight: 550;
    line-height: 20px;
  }
  .chip.error {
    background: var(--bar-chip-error);
    color: var(--bar-chip-error-text);
  }
  .chip svg {
    width: 12px;
    height: 12px;
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
    font-weight: 400;
    line-height: 20px;
    color: var(--bar-text-muted);
  }
  .countdown {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-size: 14px;
    font-weight: 600;
    color: var(--bar-accent);
  }
  .ring {
    width: 16px;
    height: 16px;
    transform: rotate(-90deg);
  }
  .ring circle,
  .bigring circle {
    fill: none;
    stroke-width: 2.4;
  }
  .ring .track,
  .bigring .track {
    stroke: var(--bar-track);
  }
  .ring .run,
  .bigring .run {
    stroke: var(--bar-accent);
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s linear;
  }
  /* The countdown as the headline (the quiet meeting): readable at a
     glance — the seconds big in a 40 px ring, the line in 20 / 650. */
  .headline {
    align-self: stretch;
    display: flex;
    align-items: center;
    gap: 12px;
    padding-right: 32px;
    margin-bottom: 10px;
  }
  .bigring {
    flex: none;
    width: 40px;
    height: 40px;
  }
  .bigring .track,
  .bigring .run {
    stroke-width: 3;
    transform: rotate(-90deg);
    transform-origin: 20px 20px;
  }
  .bigring .secs {
    fill: var(--bar-text);
    font-size: 15px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .headtext {
    font-size: 20px;
    font-weight: 650;
    line-height: 26px;
    font-variant-numeric: tabular-nums;
    color: var(--bar-text);
  }
  .title.under {
    padding-right: 0;
  }
  /* The Voice Assistant's answer: the question quiet and quoted, the answer
     in the title's colour, scrolling past ~10 lines (the card is a region,
     so the wheel works on it), a note under it. */
  .asked {
    align-self: stretch;
    padding-right: 40px;
    font-size: 13px;
    font-style: italic;
    line-height: 18px;
    color: var(--bar-text-muted);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .body.answer {
    align-self: stretch;
    max-height: 210px;
    overflow-y: auto;
    color: var(--bar-text);
    font-size: 14px;
    overscroll-behavior: contain;
  }
  .body.answer :global(p) {
    margin: 0 0 6px;
  }
  .body.answer :global(p:last-child) {
    margin-bottom: 0;
  }
  .body.answer :global(ul),
  .body.answer :global(ol) {
    margin: 0 0 6px;
    padding-left: 20px;
  }
  .body.answer :global(h1),
  .body.answer :global(h2),
  .body.answer :global(h3) {
    margin: 0 0 4px;
    font-size: 14px;
    font-weight: 650;
  }
  .body.answer :global(pre),
  .body.answer :global(code) {
    font-size: 12px;
  }
  .body.answer :global(pre) {
    margin: 0 0 6px;
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--bar-hover);
    white-space: pre-wrap;
  }
  .note {
    margin-top: 8px;
    font-size: 12px;
    color: var(--bar-text-muted);
  }
  .esc {
    font-weight: 500;
    font-size: 12px;
    color: var(--bar-text-muted);
  }
  /* A right-aligned row 12 px under the body: a ghost button and a cream
     one, both radius 8, 15 / 600 / 20. */
  .actions {
    align-self: stretch;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
  .ghost,
  .cream {
    border: none;
    border-radius: 8px;
    font: inherit;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    cursor: pointer;
    transition: background-color 0.4s cubic-bezier(0.2, 0.9, 0.25, 1.1);
  }
  .ghost {
    padding: 6px 10px;
    background: none;
    color: var(--bar-text-soft);
  }
  .ghost:hover {
    background: var(--bar-hover);
  }
  .cream {
    padding: 6px 12px;
    background: var(--bar-button);
    color: var(--bar-button-text);
  }
  .cream:hover {
    background: var(--bar-button-hover);
  }
  .link {
    align-self: stretch;
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }
  .link button {
    padding: 2px 4px;
    margin-right: -4px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--bar-text-muted);
    font: inherit;
    font-size: 13px;
    text-decoration: underline;
    text-decoration-color: var(--bar-text-faint);
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .link button:hover {
    color: var(--bar-text);
    text-decoration-color: currentColor;
  }
  .hairline {
    position: absolute;
    left: 20px;
    right: 20px;
    bottom: 0;
    height: 2px;
    overflow: hidden;
    border-radius: 2px 2px 0 0;
  }
  .run-down {
    height: 100%;
    background: var(--bar-track);
    animation-name: drain;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .run-down.paused {
    animation-play-state: paused;
  }
  @keyframes drain {
    from {
      width: 100%;
    }
    to {
      width: 0%;
    }
  }
</style>
