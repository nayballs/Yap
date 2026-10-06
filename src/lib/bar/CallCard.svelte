<script>
  // The call prompt on the Yap bar (bar.rs `Card` with style "call",
  // meeting_detect.rs `native::bar_card`), after Wispr's "Meeting detected"
  // card (screenshot 25) on the measured card tokens: one black row — the
  // call app's icon (CallAppIcon: its own as installed here, else its
  // mark), "Teams call" over "● Now", and a cream split button,
  // [Yap] Record notes, whose ^ menu holds the other answers (Not now,
  // Don't ask for Teams). The ✕ on its top-left corner is the quiet answer,
  // as is fading after 30 s (a hairline runs along the bottom; paused while
  // the pointer is on it). With "Start notes automatically" a ring drains
  // around the logo and the line under the title counts down; Esc cancels
  // (Rust watches the key). (The call ending asks in a notice card.) The
  // calendar's reminder (calendar.rs `bar_card`) is one too: the meeting
  // over "● In 1 min · with Tanay +1" (amber, then green once it's on),
  // Join & take notes | ^ Start notes, Snooze 2 min; a calendar glyph when
  // the meeting has no call app.
  //
  // `preview`: a static picture of the card for the main window (the
  // "Connect your calendar" dialog's band), drawn from sample data by this
  // same component so it always matches the bar: inert (nothing to click
  // or focus), hidden from screen readers, no regions reported to the bar.
  import yapLogo from '../../assets/yap-logo.svg';
  import { callAppIcon } from './callApps.js';
  import CallAppIcon from '../CallAppIcon.svelte';
  import { fadeTimer, countdownClock } from './cardTimers.svelte.js';

  /** @type {{ card: any, paused?: boolean, hovered?: boolean, preview?: boolean, onaction?: (action: string) => void }} */
  let { card, paused = false, hovered = false, preview = false, onaction = () => {} } = $props();

  fadeTimer(() => card, () => paused, (a) => onaction(a));
  const clock = countdownClock(() => card);
  const icon = $derived(callAppIcon(card.app));
  // The bar's hit regions (Overlay.svelte); a preview has none.
  const region = $derived(preview ? undefined : `card:${card.id}`);
  const RING = 2 * Math.PI * 12;

  let menuOpen = $state(false);
  // The menu closes once the pointer has left the card for a moment.
  let menuTimer = null;
  $effect(() => {
    clearTimeout(menuTimer);
    if (menuOpen && !hovered) menuTimer = setTimeout(() => (menuOpen = false), 700);
    return () => clearTimeout(menuTimer);
  });

  function answer(id) {
    menuOpen = false;
    onaction(id ?? '');
  }

  // A long answer ("Join & take notes") goes on two lines, as Wispr's
  // pre-meeting card's "Join meeting / & start Notetaker", so the row keeps
  // room for the meeting's name at 400 px.
  const label = $derived.by(() => {
    const text = card.primary?.label ?? '';
    const at = text.indexOf(' & ');
    return at > 0 ? { main: text.slice(0, at), sub: text.slice(at + 1) } : { main: text, sub: '' };
  });
</script>

<div
  class="call"
  class:counting={!!card.countdown}
  class:preview
  data-region={region}
  role={preview ? undefined : 'status'}
  aria-live={preview ? undefined : 'polite'}
  aria-hidden={preview ? 'true' : undefined}
  inert={preview}
