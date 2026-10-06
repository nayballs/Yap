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
  // ring. Every answer goes back to Rust (`bar_card_action`).
  import { fadeTimer, countdownClock } from './cardTimers.svelte.js';

  /** @type {{ card: any, paused?: boolean, onaction: (action: string) => void }} */
  let { card, paused = false, onaction } = $props();

  fadeTimer(() => card, () => paused, (a) => onaction(a));
  const clock = countdownClock(() => card);
  const RING = 2 * Math.PI * 7;

  // The chip names the card's kind (Wispr's amber "New" chip).
  const CHIPS = {
    call: 'Call',
    update: 'Update',
    notes: 'Meeting',
    timer: 'Meeting',
    screen: 'Tip',
    error: 'Error',
  };
  const chip = $derived(card.chip ?? CHIPS[card.icon] ?? null);
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
      {:else if card.icon === 'notes'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><circle cx="12" cy="12" r="8" /><circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" /></svg>
      {/if}
      {chip}
    </span>
  {/if}
  <div class="title">{card.title}</div>
  {#if card.body}<div class="body">{card.body}</div>{/if}
  {#if card.countdown}
    <div class="countdown">
      <svg class="ring" viewBox="0 0 18 18" aria-hidden="true">
        <circle cx="9" cy="9" r="7" class="track" />
        <circle cx="9" cy="9" r="7" class="run" stroke-dasharray={RING} stroke-dashoffset={RING * (1 - clock.fraction)} />
      </svg>
      <span>{clock.secs > 0 ? `${card.countdown.label} ${clock.secs}…` : 'Starting notes…'}</span>
      <span class="esc">Esc to cancel</span>
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
  /* Wispr's card, measured: 400 × auto, black, 1 px rgb(48,48,47), radius
     16, padding 20, a 1–2 px shadow (nothing near the transparent window's
     edge), text rgb(252,252,251). */
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
    background: #000;
    border: 1px solid rgb(48, 48, 47);
    box-shadow: 0 1px 2px rgba(26, 26, 26, 0.05);
    color: rgb(252, 252, 251);
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
    color: rgb(238, 235, 227);
    opacity: 0.3;
    cursor: pointer;
    transition:
      opacity 0.15s ease,
      background 0.15s ease;
  }
  .close:hover {
    opacity: 0.8;
    background: rgba(255, 255, 255, 0.08);
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
    background: rgb(255, 169, 70);
    color: rgb(26, 26, 26);
    font-size: 12px;
    font-weight: 550;
    line-height: 20px;
  }
  .chip.error {
    background: rgb(240, 196, 191);
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
    color: rgb(179, 178, 173);
  }
  .countdown {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-size: 14px;
    font-weight: 600;
    color: rgb(255, 169, 70);
  }
  .ring {
    width: 16px;
    height: 16px;
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 2.4;
  }
  .ring .track {
    stroke: rgba(255, 255, 255, 0.14);
  }
  .ring .run {
    stroke: rgb(255, 169, 70);
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s linear;
  }
  .esc {
    font-weight: 500;
    font-size: 12px;
    color: rgb(179, 178, 173);
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
    color: rgb(238, 235, 227);
  }
  .ghost:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .cream {
    padding: 6px 12px;
    background: rgb(255, 255, 235);
    color: rgb(26, 26, 26);
  }
  .cream:hover {
    background: #fff;
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
    color: rgb(179, 178, 173);
    font: inherit;
    font-size: 13px;
    text-decoration: underline;
    text-decoration-color: rgba(179, 178, 173, 0.4);
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .link button:hover {
    color: rgb(252, 252, 251);
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
    background: rgba(255, 255, 255, 0.14);
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
