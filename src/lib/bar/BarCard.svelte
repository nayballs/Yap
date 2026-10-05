<script>
  // A notice card above the Yap bar (bar.rs `Card`, the default style):
  // Yap's notices while its main window isn't focused — notes it started,
  // an update, something that went wrong. (A call prompt has its own layout,
  // CallCard.) Styled like the app's toasts: dark ink, a light primary
  // button, a quiet secondary, a small link line for a rarer third answer,
  // an always-there ✕. A timed card fades after `timeoutMs` with a hairline
  // running down (paused while the pointer is on it, or while the bar is
  // hidden) and reports `expireAction`; a `countdown` shows "Starting notes
  // in 7…" with a ring. Every answer goes back to Rust (`bar_card_action`).
  import { fadeTimer, countdownClock } from './cardTimers.svelte.js';

  /** @type {{ card: any, paused?: boolean, onaction: (action: string) => void }} */
  let { card, paused = false, onaction } = $props();

  fadeTimer(() => card, () => paused, (a) => onaction(a));
  const clock = countdownClock(() => card);
  const RING = 2 * Math.PI * 9;
</script>

<div class="card" class:counting={!!card.countdown} data-region={`card:${card.id}`} role="status" aria-live="polite">
  <div class="head">
    {#if card.icon}
      <span class="badge {card.icon}" aria-hidden="true">
        {#if card.icon === 'call'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z" /></svg>
        {:else if card.icon === 'update'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11" /><path d="M7 10.5l5 5 5-5" /><path d="M5 20h14" /></svg>
        {:else if card.icon === 'timer'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="13" r="8" /><path d="M12 9v4l2.5 2.5" /><path d="M9.5 2.5h5" /></svg>
        {:else if card.icon === 'screen'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8" /><path d="M12 16v4" /></svg>
        {:else if card.icon === 'error'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8" /><circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" /></svg>
        {/if}
      </span>
    {/if}
    <div class="title">{card.title}</div>
    <button class="close" aria-label="Close" onclick={() => onaction(card.closeAction ?? '')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  </div>
  {#if card.body}<div class="body">{card.body}</div>{/if}
  {#if card.countdown}
    <div class="countdown">
      <svg class="ring" viewBox="0 0 22 22" aria-hidden="true">
        <circle cx="11" cy="11" r="9" class="track" />
        <circle
          cx="11"
          cy="11"
          r="9"
          class="fill"
          stroke-dasharray={RING}
          stroke-dashoffset={RING * (1 - clock.fraction)}
        />
      </svg>
      <span>{clock.secs > 0 ? `${card.countdown.label} ${clock.secs}…` : 'Starting notes…'}</span>
      <span class="esc">Esc to cancel</span>
    </div>
  {/if}
  {#if card.primary || card.secondary}
    <div class="actions">
      {#if card.secondary}
        <button class="secondary" onclick={() => onaction(card.secondary.id)}>{card.secondary.label}</button>
      {/if}
      {#if card.primary}
        <button class="primary" onclick={() => onaction(card.primary.id)}>{card.primary.label}</button>
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
      <div class="run" class:paused style={`animation-duration:${card.timeoutMs}ms`}></div>
    </div>
  {/if}
</div>

<style>
  /* Wispr's bar card (screenshot 21), in Yap's toast palette: warm
     near-black, a big radius, generous padding. Its shadow is soft and
     stays well inside the bar window (one reaching a transparent window's
     edge draws a grey box). */
  .card {
    position: relative;
    width: 360px;
    box-sizing: border-box;
    padding: 14px 16px 15px;
    border-radius: 18px;
    overflow: hidden;
    background: #1c1a16;
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.32);
    color: rgba(255, 255, 255, 0.95);
    font-size: 13px;
    pointer-events: auto;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .badge {
    flex: 0 0 auto;
    display: inline-flex;
    width: 28px;
    height: 28px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: #ecd9b8;
    color: #453413;
  }
  .badge.error {
    background: #f0c4bf;
    color: #5c1a14;
  }
  .badge.notes {
    background: #c6e3c4;
    color: #1e4620;
  }
  .badge svg {
    width: 14px;
    height: 14px;
  }
  .title {
    flex: 1;
    min-width: 0;
    font-size: 14.5px;
    font-weight: 700;
    line-height: 1.3;
  }
  .close {
    flex: 0 0 auto;
    display: flex;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
    transition: background 150ms ease;
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.2);
    color: #fff;
  }
  .close svg {
    width: 11px;
    height: 11px;
  }
  .body {
    margin: 6px 0 0 38px;
    line-height: 1.5;
    color: rgba(255, 255, 255, 0.58);
  }
  .card:not(:has(.badge)) .body {
    margin-left: 0;
  }
  .countdown {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 0 0 38px;
    font-weight: 650;
    color: #f6c77a;
  }
  .ring {
    width: 20px;
    height: 20px;
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 2.4;
  }
  .ring .track {
    stroke: rgba(255, 255, 255, 0.14);
  }
  .ring .fill {
    stroke: #f0b04a;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s linear;
  }
  .esc {
    margin-left: auto;
    font-weight: 500;
    font-size: 11.5px;
    color: rgba(255, 255, 255, 0.4);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 12px;
  }
  .primary,
  .secondary {
    height: 32px;
    padding: 0 14px;
    border: none;
    border-radius: 10px;
    font: inherit;
    font-size: 13px;
    font-weight: 650;
    cursor: pointer;
    transition:
      background 150ms ease,
      color 150ms ease;
  }
  .secondary {
    padding: 0 12px;
    background: none;
    color: rgba(255, 255, 255, 0.72);
    font-weight: 600;
  }
  .secondary:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }
  /* The light action button, Wispr's "Connect calendar". */
  .primary {
    background: #f5f3ee;
    color: #26231c;
  }
  .primary:hover {
    background: #fff;
  }
  .link {
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
    color: rgba(255, 255, 255, 0.5);
    font: inherit;
    font-size: 12px;
    text-decoration: underline;
    text-decoration-color: rgba(255, 255, 255, 0.25);
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .link button:hover {
    color: rgba(255, 255, 255, 0.85);
    text-decoration-color: currentColor;
  }
  .hairline {
    position: absolute;
    left: 18px;
    right: 18px;
    bottom: 0;
    height: 2px;
    overflow: hidden;
    border-radius: 2px 2px 0 0;
  }
  .run {
    height: 100%;
    background: rgba(255, 255, 255, 0.16);
    animation-name: drain;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .run.paused {
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