>
  {#if !preview}
    <!-- Not on the picture of the card: Wispr's illustration has no ✕ either,
         and the dialog showing it has its own. -->
    <button class="x" data-region={region} aria-label="Close" onclick={() => answer(card.closeAction)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
    </button>
  {/if}

  <span class="mark" aria-hidden="true">
    {#if icon.kind === 'phone' && card.icon === 'calendar'}
      <!-- A calendar meeting with no call app's mark (calendar.rs). -->
      <svg viewBox="0 0 24 24" fill="none" stroke="#26231c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="16" rx="3" /><path d="M3.5 9.5h17M8 2.8v3.4M16 2.8v3.4" /></svg>
    {:else}
      <!-- The app's own icon as installed here, else its mark (CallAppIcon). -->
      <CallAppIcon app={card.app} />
    {/if}
  </span>

  <div class="text">
    <div class="title">{card.title}</div>
    <div class="status">
      {#if card.countdown}
        <span class="count">{clock.secs > 0 ? `${card.countdown.label} ${clock.secs}…` : 'Starting notes…'}</span>
        <span class="esc" title="Esc to cancel">Esc</span>
      {:else if card.status}
        {#if card.dot}<span class="dot {card.dot}" aria-hidden="true"></span>{/if}
        <span class="line">{card.status}</span>
      {/if}
    </div>
  </div>

  {#if card.primary}
    <div class="split">
      <button class="go" onclick={() => answer(card.primary.id)}>
        <span class="logo">
          <img src={yapLogo} alt="" />
          {#if card.countdown}
            <svg class="ring" viewBox="0 0 28 28" aria-hidden="true">
              <circle cx="14" cy="14" r="12" class="track" />
              <circle cx="14" cy="14" r="12" class="run" stroke-dasharray={RING} stroke-dashoffset={RING * (1 - clock.fraction)} />
            </svg>
          {/if}
        </span>
        <span class="label" class:two={!!label.sub}>
          <span class="main">{label.main}</span>
          {#if label.sub}<span class="sub">{label.sub}</span>{/if}
        </span>
      </button>
      {#if card.secondary || card.link}
        <span class="divider" aria-hidden="true"></span>
        <button
          class="more"
          class:on={menuOpen}
          aria-label="More answers"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          onclick={() => (menuOpen = !menuOpen)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 14.5l5-5 5 5" /></svg>
        </button>
      {/if}
      {#if menuOpen}
        <div class="menu" role="menu" data-region={region}>
          {#if card.secondary}
            <button role="menuitem" onclick={() => answer(card.secondary.id)}>{card.secondary.label}</button>
          {/if}
          {#if card.link}
            <button role="menuitem" onclick={() => answer(card.link.id)}>{card.link.label}</button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if card.timeoutMs && !card.countdown}
    <div class="hairline">
      <div class="drain" class:paused style={`animation-duration:${card.timeoutMs}ms`}></div>
    </div>
  {/if}
</div>

<style>
  /* Wispr's "Meeting detected" card on the measured card layout: 400 wide,
     a 1 px edge, radius 16, a 1–2 px shadow, in Yap's bar colours (--bar-*,
     app.css). One row,
     so it keeps the screenshot's slimmer padding rather than the notice
     cards' 20 px. */
  .call {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 400px;
    box-sizing: border-box;
    padding: 12px 12px 12px 14px;
    border-radius: 16px;
    background: var(--bar-surface);
    border: 1px solid var(--bar-border);
    box-shadow: var(--bar-shadow);
    color: var(--bar-text);
    pointer-events: auto;
  }
  /* A picture of the card (`preview`): nothing on it reacts to the pointer. */
  .call.preview {
    pointer-events: none;
  }
  /* The ✕ sits on the top-left corner (screenshot 25). */
  .x {
    position: absolute;
    top: -9px;
    left: -9px;
    display: flex;
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 1px solid var(--bar-border);
    border-radius: 50%;
    background: var(--bar-surface);
    color: var(--bar-text-muted);
    cursor: pointer;
    transition:
      background 0.15s ease,
      color 0.15s ease;
  }
  .x:hover {
    background: var(--bar-surface-hover);
    color: var(--bar-text-soft);
  }
  .x svg {
    width: 11px;
    height: 11px;
  }
  /* The call app's mark on a light tile, in its own colour. */
  .mark {
    flex: 0 0 auto;
    display: flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    border-radius: 10px;
    background: var(--bar-mark-tile);
  }
  .mark svg {
    width: 20px;
    height: 20px;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    line-height: 18px;
    color: var(--bar-text-muted);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--bar-now);
  }
  .dot.recording {
    background: var(--bar-recording);
    animation: pulse 1.4s ease-in-out infinite;
  }
  /* A calendar meeting about to start (calendar.rs): the cards' amber. */
  .dot.soon {
    background: var(--bar-accent);
  }
  /* A long line ("Started 12 min ago · with Tanay +3") ends in "…". */
  .line {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }
  .count {
    font-weight: 600;
    color: var(--bar-accent);
  }
  /* "Esc" as a small key, so the countdown line fits the row. */
  .esc {
    padding: 0 5px;
    border: 1px solid var(--bar-border);
    border-radius: 4px;
    font-size: 11px;
    line-height: 15px;
    color: var(--bar-text-muted);
  }

  /* The cream split button: [Yap] Record notes | ^ (radius 8, 15 / 600). */
  .split {
    position: relative;
    flex: 0 0 auto;
    display: flex;
    align-items: stretch;
    height: 36px;
    border-radius: 8px;
    background: var(--bar-button);
    color: var(--bar-button-text);
  }
  .go,
  .more {
    display: inline-flex;
    align-items: center;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
    transition: background-color 0.4s cubic-bezier(0.2, 0.9, 0.25, 1.1);
  }
  .go {
    gap: 8px;
    padding: 0 12px 0 7px;
    border-radius: 8px 0 0 8px;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
  }
  .split:not(:has(.more)) .go {
    border-radius: 8px;
  }
  .label {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    white-space: nowrap;
  }
  /* Two lines: the answer, then what comes with it, smaller and muted. */
  .label.two .main {
    font-size: 14px;
    line-height: 16px;
  }
  .label .sub {
    font-size: 12px;
    font-weight: 500;
    line-height: 14px;
    color: var(--bar-button-text-sub);
  }
  .go:hover,
  .more:hover,
  .more.on {
    background: var(--bar-button-hover);
  }
  .logo {
    position: relative;
    display: flex;
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
  }
  .logo img {
    width: 22px;
    height: 22px;
    border-radius: 6px;
  }
  .counting .logo img {
    width: 16px;
    height: 16px;
    border-radius: 4px;
  }
  .ring {
    position: absolute;
    inset: -1px;
    width: 26px;
    height: 26px;
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 2.6;
  }
  .ring .track {
    stroke: var(--bar-button-track);
  }
  .ring .run {
    stroke: var(--yap-primary);
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s linear;
  }
  .divider {
    width: 1px;
    margin: 8px 0;
    background: var(--bar-button-divider);
  }
  .more {
    justify-content: center;
    width: 32px;
    border-radius: 0 8px 8px 0;
  }
  .more svg {
    width: 15px;
    height: 15px;
    transition: transform 180ms ease;
  }
  .more.on svg {
    transform: rotate(180deg);
  }
  /* The other answers, above the button, on the card tokens. */
  .menu {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 6px;
    border-radius: 12px;
    background: var(--bar-surface);
    border: 1px solid var(--bar-border);
    box-shadow: var(--bar-shadow);
    animation: menu-in 0.12s ease-out;
  }
  .menu button {
    padding: 8px 12px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--bar-text-soft);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--bar-hover);
    color: var(--bar-text);
  }
  @keyframes menu-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  .hairline {
    position: absolute;
    left: 16px;
    right: 16px;
    bottom: 0;
    height: 2px;
    overflow: hidden;
    border-radius: 2px 2px 0 0;
  }
  .drain {
    height: 100%;
    background: var(--bar-track);
    animation-name: drain;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .drain.paused {
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
