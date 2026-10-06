<script>
  // The call prompt on the Yap bar (bar.rs `Card` with style "call",
  // meeting_detect.rs `native::bar_card`), after Wispr's "Meeting detected"
  // card (screenshot 25) on the measured card tokens: one black row — the
  // call app's mark, "Teams call" over "● Now", and a cream split button,
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
    {#if icon.kind === 'glyph'}
      <svg viewBox="0 0 24 24"><path d={icon.path} fill={icon.color} /></svg>
    {:else if icon.kind === 'monogram'}
      <span class="letter" style={`color:${icon.color}`}>{icon.letter}</span>
    {:else if card.icon === 'calendar'}
      <!-- A calendar meeting with no call app's mark (calendar.rs). -->
      <svg viewBox="0 0 24 24" fill="none" stroke="#26231c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="16" rx="3" /><path d="M3.5 9.5h17M8 2.8v3.4M16 2.8v3.4" /></svg>
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="#26231c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z" /></svg>
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
  /* Wispr's "Meeting detected" card on the measured card tokens: 400 wide,
     black, a 1 px rgb(48,48,47) border, radius 16, a 1–2 px shadow. One row,
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
    background: #000;
    border: 1px solid rgb(48, 48, 47);
    box-shadow: 0 1px 2px rgba(26, 26, 26, 0.05);
    color: rgb(252, 252, 251);
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
    border: 1px solid rgb(48, 48, 47);
    border-radius: 50%;
    background: #000;
    color: rgba(238, 235, 227, 0.7);
    cursor: pointer;
    transition:
      background 0.15s ease,
      color 0.15s ease;
  }
  .x:hover {
    background: rgb(26, 26, 26);
    color: rgb(238, 235, 227);
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
    background: rgb(252, 252, 251);
  }
  .mark svg {
    width: 20px;
    height: 20px;
  }
  .letter {
    font-size: 18px;
    font-weight: 800;
    line-height: 1;
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
    color: rgb(179, 178, 173);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #3dbb74;
  }
  .dot.recording {
    background: #e5645e;
    animation: pulse 1.4s ease-in-out infinite;
  }
  /* A calendar meeting about to start (calendar.rs): the cards' amber. */
  .dot.soon {
    background: rgb(255, 169, 70);
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
    color: rgb(255, 169, 70);
  }
  /* "Esc" as a small key, so the countdown line fits the row. */
  .esc {
    padding: 0 5px;
    border: 1px solid rgb(48, 48, 47);
    border-radius: 4px;
    font-size: 11px;
    line-height: 15px;
    color: rgb(179, 178, 173);
  }

  /* The cream split button: [Yap] Record notes | ^ (radius 8, 15 / 600). */
  .split {
    position: relative;
    flex: 0 0 auto;
    display: flex;
    align-items: stretch;
    height: 36px;
    border-radius: 8px;
    background: rgb(255, 255, 235);
    color: rgb(26, 26, 26);
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
    color: rgba(26, 26, 26, 0.6);
  }
  .go:hover,
  .more:hover,
  .more.on {
    background: #fff;
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
    stroke: rgba(26, 26, 26, 0.14);
  }
  .ring .run {
    stroke: #c2690a;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s linear;
  }
  .divider {
    width: 1px;
    margin: 8px 0;
    background: rgba(26, 26, 26, 0.16);
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
    background: #000;
    border: 1px solid rgb(48, 48, 47);
    box-shadow: 0 1px 2px rgba(26, 26, 26, 0.05);
    animation: menu-in 0.12s ease-out;
  }
  .menu button {
    padding: 8px 12px;
    border: none;
    border-radius: 8px;
    background: none;
    color: rgb(238, 235, 227);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .menu button:hover {
    background: rgba(255, 255, 255, 0.09);
    color: #fff;
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
    background: rgba(255, 255, 255, 0.14);
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
